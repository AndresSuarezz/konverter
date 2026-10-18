//! Markdown → DOCX en Rust puro (`pulldown-cmark` para leer, `zip` para empaquetar).
//!
//! Existe por dos motivos: es una conversión útil por sí misma (Markdown → Word) y es **la forma
//! de imprimir Markdown con WordCraft**. Su importador de HTML ignora los anchos de columna (todas
//! las columnas salen iguales), mientras que al leer DOCX sí los respeta: por eso el Markdown se
//! convierte antes a DOCX, con los anchos calculados por contenido, y no a HTML.
//!
//! Es **seguro** como el resto del Markdown de Converter: el HTML en bruto se reduce a su texto
//! (nunca se interpreta), las imágenes no se descargan (salen como `[imagen: texto alternativo]`)
//! y solo se conservan los enlaces `http`, `https` y `mailto`.
//!
//! Cubre títulos, párrafos, negrita, cursiva, tachado, código en línea y en bloque, listas
//! (anidadas, con viñetas o numeradas desde el número que indique el documento, con casillas),
//! citas, reglas, tablas (con alineación, cabecera sombreada y repetida en cada página) y enlaces.
//! No cubre notas al pie, fórmulas ni imágenes.

use std::io::{Cursor, Write};

use pulldown_cmark::{Alignment, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::convert::{ConvertOptions, Converter, Fidelity, PdfPaper};
use crate::error::{Error, Result};
use crate::format::Format;
use crate::tabular::decode_text;

/// Conversión Markdown → DOCX.
pub struct MarkdownDocxConverter;

impl Converter for MarkdownDocxConverter {
    fn supports(&self, from: Format, to: Format) -> Option<Fidelity> {
        (from == Format::Markdown && to == Format::Docx).then_some(Fidelity::Lossless)
    }

    fn convert(
        &self,
        input: &[u8],
        from: Format,
        to: Format,
        options: &ConvertOptions,
    ) -> Result<Vec<u8>> {
        if self.supports(from, to).is_none() {
            return Err(Error::Unsupported { from, to });
        }
        to_docx(&decode_text(input), options.pdf_paper)
    }
}

/// Niveles de lista que se distinguen (los más hondos se sangran como el último).
const MAX_LEVEL: usize = 8;
/// Word no abre una tabla con 64 o más columnas.
const MAX_COLUMNS: usize = 63;
/// Márgenes de la página, en veinteavos de punto (2 cm).
const MARGIN: u32 = 1134;
/// Cuántos caracteres de una celda cuentan para repartir el ancho: una celda enorme no debe dejar
/// a las demás sin sitio.
const WIDTH_CAP: usize = 120;
/// Cuántos caracteres de la palabra más larga de una columna cuentan para su ancho mínimo.
const WORD_CAP: usize = 14;

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
                  xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"";

/// Markdown → bytes de un archivo DOCX.
pub(crate) fn to_docx(markdown: &str, paper: PdfPaper) -> Result<Vec<u8>> {
    let (page_w, page_h) = match paper {
        PdfPaper::A4 => (11906_u32, 16838_u32),
        PdfPaper::Letter => (12240, 15840),
    };
    let mut builder = Builder::new(page_w - 2 * MARGIN);
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for event in Parser::new_ext(markdown, options) {
        builder.event(event);
    }
    builder.flush_para();
    package(&builder, page_w, page_h)
}

// ---------------------------------------------------------------- construcción

struct Cell {
    runs: String,
    /// Caracteres de texto de la celda (reparte el ancho entre columnas).
    chars: usize,
    /// Su palabra más larga: una columna nunca debe quedar más estrecha que ella.
    longest_word: usize,
}

struct TableBuilder {
    aligns: Vec<Alignment>,
    rows: Vec<(bool, Vec<Cell>)>,
    row: Vec<Cell>,
}

struct Builder {
    text_width: u32,
    body: String,
    /// URL de cada enlace conservado; su relación se llama `rIdLk<índice + 1>`.
    links: Vec<String>,
    /// Por cada enlace abierto: (índice de la URL, inicio de sus `runs`), o `None` si se descartó.
    link_stack: Vec<Option<(usize, usize)>>,
    /// (número inicial, nivel) de cada lista numerada; su `numId` es `2 + índice`.
    ordered: Vec<(u64, usize)>,
    /// Los `<w:r>` del párrafo (o de la celda) que se está construyendo.
    runs: String,
    para_open: bool,
    heading: Option<usize>,
    bold: u32,
    italic: u32,
    strike: u32,
    link_depth: u32,
    /// `numId` de cada lista abierta.
    lists: Vec<usize>,
    /// El próximo párrafo abre un elemento de lista (lleva viñeta o número).
    item_pending: bool,
    quote: usize,
    table: Option<TableBuilder>,
    cell_chars: usize,
    cell_longest: usize,
    code: Option<String>,
    /// Dentro de un `<script>` o `<style>` en bruto: se ignora hasta su cierre.
    skip_until: Option<&'static str>,
}

impl Builder {
    fn new(text_width: u32) -> Self {
        Self {
            text_width,
            body: String::new(),
            links: Vec::new(),
            link_stack: Vec::new(),
            ordered: Vec::new(),
            runs: String::new(),
            para_open: false,
            heading: None,
            bold: 0,
            italic: 0,
            strike: 0,
            link_depth: 0,
            lists: Vec::new(),
            item_pending: false,
            quote: 0,
            table: None,
            cell_chars: 0,
            cell_longest: 0,
            code: None,
            skip_until: None,
        }
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => self.run(&code, true),
            Event::Html(raw) | Event::InlineHtml(raw) => self.html(&raw),
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.line_break(),
            Event::Rule => {
                self.flush_para();
                self.body.push_str(
                    "<w:p><w:pPr><w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"1\" \
                     w:color=\"AAAAAA\"/></w:pBdr><w:spacing w:before=\"120\" w:after=\"120\"/>\
                     </w:pPr></w:p>",
                );
            }
            Event::TaskListMarker(done) => self.text(if done { "[x] " } else { "[ ] " }),
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush_para();
                self.heading = Some(heading_number(level));
            }
            Tag::BlockQuote(_) => {
                self.flush_para();
                self.quote += 1;
            }
            Tag::CodeBlock(_) => {
                self.flush_para();
                self.code = Some(String::new());
            }
            Tag::List(start) => {
                self.flush_para();
                let level = self.lists.len().min(MAX_LEVEL);
                let num_id = match start {
                    Some(first) => {
                        self.ordered.push((first, level));
                        1 + self.ordered.len()
                    }
                    None => 1,
                };
                self.lists.push(num_id);
            }
            Tag::Item => {
                self.flush_para();
                self.item_pending = true;
            }
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => {
                if is_safe_link(&dest_url) {
                    self.links.push(dest_url.to_string());
                    self.link_stack
                        .push(Some((self.links.len() - 1, self.runs.len())));
                    self.link_depth += 1;
                } else {
                    self.link_stack.push(None);
                }
            }
            Tag::Image { .. } => self.text("[imagen: "),
            Tag::Table(aligns) => {
                self.flush_para();
                self.table = Some(TableBuilder {
                    aligns,
                    rows: Vec::new(),
                    row: Vec::new(),
                });
            }
            Tag::TableHead => self.bold += 1,
            Tag::TableCell => {
                self.runs.clear();
                self.cell_chars = 0;
                self.cell_longest = 0;
                self.para_open = false;
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush_para(),
            TagEnd::Heading(_) => {
                self.flush_para();
                self.heading = None;
            }
            TagEnd::BlockQuote(_) => {
                self.flush_para();
                self.quote = self.quote.saturating_sub(1);
            }
            TagEnd::CodeBlock => {
                if let Some(code) = self.code.take() {
                    self.code_block(&code);
                }
            }
            TagEnd::List(_) => {
                self.flush_para();
                self.lists.pop();
            }
            TagEnd::Item => {
                self.flush_para();
                self.item_pending = false;
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => {
                if let Some(Some((index, start))) = self.link_stack.pop() {
                    self.link_depth = self.link_depth.saturating_sub(1);
                    let start = start.min(self.runs.len());
                    if self.runs.is_char_boundary(start) {
                        let inner = self.runs.split_off(start);
                        self.runs.push_str(&format!(
                            "<w:hyperlink r:id=\"rIdLk{}\" w:history=\"1\">{inner}</w:hyperlink>",
                            index + 1
                        ));
                    }
                }
            }
            TagEnd::Image => self.text("]"),
            TagEnd::TableCell => {
                let cell = Cell {
                    runs: std::mem::take(&mut self.runs),
                    chars: self.cell_chars,
                    longest_word: self.cell_longest,
                };
                self.para_open = false;
                if let Some(table) = &mut self.table {
                    table.row.push(cell);
                }
            }
            TagEnd::TableHead => {
                self.bold = self.bold.saturating_sub(1);
                if let Some(table) = &mut self.table {
                    let row = std::mem::take(&mut table.row);
                    table.rows.push((true, row));
                }
            }
            TagEnd::TableRow => {
                if let Some(table) = &mut self.table {
                    let row = std::mem::take(&mut table.row);
                    table.rows.push((false, row));
                }
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.render_table(&table);
                }
            }
            TagEnd::HtmlBlock => self.flush_para(),
            _ => {}
        }
    }

    // ----------------------------------------------------------------- texto

    fn text(&mut self, text: &str) {
        if let Some(code) = &mut self.code {
            code.push_str(text);
        } else if !text.is_empty() {
            self.run(text, false);
        }
    }

    /// Añade `text` al párrafo (o a la celda) con el formato vigente.
    fn run(&mut self, text: &str, code: bool) {
        // En un bloque de código el texto va aparte; dentro de un `<script>` o `<style>` en línea
        // (su contenido llega como texto, entre dos etiquetas) no se escribe nada.
        if self.code.is_some() || self.skip_until.is_some() {
            return;
        }
        self.para_open = true;
        if self.table.is_some() {
            self.cell_chars += text.chars().count();
            for word in text.split_whitespace() {
                self.cell_longest = self.cell_longest.max(word.chars().count());
            }
        }
        let props = self.run_props(code);
        let clean = clean_text(text);
        for (index, piece) in clean.split('\t').enumerate() {
            if index > 0 {
                self.runs.push_str(&format!("<w:r>{props}<w:tab/></w:r>"));
            }
            if !piece.is_empty() {
                self.runs.push_str(&format!(
                    "<w:r>{props}<w:t xml:space=\"preserve\">{}</w:t></w:r>",
                    escape(piece)
                ));
            }
        }
    }

    fn run_props(&self, code: bool) -> String {
        let mut props = String::new();
        if code {
            props.push_str(
                "<w:rFonts w:ascii=\"Courier New\" w:hAnsi=\"Courier New\" w:cs=\"Courier New\"/>",
            );
        }
        if self.bold > 0 {
            props.push_str("<w:b/>");
        }
        if self.italic > 0 {
            props.push_str("<w:i/>");
        }
        if self.strike > 0 {
            props.push_str("<w:strike/>");
        }
        if self.link_depth > 0 {
            props.push_str("<w:color w:val=\"0B57D0\"/>");
        }
        if code {
            props.push_str("<w:sz w:val=\"19\"/>");
        }
        if self.link_depth > 0 {
            props.push_str("<w:u w:val=\"single\"/>");
        }
        if code {
            props.push_str("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"EFEFEF\"/>");
        }
        if props.is_empty() {
            props
        } else {
            format!("<w:rPr>{props}</w:rPr>")
        }
    }

    fn line_break(&mut self) {
        if self.code.is_none() && self.skip_until.is_none() {
            self.para_open = true;
            self.runs.push_str("<w:r><w:br/></w:r>");
        }
    }

    /// HTML en bruto: nunca se interpreta; se queda su texto, `<br>` como salto y `<img alt>` como
    /// texto alternativo. El contenido de `<script>` y `<style>` y los comentarios se descartan.
    fn html(&mut self, raw: &str) {
        let (text, until) = strip_html(raw, self.skip_until);
        self.skip_until = until;
        // Las etiquetas sueltas (`<p align="center">`) no dejan texto: no abren un párrafo vacío.
        if text.trim().is_empty() {
            return;
        }
        let mut first = true;
        for line in text.split('\n') {
            if !first {
                self.line_break();
            }
            first = false;
            if !line.is_empty() {
                self.run(line, false);
            }
        }
    }

    // -------------------------------------------------------------- bloques

    /// Cierra el párrafo en curso (si lo hay) y lo añade al cuerpo.
    fn flush_para(&mut self) {
        if self.table.is_some() || !self.para_open {
            return;
        }
        let runs = std::mem::take(&mut self.runs);
        self.para_open = false;
        let props = self.para_props();
        self.body
            .push_str(&format!("<w:p><w:pPr>{props}</w:pPr>{runs}</w:p>"));
    }

    /// Propiedades del párrafo, en el orden que exige el esquema (estilo, numeración, bordes,
    /// espaciado, sangría).
    fn para_props(&mut self) -> String {
        let mut props = String::new();
        if let Some(level) = self.heading {
            props.push_str(&format!("<w:pStyle w:val=\"Heading{level}\"/>"));
            return props;
        }
        if self.quote > 0 {
            props.push_str("<w:pStyle w:val=\"Quote\"/>");
        }
        let quote_indent = self.quote as u32 * 567;
        if let Some(&num_id) = self.lists.last() {
            let level = (self.lists.len() - 1).min(MAX_LEVEL);
            if self.item_pending {
                props.push_str(&format!(
                    "<w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"{num_id}\"/></w:numPr>"
                ));
                self.item_pending = false;
            }
            if self.quote > 0 {
                props.push_str(&quote_border());
            }
            props.push_str("<w:spacing w:before=\"0\" w:after=\"60\"/>");
            if self.quote > 0 {
                props.push_str(&format!("<w:ind w:left=\"{}\"/>", quote_indent + 567));
            } else if !props.contains("<w:numPr>") {
                // Segundo párrafo de un elemento: se alinea con el texto de su viñeta.
                props.push_str(&format!("<w:ind w:left=\"{}\"/>", (level as u32 + 1) * 567));
            }
        } else if self.quote > 0 {
            props.push_str(&quote_border());
            props.push_str(&format!("<w:ind w:left=\"{quote_indent}\"/>"));
        }
        props
    }

    fn code_block(&mut self, code: &str) {
        let indent = (self.lists.len() + self.quote) as u32 * 567;
        let mut lines: Vec<&str> = code.split('\n').collect();
        if lines.last() == Some(&"") {
            lines.pop();
        }
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.iter().enumerate() {
            let line = clean_text(line.trim_end_matches('\r')).replace('\t', "    ");
            let spacing = if index == last {
                "<w:spacing w:after=\"160\"/>"
            } else {
                ""
            };
            let ind = if indent > 0 {
                format!("<w:ind w:left=\"{indent}\"/>")
            } else {
                String::new()
            };
            self.body.push_str(&format!(
                "<w:p><w:pPr><w:pStyle w:val=\"CodeBlock\"/>{spacing}{ind}</w:pPr><w:r>\
                 <w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                escape(&line)
            ));
        }
    }

    /// Reparte el ancho de la página entre las columnas según lo que llevan dentro (como haría un
    /// navegador), con un mínimo para que ninguna se aplaste.
    fn render_table(&mut self, table: &TableBuilder) {
        let columns = table
            .rows
            .iter()
            .map(|(_, row)| row.len())
            .max()
            .unwrap_or(0)
            .min(MAX_COLUMNS);
        if columns == 0 {
            return;
        }
        let mut score = vec![6.0_f64; columns];
        let mut longest = vec![0_usize; columns];
        for (_, row) in &table.rows {
            for (index, cell) in row.iter().take(columns).enumerate() {
                score[index] = score[index].max(cell.chars.min(WIDTH_CAP) as f64);
                longest[index] = longest[index].max(cell.longest_word);
            }
        }
        let widths: Vec<u32> = column_widths(&score, &longest, f64::from(self.text_width))
            .iter()
            .map(|w| w.round() as u32)
            .collect();
        let table_width: u32 = widths.iter().sum();

        let border = |side: &str| {
            format!("<w:{side} w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"999999\"/>")
        };
        let mut xml = format!(
            "<w:tbl><w:tblPr><w:tblW w:w=\"{table_width}\" w:type=\"dxa\"/><w:tblBorders>{}{}{}{}{}{}\
             </w:tblBorders><w:tblLayout w:type=\"fixed\"/><w:tblCellMar><w:top w:w=\"50\" w:type=\"dxa\"/>\
             <w:left w:w=\"100\" w:type=\"dxa\"/><w:bottom w:w=\"50\" w:type=\"dxa\"/>\
             <w:right w:w=\"100\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr><w:tblGrid>",
            border("top"),
            border("left"),
            border("bottom"),
            border("right"),
            border("insideH"),
            border("insideV"),
        );
        for w in &widths {
            xml.push_str(&format!("<w:gridCol w:w=\"{w}\"/>"));
        }
        xml.push_str("</w:tblGrid>");
        for (header, row) in &table.rows {
            xml.push_str("<w:tr><w:trPr><w:cantSplit/>");
            if *header {
                xml.push_str("<w:tblHeader/>");
            }
            xml.push_str("</w:trPr>");
            for index in 0..columns {
                let cell_width = widths.get(index).copied().unwrap_or(0);
                let shade = if *header {
                    "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"EEEEEE\"/>"
                } else {
                    ""
                };
                let align = match table.aligns.get(index) {
                    Some(Alignment::Center) => "<w:jc w:val=\"center\"/>",
                    Some(Alignment::Right) => "<w:jc w:val=\"right\"/>",
                    _ => "",
                };
                let runs = row.get(index).map_or("", |cell| cell.runs.as_str());
                xml.push_str(&format!(
                    "<w:tc><w:tcPr><w:tcW w:w=\"{cell_width}\" w:type=\"dxa\"/>{shade}</w:tcPr>\
                     <w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\"/>{align}</w:pPr>{runs}</w:p></w:tc>"
                ));
            }
            xml.push_str("</w:tr>");
        }
        xml.push_str("</w:tbl>");
        self.body.push_str(&xml);
        // Word exige un párrafo tras una tabla; además separa la tabla del texto que sigue.
        self.body
            .push_str("<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"120\"/></w:pPr></w:p>");
    }
}

/// Reparte `width` entre las columnas en proporción a su contenido (`score`), pero ninguna baja de
/// lo que ocupa su palabra más larga (`longest`, en caracteres): así no se parten palabras. Lo que
/// sobra al respetar los mínimos se quita a las columnas que están por encima del suyo.
fn column_widths(score: &[f64], longest: &[usize], width: f64) -> Vec<f64> {
    // ~135 veinteavos de punto por carácter (Arial de 10,5 pt en negrita, el peor caso: las
    // cabeceras y la primera columna suelen ir en negrita) y los márgenes interiores de la celda.
    // Una palabra larguísima sin espacios (una ruta, una URL) no puede obligar a ensanchar su
    // columna a costa de las cortas: se parte donde haga falta. Por eso cuenta como mucho
    // `WORD_CAP` caracteres, suficiente para fechas, identificadores y nombres.
    let minimums: Vec<f64> = longest
        .iter()
        .map(|&n| n.min(WORD_CAP) as f64 * 135.0 + 240.0)
        .collect();
    let total: f64 = score.iter().sum();
    let mut widths: Vec<f64> = score
        .iter()
        .zip(&minimums)
        .map(|(s, min)| (s / total * width).max(*min).max(700.0))
        .collect();
    let mut excess = widths.iter().sum::<f64>() - width;
    if excess > 0.0 {
        let slack: f64 = widths
            .iter()
            .zip(&minimums)
            .map(|(w, min)| (w - min.max(700.0)).max(0.0))
            .sum();
        if slack > 0.0 {
            let take = excess.min(slack);
            for (w, min) in widths.iter_mut().zip(&minimums) {
                let room = (*w - min.max(700.0)).max(0.0);
                *w -= take * room / slack;
            }
            excess -= take;
        }
        if excess > 0.0 {
            // Ni así caben las palabras más largas: se encoge todo por igual.
            let factor = width / widths.iter().sum::<f64>();
            for w in &mut widths {
                *w *= factor;
            }
        }
    }
    widths
}

fn quote_border() -> String {
    "<w:pBdr><w:left w:val=\"single\" w:sz=\"18\" w:space=\"8\" w:color=\"BBBBBB\"/></w:pBdr>"
        .to_string()
}

fn heading_number(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn is_safe_link(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
}

// ----------------------------------------------------------------- HTML en bruto

/// Reduce un trozo de HTML a su texto. Devuelve el texto (con `\n` donde había un `<br>`) y, si el
/// trozo termina dentro de un `<script>` o `<style>`, cuál hay que esperar que se cierre.
fn strip_html(raw: &str, skipping: Option<&'static str>) -> (String, Option<&'static str>) {
    let mut out = String::new();
    let mut rest = raw;
    let mut skip = skipping;
    while !rest.is_empty() {
        if let Some(name) = skip {
            let close = format!("</{name}");
            match rest.to_ascii_lowercase().find(&close) {
                Some(at) => {
                    let after = &rest[at..];
                    rest = after.find('>').map_or("", |end| &after[end + 1..]);
                    skip = None;
                }
                None => return (out, Some(name)),
            }
            continue;
        }
        let Some(open) = rest.find('<') else {
            out.push_str(&decode_entities(rest));
            break;
        };
        out.push_str(&decode_entities(&rest[..open]));
        let tag_and_rest = &rest[open..];
        if let Some(comment) = tag_and_rest.strip_prefix("<!--") {
            rest = comment.find("-->").map_or("", |end| &comment[end + 3..]);
            continue;
        }
        let Some(end) = tag_and_rest.find('>') else {
            // «<» suelto sin cerrar: es texto.
            out.push_str(&decode_entities(tag_and_rest));
            break;
        };
        let tag = &tag_and_rest[1..end];
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_ascii_lowercase();
        match name.as_str() {
            "br" => out.push('\n'),
            "img" => {
                if let Some(alt) = attribute(tag, "alt") {
                    out.push_str(&format!("[imagen: {}]", decode_entities(&alt)));
                }
            }
            "script" if !tag.starts_with('/') => skip = Some("script"),
            "style" if !tag.starts_with('/') => skip = Some("style"),
            _ => {}
        }
        rest = &tag_and_rest[end + 1..];
    }
    (out, skip)
}

/// El valor de un atributo entrecomillado (`alt="..."`) dentro de una etiqueta.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let at = from + found;
        let before_ok = at == 0
            || lower[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        let after = tag[at + name.len()..].trim_start();
        if before_ok && let Some(value) = after.strip_prefix('=') {
            let value = value.trim_start();
            let quote = value.chars().next()?;
            if quote == '"' || quote == '\'' {
                let inner = &value[1..];
                return inner.find(quote).map(|end| inner[..end].to_string());
            }
        }
        from = at + name.len();
    }
    None
}

fn decode_entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

// ------------------------------------------------------------------------ XML

/// Quita los caracteres de control que XML 1.0 no admite (un solo `\u{0}` invalidaría el DOCX).
fn clean_text(text: &str) -> String {
    text.chars()
        .filter(|c| matches!(c, '\t' | '\n' | '\r') || !c.is_control())
        .collect()
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

// -------------------------------------------------------------------- paquete

fn encode_error(message: impl ToString) -> Error {
    Error::Encode {
        format: Format::Docx,
        message: message.to_string(),
    }
}

fn package(builder: &Builder, page_w: u32, page_h: u32) -> Result<Vec<u8>> {
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document {NS}><w:body>{}\
         <w:sectPr><w:pgSz w:w=\"{page_w}\" w:h=\"{page_h}\"/><w:pgMar w:top=\"{MARGIN}\" \
         w:right=\"{MARGIN}\" w:bottom=\"{MARGIN}\" w:left=\"{MARGIN}\" w:header=\"567\" \
         w:footer=\"567\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>",
        builder.body
    );

    let mut doc_rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships \
         xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rIdStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>\
         <Relationship Id=\"rIdNum\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/>",
    );
    for (index, url) in builder.links.iter().enumerate() {
        doc_rels.push_str(&format!(
            "<Relationship Id=\"rIdLk{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" \
             Target=\"{}\" TargetMode=\"External\"/>",
            index + 1,
            escape(&clean_text(url))
        ));
    }
    doc_rels.push_str("</Relationships>");

    let parts: [(&str, String); 6] = [
        ("[Content_Types].xml", CONTENT_TYPES.to_string()),
        ("_rels/.rels", ROOT_RELS.to_string()),
        ("word/document.xml", document),
        ("word/_rels/document.xml.rels", doc_rels),
        ("word/styles.xml", styles_xml()),
        ("word/numbering.xml", numbering_xml(&builder.ordered)),
    ];
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, data) in &parts {
        zip.start_file(*name, options).map_err(encode_error)?;
        zip.write_all(data.as_bytes()).map_err(encode_error)?;
    }
    let cursor = zip.finish().map_err(encode_error)?;
    Ok(cursor.into_inner())
}

const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>\
<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/>\
</Types>";

const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
</Relationships>";

fn styles_xml() -> String {
    // Tamaños en medios puntos. Arial (Liberation Sans en Linux) como en el resto del Markdown.
    let sizes = [40, 32, 28, 24, 22, 22];
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:styles {NS}>\
         <w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Arial\" w:hAnsi=\"Arial\" w:cs=\"Arial\"/>\
         <w:sz w:val=\"21\"/><w:lang w:val=\"es-ES\"/></w:rPr></w:rPrDefault>\
         <w:pPrDefault><w:pPr><w:spacing w:after=\"120\" w:line=\"276\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>\
         <w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>"
    );
    for (index, size) in sizes.iter().enumerate() {
        let level = index + 1;
        let border = if level <= 2 {
            "<w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"4\" w:color=\"D0D7DE\"/></w:pBdr>"
        } else {
            ""
        };
        xml.push_str(&format!(
            "<w:style w:type=\"paragraph\" w:styleId=\"Heading{level}\"><w:name w:val=\"heading {level}\"/>\
             <w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/>\
             <w:keepLines/>{border}<w:spacing w:before=\"280\" w:after=\"120\"/>\
             <w:outlineLvl w:val=\"{}\"/></w:pPr><w:rPr><w:b/><w:color w:val=\"24292F\"/>\
             <w:sz w:val=\"{size}\"/></w:rPr></w:style>",
            index
        ));
    }
    xml.push_str(
        "<w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"Normal\"/>\
         <w:qFormat/><w:rPr><w:color w:val=\"57606A\"/></w:rPr></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"CodeBlock\"><w:name w:val=\"Code Block\"/><w:basedOn w:val=\"Normal\"/>\
         <w:pPr><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F3F3F3\"/><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr>\
         <w:rPr><w:rFonts w:ascii=\"Courier New\" w:hAnsi=\"Courier New\" w:cs=\"Courier New\"/><w:sz w:val=\"19\"/></w:rPr></w:style>\
         </w:styles>",
    );
    xml
}

fn numbering_xml(ordered: &[(u64, usize)]) -> String {
    let mut xml =
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:numbering {NS}>");
    // abstractNum 0: viñetas; 1: números (1., a., i., y vuelta a empezar).
    let bullets = ["\u{2022}", "\u{25E6}", "\u{25AA}"];
    let formats = ["decimal", "lowerLetter", "lowerRoman"];
    for (abstract_id, is_bullet) in [(0, true), (1, false)] {
        xml.push_str(&format!(
            "<w:abstractNum w:abstractNumId=\"{abstract_id}\"><w:multiLevelType w:val=\"hybridMultilevel\"/>"
        ));
        for level in 0..=MAX_LEVEL {
            let (format, text) = if is_bullet {
                ("bullet".to_string(), bullets[level % 3].to_string())
            } else {
                (formats[level % 3].to_string(), format!("%{}.", level + 1))
            };
            let font = if is_bullet {
                "<w:rPr><w:rFonts w:ascii=\"Arial\" w:hAnsi=\"Arial\" w:cs=\"Arial\"/></w:rPr>"
            } else {
                ""
            };
            xml.push_str(&format!(
                "<w:lvl w:ilvl=\"{level}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{format}\"/>\
                 <w:lvlText w:val=\"{text}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" \
                 w:hanging=\"340\"/></w:pPr>{font}</w:lvl>",
                567 * (level + 1)
            ));
        }
        xml.push_str("</w:abstractNum>");
    }
    // numId 1: todas las viñetas. numId 2…: una lista numerada cada uno, con su número inicial.
    xml.push_str("<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>");
    for (index, (start, level)) in ordered.iter().enumerate() {
        xml.push_str(&format!(
            "<w:num w:numId=\"{}\"><w:abstractNumId w:val=\"1\"/><w:lvlOverride w:ilvl=\"{level}\">\
             <w:startOverride w:val=\"{}\"/></w:lvlOverride></w:num>",
            index + 2,
            (*start).min(1_000_000)
        ));
    }
    xml.push_str("</w:numbering>");
    xml
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    /// Los textos de cada parte del paquete, tras comprobar que son XML bien formado.
    fn parts(docx: &[u8]) -> Vec<(String, String)> {
        let mut archive = zip::ZipArchive::new(Cursor::new(docx)).expect("es un ZIP");
        let mut out = Vec::new();
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).expect("entrada");
            let name = file.name().map(|n| n.to_string()).unwrap_or_default();
            let mut text = String::new();
            file.read_to_string(&mut text).expect("UTF-8");
            roxmltree::Document::parse(&text)
                .unwrap_or_else(|e| panic!("{name} no es XML válido: {e}"));
            out.push((name, text));
        }
        out
    }

    fn document(markdown: &str) -> String {
        let docx = to_docx(markdown, PdfPaper::A4).expect("convierte");
        parts(&docx)
            .into_iter()
            .find(|(name, _)| name == "word/document.xml")
            .map(|(_, text)| text)
            .expect("document.xml")
    }

    #[test]
    fn every_part_is_well_formed_xml_and_the_package_is_complete() {
        let docx = to_docx("# Título\n\nHola **mundo**.\n", PdfPaper::A4).unwrap();
        let names: Vec<String> = parts(&docx).into_iter().map(|(n, _)| n).collect();
        for expected in [
            "[Content_Types].xml",
            "_rels/.rels",
            "word/document.xml",
            "word/styles.xml",
            "word/numbering.xml",
            "word/_rels/document.xml.rels",
        ] {
            assert!(names.contains(&expected.to_string()), "falta {expected}");
        }
    }

    #[test]
    fn headings_emphasis_code_and_accents_survive() {
        let xml =
            document("# Informe ñandú\n\nHola **negrita**, *cursiva*, ~~tachado~~ y `código`.\n");
        assert!(xml.contains("w:val=\"Heading1\""));
        assert!(xml.contains("Informe ñandú"));
        assert!(xml.contains("<w:b/>") && xml.contains("<w:i/>") && xml.contains("<w:strike/>"));
        assert!(xml.contains("Courier New") && xml.contains(">código<"));
    }

    #[test]
    fn a_table_gets_content_based_widths_a_shaded_repeated_header_and_alignment() {
        let xml = document(
            "| Sistema | Estado | Qué está probado |\n|:--|:-:|--:|\n\
             | Linux | Compila | 148 tests pasan, clippy limpio, se generan paquetes con WordCraft dentro y las conversiones funcionan |\n",
        );
        assert!(xml.contains("<w:tblHeader/>") && xml.contains("w:fill=\"EEEEEE\""));
        assert!(
            xml.contains("<w:jc w:val=\"center\"/>") && xml.contains("<w:jc w:val=\"right\"/>")
        );
        // La columna larga recibe mucho más ancho que las cortas (con HTML salían iguales).
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let widths: Vec<u32> = doc
            .descendants()
            .filter(|n| n.tag_name().name() == "gridCol")
            .filter_map(|n| n.attributes().find(|a| a.name() == "w"))
            .filter_map(|a| a.value().parse().ok())
            .collect();
        assert_eq!(widths.len(), 3);
        assert!(widths[2] > widths[0] * 3, "{widths:?}");
        let total: u32 = widths.iter().sum();
        assert!(total <= 11906 - 2 * MARGIN, "no cabe en la página: {total}");
    }

    #[test]
    fn no_column_is_narrower_than_its_longest_word() {
        // Una columna de una palabra larga junto a otra de texto muy largo: antes se partía «Familia».
        let widths = column_widths(&[7.0, 120.0], &[7, 8], 9638.0);
        assert!(widths[0] >= 7.0 * 135.0, "{widths:?}");
        assert!(
            (widths.iter().sum::<f64>() - 9638.0).abs() < 1.0,
            "{widths:?}"
        );
        // Si las palabras no caben ni juntas, se encoge todo pero nada se sale de la página.
        let tight = column_widths(&[50.0; 8], &[14; 8], 9638.0);
        assert!(tight.iter().sum::<f64>() <= 9638.5, "{tight:?}");
        // Una ruta larguísima en una columna no aplasta a las cortas (el caso de BITACORA.md).
        let widths = column_widths(&[4.0, 10.0, 90.0, 40.0, 60.0], &[4, 10, 35, 12, 20], 9638.0);
        assert!(widths[0] >= 4.0 * 135.0 + 240.0 - 1.0, "{widths:?}");
        assert!(widths[1] >= 10.0 * 135.0 + 240.0 - 1.0, "{widths:?}");
    }

    #[test]
    fn lists_nest_and_ordered_lists_keep_their_start_number() {
        let xml = document("- uno\n  - anidado\n- dos\n\n5. cinco\n6. seis\n");
        assert!(xml.contains("<w:ilvl w:val=\"0\"/>") && xml.contains("<w:ilvl w:val=\"1\"/>"));
        let docx = to_docx("5. cinco\n6. seis\n", PdfPaper::A4).unwrap();
        let numbering = parts(&docx)
            .into_iter()
            .find(|(n, _)| n == "word/numbering.xml")
            .unwrap()
            .1;
        assert!(
            numbering.contains("<w:startOverride w:val=\"5\"/>"),
            "{numbering}"
        );
    }

    #[test]
    fn links_become_real_hyperlinks_only_for_safe_schemes() {
        let docx = to_docx(
            "[web](https://ejemplo.com) y [mal](javascript:alert(1)) y [correo](mailto:a@b.c)",
            PdfPaper::A4,
        )
        .unwrap();
        let all = parts(&docx);
        let rels = &all
            .iter()
            .find(|(n, _)| n.ends_with("document.xml.rels"))
            .unwrap()
            .1;
        assert!(rels.contains("https://ejemplo.com") && rels.contains("mailto:a@b.c"));
        assert!(!rels.contains("javascript"), "{rels}");
        let xml = &all
            .iter()
            .find(|(n, _)| n == "word/document.xml")
            .unwrap()
            .1;
        assert_eq!(xml.matches("<w:hyperlink ").count(), 2);
        assert!(
            xml.contains(">mal<"),
            "el texto del enlace descartado se conserva"
        );
    }

    #[test]
    fn raw_html_is_reduced_to_text_and_never_interpreted() {
        let xml = document(
            "<p align=\"center\"><img alt=\"Insignia\" src=\"http://x/y.png\"></p>\n\n\
             Antes <b>negrita</b><br>después <script>alert(1)</script>fin\n\n\
             <style>body{color:red}</style>\n\n<!-- oculto -->visible\n",
        );
        assert!(xml.contains("[imagen: Insignia]"), "{xml}");
        assert!(xml.contains("negrita") && xml.contains("después") && xml.contains("fin"));
        assert!(!xml.contains("alert") && !xml.contains("color:red") && !xml.contains("oculto"));
        assert!(!xml.contains("<script") && !xml.contains("<p align"));
        assert!(xml.contains("visible"));
    }

    #[test]
    fn images_are_never_fetched_and_show_their_alt_text() {
        let xml = document("![logo](http://127.0.0.1:9/x.png)");
        assert!(xml.contains("[imagen: ") && xml.contains("logo") && xml.contains("]"));
        assert!(!xml.contains("127.0.0.1"));
    }

    #[test]
    fn code_blocks_keep_their_lines_and_indentation() {
        let xml = document("```\nfn main() {\n    println!(\"<hola>\");\n}\n```\n");
        assert_eq!(xml.matches("w:val=\"CodeBlock\"").count(), 3);
        assert!(xml.contains("    println!(&quot;&lt;hola&gt;&quot;);"));
    }

    #[test]
    fn quotes_rules_and_task_lists_are_marked() {
        let xml = document("> cita\n\n---\n\n- [x] hecho\n- [ ] pendiente\n");
        assert!(xml.contains("w:val=\"Quote\"") && xml.contains("w:left w:val=\"single\""));
        assert!(xml.contains("w:bottom w:val=\"single\""));
        assert!(xml.contains("[x] ") && xml.contains("[ ] "));
    }

    #[test]
    fn hostile_input_never_panics_and_always_yields_a_valid_package() {
        // Caracteres de control (invalidarían el XML), anidación enorme y una tabla anchísima.
        let wide = format!(
            "|{}\n|{}\n|{}\n",
            "a|".repeat(100),
            "-|".repeat(100),
            "b|".repeat(100)
        );
        let nested = format!("{}texto\n", "> ".repeat(200));
        let lists = (0..60)
            .map(|i| format!("{}- nivel {i}\n", "  ".repeat(i)))
            .collect::<String>();
        for input in [
            "\u{0}\u{1}\u{8}\u{B}texto\u{7f}\u{0}".to_string(),
            wide,
            nested,
            lists,
            "<".repeat(5000),
            "[x](".repeat(2000),
            "# ".to_string(),
            String::new(),
        ] {
            let docx = to_docx(&input, PdfPaper::Letter).expect("no falla");
            parts(&docx); // XML bien formado en todas las partes
        }
    }

    #[test]
    fn wide_tables_are_capped_at_words_column_limit() {
        let header = format!("|{}\n|{}\n", "a|".repeat(100), "-|".repeat(100));
        let xml = document(&header);
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let columns = doc
            .descendants()
            .filter(|n| n.tag_name().name() == "gridCol")
            .count();
        assert_eq!(columns, MAX_COLUMNS);
    }

    #[test]
    fn letter_paper_changes_the_page_size() {
        let a4 = document("hola");
        let docx = to_docx("hola", PdfPaper::Letter).unwrap();
        let letter = parts(&docx)
            .into_iter()
            .find(|(n, _)| n == "word/document.xml")
            .unwrap()
            .1;
        assert!(a4.contains("w:w=\"11906\"") && letter.contains("w:w=\"12240\""));
    }

    #[test]
    fn the_converter_declares_exactly_markdown_to_docx() {
        let converter = MarkdownDocxConverter;
        assert_eq!(
            converter.supports(Format::Markdown, Format::Docx),
            Some(Fidelity::Lossless)
        );
        assert_eq!(converter.supports(Format::Docx, Format::Markdown), None);
        assert_eq!(converter.supports(Format::Markdown, Format::Pdf), None);
    }

    #[test]
    fn windows_1252_and_bom_input_is_decoded() {
        let converter = MarkdownDocxConverter;
        let options = ConvertOptions::default();
        let mut cp1252 = b"caf".to_vec();
        cp1252.push(0xE9); // é en Windows-1252
        let out = converter
            .convert(&cp1252, Format::Markdown, Format::Docx, &options)
            .unwrap();
        let xml = parts(&out)
            .into_iter()
            .find(|(n, _)| n == "word/document.xml")
            .unwrap()
            .1;
        assert!(xml.contains("café"));
        let with_bom = [b"\xEF\xBB\xBF".as_slice(), "# ñ".as_bytes()].concat();
        let out = converter
            .convert(&with_bom, Format::Markdown, Format::Docx, &options)
            .unwrap();
        assert!(!parts(&out).is_empty());
    }
}

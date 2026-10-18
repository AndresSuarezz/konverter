//! PDF → Markdown: reconstruye la estructura a partir de la posición, el tamaño y el estilo de
//! cada carácter (lo que da pdfium). Es una **heurística**, no una conversión exacta: un PDF no
//! guarda títulos ni listas, solo texto colocado en la página.
//!
//! Lo que reconoce: títulos (letra claramente mayor que la del cuerpo), párrafos (por el espacio
//! entre líneas), listas con viñetas o numeradas, bloques de código (fuente monoespaciada),
//! negrita y cursiva, palabras partidas con guion al final de línea, y descarta encabezados,
//! pies y números de página repetidos. Lo que **no** reconoce: tablas (salen como texto),
//! imágenes y columnas muy irregulares.
//!
//! La parte de geometría y reglas (`to_markdown`) es pura y se prueba sin pdfium.

use std::collections::{HashMap, HashSet};

use pdfium_render::prelude::*;

/// Un carácter con su estilo y posición (coordenadas de PDF: `y` crece hacia arriba).
#[derive(Debug, Clone)]
pub(crate) struct Glyph {
    pub c: char,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
    pub left: f32,
    pub right: f32,
    pub baseline: f32,
}

pub(crate) struct PageGlyphs {
    pub height: f32,
    pub glyphs: Vec<Glyph>,
}

// ---------------------------------------------------------------- lectura

pub(crate) fn read_page(page: &PdfPage) -> Result<PageGlyphs, PdfiumError> {
    let text = page.text()?;
    let mut glyphs = Vec::new();
    for ch in text.chars().iter() {
        let Some(c) = ch.unicode_char() else { continue };
        // pdfium devuelve el guion de fin de línea como un carácter de control (U+0002) marcado
        // como "guion"; es justo el que hay que conservar para poder unir la palabra partida.
        let hyphen = ch.is_hyphen().unwrap_or(false);
        let c = if hyphen { '-' } else { c };
        if c.is_control() || (!hyphen && ch.is_generated().unwrap_or(false)) {
            continue;
        }
        let Ok(bounds) = ch.loose_bounds() else {
            continue;
        };
        let baseline = ch
            .origin_y()
            .map_or_else(|_| bounds.bottom().value, |y| y.value);
        let name = ch.font_name().to_lowercase();
        let heavy = match ch.font_weight() {
            Some(PdfFontWeight::Weight600 | PdfFontWeight::Weight700Bold) => true,
            Some(PdfFontWeight::Weight800 | PdfFontWeight::Weight900) => true,
            Some(PdfFontWeight::Custom(n)) => n >= 600,
            _ => false,
        };
        glyphs.push(Glyph {
            c,
            size: ch.scaled_font_size().value,
            bold: heavy
                || ch.font_is_bold_reenforced()
                || ["bold", "black", "heavy"].iter().any(|w| name.contains(w)),
            italic: ch.font_is_italic() || name.contains("italic") || name.contains("oblique"),
            mono: ch.font_is_fixed_pitch()
                || ["courier", "mono", "consolas"]
                    .iter()
                    .any(|w| name.contains(w)),
            left: bounds.left().value,
            right: bounds.right().value,
            baseline,
        });
    }
    Ok(PageGlyphs {
        height: page.height().value,
        glyphs,
    })
}

// ------------------------------------------------------------ líneas

struct Line {
    glyphs: Vec<Glyph>,
    left: f32,
    right: f32,
    baseline: f32,
    /// Tamaño más frecuente de la línea.
    size: f32,
    page: usize,
}

impl Line {
    fn text(&self) -> String {
        self.glyphs.iter().map(|g| g.c).collect()
    }

    fn all_mono(&self) -> bool {
        let mut any = false;
        for g in self.glyphs.iter().filter(|g| !g.c.is_whitespace()) {
            if !g.mono {
                return false;
            }
            any = true;
        }
        any
    }
}

fn normalize_char(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{FB00}' => "ff",
        '\u{FB01}' => "fi",
        '\u{FB02}' => "fl",
        '\u{FB03}' => "ffi",
        '\u{FB04}' => "ffl",
        _ => return None,
    })
}

fn finish_line(mut glyphs: Vec<Glyph>, page: usize) -> Option<Line> {
    while glyphs.last().is_some_and(|g| g.c.is_whitespace()) {
        glyphs.pop();
    }
    let start = glyphs.iter().position(|g| !g.c.is_whitespace())?;
    glyphs.drain(..start);
    let mut sizes: HashMap<i32, usize> = HashMap::new();
    for g in glyphs.iter().filter(|g| !g.c.is_whitespace()) {
        *sizes.entry((g.size * 2.0).round() as i32).or_default() += 1;
    }
    let size = sizes
        .into_iter()
        .max_by_key(|&(size, count)| (count, size))
        .map_or(0.0, |(size, _)| size as f32 / 2.0);
    Some(Line {
        left: glyphs.first()?.left,
        right: glyphs.iter().map(|g| g.right).fold(f32::MIN, f32::max),
        baseline: glyphs.first()?.baseline,
        size,
        glyphs,
        page,
    })
}

/// Agrupa los caracteres en líneas por su línea base y añade los espacios que el PDF no
/// escribe (muchos generadores colocan las palabras sin carácter de espacio).
fn build_lines(index: usize, page: &PageGlyphs) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut current: Vec<Glyph> = Vec::new();
    for g in &page.glyphs {
        if let Some(last) = current.last() {
            let new_line = (g.baseline - last.baseline).abs() > 0.35 * g.size.max(last.size)
                || g.left < last.right - 2.0 * g.size.max(1.0);
            if new_line {
                lines.extend(finish_line(std::mem::take(&mut current), index));
            }
        }
        if let Some(last) = current.last().cloned() {
            let gap = g.left - last.right;
            if gap > 0.22 * g.size && !last.c.is_whitespace() && !g.c.is_whitespace() {
                current.push(Glyph {
                    c: ' ',
                    left: last.right,
                    right: g.left,
                    ..last
                });
            } else if g.c.is_whitespace() && last.c.is_whitespace() {
                continue;
            }
        }
        current.push(g.clone());
    }
    lines.extend(finish_line(current, index));
    lines
}

// --------------------------------------------- encabezados y pies repetidos

fn is_page_number(text: &str) -> bool {
    let lower = text.trim().to_lowercase();
    let mut has_digits = false;
    for token in lower.split_whitespace() {
        if token.chars().all(|c| c.is_ascii_digit()) {
            has_digits = true;
        } else if !matches!(
            token,
            "página" | "pagina" | "page" | "pág." | "pag." | "p." | "de" | "of" | "/" | "-"
        ) {
            return false;
        }
    }
    has_digits
}

fn repeat_key(text: &str) -> String {
    let mut key = String::new();
    let mut in_digits = false;
    for c in text.trim().to_lowercase().chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                key.push('#');
            }
            in_digits = true;
        } else {
            in_digits = false;
            key.push(c);
        }
    }
    key
}

fn drop_page_furniture(pages: &[PageGlyphs], lines: &mut Vec<Line>) {
    let at_edge = |l: &Line| {
        let h = pages[l.page].height;
        l.baseline < h * 0.08 || l.baseline > h * 0.92
    };
    let mut seen_on: HashMap<String, HashSet<usize>> = HashMap::new();
    for l in lines.iter().filter(|l| at_edge(l)) {
        seen_on
            .entry(repeat_key(&l.text()))
            .or_default()
            .insert(l.page);
    }
    lines.retain(|l| {
        if !at_edge(l) {
            return true;
        }
        let repeated = pages.len() >= 2 && seen_on[&repeat_key(&l.text())].len() >= 2;
        !(repeated || is_page_number(&l.text()))
    });
}

// ------------------------------------------------------------- bloques

enum Block {
    Heading {
        level: usize,
        text: String,
    },
    Paragraph(String),
    Item {
        marker: String,
        text: String,
    },
    /// (sangría en caracteres, texto)
    Code(Vec<(usize, String)>),
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Heading(usize),
    Paragraph,
    Item,
    Code,
}

struct Last {
    kind: Kind,
    page: usize,
    baseline: f32,
    size: f32,
    left: f32,
    right: f32,
    ends_sentence: bool,
}

fn ends_sentence(text: &str) -> bool {
    text.trim_end()
        .chars()
        .last()
        .is_some_and(|c| matches!(c, '.' | '!' | '?' | ':' | ';' | '…'))
}

/// Viñeta o número al principio de la línea: (marcador Markdown, caracteres a saltar).
fn list_marker(text: &str) -> Option<(String, usize)> {
    let mut chars = text.chars();
    let first = chars.next()?;
    if "•●○▪■◦·–—-*".contains(first) && chars.next().is_some_and(char::is_whitespace)
    {
        let skip = 1 + text
            .chars()
            .skip(1)
            .take_while(|c| c.is_whitespace())
            .count();
        return Some(("-".to_string(), skip));
    }
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    if (1..=3).contains(&digits.len()) {
        let rest = &text[digits.len()..];
        let mut rest_chars = rest.chars();
        if matches!(rest_chars.next(), Some('.' | ')'))
            && rest_chars.next().is_some_and(char::is_whitespace)
        {
            let skip = digits.len()
                + 1
                + rest
                    .chars()
                    .skip(1)
                    .take_while(|c| c.is_whitespace())
                    .count();
            return Some((format!("{digits}."), skip));
        }
    }
    None
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '>') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn plain(glyphs: &[Glyph]) -> String {
    let mut out = String::new();
    for g in glyphs {
        match g.c {
            '\u{00A0}' => out.push(' '),
            '\u{00AD}' => {}
            c => match normalize_char(c) {
                Some(s) => out.push_str(s),
                None => out.push(c),
            },
        }
    }
    out
}

/// Texto de los glifos con negrita, cursiva y código en línea.
fn inline(glyphs: &[Glyph]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < glyphs.len() {
        let style = (glyphs[i].bold, glyphs[i].italic, glyphs[i].mono);
        let mut j = i;
        while j < glyphs.len() && (glyphs[j].bold, glyphs[j].italic, glyphs[j].mono) == style {
            j += 1;
        }
        let chunk = plain(&glyphs[i..j]);
        let core = chunk.trim();
        if core.is_empty() {
            out.push_str(&chunk);
        } else {
            let lead = &chunk[..chunk.len() - chunk.trim_start().len()];
            let trail = &chunk[chunk.trim_end().len()..];
            let marker = match style {
                (_, _, true) => "`",
                (true, true, false) => "***",
                (true, false, false) => "**",
                (false, true, false) => "*",
                _ => "",
            };
            out.push_str(lead);
            out.push_str(marker);
            out.push_str(&if marker == "`" {
                core.to_string()
            } else {
                escape(core)
            });
            out.push_str(marker);
            out.push_str(trail);
        }
        i = j;
    }
    // "**uno** **dos**" -> "**uno dos**"
    out.replace("** **", " ")
}

fn join(previous: &mut String, next: &str) {
    let dehyphenate = previous.ends_with('-')
        && previous
            .chars()
            .rev()
            .nth(1)
            .is_some_and(char::is_alphabetic)
        && next.chars().next().is_some_and(char::is_lowercase);
    if dehyphenate {
        previous.pop();
    } else if !previous.is_empty() {
        previous.push(' ');
    }
    previous.push_str(next);
}

fn most_common_body_size(lines: &[Line]) -> f32 {
    let mut sizes: HashMap<i32, usize> = HashMap::new();
    for l in lines {
        let count = l
            .glyphs
            .iter()
            .filter(|g| !g.c.is_whitespace() && !g.mono)
            .count();
        *sizes.entry((l.size * 2.0).round() as i32).or_default() += count;
    }
    sizes
        .into_iter()
        .max_by_key(|&(size, count)| (count, size))
        .map_or(12.0, |(size, _)| size as f32 / 2.0)
        .max(1.0)
}

fn heading_level(size: f32, body: f32, text: &str) -> Option<usize> {
    let ratio = size / body;
    if text.chars().count() > 160 || ratio < 1.15 {
        None
    } else if ratio >= 1.7 {
        Some(1)
    } else if ratio >= 1.35 {
        Some(2)
    } else {
        Some(3)
    }
}

/// Reconstruye el Markdown de un documento. Vacío si no hay texto.
pub(crate) fn to_markdown(pages: &[PageGlyphs]) -> String {
    let mut lines: Vec<Line> = pages
        .iter()
        .enumerate()
        .flat_map(|(i, page)| build_lines(i, page))
        .collect();
    drop_page_furniture(pages, &mut lines);
    if lines.is_empty() {
        return String::new();
    }
    let body = most_common_body_size(&lines);
    let text_right: Vec<f32> = pages
        .iter()
        .enumerate()
        .map(|(i, _)| {
            lines
                .iter()
                .filter(|l| l.page == i && l.size < body * 1.15)
                .map(|l| l.right)
                .fold(0.0, f32::max)
        })
        .collect();
    let text_left: Vec<f32> = pages
        .iter()
        .enumerate()
        .map(|(i, _)| {
            lines
                .iter()
                .filter(|l| l.page == i)
                .map(|l| l.left)
                .fold(f32::MAX, f32::min)
        })
        .collect();

    let mut blocks: Vec<Block> = Vec::new();
    let mut last: Option<Last> = None;
    for line in &lines {
        let text = plain(&line.glyphs);
        let dy = |last: &Last| {
            if last.page == line.page {
                Some(last.baseline - line.baseline)
            } else {
                None
            }
        };
        // ¿Esta línea continúa el bloque anterior (mismo párrafo o misma viñeta)?
        let continues = |last: &Last| match dy(last) {
            Some(dy) => {
                let width = (text_right[last.page] - text_left[last.page]).max(1.0);
                let short_end =
                    last.right < text_right[last.page] - 0.2 * width && last.ends_sentence;
                dy > 0.0 && dy <= 1.65 * last.size.max(line.size) && !short_end
            }
            None => !last.ends_sentence && text.chars().next().is_some_and(char::is_lowercase),
        };

        if line.all_mono() {
            let entry = (line.left, text.trim_end().to_string());
            match (&last, blocks.last_mut()) {
                (Some(l), Some(Block::Code(code)))
                    if l.kind == Kind::Code
                        && dy(l).is_none_or(|d| d > 0.0 && d <= 2.2 * line.size) =>
                {
                    // Sangría provisional en puntos; se pasa a caracteres al final.
                    code.push((entry.0 as usize, entry.1));
                }
                _ => blocks.push(Block::Code(vec![(entry.0 as usize, entry.1)])),
            }
            last = Some(Last {
                kind: Kind::Code,
                page: line.page,
                baseline: line.baseline,
                size: line.size,
                left: line.left,
                right: line.right,
                ends_sentence: false,
            });
            continue;
        }

        if let Some(level) = heading_level(line.size, body, &text) {
            let heading = escape(text.trim());
            match (&last, blocks.last_mut()) {
                (Some(l), Some(Block::Heading { text: previous, .. }))
                    if l.kind == Kind::Heading(level)
                        && dy(l).is_some_and(|d| d > 0.0 && d <= 1.5 * line.size) =>
                {
                    join(previous, &heading);
                }
                _ => blocks.push(Block::Heading {
                    level,
                    text: heading,
                }),
            }
            last = Some(Last {
                kind: Kind::Heading(level),
                page: line.page,
                baseline: line.baseline,
                size: line.size,
                left: line.left,
                right: line.right,
                ends_sentence: ends_sentence(&text),
            });
            continue;
        }

        if let Some((marker, skip)) = list_marker(&text) {
            let rest = inline(&line.glyphs[skip.min(line.glyphs.len())..]);
            blocks.push(Block::Item {
                marker,
                text: rest.trim().to_string(),
            });
            last = Some(Last {
                kind: Kind::Item,
                page: line.page,
                baseline: line.baseline,
                size: line.size,
                left: line.left,
                right: line.right,
                ends_sentence: ends_sentence(&text),
            });
            continue;
        }

        let rendered = inline(&line.glyphs);
        let appended = match (&last, blocks.last_mut()) {
            (Some(l), Some(Block::Paragraph(previous)))
                if l.kind == Kind::Paragraph && continues(l) =>
            {
                join(previous, &rendered);
                true
            }
            (Some(l), Some(Block::Item { text: previous, .. }))
                if l.kind == Kind::Item
                    && continues(l)
                    && line.left >= l.left - 0.5 * line.size =>
            {
                join(previous, &rendered);
                true
            }
            _ => false,
        };
        if !appended {
            blocks.push(Block::Paragraph(rendered));
        }
        last = Some(Last {
            kind: if appended {
                last.map_or(Kind::Paragraph, |l| l.kind)
            } else {
                Kind::Paragraph
            },
            page: line.page,
            baseline: line.baseline,
            size: line.size,
            left: line.left,
            right: line.right,
            ends_sentence: ends_sentence(&text),
        });
    }
    render(&blocks, body)
}

fn escape_start(paragraph: &str) -> String {
    let digits = paragraph.chars().take_while(char::is_ascii_digit).count();
    let after = &paragraph[digits..];
    if digits > 0 && (after.starts_with(". ") || after.starts_with(") ")) {
        return format!("{}\\{}", &paragraph[..digits], after);
    }
    if paragraph.starts_with(['#', '-', '+']) {
        return format!("\\{paragraph}");
    }
    paragraph.to_string()
}

fn render(blocks: &[Block], body: f32) -> String {
    let mut out = String::new();
    // Tipo de la lista anterior (¿numerada?) si el bloque anterior era una viñeta.
    let mut previous_list: Option<bool> = None;
    for block in blocks {
        let this_list = match block {
            Block::Item { marker, .. } => Some(marker != "-"),
            _ => None,
        };
        if !out.is_empty() {
            // Las viñetas consecutivas del mismo tipo forman una sola lista, sin línea en blanco.
            let same_list = this_list.is_some() && this_list == previous_list;
            out.push_str(if same_list { "\n" } else { "\n\n" });
        }
        match block {
            Block::Heading { level, text } => {
                out.push_str(&"#".repeat(*level));
                out.push(' ');
                out.push_str(text);
            }
            Block::Paragraph(text) => out.push_str(&escape_start(text)),
            Block::Item { marker, text } => {
                out.push_str(marker);
                out.push(' ');
                out.push_str(text);
            }
            Block::Code(lines) => {
                let min_left = lines.iter().map(|(left, _)| *left).min().unwrap_or(0);
                let char_width = (body * 0.6).max(1.0) as usize;
                out.push_str("```\n");
                for (left, text) in lines {
                    out.push_str(&" ".repeat((left - min_left) / char_width));
                    out.push_str(text);
                    out.push('\n');
                }
                out.push_str("```");
            }
        }
        previous_list = this_list;
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Escribe `text` en la página como lo haría un PDF: un glifo por carácter, ancho 0,5·tamaño.
    fn put(page: &mut PageGlyphs, text: &str, x: f32, y: f32, size: f32, bold: bool, mono: bool) {
        let mut cursor = x;
        for c in text.chars() {
            let width = if mono { 0.6 } else { 0.5 } * size;
            page.glyphs.push(Glyph {
                c,
                size,
                bold,
                italic: false,
                mono,
                left: cursor,
                right: cursor + width,
                baseline: y,
            });
            cursor += width;
        }
    }

    fn page() -> PageGlyphs {
        PageGlyphs {
            height: 842.0,
            glyphs: Vec::new(),
        }
    }

    #[test]
    fn headings_by_size_paragraphs_by_gap_and_lists() {
        let mut p = page();
        put(&mut p, "Manual de usuario", 72.0, 700.0, 24.0, true, false);
        put(
            &mut p,
            "Este documento explica",
            72.0,
            660.0,
            11.0,
            false,
            false,
        );
        put(
            &mut p,
            "cómo usar el programa.",
            72.0,
            646.0,
            11.0,
            false,
            false,
        );
        put(
            &mut p,
            "Otro párrafo aparte.",
            72.0,
            610.0,
            11.0,
            false,
            false,
        );
        put(&mut p, "Instalación", 72.0, 570.0, 16.0, true, false);
        put(&mut p, "• Primer paso", 72.0, 540.0, 11.0, false, false);
        put(&mut p, "• Segundo paso", 72.0, 526.0, 11.0, false, false);
        put(&mut p, "1. Descargar", 72.0, 500.0, 11.0, false, false);
        put(&mut p, "2. Ejecutar", 72.0, 486.0, 11.0, false, false);
        let md = to_markdown(&[p]);
        assert_eq!(
            md,
            "# Manual de usuario\n\n\
             Este documento explica cómo usar el programa.\n\n\
             Otro párrafo aparte.\n\n\
             ## Instalación\n\n\
             - Primer paso\n- Segundo paso\n\n\
             1. Descargar\n2. Ejecutar\n"
        );
    }

    #[test]
    fn code_blocks_keep_relative_indentation() {
        let mut p = page();
        put(&mut p, "Para compilar:", 72.0, 700.0, 11.0, false, false);
        put(&mut p, "fn main() {", 72.0, 670.0, 10.0, false, true);
        put(&mut p, "    run();", 72.0, 658.0, 10.0, false, true);
        put(&mut p, "}", 72.0, 646.0, 10.0, false, true);
        put(&mut p, "Eso es todo.", 72.0, 610.0, 11.0, false, false);
        let md = to_markdown(&[p]);
        assert!(md.contains("```\nfn main() {\n    run();\n}\n```"), "{md}");
        assert!(md.starts_with("Para compilar:\n\n```"));
    }

    #[test]
    fn hyphenated_words_are_joined_and_emphasis_is_marked() {
        let mut p = page();
        put(&mut p, "Lee la docu-", 72.0, 700.0, 11.0, false, false);
        put(
            &mut p,
            "mentación con cuidado:",
            72.0,
            686.0,
            11.0,
            false,
            false,
        );
        put(&mut p, "importante", 72.0, 672.0, 11.0, true, false);
        let md = to_markdown(&[p]);
        assert!(
            md.contains("Lee la documentación con cuidado: **importante**"),
            "{md}"
        );
    }

    #[test]
    fn repeated_headers_footers_and_page_numbers_are_dropped() {
        let mut pages = Vec::new();
        for n in 1..=3 {
            let mut p = page();
            put(&mut p, "Informe interno", 72.0, 810.0, 9.0, false, false);
            put(
                &mut p,
                &format!("Texto de la página {n}."),
                72.0,
                600.0,
                11.0,
                false,
                false,
            );
            put(
                &mut p,
                &format!("Página {n} de 3"),
                250.0,
                30.0,
                9.0,
                false,
                false,
            );
            pages.push(p);
        }
        let md = to_markdown(&pages);
        assert!(!md.contains("Informe interno"), "{md}");
        assert!(!md.contains("de 3"), "{md}");
        assert!(md.contains("Texto de la página 1.") && md.contains("Texto de la página 3."));
    }

    #[test]
    fn a_paragraph_continues_across_a_page_break() {
        let mut one = page();
        put(
            &mut one,
            "Esta frase empieza al final de la",
            72.0,
            120.0,
            11.0,
            false,
            false,
        );
        let mut two = page();
        put(
            &mut two,
            "página anterior y termina aquí.",
            72.0,
            780.0,
            11.0,
            false,
            false,
        );
        let md = to_markdown(&[one, two]);
        assert_eq!(
            md,
            "Esta frase empieza al final de la página anterior y termina aquí.\n"
        );
    }

    #[test]
    fn special_characters_are_escaped_and_nothing_means_empty() {
        let mut p = page();
        put(
            &mut p,
            "2 * 3 = 6 y snake_case [x]",
            72.0,
            700.0,
            11.0,
            false,
            false,
        );
        put(&mut p, "# no es título", 72.0, 600.0, 11.0, false, false);
        let md = to_markdown(&[p]);
        assert!(md.contains(r"2 \* 3 = 6 y snake\_case \[x\]"), "{md}");
        assert!(md.contains(r"\# no es título"), "{md}");
        assert_eq!(to_markdown(&[page()]), "");
    }

    #[test]
    fn words_without_space_glyphs_get_spaces_from_the_gap() {
        let mut p = page();
        put(&mut p, "Hola", 72.0, 700.0, 11.0, false, false);
        put(
            &mut p,
            "mundo",
            72.0 + 4.0 * 5.5 + 4.0,
            700.0,
            11.0,
            false,
            false,
        );
        assert_eq!(to_markdown(&[p]), "Hola mundo\n");
    }

    #[test]
    fn list_marker_recognises_bullets_and_numbers_only() {
        assert_eq!(list_marker("• uno"), Some(("-".into(), 2)));
        assert_eq!(list_marker("- uno"), Some(("-".into(), 2)));
        assert_eq!(list_marker("12) doce"), Some(("12.".into(), 4)));
        assert_eq!(list_marker("3.14 es pi"), None);
        assert_eq!(list_marker("-5 grados"), None);
        assert_eq!(list_marker("2024 fue un año"), None);
    }
}

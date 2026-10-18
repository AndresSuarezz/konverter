//! DOCX → TXT en Rust puro (un DOCX es un ZIP con `word/document.xml`): no necesita Word ni
//! LibreOffice. Extrae el texto del cuerpo; no incluye notas al pie, encabezados ni pies de página.

use std::io::{Cursor, Read};

use roxmltree::{Document, Node, ParsingOptions};

use crate::convert::{ConvertOptions, Converter, Fidelity};
use crate::error::{Error, Result};
use crate::format::Format;

/// Tope de lo que se descomprime de `document.xml` (protege de ZIP "bomba").
const MAX_XML_BYTES: u64 = 200 * 1024 * 1024;

/// Conversión DOCX → TXT.
pub struct DocxTextConverter;

impl Converter for DocxTextConverter {
    fn supports(&self, from: Format, to: Format) -> Option<Fidelity> {
        (from == Format::Docx && to == Format::Txt).then_some(Fidelity::Lossy)
    }

    fn convert(
        &self,
        input: &[u8],
        from: Format,
        to: Format,
        _options: &ConvertOptions,
    ) -> Result<Vec<u8>> {
        if self.supports(from, to).is_none() {
            return Err(Error::Unsupported { from, to });
        }
        let fail = |message: String| Error::Decode {
            format: Format::Docx,
            message,
        };
        let mut archive = zip::ZipArchive::new(Cursor::new(input))
            .map_err(|e| fail(format!("no es un DOCX válido: {e}")))?;
        let mut xml = String::new();
        archive
            .by_name("word/document.xml")
            .map_err(|_| fail("no contiene word/document.xml".into()))?
            .take(MAX_XML_BYTES)
            .read_to_string(&mut xml)
            .map_err(|e| fail(format!("document.xml ilegible: {e}")))?;

        let text = extract_text(&xml).map_err(fail)?;
        if text.trim().is_empty() {
            return Err(fail("el documento no contiene texto".into()));
        }
        // UTF-8 con BOM y CRLF, como el resto de salidas TXT.
        let mut out = b"\xEF\xBB\xBF".to_vec();
        out.extend_from_slice(text.replace('\n', "\r\n").as_bytes());
        Ok(out)
    }
}

pub(crate) fn extract_text(xml: &str) -> std::result::Result<String, String> {
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: 20_000_000,
        ..ParsingOptions::default()
    };
    let document =
        Document::parse_with_options(xml, options).map_err(|e| format!("XML inválido: {e}"))?;
    let mut out = String::new();
    walk(document.root(), &mut out, false);
    Ok(out.trim_end().to_string())
}

/// Recorre bloques (párrafos, tablas) en orden. Dentro de una celda, los párrafos se separan con
/// espacio y las celdas con tabulador, para que una tabla siga leyéndose como tabla.
fn walk(node: Node, out: &mut String, in_cell: bool) {
    for child in node.children().filter(Node::is_element) {
        match child.tag_name().name() {
            "p" => {
                paragraph(child, out);
                out.push(if in_cell { ' ' } else { '\n' });
            }
            "tc" => {
                walk(child, out, true);
                // El último párrafo de la celda dejó un espacio separador: sobra antes del tabulador.
                while out.ends_with(' ') {
                    out.pop();
                }
                out.push('\t');
            }
            "tr" => {
                walk(child, out, in_cell);
                while out.ends_with(['\t', ' ']) {
                    out.pop();
                }
                out.push('\n');
            }
            _ => walk(child, out, in_cell),
        }
    }
}

fn paragraph(p: Node, out: &mut String) {
    for node in p.descendants().filter(Node::is_element) {
        match node.tag_name().name() {
            "t" => out.push_str(node.text().unwrap_or("")),
            "tab" => out.push('\t'),
            "br" | "cr" => out.push('\n'),
            "noBreakHyphen" => out.push('-'),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(inner: &str) -> String {
        format!(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{inner}</w:body></w:document>"#
        )
    }

    #[test]
    fn paragraphs_runs_tabs_and_breaks() {
        let xml = body(
            r#"<w:p><w:r><w:t>Hola </w:t></w:r><w:r><w:t>mundo</w:t></w:r></w:p>
               <w:p><w:r><w:t>a</w:t><w:tab/><w:t>b</w:t><w:br/><w:t>c</w:t></w:r></w:p>"#,
        );
        assert_eq!(extract_text(&xml).unwrap(), "Hola mundo\na\tb\nc");
    }

    #[test]
    fn tables_read_as_rows_of_tab_separated_cells() {
        let xml = body(
            r#"<w:tbl>
                 <w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr>
                 <w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p><w:p><w:r><w:t>dos</w:t></w:r></w:p></w:tc></w:tr>
               </w:tbl>
               <w:p><w:r><w:t>después</w:t></w:r></w:p>"#,
        );
        assert_eq!(extract_text(&xml).unwrap(), "A\tB\n1\t2 dos\ndespués");
    }

    #[test]
    fn entities_accents_and_deleted_text() {
        let xml = body(
            r#"<w:p><w:r><w:t>Tom &amp; Jerry: ñandú</w:t></w:r><w:del><w:r><w:delText>borrado</w:delText></w:r></w:del></w:p>"#,
        );
        assert_eq!(extract_text(&xml).unwrap(), "Tom & Jerry: ñandú");
    }

    #[test]
    fn dtd_is_rejected() {
        let xml =
            r#"<!DOCTYPE d [<!ENTITY a "x">]><w:document xmlns:w="urn:w"><w:body/></w:document>"#;
        assert!(extract_text(xml).is_err());
    }

    #[test]
    fn invalid_zip_and_missing_part_are_decode_errors() {
        let converter = DocxTextConverter;
        let options = ConvertOptions::default();
        assert!(matches!(
            converter.convert(b"PK\x03\x04no", Format::Docx, Format::Txt, &options),
            Err(Error::Decode {
                format: Format::Docx,
                ..
            })
        ));
        assert!(matches!(
            converter.convert(b"hola", Format::Docx, Format::Txt, &options),
            Err(Error::Decode { .. })
        ));
    }
}

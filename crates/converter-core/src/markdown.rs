//! Markdown → HTML (Rust puro, `pulldown-cmark`). El HTML se entrega después a Word o
//! LibreOffice para producir un PDF, así que el documento es autocontenido (hoja de estilos
//! incluida) y **seguro**: el HTML en bruto del Markdown se escapa, las imágenes no se
//! descargan (se sustituyen por su texto alternativo) y solo se conservan los enlaces
//! `http`, `https` y `mailto`.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};

/// Estilo mínimo y compatible con los importadores de HTML de Word y LibreOffice (que entienden
/// un subconjunto de CSS: nada de flexbox ni variables).
const CSS: &str = "\
body { font-family: Arial, 'Liberation Sans', sans-serif; font-size: 11pt; line-height: 1.4; }\
h1, h2, h3, h4, h5, h6 { font-family: Arial, 'Liberation Sans', sans-serif; }\
h1 { font-size: 22pt; } h2 { font-size: 17pt; } h3 { font-size: 14pt; }\
h4, h5, h6 { font-size: 11pt; }\
code, pre { font-family: 'Courier New', 'Liberation Mono', monospace; font-size: 10pt; }\
pre { background-color: #f3f3f3; border: 1px solid #d0d0d0; padding: 6px; }\
blockquote { border-left: 3px solid #bbbbbb; margin-left: 0; padding-left: 10px; color: #444444; }\
table { border-collapse: collapse; }\
th, td { border: 1px solid #999999; padding: 4px 8px; }\
th { background-color: #eeeeee; }\
a { color: #0b57d0; }";

pub(crate) fn to_html(markdown: &str) -> String {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut body = String::new();
    html::push_html(&mut body, sanitize(Parser::new_ext(markdown, options)));
    // LibreOffice ignora los bordes de tabla de la hoja de estilos pero respeta estos atributos.
    let body = body.replace(
        "<table>",
        "<table border=\"1\" cellspacing=\"0\" cellpadding=\"4\">",
    );
    format!(
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>Documento</title>\
         <style>{CSS}</style></head><body>\n{body}</body></html>\n"
    )
}

fn is_safe_link(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
}

/// Quita lo peligroso o inútil sin cambiar el texto: HTML en bruto → texto, imágenes → texto
/// alternativo entre corchetes, enlaces con esquemas raros → solo su texto.
fn sanitize<'a>(events: impl Iterator<Item = Event<'a>>) -> impl Iterator<Item = Event<'a>> {
    let mut link_kept: Vec<bool> = Vec::new();
    events.flat_map(move |event| -> Vec<Event<'a>> {
        match event {
            Event::Html(raw) | Event::InlineHtml(raw) => vec![Event::Text(raw)],
            Event::Start(Tag::Image { .. }) => vec![Event::Text("[imagen: ".into())],
            Event::End(TagEnd::Image) => vec![Event::Text("]".into())],
            Event::Start(Tag::Link { ref dest_url, .. }) => {
                let keep = is_safe_link(dest_url);
                link_kept.push(keep);
                if keep { vec![event] } else { Vec::new() }
            }
            Event::End(TagEnd::Link) => {
                if link_kept.pop().unwrap_or(false) {
                    vec![event]
                } else {
                    Vec::new()
                }
            }
            other => vec![other],
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_lists_tables_and_code_become_html() {
        let html = to_html(
            "# Título\n\nHola **mundo** y *ñandú*.\n\n- uno\n- dos\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```\ncargo build\n```\n",
        );
        assert!(html.contains("<h1>Título</h1>"));
        assert!(html.contains("<strong>mundo</strong>") && html.contains("<em>ñandú</em>"));
        assert!(html.contains("<li>uno</li>"));
        assert!(html.contains("<table border=\"1\"") && html.contains("<th>a</th>"));
        assert!(html.contains("<pre><code>cargo build"));
        assert!(html.contains("charset=\"utf-8\""));
    }

    #[test]
    fn raw_html_is_escaped_not_interpreted() {
        let html = to_html(
            "Antes <script>alert(1)</script> después\n\n<iframe src=\"http://x\"></iframe>\n",
        );
        assert!(!html.contains("<script>") && !html.contains("<iframe"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn images_are_never_fetched_and_unsafe_links_lose_their_target() {
        let html = to_html(
            "![logo](http://tracker.example/x.png) [mal](javascript:alert(1)) [bien](https://ejemplo.com) [correo](mailto:a@b.co)",
        );
        assert!(!html.contains("<img") && !html.contains("tracker.example"));
        assert!(html.contains("[imagen: logo]"));
        assert!(!html.contains("javascript:"));
        assert!(html.contains("mal"), "el texto del enlace se conserva");
        assert!(
            html.contains("href=\"https://ejemplo.com\"")
                && html.contains("href=\"mailto:a@b.co\"")
        );
    }
}

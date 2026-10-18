//! Convierte un archivo desde la línea de comandos, para revisar resultados a mano.
//!
//! ```text
//! cargo run -p converter-core --example convert -- <archivo> <formato> [word|libreoffice]
//! ```
//! `<formato>` es la extensión de destino (pdf, md, docx, png...). El tercer argumento fuerza el
//! motor de documentos de Office. Imprime las rutas escritas.

use converter_core::{ConvertOptions, DocumentEngine, Engine, Format};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, target, rest @ ..] = args.as_slice() else {
        eprintln!("uso: convert <archivo> <formato> [word|libreoffice]");
        std::process::exit(2);
    };
    let Some(to) = Format::from_extension(target) else {
        eprintln!("formato desconocido: {target}");
        std::process::exit(2);
    };
    let options = ConvertOptions {
        document_engine: match rest.first().map(String::as_str) {
            Some("wordcraft") => DocumentEngine::WordCraft,
            Some("word") => DocumentEngine::Word,
            Some("libreoffice") => DocumentEngine::LibreOffice,
            _ => DocumentEngine::Auto,
        },
        ..Default::default()
    };
    match Engine::default().convert_file(input.as_ref(), to, &options) {
        Ok(paths) => paths.iter().for_each(|p| println!("{}", p.display())),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

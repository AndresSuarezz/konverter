//! PDF → Markdown con PDF reales (`tests/fixtures`, escritos a mano sin usar el motor).
//! Necesita pdfium: ejecuta `scripts/fetch-pdfium.ps1` una vez. Markdown → PDF (que usa Word o
//! LibreOffice) se prueba en `tests/office.rs`.

use std::fs;
use std::path::{Path, PathBuf};

use converter_core::{ConvertOptions, Engine, Error, Format};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn markdown_of(name: &str) -> String {
    let bytes = fs::read(fixture(name)).unwrap();
    let out = Engine::default()
        .convert_bytes(
            &bytes,
            Format::Pdf,
            Format::Markdown,
            &ConvertOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{name}: {e} (¿falta pdfium? scripts/fetch-pdfium.ps1)"));
    String::from_utf8(out).expect("Markdown en UTF-8")
}

#[test]
fn structured_pdf_becomes_structured_markdown() {
    let md = markdown_of("structured.pdf");
    let expected = "# Manual de usuario\n\n\
         Este manual explica la documentación del programa y sus funciones principales.\n\n\
         Lee con cuidado **importante** antes de empezar.\n\n\
         ## Instalación\n\n\
         - Descargar el instalador\n- Ejecutarlo y aceptar\n\n\
         1. Abrir la aplicación\n2. Pulsar el atajo\n\n\
         ```\ncargo build --release\n  converter.exe\n```\n\n\
         Esta frase termina en la página siguiente sin cortar el párrafo.\n\n\
         ## Notas\n\n\
         Texto final de la segunda página.\n";
    assert_eq!(md, expected, "\n--- obtenido ---\n{md}");
}

#[test]
fn repeated_header_and_footer_are_not_in_the_output() {
    let md = markdown_of("structured.pdf");
    assert!(!md.contains("Informe interno"), "{md}");
    assert!(!md.contains("Página 1") && !md.contains("Página 2"), "{md}");
}

#[test]
fn plain_text_pdf_keeps_accents_and_page_order() {
    let md = markdown_of("sample.pdf");
    assert!(md.contains("Hola mundo, ñandú y café"), "{md}");
    assert!(md.find("Página 1").unwrap() < md.find("Página 3").unwrap());
}

#[test]
fn scanned_pdf_asks_for_ocr_and_corrupt_pdf_is_a_decode_error() {
    let engine = Engine::default();
    let options = ConvertOptions::default();
    let scan = fs::read(fixture("scan.pdf")).unwrap();
    assert!(matches!(
        engine.convert_bytes(&scan, Format::Pdf, Format::Markdown, &options),
        Err(Error::NeedsOcr {
            format: Format::Pdf
        })
    ));
    assert!(matches!(
        engine.convert_bytes(b"%PDF-1.4\nbasura", Format::Pdf, Format::Markdown, &options),
        Err(Error::Decode {
            format: Format::Pdf,
            ..
        })
    ));
}

#[test]
fn convert_file_writes_an_md_next_to_the_pdf_without_a_bom() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("Manual ñ.pdf");
    fs::copy(fixture("structured.pdf"), &source).unwrap();
    let out = Engine::default()
        .convert_file(&source, Format::Markdown, &ConvertOptions::default())
        .unwrap();
    assert_eq!(out, [dir.path().join("Manual ñ.md")]);
    assert!(fs::read(&out[0]).unwrap().starts_with(b"# Manual"));
}

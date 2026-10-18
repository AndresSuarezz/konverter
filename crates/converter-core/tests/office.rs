//! Tests de documentos de Office con archivos reales (`tests/fixtures`).
//!
//! DOCX → TXT es Rust puro y corre siempre. Las conversiones que usan Word o LibreOffice están
//! marcadas `#[ignore]` porque dependen de un programa externo y automatizar Word puede tardar
//! o colgarse en equipos sin él. Se ejecutan a propósito con:
//!
//! ```text
//! cargo test -p converter-core --test office -- --ignored --test-threads=1
//! ```
//! (un hilo: dos Word automatizados a la vez no es un caso que queramos probar aquí).

use std::fs;
use std::path::{Path, PathBuf};

use converter_core::{ConvertOptions, DocumentEngine, Engine, Error, Format};

/// Pares que convierte un programa externo. `tests/conversions.rs` los excluye de su recorrido
/// automático y comprueba que esta lista siga siendo exactamente la del motor.
const ENGINE_PAIRS: [(Format, Format); 5] = [
    (Format::Docx, Format::Pdf),
    (Format::Markdown, Format::Pdf),
    (Format::Doc, Format::Pdf),
    (Format::Doc, Format::Docx),
    (Format::Pdf, Format::Docx),
];

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn read(name: &str) -> Vec<u8> {
    fs::read(fixture(name)).unwrap_or_else(|e| panic!("falta el fixture {name}: {e}"))
}

fn with(engine: DocumentEngine) -> ConvertOptions {
    ConvertOptions {
        document_engine: engine,
        ..Default::default()
    }
}

fn convert(input: &[u8], from: Format, to: Format, engine: DocumentEngine) -> Vec<u8> {
    Engine::default()
        .convert_bytes(input, from, to, &with(engine))
        .unwrap_or_else(|e| panic!("{from} -> {to} con {engine:?}: {e}"))
}

/// Texto de un PDF, para comprobar lo que generó el motor (usa pdfium).
fn pdf_text(pdf: &[u8]) -> String {
    let txt = convert(pdf, Format::Pdf, Format::Txt, DocumentEngine::Auto);
    String::from_utf8(txt[3..].to_vec()).unwrap()
}

fn docx_text(docx: &[u8]) -> String {
    let txt = convert(docx, Format::Docx, Format::Txt, DocumentEngine::Auto);
    String::from_utf8(txt[3..].to_vec()).unwrap()
}

/// Comparación tolerante a saltos de línea y espacios: cada motor maqueta distinto.
fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn word_pids() -> Vec<u32> {
    let out = std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq WINWORD.EXE", "/FO", "CSV", "/NH"])
        .output()
        .expect("tasklist");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.split(',').nth(1))
        .filter_map(|pid| pid.trim_matches('"').parse().ok())
        .collect()
}

// ------------------------------------------------- DOCX -> TXT (Rust puro)

#[test]
fn docx_to_txt_reads_paragraphs_tabs_tables_and_accents() {
    let txt = Engine::default()
        .convert_bytes(
            &read("sample.docx"),
            Format::Docx,
            Format::Txt,
            &ConvertOptions::default(),
        )
        .unwrap();
    assert!(txt.starts_with(b"\xEF\xBB\xBF"), "UTF-8 con BOM");
    let text = String::from_utf8(txt[3..].to_vec()).unwrap();
    assert_eq!(
        text,
        "Informe de prueba\r\n\
         Hola mundo, ñandú y café: acentuación correcta.\r\n\
         uno\tdos\r\n\
         Producto\tPrecio\r\n\
         Café\t2.500\r\n\
         Fin del documento."
    );
}

#[test]
fn corrupt_docx_is_a_decode_error() {
    let engine = Engine::default();
    let options = ConvertOptions::default();
    let docx = read("sample.docx");
    for (label, bytes) in [
        ("truncado", docx[..docx.len() / 2].to_vec()),
        ("no es zip", b"PK\x03\x04basura".to_vec()),
    ] {
        let err = engine
            .convert_bytes(&bytes, Format::Docx, Format::Txt, &options)
            .unwrap_err();
        assert!(
            matches!(
                err,
                Error::Decode {
                    format: Format::Docx,
                    ..
                }
            ),
            "{label}: {err}"
        );
    }
}

#[test]
fn docx_and_doc_are_detected_by_extension_and_content() {
    let dir = tempfile::tempdir().unwrap();
    for (name, expected) in [("sample.docx", Format::Docx), ("sample.doc", Format::Doc)] {
        let path = dir.path().join(name);
        fs::copy(fixture(name), &path).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert_eq!(
            Format::detect(&path, &bytes[..64]),
            Some(expected),
            "{name}"
        );
    }
}

#[test]
fn engine_pairs_listed_here_are_exactly_the_ones_the_engine_offers() {
    let engine = Engine::default();
    let mut offered = Vec::new();
    for from in [Format::Docx, Format::Doc, Format::Pdf, Format::Markdown] {
        for target in engine.targets(from) {
            if matches!(
                (from, target.format),
                (Format::Docx | Format::Markdown, Format::Pdf)
                    | (Format::Doc, Format::Pdf | Format::Docx)
                    | (Format::Pdf, Format::Docx)
            ) {
                offered.push((from, target.format));
            }
        }
    }
    offered.sort();
    let mut expected = ENGINE_PAIRS.to_vec();
    expected.sort();
    assert_eq!(offered, expected);
}

#[test]
fn without_an_engine_the_error_says_what_to_install() {
    if converter_core::find_libreoffice().is_some() {
        return; // hay LibreOffice: no se puede probar su ausencia
    }
    let err = Engine::default()
        .convert_bytes(
            &read("sample.docx"),
            Format::Docx,
            Format::Pdf,
            &with(DocumentEngine::LibreOffice),
        )
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::MissingDependency {
                name: "LibreOffice",
                ..
            }
        ),
        "{err}"
    );
    assert!(err.to_string().contains("CONVERTER_LIBREOFFICE"));
}

// --------------------------------------------------------------- Word

#[test]
#[ignore = "requiere Microsoft Word instalado"]
fn word_converts_every_engine_pair_with_real_content() {
    let before = word_pids();
    for (from, to) in ENGINE_PAIRS {
        let name = match from {
            Format::Docx => "sample.docx",
            Format::Doc => "sample.doc",
            Format::Markdown => "sample.md",
            _ => "sample.pdf",
        };
        let out = convert(&read(name), from, to, DocumentEngine::Word);
        match to {
            Format::Pdf => {
                assert!(out.starts_with(b"%PDF-"), "{from} -> PDF");
                let text = flat(&pdf_text(&out));
                assert!(
                    text.contains("Hola mundo, ñandú y café: acentuación correcta."),
                    "{from} -> PDF: {text:?}"
                );
                if from == Format::Markdown {
                    // El título, la lista, la tabla y el código llegaron al PDF; la imagen remota no.
                    for expected in [
                        "Informe de prueba",
                        "uno",
                        "dos",
                        "Producto",
                        "2.500",
                        "cargo build",
                        "la web",
                    ] {
                        assert!(text.contains(expected), "falta {expected:?}: {text:?}");
                    }
                    assert!(text.contains("[imagen: logo]"), "{text:?}");
                }
                assert!(
                    text.contains("Producto Precio") || text.contains("Producto"),
                    "{text:?}"
                );
            }
            _ => {
                assert!(out.starts_with(b"PK"), "{from} -> DOCX es un ZIP");
                let text = flat(&docx_text(&out));
                let needle = if from == Format::Pdf {
                    "Hola mundo, ñandú y café"
                } else {
                    "Hola mundo, ñandú y café: acentuación correcta."
                };
                assert!(text.contains(needle), "{from} -> DOCX: {text:?}");
            }
        }
    }
    let leaked: Vec<_> = word_pids()
        .into_iter()
        .filter(|pid| !before.contains(pid))
        .collect();
    assert!(leaked.is_empty(), "Word se quedó abierto: {leaked:?}");
}

#[test]
#[ignore = "requiere Microsoft Word instalado"]
fn word_docx_to_pdf_is_a_real_a4_document() {
    let pdf = convert(
        &read("sample.docx"),
        Format::Docx,
        Format::Pdf,
        DocumentEngine::Word,
    );
    let pages = Engine::default()
        .convert_bytes_pages(
            &pdf,
            Format::Pdf,
            Format::Png,
            &ConvertOptions {
                pdf_dpi: 72,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(pages.len(), 1);
    let page = image::load_from_memory(&pages[0]).unwrap();
    assert_eq!((page.width(), page.height()), (595, 842), "A4 como el DOCX");
}

#[test]
#[ignore = "requiere Microsoft Word instalado"]
fn word_reports_a_corrupt_document_instead_of_hanging() {
    let started = std::time::Instant::now();
    let err = Engine::default()
        .convert_bytes(
            b"PK\x03\x04esto no es un docx",
            Format::Docx,
            Format::Pdf,
            &with(DocumentEngine::Word),
        )
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::External {
                tool: "Microsoft Word",
                ..
            }
        ),
        "{err}"
    );
    assert!(
        started.elapsed().as_secs() < 60,
        "tardó {:?}",
        started.elapsed()
    );
}

#[test]
#[ignore = "requiere Microsoft Word instalado"]
fn word_convert_file_writes_next_to_the_original() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("Mi informe ñ.docx");
    fs::copy(fixture("sample.docx"), &source).unwrap();
    let before = fs::read(&source).unwrap();
    let out = Engine::default()
        .convert_file(&source, Format::Pdf, &with(DocumentEngine::Word))
        .unwrap();
    assert_eq!(out, [dir.path().join("Mi informe ñ.pdf")]);
    assert_eq!(fs::read(&source).unwrap(), before);
    assert!(fs::read(&out[0]).unwrap().starts_with(b"%PDF-"));
}

// --------------------------------------------------------- LibreOffice

#[test]
#[ignore = "requiere LibreOffice (define CONVERTER_LIBREOFFICE o instálalo)"]
fn libreoffice_converts_every_engine_pair_with_real_content() {
    assert!(
        converter_core::find_libreoffice().is_some(),
        "no se encontró LibreOffice"
    );
    for (from, to) in ENGINE_PAIRS {
        let name = match from {
            Format::Docx => "sample.docx",
            Format::Doc => "sample.doc",
            Format::Markdown => "sample.md",
            _ => "sample.pdf",
        };
        let out = convert(&read(name), from, to, DocumentEngine::LibreOffice);
        match to {
            Format::Pdf => {
                assert!(out.starts_with(b"%PDF-"), "{from} -> PDF");
                let text = flat(&pdf_text(&out));
                assert!(
                    text.contains("Hola mundo, ñandú y café: acentuación correcta."),
                    "{from} -> PDF: {text:?}"
                );
                if from == Format::Markdown {
                    // El título, la lista, la tabla y el código llegaron al PDF; la imagen remota no.
                    for expected in [
                        "Informe de prueba",
                        "uno",
                        "dos",
                        "Producto",
                        "2.500",
                        "cargo build",
                        "la web",
                    ] {
                        assert!(text.contains(expected), "falta {expected:?}: {text:?}");
                    }
                    assert!(text.contains("[imagen: logo]"), "{text:?}");
                }
            }
            _ => {
                assert!(out.starts_with(b"PK"), "{from} -> DOCX es un ZIP");
                let text = flat(&docx_text(&out));
                assert!(text.contains("Hola mundo"), "{from} -> DOCX: {text:?}");
            }
        }
    }
}

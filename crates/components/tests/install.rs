//! Instalación real de LibreOffice (descarga ~360 MB). Se ejecuta a propósito:
//!
//! ```text
//! cargo test -p components --test install -- --ignored --nocapture
//! ```

use std::fs;
use std::time::Instant;

use components::{Progress, install_libreoffice_into};
use converter_core::{ConvertOptions, DocumentEngine, Engine, Format};

#[test]
#[ignore = "descarga 360 MB de internet y tarda varios minutos"]
fn installs_libreoffice_and_it_converts_a_real_document() {
    let root = tempfile::tempdir().unwrap();
    let started = Instant::now();
    let mut phases = Vec::new();
    let mut last_percent = 0;
    let soffice = install_libreoffice_into(root.path(), &mut |p| match p {
        Progress::Downloading { done, total } => {
            let percent = done * 100 / total;
            if percent >= last_percent + 10 {
                last_percent = percent;
                println!(
                    "  descargando {percent} % ({:.0} s)",
                    started.elapsed().as_secs_f32()
                );
            }
        }
        other => {
            println!("  {other:?} ({:.0} s)", started.elapsed().as_secs_f32());
            phases.push(other);
        }
    })
    .unwrap_or_else(|e| panic!("la instalación falló: {e}"));

    assert!(soffice.is_file(), "{}", soffice.display());
    assert_eq!(
        phases,
        [
            Progress::Verifying,
            Progress::Extracting,
            Progress::Finished
        ]
    );
    let leftovers: Vec<_> = fs::read_dir(root.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        leftovers,
        ["libreoffice-26.8.1"],
        "sin .msi, .part, .partial ni bloqueo"
    );

    // Segunda llamada: ya instalado, no descarga ni extrae nada.
    let mut again = Vec::new();
    let same = install_libreoffice_into(root.path(), &mut |p| again.push(p)).unwrap();
    assert_eq!(same, soffice);
    assert_eq!(again, [Progress::Finished]);

    // Y el motor lo usa de verdad para convertir un documento.
    let docx = root.path().join("prueba.docx");
    fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../converter-core/tests/fixtures/sample.docx"),
        &docx,
    )
    .unwrap();
    // SAFETY: este test es el único del proceso que toca esa variable.
    unsafe { std::env::set_var(converter_core::LIBREOFFICE_ENV, &soffice) };
    let out = Engine::default()
        .convert_file(
            &docx,
            Format::Pdf,
            &ConvertOptions {
                document_engine: DocumentEngine::LibreOffice,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(fs::read(&out[0]).unwrap().starts_with(b"%PDF-"));
}

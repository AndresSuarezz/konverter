//! Modelo de vista de la paleta: lo que la interfaz dibuja. Lógica pura, sin Tauri, para poder
//! probarla con `cargo test`. La interfaz no ordena ni filtra: pinta lo que `converter-core`
//! ofrece, en el orden en que lo ofrece.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use converter_core::{Engine, Fidelity, Format};
use serde::Serialize;

/// Pétalos de la rueda.
pub const SLOTS: usize = 8;
/// Pétalo inferior derecho (el hueco del boceto): la tuerca de ajustes. Nunca lleva un formato.
pub const SETTINGS_SLOT: usize = 3;
/// Posiciones que se rellenan, en el orden de coherencia: arriba y en sentido horario.
const FILL_ORDER: [usize; SLOTS - 1] = [0, 1, 2, 4, 5, 6, 7];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Petal {
    /// Extensión canónica; la interfaz la devuelve a `convert`.
    pub id: String,
    pub label: String,
    /// La conversión pierde información (transparencia, colores, hojas...).
    pub lossy: bool,
    /// Atajo de teclado (1-7), según el orden de coherencia.
    pub key: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Source {
    pub label: String,
    pub name: String,
    pub count: usize,
}

/// Un componente que hay que descargar antes de poder convertir (hoy, LibreOffice).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Offer {
    pub id: String,
    pub title: String,
    pub detail: String,
}

pub fn libreoffice_offer(reason: &str) -> Offer {
    let size = components::libreoffice_download_size().map_or_else(String::new, |bytes| {
        format!(" (≈ {} MB)", bytes / 1_000_000)
    });
    Offer {
        id: "libreoffice".into(),
        title: "LibreOffice".into(),
        detail: format!("{reason} Se descarga una vez{size}."),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Payload {
    pub source: Option<Source>,
    /// Siempre `SLOTS` posiciones; `None` = vacía.
    pub slots: Vec<Option<Petal>>,
    pub settings_slot: usize,
    /// Mensaje para el centro cuando no hay nada que convertir (o hubo un problema).
    pub notice: Option<String>,
    /// Componente que se ofrece instalar (sustituye a los pétalos).
    pub offer: Option<Offer>,
}

impl Payload {
    pub fn notice(message: impl Into<String>) -> Self {
        Self {
            source: None,
            slots: (0..SLOTS).map(|_| None).collect(),
            settings_slot: SETTINGS_SLOT,
            notice: Some(message.into()),
            offer: None,
        }
    }

    pub fn offer(offer: Offer) -> Self {
        Self {
            offer: Some(offer),
            ..Self::notice(String::new())
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn detect(path: &Path) -> Result<Format, String> {
    let mut head = [0u8; 64];
    let read = File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .map_err(|e| format!("No se puede leer {}: {e}", file_name(path)))?;
    Format::detect(path, &head[..read])
        .ok_or_else(|| format!("{} no es un formato que sepamos convertir", file_name(path)))
}

/// Construye la paleta para los archivos dados. Con varios archivos solo se ofrecen los
/// destinos a los que *todos* se pueden convertir.
pub fn build(files: &[PathBuf], engine: &Engine) -> Payload {
    let Some(first) = files.first() else {
        return Payload::notice("Suelta un archivo aquí");
    };
    let mut formats = Vec::with_capacity(files.len());
    for file in files {
        match detect(file) {
            Ok(format) => formats.push(format),
            Err(message) => return Payload::notice(message),
        }
    }
    let targets: Vec<_> = engine
        .targets(formats[0])
        .into_iter()
        .filter(|t| {
            formats
                .iter()
                .all(|&from| engine.can_convert(from, t.format))
        })
        .collect();
    if targets.is_empty() {
        return Payload::notice("Estos archivos no comparten ninguna conversión");
    }

    let mut slots: Vec<Option<Petal>> = (0..SLOTS).map(|_| None).collect();
    for (rank, (target, &slot)) in targets.iter().zip(&FILL_ORDER).enumerate() {
        slots[slot] = Some(Petal {
            id: target.format.extension().to_string(),
            label: target.format.label().to_string(),
            lossy: target.fidelity == Fidelity::Lossy,
            key: rank + 1,
        });
    }
    let same_format = formats.iter().all(|&f| f == formats[0]);
    Payload {
        source: Some(Source {
            label: if same_format {
                formats[0].label().to_string()
            } else {
                "VARIOS".to_string()
            },
            name: if files.len() == 1 {
                file_name(first)
            } else {
                format!("{} archivos", files.len())
            },
            count: files.len(),
        }),
        slots,
        settings_slot: SETTINGS_SLOT,
        notice: None,
        offer: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];

    fn file(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn ids(payload: &Payload) -> Vec<Option<&str>> {
        payload
            .slots
            .iter()
            .map(|s| s.as_ref().map(|p| p.id.as_str()))
            .collect()
    }

    #[test]
    fn png_fills_clockwise_from_the_top_and_skips_the_reserved_petal() {
        let dir = tempfile::tempdir().unwrap();
        let payload = build(&[file(&dir, "a.png", PNG)], &Engine::default());
        let source = payload.source.as_ref().unwrap();
        assert_eq!(
            (source.label.as_str(), source.name.as_str()),
            ("PNG", "a.png")
        );
        // 6 destinos: WEBP, JPG y PDF primero (los más coherentes), nunca el propio PNG.
        assert_eq!(
            ids(&payload),
            [
                Some("webp"),
                Some("jpg"),
                Some("pdf"),
                None,
                Some("bmp"),
                Some("tiff"),
                Some("gif"),
                None
            ]
        );
        assert_eq!(payload.slots[SETTINGS_SLOT], None);
        assert_eq!(payload.slots.len(), SLOTS);
    }

    #[test]
    fn keys_follow_coherence_rank_and_lossy_targets_are_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let payload = build(&[file(&dir, "a.png", PNG)], &Engine::default());
        let jpg = payload.slots[1].as_ref().unwrap();
        assert_eq!((jpg.key, jpg.lossy), (2, true));
        assert_eq!(payload.slots[0].as_ref().unwrap().key, 1);
    }

    #[test]
    fn a_pdf_offers_images_and_text() {
        let dir = tempfile::tempdir().unwrap();
        let payload = build(
            &[file(
                &dir,
                "informe.pdf",
                b"%PDF-1.4
%",
            )],
            &Engine::default(),
        );
        assert_eq!(payload.source.as_ref().unwrap().label, "PDF");
        // PDF → Word es de lo más pedido: va arriba.
        assert_eq!(
            ids(&payload),
            [
                Some("docx"),
                Some("png"),
                Some("jpg"),
                None,
                Some("md"),
                Some("txt"),
                Some("webp"),
                Some("tiff")
            ]
        );
    }

    #[test]
    fn a_word_document_offers_pdf_and_text() {
        let dir = tempfile::tempdir().unwrap();
        let docx = file(&dir, "contrato.docx", b"PK\x03\x04resto");
        let payload = build(&[docx], &Engine::default());
        assert_eq!(payload.source.as_ref().unwrap().label, "DOCX");
        let offered: Vec<_> = ids(&payload).into_iter().flatten().collect();
        assert_eq!(offered, ["pdf", "txt"]);

        let old = file(
            &dir,
            "viejo.doc",
            &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0],
        );
        let payload = build(&[old], &Engine::default());
        assert_eq!(payload.source.as_ref().unwrap().label, "DOC");
        let offered: Vec<_> = ids(&payload).into_iter().flatten().collect();
        assert_eq!(offered, ["pdf", "docx"]);
    }

    #[test]
    fn a_png_and_a_pdf_together_offer_only_what_both_can_become() {
        let dir = tempfile::tempdir().unwrap();
        let payload = build(
            &[
                file(&dir, "a.png", PNG),
                file(
                    &dir,
                    "b.pdf",
                    b"%PDF-1.4
%",
                ),
            ],
            &Engine::default(),
        );
        assert_eq!(payload.source.as_ref().unwrap().label, "VARIOS");
        let offered: Vec<_> = ids(&payload).into_iter().flatten().collect();
        assert_eq!(offered, ["webp", "jpg", "tiff"]);
    }

    #[test]
    fn several_files_offer_only_common_targets() {
        let dir = tempfile::tempdir().unwrap();
        let payload = build(
            &[file(&dir, "a.png", PNG), file(&dir, "b.png", PNG)],
            &Engine::default(),
        );
        assert_eq!(payload.source.unwrap().name, "2 archivos");
    }

    #[test]
    fn mixed_families_have_no_common_conversion() {
        let dir = tempfile::tempdir().unwrap();
        let payload = build(
            &[file(&dir, "a.png", PNG), file(&dir, "b.csv", b"a,b\n1,2\n")],
            &Engine::default(),
        );
        assert!(payload.source.is_none());
        assert!(payload.notice.unwrap().contains("ninguna conversión"));
    }

    #[test]
    fn unknown_and_missing_files_become_notices_not_panics() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::default();
        let unknown = build(&[file(&dir, "notas.xyz", b"hola")], &engine);
        assert!(unknown.notice.unwrap().contains("notas.xyz"));
        let missing = build(&[dir.path().join("no_existe.png")], &engine);
        assert!(missing.notice.unwrap().contains("no_existe.png"));
    }

    #[test]
    fn no_files_asks_to_drop_one() {
        let payload = build(&[], &Engine::default());
        assert!(payload.source.is_none());
        assert_eq!(payload.settings_slot, SETTINGS_SLOT);
    }
}

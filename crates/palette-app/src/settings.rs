//! Ajustes del usuario (la tuerca de la paleta). Se guardan en `settings.json`, junto a la carpeta
//! de componentes (`%LOCALAPPDATA%\Converter\settings.json` en Windows).
//!
//! Son solo los ajustes que existen de verdad hoy: motor de documentos, calidades, resolución y
//! papel. Cualquier problema con el archivo (falta, está corrupto o trae valores absurdos) se
//! resuelve volviendo a los valores por defecto: unos ajustes rotos nunca deben impedir convertir.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use converter_core::{ConvertOptions, DocumentEngine, PdfPaper};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineChoice {
    /// WordCraft si sabe hacerlo (rápido, viene con la aplicación); si no, LibreOffice y Word.
    #[default]
    Auto,
    Wordcraft,
    Libreoffice,
    Word,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaperChoice {
    #[default]
    A4,
    Letter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub document_engine: EngineChoice,
    pub jpeg_quality: u8,
    pub webp_quality: u8,
    /// Resolución al convertir páginas de PDF en imágenes.
    pub pdf_dpi: u32,
    pub pdf_paper: PaperChoice,
}

impl Default for Settings {
    fn default() -> Self {
        let defaults = ConvertOptions::default();
        Self {
            document_engine: EngineChoice::Auto,
            jpeg_quality: defaults.jpeg_quality,
            webp_quality: defaults.webp_quality,
            pdf_dpi: defaults.pdf_dpi,
            pdf_paper: PaperChoice::A4,
        }
    }
}

impl Settings {
    /// Acota los valores a rangos razonables (la interfaz ya lo hace; esto cubre un archivo editado
    /// a mano o una versión futura).
    pub fn sanitized(mut self) -> Self {
        self.jpeg_quality = self.jpeg_quality.clamp(30, 100);
        self.webp_quality = self.webp_quality.clamp(30, 100);
        self.pdf_dpi = self.pdf_dpi.clamp(72, 600);
        self
    }

    pub fn to_options(&self) -> ConvertOptions {
        ConvertOptions {
            jpeg_quality: self.jpeg_quality,
            webp_quality: self.webp_quality,
            pdf_dpi: self.pdf_dpi,
            pdf_paper: match self.pdf_paper {
                PaperChoice::A4 => PdfPaper::A4,
                PaperChoice::Letter => PdfPaper::Letter,
            },
            document_engine: match self.document_engine {
                EngineChoice::Auto => DocumentEngine::Auto,
                EngineChoice::Wordcraft => DocumentEngine::WordCraft,
                EngineChoice::Libreoffice => DocumentEngine::LibreOffice,
                EngineChoice::Word => DocumentEngine::Word,
            },
            ..ConvertOptions::default()
        }
    }
}

pub fn settings_path() -> PathBuf {
    converter_core::components_dir().with_file_name("settings.json")
}

/// Lee los ajustes; si no se puede, devuelve los de por defecto.
pub fn load_from(path: &Path) -> Settings {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
        .map_or_else(Settings::default, Settings::sanitized)
}

/// Guarda de forma atómica (archivo temporal + renombrado).
pub fn save_to(path: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    let mut file = fs::File::create(&temp)?;
    file.write_all(&serde_json::to_vec_pretty(settings).map_err(io::Error::other)?)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("Converter").join("settings.json")
    }

    #[test]
    fn missing_and_corrupt_files_fall_back_to_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_from(&file(&dir)), Settings::default());
        fs::create_dir_all(file(&dir).parent().unwrap()).unwrap();
        fs::write(file(&dir), b"{ esto no es json").unwrap();
        assert_eq!(load_from(&file(&dir)), Settings::default());
        fs::write(
            file(&dir),
            br#"{"document_engine":"un-motor-que-no-existe"}"#,
        )
        .unwrap();
        assert_eq!(load_from(&file(&dir)), Settings::default());
    }

    #[test]
    fn saved_settings_survive_a_restart_and_leave_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let wanted = Settings {
            document_engine: EngineChoice::Libreoffice,
            jpeg_quality: 75,
            webp_quality: 60,
            pdf_dpi: 300,
            pdf_paper: PaperChoice::Letter,
        };
        save_to(&file(&dir), &wanted).unwrap();
        assert_eq!(load_from(&file(&dir)), wanted);
        let names: Vec<_> = fs::read_dir(file(&dir).parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["settings.json"]);
    }

    #[test]
    fn a_file_with_only_some_fields_keeps_the_defaults_for_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(file(&dir).parent().unwrap()).unwrap();
        fs::write(file(&dir), br#"{"pdf_paper":"letter"}"#).unwrap();
        let loaded = load_from(&file(&dir));
        assert_eq!(loaded.pdf_paper, PaperChoice::Letter);
        assert_eq!(loaded.jpeg_quality, Settings::default().jpeg_quality);
    }

    #[test]
    fn absurd_values_are_clamped() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(file(&dir).parent().unwrap()).unwrap();
        fs::write(
            file(&dir),
            br#"{"jpeg_quality":0,"webp_quality":255,"pdf_dpi":999999}"#,
        )
        .unwrap();
        let loaded = load_from(&file(&dir));
        assert_eq!(
            (loaded.jpeg_quality, loaded.webp_quality, loaded.pdf_dpi),
            (30, 100, 600)
        );
    }

    #[test]
    fn settings_become_conversion_options() {
        let options = Settings {
            document_engine: EngineChoice::Word,
            jpeg_quality: 70,
            webp_quality: 65,
            pdf_dpi: 200,
            pdf_paper: PaperChoice::Letter,
        }
        .to_options();
        assert_eq!(options.document_engine, DocumentEngine::Word);
        assert_eq!(
            (options.jpeg_quality, options.webp_quality, options.pdf_dpi),
            (70, 65, 200)
        );
        assert_eq!(options.pdf_paper, PdfPaper::Letter);
    }

    #[test]
    fn the_wordcraft_choice_is_saved_by_name_and_maps_to_the_engine() {
        assert_eq!(
            serde_json::to_string(&EngineChoice::Wordcraft).unwrap(),
            "\"wordcraft\""
        );
        let loaded: Settings = serde_json::from_str(r#"{"document_engine":"wordcraft"}"#).unwrap();
        assert_eq!(
            loaded.to_options().document_engine,
            DocumentEngine::WordCraft
        );
        // Un archivo de ajustes de una versión anterior (sin esta opción) sigue siendo válido.
        let old: Settings = serde_json::from_str(r#"{"document_engine":"libreoffice"}"#).unwrap();
        assert_eq!(old.document_engine, EngineChoice::Libreoffice);
    }

    #[test]
    fn the_defaults_match_the_engine_defaults() {
        let options = Settings::default().to_options();
        let engine = ConvertOptions::default();
        assert_eq!(options.jpeg_quality, engine.jpeg_quality);
        assert_eq!(options.pdf_paper, engine.pdf_paper);
        assert_eq!(options.document_engine, engine.document_engine);
    }
}

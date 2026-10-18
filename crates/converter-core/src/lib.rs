//! Motor de conversión de archivos. Biblioteca pura: sin interfaz, sin red y sin
//! dependencias de ningún sistema operativo.
//!
//! ```no_run
//! use converter_core::{ConvertOptions, Engine, Format};
//!
//! let engine = Engine::default();
//! // Pétalos de la paleta para un JPG, ya ordenados por coherencia.
//! let targets = engine.targets(Format::Jpeg);
//! assert_eq!(targets[0].format, Format::Webp);
//! // Devuelve las rutas escritas: una, o una por página si el origen es un PDF.
//! let written = engine
//!     .convert_file("foto.jpg".as_ref(), Format::Webp, &ConvertOptions::default())
//!     .unwrap();
//! println!("{}", written[0].display());
//! ```

mod convert;
mod docx_text;
mod engine;
mod error;
mod format;
mod image_conv;
mod markdown;
mod md_docx;
mod office;
mod output;
mod pdf;
mod pdf_markdown;
mod pdf_write;
mod ranking;
mod tabular;

pub use convert::{ConvertOptions, Converter, DocumentEngine, Fidelity, Page, PdfPaper, WebpMode};
pub use engine::{Engine, Target};
pub use error::{Error, Result};
pub use format::{Family, Format};
pub use office::{
    COMPONENTS_DIR_ENV, DocumentEngines, LIBREOFFICE_ENV, WORDCRAFT_ENV, components_dir,
    document_engines, find_libreoffice, find_wordcraft, libreoffice_install_hint,
};
pub use pdf::{PDFIUM_DIR_ENV, default_pdfium_dirs};
pub use ranking::popularity;

use std::path::PathBuf;

use crate::format::Format;

/// Errores del motor de conversión. Los mensajes están pensados para mostrarse al usuario.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no se pudo acceder a {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("no se reconoce el formato de {}", .0.display())]
    UnknownFormat(PathBuf),

    #[error("no hay conversión de {from} a {to}")]
    Unsupported { from: Format, to: Format },

    #[error("el archivo {format} está vacío")]
    Empty { format: Format },

    #[error("no se pudo leer el archivo como {format}: {message}")]
    Decode { format: Format, message: String },

    #[error("no se pudo generar el archivo {format}: {message}")]
    Encode { format: Format, message: String },

    /// Falta un componente externo (p. ej. `pdfium`). La interfaz puede ofrecer instalarlo.
    #[error("falta {name}: {detail}")]
    MissingDependency { name: &'static str, detail: String },

    /// Un programa externo (Word, LibreOffice) falló, no respondió o no generó la salida.
    #[error("{tool} no pudo convertir el archivo: {message}")]
    External { tool: &'static str, message: String },

    /// El archivo no contiene texto utilizable (p. ej. un PDF escaneado): haría falta OCR.
    #[error("el archivo {format} no contiene texto seleccionable; haría falta OCR")]
    NeedsOcr { format: Format },
}

pub type Result<T> = std::result::Result<T, Error>;

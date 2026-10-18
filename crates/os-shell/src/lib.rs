//! Integración con el gestor de archivos del sistema operativo.
//!
//! Por ahora solo Windows (Explorador y Escritorio). En otros sistemas `selected_files`
//! devuelve [`ShellError::Unsupported`]; macOS (Apple Events) y Linux (portales y acciones del
//! gestor de archivos) irán como módulos hermanos sin tocar a los llamadores.

use std::path::PathBuf;

#[cfg(windows)]
mod windows_impl;

#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("la ventana activa no es el Explorador de archivos ni el Escritorio")]
    NotAFileManager,
    #[error("no se pudo leer la selección del Explorador: {0}")]
    Platform(String),
    #[error("leer la selección del gestor de archivos aún no está soportado en este sistema")]
    Unsupported,
}

/// Archivos seleccionados en la ventana activa del gestor de archivos (o en el Escritorio).
///
/// Devuelve una lista vacía si la ventana es el Explorador pero no hay nada seleccionado.
/// Solo incluye elementos con ruta real del sistema de archivos (se descartan "Este equipo",
/// la Papelera, etc.). Debe llamarse desde un hilo cualquiera; inicializa COM por su cuenta.
pub fn selected_files() -> Result<Vec<PathBuf>, ShellError> {
    #[cfg(windows)]
    {
        windows_impl::selected_files_in_foreground()
    }
    #[cfg(not(windows))]
    {
        Err(ShellError::Unsupported)
    }
}

/// Como [`selected_files`], pero para una ventana concreta del Explorador (identificador `HWND`
/// como entero). Existe para poder probar la lectura sin depender de la ventana en primer plano.
#[cfg(windows)]
pub fn selected_files_in_window(hwnd: isize) -> Result<Vec<PathBuf>, ShellError> {
    windows_impl::selected_files_in_window(hwnd)
}

/// Selección actual del Escritorio, esté o no en primer plano. Existe para poder probar la
/// consulta COM del Escritorio sin depender del foco.
#[cfg(windows)]
pub fn selected_files_on_desktop() -> Result<Vec<PathBuf>, ShellError> {
    windows_impl::selected_files_on_desktop()
}

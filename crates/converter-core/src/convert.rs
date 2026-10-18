use crate::error::Result;
use crate::format::Format;

/// Si una conversión conserva toda la información del original.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fidelity {
    Lossless,
    /// Se pierde algo (transparencia, colores, hojas, formato de celdas...). La interfaz puede avisarlo.
    Lossy,
}

/// Cómo se escribe WEBP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WebpMode {
    /// Con pérdida si el origen ya la tenía (JPG), sin pérdida en el resto.
    #[default]
    Auto,
    Lossy,
    Lossless,
}

/// Programa que convierte los documentos de Office.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DocumentEngine {
    /// El mejor disponible **para cada par**: WordCraft (el incluido con la aplicación) si sabe
    /// hacerlo; si no, LibreOffice y, por último, Word (Windows).
    #[default]
    Auto,
    /// WordCraft: rápido y ligero (Rust puro), hace DOCX/DOC/Markdown → PDF y DOC → DOCX. Para lo
    /// que no sabe (PDF → DOCX) se usa el motor que haya, como en `Auto`.
    WordCraft,
    Word,
    LibreOffice,
}

/// Papel de los PDF que se crean a partir de imágenes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PdfPaper {
    #[default]
    A4,
    /// Carta (Letter): el papel habitual en Colombia, México, Estados Unidos y Canadá.
    Letter,
}

impl PdfPaper {
    /// Ancho y alto en puntos (1 pt = 1/72 in), en vertical.
    pub fn size(self) -> (f32, f32) {
        match self {
            PdfPaper::A4 => (595.28, 841.89),
            PdfPaper::Letter => (612.0, 792.0),
        }
    }
}

/// Ajustes con valores por defecto razonables; la paleta no los pregunta.
#[derive(Debug, Clone)]
pub struct ConvertOptions {
    /// Calidad de JPG, de 1 a 100.
    pub jpeg_quality: u8,
    /// Calidad de WEBP con pérdida, de 1 a 100.
    pub webp_quality: u8,
    pub webp_mode: WebpMode,
    /// Separador al escribir CSV.
    pub csv_delimiter: u8,
    /// Escribir CSV con BOM UTF-8 para que Excel muestre bien los acentos.
    pub csv_bom: bool,
    /// Resolución al convertir páginas de PDF en imágenes.
    pub pdf_dpi: u32,
    /// Qué programa convierte DOCX/DOC ↔ PDF.
    pub document_engine: DocumentEngine,
    /// Papel al crear un PDF desde una imagen.
    pub pdf_paper: PdfPaper,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            jpeg_quality: 90,
            webp_quality: 80,
            webp_mode: WebpMode::Auto,
            csv_delimiter: b',',
            csv_bom: true,
            pdf_dpi: 150,
            document_engine: DocumentEngine::Auto,
            pdf_paper: PdfPaper::A4,
        }
    }
}

/// Una salida de una conversión. La mayoría de conversiones producen una sola; un PDF de varias
/// páginas convertido a imagen produce una por página.
#[derive(Debug)]
pub struct Page {
    pub bytes: Vec<u8>,
    /// Posición de esta salida, desde 0.
    pub index: usize,
    /// Cuántas salidas habrá en total (conocido antes de entregar la primera).
    pub total: usize,
}

/// Un conversor declara qué pares soporta y los ejecuta sobre bytes en memoria.
///
/// Trabajar con bytes (y no con rutas) mantiene a los conversores puros y fáciles de probar;
/// la lectura, la escritura atómica y el nombre de salida los resuelve el [`Engine`](crate::Engine).
pub trait Converter: Send + Sync {
    /// `Some` si este conversor sabe pasar de `from` a `to`.
    fn supports(&self, from: Format, to: Format) -> Option<Fidelity>;

    /// Convierte y devuelve **una** salida. Si el conversor produce varias (PDF → imagen),
    /// devuelve la primera; usa [`Converter::convert_pages`] para todas.
    fn convert(
        &self,
        input: &[u8],
        from: Format,
        to: Format,
        options: &ConvertOptions,
    ) -> Result<Vec<u8>>;

    /// Convierte y entrega cada salida a `sink` en cuanto está lista, sin acumularlas en memoria.
    /// Los conversores de una sola salida no necesitan sobrescribirlo.
    fn convert_pages(
        &self,
        input: &[u8],
        from: Format,
        to: Format,
        options: &ConvertOptions,
        sink: &mut dyn FnMut(Page) -> Result<()>,
    ) -> Result<()> {
        sink(Page {
            bytes: self.convert(input, from, to, options)?,
            index: 0,
            total: 1,
        })
    }
}

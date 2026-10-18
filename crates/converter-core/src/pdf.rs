//! PDF: imágenes → PDF (Rust puro) y PDF → imágenes / texto (con `pdfium`, biblioteca nativa
//! que se carga en tiempo de ejecución desde un archivo `pdfium.dll` / `libpdfium.so` /
//! `libpdfium.dylib`; si no está, se devuelve [`Error::MissingDependency`]).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use image::{ColorType, DynamicImage};
use pdfium_render::prelude::*;

use crate::convert::{ConvertOptions, Converter, Fidelity, Page, PdfPaper};
use crate::error::{Error, Result};
use crate::format::{Family, Format};
use crate::image_conv::{decode_image, encode_image, probe};
use crate::pdf_markdown;
use crate::pdf_write::{PdfImage, build_pdf};

/// Lado máximo de una página rasterizada. Evita que un PDF con una página gigantesca (planos,
/// pósters) agote la memoria.
const MAX_PIXELS: f32 = 10_000.0;

/// Variable de entorno para indicar la carpeta de pdfium (sobrescribe las demás ubicaciones).
pub const PDFIUM_DIR_ENV: &str = "CONVERTER_PDFIUM_DIR";

/// Conversiones de y hacia PDF.
///
/// Imágenes → PDF: una página (A4 o carta, según `ConvertOptions::pdf_paper`) por imagen; los JPG se incrustan sin recomprimir. PDF →
/// imagen: una imagen por página a `ConvertOptions::pdf_dpi`. PDF → TXT: el texto
/// seleccionable (un escaneo no tiene; devuelve [`Error::NeedsOcr`]).
pub struct PdfConverter {
    pdfium_dirs: Vec<PathBuf>,
}

impl Default for PdfConverter {
    fn default() -> Self {
        Self::with_pdfium_dirs(default_pdfium_dirs())
    }
}

impl PdfConverter {
    /// Carpetas donde buscar la biblioteca de pdfium, por orden de prioridad.
    pub fn with_pdfium_dirs(pdfium_dirs: Vec<PathBuf>) -> Self {
        Self { pdfium_dirs }
    }
}

/// Dónde se busca pdfium por defecto: `CONVERTER_PDFIUM_DIR`; las carpetas donde lo deja cada
/// paquete (ver [`packaged_pdfium_dirs`]); y, solo en compilaciones de depuración,
/// `vendor/bin/pdfium` del repositorio (lo dejan `scripts/fetch-pdfium.ps1` y `.sh`).
pub fn default_pdfium_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os(PDFIUM_DIR_ENV) {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
    {
        dirs.extend(packaged_pdfium_dirs(&exe_dir));
    }
    #[cfg(debug_assertions)]
    dirs.push(PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/bin/pdfium"
    )));
    dirs
}

/// Dónde deja cada paquete la biblioteca de pdfium, relativo a la carpeta del ejecutable:
/// - `pdfium/`: Windows (instalador NSIS) y, también, la carpeta de un árbol de desarrollo;
/// - `../Resources/pdfium`: macOS (`Contents/MacOS` → `Contents/Resources`);
/// - `../lib/Converter/pdfium`: Linux (`.deb`, `.rpm`, AppImage: `bin/` → `lib/<productName>/`);
/// - el propio directorio del ejecutable.
fn packaged_pdfium_dirs(exe_dir: &Path) -> Vec<PathBuf> {
    vec![
        exe_dir.join("pdfium"),
        exe_dir.join("../Resources/pdfium"),
        exe_dir.join("../lib/Converter/pdfium"),
        exe_dir.to_path_buf(),
    ]
}

fn is_raster_target(format: Format) -> bool {
    matches!(
        format,
        Format::Png | Format::Jpeg | Format::Webp | Format::Tiff
    )
}

impl Converter for PdfConverter {
    fn supports(&self, from: Format, to: Format) -> Option<Fidelity> {
        match (from, to) {
            (from, Format::Pdf) if from.family() == Family::Image => Some(Fidelity::Lossless),
            (Format::Pdf, Format::Jpeg | Format::Txt | Format::Markdown) => Some(Fidelity::Lossy),
            (Format::Pdf, to) if is_raster_target(to) => Some(Fidelity::Lossless),
            _ => None,
        }
    }

    fn convert(
        &self,
        input: &[u8],
        from: Format,
        to: Format,
        options: &ConvertOptions,
    ) -> Result<Vec<u8>> {
        let mut first = None;
        self.convert_pages(input, from, to, options, &mut |page| {
            first.get_or_insert(page.bytes);
            Ok(())
        })?;
        first.ok_or(Error::Decode {
            format: from,
            message: "no se obtuvo ninguna salida".into(),
        })
    }

    fn convert_pages(
        &self,
        input: &[u8],
        from: Format,
        to: Format,
        options: &ConvertOptions,
        sink: &mut dyn FnMut(Page) -> Result<()>,
    ) -> Result<()> {
        match (from, to) {
            (from, Format::Pdf) if from.family() == Family::Image => sink(Page {
                bytes: image_to_pdf(input, from, options.pdf_paper)?,
                index: 0,
                total: 1,
            }),
            (Format::Pdf, Format::Txt) => sink(Page {
                bytes: pdf_to_text(&self.pdfium_dirs, input)?,
                index: 0,
                total: 1,
            }),
            (Format::Pdf, Format::Markdown) => sink(Page {
                bytes: pdf_to_markdown(&self.pdfium_dirs, input)?,
                index: 0,
                total: 1,
            }),
            (Format::Pdf, to) if is_raster_target(to) => {
                pdf_to_images(&self.pdfium_dirs, input, to, options, sink)
            }
            _ => Err(Error::Unsupported { from, to }),
        }
    }
}

// ------------------------------------------------------------ imagen → PDF

fn image_to_pdf(input: &[u8], from: Format, paper: PdfPaper) -> Result<Vec<u8>> {
    // Se decodifica siempre: valida que el archivo no está corrupto y aplica la orientación.
    let decoded = decode_image(input, from)?;
    let encode_error = |e: std::io::Error| Error::Encode {
        format: Format::Pdf,
        message: e.to_string(),
    };
    // Un JPG normal (RGB o gris, sin giro EXIF pendiente) se incrusta tal cual.
    if from == Format::Jpeg
        && let Ok(info) = probe(input, from)
        && info.oriented
        && matches!(info.color, ColorType::Rgb8 | ColorType::L8)
    {
        let image = PdfImage::jpeg(
            input.to_vec(),
            info.width,
            info.height,
            info.color == ColorType::L8,
        );
        return Ok(build_pdf(&[image], paper));
    }
    let image = PdfImage::from_decoded(&decoded).map_err(encode_error)?;
    Ok(build_pdf(&[image], paper))
}

// ------------------------------------------------------------------ pdfium

/// pdfium solo puede inicializarse una vez por proceso: una sola instancia compartida.
static PDFIUM: OnceLock<Pdfium> = OnceLock::new();
static PDFIUM_INIT: Mutex<()> = Mutex::new(());

fn pdfium(dirs: &[PathBuf]) -> Result<&'static Pdfium> {
    if let Some(pdfium) = PDFIUM.get() {
        return Ok(pdfium);
    }
    let _guard = PDFIUM_INIT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(pdfium) = PDFIUM.get() {
        return Ok(pdfium);
    }
    let bindings = dirs
        .iter()
        .find_map(|dir| {
            Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(dir)).ok()
        })
        .or_else(|| Pdfium::bind_to_system_library().ok())
        .ok_or_else(|| Error::MissingDependency {
            name: "pdfium",
            detail: format!(
                "no se encontró la biblioteca (buscada en {}). Ejecuta scripts/fetch-pdfium.ps1 \
                 (Windows) o scripts/fetch-pdfium.sh (Linux y macOS), o define {PDFIUM_DIR_ENV}",
                dirs.iter()
                    .map(|d| d.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        })?;
    Ok(PDFIUM.get_or_init(|| Pdfium::new(bindings)))
}

fn decode_error(message: impl ToString) -> Error {
    Error::Decode {
        format: Format::Pdf,
        message: message.to_string(),
    }
}

fn load<'a>(pdfium: &'a Pdfium, input: &'a [u8]) -> Result<PdfDocument<'a>> {
    pdfium.load_pdf_from_byte_slice(input, None).map_err(|e| {
        let text = format!("{e:?}");
        if text.contains("Password") {
            decode_error("el PDF está protegido con contraseña")
        } else {
            decode_error(e)
        }
    })
}

fn pdf_to_images(
    dirs: &[PathBuf],
    input: &[u8],
    to: Format,
    options: &ConvertOptions,
    sink: &mut dyn FnMut(Page) -> Result<()>,
) -> Result<()> {
    let pdfium = pdfium(dirs)?;
    let document = load(pdfium, input)?;
    let total = usize::try_from(document.pages().len()).unwrap_or(0);
    if total == 0 {
        return Err(decode_error("el PDF no tiene páginas"));
    }
    for (index, page) in document.pages().iter().enumerate() {
        let image = render(&page, options.pdf_dpi)?;
        sink(Page {
            bytes: encode_image(&image, Format::Pdf, to, options)?,
            index,
            total,
        })?;
    }
    Ok(())
}

fn render(page: &PdfPage, dpi: u32) -> Result<DynamicImage> {
    let scale = dpi.clamp(36, 600) as f32 / 72.0;
    let (mut width, mut height) = (page.width().value * scale, page.height().value * scale);
    let longest = width.max(height);
    if longest > MAX_PIXELS {
        let shrink = MAX_PIXELS / longest;
        (width, height) = (width * shrink, height * shrink);
    }
    let config = PdfRenderConfig::new()
        .set_target_size(
            width.round().max(1.0) as i32,
            height.round().max(1.0) as i32,
        )
        .render_form_data(true);
    page.render_with_config(&config)
        .and_then(|bitmap| bitmap.as_image())
        .map_err(decode_error)
}

fn pdf_to_markdown(dirs: &[PathBuf], input: &[u8]) -> Result<Vec<u8>> {
    let pdfium = pdfium(dirs)?;
    let document = load(pdfium, input)?;
    let mut pages = Vec::new();
    for page in document.pages().iter() {
        pages.push(pdf_markdown::read_page(&page).map_err(decode_error)?);
    }
    let markdown = pdf_markdown::to_markdown(&pages);
    if markdown.trim().is_empty() {
        return Err(Error::NeedsOcr {
            format: Format::Pdf,
        });
    }
    Ok(markdown.into_bytes())
}

fn pdf_to_text(dirs: &[PathBuf], input: &[u8]) -> Result<Vec<u8>> {
    let pdfium = pdfium(dirs)?;
    let document = load(pdfium, input)?;
    let mut text = String::new();
    for page in document.pages().iter() {
        let page_text = page.text().map_err(decode_error)?.all();
        if !page_text.trim().is_empty() {
            if !text.is_empty() {
                text.push_str("\r\n\r\n");
            }
            text.push_str(page_text.trim_end());
        }
    }
    if text.is_empty() {
        return Err(Error::NeedsOcr {
            format: Format::Pdf,
        });
    }
    // UTF-8 con BOM: el Bloc de notas de Windows lo usa para reconocer la codificación.
    let mut out = b"\xEF\xBB\xBF".to_vec();
    out.extend_from_slice(text.as_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_for_pdfium_where_each_package_leaves_it() {
        let dirs = packaged_pdfium_dirs(Path::new("/opt/app/bin"));
        // Windows, macOS (.app) y Linux (.deb/.rpm/AppImage), medidos con los paquetes reales.
        for relative in ["pdfium", "../Resources/pdfium", "../lib/Converter/pdfium"] {
            assert!(
                dirs.contains(&Path::new("/opt/app/bin").join(relative)),
                "falta {relative}"
            );
        }
        assert!(dirs.contains(&PathBuf::from("/opt/app/bin")));
    }

    #[test]
    fn declares_exactly_the_supported_pairs() {
        let converter = PdfConverter::with_pdfium_dirs(Vec::new());
        for image in [
            Format::Jpeg,
            Format::Png,
            Format::Webp,
            Format::Bmp,
            Format::Tiff,
            Format::Gif,
        ] {
            assert_eq!(
                converter.supports(image, Format::Pdf),
                Some(Fidelity::Lossless)
            );
        }
        assert_eq!(
            converter.supports(Format::Pdf, Format::Png),
            Some(Fidelity::Lossless)
        );
        assert_eq!(
            converter.supports(Format::Pdf, Format::Jpeg),
            Some(Fidelity::Lossy)
        );
        assert_eq!(
            converter.supports(Format::Pdf, Format::Txt),
            Some(Fidelity::Lossy)
        );
        assert_eq!(converter.supports(Format::Pdf, Format::Gif), None);
        assert_eq!(converter.supports(Format::Pdf, Format::Pdf), None);
        assert_eq!(converter.supports(Format::Csv, Format::Pdf), None);
        assert_eq!(converter.supports(Format::Txt, Format::Pdf), None);
    }

    #[test]
    fn missing_pdfium_is_a_clear_missing_dependency_error() {
        // Solo si este proceso aún no cargó pdfium (otro test podría haberlo hecho).
        if PDFIUM.get().is_some() {
            return;
        }
        let converter = PdfConverter::with_pdfium_dirs(vec![PathBuf::from("no/existe")]);
        let err = converter
            .convert(
                b"%PDF-1.4",
                Format::Pdf,
                Format::Png,
                &ConvertOptions::default(),
            )
            .unwrap_err();
        // En esta máquina no hay pdfium en el sistema; si lo hubiera, fallaría al leer el PDF.
        assert!(
            matches!(
                err,
                Error::MissingDependency { name: "pdfium", .. } | Error::Decode { .. }
            ),
            "{err}"
        );
    }
}

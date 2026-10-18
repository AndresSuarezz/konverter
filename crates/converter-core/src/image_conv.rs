use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageDecoder, ImageFormat, Rgb, RgbImage};

use crate::convert::{ConvertOptions, Converter, Fidelity, WebpMode};
use crate::error::{Error, Result};
use crate::format::{Family, Format};

/// Límite del formato WEBP por lado.
const WEBP_MAX_DIMENSION: u32 = 16_383;

/// Conversiones entre formatos de imagen rasterizada.
///
/// Limitaciones conocidas: de GIF/WEBP animados y de TIFF multipágina solo se convierte el
/// primer fotograma/página. WEBP se escribe con libwebp (código C enlazado estáticamente);
/// se lee con el decodificador en Rust puro de `image`.
pub struct ImageConverter;

fn image_format(format: Format) -> Option<ImageFormat> {
    match format {
        Format::Jpeg => Some(ImageFormat::Jpeg),
        Format::Png => Some(ImageFormat::Png),
        Format::Webp => Some(ImageFormat::WebP),
        Format::Bmp => Some(ImageFormat::Bmp),
        Format::Tiff => Some(ImageFormat::Tiff),
        Format::Gif => Some(ImageFormat::Gif),
        Format::Csv
        | Format::Xlsx
        | Format::Json
        | Format::Pdf
        | Format::Txt
        | Format::Docx
        | Format::Doc
        | Format::Markdown => None,
    }
}

impl Converter for ImageConverter {
    fn supports(&self, from: Format, to: Format) -> Option<Fidelity> {
        if from == to || from.family() != Family::Image || to.family() != Family::Image {
            return None;
        }
        Some(match (from, to) {
            // JPG no tiene transparencia y GIF se limita a 256 colores; desde JPG, WEBP se
            // recodifica con pérdida en modo automático.
            (_, Format::Jpeg | Format::Gif) | (Format::Jpeg, Format::Webp) => Fidelity::Lossy,
            _ => Fidelity::Lossless,
        })
    }

    fn convert(
        &self,
        input: &[u8],
        from: Format,
        to: Format,
        options: &ConvertOptions,
    ) -> Result<Vec<u8>> {
        let image = decode_image(input, from)?;
        encode_image(&image, from, to, options)
    }
}

/// Lee una imagen aplicando su orientación EXIF: las fotos de móvil se guardan "tumbadas" con
/// una etiqueta que dice cómo girarlas, y como los formatos de salida no la conservan, sin
/// este paso saldrían rotadas.
pub(crate) fn decode_image(input: &[u8], from: Format) -> Result<DynamicImage> {
    let source = image_format(from).ok_or(Error::Unsupported { from, to: from })?;
    let fail = |e: image::ImageError| Error::Decode {
        format: from,
        message: e.to_string(),
    };
    let mut decoder = image::ImageReader::with_format(Cursor::new(input), source)
        .into_decoder()
        .map_err(fail)?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).map_err(fail)?;
    // El decodificador de JPG es tolerante y devuelve una imagen parcial si el archivo está
    // cortado; para un conversor eso es ocultar un archivo dañado.
    if from == Format::Jpeg && !jpeg_is_complete(input) {
        return Err(Error::Decode {
            format: from,
            message: "el JPG está incompleto o dañado (le falta el final del archivo)".into(),
        });
    }
    image.apply_orientation(orientation);
    Ok(image)
}

/// `true` si el JPG llega hasta su marcador de fin (EOI). Recorre los segmentos en vez de buscar
/// `FF D9` a ciegas: la miniatura EXIF lleva el suyo, y hay fotos con datos añadidos tras el
/// final (p. ej. un vídeo incrustado), que no deben contar como "incompleto".
fn jpeg_is_complete(data: &[u8]) -> bool {
    if !data.starts_with(&[0xFF, 0xD8]) {
        return false;
    }
    let mut i = 2;
    while i + 1 < data.len() {
        if data[i] != 0xFF {
            return false;
        }
        match data[i + 1] {
            0xFF => i += 1, // relleno
            0xD9 => return true,
            0x01 | 0xD0..=0xD8 => i += 2, // marcadores sin longitud
            marker => {
                let Some(length) = data
                    .get(i + 2..i + 4)
                    .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
                else {
                    return false;
                };
                i += 2 + length;
                if marker == 0xDA {
                    // Datos entrelazados: un 0xFF va seguido de 00 (relleno) o de un RSTn; lo
                    // demás es el siguiente marcador.
                    while i + 1 < data.len()
                        && !(data[i] == 0xFF
                            && data[i + 1] != 0x00
                            && !(0xD0..=0xD7).contains(&data[i + 1]))
                    {
                        i += 1;
                    }
                }
            }
        }
    }
    false
}

/// Orientación EXIF y tipo de color sin decodificar los píxeles (barato). Sirve para decidir
/// si un JPG puede incrustarse tal cual en un PDF.
pub(crate) fn probe(input: &[u8], from: Format) -> Result<Probe> {
    let source = image_format(from).ok_or(Error::Unsupported { from, to: from })?;
    let fail = |e: image::ImageError| Error::Decode {
        format: from,
        message: e.to_string(),
    };
    let mut decoder = image::ImageReader::with_format(Cursor::new(input), source)
        .into_decoder()
        .map_err(fail)?;
    Ok(Probe {
        width: decoder.dimensions().0,
        height: decoder.dimensions().1,
        color: decoder.color_type(),
        oriented: decoder
            .orientation()
            .is_ok_and(|o| o == image::metadata::Orientation::NoTransforms),
    })
}

pub(crate) struct Probe {
    pub width: u32,
    pub height: u32,
    pub color: image::ColorType,
    /// `true` si no hay que girar la imagen al mostrarla.
    pub oriented: bool,
}

pub(crate) fn encode_image(
    image: &DynamicImage,
    from: Format,
    to: Format,
    options: &ConvertOptions,
) -> Result<Vec<u8>> {
    let target = image_format(to).ok_or(Error::Unsupported { from, to })?;
    encode(image, from, target, to, options).map_err(|message| Error::Encode {
        format: to,
        message,
    })
}

fn encode(
    image: &DynamicImage,
    from: Format,
    target: ImageFormat,
    to: Format,
    options: &ConvertOptions,
) -> std::result::Result<Vec<u8>, String> {
    let image_error = |e: image::ImageError| e.to_string();
    let mut out = Vec::new();
    match to {
        Format::Jpeg => {
            let rgb = DynamicImage::ImageRgb8(flatten_on_white(image));
            let encoder =
                JpegEncoder::new_with_quality(&mut out, options.jpeg_quality.clamp(1, 100));
            rgb.write_with_encoder(encoder).map_err(image_error)?;
        }
        Format::Webp => return encode_webp(image, webp_is_lossless(from, options), options),
        Format::Bmp | Format::Gif => {
            to_8bit(image)
                .write_to(&mut Cursor::new(&mut out), target)
                .map_err(image_error)?;
        }
        // PNG y TIFF conservan la profundidad de color original (p. ej. TIFF de 16 bits).
        _ => image
            .write_to(&mut Cursor::new(&mut out), target)
            .map_err(image_error)?,
    }
    Ok(out)
}

/// En modo automático, lo que ya viene con pérdida (JPG) se recodifica con pérdida: un WEBP
/// sin pérdida de un JPG suele pesar más que el propio JPG. Lo demás (PNG, BMP, TIFF, GIF)
/// va sin pérdida para no degradar capturas, texto ni gráficos.
fn webp_is_lossless(from: Format, options: &ConvertOptions) -> bool {
    match options.webp_mode {
        WebpMode::Lossless => true,
        WebpMode::Lossy => false,
        WebpMode::Auto => from != Format::Jpeg,
    }
}

/// libwebp (enlazada estáticamente). Usa `encode_simple` y no `encode`, que hace `unwrap`.
fn encode_webp(
    image: &DynamicImage,
    lossless: bool,
    options: &ConvertOptions,
) -> std::result::Result<Vec<u8>, String> {
    let (width, height) = (image.width(), image.height());
    if width > WEBP_MAX_DIMENSION || height > WEBP_MAX_DIMENSION {
        return Err(format!(
            "WEBP admite como máximo {WEBP_MAX_DIMENSION} px por lado y la imagen mide {width}x{height}"
        ));
    }
    // En modo sin pérdida el parámetro es el esfuerzo de compresión, no la calidad.
    let quality = if lossless {
        75.0
    } else {
        f32::from(options.webp_quality.clamp(1, 100))
    };
    let memory = if image.color().has_alpha() {
        let pixels = image.to_rgba8();
        webp::Encoder::from_rgba(pixels.as_raw(), width, height).encode_simple(lossless, quality)
    } else {
        let pixels = image.to_rgb8();
        webp::Encoder::from_rgb(pixels.as_raw(), width, height).encode_simple(lossless, quality)
    }
    .map_err(|e| format!("libwebp: {e:?}"))?;
    Ok(memory.to_vec())
}

/// BMP, GIF y WEBP solo admiten 8 bits por canal.
fn to_8bit(image: &DynamicImage) -> DynamicImage {
    if image.color().has_alpha() {
        DynamicImage::ImageRgba8(image.to_rgba8())
    } else {
        DynamicImage::ImageRgb8(image.to_rgb8())
    }
}

/// Compone la imagen sobre fondo blanco; los píxeles transparentes quedan blancos, no negros.
fn flatten_on_white(image: &DynamicImage) -> RgbImage {
    let rgba = image.to_rgba8();
    let mut out = RgbImage::new(rgba.width(), rgba.height());
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let alpha = u32::from(pixel[3]);
        let blend = |c: u8| ((u32::from(c) * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
        out.put_pixel(
            x,
            y,
            Rgb([blend(pixel[0]), blend(pixel[1]), blend(pixel[2])]),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_format_and_cross_family_are_unsupported() {
        let converter = ImageConverter;
        assert!(converter.supports(Format::Png, Format::Png).is_none());
        assert!(converter.supports(Format::Png, Format::Csv).is_none());
        assert!(converter.supports(Format::Csv, Format::Png).is_none());
    }

    #[test]
    fn only_jpeg_and_gif_targets_are_lossy() {
        let converter = ImageConverter;
        assert_eq!(
            converter.supports(Format::Png, Format::Jpeg),
            Some(Fidelity::Lossy)
        );
        assert_eq!(
            converter.supports(Format::Png, Format::Gif),
            Some(Fidelity::Lossy)
        );
        assert_eq!(
            converter.supports(Format::Jpeg, Format::Png),
            Some(Fidelity::Lossless)
        );
    }

    #[test]
    fn flatten_turns_transparent_pixels_white_and_keeps_opaque_ones() {
        let mut img = image::RgbaImage::new(2, 1);
        img.put_pixel(0, 0, image::Rgba([10, 20, 30, 0]));
        img.put_pixel(1, 0, image::Rgba([10, 20, 30, 255]));
        let flat = flatten_on_white(&DynamicImage::ImageRgba8(img));
        assert_eq!(flat.get_pixel(0, 0), &Rgb([255, 255, 255]));
        assert_eq!(flat.get_pixel(1, 0), &Rgb([10, 20, 30]));
    }

    #[test]
    fn jpeg_completeness_follows_segments_not_a_blind_search() {
        // SOI, APP1 con un "FF D9" falso dentro de su carga, SOS, datos con relleno FF00, EOI.
        let complete = [
            &[0xFF, 0xD8][..],
            &[0xFF, 0xE1, 0x00, 0x06, 0xFF, 0xD9, 0x00, 0x00],
            &[0xFF, 0xDA, 0x00, 0x04, 0x01, 0x02],
            &[0x12, 0xFF, 0x00, 0x34, 0xFF, 0xD0, 0x56],
            &[0xFF, 0xD9],
            b"datos extra tras el final",
        ]
        .concat();
        assert!(jpeg_is_complete(&complete));
        // Cortado dentro de los datos: el FF D9 falso del APP1 no debe engañarnos.
        assert!(!jpeg_is_complete(&complete[..complete.len() - 28]));
        assert!(!jpeg_is_complete(b"no es un jpg"));
        assert!(!jpeg_is_complete(&[0xFF, 0xD8]));
    }
}

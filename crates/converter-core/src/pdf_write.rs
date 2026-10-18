//! Escritura de PDF a partir de imágenes, con `pdf-writer` (Rust puro).

use std::io::{self, Write};

use crate::convert::PdfPaper;
use flate2::Compression;
use flate2::write::ZlibEncoder;
use image::DynamicImage;
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, TextStr};

/// 10 mm de margen.
const MARGIN: f32 = 28.35;

pub(crate) enum Raster {
    /// JPG original incrustado tal cual (`DCTDecode`): no se recomprime, así que no se
    /// pierde calidad ni crece el tamaño.
    Jpeg { data: Vec<u8>, gray: bool },
    /// Píxeles de 8 bits por canal comprimidos con Flate; el alfa va en una máscara aparte.
    Flate {
        rgb: Vec<u8>,
        alpha: Option<Vec<u8>>,
    },
}

pub(crate) struct PdfImage {
    pub width: u32,
    pub height: u32,
    pub raster: Raster,
}

impl PdfImage {
    pub fn jpeg(data: Vec<u8>, width: u32, height: u32, gray: bool) -> Self {
        Self {
            width,
            height,
            raster: Raster::Jpeg { data, gray },
        }
    }

    pub fn from_decoded(image: &DynamicImage) -> io::Result<Self> {
        let (width, height) = (image.width(), image.height());
        let (rgb, alpha) = if image.color().has_alpha() {
            let rgba = image.to_rgba8();
            let mut rgb = Vec::with_capacity(rgba.as_raw().len() / 4 * 3);
            let mut alpha = Vec::with_capacity(rgba.as_raw().len() / 4);
            for pixel in rgba.pixels() {
                rgb.extend_from_slice(&pixel.0[..3]);
                alpha.push(pixel.0[3]);
            }
            (rgb, Some(deflate(&alpha)?))
        } else {
            (image.to_rgb8().into_raw(), None)
        };
        Ok(Self {
            width,
            height,
            raster: Raster::Flate {
                rgb: deflate(&rgb)?,
                alpha,
            },
        })
    }
}

fn deflate(data: &[u8]) -> io::Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    encoder.finish()
}

/// Página del papel elegido (vertical u horizontal según la imagen) con la imagen contenida y centrada,
/// respetando su proporción.
fn layout(width: u32, height: u32, paper: PdfPaper) -> ([f32; 2], [f32; 4]) {
    let (short, long) = paper.size();
    let (page_w, page_h) = if width > height {
        (long, short)
    } else {
        (short, long)
    };
    let scale =
        ((page_w - 2.0 * MARGIN) / width as f32).min((page_h - 2.0 * MARGIN) / height as f32);
    let (w, h) = (width as f32 * scale, height as f32 * scale);
    (
        [page_w, page_h],
        [w, h, (page_w - w) / 2.0, (page_h - h) / 2.0],
    )
}

/// Un PDF con una página por imagen.
pub(crate) fn build_pdf(images: &[PdfImage], paper: PdfPaper) -> Vec<u8> {
    let mut next = 1;
    let mut alloc = || {
        let id = Ref::new(next);
        next += 1;
        id
    };
    let catalog_id = alloc();
    let pages_id = alloc();
    let info_id = alloc();

    let mut pdf = Pdf::new();
    pdf.document_info(info_id).producer(TextStr("Converter"));
    pdf.catalog(catalog_id).pages(pages_id);

    let mut page_ids = Vec::with_capacity(images.len());
    for image in images {
        let (page_id, content_id, image_id) = (alloc(), alloc(), alloc());
        page_ids.push(page_id);
        let ([page_w, page_h], [w, h, x, y]) = layout(image.width, image.height, paper);

        let mut page = pdf.page(page_id);
        page.media_box(Rect::new(0.0, 0.0, page_w, page_h))
            .parent(pages_id)
            .contents(content_id);
        page.resources().x_objects().pair(Name(b"Im0"), image_id);
        page.finish();

        let mut content = Content::new();
        content
            .save_state()
            .transform([w, 0.0, 0.0, h, x, y])
            .x_object(Name(b"Im0"))
            .restore_state();
        pdf.stream(content_id, &content.finish());

        match &image.raster {
            Raster::Jpeg { data, gray } => {
                let mut xobject = pdf.image_xobject(image_id, data);
                xobject.filter(Filter::DctDecode);
                xobject
                    .width(image.width as i32)
                    .height(image.height as i32);
                if *gray {
                    xobject.color_space().device_gray();
                } else {
                    xobject.color_space().device_rgb();
                }
                xobject.bits_per_component(8);
            }
            Raster::Flate { rgb, alpha } => {
                let mask_id = alpha.as_ref().map(|_| alloc());
                let mut xobject = pdf.image_xobject(image_id, rgb);
                xobject.filter(Filter::FlateDecode);
                xobject
                    .width(image.width as i32)
                    .height(image.height as i32);
                xobject.color_space().device_rgb();
                xobject.bits_per_component(8);
                if let Some(mask_id) = mask_id {
                    xobject.s_mask(mask_id);
                }
                xobject.finish();
                if let (Some(mask_id), Some(alpha)) = (mask_id, alpha) {
                    let mut mask = pdf.image_xobject(mask_id, alpha);
                    mask.filter(Filter::FlateDecode);
                    mask.width(image.width as i32).height(image.height as i32);
                    mask.color_space().device_gray();
                    mask.bits_per_component(8);
                }
            }
        }
    }

    pdf.pages(pages_id)
        .kids(page_ids.iter().copied())
        .count(page_ids.len() as i32);
    pdf.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn portrait_images_get_a_portrait_a4_page_and_landscape_a_landscape_one() {
        let (portrait, _) = layout(300, 400, PdfPaper::A4);
        let (landscape, _) = layout(400, 300, PdfPaper::A4);
        assert!(portrait[1] > portrait[0]);
        assert!(landscape[0] > landscape[1]);
    }

    #[test]
    fn image_is_contained_centered_and_keeps_its_aspect_ratio() {
        let ([page_w, page_h], [w, h, x, y]) = layout(1000, 500, PdfPaper::A4);
        assert!((w / h - 2.0).abs() < 1e-3);
        assert!(w <= page_w - 2.0 * MARGIN + 1e-3 && h <= page_h);
        assert!((x - (page_w - w) / 2.0).abs() < 1e-3 && (y - (page_h - h) / 2.0).abs() < 1e-3);
    }

    #[test]
    fn jpeg_is_embedded_untouched_and_alpha_gets_a_soft_mask() {
        let jpeg = PdfImage::jpeg(vec![0xFF, 0xD8, 0xFF, 0xD9], 10, 10, false);
        let bytes = build_pdf(&[jpeg], PdfPaper::A4);
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(contains(&bytes, b"/DCTDecode"));
        assert!(!contains(&bytes, b"/SMask"));

        let rgba = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            4,
            4,
            image::Rgba([1, 2, 3, 128]),
        ));
        let bytes = build_pdf(&[PdfImage::from_decoded(&rgba).unwrap()], PdfPaper::A4);
        assert!(contains(&bytes, b"/FlateDecode") && contains(&bytes, b"/SMask"));
    }

    #[test]
    fn letter_paper_changes_the_page_size() {
        let ([w, h], _) = layout(300, 400, PdfPaper::Letter);
        assert_eq!((w, h), (612.0, 792.0));
        let ([w, h], _) = layout(400, 300, PdfPaper::Letter);
        assert_eq!((w, h), (792.0, 612.0), "apaisada");
    }

    #[test]
    fn one_page_per_image() {
        let rgb = DynamicImage::ImageRgb8(image::RgbImage::new(4, 4));
        let one = || PdfImage::from_decoded(&rgb).unwrap();
        let bytes = build_pdf(&[one(), one(), one()], PdfPaper::A4);
        assert!(contains(&bytes, b"/Count 3"));
    }
}

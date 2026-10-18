//! Tests de PDF con archivos reales (`tests/fixtures`, generados con Pillow y a mano, sin
//! usar el código del motor). Necesitan `pdfium`: ejecuta `scripts/fetch-pdfium.ps1` una vez.

use std::fs;
use std::path::{Path, PathBuf};

use converter_core::{ConvertOptions, Engine, Error, Format};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn read(name: &str) -> Vec<u8> {
    fs::read(fixture(name)).unwrap_or_else(|e| panic!("falta el fixture {name}: {e}"))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn options_at(dpi: u32) -> ConvertOptions {
    ConvertOptions {
        pdf_dpi: dpi,
        ..Default::default()
    }
}

/// Páginas de un PDF como imágenes a 72 dpi: sus dimensiones son las de la página en puntos.
fn render_pages(pdf: &[u8]) -> Vec<image::RgbaImage> {
    Engine::default()
        .convert_bytes_pages(pdf, Format::Pdf, Format::Png, &options_at(72))
        .unwrap_or_else(|e| panic!("{e} (¿falta pdfium? ejecuta scripts/fetch-pdfium.ps1)"))
        .iter()
        .map(|png| image::load_from_memory(png).unwrap().to_rgba8())
        .collect()
}

fn to_pdf(name: &str, from: Format) -> Vec<u8> {
    Engine::default()
        .convert_bytes(&read(name), from, Format::Pdf, &ConvertOptions::default())
        .unwrap()
}

// ---------------------------------------------------------- imagen -> PDF

#[test]
fn image_to_pdf_makes_a_valid_a4_page_that_pdfium_can_open() {
    let pdf = to_pdf("sample.png", Format::Png);
    assert!(pdf.starts_with(b"%PDF-"));
    let pages = render_pages(&pdf);
    assert_eq!(pages.len(), 1);
    // 64x48 es apaisada -> A4 horizontal (842x595 pt)
    assert_eq!((pages[0].width(), pages[0].height()), (842, 595));
}

#[test]
fn jpeg_is_embedded_byte_for_byte_without_recompressing() {
    let jpg = read("photo.jpg");
    let pdf = Engine::default()
        .convert_bytes(&jpg, Format::Jpeg, Format::Pdf, &ConvertOptions::default())
        .unwrap();
    assert!(contains(&pdf, &jpg), "el JPG original va dentro del PDF");
    assert!(contains(&pdf, b"/DCTDecode"));
    assert!(
        pdf.len() < jpg.len() + 2_000,
        "sin hincharse: {} vs {}",
        pdf.len(),
        jpg.len()
    );
}

#[test]
fn png_is_embedded_losslessly_and_a_roundtrip_keeps_the_picture() {
    let original = image::load_from_memory(&read("photo.png"))
        .unwrap()
        .to_rgb8();
    let pdf = to_pdf("photo.png", Format::Png);
    // 640x480 en A4 horizontal: caben 784x538 pt -> escala 1.12 -> 717x538 pt.
    let page = &render_pages(&pdf)[0];
    assert_eq!((page.width(), page.height()), (842, 595));
    // El centro de la página es el centro de la imagen (degradado suave, ruido de ±18).
    let center = page.get_pixel(421, 297);
    let expected = original.get_pixel(320, 240);
    for channel in 0..3 {
        let diff = i32::from(center[channel]) - i32::from(expected[channel]);
        assert!(
            diff.abs() <= 30,
            "canal {channel}: {center:?} vs {expected:?}"
        );
    }
    // Los márgenes son blancos.
    assert_eq!(page.get_pixel(5, 5).0, [255, 255, 255, 255]);
}

#[test]
fn transparency_becomes_a_soft_mask_so_the_page_shows_through() {
    let pdf = to_pdf("sample_alpha.png", Format::Png);
    assert!(contains(&pdf, b"/SMask"));
    let page = &render_pages(&pdf)[0];
    // El rincón transparente del PNG (arriba a la izquierda de la imagen) deja ver el papel blanco.
    // Imagen 64x48 en A4 apaisada: ocupa x 28.35..813.65, y 32.4..562.6 (escala 12.27).
    let corner = page.get_pixel(60, 60);
    assert!(corner.0[..3].iter().all(|&c| c >= 250), "{corner:?}");
    // Y un píxel opaco de la imagen no es blanco.
    let opaque = page.get_pixel(421, 297);
    assert!(opaque.0[..3].iter().any(|&c| c < 240), "{opaque:?}");
}

#[test]
fn exif_orientation_is_applied_so_phone_photos_are_not_sideways() {
    // 64x48 con orientación 6 se ve 48x64 (vertical): página A4 vertical.
    let rotated = to_pdf("rotated_exif.jpg", Format::Jpeg);
    let pages = render_pages(&rotated);
    assert_eq!((pages[0].width(), pages[0].height()), (595, 842));

    let upright = to_pdf("sample.jpg", Format::Jpeg);
    let pages = render_pages(&upright);
    assert_eq!((pages[0].width(), pages[0].height()), (842, 595));
    // Con giro pendiente no se puede incrustar tal cual: se recodifica ya girado.
    assert!(!contains(&rotated, b"/DCTDecode"));

    // Lo mismo en conversiones entre imágenes: antes salían tumbadas.
    let png = Engine::default()
        .convert_bytes(
            &read("rotated_exif.jpg"),
            Format::Jpeg,
            Format::Png,
            &ConvertOptions::default(),
        )
        .unwrap();
    let img = image::load_from_memory(&png).unwrap();
    assert_eq!((img.width(), img.height()), (48, 64));
}

#[test]
fn grayscale_jpeg_is_embedded_as_devicegray() {
    let pdf = to_pdf("gray.jpg", Format::Jpeg);
    assert!(contains(&pdf, b"/DeviceGray") && contains(&pdf, b"/DCTDecode"));
    let page = &render_pages(&pdf)[0];
    let pixel = page.get_pixel(421, 297);
    assert!(
        pixel[0].abs_diff(pixel[1]) <= 2 && pixel[1].abs_diff(pixel[2]) <= 2,
        "{pixel:?}"
    );
}

#[test]
fn corrupt_image_does_not_produce_a_pdf() {
    let jpg = read("photo.jpg");
    let err = Engine::default()
        .convert_bytes(
            &jpg[..jpg.len() / 3],
            Format::Jpeg,
            Format::Pdf,
            &ConvertOptions::default(),
        )
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::Decode {
                format: Format::Jpeg,
                ..
            }
        ),
        "{err}"
    );
}

// ---------------------------------------------------------- PDF -> imagen

#[test]
fn every_page_becomes_an_image_with_the_size_of_its_page() {
    let engine = Engine::default();
    let pdf = read("sample.pdf");
    let pages = engine
        .convert_bytes_pages(&pdf, Format::Pdf, Format::Png, &ConvertOptions::default())
        .unwrap();
    assert_eq!(pages.len(), 3);
    let dims: Vec<_> = pages
        .iter()
        .map(|p| {
            let img = image::load_from_memory(p).unwrap();
            (img.width(), img.height())
        })
        .collect();
    // 150 dpi: A4 vertical 1240x1754, apaisada 1754x1240.
    assert_eq!(dims, [(1240, 1754), (1754, 1240), (1240, 1754)]);
    // No está en blanco: el rectángulo negro de cada página existe.
    for page in &pages {
        let img = image::load_from_memory(page).unwrap().to_rgb8();
        assert!(img.pixels().any(|p| p.0 == [0, 0, 0]), "página en blanco");
    }
}

#[test]
fn dpi_option_scales_the_output() {
    let pages = render_pages(&read("sample.pdf"));
    assert_eq!((pages[0].width(), pages[0].height()), (595, 842));
    let at_300 = Engine::default()
        .convert_bytes(
            &read("sample.pdf"),
            Format::Pdf,
            Format::Png,
            &options_at(300),
        )
        .unwrap();
    let img = image::load_from_memory(&at_300).unwrap();
    assert_eq!((img.width(), img.height()), (2479, 3508));
}

#[test]
fn pdf_converts_to_jpg_webp_and_tiff_too() {
    let engine = Engine::default();
    for target in [Format::Jpeg, Format::Webp, Format::Tiff] {
        let out = engine
            .convert_bytes(
                &read("sample.pdf"),
                Format::Pdf,
                target,
                &ConvertOptions::default(),
            )
            .unwrap();
        assert_eq!(Format::sniff(&out), Some(target), "{target}");
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!((img.width(), img.height()), (1240, 1754), "{target}");
    }
}

#[test]
fn text_is_extracted_with_accents_and_pages_are_separated() {
    let txt = Engine::default()
        .convert_bytes(
            &read("sample.pdf"),
            Format::Pdf,
            Format::Txt,
            &ConvertOptions::default(),
        )
        .unwrap();
    assert!(txt.starts_with(b"\xEF\xBB\xBF"), "UTF-8 con BOM");
    let text = String::from_utf8(txt[3..].to_vec()).unwrap();
    assert!(text.contains("Página 1"), "{text:?}");
    assert!(text.contains("Hola mundo, ñandú y café"), "{text:?}");
    assert!(text.contains("Página 2 (apaisada)") && text.contains("Fin del documento"));
    assert!(text.find("Página 1").unwrap() < text.find("Página 3").unwrap());
}

#[test]
fn a_scanned_pdf_without_text_asks_for_ocr() {
    let err = Engine::default()
        .convert_bytes(
            &read("scan.pdf"),
            Format::Pdf,
            Format::Txt,
            &ConvertOptions::default(),
        )
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::NeedsOcr {
                format: Format::Pdf
            }
        ),
        "{err}"
    );
    // Pero se puede rasterizar sin problema.
    assert_eq!(render_pages(&read("scan.pdf")).len(), 1);
}

#[test]
fn corrupt_and_empty_pdfs_are_decode_errors_not_panics() {
    let engine = Engine::default();
    let options = ConvertOptions::default();
    let pdf = read("sample.pdf");
    for (label, bytes) in [
        ("truncado", pdf[..pdf.len() / 2].to_vec()),
        ("solo cabecera", b"%PDF-1.4\n".to_vec()),
        ("basura", b"%PDF-1.7 esto no es un pdf de verdad".to_vec()),
    ] {
        let err = engine
            .convert_bytes(&bytes, Format::Pdf, Format::Png, &options)
            .unwrap_err();
        assert!(
            matches!(
                err,
                Error::Decode {
                    format: Format::Pdf,
                    ..
                }
            ),
            "{label}: {err}"
        );
    }
    assert!(matches!(
        engine.convert_bytes(b"", Format::Pdf, Format::Png, &options),
        Err(Error::Empty {
            format: Format::Pdf
        })
    ));
}

// ------------------------------------------------------------- archivos

#[test]
fn multipage_pdf_goes_into_a_folder_and_single_page_next_to_the_original() {
    let dir = tempfile::tempdir().unwrap();
    let multi = dir.path().join("informe.pdf");
    fs::copy(fixture("sample.pdf"), &multi).unwrap();
    let before = fs::read(&multi).unwrap();
    let engine = Engine::default();

    let files = engine
        .convert_file(&multi, Format::Png, &ConvertOptions::default())
        .unwrap();
    let names: Vec<_> = files
        .iter()
        .map(|f| {
            f.strip_prefix(dir.path())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert_eq!(
        names,
        [
            "informe/informe-1.png",
            "informe/informe-2.png",
            "informe/informe-3.png"
        ]
    );
    assert_eq!(fs::read(&multi).unwrap(), before, "el original no cambia");
    assert!(
        files
            .iter()
            .all(|f| Format::sniff(&fs::read(f).unwrap()) == Some(Format::Png))
    );

    // Una segunda conversión no pisa la carpeta anterior.
    let again = engine
        .convert_file(&multi, Format::Png, &ConvertOptions::default())
        .unwrap();
    assert!(again[0].starts_with(dir.path().join("informe (1)")));

    let scan = dir.path().join("hoja.pdf");
    fs::copy(fixture("scan.pdf"), &scan).unwrap();
    let single = engine
        .convert_file(&scan, Format::Png, &ConvertOptions::default())
        .unwrap();
    assert_eq!(single, [dir.path().join("hoja.png")]);
}

#[test]
fn a_failed_pdf_conversion_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("roto.pdf");
    fs::write(&path, b"%PDF-1.4\nbasura").unwrap();
    assert!(
        Engine::default()
            .convert_file(&path, Format::Png, &ConvertOptions::default())
            .is_err()
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn concurrent_conversions_do_not_conflict_in_pdfium() {
    let pdf = read("sample.pdf");
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..6)
            .map(|_| {
                scope.spawn(|| {
                    Engine::default()
                        .convert_bytes_pages(&pdf, Format::Pdf, Format::Png, &options_at(72))
                        .unwrap()
                        .len()
                })
            })
            .collect();
        for handle in handles {
            assert_eq!(handle.join().unwrap(), 3);
        }
    });
}

#[test]
fn letter_paper_produces_us_letter_pages() {
    let letter = ConvertOptions {
        pdf_paper: converter_core::PdfPaper::Letter,
        ..Default::default()
    };
    let pdf = Engine::default()
        .convert_bytes(&read("sample.png"), Format::Png, Format::Pdf, &letter)
        .unwrap();
    let page = &render_pages(&pdf)[0];
    // 64x48 es apaisada: carta horizontal = 792x612 pt (A4 sería 842x595).
    assert_eq!((page.width(), page.height()), (792, 612));

    let jpg = Engine::default()
        .convert_bytes(
            &read("rotated_exif.jpg"),
            Format::Jpeg,
            Format::Pdf,
            &letter,
        )
        .unwrap();
    let page = &render_pages(&jpg)[0];
    assert_eq!(
        (page.width(), page.height()),
        (612, 792),
        "vertical tras girar por EXIF"
    );
}

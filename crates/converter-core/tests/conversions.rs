//! Tests de conversión con archivos reales de `tests/fixtures/` (generados con Pillow y
//! `zipfile`, es decir, con codificadores independientes de los que usa el motor).

use std::fs;
use std::path::{Path, PathBuf};

use converter_core::{ConvertOptions, Engine, Error, Family, Fidelity, Format, WebpMode};

/// Conversiones que hace un programa externo (Word o LibreOffice); ver `tests/office.rs`, que
/// comprueba que esta lista coincide con la del motor.
const ENGINE_PAIRS: [(Format, Format); 5] = [
    (Format::Docx, Format::Pdf),
    (Format::Markdown, Format::Pdf),
    (Format::Doc, Format::Pdf),
    (Format::Doc, Format::Docx),
    (Format::Pdf, Format::Docx),
];

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn read(name: &str) -> Vec<u8> {
    fs::read(fixture(name)).unwrap_or_else(|e| panic!("falta el fixture {name}: {e}"))
}

/// Un archivo real por cada formato de origen. Si se registra un formato nuevo sin fixture,
/// `every_published_pair_converts_a_real_file` falla.
fn sample(format: Format) -> Vec<u8> {
    read(match format {
        Format::Jpeg => "sample.jpg",
        Format::Png => "sample.png",
        Format::Webp => "sample.webp",
        Format::Bmp => "sample.bmp",
        Format::Tiff => "sample.tif",
        Format::Gif => "sample.gif",
        Format::Csv => "plain.csv",
        Format::Xlsx => "sample.xlsx",
        Format::Json => "records.json",
        Format::Pdf => "sample.pdf",
        Format::Docx => "sample.docx",
        Format::Doc => "sample.doc",
        Format::Markdown => "sample.md",
        Format::Txt => panic!("TXT aún no es origen de ninguna conversión: añade su fixture"),
    })
}

fn convert(input: &[u8], from: Format, to: Format) -> Vec<u8> {
    Engine::default()
        .convert_bytes(input, from, to, &ConvertOptions::default())
        .unwrap_or_else(|e| panic!("{from} -> {to}: {e}"))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("salida UTF-8")
}

fn csv_rows(bytes: &[u8]) -> Vec<Vec<String>> {
    let text = text(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes())
        .records()
        .map(|r| r.unwrap().iter().map(str::to_string).collect())
        .collect()
}

// ------------------------------------------------------------ cobertura

#[test]
fn every_published_pair_converts_a_real_file() {
    let engine = Engine::default();
    let mut pairs = 0;
    for from in Format::ALL {
        for target in engine.targets(from) {
            // Word/LibreOffice se prueban (a propósito) en tests/office.rs.
            if ENGINE_PAIRS.contains(&(from, target.format)) {
                continue;
            }
            let output = convert(&sample(from), from, target.format);
            assert!(
                !output.is_empty(),
                "{from} -> {} produjo 0 bytes",
                target.format
            );
            if let Some(sniffed) = Format::sniff(&output) {
                assert_eq!(sniffed, target.format, "{from} -> {}", target.format);
            }
            // Solo las conversiones imagen -> imagen conservan el tamaño; un PDF rasterizado
            // tiene el de su página (se comprueba en tests/pdf.rs).
            if from.family() == Family::Image && target.format.family() == Family::Image {
                let img = image::load_from_memory(&output).unwrap();
                assert_eq!(
                    (img.width(), img.height()),
                    (WIDTH, HEIGHT),
                    "{from} -> {}",
                    target.format
                );
            }
            pairs += 1;
        }
    }
    // 6 imágenes -> 5 imágenes + PDF; 6 pares de datos; PDF -> PNG, JPG, WEBP, TIFF y TXT;
    // DOCX -> TXT; PDF -> MD. (DOCX/DOC/MD -> PDF, DOC -> DOCX y PDF -> DOCX van por un programa externo.)
    assert_eq!(pairs, 6 * 6 + 6 + 5 + 1 + 1);
}

#[test]
fn targets_are_ranked_by_coherence_and_only_cross_families_where_it_makes_sense() {
    let engine = Engine::default();

    let from_jpg: Vec<_> = engine
        .targets(Format::Jpeg)
        .iter()
        .map(|t| t.format)
        .collect();
    assert_eq!(from_jpg[..3], [Format::Webp, Format::Png, Format::Pdf]);
    assert!(
        !from_jpg.contains(&Format::Jpeg),
        "no ofrece el propio formato"
    );
    assert!(!from_jpg.contains(&Format::Csv) && !from_jpg.contains(&Format::Xlsx));
    assert!(
        !from_jpg.contains(&Format::Txt),
        "una foto no se ofrece como texto"
    );

    let from_csv: Vec<_> = engine
        .targets(Format::Csv)
        .iter()
        .map(|t| t.format)
        .collect();
    assert_eq!(from_csv, [Format::Xlsx, Format::Json]);

    let scores: Vec<_> = engine
        .targets(Format::Png)
        .iter()
        .map(|t| t.score)
        .collect();
    assert!(
        scores.windows(2).all(|w| w[0] >= w[1]),
        "orden descendente: {scores:?}"
    );
}

#[test]
fn lossy_targets_are_flagged() {
    let engine = Engine::default();
    let fidelity = |from, to| {
        engine
            .targets(from)
            .into_iter()
            .find(|t| t.format == to)
            .unwrap()
            .fidelity
    };
    assert_eq!(fidelity(Format::Png, Format::Jpeg), Fidelity::Lossy);
    assert_eq!(fidelity(Format::Jpeg, Format::Png), Fidelity::Lossless);
    assert_eq!(fidelity(Format::Xlsx, Format::Csv), Fidelity::Lossy);
    assert_eq!(fidelity(Format::Csv, Format::Xlsx), Fidelity::Lossless);
}

// -------------------------------------------------------------- imágenes

#[test]
fn png_with_transparency_to_jpg_flattens_onto_white() {
    let jpg = convert(&read("sample_alpha.png"), Format::Png, Format::Jpeg);
    let img = image::load_from_memory(&jpg).unwrap().to_rgb8();
    let corner = img.get_pixel(2, 2); // zona transparente en el original
    assert!(
        corner.0.iter().all(|&c| c >= 250),
        "esperaba blanco, salió {corner:?}"
    );
}

#[test]
fn png_with_transparency_to_webp_keeps_alpha_and_pixels() {
    let original = image::load_from_memory(&read("sample_alpha.png"))
        .unwrap()
        .to_rgba8();
    let webp = convert(&read("sample_alpha.png"), Format::Png, Format::Webp);
    let decoded = image::load_from_memory(&webp).unwrap().to_rgba8();
    assert_eq!(
        decoded.get_pixel(2, 2)[3],
        0,
        "la transparencia se conserva"
    );
    assert_eq!(
        decoded.get_pixel(30, 30),
        original.get_pixel(30, 30),
        "WEBP sin pérdida"
    );
}

#[test]
fn lossless_chain_png_bmp_tiff_png_preserves_every_pixel() {
    let original = image::load_from_memory(&read("sample.png"))
        .unwrap()
        .to_rgb8();
    let bmp = convert(&read("sample.png"), Format::Png, Format::Bmp);
    let tiff = convert(&bmp, Format::Bmp, Format::Tiff);
    let png = convert(&tiff, Format::Tiff, Format::Png);
    let round_trip = image::load_from_memory(&png).unwrap().to_rgb8();
    assert_eq!(original, round_trip);
}

#[test]
fn jpeg_quality_changes_the_output_size() {
    let engine = Engine::default();
    let size_at = |q| {
        let options = ConvertOptions {
            jpeg_quality: q,
            ..Default::default()
        };
        engine
            .convert_bytes(&read("sample.png"), Format::Png, Format::Jpeg, &options)
            .unwrap()
            .len()
    };
    assert!(size_at(95) > size_at(10));
}

#[test]
fn gif_with_palette_converts_to_png() {
    let png = convert(&read("sample.gif"), Format::Gif, Format::Png);
    assert_eq!(Format::sniff(&png), Some(Format::Png));
}

// ----------------------------------------------------- entradas inválidas

#[test]
fn truncated_image_is_a_decode_error_not_a_panic() {
    let png = read("sample.png");
    let err = Engine::default()
        .convert_bytes(
            &png[..png.len() / 2],
            Format::Png,
            Format::Webp,
            &ConvertOptions::default(),
        )
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::Decode {
                format: Format::Png,
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn garbage_with_an_image_extension_is_a_decode_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("falso.jpg");
    fs::write(&path, b"esto no es una imagen").unwrap();
    let err = Engine::default()
        .convert_file(&path, Format::Png, &ConvertOptions::default())
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
    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        1,
        "no deja residuos"
    );
}

#[test]
fn empty_file_is_reported_as_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vacio.png");
    fs::write(&path, b"").unwrap();
    let err = Engine::default()
        .convert_file(&path, Format::Jpeg, &ConvertOptions::default())
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::Empty {
                format: Format::Png
            }
        ),
        "{err}"
    );
}

#[test]
fn unknown_extension_and_missing_file_are_distinct_errors() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::default();
    let options = ConvertOptions::default();

    let notes = dir.path().join("notas.xyz");
    fs::write(&notes, b"hola").unwrap();
    assert!(matches!(
        engine.convert_file(&notes, Format::Png, &options),
        Err(Error::UnknownFormat(_))
    ));
    assert!(matches!(
        engine.convert_file(&dir.path().join("nada.png"), Format::Jpeg, &options),
        Err(Error::Io { .. })
    ));
}

#[test]
fn unsupported_pairs_are_rejected() {
    let engine = Engine::default();
    let options = ConvertOptions::default();
    for (from, to) in [
        (Format::Png, Format::Csv),
        (Format::Csv, Format::Png),
        (Format::Png, Format::Png),
    ] {
        assert!(matches!(
            engine.convert_bytes(&sample(from), from, to, &options),
            Err(Error::Unsupported { .. })
        ));
    }
}

// ------------------------------------------------------------ convert_file

#[test]
fn convert_file_writes_next_to_the_original_without_touching_it() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("Mi foto ñ.jpg");
    fs::copy(fixture("sample.jpg"), &original).unwrap();
    let before = fs::read(&original).unwrap();
    let engine = Engine::default();
    let options = ConvertOptions::default();

    let first = engine
        .convert_file(&original, Format::Webp, &options)
        .unwrap()
        .remove(0);
    let second = engine
        .convert_file(&original, Format::Webp, &options)
        .unwrap()
        .remove(0);

    assert_eq!(first.file_name().unwrap(), "Mi foto ñ.webp");
    assert_eq!(second.file_name().unwrap(), "Mi foto ñ (1).webp");
    assert_eq!(
        fs::read(&original).unwrap(),
        before,
        "el original no cambia"
    );
    assert_eq!(
        Format::sniff(&fs::read(&first).unwrap()),
        Some(Format::Webp)
    );
    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        3,
        "sin temporales"
    );
}

#[test]
fn a_jpg_renamed_to_png_is_still_converted_by_its_real_content() {
    let dir = tempfile::tempdir().unwrap();
    let disguised = dir.path().join("foto.png");
    fs::copy(fixture("sample.jpg"), &disguised).unwrap();
    let out = Engine::default()
        .convert_file(&disguised, Format::Webp, &ConvertOptions::default())
        .unwrap();
    assert_eq!(
        Format::sniff(&fs::read(&out[0]).unwrap()),
        Some(Format::Webp)
    );
}

// ---------------------------------------------------------------- datos

#[test]
fn spanish_excel_csv_to_json_keeps_accents_codes_numbers_and_multiline_cells() {
    // Separador ';', BOM, acentos, "007" (código), 1234.5 y una celda con salto de línea.
    let json = convert(&read("es_semicolon.csv"), Format::Csv, Format::Json);
    let value: serde_json::Value = serde_json::from_slice(&json).unwrap();
    let rows = value.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["nombre"], "Ñandú");
    assert_eq!(
        rows[0]["código"], "007",
        "los ceros a la izquierda no se pierden"
    );
    assert_eq!(rows[0]["importe"], 1234.5);
    assert_eq!(rows[0]["nota"], "línea 1\nlínea 2");
    assert_eq!(rows[1]["código"], 12);
    assert_eq!(rows[1]["importe"], -3);
    assert!(rows[1]["nota"].is_null());
}

#[test]
fn windows_1252_csv_is_decoded_instead_of_failing() {
    let ansi = b"nombre;ciudad\r\nJos\xE9;Bogot\xE1\r\n";
    let json = convert(ansi, Format::Csv, Format::Json);
    let value: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(value[0]["nombre"], "José");
    assert_eq!(value[0]["ciudad"], "Bogotá");
}

#[test]
fn csv_to_xlsx_to_csv_round_trip_preserves_every_cell() {
    let original = read("es_semicolon.csv");
    let xlsx = convert(&original, Format::Csv, Format::Xlsx);
    let back = convert(&xlsx, Format::Xlsx, Format::Csv);
    assert_eq!(
        csv_rows(&back),
        [
            vec!["nombre", "código", "importe", "nota"],
            vec!["Ñandú", "007", "1234.5", "línea 1\nlínea 2"],
            vec!["José", "12", "-3", ""],
        ]
    );
}

#[test]
fn xlsx_reads_only_the_first_sheet_and_keeps_value_types() {
    let csv = convert(&read("sample.xlsx"), Format::Xlsx, Format::Csv);
    assert_eq!(
        csv_rows(&csv),
        [
            vec!["producto", "unidades", "precio", "activo"],
            vec!["Café", "3", "2.5", "true"],
            vec!["Té", "10", "", "false"],
        ]
    );
    assert!(!text(&csv).contains("segunda hoja"));
}

#[test]
fn json_to_csv_unions_columns_and_nulls_become_empty() {
    let csv = convert(&read("records.json"), Format::Json, Format::Csv);
    assert_eq!(
        csv_rows(&csv),
        [
            vec!["id", "name", "tags", "vip"],
            vec!["1", "Ana", r#"["a","b"]"#, ""],
            vec!["2", "Luis", "", "true"],
            vec!["3", "", "", ""],
        ]
    );
}

#[test]
fn csv_output_has_bom_and_respects_the_delimiter_option() {
    let engine = Engine::default();
    let options = ConvertOptions {
        csv_delimiter: b';',
        csv_bom: true,
        ..Default::default()
    };
    let out = engine
        .convert_bytes(&read("records.json"), Format::Json, Format::Csv, &options)
        .unwrap();
    assert!(out.starts_with(b"\xEF\xBB\xBF"));
    assert!(text(&out).contains("id;name;tags;vip"));

    let no_bom = ConvertOptions {
        csv_bom: false,
        ..Default::default()
    };
    let out = engine
        .convert_bytes(&read("records.json"), Format::Json, Format::Csv, &no_bom)
        .unwrap();
    assert!(!out.starts_with(b"\xEF\xBB\xBF"));
}

#[test]
fn malformed_xlsx_and_json_are_decode_errors() {
    let engine = Engine::default();
    let options = ConvertOptions::default();
    assert!(matches!(
        engine.convert_bytes(
            b"PK\x03\x04no es un xlsx",
            Format::Xlsx,
            Format::Csv,
            &options
        ),
        Err(Error::Decode {
            format: Format::Xlsx,
            ..
        })
    ));
    assert!(matches!(
        engine.convert_bytes(b"{ roto", Format::Json, Format::Csv, &options),
        Err(Error::Decode {
            format: Format::Json,
            ..
        })
    ));
}

#[test]
fn empty_csv_and_empty_json_array_convert_without_panicking() {
    assert_eq!(
        text(&convert(b"\n", Format::Csv, Format::Json)).trim(),
        "[]"
    );
    let csv = convert(b"[]", Format::Json, Format::Csv);
    assert!(csv_rows(&csv).is_empty());
    assert!(!convert(b"\n", Format::Csv, Format::Xlsx).is_empty());
}

// ------------------------------------------------------------------ WEBP

fn webp_size(name: &str, from: Format, options: &ConvertOptions) -> usize {
    Engine::default()
        .convert_bytes(&read(name), from, Format::Webp, options)
        .unwrap()
        .len()
}

#[test]
fn jpg_to_webp_is_lossy_by_default_and_much_smaller_than_lossless() {
    let auto = webp_size("photo.jpg", Format::Jpeg, &ConvertOptions::default());
    let lossless = webp_size(
        "photo.jpg",
        Format::Jpeg,
        &ConvertOptions {
            webp_mode: WebpMode::Lossless,
            ..Default::default()
        },
    );
    assert!(auto * 2 < lossless, "auto={auto} lossless={lossless}");
    assert!(
        auto < read("photo.jpg").len(),
        "el WEBP debe pesar menos que el JPG de origen"
    );
}

#[test]
fn webp_quality_option_changes_the_size_and_output_decodes() {
    let size_at = |q| {
        webp_size(
            "photo.jpg",
            Format::Jpeg,
            &ConvertOptions {
                webp_quality: q,
                ..Default::default()
            },
        )
    };
    assert!(size_at(95) > size_at(30));

    let webp = convert(&read("photo.jpg"), Format::Jpeg, Format::Webp);
    let decoded = image::load_from_memory(&webp).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (640, 480));
}

#[test]
fn png_to_webp_is_lossless_by_default_and_exact() {
    let original = image::load_from_memory(&read("photo.png"))
        .unwrap()
        .to_rgb8();
    let webp = convert(&read("photo.png"), Format::Png, Format::Webp);
    let decoded = image::load_from_memory(&webp).unwrap().to_rgb8();
    assert_eq!(original, decoded);
}

#[test]
fn lossy_webp_keeps_transparency() {
    let options = ConvertOptions {
        webp_mode: WebpMode::Lossy,
        ..Default::default()
    };
    let webp = Engine::default()
        .convert_bytes(
            &read("sample_alpha.png"),
            Format::Png,
            Format::Webp,
            &options,
        )
        .unwrap();
    let decoded = image::load_from_memory(&webp).unwrap().to_rgba8();
    assert_eq!(decoded.get_pixel(2, 2)[3], 0);
    assert_eq!(decoded.get_pixel(30, 30)[3], 255);
}

#[test]
fn images_beyond_webp_dimension_limit_fail_cleanly() {
    let mut png = Vec::new();
    image::RgbImage::new(16_384, 1)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let err = Engine::default()
        .convert_bytes(&png, Format::Png, Format::Webp, &ConvertOptions::default())
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::Encode {
                format: Format::Webp,
                ..
            }
        ),
        "{err}"
    );
}

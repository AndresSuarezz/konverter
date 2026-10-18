use crate::format::Format;

/// Probabilidad a priori (0-100) de que un usuario quiera pasar de `from` a `to`.
///
/// Es la base del orden de los pétalos: JPG→WEBP/PNG es mucho más habitual que JPG→BMP.
/// Los contadores de uso local la ajustarán más adelante; los pares sin entrada valen 50.
pub fn popularity(from: Format, to: Format) -> u8 {
    use Format::*;
    match (from, to) {
        (Jpeg, Webp) => 95,
        (Jpeg, Png) => 85,
        (Jpeg, Tiff) => 35,
        (Jpeg, Bmp) => 40,
        (Jpeg, Gif) => 20,

        (Png, Webp) => 95,
        (Png, Jpeg) => 90,
        (Png, Bmp) => 40,
        (Png, Tiff) => 35,
        (Png, Gif) => 25,

        (Webp, Png) => 95,
        (Webp, Jpeg) => 90,
        (Webp, Bmp) => 35,
        (Webp, Tiff) => 30,
        (Webp, Gif) => 25,

        (Bmp, Png) => 95,
        (Bmp, Jpeg) => 90,
        (Bmp, Webp) => 85,
        (Bmp, Tiff) => 30,
        (Bmp, Gif) => 20,

        (Tiff, Png) => 95,
        (Tiff, Jpeg) => 90,
        (Tiff, Webp) => 80,
        (Tiff, Bmp) => 40,
        (Tiff, Gif) => 20,

        (Gif, Png) => 95,
        (Gif, Webp) => 90,
        (Gif, Jpeg) => 70,
        (Gif, Bmp) => 30,
        (Gif, Tiff) => 30,

        (Csv, Xlsx) => 95,
        (Csv, Json) => 70,
        (Xlsx, Csv) => 95,
        (Xlsx, Json) => 60,
        (Json, Csv) => 90,
        (Json, Xlsx) => 70,

        // Imágenes → PDF: fotos de formularios y escaneos (TIFF) son los casos habituales.
        (Jpeg, Pdf) => 75,
        (Tiff, Pdf) => 75,
        (Png, Pdf) => 70,
        (Webp, Pdf) | (Bmp, Pdf) => 55,
        (Gif, Pdf) => 30,

        // Documentos de Office: pasar a PDF es lo habitual; PDF → Word es de las peticiones más
        // comunes, así que sube por encima de las imágenes.
        (Docx, Pdf) | (Doc, Pdf) => 95,
        (Doc, Docx) => 90,
        (Docx, Txt) => 60,
        (Pdf, Docx) => 95,

        // Markdown: de .md se quiere un PDF; de PDF se quiere texto con estructura.
        (Markdown, Pdf) => 95,
        (Markdown, Docx) => 75,
        (Pdf, Markdown) => 80,

        (Pdf, Png) => 90,
        (Pdf, Jpeg) => 85,
        (Pdf, Txt) => 70,
        (Pdf, Webp) => 45,
        (Pdf, Tiff) => 30,

        _ => 50,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_conversions_outrank_exotic_ones() {
        assert!(popularity(Format::Jpeg, Format::Webp) > popularity(Format::Jpeg, Format::Bmp));
        assert!(popularity(Format::Png, Format::Jpeg) > popularity(Format::Png, Format::Gif));
    }
}

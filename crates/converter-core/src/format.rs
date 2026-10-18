use std::fmt;
use std::path::Path;

/// Formatos de archivo que el motor sabe leer o escribir.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Format {
    Jpeg,
    Png,
    Webp,
    Bmp,
    Tiff,
    Gif,
    Csv,
    Xlsx,
    Json,
    Pdf,
    Txt,
    Docx,
    Doc,
    Markdown,
}

/// Familia de un formato. Una conversión solo tiene sentido dentro de su familia.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Image,
    Data,
    Document,
}

impl Format {
    pub const ALL: [Format; 14] = [
        Format::Jpeg,
        Format::Png,
        Format::Webp,
        Format::Bmp,
        Format::Tiff,
        Format::Gif,
        Format::Csv,
        Format::Xlsx,
        Format::Json,
        Format::Pdf,
        Format::Txt,
        Format::Docx,
        Format::Doc,
        Format::Markdown,
    ];

    /// Extensión canónica, sin punto y en minúsculas.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Jpeg => "jpg",
            Format::Png => "png",
            Format::Webp => "webp",
            Format::Bmp => "bmp",
            Format::Tiff => "tiff",
            Format::Gif => "gif",
            Format::Csv => "csv",
            Format::Xlsx => "xlsx",
            Format::Json => "json",
            Format::Pdf => "pdf",
            Format::Txt => "txt",
            Format::Docx => "docx",
            Format::Doc => "doc",
            Format::Markdown => "md",
        }
    }

    /// Nombre para mostrar en la interfaz.
    pub fn label(self) -> &'static str {
        match self {
            Format::Jpeg => "JPG",
            Format::Png => "PNG",
            Format::Webp => "WEBP",
            Format::Bmp => "BMP",
            Format::Tiff => "TIFF",
            Format::Gif => "GIF",
            Format::Csv => "CSV",
            Format::Xlsx => "XLSX",
            Format::Json => "JSON",
            Format::Pdf => "PDF",
            Format::Txt => "TXT",
            Format::Docx => "DOCX",
            Format::Doc => "DOC",
            Format::Markdown => "MD",
        }
    }

    pub fn family(self) -> Family {
        match self {
            Format::Jpeg
            | Format::Png
            | Format::Webp
            | Format::Bmp
            | Format::Tiff
            | Format::Gif => Family::Image,
            Format::Csv | Format::Xlsx | Format::Json => Family::Data,
            Format::Pdf | Format::Txt | Format::Docx | Format::Doc | Format::Markdown => {
                Family::Document
            }
        }
    }

    pub fn from_extension(ext: &str) -> Option<Format> {
        match ext.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" | "jpe" | "jfif" => Some(Format::Jpeg),
            "png" => Some(Format::Png),
            "webp" => Some(Format::Webp),
            "bmp" => Some(Format::Bmp),
            "tif" | "tiff" => Some(Format::Tiff),
            "gif" => Some(Format::Gif),
            "csv" => Some(Format::Csv),
            "xlsx" => Some(Format::Xlsx),
            "json" => Some(Format::Json),
            "pdf" => Some(Format::Pdf),
            "txt" => Some(Format::Txt),
            "docx" => Some(Format::Docx),
            "doc" => Some(Format::Doc),
            "md" | "markdown" | "mdown" | "mkd" => Some(Format::Markdown),
            _ => None,
        }
    }

    /// Reconoce un formato por sus bytes iniciales (firma "mágica").
    pub fn sniff(head: &[u8]) -> Option<Format> {
        if head.starts_with(b"%PDF-") {
            Some(Format::Pdf)
        } else if head.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
            Some(Format::Png)
        } else if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(Format::Jpeg)
        } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
            Some(Format::Gif)
        } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
            Some(Format::Webp)
        } else if head.starts_with(b"II*\0") || head.starts_with(b"MM\0*") {
            Some(Format::Tiff)
        } else if head.len() >= 14 && head.starts_with(b"BM") && head[6..10] == [0; 4] {
            // "BM" por sí solo también abre muchos CSV/TXT; los cuatro bytes reservados
            // a cero de la cabecera BMP evitan el falso positivo.
            Some(Format::Bmp)
        } else {
            None
        }
    }

    /// Determina el formato real de un archivo. Si el contenido contradice la
    /// extensión (p. ej. un JPG renombrado a `.png`) manda el contenido.
    pub fn detect(path: &Path, head: &[u8]) -> Option<Format> {
        if let Some(format) = Format::sniff(head) {
            return Some(format);
        }
        let by_extension = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(Format::from_extension);
        if head.starts_with(b"PK\x03\x04") {
            // Un ZIP solo se acepta si la extensión dice que es un paquete de Office que
            // conocemos; un .zip cualquiera no debe confundirse con ellos.
            return by_extension.filter(|f| matches!(f, Format::Xlsx | Format::Docx));
        }
        if head.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
            // Contenedor OLE: lo usan .doc, .xls, .ppt, .msg... Solo se acepta como .doc.
            return by_extension.filter(|f| *f == Format::Doc);
        }
        by_extension
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_are_case_insensitive_and_cover_aliases() {
        assert_eq!(Format::from_extension("JPEG"), Some(Format::Jpeg));
        assert_eq!(Format::from_extension("tif"), Some(Format::Tiff));
        assert_eq!(Format::from_extension("xyz"), None);
    }

    #[test]
    fn every_format_round_trips_through_its_extension() {
        for format in Format::ALL {
            assert_eq!(Format::from_extension(format.extension()), Some(format));
        }
    }

    #[test]
    fn content_wins_over_a_lying_extension() {
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];
        assert_eq!(
            Format::detect(Path::new("foto.jpg"), &png),
            Some(Format::Png)
        );
    }

    #[test]
    fn text_starting_with_bm_is_not_a_bitmap() {
        let csv = b"BM,precio\n1,2\n";
        assert_eq!(
            Format::detect(Path::new("datos.csv"), csv),
            Some(Format::Csv)
        );
    }

    #[test]
    fn office_packages_are_only_accepted_when_the_extension_agrees() {
        let zip = b"PK\x03\x04rest";
        assert_eq!(Format::detect(Path::new("a.xlsx"), zip), Some(Format::Xlsx));
        assert_eq!(Format::detect(Path::new("a.docx"), zip), Some(Format::Docx));
        assert_eq!(Format::detect(Path::new("a.pptx"), zip), None);
        let ole = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0];
        assert_eq!(Format::detect(Path::new("a.doc"), &ole), Some(Format::Doc));
        assert_eq!(Format::detect(Path::new("a.xls"), &ole), None);
        assert_eq!(Format::detect(Path::new("a.csv"), zip), None);
    }

    #[test]
    fn pdf_is_recognised_by_its_header_even_with_a_wrong_extension() {
        assert_eq!(
            Format::detect(
                Path::new("informe.dat"),
                b"%PDF-1.7
%"
            ),
            Some(Format::Pdf)
        );
    }

    #[test]
    fn unknown_extension_without_magic_is_unknown() {
        assert_eq!(Format::detect(Path::new("notas.xyz"), b"hola"), None);
    }
}

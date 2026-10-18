use std::fs;
use std::path::{Path, PathBuf};

use crate::convert::{ConvertOptions, Converter, Fidelity, Page};
use crate::docx_text::DocxTextConverter;
use crate::error::{Error, Result};
use crate::format::Format;
use crate::image_conv::ImageConverter;
use crate::md_docx::MarkdownDocxConverter;
use crate::office::OfficeConverter;
use crate::output::OutputSet;
use crate::pdf::PdfConverter;
use crate::ranking::popularity;
use crate::tabular::TabularConverter;

/// Un destino ofrecido para un formato de origen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub format: Format,
    pub fidelity: Fidelity,
    /// Coherencia a priori, 0-100; ya viene ordenado de mayor a menor.
    pub score: u8,
}

/// Registro de conversores. Es la única fuente de verdad de qué se puede convertir:
/// la paleta consume [`Engine::targets`] y no decide nada por su cuenta.
pub struct Engine {
    converters: Vec<Box<dyn Converter>>,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            converters: vec![
                Box::new(ImageConverter),
                Box::new(TabularConverter),
                Box::new(PdfConverter::default()),
                Box::new(DocxTextConverter),
                Box::new(MarkdownDocxConverter),
                Box::new(OfficeConverter),
            ],
        }
    }
}

impl Engine {
    /// Destinos para `from`, del más al menos coherente. Nunca incluye el propio formato.
    pub fn targets(&self, from: Format) -> Vec<Target> {
        let mut targets: Vec<Target> = Format::ALL
            .into_iter()
            .filter(|&to| to != from)
            .filter_map(|to| {
                self.converter_for(from, to).map(|(_, fidelity)| Target {
                    format: to,
                    fidelity,
                    score: popularity(from, to),
                })
            })
            .collect();
        targets.sort_by(|a, b| b.score.cmp(&a.score).then(a.format.cmp(&b.format)));
        targets
    }

    pub fn can_convert(&self, from: Format, to: Format) -> bool {
        self.converter_for(from, to).is_some()
    }

    pub fn convert_bytes(
        &self,
        input: &[u8],
        from: Format,
        to: Format,
        options: &ConvertOptions,
    ) -> Result<Vec<u8>> {
        let (converter, _) = self
            .converter_for(from, to)
            .ok_or(Error::Unsupported { from, to })?;
        if input.is_empty() {
            return Err(Error::Empty { format: from });
        }
        converter.convert(input, from, to, options)
    }

    /// Como [`Engine::convert_bytes`] pero devuelve todas las salidas (una por página en un PDF).
    pub fn convert_bytes_pages(
        &self,
        input: &[u8],
        from: Format,
        to: Format,
        options: &ConvertOptions,
    ) -> Result<Vec<Vec<u8>>> {
        let (converter, _) = self
            .converter_for(from, to)
            .ok_or(Error::Unsupported { from, to })?;
        if input.is_empty() {
            return Err(Error::Empty { format: from });
        }
        let mut pages = Vec::new();
        converter.convert_pages(input, from, to, options, &mut |page: Page| {
            pages.push(page.bytes);
            Ok(())
        })?;
        Ok(pages)
    }

    /// Convierte un archivo y devuelve las rutas escritas. Con una sola salida va junto al
    /// original con la extensión nueva (`foto.jpg` → `foto.webp`, o `foto (1).webp` si ya existe);
    /// con varias (páginas de un PDF) van a una carpeta nueva con el nombre del original.
    /// El original no se toca y cada archivo se escribe de forma atómica; si algo falla, no queda
    /// nada a medias.
    pub fn convert_file(
        &self,
        input: &Path,
        to: Format,
        options: &ConvertOptions,
    ) -> Result<Vec<PathBuf>> {
        let bytes = fs::read(input).map_err(|source| Error::Io {
            path: input.to_path_buf(),
            source,
        })?;
        let from = Format::detect(input, &bytes)
            .ok_or_else(|| Error::UnknownFormat(input.to_path_buf()))?;
        let (converter, _) = self
            .converter_for(from, to)
            .ok_or(Error::Unsupported { from, to })?;
        if bytes.is_empty() {
            return Err(Error::Empty { format: from });
        }
        let mut outputs = OutputSet::new(input, to);
        let result =
            converter.convert_pages(&bytes, from, to, options, &mut |page| outputs.write(page));
        match result {
            Ok(()) if outputs.is_empty() => {
                outputs.rollback();
                Err(Error::Decode {
                    format: from,
                    message: "no se obtuvo ninguna salida".into(),
                })
            }
            Ok(()) => Ok(outputs.finish()),
            Err(e) => {
                outputs.rollback();
                Err(e)
            }
        }
    }

    fn converter_for(&self, from: Format, to: Format) -> Option<(&dyn Converter, Fidelity)> {
        self.converters
            .iter()
            .find_map(|c| c.supports(from, to).map(|fidelity| (c.as_ref(), fidelity)))
    }
}

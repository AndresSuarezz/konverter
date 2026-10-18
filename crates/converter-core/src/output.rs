use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::convert::Page;
use crate::error::{Error, Result};
use crate::format::Format;

const MAX_NAME_ATTEMPTS: u32 = 10_000;

/// Destino de una conversión: un archivo junto al original o, si hay varias salidas (las páginas
/// de un PDF), una carpeta nueva con el nombre del original. Nunca sobrescribe nada, y si algo
/// falla a mitad, [`OutputSet::rollback`] deja el disco como estaba.
pub(crate) struct OutputSet {
    source: PathBuf,
    to: Format,
    folder: Option<PathBuf>,
    written: Vec<PathBuf>,
}

impl OutputSet {
    pub fn new(source: &Path, to: Format) -> Self {
        Self {
            source: source.to_path_buf(),
            to,
            folder: None,
            written: Vec::new(),
        }
    }

    pub fn write(&mut self, page: Page) -> Result<()> {
        let path = if page.total <= 1 {
            write_next_to(&self.source, self.to, &page.bytes)?
        } else {
            let folder = match &self.folder {
                Some(folder) => folder.clone(),
                None => {
                    let folder = create_folder(&self.source)?;
                    self.folder = Some(folder.clone());
                    folder
                }
            };
            // Con cero a la izquierda para que el orden alfabético sea el de las páginas.
            let width = page.total.to_string().len();
            let name = format!(
                "{}-{:0width$}.{}",
                stem(&self.source),
                page.index + 1,
                self.to.extension()
            );
            let path = folder.join(name);
            write_atomic(&path, &page.bytes).map_err(|e| io_error(&path, e))?;
            path
        };
        self.written.push(path);
        Ok(())
    }

    /// Borra lo escrito hasta ahora (y la carpeta, si se creó).
    pub fn rollback(self) {
        for file in &self.written {
            let _ = fs::remove_file(file);
        }
        if let Some(folder) = &self.folder {
            let _ = fs::remove_dir(folder);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.written.is_empty()
    }

    pub fn finish(self) -> Vec<PathBuf> {
        self.written
    }
}

fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn stem(source: &Path) -> String {
    source
        .file_stem()
        .map_or_else(|| "archivo".into(), |s| s.to_string_lossy().into_owned())
}

fn directory_of(source: &Path) -> Option<&Path> {
    source.parent().filter(|p| !p.as_os_str().is_empty())
}

/// Escribe `bytes` junto a `source` con la extensión de `to`, sin sobrescribir nada.
///
/// El nombre libre se reserva con `create_new` (a prueba de carreras entre conversiones
/// simultáneas), el contenido se escribe en un temporal del mismo directorio y se renombra
/// sobre la reserva. Si algo falla, no queda ni el temporal ni la reserva.
fn write_next_to(source: &Path, to: Format, bytes: &[u8]) -> Result<PathBuf> {
    let target =
        reserve(directory_of(source), &stem(source), to).map_err(|e| io_error(source, e))?;
    if let Err(e) = write_atomic(&target, bytes) {
        let _ = fs::remove_file(&target);
        return Err(io_error(&target, e));
    }
    Ok(target)
}

/// Temporal en el mismo directorio y renombrado; el temporal se borra si algo falla.
fn write_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = target.with_file_name(format!(
        ".{}.converter-tmp",
        target.file_name().unwrap_or_default().to_string_lossy()
    ));
    let result = write_temp(&temp, bytes).and_then(|()| fs::rename(&temp, target));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Crea (vacío) y cierra el primer archivo libre. El manejador se cierra antes de renombrar
/// encima: en Windows no se puede reemplazar un archivo abierto.
fn reserve(directory: Option<&Path>, stem: &str, to: Format) -> io::Result<PathBuf> {
    for attempt in 0..MAX_NAME_ATTEMPTS {
        let name = if attempt == 0 {
            format!("{stem}.{}", to.extension())
        } else {
            format!("{stem} ({attempt}).{}", to.extension())
        };
        let candidate = directory.map_or_else(|| PathBuf::from(&name), |d| d.join(&name));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no hay un nombre de archivo libre",
    ))
}

/// Carpeta nueva `<nombre>` (o `<nombre> (1)`...) junto al original; `create_dir` falla si ya
/// existe, así que dos conversiones simultáneas nunca comparten carpeta.
fn create_folder(source: &Path) -> Result<PathBuf> {
    let stem = stem(source);
    let directory = directory_of(source);
    for attempt in 0..MAX_NAME_ATTEMPTS {
        let name = if attempt == 0 {
            stem.clone()
        } else {
            format!("{stem} ({attempt})")
        };
        let candidate = directory.map_or_else(|| PathBuf::from(&name), |d| d.join(&name));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(io_error(&candidate, e)),
        }
    }
    Err(io_error(
        source,
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "no hay un nombre de carpeta libre",
        ),
    ))
}

fn write_temp(temp: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::File::create(temp)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(index: usize, total: usize) -> Page {
        Page {
            bytes: format!("p{index}").into_bytes(),
            index,
            total,
        }
    }

    #[test]
    fn picks_the_next_free_name_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("foto.jpg");
        fs::write(&source, b"original").unwrap();
        fs::write(dir.path().join("foto.png"), b"ya existia").unwrap();

        let first = write_next_to(&source, Format::Png, b"nuevo").unwrap();
        let second = write_next_to(&source, Format::Png, b"otro").unwrap();

        assert_eq!(first.file_name().unwrap(), "foto (1).png");
        assert_eq!(second.file_name().unwrap(), "foto (2).png");
        assert_eq!(
            fs::read(dir.path().join("foto.png")).unwrap(),
            b"ya existia"
        );
        assert_eq!(fs::read(&first).unwrap(), b"nuevo");
    }

    #[test]
    fn leaves_no_temporary_files_behind() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("a.csv");
        fs::write(&source, b"x").unwrap();
        write_next_to(&source, Format::Json, b"[]").unwrap();
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().all(|n| !n.contains("converter-tmp")),
            "{names:?}"
        );
    }

    #[test]
    fn keeps_unicode_and_spaces_in_names() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("Año nuevo ñandú.csv");
        fs::write(&source, b"x").unwrap();
        let out = write_next_to(&source, Format::Xlsx, b"data").unwrap();
        assert_eq!(out.file_name().unwrap(), "Año nuevo ñandú.xlsx");
    }

    #[test]
    fn several_pages_go_into_a_new_numbered_folder() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("informe.pdf");
        fs::write(&source, b"pdf").unwrap();
        fs::create_dir(dir.path().join("informe")).unwrap(); // ya existe: no se reutiliza

        let mut set = OutputSet::new(&source, Format::Png);
        for i in 0..12 {
            set.write(page(i, 12)).unwrap();
        }
        let files = set.finish();

        assert_eq!(files.len(), 12);
        assert_eq!(
            files[0],
            dir.path().join("informe (1)").join("informe-01.png")
        );
        assert_eq!(files[11].file_name().unwrap(), "informe-12.png");
        assert_eq!(fs::read(&files[3]).unwrap(), b"p3");
    }

    #[test]
    fn a_single_page_is_written_next_to_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("hoja.pdf");
        fs::write(&source, b"pdf").unwrap();
        let mut set = OutputSet::new(&source, Format::Png);
        set.write(page(0, 1)).unwrap();
        assert_eq!(set.finish(), [dir.path().join("hoja.png")]);
    }

    #[test]
    fn rollback_removes_written_files_and_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("informe.pdf");
        fs::write(&source, b"pdf").unwrap();
        let mut set = OutputSet::new(&source, Format::Png);
        set.write(page(0, 3)).unwrap();
        set.write(page(1, 3)).unwrap();
        set.rollback();
        let left: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, ["informe.pdf"]);
    }
}

//! Componentes que la aplicación descarga bajo demanda y guarda por usuario (hoy: LibreOffice).
//!
//! Está separado de `converter-core` a propósito: el core no abre conexiones de red. Aquí vive
//! todo lo que sí las abre, con tres garantías: la descarga **se reanuda** si se corta, el archivo
//! se **verifica con un SHA-256 fijado en el código** antes de usarlo, y la instalación es
//! **atómica** (se extrae a una carpeta `.partial` y solo se renombra si salió bien), de modo que
//! una instalación a medias nunca es usada por el motor.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

/// Versión de LibreOffice que se instala. Para cambiarla hay que actualizar también la URL, el
/// tamaño y el SHA-256 (se publica junto al instalador, en `<url>.sha256`).
pub const LIBREOFFICE_VERSION: &str = "26.8.1";

#[derive(Debug, thiserror::Error)]
pub enum ComponentError {
    #[error("error de red: {0}")]
    Network(String),
    #[error("no se pudo acceder a {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("la descarga está dañada (SHA-256 distinto del esperado); se descartó")]
    HashMismatch { expected: String, actual: String },
    #[error("no se pudo extraer: {0}")]
    Extract(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("ya hay otra instalación en curso")]
    Busy,
}

/// Avance de una instalación, para mostrarlo en la interfaz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Downloading { done: u64, total: u64 },
    Verifying,
    Extracting,
    Finished,
}

fn io_error(path: &Path, source: io::Error) -> ComponentError {
    ComponentError::Io {
        path: path.to_path_buf(),
        source,
    }
}

// ------------------------------------------------------------- LibreOffice

struct Package {
    url: &'static str,
    size: u64,
    sha256: &'static str,
}

fn libreoffice_package() -> Result<Package, ComponentError> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Ok(Package {
            url: "https://download.documentfoundation.org/libreoffice/stable/26.8.1/win/x86_64/\
                  LibreOffice_26.8.1_Win_x86-64.msi",
            size: 374_870_016,
            sha256: "c6298b10cfa1748bcbedad94c62ce771e240e5021bfd6d7a967cd571b04b5312",
        })
    } else {
        Err(ComponentError::Unsupported(
            "la descarga automática de LibreOffice solo está disponible en Windows x64 por ahora; \
             instálalo con tu gestor de paquetes o define CONVERTER_LIBREOFFICE"
                .into(),
        ))
    }
}

/// ¿Puede la aplicación descargar LibreOffice en este sistema? Hoy solo en Windows x64; en el resto
/// no se ofrece la descarga (se instala con el gestor de paquetes del sistema).
pub fn libreoffice_installable() -> bool {
    libreoffice_package().is_ok()
}

/// Tamaño aproximado de la descarga, en bytes, para avisar al usuario antes de empezar.
pub fn libreoffice_download_size() -> Option<u64> {
    libreoffice_package().ok().map(|p| p.size)
}

/// Instala LibreOffice en el directorio de componentes del usuario y devuelve la ruta de `soffice`.
/// Si ya está instalado, no descarga nada.
pub fn install_libreoffice(
    on_progress: &mut dyn FnMut(Progress),
) -> Result<PathBuf, ComponentError> {
    install_libreoffice_into(&converter_core::components_dir(), on_progress)
}

/// Como [`install_libreoffice`], con carpeta de componentes explícita (para pruebas).
pub fn install_libreoffice_into(
    root: &Path,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<PathBuf, ComponentError> {
    // msiexec solo entiende rutas absolutas (con una relativa falla con el 1619 "no se pudo abrir
    // el paquete"), así que se normaliza una sola vez aquí.
    let root = absolute(root)?;
    let root = root.as_path();
    let package = libreoffice_package()?;
    let exe = if cfg!(windows) {
        "soffice.exe"
    } else {
        "soffice"
    };
    let target = root.join(format!("libreoffice-{LIBREOFFICE_VERSION}"));
    let soffice = target.join("program").join(exe);
    if soffice.is_file() {
        on_progress(Progress::Finished);
        return Ok(soffice);
    }

    let partial = root.join(format!("libreoffice-{LIBREOFFICE_VERSION}.partial"));
    // Antes de descargar 374 MB: si la ruta es demasiado larga, la extracción fallaría al final.
    check_path_length(&partial)?;

    fs::create_dir_all(root).map_err(|e| io_error(root, e))?;
    let _lock = InstallLock::acquire(root)?;

    let archive = root.join(format!("libreoffice-{LIBREOFFICE_VERSION}.msi"));
    let part = root.join(format!("libreoffice-{LIBREOFFICE_VERSION}.msi.part"));
    // Un MSI ya descargado (p. ej. de un intento cuya extracción falló) se reutiliza si su hash
    // es el correcto; si no, se descarta y se baja de nuevo.
    let reusable = archive.is_file() && {
        on_progress(Progress::Verifying);
        sha256_of(&archive).is_ok_and(|actual| actual.eq_ignore_ascii_case(package.sha256))
    };
    if !reusable {
        let _ = fs::remove_file(&archive);
        fetch(
            package.url,
            &part,
            package.size,
            package.sha256,
            on_progress,
        )?;
        fs::rename(&part, &archive).map_err(|e| io_error(&archive, e))?;
    }

    let _ = fs::remove_dir_all(&partial);
    on_progress(Progress::Extracting);
    let log = root.join("install.log");
    let extracted = extract_msi(&archive, &partial, &log)
        // Se poda antes de renombrar: lo que se usa y se guarda ocupa ~690 MB, no 1,5 GB.
        .and_then(|()| prune_libreoffice(&partial).map(drop))
        .and_then(|()| {
            if partial.join("program").join(exe).is_file() {
                Ok(())
            } else {
                Err(ComponentError::Extract(
                    "el paquete no contiene program/soffice".into(),
                ))
            }
        });
    if let Err(e) = extracted {
        let _ = fs::remove_dir_all(&partial);
        return Err(e);
    }
    fs::rename(&partial, &target).map_err(|e| io_error(&target, e))?;
    let _ = fs::remove_file(&archive);
    on_progress(Progress::Finished);
    Ok(soffice)
}

/// Extracción "administrativa" del MSI (`msiexec /a`): descomprime los archivos sin instalar
/// nada en el sistema, sin permisos de administrador y sin tocar el registro.
#[cfg(windows)]
fn extract_msi(msi: &Path, target: &Path, log: &Path) -> Result<(), ComponentError> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    let mut child = Command::new("msiexec")
        .arg("/a")
        .arg(msi)
        .arg("/qn")
        .arg("/L*v")
        .arg(log)
        .arg(format!("TARGETDIR={}", target.display()))
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| ComponentError::Extract(format!("no se pudo ejecutar msiexec: {e}")))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(ComponentError::Extract(explain_msi_failure(
                    status.code(),
                    log,
                )));
            }
            Ok(None) if started.elapsed() > Duration::from_secs(15 * 60) => {
                let _ = child.kill();
                return Err(ComponentError::Extract(
                    "msiexec tardó más de 15 minutos".into(),
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(250)),
            Err(e) => return Err(ComponentError::Extract(e.to_string())),
        }
    }
}

/// La ruta relativa más larga dentro del paquete de LibreOffice (medida: 100 caracteres, en
/// `program\python-core-…\lib\setuptools\…`) más un margen.
const LONGEST_PACKAGE_PATH: usize = 110;
/// Límite clásico de Windows (`MAX_PATH`), con margen para el terminador y el prefijo.
const MAX_PATH: usize = 255;

/// Windows Installer falla con un 1603 genérico si algún archivo pasa de `MAX_PATH`. Se avisa
/// antes de descargar, con una solución concreta.
fn check_path_length(destination: &Path) -> Result<(), ComponentError> {
    if !cfg!(windows) {
        return Ok(());
    }
    let length = destination.as_os_str().len();
    if length + LONGEST_PACKAGE_PATH > MAX_PATH {
        return Err(ComponentError::Unsupported(format!(
            "la carpeta de instalación es demasiado larga ({length} caracteres; Windows admite \
             {MAX_PATH} por archivo y LibreOffice necesita {LONGEST_PACKAGE_PATH} más). Define \
             CONVERTER_COMPONENTS_DIR con una ruta corta, por ejemplo C:\\Converter"
        )));
    }
    Ok(())
}

/// Traduce el código de salida de `msiexec` y añade la línea relevante de su registro.
#[cfg(any(windows, test))]
fn explain_msi_failure(code: Option<i32>, log: &Path) -> String {
    let meaning = match code {
        Some(1603) => "error fatal durante la instalación",
        Some(1618) => "otra instalación de Windows está en curso; vuelve a intentarlo en un minuto",
        Some(1619 | 1620) => "no se pudo abrir el paquete descargado",
        Some(1625 | 1730) => "la directiva del sistema o los permisos lo impiden",
        Some(1601) => "el servicio de Windows Installer no está disponible",
        _ => "error al extraer el paquete",
    };
    let detail = fs::read(log)
        .ok()
        .map(|bytes| {
            // El registro de Windows Installer suele estar en UTF-16.
            if bytes.starts_with(&[0xFF, 0xFE]) {
                let units: Vec<u16> = bytes[2..]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_le_bytes(*pair))
                    .collect();
                String::from_utf16_lossy(&units)
            } else {
                String::from_utf8_lossy(&bytes).into_owned()
            }
        })
        .and_then(|text| {
            text.lines()
                .rfind(|l| {
                    let l = l.to_lowercase();
                    l.contains("error ") || l.contains("return value 3") || l.contains("failed")
                })
                .map(|l| l.trim().chars().take(180).collect::<String>())
        });
    match (code, detail) {
        (Some(code), Some(detail)) => format!("msiexec {code}: {meaning} ({detail})"),
        (Some(code), None) => format!("msiexec {code}: {meaning}"),
        (None, _) => format!("msiexec se interrumpió: {meaning}"),
    }
}

#[cfg(not(windows))]
fn extract_msi(_msi: &Path, _target: &Path, _log: &Path) -> Result<(), ComponentError> {
    Err(ComponentError::Unsupported(
        "los paquetes MSI solo se pueden extraer en Windows".into(),
    ))
}

// ---------------------------------------------------------------- desinstalar

/// ¿Es algo que instaló este gestor para LibreOffice? Cubre la carpeta final, la `.partial` de
/// una extracción a medias, el `.msi`/`.msi.part` descargado y el registro de `msiexec`.
fn is_libreoffice_leftover(name: &str) -> bool {
    name.starts_with("libreoffice-") || name == "install.log"
}

/// Bytes que ocupa lo que este gestor instaló de LibreOffice (0 si no hay nada).
pub fn managed_libreoffice_size() -> u64 {
    managed_libreoffice_size_in(&converter_core::components_dir())
}

pub fn managed_libreoffice_size_in(root: &Path) -> u64 {
    fs::read_dir(root)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| is_libreoffice_leftover(&e.file_name().to_string_lossy()))
                .map(|e| dir_size(&e.path()))
                .sum()
        })
        .unwrap_or(0)
}

/// Quita el LibreOffice que descargó la aplicación (no toca el que venga incluido en ella ni el
/// que el usuario tenga instalado en el sistema). Devuelve los bytes liberados.
pub fn uninstall_libreoffice() -> Result<u64, ComponentError> {
    uninstall_libreoffice_from(&converter_core::components_dir())
}

/// Como [`uninstall_libreoffice`], con carpeta de componentes explícita (para pruebas).
///
/// Solo borra entradas de LibreOffice (`libreoffice-*` e `install.log`); cualquier otra cosa de la
/// carpeta (ajustes, otros componentes) queda intacta. Se niega si hay una instalación en curso y
/// devuelve un error claro si algún archivo está en uso (p. ej. una conversión en marcha).
pub fn uninstall_libreoffice_from(root: &Path) -> Result<u64, ComponentError> {
    if !root.is_dir() {
        return Ok(0);
    }
    let freed = {
        let _lock = InstallLock::acquire(root)?;
        let mut freed = 0;
        for entry in fs::read_dir(root).map_err(|e| io_error(root, e))?.flatten() {
            if !is_libreoffice_leftover(&entry.file_name().to_string_lossy()) {
                continue;
            }
            let path = entry.path();
            let size = dir_size(&path);
            let removed = if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            removed.map_err(|e| io_error(&path, e))?;
            freed += size;
        }
        freed
    };
    // Si la carpeta de componentes quedó vacía, se retira también (no falla si no lo está).
    let _ = fs::remove_dir(root);
    Ok(freed)
}

fn absolute(path: &Path) -> Result<PathBuf, ComponentError> {
    std::path::absolute(path).map_err(|e| io_error(path, e))
}

// -------------------------------------------------------------------- poda

/// Idiomas de interfaz que se conservan; los otros ~120 ocupan unos 260 MB y no se usan al
/// convertir sin ventana.
const KEPT_LOCALES: [&str; 4] = ["es", "en", "en-US", "en-GB"];

fn dir_size(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_dir() {
        fs::read_dir(path)
            .map(|entries| entries.flatten().map(|e| dir_size(&e.path())).sum())
            .unwrap_or(0)
    } else {
        meta.len()
    }
}

fn remove_path(path: &Path) -> u64 {
    let size = dir_size(path);
    let removed = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    if removed.is_ok() { size } else { 0 }
}

/// Quita de una copia de LibreOffice lo que las conversiones sin ventana no necesitan:
/// diccionarios y extensiones, traducciones de la interfaz (salvo español e inglés), temas de
/// iconos, galería, plantillas, asistentes, autotexto y ayuda. Pasa de ~1,5 GB a ~690 MB y las
/// conversiones (DOCX/DOC/MD ↔ PDF) siguen funcionando: se comprobó con las pruebas reales.
///
/// Se **conservan** `program/`, `share/registry` y los avisos de licencia (`readmes/`), que la
/// licencia de LibreOffice (MPL 2.0) exige distribuir. Devuelve los bytes liberados.
pub fn prune_libreoffice(root: &Path) -> Result<u64, ComponentError> {
    let exe = if cfg!(windows) {
        "soffice.exe"
    } else {
        "soffice"
    };
    if !root.join("program").join(exe).is_file() {
        return Err(ComponentError::Extract(format!(
            "{} no parece una copia de LibreOffice (falta program/{exe}); no se poda",
            root.display()
        )));
    }
    let mut freed = 0;

    // Extensiones (diccionarios): se vacía la carpeta, no se borra.
    if let Ok(entries) = fs::read_dir(root.join("share/extensions")) {
        freed += entries
            .flatten()
            .map(|e| remove_path(&e.path()))
            .sum::<u64>();
    }
    if let Ok(entries) = fs::read_dir(root.join("program/resource")) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if entry.path().is_dir() && !KEPT_LOCALES.contains(&name.to_string_lossy().as_ref()) {
                freed += remove_path(&entry.path());
            }
        }
    }
    if let Ok(entries) = fs::read_dir(root.join("share/config")) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("images_") && name.ends_with(".zip") {
                freed += remove_path(&entry.path());
            }
        }
    }
    for dir in [
        "share/gallery",
        "share/template",
        "share/wizards",
        "share/autotext",
        "help",
    ] {
        freed += remove_path(&root.join(dir));
    }
    Ok(freed)
}

// ------------------------------------------------------------------ bloqueo

/// Archivo de bloqueo: evita dos instalaciones simultáneas del mismo componente. Un bloqueo de
/// más de una hora se considera abandonado (la aplicación se cerró a medias).
struct InstallLock(PathBuf);

impl InstallLock {
    fn acquire(root: &Path) -> Result<Self, ComponentError> {
        let path = root.join(".install.lock");
        for _ in 0..2 {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(Self(path)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    let stale = fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age > Duration::from_secs(3600));
                    if !stale {
                        return Err(ComponentError::Busy);
                    }
                    let _ = fs::remove_file(&path);
                }
                Err(e) => return Err(io_error(&path, e)),
            }
        }
        Err(ComponentError::Busy)
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

// ----------------------------------------------------------------- descarga

/// Descarga `url` en `part` (reanudando si ya hay una parte) y comprueba el SHA-256. Si el hash
/// no coincide, borra el archivo y devuelve [`ComponentError::HashMismatch`].
///
/// `size` es el tamaño exacto esperado: permite saber cuándo está completo, reanudar con
/// `Range` y mostrar el avance. Los fallos de red se reintentan con espera creciente (hasta 20),
/// y cada reintento continúa donde quedó.
pub fn fetch(
    url: &str,
    part: &Path,
    size: u64,
    sha256: &str,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<(), ComponentError> {
    fetch_with(url, part, size, sha256, RetryPolicy::default(), on_progress)
}

/// Cuántos fallos seguidos se toleran y cuánto se espera entre reintentos.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_failures: u32,
    /// Espera base en milisegundos; crece con cada fallo seguido (hasta 12 veces).
    pub backoff_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_failures: 20,
            backoff_ms: 500,
        }
    }
}

/// Como [`fetch`], con política de reintentos explícita.
pub fn fetch_with(
    url: &str,
    part: &Path,
    size: u64,
    sha256: &str,
    policy: RetryPolicy,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<(), ComponentError> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            // Por intento, no total: si la conexión se arrastra, se corta y se reanuda.
            .timeout_recv_body(Some(Duration::from_secs(600)))
            .build(),
    );
    let mut failures = 0u32;
    let mut last_report = Instant::now() - Duration::from_secs(1);
    loop {
        let mut offset = fs::metadata(part).map_or(0, |m| m.len());
        if offset > size {
            // Más grande que lo esperado: está corrupta, se empieza de cero.
            fs::remove_file(part).map_err(|e| io_error(part, e))?;
            offset = 0;
        }
        if offset == size {
            break;
        }

        let mut request = agent.get(url);
        if offset > 0 {
            request = request.header("Range", format!("bytes={offset}-"));
        }
        let outcome = request
            .call()
            .map_err(|e| e.to_string())
            .and_then(|response| {
                match response.status().as_u16() {
                    206 => Ok((response, true)),
                    // El servidor ignoró el Range: se vuelve a empezar desde cero.
                    200 => Ok((response, false)),
                    416 => Err("416".to_string()),
                    other => Err(format!("el servidor respondió {other}")),
                }
            });

        match outcome {
            Ok((response, resumed)) => {
                let mut file = if resumed {
                    OpenOptions::new().append(true).open(part)
                } else {
                    offset = 0;
                    File::create(part)
                }
                .map_err(|e| io_error(part, e))?;
                let mut done = offset;
                let mut reader = response.into_body().into_reader();
                let mut buffer = vec![0u8; 256 * 1024];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => {
                            file.write_all(&buffer[..n])
                                .map_err(|e| io_error(part, e))?;
                            done += n as u64;
                            failures = 0; // hay avance: el contador cuenta fallos seguidos
                            if last_report.elapsed() > Duration::from_millis(250) {
                                on_progress(Progress::Downloading { done, total: size });
                                last_report = Instant::now();
                            }
                        }
                        Err(_) => {
                            failures += 1;
                            break;
                        }
                    }
                }
                file.flush().map_err(|e| io_error(part, e))?;
                if done == offset && failures == 0 {
                    // Respuesta vacía sin avance: se cuenta como fallo para no girar sin fin.
                    failures += 1;
                }
            }
            Err(message) => {
                if message == "416" {
                    // Rango imposible: la parte local no encaja con el servidor.
                    let _ = fs::remove_file(part);
                }
                failures += 1;
                if failures >= policy.max_failures {
                    return Err(ComponentError::Network(message));
                }
            }
        }
        if failures >= policy.max_failures {
            return Err(ComponentError::Network(
                "la conexión se cortó demasiadas veces".into(),
            ));
        }
        if failures > 0 {
            std::thread::sleep(Duration::from_millis(
                policy.backoff_ms * u64::from(failures.min(12)),
            ));
        }
    }
    on_progress(Progress::Downloading {
        done: size,
        total: size,
    });

    on_progress(Progress::Verifying);
    let actual = sha256_of(part)?;
    if !actual.eq_ignore_ascii_case(sha256) {
        let _ = fs::remove_file(part);
        return Err(ComponentError::HashMismatch {
            expected: sha256.to_string(),
            actual,
        });
    }
    Ok(())
}

fn sha256_of(path: &Path) -> Result<String, ComponentError> {
    let mut file = File::open(path).map_err(|e| io_error(path, e))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(|e| io_error(path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libreoffice_is_downloadable_only_where_a_package_is_pinned() {
        // Un paquete fijado (URL, tamaño, SHA-256) solo existe para Windows x64.
        assert_eq!(
            libreoffice_installable(),
            cfg!(all(windows, target_arch = "x86_64"))
        );
        assert_eq!(
            libreoffice_installable(),
            libreoffice_download_size().is_some()
        );
    }

    #[test]
    fn sha256_matches_the_known_digest_of_abc() {
        let dir = std::env::temp_dir().join(format!("components-sha-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("abc");
        fs::write(&file, b"abc").unwrap();
        assert_eq!(
            sha256_of(&file).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    #[cfg(windows)]
    fn a_too_long_install_path_is_rejected_before_downloading_anything() {
        // Era el caso real del error 1603: una ruta de 169 caracteres + 100 del paquete > 260.
        let long = PathBuf::from(format!("C:\\{}\\componentes", "x".repeat(160)));
        let mut events = Vec::new();
        let err = install_libreoffice_into(&long, &mut |p| events.push(p)).unwrap_err();
        let message = err.to_string();
        assert!(matches!(err, ComponentError::Unsupported(_)), "{message}");
        assert!(
            message.contains("demasiado larga") && message.contains("CONVERTER_COMPONENTS_DIR")
        );
        assert!(events.is_empty(), "no empezó a descargar: {events:?}");
        assert!(!long.exists(), "ni siquiera creó la carpeta");
    }

    #[test]
    #[cfg(windows)]
    fn the_default_user_location_fits_even_with_a_long_user_name() {
        let typical = Path::new(
            r"C:\Users\anfes\AppData\Local\Converter\components\libreoffice-26.8.1.partial",
        );
        assert!(check_path_length(typical).is_ok());
        let long_user = Path::new(
            r"C:\Users\nombre.apellido.muy.largo.de.usuario\AppData\Local\Converter\components\libreoffice-26.8.1.partial",
        );
        assert!(
            check_path_length(long_user).is_ok(),
            "{}",
            long_user.as_os_str().len()
        );
    }

    fn fake_libreoffice(root: &Path) {
        let exe = if cfg!(windows) {
            "soffice.exe"
        } else {
            "soffice"
        };
        let write = |rel: &str, bytes: usize| {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, vec![0u8; bytes]).unwrap();
        };
        write(&format!("program/{exe}"), 10);
        write("program/mergedlo.dll", 1000);
        for locale in ["es", "en-US", "de", "ja", "zh-CN"] {
            write(&format!("program/resource/{locale}/LC_MESSAGES/x.mo"), 100);
        }
        write("share/registry/main.xcd", 50);
        write("share/extensions/dict-de/de.aff", 400);
        write("share/extensions/dict-es/es.dic", 300);
        write("share/config/images_colibre.zip", 200);
        write("share/config/soffice.cfg/keep.xml", 5);
        write("share/gallery/a.png", 20);
        write("share/template/t.ott", 20);
        write("help/h.html", 20);
        write("readmes/LICENSE", 7);
    }

    #[test]
    fn pruning_removes_what_conversions_do_not_use_and_keeps_what_they_need() {
        let dir = tempfile::tempdir().unwrap();
        fake_libreoffice(dir.path());
        let before = dir_size(dir.path());

        let freed = prune_libreoffice(dir.path()).unwrap();

        // Lo que debe seguir ahí para convertir y para cumplir la licencia:
        let exe = if cfg!(windows) {
            "soffice.exe"
        } else {
            "soffice"
        };
        for kept in [
            format!("program/{exe}"),
            "program/mergedlo.dll".into(),
            "program/resource/es/LC_MESSAGES/x.mo".into(),
            "program/resource/en-US/LC_MESSAGES/x.mo".into(),
            "share/registry/main.xcd".into(),
            "share/config/soffice.cfg/keep.xml".into(),
            "readmes/LICENSE".into(),
        ] {
            assert!(dir.path().join(&kept).exists(), "se borró {kept}");
        }
        // Lo que sobra:
        for gone in [
            "program/resource/de",
            "program/resource/ja",
            "program/resource/zh-CN",
            "share/extensions/dict-de",
            "share/extensions/dict-es",
            "share/config/images_colibre.zip",
            "share/gallery",
            "share/template",
            "help",
        ] {
            assert!(!dir.path().join(gone).exists(), "no se borró {gone}");
        }
        assert!(
            dir.path().join("share/extensions").is_dir(),
            "la carpeta se vacía, no se elimina"
        );
        assert_eq!(
            freed,
            before - dir_size(dir.path()),
            "los bytes liberados cuadran"
        );
        assert_eq!(freed, 100 * 3 + 400 + 300 + 200 + 20 + 20 + 20);
    }

    #[test]
    fn pruning_something_that_is_not_libreoffice_is_refused_and_touches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("share/extensions")).unwrap();
        fs::write(dir.path().join("share/extensions/importante.txt"), b"x").unwrap();
        assert!(prune_libreoffice(dir.path()).is_err());
        assert!(dir.path().join("share/extensions/importante.txt").exists());
    }

    fn put(root: &Path, rel: &str, bytes: usize) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![1u8; bytes]).unwrap();
    }

    #[test]
    fn uninstalling_removes_only_libreoffice_and_reports_the_freed_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("components");
        put(&root, "libreoffice-26.8.1/program/soffice.exe", 1000);
        put(&root, "libreoffice-26.8.1.partial/program/x.dll", 200);
        put(&root, "libreoffice-26.8.1.msi", 300);
        put(&root, "libreoffice-26.8.1.msi.part", 40);
        put(&root, "install.log", 5);
        put(&root, "tesseract-5.5/tesseract.exe", 77); // otro componente: no se toca

        assert_eq!(
            managed_libreoffice_size_in(&root),
            1000 + 200 + 300 + 40 + 5
        );
        let freed = uninstall_libreoffice_from(&root).unwrap();

        assert_eq!(freed, 1545);
        assert!(
            root.join("tesseract-5.5/tesseract.exe").exists(),
            "no toca otros componentes"
        );
        assert_eq!(managed_libreoffice_size_in(&root), 0);
        assert!(
            root.is_dir(),
            "la carpeta sigue porque aún tiene otro componente"
        );
    }

    #[test]
    fn an_emptied_components_folder_is_removed_and_a_missing_one_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("components");
        put(&root, "libreoffice-26.8.1/program/soffice.exe", 10);
        uninstall_libreoffice_from(&root).unwrap();
        assert!(!root.exists(), "no deja una carpeta vacía");
        assert_eq!(
            uninstall_libreoffice_from(&root).unwrap(),
            0,
            "ya no hay nada"
        );
    }

    #[test]
    fn uninstalling_is_refused_while_an_installation_is_running() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("components");
        put(&root, "libreoffice-26.8.1.partial/a.dll", 10);
        let _installing = InstallLock::acquire(&root).unwrap();
        assert!(matches!(
            uninstall_libreoffice_from(&root),
            Err(ComponentError::Busy)
        ));
        assert!(
            root.join("libreoffice-26.8.1.partial/a.dll").exists(),
            "no borró nada"
        );
    }

    #[test]
    fn relative_roots_are_made_absolute_because_msiexec_needs_that() {
        let made = absolute(Path::new("vendor/bin/bundle")).unwrap();
        assert!(made.is_absolute(), "{}", made.display());
        assert!(made.ends_with("vendor/bin/bundle") || made.ends_with(r"vendor\bin\bundle"));
    }

    #[test]
    fn msi_exit_codes_become_readable_messages() {
        let missing = Path::new("no-existe.log");
        assert!(explain_msi_failure(Some(1603), missing).contains("1603: error fatal"));
        assert!(explain_msi_failure(Some(1618), missing).contains("otra instalación"));
        assert!(explain_msi_failure(None, missing).contains("interrumpió"));
    }

    #[test]
    fn the_last_error_line_of_the_msi_log_is_included() {
        let dir = std::env::temp_dir().join(format!("components-log-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("install.log");
        // Formato real: UTF-16 con BOM.
        let text = "MSI (s): todo bien\r\nError 1327. Unidad no válida: Z:\\\r\nMSI (s): fin\r\n";
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        fs::write(&log, bytes).unwrap();
        let message = explain_msi_failure(Some(1603), &log);
        assert!(
            message.contains("Error 1327. Unidad no válida"),
            "{message}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_second_installation_is_refused_while_the_lock_is_held() {
        let dir = std::env::temp_dir().join(format!("components-lock-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let first = InstallLock::acquire(&dir).unwrap();
        assert!(matches!(
            InstallLock::acquire(&dir),
            Err(ComponentError::Busy)
        ));
        drop(first);
        assert!(InstallLock::acquire(&dir).is_ok(), "se libera al terminar");
        fs::remove_dir_all(&dir).unwrap();
    }
}

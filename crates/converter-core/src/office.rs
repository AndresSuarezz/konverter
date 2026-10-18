//! Documentos de Office (DOCX/DOC/Markdown → PDF, DOC → DOCX, PDF → DOCX) con un motor externo:
//! **WordCraft** (Rust puro, 35 MB, el que viaja con la aplicación), LibreOffice (cualquier
//! sistema) o Microsoft Word (Windows, automatización COM a través de PowerShell).
//!
//! El motor se elige con [`DocumentEngine`] **por par de formatos**: en modo automático, WordCraft
//! si sabe hacerlo (décimas de segundo frente a ~20 s de LibreOffice); para lo que no sabe
//! (PDF → DOCX), LibreOffice y, por último, Word. Sin ninguno se devuelve
//! [`Error::MissingDependency`]. Ver `docs/REFERENCIAS-CRAFT.md` para las mediciones.

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::convert::{ConvertOptions, Converter, DocumentEngine, Fidelity};
use crate::error::{Error, Result};
use crate::format::Format;
use crate::markdown;
use crate::md_docx;
use crate::tabular::decode_text;

/// Variable de entorno que cambia la carpeta de componentes (instalaciones portables y pruebas).
pub const COMPONENTS_DIR_ENV: &str = "CONVERTER_COMPONENTS_DIR";

/// Variable de entorno con la ruta de `soffice` (o de la carpeta `program` de LibreOffice).
pub const LIBREOFFICE_ENV: &str = "CONVERTER_LIBREOFFICE";

/// Variable de entorno con la ruta de `wordcraft-cli` (o de la carpeta que lo contiene).
pub const WORDCRAFT_ENV: &str = "CONVERTER_WORDCRAFT";

/// Tiempo máximo de una conversión; pasado, se mata el motor para no dejar procesos colgados.
const TIMEOUT: Duration = Duration::from_secs(180);

#[cfg(windows)]
const WORD_SCRIPT: &str = include_str!("word_convert.ps1");

/// Conversiones de documentos de Office.
///
/// DOCX/DOC → PDF reproduce el documento tal como lo maquetaría el motor (las fuentes que falten
/// en el equipo se sustituyen). PDF → DOCX *reconstruye* un documento editable a partir de la
/// maquetación: es una aproximación (y un escaneo, sin texto, sale como imágenes).
pub struct OfficeConverter;

impl Converter for OfficeConverter {
    fn supports(&self, from: Format, to: Format) -> Option<Fidelity> {
        match (from, to) {
            (Format::Docx | Format::Doc | Format::Markdown, Format::Pdf)
            | (Format::Doc, Format::Docx) => Some(Fidelity::Lossless),
            (Format::Pdf, Format::Docx) => Some(Fidelity::Lossy),
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
        if self.supports(from, to).is_none() {
            return Err(Error::Unsupported { from, to });
        }
        let engine = pick_engine(options.document_engine, from, to)?;
        // Markdown no lo entiende ningún motor: se convierte antes (Rust puro, seguro). Con
        // WordCraft, a DOCX, porque su importador de HTML ignora los anchos de las columnas de las
        // tablas y el de DOCX los respeta; con LibreOffice y Word, a HTML, que imprimen bien.
        let (kind, payload) = match from {
            Format::Markdown if matches!(engine, Engine::WordCraft(_)) => (
                Input::Docx,
                md_docx::to_docx(&decode_text(input), options.pdf_paper)?,
            ),
            Format::Markdown => (
                Input::Html,
                markdown::to_html(&decode_text(input)).into_bytes(),
            ),
            Format::Docx => (Input::Docx, input.to_vec()),
            Format::Doc => (Input::Doc, input.to_vec()),
            _ => (Input::Pdf, input.to_vec()),
        };
        let dir = tempfile::tempdir().map_err(|e| failure(engine.name(), e))?;
        // Nombres ASCII fijos: el motor no tiene que lidiar con tildes ni espacios de la ruta.
        let source = dir.path().join(format!("input.{}", kind.extension()));
        fs::write(&source, payload).map_err(|e| failure(engine.name(), e))?;

        let output = match &engine {
            Engine::WordCraft(exe) => run_wordcraft(exe, dir.path(), &source, to)?,
            #[cfg(windows)]
            Engine::Word => run_word(dir.path(), &source, to)?,
            Engine::LibreOffice(soffice) => {
                run_libreoffice(soffice, dir.path(), &source, kind, to)?
            }
            #[cfg(not(windows))]
            Engine::Word => unreachable!("Word solo se elige en Windows"),
        };
        let bytes = fs::read(&output).map_err(|e| {
            failure(
                engine.name(),
                format!("no generó el archivo de salida: {e}"),
            )
        })?;
        if bytes.is_empty() {
            return Err(failure(engine.name(), "generó un archivo vacío"));
        }
        Ok(bytes)
    }
}

fn failure(tool: &'static str, message: impl ToString) -> Error {
    Error::External {
        tool,
        message: message.to_string(),
    }
}

/// Qué se le entrega al motor (no es lo mismo que el formato de origen: Markdown llega como HTML).
#[derive(Clone, Copy)]
enum Input {
    Docx,
    Doc,
    Pdf,
    Html,
}

impl Input {
    fn extension(self) -> &'static str {
        match self {
            Input::Docx => "docx",
            Input::Doc => "doc",
            Input::Pdf => "pdf",
            Input::Html => "html",
        }
    }
}

// ----------------------------------------------------------- elección de motor

enum Engine {
    WordCraft(PathBuf),
    Word,
    LibreOffice(PathBuf),
}

impl Engine {
    fn name(&self) -> &'static str {
        match self {
            Engine::WordCraft(_) => "WordCraft",
            Engine::Word => "Microsoft Word",
            Engine::LibreOffice(_) => "LibreOffice",
        }
    }
}

/// Lo que WordCraft sabe hacer. PDF → DOCX **no**: no lee PDF, y el exportador de PdfCraft que
/// lo intenta salió peor que LibreOffice en las pruebas (ver `docs/REFERENCIAS-CRAFT.md`).
fn wordcraft_supports(from: Format, to: Format) -> bool {
    matches!(
        (from, to),
        (Format::Docx | Format::Doc | Format::Markdown, Format::Pdf) | (Format::Doc, Format::Docx)
    )
}

/// Los programas que hay ahora mismo (separado de [`choose_engine`] para poder probar la elección).
struct Available {
    wordcraft: Option<PathBuf>,
    libreoffice: Option<PathBuf>,
    word: bool,
}

/// El motor para un par de formatos. `Auto` y `WordCraft` van por el mismo camino: WordCraft si
/// sabe hacerlo y está; si no (PDF → DOCX, o no está), LibreOffice y después Word. Elegir
/// LibreOffice o Word a mano los fuerza.
fn choose_engine(
    preference: DocumentEngine,
    from: Format,
    to: Format,
    available: &Available,
) -> Option<Engine> {
    let wordcraft = || {
        available
            .wordcraft
            .clone()
            .filter(|_| wordcraft_supports(from, to))
            .map(Engine::WordCraft)
    };
    let libreoffice = || available.libreoffice.clone().map(Engine::LibreOffice);
    let word = || available.word.then_some(Engine::Word);
    match preference {
        DocumentEngine::Auto | DocumentEngine::WordCraft => {
            wordcraft().or_else(libreoffice).or_else(word)
        }
        DocumentEngine::Word => word(),
        DocumentEngine::LibreOffice => libreoffice(),
    }
}

fn pick_engine(preference: DocumentEngine, from: Format, to: Format) -> Result<Engine> {
    let available = Available {
        wordcraft: find_wordcraft(),
        libreoffice: find_libreoffice(),
        word: word_available(),
    };
    choose_engine(preference, from, to, &available).ok_or_else(|| {
        let by_wordcraft = wordcraft_supports(from, to);
        let (name, detail) = match preference {
            DocumentEngine::Word => (
                "Microsoft Word",
                format!(
                    "se necesita para convertir documentos de Office. {}",
                    libreoffice_install_hint()
                ),
            ),
            DocumentEngine::LibreOffice => (
                "LibreOffice",
                format!(
                    "se necesita para convertir documentos de Office. {}",
                    libreoffice_install_hint()
                ),
            ),
            // El nombre contiene «LibreOffice»: la paleta ofrece descargarlo donde se puede.
            _ if by_wordcraft => (
                "WordCraft o LibreOffice",
                format!(
                    "WordCraft viene incluido con la aplicación (o define {WORDCRAFT_ENV} con la \
                     ruta de wordcraft-cli). Alternativa: {}",
                    libreoffice_install_hint()
                ),
            ),
            // Word solo existe en Windows (se automatiza con COM/PowerShell).
            _ if cfg!(windows) => (
                "Microsoft Word o LibreOffice",
                format!(
                    "se necesita para convertir PDF a Word. {}",
                    libreoffice_install_hint()
                ),
            ),
            _ => (
                "LibreOffice",
                format!(
                    "se necesita para convertir PDF a Word. {}",
                    libreoffice_install_hint()
                ),
            ),
        };
        Error::MissingDependency { name, detail }
    })
}

/// Cómo conseguir LibreOffice en este sistema, para los mensajes de «falta LibreOffice». Solo
/// Windows puede descargarlo la propia aplicación; en el resto se instala con el sistema.
pub fn libreoffice_install_hint() -> String {
    if cfg!(windows) {
        format!(
            "Instala LibreOffice (gratuito; la aplicación puede descargarlo) o define \
             {LIBREOFFICE_ENV} con la ruta de soffice"
        )
    } else if cfg!(target_os = "macos") {
        format!(
            "Instala LibreOffice desde https://www.libreoffice.org (o con `brew install --cask \
             libreoffice`) o define {LIBREOFFICE_ENV} con la ruta de soffice"
        )
    } else {
        format!(
            "Instala LibreOffice con tu gestor de paquetes (por ejemplo `sudo apt install \
             libreoffice-writer`) o define {LIBREOFFICE_ENV} con la ruta de soffice"
        )
    }
}

/// Word está instalado si existe el ProgID `Word.Application` en el registro.
fn word_available() -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::sync::OnceLock;
        static AVAILABLE: OnceLock<bool> = OnceLock::new();
        *AVAILABLE.get_or_init(|| {
            Command::new("reg")
                .args(["query", r"HKCR\Word.Application", "/ve"])
                .creation_flags(CREATE_NO_WINDOW)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        })
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Busca `soffice`: variable de entorno, ubicaciones habituales y el `PATH`.
pub fn find_libreoffice() -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        "soffice.exe"
    } else {
        "soffice"
    };
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(path) = std::env::var_os(LIBREOFFICE_ENV) {
        let path = PathBuf::from(path);
        candidates.push(if path.is_dir() { path.join(exe) } else { path });
    }
    candidates.extend(bundled_libreoffice(exe));
    candidates.extend(managed_libreoffice(exe));
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(base) = std::env::var_os(var) {
            candidates.push(PathBuf::from(base).join("LibreOffice/program").join(exe));
        }
    }
    candidates.push("/Applications/LibreOffice.app/Contents/MacOS/soffice".into());
    candidates.push("/usr/bin/soffice".into());
    candidates.push("/usr/local/bin/soffice".into());
    candidates.push("/opt/libreoffice/program/soffice".into());
    if let Some(paths) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&paths).map(|dir| dir.join(exe)));
    }
    candidates.into_iter().find(|c| c.is_file())
}

/// LibreOffice que viaja **dentro** de la aplicación (instalador completo): junto al ejecutable
/// en Windows y Linux, y dentro de `Contents/Resources` en el paquete `.app` de macOS.
fn bundled_libreoffice(exe: &str) -> Vec<PathBuf> {
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
    else {
        return Vec::new();
    };
    vec![
        dir.join("libreoffice/program").join(exe),
        // Linux (.deb, .rpm, AppImage): Tauri deja los recursos en `lib/<productName>/`.
        dir.join("../lib/Converter/libreoffice/program").join(exe),
        dir.join("../Resources/libreoffice/program").join(exe),
        dir.join("../Resources/LibreOffice.app/Contents/MacOS/soffice"),
    ]
}

/// Copias de LibreOffice que gestiona la aplicación (`<componentes>/libreoffice-<versión>`),
/// la más reciente primero.
fn managed_libreoffice(exe: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(components_dir()) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                // `.partial` es una instalación a medias: nunca se usa.
                .is_some_and(|n| n.starts_with("libreoffice-") && !n.ends_with(".partial"))
        })
        .collect();
    dirs.sort();
    dirs.reverse();
    dirs.into_iter()
        .map(|d| d.join("program").join(exe))
        .collect()
}

/// Dónde se instalan los componentes descargados por la aplicación (LibreOffice y, más adelante,
/// Tesseract): datos por usuario, sin permisos de administrador.
pub fn components_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(COMPONENTS_DIR_ENV).filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    let from_env = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    #[cfg(windows)]
    let base = from_env("LOCALAPPDATA").map(|p| p.join("Converter"));
    #[cfg(target_os = "macos")]
    let base = from_env("HOME").map(|p| p.join("Library/Application Support/Converter"));
    #[cfg(not(any(windows, target_os = "macos")))]
    let base = from_env("XDG_DATA_HOME")
        .or_else(|| from_env("HOME").map(|p| p.join(".local/share")))
        .map(|p| p.join("converter"));
    base.unwrap_or_else(|| std::env::temp_dir().join("converter"))
        .join("components")
}

/// Qué programas de documentos hay disponibles ahora mismo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentEngines {
    /// Ruta de `wordcraft-cli` (el que viaja con la aplicación, o el de `CONVERTER_WORDCRAFT`).
    pub wordcraft: Option<PathBuf>,
    /// Ruta de `soffice` si hay un LibreOffice utilizable (gestionado, del sistema o por variable).
    pub libreoffice: Option<PathBuf>,
    pub word: bool,
}

pub fn document_engines() -> DocumentEngines {
    DocumentEngines {
        wordcraft: find_wordcraft(),
        libreoffice: find_libreoffice(),
        word: word_available(),
    }
}

// -------------------------------------------------------------- WordCraft

/// Busca `wordcraft-cli`: la variable de entorno y las carpetas donde lo deja cada paquete. **No
/// mira el `PATH`**: otro `wordcraft-cli` de una versión distinta podría comportarse distinto de
/// la que probamos (la fijada en `scripts/build-wordcraft.ps1`).
pub fn find_wordcraft() -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        "wordcraft-cli.exe"
    } else {
        "wordcraft-cli"
    };
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(path) = std::env::var_os(WORDCRAFT_ENV).filter(|v| !v.is_empty()) {
        let path = PathBuf::from(path);
        candidates.push(if path.is_dir() { path.join(exe) } else { path });
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
    {
        candidates.extend(packaged_wordcraft(&dir, exe));
        // En desarrollo, el que deja scripts/build-wordcraft.ps1 en vendor/bin.
        #[cfg(debug_assertions)]
        candidates.push(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../vendor/bin/wordcraft")
                .join(exe),
        );
    }
    candidates.into_iter().find(|c| c.is_file())
}

/// Dónde deja cada paquete `wordcraft-cli`, relativo a la carpeta del ejecutable (como pdfium).
fn packaged_wordcraft(exe_dir: &Path, exe: &str) -> Vec<PathBuf> {
    vec![
        exe_dir.join("wordcraft").join(exe),
        exe_dir.join("../Resources/wordcraft").join(exe),
        exe_dir.join("../lib/Converter/wordcraft").join(exe),
        exe_dir.join(exe),
    ]
}

/// `wordcraft-cli convert entrada salida`: el formato sale de las extensiones.
fn run_wordcraft(exe: &Path, dir: &Path, source: &Path, to: Format) -> Result<PathBuf> {
    let output = dir.join(format!("output.{}", to.extension()));
    let mut command = Command::new(exe);
    command
        .arg("convert")
        .arg(source)
        .arg(&output)
        // Sin esto escribiría preferencias y un registro en el perfil del usuario; con esto, solo
        // a stderr (y sin el texto del documento).
        .env("WORDCRAFT_NO_PREFS", "1");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    run_tool(command, "WordCraft")?;
    Ok(output)
}

// ------------------------------------------------------------------- Word

#[cfg(windows)]
fn run_word(dir: &Path, source: &Path, to: Format) -> Result<PathBuf> {
    use std::os::windows::process::CommandExt;

    let script = dir.join("word_convert.ps1");
    fs::write(&script, WORD_SCRIPT).map_err(|e| failure("Microsoft Word", e))?;
    let output = dir.join(format!("output.{}", to.extension()));
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .arg("-InPath")
        .arg(source)
        .arg("-OutPath")
        .arg(&output)
        .arg("-To")
        .arg(to.extension())
        .creation_flags(CREATE_NO_WINDOW);
    run_tool(command, "Microsoft Word")?;
    Ok(output)
}

// ------------------------------------------------------------ LibreOffice

fn run_libreoffice(
    soffice: &Path,
    dir: &Path,
    source: &Path,
    kind: Input,
    to: Format,
) -> Result<PathBuf> {
    // Perfil propio: con el perfil del usuario, un LibreOffice ya abierto bloquearía la conversión.
    let profile = dir.join("profile");
    let out_dir = dir.join("out");
    fs::create_dir_all(&out_dir).map_err(|e| failure("LibreOffice", e))?;

    let mut command = Command::new(soffice);
    command.args([
        "--headless",
        "--norestore",
        "--nolockcheck",
        "--nodefault",
        "--nologo",
    ]);
    command.arg(format!("-env:UserInstallation={}", file_url(&profile)));
    match kind {
        // Sin esto LibreOffice abre los PDF en Draw, que no sabe exportar a DOCX.
        Input::Pdf => {
            command.arg("--infilter=writer_pdf_import");
        }
        // Writer (páginas) y no Writer/Web: así el PDF sale paginado y no como una hoja continua.
        Input::Html => {
            command.arg("--infilter=HTML (StarWriter)");
        }
        Input::Docx | Input::Doc => {}
    }
    command
        .arg("--convert-to")
        .arg(match to {
            Format::Pdf => "pdf:writer_pdf_Export",
            _ => "docx:MS Word 2007 XML",
        })
        .arg("--outdir")
        .arg(&out_dir)
        .arg(source);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    run_tool(command, "LibreOffice")?;

    // LibreOffice nombra la salida como la entrada con la extensión nueva.
    Ok(out_dir.join(format!("input.{}", to.extension())))
}

/// `file:///C:/ruta/con%20espacios` (Windows) o `file:///ruta` (Unix).
fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut url = String::from("file://");
    if !text.starts_with('/') {
        url.push('/');
    }
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'-' | b'_' | b'.' | b'~' => {
                url.push(char::from(byte));
            }
            other => url.push_str(&format!("%{other:02X}")),
        }
    }
    url
}

// -------------------------------------------------------- proceso externo

/// Ejecuta el motor con tiempo máximo. Si se pasa, mata el proceso (y los de Word que anunció
/// el script) para no dejar un `WINWORD.EXE` invisible consumiendo memoria.
fn run_tool(mut command: Command, tool: &'static str) -> Result<()> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| failure(tool, e))?;

    let pids = Arc::new(Mutex::new(Vec::<u32>::new()));
    let stdout = child.stdout.take().map(|out| {
        let pids = Arc::clone(&pids);
        thread::spawn(move || {
            for line in BufReader::new(out)
                .lines()
                .map_while(std::result::Result::ok)
            {
                if let Some(pid) = line
                    .strip_prefix("WORD_PID=")
                    .and_then(|p| p.trim().parse().ok())
                    && let Ok(mut pids) = pids.lock()
                {
                    pids.push(pid);
                }
            }
        })
    });
    let stderr = child.stderr.take().map(|mut err| {
        thread::spawn(move || {
            let mut text = String::new();
            let _ = err.read_to_string(&mut text);
            text
        })
    });

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() > TIMEOUT => break None,
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(failure(tool, e)),
        }
    };

    let Some(status) = status else {
        kill_tree(&mut child);
        for pid in pids.lock().map(|p| p.clone()).unwrap_or_default() {
            kill_pid(pid);
        }
        return Err(failure(
            tool,
            format!("no respondió en {} s y se canceló", TIMEOUT.as_secs()),
        ));
    };
    if let Some(handle) = stdout {
        let _ = handle.join();
    }
    let stderr_text = stderr
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    if status.success() {
        Ok(())
    } else {
        let detail = stderr_text.trim();
        Err(failure(
            tool,
            if detail.is_empty() {
                format!("terminó con error ({status})")
            } else {
                detail.to_string()
            },
        ))
    }
}

fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    kill_pid(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

fn kill_pid(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Available {
        Available {
            wordcraft: Some("wordcraft-cli".into()),
            libreoffice: Some("soffice".into()),
            word: true,
        }
    }

    fn chosen(
        preference: DocumentEngine,
        from: Format,
        to: Format,
        available: &Available,
    ) -> Option<&'static str> {
        choose_engine(preference, from, to, available).map(|e| e.name())
    }

    #[test]
    fn auto_uses_wordcraft_for_what_it_can_do_and_falls_back_for_the_rest() {
        let a = all();
        for (from, to) in [
            (Format::Docx, Format::Pdf),
            (Format::Doc, Format::Pdf),
            (Format::Markdown, Format::Pdf),
            (Format::Doc, Format::Docx),
        ] {
            assert_eq!(
                chosen(DocumentEngine::Auto, from, to, &a),
                Some("WordCraft"),
                "{from} -> {to}"
            );
        }
        // PDF → DOCX no lo sabe hacer: LibreOffice, y si no está, Word.
        assert_eq!(
            chosen(DocumentEngine::Auto, Format::Pdf, Format::Docx, &a),
            Some("LibreOffice")
        );
        let without_lo = Available {
            libreoffice: None,
            ..all()
        };
        assert_eq!(
            chosen(DocumentEngine::Auto, Format::Pdf, Format::Docx, &without_lo),
            Some("Microsoft Word")
        );
    }

    #[test]
    fn without_wordcraft_auto_still_converts_with_libreoffice_or_word() {
        let a = Available {
            wordcraft: None,
            ..all()
        };
        assert_eq!(
            chosen(DocumentEngine::Auto, Format::Docx, Format::Pdf, &a),
            Some("LibreOffice")
        );
        let nothing = Available {
            wordcraft: None,
            libreoffice: None,
            word: false,
        };
        assert!(chosen(DocumentEngine::Auto, Format::Docx, Format::Pdf, &nothing).is_none());
    }

    #[test]
    fn forcing_an_engine_never_uses_another() {
        let a = all();
        assert_eq!(
            chosen(DocumentEngine::LibreOffice, Format::Docx, Format::Pdf, &a),
            Some("LibreOffice")
        );
        assert_eq!(
            chosen(DocumentEngine::Word, Format::Docx, Format::Pdf, &a),
            Some("Microsoft Word")
        );
        let only_wordcraft = Available {
            libreoffice: None,
            word: false,
            ..all()
        };
        assert!(
            chosen(
                DocumentEngine::LibreOffice,
                Format::Docx,
                Format::Pdf,
                &only_wordcraft
            )
            .is_none()
        );
        // WordCraft elegido a mano para PDF → DOCX: no sabe, y sin otro motor no hay nada.
        assert!(
            chosen(
                DocumentEngine::WordCraft,
                Format::Pdf,
                Format::Docx,
                &only_wordcraft
            )
            .is_none()
        );
    }

    #[test]
    fn wordcraft_supports_exactly_the_pairs_it_was_tested_on() {
        assert!(wordcraft_supports(Format::Docx, Format::Pdf));
        assert!(wordcraft_supports(Format::Doc, Format::Docx));
        assert!(!wordcraft_supports(Format::Pdf, Format::Docx));
        assert!(!wordcraft_supports(Format::Docx, Format::Doc));
    }

    #[test]
    fn wordcraft_is_found_where_each_package_leaves_it() {
        let dirs = packaged_wordcraft(Path::new("/opt/app/bin"), "wordcraft-cli");
        for relative in [
            "wordcraft/wordcraft-cli",
            "../Resources/wordcraft/wordcraft-cli",
            "../lib/Converter/wordcraft/wordcraft-cli",
            "wordcraft-cli",
        ] {
            assert!(
                dirs.contains(&Path::new("/opt/app/bin").join(relative)),
                "falta {relative}"
            );
        }
    }

    #[test]
    fn install_hint_is_specific_to_the_system() {
        let hint = libreoffice_install_hint();
        assert!(hint.contains(LIBREOFFICE_ENV));
        if cfg!(windows) {
            assert!(hint.contains("puede descargarlo"));
        } else if cfg!(target_os = "macos") {
            assert!(hint.contains("brew"));
        } else {
            // En Linux la aplicación no descarga nada: no debe prometerlo.
            assert!(hint.contains("gestor de paquetes"));
            assert!(!hint.contains("puede descargarlo"));
        }
    }

    #[test]
    fn declares_exactly_the_supported_pairs() {
        let converter = OfficeConverter;
        assert_eq!(
            converter.supports(Format::Docx, Format::Pdf),
            Some(Fidelity::Lossless)
        );
        assert_eq!(
            converter.supports(Format::Doc, Format::Pdf),
            Some(Fidelity::Lossless)
        );
        assert_eq!(
            converter.supports(Format::Doc, Format::Docx),
            Some(Fidelity::Lossless)
        );
        assert_eq!(
            converter.supports(Format::Pdf, Format::Docx),
            Some(Fidelity::Lossy)
        );
        assert_eq!(converter.supports(Format::Docx, Format::Doc), None);
        assert_eq!(converter.supports(Format::Docx, Format::Docx), None);
        assert_eq!(converter.supports(Format::Png, Format::Docx), None);
    }

    #[test]
    fn file_urls_are_percent_encoded() {
        let url = file_url(Path::new("C:\\Users\\Ana María\\Temp\\perfil"));
        assert_eq!(url, "file:///C:/Users/Ana%20Mar%C3%ADa/Temp/perfil");
        assert_eq!(file_url(Path::new("/tmp/a b")), "file:///tmp/a%20b");
    }

    #[test]
    fn a_missing_engine_is_a_missing_dependency_error() {
        // Fuerza LibreOffice con una ruta que no existe y sin otras ubicaciones fiables.
        if find_libreoffice().is_some() {
            return;
        }
        let options = ConvertOptions {
            document_engine: DocumentEngine::LibreOffice,
            ..Default::default()
        };
        let err = OfficeConverter
            .convert(b"x", Format::Docx, Format::Pdf, &options)
            .unwrap_err();
        assert!(
            matches!(
                err,
                Error::MissingDependency {
                    name: "LibreOffice",
                    ..
                }
            ),
            "{err}"
        );
    }
}

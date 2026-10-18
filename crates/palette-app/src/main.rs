// En release no se abre una consola junto a la ventana.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod palette;
mod settings;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use converter_core::{ConvertOptions, Engine, Format};
use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, DragDropEvent, Emitter, Manager, PhysicalPosition, State, WebviewWindow, WindowEvent,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

const WINDOW: &str = "palette";

struct AppState {
    engine: Arc<Engine>,
    /// Archivos con los que está abierta la paleta.
    files: Mutex<Vec<PathBuf>>,
    /// Último estado enviado a la interfaz: si el evento llega antes de que la página cargue
    /// (arranque con archivos), la página lo pide al iniciar con `current_palette`.
    payload: Mutex<Option<palette::Payload>>,
    /// Operaciones largas en curso (conversiones, descargas). Mientras haya alguna, la paleta no
    /// se oculta al perder el foco: si no, el usuario no vería el resultado.
    busy: Arc<AtomicUsize>,
    /// Ajustes del usuario (se cargan al arrancar y se guardan al cambiarlos).
    settings: Mutex<settings::Settings>,
    /// Mientras el panel de ajustes está abierto, la paleta no se oculta al perder el foco (los
    /// menús desplegables y los selectores del sistema le quitan el foco a la ventana).
    pin: Mutex<Option<BusyGuard>>,
}

/// Marca una operación larga en curso mientras viva.
struct BusyGuard(Arc<AtomicUsize>);

impl BusyGuard {
    fn new(counter: &Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self(Arc::clone(counter))
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Serialize)]
struct Failure {
    name: String,
    error: String,
}

#[derive(Serialize)]
struct Outcome {
    converted: usize,
    failed: Vec<Failure>,
    outputs: Vec<String>,
    /// Texto para mostrar en la paleta cuando todo salió bien.
    summary: String,
    /// Falta un componente (LibreOffice): la interfaz ofrece descargarlo y reintenta.
    needs: Option<palette::Offer>,
}

/// "foto.webp" si salió un archivo; "3 archivos en «informe»" si un origen produjo varios
/// (páginas de un PDF); "5 archivos convertidos" si eran varios orígenes.
fn summarize(sources: usize, written: &[PathBuf]) -> String {
    let name = |p: &PathBuf| {
        p.file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    };
    match (sources, written) {
        (_, []) => String::new(),
        (_, [only]) => name(only),
        (1, many) => match many[0].parent().and_then(|p| p.file_name()) {
            Some(folder) => format!("{} archivos en «{}»", many.len(), folder.to_string_lossy()),
            None => format!("{} archivos", many.len()),
        },
        (_, many) if many.len() == sources => format!("{} archivos convertidos", many.len()),
        (_, many) => format!("{} archivos creados", many.len()),
    }
}

/// Opciones de conversión a partir de los ajustes del usuario. La variable de entorno
/// `CONVERTER_DOCUMENT_ENGINE=word|libreoffice` manda sobre el ajuste (pruebas y soporte).
fn conversion_options(settings: &settings::Settings) -> ConvertOptions {
    let mut options = settings.to_options();
    match std::env::var("CONVERTER_DOCUMENT_ENGINE").as_deref() {
        Ok("word") => options.document_engine = converter_core::DocumentEngine::Word,
        Ok("libreoffice") => options.document_engine = converter_core::DocumentEngine::LibreOffice,
        _ => {}
    }
    options
}

/// Convierte los archivos de la paleta al formato `format` (extensión canónica).
#[tauri::command]
async fn convert(state: State<'_, AppState>, format: String) -> Result<Outcome, String> {
    let to =
        Format::from_extension(&format).ok_or_else(|| format!("formato desconocido: {format}"))?;
    let files = state.files.lock().map_err(|e| e.to_string())?.clone();
    let engine = Arc::clone(&state.engine);
    let busy = BusyGuard::new(&state.busy);
    let settings = state.settings.lock().map_err(|e| e.to_string())?.clone();
    let options = conversion_options(&settings);
    tauri::async_runtime::spawn_blocking(move || {
        let _busy = busy;
        let mut outcome = Outcome {
            converted: 0,
            failed: Vec::new(),
            outputs: Vec::new(),
            summary: String::new(),
            needs: None,
        };
        let sources = files.len();
        let mut written_all: Vec<PathBuf> = Vec::new();
        for file in files {
            match engine.convert_file(&file, to, &options) {
                Ok(written) => {
                    outcome.converted += 1;
                    outcome
                        .outputs
                        .extend(written.iter().map(|p| p.display().to_string()));
                    written_all.extend(written);
                }
                Err(e) => {
                    if let converter_core::Error::MissingDependency { name, .. } = &e
                        && name.contains("LibreOffice")
                    {
                        outcome.needs = Some(palette::libreoffice_offer(
                            "Hace falta para este documento.",
                        ));
                    }
                    outcome.failed.push(Failure {
                        name: file
                            .file_name()
                            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                        error: e.to_string(),
                    });
                }
            }
        }
        outcome.summary = summarize(sources, &written_all);
        outcome
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(Clone, Serialize)]
struct ProgressEvent {
    phase: &'static str,
    done: u64,
    total: u64,
}

/// Descarga e instala un componente. Emite `component://progress` mientras avanza.
#[tauri::command]
async fn install_component(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<String, String> {
    if id != "libreoffice" {
        return Err(format!("componente desconocido: {id}"));
    }
    let busy = BusyGuard::new(&state.busy);
    tauri::async_runtime::spawn_blocking(move || {
        let _busy = busy;
        components::install_libreoffice(&mut |progress| {
            let (phase, done, total) = match progress {
                components::Progress::Downloading { done, total } => ("downloading", done, total),
                components::Progress::Verifying => ("verifying", 0, 0),
                components::Progress::Extracting => ("extracting", 0, 0),
                components::Progress::Finished => ("finished", 0, 0),
            };
            let _ = app.emit("component://progress", ProgressEvent { phase, done, total });
        })
        .map(|path| path.display().to_string())
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// De dónde sale el LibreOffice que se usaría ahora, para mostrarlo en los ajustes.
fn libreoffice_source() -> Option<&'static str> {
    let path = converter_core::find_libreoffice()?;
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .is_some_and(|dir| path.starts_with(dir));
    Some(if beside_exe {
        "incluido"
    } else if path.starts_with(converter_core::components_dir()) {
        "descargado"
    } else {
        "sistema"
    })
}

#[derive(Serialize)]
struct SettingsView {
    settings: settings::Settings,
    /// "incluido" | "descargado" | "sistema" | nada si no hay LibreOffice.
    libreoffice: Option<&'static str>,
    word: bool,
    shortcut: &'static str,
    offer: palette::Offer,
    /// Megabytes que ocupa el LibreOffice **descargado por la aplicación** (se puede quitar).
    managed_mb: Option<u64>,
    /// ¿Se puede desinstalar la aplicación desde aquí? Solo si se instaló con el instalador.
    can_uninstall_app: bool,
}

/// El desinstalador que deja el instalador de Windows junto al ejecutable.
fn app_uninstaller() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let uninstaller = exe.parent()?.join("uninstall.exe");
    uninstaller.is_file().then_some(uninstaller)
}

/// ¿Hay una conversión o una descarga en marcha? La fijación del panel de ajustes también suma
/// al contador `busy`, así que se descuenta.
fn operations_running(state: &AppState) -> bool {
    let pinned = usize::from(state.pin.lock().is_ok_and(|p| p.is_some()));
    state.busy.load(Ordering::SeqCst) > pinned
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<SettingsView, String> {
    Ok(SettingsView {
        settings: state.settings.lock().map_err(|e| e.to_string())?.clone(),
        libreoffice: libreoffice_source(),
        word: converter_core::document_engines().word,
        shortcut: "Ctrl+Alt+Espacio",
        offer: palette::libreoffice_offer("Convierte documentos sin otros programas."),
        managed_mb: Some(components::managed_libreoffice_size() / 1_000_000).filter(|mb| *mb > 0),
        can_uninstall_app: app_uninstaller().is_some(),
    })
}

/// Valida, guarda en disco y aplica los ajustes. Devuelve los valores ya acotados.
#[tauri::command]
fn save_settings(
    state: State<'_, AppState>,
    settings: settings::Settings,
) -> Result<settings::Settings, String> {
    let settings = settings.sanitized();
    settings::save_to(&settings::settings_path(), &settings)
        .map_err(|e| format!("no se pudieron guardar los ajustes: {e}"))?;
    *state.settings.lock().map_err(|e| e.to_string())? = settings.clone();
    Ok(settings)
}

/// Quita el LibreOffice que descargó la aplicación. Devuelve los MB liberados.
#[tauri::command]
async fn uninstall_libreoffice(state: State<'_, AppState>) -> Result<u64, String> {
    if operations_running(&state) {
        return Err("Hay una conversión o una descarga en curso; espera a que termine.".into());
    }
    tauri::async_runtime::spawn_blocking(|| {
        components::uninstall_libreoffice()
            .map(|bytes| bytes / 1_000_000)
            .map_err(|e| match e {
                components::ComponentError::Busy => {
                    "Hay una instalación de LibreOffice en curso.".to_string()
                }
                other => format!("No se pudo quitar LibreOffice: {other}"),
            })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Abre el desinstalador de Windows y cierra la aplicación para que pueda borrarla.
#[tauri::command]
fn uninstall_app(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if operations_running(&state) {
        return Err("Hay una conversión o una descarga en curso; espera a que termine.".into());
    }
    let uninstaller = app_uninstaller().ok_or_else(|| {
        "Esta copia no se instaló con el instalador (no hay desinstalador). Borra su carpeta."
            .to_string()
    })?;
    let mut command = std::process::Command::new(&uninstaller);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP: que sobreviva al cierre de esta aplicación.
        command.creation_flags(0x0000_0008 | 0x0000_0200);
    }
    command
        .spawn()
        .map_err(|e| format!("No se pudo abrir el desinstalador: {e}"))?;
    // El desinstalador no puede borrar el ejecutable mientras esté en marcha.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(600));
        app.exit(0);
    });
    Ok(())
}

/// Mantiene la paleta visible aunque pierda el foco (panel de ajustes abierto).
#[tauri::command]
fn pin_palette(state: State<'_, AppState>, pinned: bool) -> Result<(), String> {
    let mut pin = state.pin.lock().map_err(|e| e.to_string())?;
    *pin = pinned.then(|| BusyGuard::new(&state.busy));
    Ok(())
}

#[tauri::command]
fn current_palette(state: State<'_, AppState>) -> Option<palette::Payload> {
    state.payload.lock().ok().and_then(|p| p.clone())
}

#[tauri::command]
fn hide_palette(app: AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.hide();
    }
}

/// Centra la paleta en el cursor sin salirse del monitor.
fn place_at_cursor(app: &AppHandle, window: &WebviewWindow) {
    let (Ok(cursor), Ok(size)) = (app.cursor_position(), window.outer_size()) else {
        return;
    };
    let (width, height) = (f64::from(size.width), f64::from(size.height));
    let (mut x, mut y) = (cursor.x - width / 2.0, cursor.y - height / 2.0);
    if let Ok(Some(monitor)) = app.monitor_from_point(cursor.x, cursor.y) {
        let (origin, extent) = (monitor.position(), monitor.size());
        let (left, top) = (f64::from(origin.x), f64::from(origin.y));
        x = x.clamp(left, (left + f64::from(extent.width) - width).max(left));
        y = y.clamp(top, (top + f64::from(extent.height) - height).max(top));
    }
    let _ = window.set_position(PhysicalPosition::new(x as i32, y as i32));
}

/// Abre la paleta para `files` (vacío = estado de "suelta un archivo").
fn open_palette(app: &AppHandle, files: Vec<PathBuf>, reposition: bool) {
    let Some(window) = app.get_webview_window(WINDOW) else {
        return;
    };
    let state = app.state::<AppState>();
    let payload = palette::build(&files, &state.engine);
    if let Ok(mut current) = state.files.lock() {
        *current = files;
    }
    publish(&window, payload);
    if reposition {
        place_at_cursor(app, &window);
    }
    let _ = window.show();
    let _ = window.set_focus();
}

fn publish(window: &WebviewWindow, payload: palette::Payload) {
    let _ = window.emit("palette://open", &payload);
    if let Ok(mut current) = window.state::<AppState>().payload.lock() {
        *current = Some(payload);
    }
}

/// Desde la bandeja: abre la paleta directamente en el panel de ajustes.
fn open_settings(app: &AppHandle) {
    let Some(window) = app.get_webview_window(WINDOW) else {
        return;
    };
    publish(&window, palette::Payload::notice("Suelta un archivo aquí"));
    place_at_cursor(app, &window);
    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.emit("palette://settings", ());
}

/// Desde la bandeja: ofrece instalar LibreOffice (o avisa de que ya está).
fn open_libreoffice_offer(app: &AppHandle) {
    let Some(window) = app.get_webview_window(WINDOW) else {
        return;
    };
    let payload = if converter_core::find_libreoffice().is_some() {
        palette::Payload::notice("LibreOffice ya está instalado")
    } else {
        palette::Payload::offer(palette::libreoffice_offer(
            "Convierte documentos sin otros programas.",
        ))
    };
    publish(&window, payload);
    place_at_cursor(app, &window);
    let _ = window.show();
    let _ = window.set_focus();
}

fn open_with_notice(app: &AppHandle, message: String) {
    if let Some(window) = app.get_webview_window(WINDOW) {
        publish(&window, palette::Payload::notice(message));
        place_at_cursor(app, &window);
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Atajo global: oculta la paleta si está visible; si no, lee la selección del Explorador.
fn on_hotkey(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW)
        && window.is_visible().unwrap_or(false)
    {
        let _ = window.hide();
        return;
    }
    let app = app.clone();
    // La lectura usa COM: va en un hilo propio para no bloquear el del atajo.
    std::thread::spawn(move || match os_shell::selected_files() {
        Ok(files) if !files.is_empty() => open_palette(&app, files, true),
        Ok(_) => open_with_notice(&app, "No hay nada seleccionado".into()),
        // Otra aplicación al frente: se abre vacía por si se quiere arrastrar un archivo.
        Err(os_shell::ShellError::NotAFileManager | os_shell::ShellError::Unsupported) => {
            open_palette(&app, Vec::new(), true);
        }
        Err(e) => open_with_notice(&app, e.to_string()),
    });
}

/// Rutas existentes entre los argumentos (menú contextual, "Enviar a", segunda instancia).
fn paths_from_args(args: impl IntoIterator<Item = String>) -> Vec<PathBuf> {
    args.into_iter()
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .collect()
}

fn main() {
    tauri::Builder::default()
        // Debe ser el primer plugin: reenvía los argumentos de una segunda instancia.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            let files = paths_from_args(args.into_iter().skip(1));
            open_palette(app, files, true);
        }))
        .manage(AppState {
            engine: Arc::new(Engine::default()),
            files: Mutex::new(Vec::new()),
            payload: Mutex::new(None),
            busy: Arc::new(AtomicUsize::new(0)),
            settings: Mutex::new(settings::load_from(&settings::settings_path())),
            pin: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            convert,
            hide_palette,
            current_palette,
            install_component,
            get_settings,
            save_settings,
            pin_palette,
            uninstall_libreoffice,
            uninstall_app
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let window = app
                .get_webview_window(WINDOW)
                .ok_or("falta la ventana de la paleta")?;

            let events_handle = handle.clone();
            window.on_window_event(move |event| match event {
                WindowEvent::Focused(false) => {
                    let busy = events_handle
                        .state::<AppState>()
                        .busy
                        .load(Ordering::SeqCst);
                    if busy == 0 {
                        hide_palette(events_handle.clone());
                    }
                }
                WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) => {
                    open_palette(&events_handle, paths.clone(), false);
                }
                _ => {}
            });

            // Ctrl+Alt+Espacio. Si otra aplicación ya lo tiene, se sigue funcionando por la bandeja.
            let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
            handle.plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(move |app, pressed, event| {
                        if pressed == &shortcut && event.state() == ShortcutState::Pressed {
                            on_hotkey(app);
                        }
                    })
                    .build(),
            )?;
            if let Err(e) = handle.global_shortcut().register(shortcut) {
                eprintln!("no se pudo registrar Ctrl+Alt+Espacio: {e}");
            }

            let menu = Menu::with_items(
                app,
                &[
                    &MenuItem::with_id(app, "show", "Mostrar paleta", true, None::<&str>)?,
                    &MenuItem::with_id(app, "settings", "Configuración…", true, None::<&str>)?,
                    &MenuItem::with_id(
                        app,
                        "libreoffice",
                        "Instalar LibreOffice…",
                        true,
                        None::<&str>,
                    )?,
                    &MenuItem::with_id(app, "quit", "Salir", true, None::<&str>)?,
                ],
            )?;
            let mut tray = TrayIconBuilder::new()
                .tooltip("Converter · Ctrl+Alt+Espacio")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => open_palette(app, Vec::new(), true),
                    "libreoffice" => open_libreoffice_offer(app),
                    "settings" => open_settings(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        open_palette(tray.app_handle(), Vec::new(), true);
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            // Arranque con archivos (menú contextual / "Enviar a"): abre la paleta directamente.
            let files = paths_from_args(std::env::args().skip(1));
            if !files.is_empty() {
                open_palette(&handle, files, true);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error al ejecutar la aplicación");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn summary_names_the_file_the_folder_or_the_count() {
        assert_eq!(summarize(1, &paths(&["C:/x/foto.webp"])), "foto.webp");
        assert_eq!(
            summarize(
                1,
                &paths(&["C:/x/informe/informe-1.png", "C:/x/informe/informe-2.png"])
            ),
            "2 archivos en «informe»"
        );
        assert_eq!(
            summarize(2, &paths(&["C:/x/a.webp", "C:/x/b.webp"])),
            "2 archivos convertidos"
        );
        assert_eq!(
            summarize(2, &paths(&["a/1.png", "a/2.png", "b/1.png"])),
            "3 archivos creados"
        );
        assert_eq!(summarize(1, &[]), "");
    }
}

use std::path::PathBuf;

use windows::Win32::Foundation::{HWND, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize, IServiceProvider,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::{
    IFolderView2, IShellBrowser, IShellItem, IShellItemArray, IShellWindows, IWebBrowserApp,
    SID_STopLevelBrowser, SIGDN_FILESYSPATH, SVGIO_SELECTION, SWC_DESKTOP, SWFO_NEEDDISPATCH,
    ShellWindows,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, IsWindowVisible,
};
use windows::core::Interface;

use crate::ShellError;

const CSIDL_DESKTOP: i32 = 0;

fn platform<E: std::fmt::Display>(e: E) -> ShellError {
    ShellError::Platform(e.to_string())
}

/// Inicializa COM en el hilo actual y lo libera al salir de ámbito.
struct ComGuard(bool);

impl ComGuard {
    fn new() -> Result<Self, ShellError> {
        // SAFETY: llamada FFI sin punteros; el hilo la empareja con `CoUninitialize` en `Drop`.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr == RPC_E_CHANGED_MODE {
            // El hilo ya tenía COM en otro modo: se puede usar, pero no hay que liberarlo.
            Ok(Self(false))
        } else {
            hr.ok().map_err(platform)?;
            Ok(Self(true))
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: empareja el `CoInitializeEx` exitoso de `ComGuard::new`.
            unsafe { CoUninitialize() };
        }
    }
}

fn class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 256];
    // SAFETY: `buffer` es un búfer válido y escribible durante la llamada.
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..usize::try_from(len).unwrap_or(0)])
}

pub fn selected_files_in_foreground() -> Result<Vec<PathBuf>, ShellError> {
    let _com = ComGuard::new()?;
    // SAFETY: sin argumentos; devuelve un HWND (posiblemente nulo).
    let foreground = unsafe { GetForegroundWindow() };
    match class_name(foreground).as_str() {
        "CabinetWClass" => selection_in_explorer_window(foreground),
        // El Escritorio vive en `Progman` o, con fondos animados, en un `WorkerW`.
        "Progman" | "WorkerW" => selection_on_desktop(),
        _ => Err(ShellError::NotAFileManager),
    }
}

pub fn selected_files_in_window(hwnd: isize) -> Result<Vec<PathBuf>, ShellError> {
    let _com = ComGuard::new()?;
    selection_in_explorer_window(HWND(hwnd as *mut _))
}

pub fn selected_files_on_desktop() -> Result<Vec<PathBuf>, ShellError> {
    let _com = ComGuard::new()?;
    selection_on_desktop()
}

fn shell_windows() -> Result<IShellWindows, ShellError> {
    // SAFETY: crea el objeto COM del sistema; el resultado es un puntero inteligente.
    unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL) }.map_err(platform)
}

/// Con pestañas, varias entradas de `IShellWindows` comparten el mismo `HWND` de nivel
/// superior; la pestaña activa es la única cuya vista está visible.
fn selection_in_explorer_window(top_level: HWND) -> Result<Vec<PathBuf>, ShellError> {
    let windows = shell_windows()?;
    // SAFETY: todas las llamadas son métodos COM sobre interfaces válidas obtenidas arriba.
    unsafe {
        let count = windows.Count().map_err(platform)?;
        let mut fallback = None;
        for index in 0..count {
            let Ok(dispatch) = windows.Item(&VARIANT::from(index)) else {
                continue;
            };
            let Ok(browser) = dispatch.cast::<IWebBrowserApp>() else {
                continue;
            };
            if browser.HWND().map_err(platform)?.0 != top_level.0 as isize {
                continue;
            }
            let view = active_view(&dispatch)?;
            let visible = view
                .GetWindow()
                .map(|w| IsWindowVisible(w).as_bool())
                .unwrap_or(false);
            if visible {
                return paths_from_view(&view);
            }
            fallback.get_or_insert(view);
        }
        match fallback {
            Some(view) => paths_from_view(&view),
            None => Err(ShellError::NotAFileManager),
        }
    }
}

fn selection_on_desktop() -> Result<Vec<PathBuf>, ShellError> {
    let windows = shell_windows()?;
    let mut hwnd = 0i32;
    // SAFETY: `hwnd` es un entero escribible durante la llamada; el resto son métodos COM.
    unsafe {
        let dispatch = windows
            .FindWindowSW(
                &VARIANT::from(CSIDL_DESKTOP),
                &VARIANT::default(),
                SWC_DESKTOP,
                &mut hwnd,
                SWFO_NEEDDISPATCH,
            )
            .map_err(platform)?;
        paths_from_view(&active_view(&dispatch)?)
    }
}

/// # Safety
/// `dispatch` debe ser el `IDispatch` de una ventana del Explorador o del Escritorio.
unsafe fn active_view(
    dispatch: &windows::Win32::System::Com::IDispatch,
) -> Result<windows::Win32::UI::Shell::IShellView, ShellError> {
    let provider: IServiceProvider = dispatch.cast().map_err(platform)?;
    let browser: IShellBrowser =
        unsafe { provider.QueryService(&SID_STopLevelBrowser) }.map_err(platform)?;
    unsafe { browser.QueryActiveShellView() }.map_err(platform)
}

/// # Safety
/// `view` debe ser una vista de shell viva.
unsafe fn paths_from_view(
    view: &windows::Win32::UI::Shell::IShellView,
) -> Result<Vec<PathBuf>, ShellError> {
    let folder_view: IFolderView2 = view.cast().map_err(platform)?;
    // Sin selección, `Items` falla con S_FALSE/E_FAIL: se interpreta como lista vacía.
    let Ok(items) = (unsafe { folder_view.Items::<IShellItemArray>(SVGIO_SELECTION) }) else {
        return Ok(Vec::new());
    };
    let count = unsafe { items.GetCount() }.map_err(platform)?;
    let mut paths = Vec::with_capacity(count as usize);
    for index in 0..count {
        let item: IShellItem = unsafe { items.GetItemAt(index) }.map_err(platform)?;
        // Elementos virtuales (Papelera, Este equipo...) no tienen ruta de archivo.
        let Ok(name) = (unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }) else {
            continue;
        };
        // SAFETY: `name` es una cadena terminada en nulo asignada por COM; se libera tras copiarla.
        let path = unsafe { name.to_string() }.map_err(platform);
        unsafe { CoTaskMemFree(Some(name.0 as *const _)) };
        paths.push(PathBuf::from(path?));
    }
    Ok(paths)
}

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
    GW_HWNDPREV, GetClassNameW, GetForegroundWindow, GetParent, GetWindow, IsWindowVisible,
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

/// Con pestañas, varias entradas de `IShellWindows` comparten el mismo `HWND` de nivel superior
/// (una por pestaña) y hay que elegir **la pestaña activa**. La visibilidad de su vista no sirve
/// (una pestaña inactiva sigue «visible», y con la ventana minimizada ninguna lo parece): la
/// pestaña activa es la ventana `ShellTabWindowClass` que está más arriba en el orden Z entre sus
/// hermanas. Medido en Windows 11 (compilación 26300) cambiando de pestaña con UI Automation.
fn selection_in_explorer_window(top_level: HWND) -> Result<Vec<PathBuf>, ShellError> {
    let windows = shell_windows()?;
    // SAFETY: todas las llamadas son métodos COM y de ventanas sobre interfaces y `HWND` válidos.
    unsafe {
        let count = windows.Count().map_err(platform)?;
        let mut views = Vec::new();
        let mut candidates = Vec::new();
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
            let window = view.GetWindow().ok();
            candidates.push(TabCandidate {
                z_rank: window.and_then(tab_z_rank),
                visible: window.is_some_and(|w| IsWindowVisible(w).as_bool()),
            });
            views.push(view);
        }
        match choose_tab(&candidates).and_then(|i| views.get(i)) {
            Some(view) => paths_from_view(view),
            None => Err(ShellError::NotAFileManager),
        }
    }
}

/// Lo que se sabe de cada pestaña para decidir cuál es la activa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TabCandidate {
    /// Cuántas ventanas hermanas hay por encima de su `ShellTabWindowClass` (0 = la de más
    /// arriba), o `None` si la vista no cuelga de una (Explorador sin pestañas).
    z_rank: Option<usize>,
    visible: bool,
}

/// Índice de la pestaña activa: la más alta en el orden Z; si no hay datos de pestañas, la
/// primera con la vista visible; y si tampoco, la primera.
fn choose_tab(candidates: &[TabCandidate]) -> Option<usize> {
    candidates
        .iter()
        .enumerate()
        .filter_map(|(index, c)| c.z_rank.map(|rank| (rank, index)))
        .min()
        .map(|(_, index)| index)
        .or_else(|| candidates.iter().position(|c| c.visible))
        .or_else(|| (!candidates.is_empty()).then_some(0))
}

/// Posición en el orden Z de la pestaña (`ShellTabWindowClass`) de la que cuelga `view`: cuántas
/// ventanas hermanas la tapan. Más bajo = más arriba = pestaña activa.
fn tab_z_rank(view: HWND) -> Option<usize> {
    let mut current = view;
    for _ in 0..16 {
        if class_name(current) == "ShellTabWindowClass" {
            let mut rank = 0;
            let mut sibling = current;
            // SAFETY: `GetWindow` solo consulta el orden de ventanas; un resultado nulo o con error
            // significa que no hay más hermanas.
            while let Ok(above) = unsafe { GetWindow(sibling, GW_HWNDPREV) } {
                if above.0.is_null() {
                    break;
                }
                rank += 1;
                sibling = above;
                if rank > 4096 {
                    return None;
                }
            }
            return Some(rank);
        }
        // SAFETY: consulta del padre de una ventana; el resultado puede ser nulo.
        current = unsafe { GetParent(current) }
            .ok()
            .filter(|p| !p.0.is_null())?;
    }
    None
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
    // Hay selección, pero nada con ruta en el disco (carpetas virtuales como «Inicio» o la
    // Papelera): se dice, en vez de dar a entender que no se seleccionó nada.
    if paths.is_empty() && count > 0 {
        return Err(ShellError::Platform(
            "lo seleccionado no son archivos del disco (por ejemplo, está en «Inicio» o en la \
             Papelera); ábrelo desde su carpeta"
                .into(),
        ));
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(z_rank: Option<usize>, visible: bool) -> TabCandidate {
        TabCandidate { z_rank, visible }
    }

    #[test]
    fn the_active_tab_is_the_one_on_top_of_the_z_order_even_if_all_look_visible() {
        // El caso real: dos pestañas «visibles»; la activa es la de menor posición en el orden Z.
        assert_eq!(
            choose_tab(&[tab(Some(3), true), tab(Some(2), true)]),
            Some(1)
        );
        assert_eq!(
            choose_tab(&[tab(Some(1), true), tab(Some(3), true)]),
            Some(0)
        );
    }

    #[test]
    fn a_minimized_window_where_nothing_looks_visible_still_has_an_active_tab() {
        // El caso del usuario: ventana minimizada, ninguna pestaña «visible».
        assert_eq!(
            choose_tab(&[tab(Some(3), false), tab(Some(2), false)]),
            Some(1)
        );
    }

    #[test]
    fn without_tab_windows_it_falls_back_to_visibility_and_then_to_the_first() {
        assert_eq!(choose_tab(&[tab(None, false), tab(None, true)]), Some(1));
        assert_eq!(choose_tab(&[tab(None, false), tab(None, false)]), Some(0));
        assert_eq!(choose_tab(&[tab(None, true)]), Some(0));
        assert_eq!(choose_tab(&[]), None);
    }

    #[test]
    fn ties_keep_the_first_and_tab_data_wins_over_visibility() {
        assert_eq!(
            choose_tab(&[tab(Some(2), true), tab(Some(2), true)]),
            Some(0)
        );
        // Una pestaña con datos de orden Z gana a otra que solo «parece» visible.
        assert_eq!(choose_tab(&[tab(None, true), tab(Some(5), false)]), Some(1));
    }

    #[test]
    fn a_window_without_children_has_no_tab_rank() {
        // El escritorio de Windows no cuelga de ninguna `ShellTabWindowClass`.
        // SAFETY: `GetDesktopWindow` no tiene precondiciones.
        let desktop = unsafe { windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow() };
        assert_eq!(tab_z_rank(desktop), None);
    }
}

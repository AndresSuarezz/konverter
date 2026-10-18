//! Imprime los archivos seleccionados. Sin argumentos lee la ventana en primer plano tras
//! 5 s (tiempo de cambiar al Explorador); con `--window <hwnd>` lee esa ventana al instante.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        #[cfg(windows)]
        [flag, hwnd] if flag == "--window" => {
            os_shell::selected_files_in_window(hwnd.parse().expect("hwnd numérico"))
        }
        #[cfg(windows)]
        [flag] if flag == "--desktop" => os_shell::selected_files_on_desktop(),
        _ => {
            eprintln!("Cambia al Explorador o al Escritorio: leyendo en 5 s...");
            std::thread::sleep(std::time::Duration::from_secs(5));
            os_shell::selected_files()
        }
    };
    match result {
        Ok(files) => {
            println!("{} archivo(s)", files.len());
            for file in files {
                println!("{}", file.display());
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

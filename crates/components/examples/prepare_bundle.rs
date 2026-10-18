//! Prepara los componentes que viajan **dentro** del instalador completo: descarga LibreOffice,
//! verifica su SHA-256, lo extrae y lo poda en `<destino>/libreoffice-<versión>`.
//!
//! ```text
//! cargo run -p components --example prepare_bundle -- vendor/bin/bundle
//! ```
//! Después, `scripts/build-installers.ps1` la pasa al instalador (`CVT_LO_DIR`), que la copia como `libreoffice/` si el usuario elige la instalación completa.
//! Si la carpeta ya existe y está completa, no hace nada.

use std::path::PathBuf;

use components::{LIBREOFFICE_VERSION, Progress, install_libreoffice_into};

fn main() {
    let Some(destination) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("uso: prepare_bundle <carpeta-de-destino>");
        std::process::exit(2);
    };
    let mut last = 0;
    let result = install_libreoffice_into(&destination, &mut |progress| match progress {
        Progress::Downloading { done, total } => {
            let percent = done * 100 / total.max(1);
            if percent >= last + 10 {
                last = percent;
                println!("descargando {percent} %");
            }
        }
        other => println!("{other:?}"),
    });
    match result {
        Ok(soffice) => {
            println!(
                "LibreOffice {LIBREOFFICE_VERSION} listo: {}",
                soffice.display()
            );
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

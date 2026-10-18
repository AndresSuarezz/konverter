# Distribución

Objetivo: que quien instale la aplicación tenga **todo lo necesario para que funcione**, sin pasos manuales, en Windows, macOS y Linux. Este documento recoge la estrategia, lo que ya está construido y probado, y lo que falta. Las decisiones están numeradas en [BITACORA.md](BITACORA.md).

## Qué necesita la aplicación

| Componente | Para qué | Tamaño | Cómo llega al usuario |
|---|---|---|---|
| **pdfium** (BSD-3) | PDF ↔ imágenes, PDF → TXT/MD | 7 MB | Siempre dentro del instalador |
| **WordCraft** (`MIT OR Apache-2.0`) | DOCX/DOC/Markdown → PDF, DOC → DOCX | 35–41 MB (el instalador lo comprime a ~9 MB) | Siempre dentro del instalador, con sus licencias (`wordcraft/LICENSE-*`, `NOTICE`, `ATTRIBUTION.md`) |
| **LibreOffice** (MPL 2.0) | **Solo PDF → DOCX** (y motor alternativo) | 1,5 GB extraído → **690 MB podado** (139 MB comprimido) | Descarga bajo demanda desde la paleta (Windows), paquete del sistema (Linux/macOS), o dentro de la variante grande del instalador |
| WebView2 (solo Windows) | La ventana de la paleta | — | Viene con Windows 11; en Windows 10, el instalador lo descarga |
| Tesseract (futuro) | OCR | por medir | Mismo mecanismo que LibreOffice |

Word, si está instalado, sirve de respaldo para PDF → DOCX, pero la aplicación no depende de él. **WordCraft se compila desde una versión fijada** (`scripts/build-wordcraft.ps1`/`.sh`: etiqueta y commit) y es alfa: ver [REFERENCIAS-CRAFT.md](REFERENCIAS-CRAFT.md) para sus riesgos (sobre todo, que maqueta con las fuentes del sistema).

## Windows (hecho y probado)

NSIS por usuario (no pide administrador), generado por `scripts/build-installers.ps1`. Hay dos variantes (D-45):

| Instalador | Descarga | En disco | Contenido |
|---|---|---|---|
| **Normal** `Converter_<versión>_x64-setup.exe` | **16 MiB** | 58 MB | App + pdfium + WordCraft. Convierte todo sin internet salvo PDF → Word, que ofrece descargar LibreOffice (374 MB) la primera vez, o usa el del equipo o Word |
| **Con LibreOffice** `...-setup-con-libreoffice.exe` (`-ConLibreOffice`) | **155 MiB** | 749 MB | Lo anterior + LibreOffice podado, para PDF → Word sin internet. Muestra una página «Tipo de instalación» (*Estándar* o *Con LibreOffice*) |

Antes (D-30/D-37) había un único instalador de 146 MiB con LibreOffice dentro y una página para elegir; al pasar WordCraft a motor principal (D-41) el LibreOffice dejó de hacer falta salvo para PDF → Word, y llevarlo dentro de todos los instaladores era el 85 % de su peso. La página existe solo en la variante grande: `crates/palette-app/nsis/installer.nsi`, copia de la plantilla de `tauri-bundler 2.10.3` (CLI `2.12.3`, fijado en el script) con los añadidos marcados «Converter»; el script fija `CVT_LO_DIR` para activarla. Sin ventana: `/S` y, en la variante grande, `/S /TYPE=estandar|completa`; reinstalar o actualizar conserva lo que había.

**Verificado** (instalador normal, instalado en `%TEMP%`, con la interfaz real por depuración remota): 58 MB en disco, WordCraft y sus licencias dentro, sin LibreOffice; DOCX → PDF en 1,4 s, Markdown → PDF en 0,43 s, DOC → PDF en 0,41 s, DOC → DOCX en 0,33 s; PDF → DOCX cayó a Microsoft Word (no había LibreOffice) en 10 s. La **variante con LibreOffice** también se construyó con la configuración nueva (155 MiB) y se instaló en silencio con `/TYPE=estandar` (58 MB, sin LibreOffice) y `/TYPE=completa` (749 MB, con `soffice.exe`); se vio la pantalla de elección y se cerró sin instalar. No se ha probado a elegir «Con LibreOffice» desde la ventana ni a reinstalar de una opción a otra.

Orden en que el motor elige, **por par de formatos**: WordCraft (si sabe hacerlo: `CONVERTER_WORDCRAFT` → `<instalación>\wordcraft` → depuración `vendor/bin/wordcraft`; nunca el `PATH`); si no, LibreOffice (`CONVERTER_LIBREOFFICE` → incluido en la app → descargado por la app en `%LOCALAPPDATA%\Converter\components` → instalado en el sistema → `PATH`) y, por último, Word. Sin ninguno, la oferta de descarga de LibreOffice.

## Desinstalar (Windows)

- **El desinstalador limpia lo descargado**: al desinstalar, borra `%LOCALAPPDATA%\Converter\components` (el LibreOffice que descargó la app, ~700 MB), salvo cuando se está **actualizando**. Con la casilla «borrar los datos de la aplicación» borra además los ajustes. Con la variante grande no hay nada que limpiar: LibreOffice vive en la carpeta de instalación y se va con ella.
- **Desde la propia aplicación** (tuerca → Mantenimiento): «Quitar LibreOffice descargado» libera el espacio sin desinstalar nada, y «Desinstalar Converter…» abre el desinstalador de Windows.
- Estado: el gancho está probado con carpetas desechables; **el usuario probó instalar y desinstalar de punta a punta en otro PC y funcionó** (la casilla «borrar datos» no se anotó).

## Linux (construido y probado en Ubuntu 22.04)

Configuración: `crates/palette-app/tauri.bundle.linux.conf.json` y `linux/converter.desktop`. Se probó en una distribución Ubuntu 22.04 desechable bajo WSL2 (ver «Cómo se prueba Linux sin un Linux»).

| Paquete | Tamaño | Verificado |
|---|---|---|
| `.deb` | 26 MiB | Construido; `dpkg -I` muestra `Recommends: fonts-crosextra-carlito, fonts-liberation`; instalado con `apt`; la aplicación **arranca y sigue viva** |
| `.rpm` | 26 MiB | Construido; **no se instaló** |
| AppImage | 98 MiB | Construido; incluye pdfium y WordCraft en `usr/lib/Converter/`; el de la versión anterior **arrancaba y seguía vivo** (el actual no se ejecutó) |

- **Dónde queda pdfium** (medido en el paquete real): `/usr/lib/Converter/pdfium/libpdfium.so`, con el ejecutable en `/usr/bin/converter`. El motor lo busca en `<exe>/../lib/Converter/pdfium` (igual en el AppImage), en `<exe>/pdfium` (Windows) y en `<exe>/../Resources/pdfium` (macOS); hay un test que lo fija. Sin la biblioteca el error lo dice y nombra el script.
- **WordCraft** va dentro del paquete (`usr/lib/Converter/wordcraft/wordcraft-cli`, 40,7 MB, con sus licencias). Medido desde esa disposición y **sin variables de entorno**: DOCX, Markdown y DOC → PDF en ~0,35 s y DOC → DOCX en 0,33 s; las 4 pruebas reales `wordcraft_*` pasan también **sin fuentes extra instaladas** (usa las de fallback de WordCraft). Los paquetes **recomiendan** `fonts-crosextra-carlito` y `fonts-liberation` (compatibles con Calibri y Arial, ~2 MB) para que la paginación se parezca a la de Word; no se comparó la paginación con y sin ellas.
- **LibreOffice** ya no se recomienda: solo hace falta para **PDF → DOCX**. Con LibreOffice 7.3.7 del sistema, las 5 conversiones pasan con contenido comprobado (`libreoffice_converts_every_engine_pair_with_real_content`). La aplicación no descarga LibreOffice en Linux: sin él, PDF → DOCX da el mensaje `sudo apt install libreoffice-writer` y no se ofrece una descarga que fallaría.
- **«Abrir con»**: el acceso directo usa `Exec=converter %F` y declara los tipos MIME que convertimos (valida con `desktop-file-validate`). No se ha visto en un gestor de archivos real.
- **Base de glibc**: se compila en Ubuntu 22.04 (glibc 2.35); sirve para sistemas igual o más nuevos.
- Tests en Linux: `fmt`, `clippy -D warnings` y **148 tests** pasan. Al hacerlo salieron y se corrigieron dos avisos que habrían roto la CI (un `use` y una función solo usados en Windows).
- **Sin probar**: que la ventana se dibuje y la bandeja funcione en un escritorio real (en el contenedor no hay bus D-Bus de sesión), el atajo global en X11 y Wayland, la lectura de la selección del gestor (`os-shell` devuelve `Unsupported`), instalar el `.rpm`, el AppImage en otras distribuciones y las actualizaciones delta del AppImage (`.zsync`).
- AppImage con LibreOffice dentro: no está hecho; TDF publica `.deb` en un tarball que se puede extraer con `dpkg-deb -x` y podar como el de Windows.

### Cómo se prueba Linux sin un Linux

Desde Windows, con WSL2 y una distribución **desechable e independiente de las que ya tengas**:

```powershell
# rootfs oficial (verificar el SHA-256 de SHA256SUMS)
curl.exe -o rootfs.tar.gz https://cloud-images.ubuntu.com/wsl/jammy/current/ubuntu-jammy-wsl-amd64-ubuntu22.04lts.rootfs.tar.gz
wsl --import cvt-linux C:\ruta\temporal\distro rootfs.tar.gz --version 2
# dentro: apt (dependencias de Tauri), rustup, copiar el código sin target/dist/vendor/bin, scripts/fetch-pdfium.sh,
# cargo test --workspace, y `npx @tauri-apps/cli@2.12.3 build --config tauri.bundle.linux.conf.json`
wsl --unregister cvt-linux   # al terminar: borra solo esa distribución
```

## macOS (configuración escrita, sin probar)

- `crates/palette-app/tauri.bundle.macos.conf.json` genera `.app` y `.dmg`; pdfium va en `Contents/Resources/pdfium/libpdfium.dylib` y el motor lo busca ahí. Se construye **una variante por arquitectura** en la CI (`macos-latest` para Apple silicon y `macos-15-intel`); un binario universal exigiría `lipo` sobre la `libpdfium.dylib`.
- **Nadie lo ha compilado en un Mac.** Lo único comprobado es que el código compila y pasa los tests en Windows y Linux; la integración continua es la forma de verlo en macOS.
- Incluir `LibreOffice.app` (extraída del `.dmg` con `hdiutil`) en `Converter.app/Contents/Resources/` y podarla. El motor ya busca ahí (`../Resources/LibreOffice.app/Contents/MacOS/soffice`).
- **Requiere firmar y notarizar**: todo binario anidado (incluida la `libpdfium.dylib`) debe llevar la firma del desarrollador (Developer ID) y el *hardened runtime*, y eso exige una cuenta de Apple Developer. Sin notarizar, Gatekeeper bloquea la aplicación. El flujo completo (firma de dentro a fuera, `notarytool`, `stapler`) está en `packaging/macos/package.sh` de PdfCraft; ver [REFERENCIAS-CRAFT.md](REFERENCIAS-CRAFT.md).

## Integración continua

En `.github/workflows/` (**todavía no se han ejecutado en GitHub**; lo único probado es lo que hacen sus pasos, a mano en Windows y Linux):

- `ci.yml`: en cada `push` a `main` y en cada PR, en Windows, Ubuntu 22.04 y macOS: descarga pdfium, `clippy -D warnings` y `cargo test --workspace` (más `fmt` en Linux). Un trabajo aparte pasa `cargo deny check`.
- `release.yml`: por etiqueta `v*` o a mano. Compila `wordcraft-cli` (~5 min, con caché por versión del script), construye el instalador de Windows (`scripts/build-installers.ps1`), `.deb`, `.rpm` y AppImage, y los `.dmg` de macOS, y deja un **borrador** de release. Nada va firmado.
- macOS cuesta más minutos que los otros en repositorios privados; en públicos es gratis.

## Por qué hay que podar LibreOffice y qué se quita

LibreOffice completo ocupa **1,5 GB** extraído. Medido en la versión 26.8.1:

| Carpeta | Tamaño | ¿Se necesita para convertir? |
|---|---|---|
| `share/extensions` (59 diccionarios) | 462 MB | No |
| `program/resource` (123 idiomas de interfaz) | 262 MB | Solo español e inglés |
| `share/config/images_*.zip` (temas de iconos) | 72 MB | No (sin ventana) |
| galería, plantillas, asistentes, autotexto, ayuda | 41 MB | No |
| **Se conserva**: `program/` (`mergedlo.dll` 145 MB, ICU, Writer, filtros PDF), `share/registry`, fuentes, `readmes/` (licencias) | 690 MB | Sí |

Resultado: **1524 MB → 690 MB**, y las **5 conversiones** (DOCX, DOC y MD → PDF; DOC → DOCX; PDF → DOCX) siguen funcionando con el contenido real verificado. `components::prune_libreoffice` hace la poda (con tests sobre un árbol falso que comprueban qué se conserva y que no toca algo que no sea LibreOffice) y se aplica tanto a la instalación bajo demanda como al paquete del instalador completo.

Se podría recortar más (módulos Calc, Impress, Draw, Base, Python: ~100 MB más), pero toca binarios con dependencias entre sí y el beneficio es menor; no se ha hecho.

## Cómo construir

```powershell
# Windows, desde la raíz del repositorio. Prepara pdfium y el LibreOffice podado si faltan.
powershell -File scripts/build-installers.ps1                    # instalador único con la elección, en dist/
powershell -File scripts/build-installers.ps1 -SinLibreOffice    # solo ligero y sin página (pruebas rápidas)
```

```bash
# Linux y macOS: pdfium con hash verificado, y después Tauri desde crates/palette-app
bash scripts/fetch-pdfium.sh                      # (--asset mac-x64 para forzar la arquitectura)
cd crates/palette-app
CARGO_TARGET_DIR=../../target/bundle npx @tauri-apps/cli@2.12.3 build --config tauri.bundle.linux.conf.json   # deb, rpm, AppImage
CARGO_TARGET_DIR=../../target/bundle npx @tauri-apps/cli@2.12.3 build --config tauri.bundle.macos.conf.json   # app, dmg
```

Piezas: `scripts/fetch-pdfium.ps1` y `scripts/fetch-pdfium.sh` (pdfium, SHA-256 fijado por sistema), `cargo run -p components --example prepare_bundle -- vendor/bin/bundle` (descarga con reanudación, verifica el SHA-256, extrae con `msiexec /a` y poda), `crates/palette-app/tauri.bundle.conf.json` (recursos y plantilla) y `crates/palette-app/nsis/installer.nsi` (página de selección y copia de LibreOffice vía `CVT_LO_DIR`).

## Licencias que hay que cumplir al redistribuir

- **LibreOffice (MPL 2.0)**: permite redistribuirlo, con sus avisos. La poda conserva `readmes/` y el instalador lo incluye. Pendiente: un archivo `THIRD_PARTY_LICENSES` propio que enumere todo lo incluido (LibreOffice, pdfium, las dependencias de Rust) y el enlace al código fuente de LibreOffice.
- **pdfium (BSD-3)**: `LICENSE-pdfium` ya se instala junto a la biblioteca.
- **Dependencias de Rust**: `deny.toml` (comprobado con `cargo deny check`, también en la CI) solo admite licencias permisivas. Se permite expresamente **MPL-2.0** (cinco crates que trae Tauri: `cssparser`, `selectors`, `dtoa-short`, `option-ext`, `cssparser-macros`), un copyleft débil por archivo que no afecta al resto del código, y **CDLA-Permissive-2.0** (datos de certificados). Al redistribuir, hay que conservar sus avisos.
- La aplicación es `MIT OR Apache-2.0` (`LICENSE-MIT`, `LICENSE-APACHE`) y no enlaza nada GPL.

## Pendiente (en orden de valor)

1. Firma de código en Windows (sin ella, SmartScreen avisa) y en macOS (obligatoria).
2. **Ejecutar la CI en GitHub** y ver qué dice macOS. Después, probar Linux y macOS en máquinas reales (ventana, bandeja, atajo, selección del gestor); el preparado de LibreOffice podado para ellos sigue sin hacerse.
3. Para Windows 10 sin internet: `webviewInstallMode: offlineInstaller` en el instalador completo (suma ~130 MB, pero elimina la única descarga que le queda).
4. Archivo de licencias de terceros.
5. Auto-actualización.

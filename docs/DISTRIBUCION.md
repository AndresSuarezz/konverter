# Distribución

Objetivo: que quien instale la aplicación tenga **todo lo necesario para que funcione**, sin pasos manuales, en Windows, macOS y Linux. Este documento recoge la estrategia, lo que ya está construido y probado, y lo que falta. Las decisiones están numeradas en [BITACORA.md](BITACORA.md).

## Qué necesita la aplicación

| Componente | Para qué | Tamaño | Cómo llega al usuario |
|---|---|---|---|
| **pdfium** (BSD-3) | PDF ↔ imágenes, PDF → TXT/MD | 7 MB | Siempre dentro del instalador |
| **LibreOffice** (MPL 2.0) | DOCX/DOC/MD ↔ PDF | 1,5 GB extraído → **690 MB podado** (139 MB comprimido) | Depende del instalador y del sistema (abajo) |
| WebView2 (solo Windows) | La ventana de la paleta | — | Viene con Windows 11; en Windows 10, el instalador lo descarga |
| Tesseract (futuro) | OCR | por medir | Mismo mecanismo que LibreOffice |

Word, si está instalado, sirve de respaldo para DOCX ↔ PDF, pero la aplicación ya no depende de él.

## Windows (hecho y probado)

**Un solo instalador** NSIS por usuario (no pide administrador), `Converter_<versión>_x64-setup.exe` (~146 MiB), generado por `scripts/build-installers.ps1`. Tiene una página «Tipo de instalación» que explica y deja elegir:

| Opción | En disco | Contenido | Cuándo usarla |
|---|---|---|---|
| **Completa** (recomendada, por defecto) | ~715 MB | App + pdfium + LibreOffice podado | Funciona sin internet desde el primer clic. |
| **Ligera** | ~24 MB | App + pdfium | Quien ya tiene LibreOffice o poco disco: la paleta ofrece descargarlo (374 MB) la primera vez que hace falta, y se puede instalar o quitar después desde la tuerca. |

El archivo pesa lo mismo con cualquiera de las dos (lleva el LibreOffice dentro): elegir «ligera» ahorra disco, no descarga. Sin ventana: `/S` instala la completa; `/S /TYPE=ligera` la ligera. Reinstalar o actualizar conserva lo que había.

La página y la copia condicional de LibreOffice no existen en Tauri: están en `crates/palette-app/nsis/installer.nsi`, copia de la plantilla de `tauri-bundler 2.10.3` (CLI `2.12.3`, fijado en el script) con los añadidos marcados «Converter». El script fija `CVT_LO_DIR`; sin esa variable el instalador no lleva LibreOffice ni muestra la página (`-SinLibreOffice`).

Verificado: el `converter.exe` de release con la carpeta de componentes **vacía** y forzando LibreOffice convirtió un DOCX en 22 s sin ofrecer ninguna descarga, es decir, usó el LibreOffice incluido (`<instalación>\libreoffice\`). El instalador único se instaló en carpetas de `%TEMP%` (silencioso con ambos tipos, y con ventana eligiendo «Ligera»); falta elegir «Completa» en la ventana y el desinstalador real.

Orden en que el motor busca LibreOffice: variable `CONVERTER_LIBREOFFICE` → incluido en la app → descargado por la app (`%LOCALAPPDATA%\Converter\components`) → instalado en el sistema → `PATH`. Si no hay ninguno, Word (si existe) y, si no, la oferta de descarga.

## Desinstalar (Windows)

- **El desinstalador limpia lo descargado**: al desinstalar, borra `%LOCALAPPDATA%\Converter\components` (el LibreOffice que descargó la app, ~700 MB), salvo cuando se está **actualizando**. Con la casilla «borrar los datos de la aplicación» borra además los ajustes. Con la instalación **completa** no hay nada que limpiar: LibreOffice vive en la carpeta de instalación y se va con ella.
- **Desde la propia aplicación** (tuerca → Mantenimiento): «Quitar LibreOffice descargado» libera el espacio sin desinstalar nada, y «Desinstalar Converter…» abre el desinstalador de Windows.
- Estado: el gancho está probado con carpetas desechables y el botón abre el desinstalador; **el desinstalador real de punta a punta aún no**.

## Linux (declarado, sin probar)

- **`.deb` y `.rpm`**: el paquete declara `libreoffice-writer` como dependencia (`bundle.linux` en `tauri.bundle*.conf.json`). `apt`/`dnf` lo instalan solos al instalar la aplicación: es la forma correcta en Linux, sin duplicar 700 MB en cada máquina.
- **AppImage**: no puede declarar dependencias, así que habría que incluir LibreOffice dentro. TDF publica `.deb` en un tarball que se puede extraer con `dpkg-deb -x` y podar igual que el de Windows.
- Falta el script que descargue pdfium para Linux (los SHA-256 de las versiones fijadas se obtienen de la API de releases de `bblanchon/pdfium-binaries`).

## macOS (diseñado, sin probar)

- Incluir `LibreOffice.app` (extraída del `.dmg` con `hdiutil`) en `Converter.app/Contents/Resources/` y podarla. El motor ya busca ahí (`../Resources/LibreOffice.app/Contents/MacOS/soffice`).
- **Requiere firmar y notarizar**: todo binario anidado debe llevar la firma del desarrollador (Developer ID) y el *hardened runtime*, y eso exige una cuenta de Apple Developer. Sin notarizar, Gatekeeper bloquea la aplicación.
- Hacen falta dos variantes (Intel y Apple Silicon) o un binario universal.

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

Piezas: `scripts/fetch-pdfium.ps1` (pdfium, SHA-256 fijado), `cargo run -p components --example prepare_bundle -- vendor/bin/bundle` (descarga con reanudación, verifica el SHA-256, extrae con `msiexec /a` y poda), `crates/palette-app/tauri.bundle.conf.json` (recursos y plantilla) y `crates/palette-app/nsis/installer.nsi` (página de selección y copia de LibreOffice vía `CVT_LO_DIR`).

## Licencias que hay que cumplir al redistribuir

- **LibreOffice (MPL 2.0)**: permite redistribuirlo, con sus avisos. La poda conserva `readmes/` y el instalador lo incluye. Pendiente: un archivo `THIRD_PARTY_LICENSES` propio que enumere todo lo incluido (LibreOffice, pdfium, las dependencias de Rust) y el enlace al código fuente de LibreOffice.
- **pdfium (BSD-3)**: `LICENSE-pdfium` ya se instala junto a la DLL.
- La aplicación es `MIT OR Apache-2.0` y no enlaza nada GPL.

## Pendiente (en orden de valor)

1. Firma de código en Windows (sin ella, SmartScreen avisa) y en macOS (obligatoria).
2. Scripts de preparación de pdfium y LibreOffice para macOS y Linux, y construir allí los paquetes (solo se puede probar en esos sistemas).
3. Para Windows 10 sin internet: `webviewInstallMode: offlineInstaller` en el instalador completo (suma ~130 MB, pero elimina la única descarga que le queda).
4. Archivo de licencias de terceros.
5. Auto-actualización.

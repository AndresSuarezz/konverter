# Converter

**Convierte archivos con el mínimo esfuerzo.** Selecciona un archivo, pulsa un atajo y elige el formato de destino en una **paleta radial** que aparece junto al cursor. Un clic y el resultado queda al lado del original, sin sobrescribir nada.

<p align="center">
  <img alt="Licencia: MIT OR Apache-2.0" src="https://img.shields.io/badge/licencia-MIT%20OR%20Apache--2.0-blue">
  <img alt="Escrito en Rust" src="https://img.shields.io/badge/escrito%20en-Rust-orange">
  <img alt="Windows, Linux, macOS" src="https://img.shields.io/badge/plataformas-Windows%20%C2%B7%20Linux%20%C2%B7%20macOS-lightgrey">
  <img alt="Sin cuentas ni telemetría" src="https://img.shields.io/badge/sin%20cuentas-sin%20telemetr%C3%ADa-green">
</p>

- [Cómo funciona](#cómo-funciona)
- [Estado y plataformas](#estado-y-plataformas)
- [Qué convierte](#qué-convierte)
- [Instalación](#instalación)
- [Uso](#uso)
- [Ajustes](#ajustes)
- [Privacidad y programas externos](#privacidad-y-programas-externos)
- [Comportamientos que conviene conocer](#comportamientos-que-conviene-conocer)
- [Compilar desde el código](#compilar-desde-el-código)
- [Arquitectura](#arquitectura)
- [Documentación del proyecto](#documentación-del-proyecto)
- [Contribuir](#contribuir)
- [Licencia](#licencia)

## Cómo funciona

1. Seleccionas uno o varios archivos en el Explorador de Windows o en el Escritorio.
2. Pulsas **`Ctrl` + `Alt` + `Espacio`**.
3. Aparece la **paleta radial** centrada en el cursor:
   - en el **centro**, el formato del archivo (detectado por su contenido, no por la extensión);
   - alrededor, hasta **7 formatos de destino**, ordenados por lo coherentes que son (de JPG se ofrece antes WEBP que BMP); con varios archivos, solo los destinos comunes a todos;
   - abajo a la derecha, la **tuerca** de ajustes.
4. Eliges un pétalo con el ratón o con las teclas `1`–`7`. Se convierte y el resultado se guarda junto al original (`foto.webp`, y si ya existe, `foto (1).webp`). `Esc` cierra la paleta.

Si el origen es un PDF de varias páginas y el destino una imagen, se crea una carpeta nueva con una imagen por página.

## Estado y plataformas

El proyecto está en **fase inicial (0.1.0)**: el motor de conversión y la aplicación funcionan, pero faltan piezas (ver [Hoja de ruta](#hoja-de-ruta)).

| Sistema | Estado | Qué está probado |
|---|---|---|
| **Windows 11** (Windows 10 sin probar) | ✅ **Funciona de extremo a extremo** | Atajo global, lectura de la selección del Explorador, paleta, conversión, instalador, desinstalación y ajustes, probado a mano en dos equipos |
| **Linux** (Ubuntu 22.04 como base) | 🟡 **Compila y se empaqueta; sin atajo ni lectura de selección** | 148 tests pasan, `clippy` limpio, se generan `.deb`, `.rpm` y AppImage con WordCraft dentro, DOCX/Markdown/DOC → PDF tardan ~0,35 s desde el paquete y las 5 conversiones de documentos funcionan también con el LibreOffice del sistema. Falta leer la selección del gestor de archivos y el atajo global, y ver la ventana en un escritorio real |
| **macOS** | ⬜ **Sin probar** | Hay configuración de empaquetado y de integración continua, pero nadie lo ha compilado todavía en un Mac |

En Linux y macOS la paleta se abre con archivos pasados como argumento o arrastrándolos sobre ella (en Linux, el acceso directo también declara los formatos que convierte para «Abrir con», aunque aún no se ha comprobado en un gestor de archivos real); todavía no lee lo que tienes seleccionado en el gestor.

## Qué convierte

Un par de formatos solo aparece en la paleta si tiene tests con archivos reales.

| Familia | Conversiones | Cómo se hace |
|---|---|---|
| **Imágenes** | JPG, PNG, WEBP, BMP, TIFF y GIF, **todos contra todos** | Rust puro (WEBP con `libwebp` enlazado estáticamente) |
| **Datos** | CSV ↔ XLSX ↔ JSON, todos contra todos | Rust puro |
| **Imagen → PDF** | Cualquier imagen de la familia anterior → PDF | Rust puro; un JPG se incrusta byte a byte, sin recomprimir |
| **PDF → imagen** | PDF → PNG, JPG, WEBP o TIFF (una imagen por página) | [pdfium](https://pdfium.googlesource.com/pdfium/) |
| **PDF → texto** | PDF → TXT y PDF → Markdown | pdfium; el Markdown se reconstruye por heurística (títulos, listas, código) |
| **Documentos** | DOCX, DOC y Markdown → PDF; DOC → DOCX | [WordCraft](https://github.com/storytold/wordcraft) (Rust puro, viene con la aplicación): décimas de segundo, sin internet |
| **Markdown → Word** | Markdown → DOCX (títulos, listas, tablas con ancho por contenido, código, citas, enlaces) | Rust puro; también es lo que usa WordCraft para imprimir el Markdown |
| **PDF → Word** | PDF → DOCX (aproximado) | [LibreOffice](https://www.libreoffice.org/) o Microsoft Word (Windows): WordCraft no lo hace |
| **Word → texto** | DOCX → TXT | Rust puro |

Todavía **no** hay: OCR (los PDF escaneados dan un mensaje claro), HEIC, AVIF, varias imágenes en un solo PDF, menú contextual del Explorador ni arranque con Windows.

## Instalación

### Windows (x64)

Descarga `Converter_<versión>_x64-setup.exe` desde la sección **Releases** del repositorio y ejecútalo. No pide permisos de administrador. Como el instalador aún no está firmado, Windows SmartScreen puede avisar: pulsa *Más información → Ejecutar de todas formas*.

El instalador pesa unos **16 MB** (58 MB en disco) e incluye la aplicación, pdfium y WordCraft: convierte imágenes, datos, PDF y documentos de Word (DOCX y DOC) y Markdown **sin internet y sin instalar nada más**. Solo **PDF → Word** necesita LibreOffice: la primera vez la aplicación ofrece descargarlo (374 MB), o usa el que ya tengas instalado o Microsoft Word. En la tuerca, *Quitar LibreOffice descargado* libera ese espacio.

¿Necesitas PDF → Word sin internet? Hay una variante, `...-setup-con-libreoffice.exe` (~155 MB), que lleva LibreOffice dentro y te deja elegir en una pantalla entre *Estándar* y *Con LibreOffice* (~750 MB en disco). Instalación sin ventana: `Converter_..._setup.exe /S` (y `/TYPE=completa` en la variante con LibreOffice). Si tienes Windows 10 sin WebView2, el instalador lo descarga.

Para desinstalar: *Configuración → Aplicaciones*, o la tuerca → *Desinstalar Converter*. Al desinstalar se borra también el LibreOffice que haya descargado la aplicación.

### Linux

Los paquetes los genera el flujo de lanzamiento (`.github/workflows/release.yml`, que aún no se ha ejecutado en GitHub) o se construyen a mano según [Compilar](#compilar-desde-el-código):

```bash
sudo apt install ./Converter_0.1.0_amd64.deb         # Debian, Ubuntu y derivadas
sudo dnf install ./Converter-0.1.0-1.x86_64.rpm      # Fedora, RHEL, openSUSE
chmod +x Converter_0.1.0_amd64.AppImage && ./Converter_0.1.0_amd64.AppImage
```

Los paquetes llevan WordCraft dentro. `libreoffice-writer` solo hace falta para **PDF → Word** (instálalo con tu gestor de paquetes; si falta, la aplicación te dice cómo). Los `.deb` y `.rpm` recomiendan unas fuentes pequeñas compatibles con Calibri y Arial (`fonts-crosextra-carlito`, `fonts-liberation`) para que los documentos de Word se maqueten como en Word. Necesitan glibc 2.35 o superior (Ubuntu 22.04+, Debian 12+, Fedora 36+). Una vez instalado, «Abrir con Converter» aparece para los formatos que convierte; desde una terminal: `converter foto.png informe.docx`.

En Wayland los atajos globales dependen del sistema y pueden no funcionar; abrir con archivos sí.

### macOS

Pendiente de probar. La configuración de `.app` y `.dmg` existe (`crates/palette-app/tauri.bundle.macos.conf.json`) pero **sin firmar ni notarizar** macOS bloqueará la aplicación (Gatekeeper). Instala LibreOffice con `brew install --cask libreoffice` solo si quieres convertir PDF a Word.

## Uso

### Con el atajo (Windows)

Selecciona archivos y pulsa `Ctrl` + `Alt` + `Espacio`; vuelve a pulsarlo para cerrar la paleta. Si la ventana en primer plano no es el Explorador ni el Escritorio, se abre vacía para que **arrastres** un archivo encima. Mientras está abierta, `1`–`7` eligen un pétalo, `0` abre los ajustes y `Esc` la cierra. La paleta se esconde sola al perder el foco, salvo mientras convierte, descarga algo o tienes abiertos los ajustes.

### Desde la bandeja del sistema

La aplicación queda en la bandeja: clic en el icono para abrir la paleta; el menú tiene *Mostrar paleta*, *Configuración…*, *Instalar LibreOffice…* (donde se pueda descargar) y *Salir*.

### Desde la línea de comandos

```bash
converter foto.png informe.docx      # abre la paleta con esos archivos
```

Si Converter ya está en marcha, esos archivos se envían a la instancia abierta (solo hay una por usuario). Es la base del menú contextual y de «Enviar a».

### Solo el motor, sin ventana

El núcleo es una biblioteca; este ejemplo convierte un archivo a mano e imprime lo escrito:

```bash
cargo run -p converter-core --example convert -- archivo.docx pdf [word|libreoffice]
```

## Ajustes

La **tuerca** abre un panel con ajustes persistentes (guardados en `settings.json`):

| Ajuste | Valores |
|---|---|
| Programa de documentos | Automático (WordCraft para lo que sabe hacer; para PDF → Word, LibreOffice o Word), WordCraft, LibreOffice o Word, con el estado de cada uno |
| Calidad JPG / WEBP | 30–100 (por defecto 90 y 80) |
| Papel de los PDF | A4 o Carta |
| Resolución PDF → imagen | 72–600 ppp (por defecto 150) |
| Atajo | Solo lectura por ahora |
| Mantenimiento | Quitar el LibreOffice descargado; desinstalar Converter |

Un archivo de ajustes ausente, dañado o con valores absurdos nunca impide convertir: se vuelve a los valores por defecto.

## Privacidad y programas externos

- **Sin cuentas, sin telemetría y sin conexión** durante el uso normal. La única actividad de red es la descarga opcional de LibreOffice (Windows), con tamaño y huella SHA-256 fijados en el código, que se verifica antes de usarlo.
- Los archivos **nunca salen de tu equipo**. El resultado se escribe de forma atómica y no se sobrescribe nada.
- Programas externos:

| Programa | Para qué | Licencia | Cómo llega |
|---|---|---|---|
| **pdfium** | PDF ↔ imagen/texto | BSD-3 | Incluido en el instalador |
| **WordCraft** | DOCX/DOC/Markdown → PDF y DOC → DOCX | MIT OR Apache-2.0 | Incluido en la aplicación (versión fijada: v0.8.0) |
| **LibreOffice** | PDF → DOCX (y motor alternativo para el resto) | MPL-2.0 | Descargado bajo demanda (Windows), del sistema o incluido en la variante grande |
| **Microsoft Word** | Alternativa a LibreOffice (solo Windows, si ya lo tienes) | — | No se instala |

El código no enlaza nada GPL.

## Comportamientos que conviene conocer

- **El formato se detecta por contenido.** Un JPG renombrado a `.png` se convierte como JPG.
- **Orientación EXIF**: las fotos de móvil guardadas «tumbadas» se giran bien. Los metadatos (EXIF, perfiles de color) **no** se conservan.
- **PNG con transparencia → JPG**: se compone sobre fondo **blanco**.
- **GIF y WEBP animados, y TIFF de varias páginas**: se convierte solo el primer fotograma o página.
- **JPG cortado o dañado**: da un error claro en vez de una imagen a medias.
- **CSV**: se detecta el separador (el `;` de Excel en español) y la codificación (UTF-8 o Windows-1252). `007` y números de más de 15 dígitos se quedan como texto. La salida lleva BOM UTF-8 y saltos CRLF.
- **XLSX → otros**: solo la **primera hoja**; se conservan valores, no fórmulas ni estilos. **JSON** debe ser un arreglo de objetos.
- **PDF**: el WEBP admite hasta 16 383 px por lado; PDF → imagen se limita a 10 000 px. Un PDF **escaneado** (sin texto) devuelve «necesita OCR». PDF → Markdown y PDF → DOCX son **aproximaciones**: no reconocen tablas ni imágenes.
- **Markdown → PDF**: el HTML en bruto se escapa, las imágenes **no se descargan** (salen como `[imagen: texto alternativo]`) y solo se conservan enlaces `http`, `https` y `mailto`. Soporta títulos, listas anidadas y numeradas, tablas (con el ancho de cada columna según su contenido, cabecera sombreada que se repite en cada página y alineación), código, citas, reglas, casillas y enlaces reales; no soporta notas al pie, fórmulas ni imágenes. El HTML en bruto (por ejemplo las insignias de un README) se reduce a su texto.
- **WordCraft es un proyecto en fase alfa** y usa las **fuentes del equipo**: un documento con una fuente que no tengas se maqueta con otra parecida y la paginación puede diferir de la de Word. En las pruebas, DOCX → PDF salió casi idéntico al de LibreOffice. Si algún documento sale mal, elige LibreOffice o Word en la tuerca.
- **PDF → Word con LibreOffice tarda ~20 s** (arranca con un perfil nuevo); el resto de conversiones de documentos, menos de 2 s.

## Compilar desde el código

Necesitas **Rust 1.88 o superior** (edición 2024) y un compilador de C (WEBP). Después, pdfium, que se descarga con hash verificado a `vendor/bin/pdfium` (ignorado por git):

```bash
# Windows (PowerShell)
powershell -File scripts/fetch-pdfium.ps1
powershell -File scripts/build-wordcraft.ps1     # opcional, ~5 min: el motor de documentos (v0.8.0)
# Linux y macOS
bash scripts/fetch-pdfium.sh
bash scripts/build-wordcraft.sh

cargo build --workspace
cargo test --workspace            # sin pdfium fallan los tests de PDF, a propósito
cargo test -p converter-core --test office -- --ignored wordcraft   # conversiones reales con WordCraft (necesita build-wordcraft)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check                  # licencias y avisos de seguridad (cargo install cargo-deny)
```

La interfaz (`crates/palette-app/ui/`) es HTML/CSS/JS estático **embebido en el binario**: no hay Node ni bundler. Tras tocarla hay que recompilar; con la aplicación abierta `cargo build` falla con «Acceso denegado» (ciérrala antes).

**Dependencias del sistema en Linux** (Ubuntu/Debian): `build-essential pkg-config libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev`.

### Paquetes e instaladores

```bash
# Windows, en dist/: el instalador normal (~16 MB, con WordCraft)...
powershell -File scripts/build-installers.ps1
# ...o la variante con LibreOffice dentro y página de elección (~155 MB)
powershell -File scripts/build-installers.ps1 -ConLibreOffice

# Linux (desde crates/palette-app): .deb, .rpm y AppImage
npx @tauri-apps/cli@2.12.3 build --config tauri.bundle.linux.conf.json

# macOS (desde crates/palette-app): .app y .dmg
npx @tauri-apps/cli@2.12.3 build --config tauri.bundle.macos.conf.json
```

Usa `CARGO_TARGET_DIR` aparte para no bloquear tu `target`. La integración continua (`.github/workflows/`) ejecuta los tests en los tres sistemas y construye los paquetes. Detalles de cada sistema, tamaños y lo que está sin probar: [docs/DISTRIBUCION.md](docs/DISTRIBUCION.md).

## Arquitectura

```
crates/
├── converter-core   Motor: una biblioteca pura, sin interfaz ni llamadas al sistema.
├── components       Descarga e instala componentes (LibreOffice). Es el único crate con red.
├── os-shell         Lee la selección del gestor de archivos del sistema (COM en Windows).
└── palette-app      La aplicación (Tauri 2): paleta radial, atajo, bandeja, ajustes.
```

El **grafo de conversión** es la idea central: los formatos son nodos y cada conversor declara qué pares sabe hacer y con qué fidelidad (con o sin pérdida). El motor pregunta a cada conversor y esa es la única fuente de lo que se ofrece; la paleta solo pinta esa lista, ordenada por una tabla de probabilidades a priori. Los conversores trabajan sobre bytes, no sobre rutas, y el motor escribe el resultado de forma atómica sin sobrescribir.

Todo lo específico de cada sistema (atajo, selección del gestor, instalación) vive fuera del núcleo, de modo que `cargo test -p converter-core` corre sin entorno gráfico. Más detalle en [CLAUDE.md](CLAUDE.md).

## Documentación del proyecto

| Documento | Contenido |
|---|---|
| [docs/REQUERIMIENTOS.md](docs/REQUERIMIENTOS.md) | Requisitos, flujo de uso, conversiones, OCR y decisiones abiertas |
| [docs/BITACORA.md](docs/BITACORA.md) | Historial, **registro de decisiones** (D-NN), matriz requisito → código → pruebas y pendientes |
| [docs/DISTRIBUCION.md](docs/DISTRIBUCION.md) | Instaladores y paquetes por sistema, tamaños, licencias y lo que falta probar |
| [docs/REFERENCIAS-CRAFT.md](docs/REFERENCIAS-CRAFT.md) | Análisis de PdfCraft y WordCraft: qué se adoptó, qué no y por qué |
| [CLAUDE.md](CLAUDE.md) | Guía técnica para quien programa (o usa un asistente de código) en el repositorio |

### Hoja de ruta

Pendiente, en orden aproximado de valor:

1. Probar y cerrar **macOS** y la lectura de la selección y el atajo en **Linux** y macOS.
2. **OCR** con Tesseract local, descargado bajo demanda (interruptor «con OCR» de la paleta ya previsto).
3. **Varias imágenes → un PDF**; HEIC; menú contextual del Explorador; arranque con Windows.
4. **Firma de código** (Windows y macOS) y auto-actualización.
5. Más formatos de documento gracias a WordCraft (ODT, RTF, HTML, DOCX → Markdown; ver [REFERENCIAS-CRAFT.md](docs/REFERENCIAS-CRAFT.md)) y valorar [`hayro`](docs/REFERENCIAS-CRAFT.md) para quitar pdfium.

## Contribuir

Las contribuciones son bienvenidas. Normas del proyecto:

- Una conversión solo se publica si tiene **un test con un archivo real** (`crates/converter-core/tests/fixtures/`; hay un test que recorre el grafo y falla si falta el fixture de un par).
- El núcleo no debe entrar en pánico ante entradas malformadas: errores como tipos (`thiserror`), nunca `unwrap` sobre datos del usuario.
- Nada de dependencias GPL. `cargo deny check` lo vigila.
- Antes de proponer un cambio: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` y `cargo test --workspace`.
- Si tocas lo visible, **actualiza [docs/BITACORA.md](docs/BITACORA.md)**: qué se hizo, la decisión si la hay, y qué quedó sin probar.

## Licencia

Doble licencia, a tu elección: [MIT](LICENSE-MIT) o [Apache-2.0](LICENSE-APACHE).

pdfium (BSD-3) y LibreOffice (MPL-2.0) conservan sus propias licencias; sus avisos viajan con la aplicación (`pdfium/LICENSE-pdfium`, `libreoffice/readmes`).

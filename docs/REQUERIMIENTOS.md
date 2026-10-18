# Requerimientos — Converter

Aplicación de escritorio en Rust para convertir archivos de forma muy fácil: seleccionas un archivo (en el Escritorio o en el Explorador), pulsas un atajo de teclado y aparece una **paleta radial** junto al cursor con los formatos de destino más coherentes. Un clic y se convierte.

Estado: **fase de definición**. Este documento es la fuente de verdad de los requisitos; las decisiones abiertas están en la sección 8.

## 1. Plataforma

- Objetivo: **Windows 11, macOS y Linux** (no móvil). Se desarrolla primero en Windows, pero ninguna decisión debe cerrar la puerta a las otras dos.
- El `core` es multiplataforma y no conoce ningún SO. Lo específico de cada SO (atajo global, leer la selección del gestor de archivos, menú contextual, códecs nativos) vive en crates de integración por plataforma detrás de traits definidos en el core.
- Limitaciones conocidas fuera de Windows: en macOS, la selección de Finder se lee por Apple Events/AppleScript y requiere permiso de Automatización; en Linux no hay una API única del gestor de archivos (Nautilus, Dolphin, Nemo...) y en Wayland los atajos globales dependen del portal `GlobalShortcuts`, así que allí la entrada principal puede ser arrastrar-y-soltar o acción del gestor de archivos.
- **Licencia: open source, `MIT OR Apache-2.0`.** Prohibido enlazar código GPL (p. ej. x265). Las librerías LGPL (libheif) solo en enlace dinámico.
- El boceto de referencia es una paleta estilo macOS.

## 2. Flujo de uso

1. El usuario selecciona uno o varios archivos (Escritorio o Explorador de archivos).
2. Pulsa el **atajo global** (comodidad ergonómica: una mano, sin teclas de función; valor por defecto por decidir, configurable). La app lee la selección del shell de Windows.
3. Aparece la **paleta radial** centrada en el cursor:
   - **Centro**: formato de origen detectado (p. ej. `TIFF`) y el interruptor **sin OCR / con OCR** (por defecto *sin OCR*).
   - **8 pétalos** con los formatos de destino, ordenados por coherencia/probabilidad (sección 4).
   - **La tuerca de configuración** ocupa siempre el pétalo inferior derecho (el hueco del boceto). Abre un panel con los ajustes (programa de documentos, calidades, papel, resolución y, más adelante, atajo y OCR). Antes estaba reservado para un servicio externo de pago; el usuario lo cambió (D-32).
4. El usuario elige un pétalo (clic, o tecla/número, o soltar el atajo sobre él). Se convierte y el resultado se guarda **junto al original** con el nuevo nombre/extensión, sin sobrescribir.
5. Notificación discreta de éxito/error con acceso a "mostrar en carpeta".

Vías de entrada adicionales (secundarias): arrastrar un archivo sobre la paleta (como en la referencia) y entrada de menú contextual del Explorador.

## 3. Conversiones soportadas

Principio: pocas conversiones **que funcionen todas y con buena calidad** antes que muchas a medias. Una conversión solo se publica en la paleta si tiene tests con archivos reales.

| Familia | Formatos | Notas |
|---|---|---|
| Imágenes | JPG, PNG, WEBP, HEIC, BMP, TIFF, GIF, AVIF, ICO | HEIC: la lectura es el caso común; la **escritura** de HEIC requiere códec HEVC (licencias/patentes) → evaluar |
| Documentos | imágenes → PDF ✅, PDF → imágenes (PNG/JPG/WEBP/TIFF) ✅, PDF → TXT ✅, PDF → MD ✅ (heurístico), MD → PDF ✅, DOCX/DOC → PDF ✅, PDF → DOCX 🟡, DOC → DOCX ✅, DOCX → TXT ✅, DOCX → MD ⬜ | DOCX/DOC/MD → PDF y DOC → DOCX usan WordCraft (incluido); PDF → DOCX, LibreOffice (descarga bajo demanda) o Word (ver sección 7). PDF → DOCX y PDF → MD reconstruyen la estructura: son aproximaciones. Varias imágenes → un solo PDF ⬜ |
| Datos tabulares | CSV ↔ XLSX, CSV ↔ JSON, XLSX → CSV | Puro Rust, alta fiabilidad |
| Texto/marcado | MD → PDF ✅, MD → DOCX ✅ (propio, Rust puro), MD → HTML ⬜, TXT → PDF ⬜ | MD → PDF con WordCraft pasa por el DOCX propio (D-47) |

Fuera de alcance por ahora: audio/vídeo, archivos comprimidos, CAD.

## 4. Conversión inteligente

La paleta no muestra todos los formatos posibles: muestra los que **tienen sentido** para el archivo de origen.

- **Grafo de conversión**: los formatos son nodos; cada conversor es una arista con *calidad* (sin pérdida / con pérdida) y *coste*. Se permiten rutas de varios saltos (p. ej. HEIC → PDF vía imagen), pero se marcan y se penalizan.
- **Ranking por coherencia**: prioridades estáticas por familia (JPG→WEBP/PNG es mucho más común que DOCX→JPG; este último queda fuera de los 8 pétalos o muy abajo).
- **Aprendizaje local**: contador de uso por par origen→destino, almacenado localmente, que reordena los pétalos con el tiempo. Sin telemetría.
- **Reglas de coherencia**: no ofrecer el propio formato; no ofrecer destinos que pierdan información crítica sin avisar (p. ej. PNG con transparencia → JPG); ofrecer *Combinar en un PDF* cuando hay varias imágenes seleccionadas.
- **Opciones por defecto razonables** (calidad WEBP/JPG, DPI de rasterizado de PDF) sin diálogos; ajustes avanzados solo en configuración.

## 5. OCR

- **Por defecto: sin OCR.** El interruptor del centro de la paleta lo activa por conversión.
- El OCR aplica a imágenes y PDF escaneados cuyo destino sea texto o documento (PDF con capa de texto, DOCX, TXT).
- Se abstrae detrás de un trait `OcrProvider`, de modo que el motor concreto sea intercambiable sin tocar el resto:
  - `LocalTesseract` — **motor OCR por defecto, local y multiplataforma** ([tesseractocr.org](https://tesseractocr.org/es/), Apache-2.0). Se invoca como **proceso externo** (`tesseract`), no como binding nativo, para no complicar el build en tres SO. El binario y los `tessdata` por idioma se **descargan bajo demanda** la primera vez que se activa el OCR (o se usa el `tesseract` del sistema si ya existe).
  - `RemoteService` — servicio externo (reservado, ver sección 6).
  - Proveedores nativos opcionales a futuro (`Windows.Media.Ocr`, Vision en macOS) si mejoran la calidad sin binarios extra.
- **Decidido (revierte el descarte inicial)**: Tesseract local, porque el proyecto debe correr en Windows, macOS y Linux y el OCR nativo de Windows no es portable.

## 6. Visión a futuro (no se implementa ahora)

- Servicio externo con **cuentas de usuario y suscripción**, donde el OCR de alta calidad corre como servicio aparte y se paga por usarlo. El pétalo vacío y `RemoteService` son los puntos de enganche.
- Consideración de producto: Tesseract es código abierto y cualquiera puede ejecutarlo en local, así que por sí solo difícilmente justifica una suscripción. El valor de pago estaría en calidad superior (modelos mejores, reconstrucción de maquetación para PDF→DOCX), comodidad y sincronización. Decidir antes de invertir en el backend.
- Por ahora: **sin cuentas, sin red, sin telemetría**. El core funciona 100 % sin conexión.

## 7. Realidades técnicas que condicionan el diseño

- **DOCX ↔ PDF** (resuelto, D-20/D-26/D-28): no existe una forma *madura* en Rust puro. Actualización de 2026-10-18: **WordCraft** (alfa, Rust puro, viaja con la aplicación) es ahora el motor principal de DOCX/DOC/Markdown → PDF y DOC → DOCX, con un resultado casi idéntico al de LibreOffice en las pruebas y 50 veces más rápido (D-41, D-44, ver [REFERENCIAS-CRAFT.md](REFERENCIAS-CRAFT.md)); LibreOffice o Word quedan para PDF → DOCX, que WordCraft no hace. Cascada implementada: **LibreOffice primero** (instalado, o descargado por la propia app: 374 MB una sola vez, verificado con SHA-256, ~1,5 GB en disco, nunca en el instalador base) y Word por PowerShell + COM como respaldo inmediato. Descarga automática solo en Windows x64; en macOS y Linux hay que instalar LibreOffice con el gestor de paquetes o apuntar `CONVERTER_LIBREOFFICE`.
- **PDF → DOCX** (reconstruir maquetación) es un problema abierto: lo realista es extracción de texto/estructura básica; la calidad alta requiere OCR/layout analysis.
- **HEIC**: x265 (el encoder HEVC de libheif) es GPL y está prohibido por la licencia elegida. Plan: **leer** HEIC con `libheif` (+ libde265, LGPL, enlace dinámico); **escribir** HEIC con los códecs nativos del SO (WIC en Windows, que exige la extensión HEVC de Microsoft instalada; ImageIO en macOS). En Linux la escritura de HEIC queda sin soporte hasta encontrar una vía sin GPL: no se ofrece en la paleta allí.
- **PDF → imagen/texto** (resuelto): `pdfium` (BSD-3) se carga en tiempo de ejecución desde `pdfium.dll`/`libpdfium.*` mediante `pdfium-render`; la versión fijada y su SHA-256 están en `scripts/fetch-pdfium.ps1`. Ocupa unos 7 MB descomprimida (3,7 MB descargada), así que la propuesta es empaquetarla con el instalador en lugar de descargarla bajo demanda. Imagen → PDF no necesita nada nativo (`pdf-writer`, Rust puro).
- **WEBP**: resuelto. La escritura usa libwebp (BSD-3) compilada desde el código fuente y enlazada estáticamente por el crate `webp`; requiere compilador de C (MSVC en Windows) al compilar, pero no añade nada al instalador. La lectura es Rust puro.
- Herramientas presentes hoy en la máquina de desarrollo: `rustc 1.99`, `cargo 1.99`, Node 24. **No** hay Tesseract, ImageMagick, LibreOffice, Pandoc ni FFmpeg instalados.

## 8. Decisiones abiertas

Resolverlas con la skill `grill-me` antes de escribir código de UI o de integración:

1. ~~**Shell de UI**~~ — **DECIDIDO: Tauri 2** (paleta en HTML/CSS/SVG con ventana transparente; el core sigue siendo Rust puro y la UI es un cliente fino).
2. ~~**Atajo global en Windows**~~ — **DECIDIDO**: `Ctrl+Alt+Espacio`, con lectura de la selección por COM (`IShellWindows`/`IFolderView2`), implementado. Pendiente: hacerlo configurable y definir el atajo en macOS/Linux.
3. ~~**Estrategia DOCX↔PDF**~~ — **DECIDIDO**: Word si existe, si no LibreOffice descargado bajo demanda (ver sección 7).
4. ~~**OCR local**~~ — **DECIDIDO**: Tesseract local como proceso externo con descarga bajo demanda; multiplataforma.
5. ~~**Distribución**~~ — **DECIDIDO** (D-30/D-31, ver [DISTRIBUCION.md](DISTRIBUCION.md)): en Windows, instalador **completo** (pdfium + LibreOffice podado dentro, ~146 MiB) y **ligero** (descarga bajo demanda); en Linux, paquetes que dependen de `libreoffice-writer`; en macOS, LibreOffice dentro del `.app` con firma y notarización. Falta probarlo en macOS y Linux, firmar el código y la auto-actualización.
6. ~~**Licencia**~~ — **DECIDIDO**: open source `MIT OR Apache-2.0`, sin GPL (pdfium BSD y Tesseract Apache-2.0 son compatibles; libheif LGPL solo dinámico).
7. **Abierta**: mecanismo concreto de selección de archivos en macOS y Linux (sección 1) y atajo por defecto en cada SO.

## 9. Criterios de calidad

- Cada conversor: tests con archivos reales en `tests/fixtures/` (entrada, salida esperada o propiedades verificables), incluido el caso de archivo corrupto/vacío.
- Nunca sobrescribir ni borrar el original; escritura atómica del resultado (archivo temporal + renombrado).
- Los errores se devuelven como tipos (`thiserror`) con mensaje accionable para el usuario; el `core` no hace `panic!` ante entradas malas.
- El `core` es una biblioteca sin UI y sin I/O de ventanas, probable desde `cargo test` sin entorno gráfico.

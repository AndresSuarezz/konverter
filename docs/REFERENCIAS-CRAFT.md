# Proyectos de referencia: PdfCraft y WordCraft

Análisis del 2026-10-18 de dos proyectos del equipo ArtCraft, también escritos en Rust y con la misma licencia (`MIT OR Apache-2.0`):

- **PdfCraft** 0.9.0 — <https://github.com/storytold/pdfcraft>: lector y editor de PDF.
- **WordCraft** 0.8.0 — <https://github.com/storytold/wordcraft>: procesador de textos que lee y escribe `.docx`.

Objetivo: ver qué puede simplificar **llevar Converter a macOS y Linux** y qué puede **facilitar o mejorar las conversiones**. Las decisiones que salen de aquí están en [BITACORA.md](BITACORA.md) (D-38 a D-46).

## Cómo se hizo

Se clonaron en una carpeta temporal (solo lectura, sin ejecutar nada dentro de ellos), se leyeron los README, los `CLAUDE.md`, el código de los crates relevantes, el empaquetado (`packaging/`) y la integración continua; se compilaron en *release* los dos CLI (`wordcraft-cli`: 4 min, 35 MB; `pdfcraft-cli`: 12 min 54 s, 40,6 MB) y se probaron con archivos reales contra lo que hace hoy Converter. **No se copió código**: lo adoptado son ideas y configuraciones propias (`deny.toml` adaptado, con su crédito). Todo lo descargado se borró al terminar.

Los dos se declaran **alfa**: PdfCraft dice estar al ~50 % de Acrobat Pro y WordCraft al ~60 % de Word, y los dos admiten que lo suyo en conversión es débil (PdfCraft: «sin Office → PDF; la exportación a Word pierde la maquetación»).

## Qué hay en cada uno (lo que nos importa)

| Pieza | Dónde | Qué hace | Dependencias |
|---|---|---|---|
| Lectura y escritura de `.docx`, `.doc` (Word 97-2003), ODT, RTF, HTML, Markdown, LaTeX, TXT | WordCraft: `crates/docx`, `docbin`, `formats` (~57 000 líneas) | Modelo de documento común | Rust puro |
| Maquetación (saltos de línea, paginación, tablas, encabezados y pies) y salida a PDF con texto seleccionable | WordCraft: `layout`, `pdf` (krilla) | DOCX → PDF sin LibreOffice ni Word | Rust puro; fuentes del sistema |
| Rasterizador de PDF | PdfCraft usa **`hayro`** (crates.io, `MIT OR Apache-2.0`) | PDF → imagen | Rust puro, sin DLL |
| Exportar PDF a DOCX / HTML / RTF / PPTX | PdfCraft: `crates/export` (3 archivos, solo depende de `flate2`) | Párrafos, títulos, tablas inferidas | Rust puro |
| Imágenes (y texto) → PDF, varias en uno | PdfCraft: `crates/create` | Combina PNG/JPEG/TIFF/GIF/BMP | Rust puro |
| OCR | PdfCraft: `crates/ocr` con `ocrs` + `rten` y dos modelos | Reconoce texto en imágenes | Rust puro + modelos descargables |
| Empaquetado multiplataforma | `packaging/` (Linux: AppImage, `.deb`, `.rpm`, Flatpak; macOS: `.dmg` firmado y notarizado; Windows: MSI firmado) y flujo de lanzamiento por sistema | Scripts y CI probados en producción | — |
| Política de calidad | `deny.toml`, `unsafe_code = "forbid"`, lints contra `unwrap/panic` | Licencias permisivas, «nunca se cae» | — |

## Resultados medidos

Todas las comparaciones, en esta máquina (Windows 11), con los mismos archivos y mirando el PDF resultante renderizado.

### DOCX → PDF: WordCraft frente a LibreOffice

| Documento | WordCraft | LibreOffice (nuestro motor actual) | Resultado visual |
|---|---|---|---|
| Informe de 3 páginas: título, subtítulos, negritas y cursivas en línea, tabla con bordes, párrafos justificados, acentos y `€ ½ «»` | **0,5 s** | ~17–20 s (arranque en frío con perfil nuevo) | **Prácticamente idéntico**, mismas 3 páginas |
| Manual de 2 páginas: imagen incrustada, lista numerada, encabezado, pie con «Página *n* de *N*» (campos `PAGE`/`NUMPAGES`) | **1,6 s** | 17 s | **Prácticamente idéntico**, mismas 2 páginas |

WordCraft también convirtió en menos de 0,3 s el `.doc` binario de prueba. Un binario de 35 MB sustituiría a 690 MB de LibreOffice podado.

### Lo que **no** mejora

| Conversión | Resultado | Por qué |
|---|---|---|
| **Markdown → PDF** | Peor con WordCraft que con nuestra ruta de entonces (HTML propio + LibreOffice). **Resuelto después (D-47)**: se genera un DOCX propio y WordCraft imprime ese | Con HTML, WordCraft **ignora los anchos de columna** (todas iguales) y no pone margen en las celdas; con DOCX sí los respeta |
| **PDF → DOCX** | **LibreOffice es claramente mejor** que el exportador de PdfCraft | PdfCraft parte las frases con negrita en párrafos sueltos, funde párrafos, pierde el justificado y los bordes de tabla |
| **OCR** | `ocrs` no nos sirve | Sus modelos **solo leen alfabeto latino sin acentos** (inglés): fallaría con *ñ, á, é…*. Los modelos además son CC-BY-SA-4.0. Tesseract con el idioma `spa` sigue siendo la decisión correcta |

### PDF → imagen: `hayro` frente a pdfium

Se compiló un proyecto desechable con `hayro` 0.8.0 (publicado, sin parches; ejecutable de 6,2 MB con las fuentes estándar) y se renderizaron a 150 dpi los mismos PDF que con pdfium.

| PDF | Píxeles que difieren (>25 %) | Tiempo por página (hayro) |
|---|---|---|
| Fixtures `sample` y `structured` (fuentes estándar) | 0,06 – 0,19 % | 16 – 35 ms |
| Informes de LibreOffice y de WordCraft (fuentes incrustadas) | 0,8 – 3,2 % | 34 – 66 ms |

Las diferencias son suavizado de bordes y un píxel de desplazamiento (la altura sale 1753 en vez de 1754). A simple vista son indistinguibles. **Limitaciones**: `hayro` no soporta PDF con contraseña (pdfium sí), y **no extrae texto**, que es lo que necesitan nuestro PDF → TXT y PDF → Markdown (hoy salen de pdfium). Reemplazar pdfium del todo exigiría adoptar también la extracción de texto de PdfCraft (`pdfcraft-edit`), que depende de su propio núcleo (`cos`, `lopdf` con parches vendidos).

## Veredicto por pieza

| Pieza | Decisión | Motivo |
|---|---|---|
| **Flujo de CI y lanzamiento por sistema** (matriz Windows/Linux/macOS, base `ubuntu-22.04`) | ✅ **Adoptado** (`.github/workflows/`) | Es la única forma de probar macOS sin un Mac |
| **`deny.toml` (cargo-deny)** | ✅ **Adoptado** | Comprueba licencias y avisos de seguridad; ya encontró cosas reales (abajo) |
| Idea del **`.desktop` con `%F` y tipos MIME** | ✅ **Adoptado** | Da «Abrir con Converter» en Linux sin leer la selección del gestor |
| Base **glibc 2.35** y recursos en `lib/<producto>/` | ✅ Adoptado, **medido** en el `.deb` real | |
| **WordCraft como motor de documentos** | ✅ **Adoptado como motor principal** (D-41, D-44–D-46) | Mucho más rápido y ligero y fiel en lo probado, pero es alfa y depende de las fuentes del sistema |
| **`hayro` en lugar de pdfium** | 🟡 Candidato a largo plazo | Quitaría la DLL por sistema y simplificaría la firma en macOS, pero no extrae texto ni abre PDF con contraseña |
| **Varias imágenes → un PDF** con `pdfcraft-create` | 🟡 Referencia útil (ya estaba en pendientes) | Maneja CMYK, TIFF y orientación; no se copió |
| Exportador PDF → DOCX/HTML/RTF/PPTX de PdfCraft | ❌ Descartado para DOCX; HTML/RTF/PPTX quedan como idea | LibreOffice es mejor en DOCX |
| `ocrs` + `rten` | ❌ Descartado (D-42) | Sin acentos |
| Firma y notarización en macOS, MSI firmado | 📌 Referencia para cuando haya certificados | Sus scripts (`packaging/macos/package.sh`) muestran el flujo completo |

## Lo que encontró `cargo deny` en nuestro propio árbol

Instalar la política de PdfCraft sirvió de inmediato:

- **Licencias**: cinco crates de Tauri son **MPL-2.0** (`cssparser`, `selectors`, `dtoa-short`, `option-ext`, `cssparser-macros`) y uno es de datos **CDLA-Permissive-2.0** (`webpki-roots`). Ninguna es GPL: la MPL solo obliga a mantener abiertos los archivos MPL que se modifiquen (la misma licencia de LibreOffice). Se permiten de forma explícita y se documentan.
- **Seguridad**: tres avisos del crate `zip` 8.6.0, que trae `calamine` (nuestro lector de XLSX; su última versión aún exige `zip ^8.6`). Los tres afectan solo a `ZipArchive::extract()`, que nadie llama (calamine lee las entradas en memoria; nuestro `zip` directo es el 9.x, ya corregido). Se registran con su motivo en `deny.toml` y hay que revisarlos cuando salga un calamine con `zip 9`.
- Cuatro avisos de «sin mantenimiento» (`zvariant`, `zbus_names`, `zvariant_derive`, `proc-macro-error`) que llegan por Tauri en Linux. No son vulnerabilidades.

## WordCraft como motor de documentos (aceptado el 2026-10-18)

Es la mejora de conversión más grande que ofrecen estos proyectos. Propuesto como motor opcional, **el usuario decidió usarlo como motor principal** para dejar de depender de LibreOffice y aligerar la aplicación (D-41, D-44, D-45, D-46). Lo que se hizo:

1. **Como proceso externo** (`wordcraft-cli convert entrada salida`), igual que LibreOffice. La elección es **por par de formatos**: WordCraft para DOCX/DOC/Markdown → PDF y DOC → DOCX; LibreOffice o Word para PDF → DOCX, que WordCraft no hace. Se compila con `scripts/build-wordcraft.ps1`/`.sh` desde la etiqueta **v0.8.0** (commit fijado, `--locked`) y viaja con la aplicación, con sus licencias.
2. **Resultado**: el instalador de Windows pasa de 146 MiB a **16 MiB**; las conversiones de documentos, de ~20 s a 0,3–1,4 s.
3. **Pendiente**: los formatos que da gratis (ODT, RTF, HTML, TeX; DOCX → MD, DOCX → HTML), cada uno con su fixture y su test, y probarlo con documentos reales y complejos.

Descartado: enlazarlo como biblioteca (`git` dependency). Meter ~190 000 líneas alfa en el árbol alarga la compilación, exige replicar su `[patch.crates-io]` de `krilla` y hace que `cargo build` necesite red para resolver aunque la función esté apagada.

**Riesgos a verificar antes de usarlo por defecto**: es alfa (su propio README pide «paginación más cercana a Word» y «barrido de estabilidad» para llegar a beta); las fuentes (usa las del sistema y trae Source Sans/Serif como último recurso, así que sin Carlito/Calibri la paginación puede diferir de Word, mientras que LibreOffice lleva fuentes métricamente compatibles); no se ha fuzzeado con documentos hostiles; y solo se probaron dos documentos sintéticos.

## Licencias y atribución

Los dos proyectos son `MIT OR Apache-2.0`, compatibles con el nuestro. Como no se copió código no hay avisos que añadir; si en el futuro se incorpora código o un binario suyo, hay que conservar sus avisos (`NOTICE`, `ATTRIBUTION.md`). **Las marcas y logotipos de ArtCraft (`docs/brand/`) no son de código abierto y no se pueden reutilizar.** Los modelos de OCR de `ocrs` son CC-BY-SA-4.0 (con copyleft): otra razón para no incorporarlos.

# Bitácora de desarrollo

Registro cronológico de qué se hizo, qué se decidió y por qué, para poder reconstruir el proyecto sin depender de la memoria de nadie. Complementa a [REQUERIMIENTOS.md](REQUERIMIENTOS.md) (qué se quiere) y a [../CLAUDE.md](../CLAUDE.md) (cómo trabajar en el código).

**Cómo mantenerla** (obligatorio al cerrar cada tanda de trabajo):
1. Añadir una entrada en [Historial](#historial) (la más reciente arriba) con fecha, qué cambió y cómo se verificó. Si algo quedó sin probar, decirlo.
2. Registrar cada decisión nueva, o revertida, en el [Registro de decisiones](#registro-de-decisiones) con su motivo. Las decisiones no se borran: se marcan como *Revertida* y se enlaza la que las sustituye.
3. Actualizar la [Matriz de trazabilidad](#matriz-de-trazabilidad) (requisito → estado → código → pruebas).
4. Mover a [Pendientes](#pendientes) lo que se descubra y no se resuelva en el momento.

---

## Historial

### 2026-10-18 · Sesión 1 (cont. 10) — Markdown → PDF fiable (y Markdown → DOCX)

**Petición del usuario**: «necesito de alguna manera poder pasar de Markdown a PDF». La conversión existía, así que se buscó por qué no servía: se probó con documentos **reales y duros** del propio proyecto (`README.md`, `BITACORA.md`, `CLAUDE.md`…).

**Hallazgo**: con WordCraft, el Markdown con **tablas** salía mal: el importador de HTML de WordCraft **ignora todo ancho de columna** (probado con `width` en `th`/`td`, `colgroup`/`col` y `style`: siempre columnas iguales), lo que aplastaba la columna larga y apretaba el texto; además no hay margen en las celdas ni color en los enlaces planos y una fila se perdía al partir la página. (Sí respeta `bgcolor` en `th`, `style="color"` en enlaces y el fondo de `pre`.) LibreOffice lo hacía mucho mejor, pero era justo lo que se quería dejar de necesitar.

**Resultado**: `md_docx.rs` (D-47): Markdown → **DOCX** en Rust puro, y WordCraft imprime ese DOCX (que sí respeta anchos de tabla). Es además un par nuevo para la paleta: **Markdown → DOCX** (pendiente desde el principio).
1. **Qué cubre**: títulos con borde, párrafos, negrita/cursiva/tachado, código en línea y en bloque (sombreado, con sangría), listas anidadas con viñetas o numeradas **desde el número del documento**, casillas, citas, reglas, enlaces reales (`w:hyperlink`, solo http/https/mailto) y tablas con **ancho por contenido**, cabecera sombreada que se repite en cada página, filas que no se parten y alineación. HTML en bruto reducido a su texto (con `<img alt>` → `[imagen: alt]` y `<script>`/`<style>` descartados); imágenes nunca descargadas; caracteres de control eliminados; 63 columnas como máximo; papel A4/Carta según el ajuste.
2. **Dos defectos encontrados al ver el resultado y corregidos**: las columnas cortas partían palabras («Famili-a») → el ancho mínimo de cada columna es su palabra más larga (~135 vtw/carácter, el peor caso en negrita); y una ruta larguísima sin espacios inflaba el mínimo de su columna y aplastaba a las demás → la palabra cuenta como mucho 14 caracteres. Además un fallo de seguridad en mi primera versión, cazado por un test: el texto **entre** `<script>` y `</script>` en línea llegaba como evento aparte y se colaba.
3. **Medido** (WordCraft, depuración): README 7 páginas en 2,3 s; `BITACORA.md` 20 páginas en 2,3 s; `CLAUDE.md`, `DISTRIBUCION.md` y `REFERENCIAS-CRAFT.md` en menos de 1 s. Revisado a ojo: tablas con anchos por contenido y ninguna palabra partida, listas anidadas, código, enlaces azules; mejor que el HTML con WordCraft y comparable a LibreOffice.
4. **Pruebas**: 15 unitarios de `md_docx` (paquete XML válido, anchos, cabecera repetida, alineación, listas con su inicio, enlaces seguros, HTML hostil, entrada con caracteres de control, anidación y 100 columnas, papel Carta, Windows-1252 y BOM, mínimo por palabra) + 1 de integración que **lee el DOCX con el lector de DOCX → TXT del proyecto** (otro código que el generador) y comprueba título, lista, tabla con tabuladores, código, acentos y la imagen neutralizada + los 4 `wordcraft_*` reales (Markdown → PDF por el camino nuevo). Windows: `fmt`, `clippy -D warnings` y **166 tests** (+9 `#[ignore]`).
5. **Instalador**: reconstruido con el cambio. LibreOffice y Word siguen imprimiendo el HTML (no se tocaron: sus pruebas reales no cambian).
6. **Sin probar**: el módulo nuevo **no se ha compilado ni probado en Linux** (es Rust puro y portable, pero la CI lo dirá); abrir el DOCX generado en **Microsoft Word** y en LibreOffice (solo se leyó con WordCraft y con el lector del proyecto; el XML es válido y sigue el orden del esquema); notas al pie, fórmulas e imágenes no están soportadas; HTML en bruto complejo se reduce a texto y se pierde su formato.

### 2026-10-18 · Sesión 1 (cont. 9) — WordCraft como motor principal; instalador de 16 MiB

**Petición del usuario**: «usar WordCraft para dejar de depender de LibreOffice y no hacer tan pesado el software… convertir algunos archivos por su velocidad». Acepta la propuesta D-41.

**Resultado**: WordCraft es el motor de documentos preferido en Windows y Linux; el instalador de Windows pasa de **146 MiB a 16 MiB**; LibreOffice queda solo para **PDF → Word** (WordCraft no lo hace), y se descarga bajo demanda. Decisiones D-41 (aceptada), D-44, D-45 y D-46.
1. **Motor por par de formatos** (`office.rs`): `choose_engine` (pura, con tests) elige WordCraft si sabe hacerlo (`DOCX/DOC/Markdown → PDF`, `DOC → DOCX`) y está; si no, LibreOffice y después Word. `DocumentEngine::WordCraft` nuevo, `find_wordcraft` (`CONVERTER_WORDCRAFT` → carpetas de cada paquete → `vendor/bin/wordcraft` en depuración; **nunca el `PATH`**), `run_wordcraft` con `WORDCRAFT_NO_PREFS=1`. El mensaje de «falta motor» ahora depende del par (para PDF → Word habla de LibreOffice).
2. **Fijado y reproducible**: `scripts/build-wordcraft.ps1` y `.sh` clonan la etiqueta **v0.8.0**, comprueban que el commit es `f2e15cf…` y compilan con el `Cargo.lock` del propio repositorio (`--locked`, 5 min); copian sus licencias (`LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`, `ATTRIBUTION.md`) que viajan en los `resources` de los tres sistemas.
3. **Medido con la v0.8.0 compilada**: DOC → DOCX conserva todo el texto; **Markdown → PDF** con nuestro HTML sale con todo el contenido y la estructura (título, lista, tabla con bordes y cabecera en negrita, código, acentos) pero **menos pulido que LibreOffice** (sin el azul del enlace ni el sombreado del código): se acepta a cambio de velocidad y peso, y quien lo prefiera puede elegir LibreOffice en la tuerca.
4. **Instalador** (D-45): el normal ya **no lleva LibreOffice ni la página de elección** (si la llevara pesaría igual 146 MiB): 16 MiB, 58 MB en disco. `build-installers.ps1 -ConLibreOffice` genera la variante grande (155 MiB) con la página, ahora «Estándar» / «Con LibreOffice». Los instaladores anteriores de `dist/` quedan sustituidos.
5. **App**: ajustes con cuarta opción «WordCraft» (y estado «incluido en la aplicación»), textos de LibreOffice («solo hace falta para PDF → Word»), `CONVERTER_DOCUMENT_ENGINE=wordcraft`, tercer argumento `wordcraft` en el ejemplo `convert`.
6. **Pruebas**: 5 tests unitarios nuevos de elección (Auto, caídas de motor, motor forzado, rutas por paquete, pares) + 1 de ajustes + **4 reales con WordCraft** (`wordcraft_*`, 2,7 s en total frente a 12–45 s de las de LibreOffice). Windows: `fmt`, `clippy -D warnings` y **148 tests** (+9 `#[ignore]` a propósito); Linux (Ubuntu 22.04 desechable en WSL2): `fmt`, `clippy` y **148 tests**, y las 4 pruebas reales de WordCraft **sin fuentes extra instaladas**.
7. **Verificación de extremo a extremo (Windows)**: el instalador normal se instaló en `%TEMP%` y, con la aplicación instalada y la interfaz real por depuración remota (ajustes y datos de la ventana en carpetas temporales): DOCX → PDF 1,4 s, Markdown → PDF 0,43 s, DOC → PDF 0,41 s, DOC → DOCX 0,33 s; **PDF → DOCX cayó a Microsoft Word** (no había LibreOffice) en 10 s. Se limpió todo (clave del registro, accesos directos y carpeta, tras comprobar que apuntaban a mi carpeta temporal).
8. **Linux**: `build-wordcraft.sh` compila allí (40,7 MB); `.deb` y `.rpm` (26 MiB) y AppImage (98 MiB) llevan WordCraft en `usr/lib/Converter/wordcraft/`; en esa disposición y sin variables de entorno DOCX/Markdown/DOC → PDF tardan ~0,35 s y PDF → DOCX sin LibreOffice da el mensaje de Linux. Los paquetes ya no recomiendan LibreOffice sino fuentes compatibles (`fonts-crosextra-carlito`, `fonts-liberation`). La app instalada arranca y sigue viva, con un aviso `Gtk-CRITICAL … gtk_widget_get_scale_factor` que no se ha investigado.
9. **Incidencias de mis pruebas** (no del proyecto): un script mío falló dos veces por un error de comillas de PowerShell y por buscar un ejecutable en la carpeta de destino equivocada; y WSL borra `/tmp` entre sesiones, lo que dejó sin extraer el `.deb` y parecía que WordCraft «no se encontraba» en el paquete (sí se encuentra).
10. **Variante grande** construida (155 MiB) e instalada en silencio con ambos tipos (58 y 749 MB; la pantalla de elección se ve bien). **Sin probar**: elegirla desde la ventana y reinstalar de una opción a otra; macOS (sigue sin compilarse); WordCraft con documentos reales y complejos de usuarios (solo se probaron cuatro documentos sintéticos), con documentos hostiles (sin fuzzing) y en equipos sin las fuentes del documento; que la ventana se vea en un escritorio Linux real; la CI en GitHub (ahora con la compilación de WordCraft y su caché, tampoco ejecutada).

### 2026-10-18 · Sesión 1 (cont. 8) — PdfCraft y WordCraft, Linux de verdad, CI y README

**Punto de partida**: el usuario subió el proyecto a GitHub (`AndresSuarezz/konverter`) y probó el instalador único en **otro PC con Windows: «todo correcto desde la instalación hasta la desinstalación»**. Pidió estudiar PdfCraft y WordCraft (Rust, `MIT OR Apache-2.0`) para simplificar el paso a macOS/Linux y las conversiones, redactar un README y documentarlo.

**Resultado**: análisis completo en [REFERENCIAS-CRAFT.md](REFERENCIAS-CRAFT.md) (D-38 a D-43), **Linux probado de verdad** (antes solo diseñado), CI para los tres sistemas, política `cargo-deny` y `README.md` con licencias.

1. **Análisis** (clonados en `%TEMP%`, leídos, compilados y probados; nada copiado). Cifras que cambian decisiones: **WordCraft convierte DOCX → PDF en 0,5–1,6 s** (LibreOffice ~17–20 s) con resultado **prácticamente idéntico** en dos documentos difíciles (tabla, imagen, lista numerada, encabezado, pie con `Página n de N`), con un binario de 35 MB en vez de 690 MB; **pero** su Markdown → PDF es peor que el nuestro, el PDF → DOCX de PdfCraft es peor que LibreOffice, `ocrs` no lee acentos y `hayro` (rasterizador en Rust puro) coincide con pdfium en 0,06–3,2 % de píxeles pero no extrae texto ni abre PDF con contraseña.
2. **Linux real**: se importó una distribución **Ubuntu 22.04 desechable en WSL2** (SHA-256 verificado) y allí: `scripts/fetch-pdfium.sh` (nuevo) descargó pdfium con hash; `fmt`, `clippy -D warnings` y **142 tests** pasan; `tauri build` generó **`.deb` y `.rpm` (10,7 MiB) y AppImage (84,8 MiB)**; el `.deb` se instaló con `apt` y la app **arrancó**; con LibreOffice 7.3.7 del sistema pasan las **5 conversiones de documentos**. La distribución se eliminó al terminar (`wsl --unregister`).
3. **Fallos que aparecieron al probar en Linux y se corrigieron**: dos avisos que habrían roto la CI (`OnceLock` y `explain_msi_failure`, solo usados en Windows); pdfium **no se encontraba** en la disposición real de los paquetes (`/usr/lib/Converter/pdfium`, con la `C` mayúscula) → `packaged_pdfium_dirs` busca también `../lib/Converter/pdfium` y `../Resources/pdfium` (macOS), con test; la ruta del LibreOffice incluido usaba `lib/converter` en minúscula; en Linux/macOS la paleta **ofrecía descargar LibreOffice y habría fallado** → solo se ofrece donde existe (`components::libreoffice_installable`), y el mensaje (`libreoffice_install_hint`) dice cómo instalarlo en cada sistema (antes decía que la aplicación podía descargarlo).
4. **Archivos nuevos**: `scripts/fetch-pdfium.sh`; `tauri.bundle.linux.conf.json`, `tauri.bundle.macos.conf.json`, `linux/converter.desktop` (con `Exec=… %F` y tipos MIME); `.github/workflows/ci.yml` y `release.yml`; `deny.toml`; `LICENSE-MIT`, `LICENSE-APACHE`; `README.md`; `docs/REFERENCIAS-CRAFT.md`.
5. **`cargo deny` encontró cosas reales** en nuestro árbol: cinco crates **MPL-2.0** de Tauri y uno **CDLA-Permissive-2.0** (permitidos de forma expresa), tres avisos del crate **`zip` 8.6** que trae `calamine` (solo afectan a `extract()`, que no se usa; registrados con su motivo) y cuatro «sin mantenimiento» de Tauri en Linux. Ahora `cargo deny check` pasa en sus cuatro categorías.
6. **Verificación**: Windows `fmt`, `clippy -D warnings` y **144 tests** (+6 `#[ignore]` a propósito); Linux **142 tests**. Tres tests nuevos: rutas de pdfium por paquete, mensaje de instalación por sistema y descarga solo donde hay paquete fijado.
7. **Sin probar**: **macOS** (nunca compilado), **la CI en GitHub** (no se ha ejecutado ni una vez), que la ventana se dibuje y la bandeja funcione en un escritorio Linux real (en el contenedor no hay D-Bus de sesión), el atajo global en X11/Wayland, instalar el `.rpm`, «Abrir con» en un gestor de archivos real, y el instalador de Windows con los cambios de este día (hay que **reconstruirlo**: cambió `palette.js` y `main.rs`). El desinstalador real y el instalador único los probó el usuario en otro PC; la rama «borrar datos» de su ventana no se anotó.
8. **Pendiente de tu decisión**: usar WordCraft como motor opcional (D-41); y el nombre para el aviso de copyright de `LICENSE-MIT` (hoy «Converter contributors»).

### 2026-10-18 · Sesión 1 (cont. 7) — Un solo instalador con página de selección

**Petición del usuario**: un único instalador en el que el usuario elija entre la versión liviana y la completa, con una ventana que explique qué trae cada una (para no tener dos instaladores).

**Resultado**: `dist/Converter_0.1.0_x64-setup.exe` (145,9 MiB) con una página «Tipo de instalación» (D-37) antes de elegir la carpeta: **Completa (recomendada)**, con LibreOffice dentro (~715 MB en disco), o **Ligera** (~24 MB en disco; LibreOffice se descarga después). Cada opción lleva su explicación.
1. **Cómo**: Tauri no permite añadir páginas por configuración, así que `crates/palette-app/nsis/installer.nsi` es una **copia de la plantilla de `tauri-bundler 2.10.3`** (la del CLI `2.12.3`, ahora fijado en `scripts/build-installers.ps1`) con tres añadidos marcados «Converter»: la página, la copia condicional de `libreoffice\` y su borrado al desinstalar. Se activa solo si existe la variable `CVT_LO_DIR` (la fija el script); sin ella el instalador es ligero y no muestra la página. `tauri.bundle.full.conf.json` desaparece.
2. **Modos sin ventana**: `/S` instala la completa; `/S /TYPE=ligera` (o `light`/`completa`/`full`) elige. Reinstalar o actualizar conserva lo que había (si no tenía LibreOffice incluido, sigue ligera); en modo pasivo/actualización la página no se muestra.
3. **Compromiso asumido**: el instalador único siempre pesa ~146 MiB, también para quien elige la ligera (ahorra disco, no descarga). Es el coste de no tener dos archivos; alternativa si importa: el mismo instalador pero la completa descarga LibreOffice en ese momento (instalador de ~7 MiB, requiere internet).
4. **Verificación** (siempre en carpetas de `%TEMP%`, sin tocar `%LOCALAPPDATA%\Converter`): instalación silenciosa de ambos tipos (ligera: 24 MB, sin `libreoffice\`; completa: 715 MB, con `soffice.exe`; ambas con `converter.exe`, `pdfium.dll` y `uninstall.exe`, código 0); instalación con ventana: la página aparece, se capturó, el clic en «Ligera» se respeta y llega a la página final. Todo lo que dejó (clave `HKCU`, accesos directos, carpeta) se borró tras comprobar que apuntaba a mi carpeta temporal.
5. **Sin probar**: la elección «Completa» desde la ventana (solo por `/TYPE`), pasar de completa a ligera reinstalando, el desinstalador real y que la aplicación instalada por el instalador único convierta con el LibreOffice incluido (se probó en la cont. 5 con el instalador anterior). Sin ejecutar `converter.exe` instalado.

### 2026-10-18 · Sesión 1 (cont. 6) — Desinstalar: gancho, LibreOffice y la aplicación

**Petición del usuario**: que el desinstalador no deje LibreOffice huérfano («hazlo») y, dentro de las opciones, poder quitar LibreOffice y desinstalar el programa.

**Resultado**: (1) gancho del desinstalador NSIS que limpia lo descargado; (2) sección «Mantenimiento» en la tuerca con «Quitar LibreOffice descargado» y «Desinstalar Converter», ambos con confirmación en dos pasos. Verificación: **141 tests** que corren siempre (3 nuevos de `components` para quitar LibreOffice), `clippy -D warnings` y `fmt` limpios; los 5 casos del gancho probados contra carpetas desechables; el comando `uninstall_app` probado en una copia instalada (abre el desinstalador y la app se cierra; se cerró el desinstalador sin pulsar nada).

1. **Gancho** (D-34): quita `components` al desinstalar, salvo en actualizaciones; con la casilla «borrar datos», también los ajustes.
2. **Quitar LibreOffice** (D-35): `components::uninstall_libreoffice`, solo lo descargado por la app.
3. **Desinstalar desde la app**: lanza `uninstall.exe` desacoplado y se cierra.
4. **Pendiente de probar**: el desinstalador **real** de punta a punta y la rama de la casilla en su interfaz (no se ejecutó para no borrar el LibreOffice real del usuario).
5. **Errores míos durante la prueba (D-36)**: (a) asumí que `%LOCALAPPDATA%\Converter` no existía (lo había comprobado minutos antes) y el usuario ya había instalado LibreOffice ahí; (b) escribí datos falsos en esa carpeta y **sobrescribí su `settings.json`**: sus ajustes originales se perdieron y no se pudieron recuperar; (c) mi instalación silenciosa de prueba registró una clave de desinstalación y dos accesos directos que habría borrado a mano. Lo corregido: se eliminó solo la carpeta falsa, se comprobó que el LibreOffice real (691 MB) seguía intacto y se quitó la instalación de prueba **a mano, verificando que cada elemento apuntaba a mi carpeta temporal**, sin pasar por su desinstalador. Se añadió una regla a `CLAUDE.md` y una memoria.
6. **Hallazgo**: `managed_mb` real = 759 MB (la extracción administrativa deja también una copia del `.msi` dentro de la carpeta).

### 2026-10-18 · Sesión 1 (cont. 5) — La tuerca de configuración

**Petición del usuario**: usar el pétalo en blanco de la paleta para una tuerca donde configurar «este tipo de cositas» (y, a futuro, el OCR), y confirmó que se ofrezca el instalador completo como descarga principal y el ligero como alternativa.

**Resultado**: el pétalo inferior derecho es ahora la tuerca de ajustes (D-32, revisa D-10). Verificación: **138 tests** que corren siempre (9 de `components`, 50+28 del core, 5 de Markdown, 5 de Office, 17 de PDF, 16 de la paleta —6 nuevos de ajustes—, 1 doctest), más los `#[ignore]` ya existentes; `clippy -D warnings` y `fmt` limpios. Comprobado en la app real (ver punto 5).

1. **Pétalo de ajustes**: `reserved_slot`/`RESERVED_SLOT` pasan a `settings_slot`/`SETTINGS_SLOT`; icono de engranaje dibujado en SVG, tecla `0`, entrada «Configuración…» en la bandeja.
2. **Panel** (D-33): programa de documentos (Automático/LibreOffice/Word) con el estado de cada uno y botón «Instalar»; calidad JPG y WEBP; papel de los PDF; resolución de PDF → imagen; atajo (solo lectura) y un hueco para el OCR («Próximamente»).
3. **Ajustes persistentes** (`settings.rs`): `settings.json` con escritura atómica y tolerante a archivos rotos; 6 tests.
4. **Papel carta** (cierra un pendiente): `ConvertOptions::pdf_paper` (`A4`/`Letter`), con test real de pdfium.
5. **Verificación en la app** (copia de prueba): la tuerca abre el panel; «Carta» y calidad 70 se guardan en `settings.json`; el PDF siguiente sale con `MediaBox [0 0 792 612]` (carta apaisada); tras reiniciar, los controles recuperan sus valores; con el panel abierto y el foco en otra ventana **la paleta sigue visible**.
6. **Detalle de diseño**: se evitó `<select>` porque su menú nativo roba el foco y la paleta se ocultaría; se usan botones segmentados y la paleta se «fija» mientras el panel está abierto.
7. **Instaladores**: se mantiene la recomendación (completo como descarga principal, ligero como alternativa), ya recogida en [DISTRIBUCION.md](DISTRIBUCION.md).
8. **Efecto colateral de mi prueba**: para forzar la pérdida de foco abrí el Bloc de notas y, al limpiar, cerré **todos** los `notepad.exe`, no solo el mío. Si tenías uno abierto, Windows 11 suele restaurar su contenido al reabrirlo, pero conviene saberlo.

### 2026-10-18 · Sesión 1 (cont. 4) — Distribución: poda e instalador completo

**Pregunta del usuario**: ¿cómo evitar que en macOS/Linux haya que instalar LibreOffice a mano y que ocupe 1,5 GB, para que la instalación final sea sencilla y "todo funcione junto con la aplicación"? Respuesta: medir antes de decidir y construir lo que se puede probar aquí (Windows); el resto queda diseñado y marcado como sin probar en [DISTRIBUCION.md](DISTRIBUCION.md).

**Resultado**: LibreOffice podado (**1524 → 690 MB**, 139 MB comprimido); instalador **completo** de Windows (145,96 MiB) que lleva pdfium y LibreOffice dentro; el **ligero** sigue existiendo. Verificación: 5 conversiones con contenido real contra la copia **podada**; la app de **release** convirtió un DOCX con la carpeta de componentes vacía y sin ofrecer descarga (22 s), o sea, con el LibreOffice incluido; 9 tests de `components` (3 nuevos: poda, poda segura, ruta absoluta).

1. **Medición** (D-29): 462 MB son diccionarios, 262 MB idiomas de interfaz, 72 MB temas de iconos, 41 MB galería/plantillas/ayuda. `mergedlo.dll` (145 MB) y Writer son el núcleo.
2. **Poda** (D-29): `components::prune_libreoffice`, aplicada en la instalación bajo demanda y en el paquete. Conserva `readmes/` (licencia).
3. **Dos instaladores** (D-30), sustituidos después por uno solo con página de selección (D-37), con `scripts/build-installers.ps1` y `prepare_bundle`.
4. **Linux** (D-31): `.deb`/`.rpm` declaran `libreoffice-writer` como dependencia. **Sin probar** (no hay Linux aquí).
5. **macOS**: diseño (LibreOffice.app dentro del `.app` + firma/notarización). **Sin probar.** El motor ya busca ahí.
6. **Otro error real corregido**: `msiexec` falla con el 1619 si la ruta es relativa (lo descubrió `prepare_bundle vendor/bin/bundle`); ahora se normaliza.
7. **Aviso de Tauri atendido**: el identificador `dev.converter.app` terminaba en `.app`; pasa a `dev.converter.desktop` (cambia la clave de instancia única).

### 2026-10-18 · Sesión 1 (cont. 3) — Markdown, LibreOffice bajo demanda

**Resultado**: PDF ↔ Markdown funcionan; LibreOffice se descarga, verifica e instala desde la propia paleta y es el motor preferido. Verificación: **122 tests** que corren siempre (49 unitarios y 28 de conversión del core, 5 de Markdown, 5 de Office, 16 de PDF, 13 de `components`, 10 de la paleta, 1 doctest) más **6 `#[ignore]`** ejecutados a propósito y en verde: 4 con Word, 1 con LibreOffice (ahora también Markdown→PDF en ambos motores) y la **instalación real** de LibreOffice (descarga + hash + extracción + conversión con esa copia, 148 s). `clippy -D warnings` y `fmt` limpios. Flujo completo comprobado en la app (ver punto 8).

1. **Markdown** (D-25): `Format::Markdown`. Markdown → PDF pasa por HTML (`pulldown-cmark`, seguro) y el motor lo imprime; PDF → Markdown es una heurística sobre los caracteres de pdfium. Se revisaron a ojo los PDF de Word y de LibreOffice.
2. **Gestor de componentes** (D-26, D-27): crate `components` con descarga reanudable, SHA-256 fijado, extracción atómica y bloqueo. 7 tests contra un servidor HTTP local que simula cortes, `Range` ignorado, archivo corrupto y errores 503.
3. **LibreOffice preferido** (D-28): `Auto` = LibreOffice si está, si no Word. Es lo que pidió el usuario: le parece "más seguro" que Word para PDF → Word.
4. **Paleta**: oferta "Descargar / Ahora no", barra de progreso, reintento automático de la conversión pendiente, entrada "Instalar LibreOffice…" en la bandeja y contador de ocupado (la paleta ya no se oculta al perder el foco mientras convierte o descarga; resuelve un pendiente anterior).
5. **Error reportado por el usuario: `msiexec` 1603** al instalar LibreOffice. **Causa**: ruta demasiado larga. Mi prueba de la app usaba una carpeta de componentes de 169 caracteres + 100 del paquete = 269 > 260 de `MAX_PATH`; la ruta real del usuario (76) no habría fallado y su carpeta `%LOCALAPPDATA%\Converter\components` ni existía (el error que vio salió de mi copia de prueba, no de su instalación). Aun así era un fallo real para nombres de usuario largos o `AppData` redirigido, y el 1603 no explica nada: ahora se comprueba antes de descargar, se guarda `install.log`, se traduce el código y se reutiliza el MSI ya descargado. Test de regresión incluido.
6. **Hallazgos de las pruebas**: pdfium entrega el guion de fin de línea como U+0002 (el test sintético pasaba y el PDF real no); LibreOffice ignora los bordes de tabla del CSS y la fuente de los títulos; `rtk`/el entorno alteran las barras invertidas en los scripts de edición (se usan los editores de archivos); la app de prueba y la del usuario chocan por la instancia única (se compila con otro identificador).
7. **Incidencia de proceso**: un `mkdir` relativo creó una carpeta anidada vacía por un cambio de directorio de trabajo; eliminada.
8. **Verificación en la app** (copia de prueba, `CONVERTER_COMPONENTS_DIR` corta): DOCX → PDF forzando LibreOffice → oferta de descarga → 27 % / 71 % → verificación → "Instalando" (91 s) → la conversión continúa sola → "Listo · Contrato LO.pdf" (32 s la primera vez). Con la ruta larga original: aviso inmediato (0,25 s) con "Reintentar / Cerrar".

### 2026-10-18 · Sesión 1 (cont. 2) — Documentos de Office y empaquetado

**Resultado**: DOCX/DOC → PDF, PDF → DOCX, DOC → DOCX (con Word o LibreOffice) y DOCX → TXT funcionan en el core y en la paleta; pdfium va empaquetado en el instalador. Verificación: **98 tests** que corren siempre (38 unitarios + 28 de conversión + 5 de Office + 16 de PDF + 10 de la paleta + 1 doctest) más **5 `#[ignore]`** ejecutados a propósito y en verde: 4 con Word (45 s) y 1 con LibreOffice 26.8.1 (86 s). `clippy -D warnings` y `fmt` limpios. El usuario había probado antes las conversiones de imagen y PDF.

1. **pdfium empaquetado** (D-19): `tauri.bundle.conf.json` añade la DLL como recurso; se construyó el instalador NSIS (6,10 MiB, instalación por usuario). Verificado: el `converter.exe` de **release** (sin la ruta de respaldo de depuración) convirtió un PDF de 3 páginas con la DLL junto al ejecutable (`exe_dir/pdfium`), y el script NSIS instala justo en `$INSTDIR\pdfium\pdfium.dll`. **No se ejecutó el instalador**: no se modificó el sistema del usuario.
2. **Motores de documentos** (D-20): `office.rs` con Word (PowerShell + COM, `word_convert.ps1`) y LibreOffice (línea de comandos), elegidos por `DocumentEngine`. Control de tiempo máximo con limpieza de procesos.
3. **DOCX → TXT en Rust puro** (D-21): `docx_text.rs`, sin motor externo.
4. **Formatos nuevos**: `Docx` y `Doc` (solo origen, D-22); detección de ZIP (xlsx/docx) y OLE (doc) por cabecera + extensión. `Error::External` para fallos de un programa externo.
5. **Fixtures**: `sample.docx` hecho a mano con `zipfile` (tabla, tabulador, acentos); `sample.doc` generado por el propio Word.
6. **Mensaje de resultado de la paleta** (D-24): ahora dice el nombre del archivo, "3 archivos en «informe»" o "5 archivos convertidos" (antes "1 archivos" para una carpeta de 3 páginas).
7. **Fallos encontrados y corregidos**:
   - `$word.Quit($false)` falla (hay que pasar `[ref]`) y dejaba Word vivo; con `Visible = $false` y `ErrorAction = Stop` el script se colgaba con la CPU al máximo. Mató procesos de prueba varias veces hasta aislarlo.
   - **Guardar como `.doc` con Word oculto se cuelga**; con Word visible funciona. El motor no guarda `.doc`, así que no le afecta, pero se documentó.
   - Las tablas de DOCX→TXT dejaban un espacio antes de cada tabulador (lo cazó un test).
   - La descarga de LibreOffice (358 MB) se cortó a los 232 MB; se reanudó con `curl -C -` y se verificó el SHA-256 publicado.
8. **Verificación manual en la app**: DOCX → PDF en 6 s y PDF → DOCX en 18 s desde la paleta, sin procesos Word residuales.

### 2026-10-18 · Sesión 1 (cont.) — Documentos: PDF y trazabilidad

**Resultado**: imágenes → PDF, PDF → imágenes (PNG/JPG/WEBP/TIFF) y PDF → TXT funcionan en el core y en la paleta. Verificación: **83 tests** (30 unitarios + 28 de conversión + 16 de PDF del core, 8 de la paleta, 1 doctest), `clippy -D warnings` y `fmt` limpios.

1. **Trazabilidad**: se crea esta bitácora (historial, registro de decisiones, matriz requisito→código→pruebas, pendientes) y se enlaza desde `CLAUDE.md`.
2. **Soporte de PDF** (D-12 a D-14, D-17, D-18):
   - Nuevos `Format::Pdf` y `Format::Txt` (familia `Document`), con detección por cabecera `%PDF-`.
   - `pdf_write.rs`: PDF desde imágenes con `pdf-writer` (Rust puro). JPG incrustado sin recomprimir.
   - `pdf.rs`: PDF → imágenes/TXT con `pdfium-render` + `pdfium.dll` de bblanchon (`chromium/8100`, 7 MB), descargada por `scripts/fetch-pdfium.ps1` con SHA-256 verificado.
   - Salidas múltiples: `Converter::convert_pages` + `Page`, y `OutputSet` (carpeta nueva con rollback). `Engine::convert_file` ahora devuelve `Vec<PathBuf>` (cambio de API que afectó a tests y a `palette-app`).
3. **Errores nuevos**: `MissingDependency` (falta pdfium; la interfaz podrá ofrecer instalarlo) y `NeedsOcr` (PDF escaneado: enganche futuro del OCR).
4. **Fallos encontrados por las pruebas y corregidos** (D-15, D-16):
   - **Fallo previo, no relacionado con PDF**: las fotos de móvil con orientación EXIF salían **rotadas** en cualquier conversión de imagen (JPG→PNG, →WEBP...). Se aplica la orientación al decodificar.
   - Un JPG **truncado se convertía sin error** (el decodificador de `image` devuelve una imagen parcial). Ahora es un `Decode` claro.
5. **Incidencias menores**: el script de descarga fallaba porque `tar` resolvía al GNU tar de Git Bash (que toma `C:` por un host remoto) → se usa ``System32/tar.exe``; mis tests usaban `.txt` como "extensión desconocida" y TXT ya es un formato; una expectativa de 300 dpi errónea en un test (595 pt × 300/72 = 2479, no 2480).
6. **Verificación manual en la app** (con depuración remota + capturas): un PDF de 3 páginas (una apaisada) abierto en la paleta → clic en PNG → carpeta `informe trimestral/` con 3 PNG a 1240×1754 / 1754×1240; la imagen de la página 2 se revisó a ojo (texto con acentos y rectángulo en su sitio). Una foto de 117 708 bytes → PDF de 118 568 bytes (JPG incrustado, +860 bytes de estructura).

### 2026-10-18 · Sesión 1 — Arranque, core, paleta

**Resultado**: el usuario probó la conversión de imágenes desde la paleta y funcionó.

1. **Inicialización** (`/init`): workspace de Rust, `CLAUDE.md`, `docs/REQUERIMIENTOS.md`; skills instaladas con `npx skills add` (`rust-testing`, `rust-best-practices`, `agent-browser`, `grill-me`) y cuatro agentes en `.claude/agents/` (`conversion-engine`, `windows-shell`, `palette-ui`, `format-qa`). La skill `grill-me` solo redirige a una skill `grilling` inexistente: la entrevista de decisiones se hizo a mano.
2. **Decisiones cerradas** con el usuario (ver D-01 a D-07).
3. **Core v0.1** (`converter-core`): `Format`, `Converter`, `Engine`, ranking por coherencia, escritura atómica sin sobrescribir; conversores de imagen (JPG, PNG, WEBP, BMP, TIFF, GIF, todos contra todos) y de datos (CSV, XLSX, JSON). Fixtures reales generados con Pillow/`zipfile`. Verificación: 43 tests.
4. **libwebp** (D-08): WEBP con pérdida desde JPG, sin pérdida desde los demás. 5 tests nuevos.
5. **`os-shell`**: lectura de la selección del Explorador y del Escritorio por COM. Verificado a mano contra un Explorador real (2 archivos con ñ y espacios; 0 sin selección). Rama del Escritorio: consulta COM probada solo sin selección.
6. **`palette-app`** (Tauri 2): paleta radial, atajo `Ctrl+Alt+Espacio`, bandeja, instancia única, apertura con rutas por argumento. Verificada de extremo a extremo con depuración remota de WebView2 + capturas: atajo → selección del Explorador → paleta → tecla `1` → 2 archivos convertidos a WEBP, originales intactos.
7. **Incidencias resueltas**: carrera de arranque (el evento `palette://open` llegaba antes que el escuchador; solución: `current_palette`), manejador de la reserva abierto al renombrar en Windows, avisos de clippy, feature `macos-private-api` de Tauri, y clics físicos de prueba que fallaron dos veces sin causa identificada (la tercera funcionó).

---

## Registro de decisiones

| ID | Fecha | Decisión | Estado | Motivo |
|---|---|---|---|---|
| D-01 | 2026-10-18 | UI con **Tauri 2**; el core sigue en Rust puro y la UI es un cliente fino | Aceptada | Una paleta radial transparente y animada sale mucho más pulida en HTML/CSS/SVG que en egui/iced |
| D-02 | 2026-10-18 | Plataformas: Windows, macOS y Linux (sin móvil); se empieza por Windows | Aceptada | Requisito del usuario; el core no conoce ningún SO |
| D-03 | 2026-10-18 | Licencia `MIT OR Apache-2.0`, sin GPL | Aceptada | Open source; excluye x265 (GPL) |
| D-04 | 2026-10-18 | DOCX↔PDF: Word si existe; si no, LibreOffice descargado bajo demanda | Aceptada | Fidelidad máxima sin inflar el instalador; en Linux siempre LibreOffice |
| D-05 | 2026-10-18 | OCR: Tesseract descartado | **Revertida** por D-06 | — |
| D-06 | 2026-10-18 | OCR: **Tesseract local** como proceso externo, descargado bajo demanda, tras el trait `OcrProvider` | Aceptada | El proyecto debe correr en tres SO y `Windows.Media.Ocr` solo existe en Windows |
| D-07 | 2026-10-18 | HEIC: leer con libheif (LGPL, enlace dinámico); escribir con códecs del SO (WIC/ImageIO); sin escritura en Linux | Aceptada (sin implementar) | x265 es GPL |
| D-08 | 2026-10-18 | WEBP con libwebp (crate `webp`, enlace estático); modo automático: con pérdida desde JPG, sin pérdida desde el resto | Aceptada | Un WEBP sin pérdida de un JPG pesa más que el JPG; JPG→WEBP es la conversión más probable |
| D-09 | 2026-10-18 | Atajo global en Windows: `Ctrl+Alt+Espacio` | Aceptada | Confirmado por el usuario; falta hacerlo configurable |
| D-10 | 2026-10-18 | El pétalo inferior derecho queda siempre vacío y reservado | **Revertida** por D-32 | Hueco futuro para un servicio externo de pago (boceto del usuario) |
| D-11 | 2026-10-18 | La paleta no ordena ni filtra: pinta lo que `Engine::targets` ofrece | Aceptada | Una sola fuente de verdad de qué se puede convertir |
| D-12 | 2026-10-18 | PDF → imagen/TXT con **pdfium** cargado en tiempo de ejecución (`pdfium-render`, feature `thread_safe`, una instancia global); binario de bblanchon fijado por versión y SHA-256 | Aceptada | No existe rasterizador de PDF fiable en Rust puro; pdfium (BSD-3) es el del navegador Chrome. Pesa ~7 MB: se propone **empaquetarlo** con el instalador (pendiente de confirmar) |
| D-13 | 2026-10-18 | Imagen → PDF: A4 con 10 mm de margen, orientación según la imagen, imagen contenida y centrada; JPG incrustado byte a byte | Aceptada | Es lo que espera quien imprime una foto o un formulario; el JPG sin recomprimir no pierde calidad ni crece |
| D-14 | 2026-10-18 | Varias salidas (páginas de un PDF): una carpeta nueva `<nombre>/<nombre>-N.ext`; con una sola, junto al original. Entrega por `sink`, sin acumular en memoria | Aceptada | 30 imágenes sueltas ensuciarían la carpeta del usuario; un PDF de 500 páginas no debe agotar la memoria |
| D-15 | 2026-10-18 | Aplicar la orientación EXIF al decodificar cualquier imagen | Aceptada | Los formatos de salida no conservan la etiqueta: sin esto, las fotos de móvil salían rotadas |
| D-16 | 2026-10-18 | Un JPG truncado es un error, no una imagen parcial | Aceptada | Un conversor no debe ocultar archivos dañados; se verifica recorriendo segmentos hasta el EOI |
| D-17 | 2026-10-18 | PDF → TXT: UTF-8 con BOM; un PDF sin texto devuelve `NeedsOcr` | Aceptada | El Bloc de notas usa el BOM para detectar la codificación; `NeedsOcr` es el enganche del OCR futuro |
| D-18 | 2026-10-18 | PDF → imagen: PNG, JPG, WEBP y TIFF (sin BMP/GIF); 150 dpi por defecto; máx. 10 000 px por lado | Aceptada | BMP/GIF de una página de documento no tienen sentido; el tope evita agotar memoria con planos o pósters |
| D-19 | 2026-10-18 | pdfium **empaquetado** con el instalador mediante `tauri.bundle.conf.json` (aparte de `tauri.conf.json`) | Aceptada (supone D-12) | El usuario pidió que "funcione todo correctamente"; pesa ~7 MB. La config de empaquetado va aparte porque `tauri-build` exige que existan los recursos y rompería `cargo build` sin descargar pdfium |
| D-20 | 2026-10-18 | DOCX/DOC ↔ PDF: Word por **PowerShell + COM** (Windows) o LibreOffice por línea de comandos; `Auto` prefiere Word | Aceptada (concreta D-04) | Sin dependencias nuevas en Rust y con la fidelidad de Word; PowerShell existe en todo Windows. Se descartó la automatización COM directa desde Rust (más código sin ganar nada) |
| D-21 | 2026-10-18 | DOCX → TXT en Rust puro (`zip` + `roxmltree`) | Aceptada | Útil sin Word ni LibreOffice y sin riesgo de cuelgues; DTD y tamaños limitados |
| D-22 | 2026-10-18 | `Format::Doc` es solo origen (→ PDF, → DOCX) | Aceptada | Guardar `.doc` con Word oculto se cuelga; además nadie quiere crear `.doc` nuevos |
| D-23 | 2026-10-18 | Los tests que usan Word o LibreOffice van `#[ignore]` y se ejecutan a propósito; un test garantiza que cada par de motor externo tiene cobertura | Aceptada | Automatizar Word en `cargo test` puede tardar o colgarse; se mantiene la regla "un par publicado, un test real" |
| D-24 | 2026-10-18 | La paleta muestra un resumen del resultado calculado en el backend | Aceptada | Antes decía "1 archivos" para una carpeta de 3 páginas |
| D-25 | 2026-10-18 | Markdown ↔ PDF: MD → PDF vía HTML seguro impreso por el motor; PDF → MD por heurística de pdfium | Aceptada | Lo pidió el usuario. Pure-Rust para MD → PDF exigiría un motor de maquetación y fuentes propios |
| D-26 | 2026-10-18 | Los componentes pesados se descargan bajo demanda en un crate aparte (`components`), con SHA-256 fijado, descarga reanudable y extracción atómica | Aceptada (concreta D-04) | El core sigue sin red; una instalación a medias nunca se usa |
| D-27 | 2026-10-18 | Extraer el MSI con `msiexec /a` en `%LOCALAPPDATA%\Converter\components`, sin instalar nada en el sistema | Aceptada | Sin permisos de administrador ni registro. Coste: ~1,5 GB en disco; se podría podar |
| D-28 | 2026-10-18 | `Auto` prefiere LibreOffice sobre Word; Word es respaldo si LibreOffice no está | **Revertida** por D-44 (ahora WordCraft va primero; el orden LibreOffice → Word se mantiene como respaldo) | El usuario considera más seguro LibreOffice y quiere distribuirlo con la app |
| D-29 | 2026-10-18 | Podar LibreOffice (diccionarios, idiomas salvo es/en, iconos, galería, plantillas, ayuda): 1524 → 690 MB | Aceptada (afina D-27) | Medido y probado con las 5 conversiones reales; se conservan `program/`, `share/registry` y `readmes/` (licencias) |
| D-30 | 2026-10-18 | Dos instaladores de Windows: **completo** (recomendado, LibreOffice dentro, ~146 MiB) y **ligero** (descarga bajo demanda, ~7 MiB) | Aceptada; **sustituida por D-37 y luego por D-45** (hoy: un instalador normal de 16 MiB y una variante con LibreOffice) | Funciona sin internet desde el primer clic y sigue habiendo opción pequeña |
| D-31 | 2026-10-18 | Linux: `.deb`/`.rpm` dependen de `libreoffice-writer` (lo instala `apt`/`dnf`); macOS: LibreOffice dentro del `.app` con firma y notarización | Aceptada como diseño; **la parte de Linux queda afinada por D-40** (pasa de dependencia a recomendación); macOS sin probar | En Linux el gestor de paquetes es la forma correcta; macOS exige notarizar los binarios anidados |
| D-32 | 2026-10-18 | El pétalo inferior derecho es la **tuerca de configuración**; ya no está reservado para un servicio de pago | Aceptada (**revierte** D-10) | Lo pidió el usuario; sirve también para el OCR futuro y cualquier servicio externo, que vivirían dentro del panel |
| D-33 | 2026-10-18 | Los ajustes viven en `settings.json` (junto a la carpeta de componentes), con valores por defecto ante cualquier fallo; el panel usa botones segmentados | Aceptada | Unos ajustes rotos no deben impedir convertir; los `<select>` nativos le quitan el foco a la paleta |
| D-34 | 2026-10-18 | El desinstalador de Windows borra `%LOCALAPPDATA%\Converter\components` salvo en actualizaciones; con la casilla «borrar datos» borra también los ajustes | Aceptada (probada con carpetas desechables; **el usuario probó instalar y desinstalar de verdad en otro PC y funcionó**; la casilla «borrar datos» no se anotó) | LibreOffice descargado (~700 MB) quedaba huérfano al desinstalar el instalador ligero |
| D-35 | 2026-10-18 | «Quitar LibreOffice» y «Desinstalar Converter» en los ajustes, con confirmación en dos pasos; solo se quita lo descargado por la app | Aceptada | Lo pidió el usuario; el LibreOffice incluido se va con el desinstalador y el del sistema no es nuestro |
| D-37 | 2026-10-18 | Un solo instalador de Windows con página «Tipo de instalación» (completa recomendada / ligera) en lugar de dos archivos; plantilla NSIS propia copiada de `tauri-bundler 2.10.3` y CLI de Tauri fijado | Aceptada (lo pidió el usuario). **Sustituye la parte de «dos instaladores» de D-30; D-45 la restringe a la variante con LibreOffice** | Un único archivo que descargar; coste: pesa ~146 MiB aunque se elija la ligera |
| D-36 | 2026-10-18 | Regla: **jamás escribir ni borrar en `%LOCALAPPDATA%\Converter` real durante pruebas**; comprobar su estado justo antes de tocar algo y usar `CONVERTER_COMPONENTS_DIR` en `%TEMP%` | Aceptada (lección de un error propio) | Se sobrescribió un `settings.json` del usuario con datos de prueba |
| D-38 | 2026-10-18 | Integración continua en los tres sistemas (`ci.yml`: Windows, Ubuntu 22.04, macOS) y flujo de lanzamiento por sistema (`release.yml`, borrador de release, sin firmar), a imagen del de PdfCraft | Aceptada, **sin ejecutar en GitHub** | Es la única forma de probar macOS sin un Mac; Ubuntu 22.04 fija la base de glibc (2.35) |
| D-39 | 2026-10-18 | Política `cargo-deny` (`deny.toml`): solo licencias permisivas, más **MPL-2.0** y **CDLA-Permissive-2.0** permitidas de forma expresa; los avisos del `zip` 8.6 de `calamine` y los «sin mantenimiento» de Tauri/Linux se registran con su motivo | Aceptada (pasa en las cuatro categorías) | Hace cumplir «sin GPL»; la MPL es copyleft débil por archivo (como LibreOffice). Revisar `zip` cuando salga un `calamine` con `zip 9` |
| D-40 | 2026-10-18 | Linux: `.deb`/`.rpm`/AppImage con Tauri; `Recommends` de fuentes compatibles (`fonts-crosextra-carlito`, `fonts-liberation`) en vez de `libreoffice-writer` desde D-44; recursos en `/usr/lib/Converter/`; acceso directo con `Exec=… %F` y tipos MIME; **no se ofrece descargar LibreOffice fuera de Windows** y el mensaje dice cómo instalarlo | Aceptada (construido y probado en Ubuntu 22.04 vía WSL2). **Afina D-31** | `Recommends` da la misma elección completa/ligera que en Windows; la descarga fallaba con «no soportado»; «Abrir con» suple la lectura de la selección que Linux aún no tiene |
| D-41 | 2026-10-18 | **WordCraft como motor de documentos** (proceso externo `wordcraft-cli`) | **Aceptada por el usuario** («dejar de depender de LibreOffice y no hacer tan pesado el software»). **Amplía lo propuesto**: no es solo opcional, es el preferido (D-44) | 0,3–1,6 s frente a ~17–20 s y 35 MB frente a 690 MB, con resultado casi idéntico en lo probado; riesgos: alfa, fuentes del sistema, sin fuzzing. Ver [REFERENCIAS-CRAFT.md](REFERENCIAS-CRAFT.md) |
| D-44 | 2026-10-18 | El motor se elige **por par de formatos**: `Auto` y `WordCraft` usan WordCraft si sabe hacerlo (DOCX/DOC/Markdown → PDF, DOC → DOCX); para PDF → DOCX, LibreOffice y después Word; LibreOffice y Word elegidos a mano se fuerzan. **Revierte D-28** | Aceptada (probada en Windows y Linux) | WordCraft no lee PDF; el exportador de PdfCraft salió peor que LibreOffice. Markdown → PDF sale algo menos pulido con WordCraft, pero se acepta por velocidad y peso |
| D-45 | 2026-10-18 | El instalador de Windows normal **no lleva LibreOffice ni la página de elección** (16 MiB); la variante con LibreOffice y página es `build-installers.ps1 -ConLibreOffice` (155 MiB). **Sustituye la parte de D-30 y D-37 que metía LibreOffice en todos los instaladores** | Aceptada (el normal, probado instalado; la variante grande, construida e instalada en silencio) | Con LibreOffice dentro pesaba 146 MiB aunque se eligiera la opción ligera; el usuario quiere el software más ligero |
| D-47 | 2026-10-18 | **Markdown → DOCX propio** (`md_docx.rs`, Rust puro) y **Markdown → PDF con WordCraft pasando por ese DOCX** (no por HTML); LibreOffice y Word siguen con el HTML | Aceptada (lo pidió el usuario: «poder pasar de Markdown a PDF»; probada con documentos reales y 16 tests) | El importador de HTML de WordCraft ignora los anchos de columna (medido); el de DOCX los respeta. De paso nace el par Markdown → DOCX |
| D-46 | 2026-10-18 | WordCraft se compila desde una **etiqueta y un commit fijados** (v0.8.0), con el `Cargo.lock` de su repositorio, y se busca solo en `CONVERTER_WORDCRAFT` y en las carpetas de los paquetes (**nunca en el `PATH`**); se ejecuta con `WORDCRAFT_NO_PREFS=1` y sus licencias viajan con el binario | Aceptada | Otra versión podría comportarse distinto de la probada; sin la variable escribiría preferencias y un registro en el perfil; MIT/Apache exigen conservar los avisos |
| D-42 | 2026-10-18 | OCR: se mantiene Tesseract (D-06); **`ocrs` descartado** | Aceptada (reafirma D-06) | Sus modelos solo leen alfabeto latino sin acentos (ñ, á, é fallarían) y son CC-BY-SA-4.0 |
| D-43 | 2026-10-18 | Se mantienen pdfium (PDF → imagen/texto), LibreOffice/Word (PDF → DOCX) y nuestro HTML + motor (Markdown → PDF). **`hayro`** queda como candidato a largo plazo | Aceptada | Medido: LibreOffice gana en PDF → DOCX y nuestro HTML gana en Markdown → PDF; `hayro` iguala a pdfium al rasterizar pero no extrae texto ni abre PDF con contraseña |

---

## Matriz de trazabilidad

Estado: ✅ hecho y probado · 🟡 hecho con límites · ⬜ pendiente.

| Requisito ([REQUERIMIENTOS.md](REQUERIMIENTOS.md)) | Estado | Dónde | Pruebas |
|---|---|---|---|
| §2 Atajo global y lectura de la selección (Windows) | ✅ | `os-shell`, `palette-app/src/main.rs` | Manual con Explorador real; sin test automático |
| §2 Paleta radial de 8 pétalos con la tuerca de ajustes | ✅ | `palette-app/ui`, `palette-app/src/palette.rs` | 6 tests del modelo de vista; verificación visual |
| §2 Resultado junto al original, sin sobrescribir | ✅ | `converter-core/src/output.rs`, `engine.rs` | `output::tests`, `convert_file_*` |
| §2 Menú contextual del Explorador / "Enviar a" | ⬜ | La base existe (instancia única + rutas por argumento) | — |
| §3 Imágenes JPG/PNG/WEBP/BMP/TIFF/GIF | ✅ | `image_conv.rs` | `tests/conversions.rs` |
| §3 Imágenes HEIC, AVIF, ICO | ⬜ | — | — |
| §3 CSV ↔ XLSX ↔ JSON | 🟡 (solo 1ª hoja de XLSX) | `tabular.rs` | `tests/conversions.rs` |
| §3 Imágenes → PDF | ✅ | `pdf_write.rs`, `pdf.rs` | `tests/pdf.rs` (A4, JPG intacto, alfa, EXIF, gris, corrupto) |
| §3 PDF → imágenes / TXT | ✅ (pdfium sin empaquetar; contraseña sin probar) | `pdf.rs` | `tests/pdf.rs` (páginas, dpi, formatos, texto, escaneo, corrupto, carpeta, concurrencia) |
| §3 DOCX/DOC/Markdown → PDF, DOC → DOCX | 🟡 **WordCraft** (alfa; casi idéntico a LibreOffice en 4 documentos de prueba; Markdown algo menos pulido); LibreOffice y Word como alternativa | `office.rs` (`choose_engine`, `find_wordcraft`), `scripts/build-wordcraft.*` | `tests/office.rs` (`wordcraft_*`, `word_*`, `libreoffice_*`; `#[ignore]`, ejecutados en verde en Windows; `wordcraft_*` también en Linux) + 6 unitarios |
| §3 PDF → DOCX | 🟡 Aproximado; solo con LibreOffice o Word (WordCraft no lo hace); descarga de LibreOffice solo en Windows x64 | `office.rs`, `word_convert.ps1` | `tests/office.rs` (`word_*`, `libreoffice_*`) |
| §3 DOCX → TXT | ✅ | `docx_text.rs` | `tests/office.rs`, `docx_text::tests` |
| §3 Markdown → PDF, PDF → Markdown | ✅ (PDF → MD es heurístico: sin tablas) | `markdown.rs`, `pdf_markdown.rs`, `office.rs` | `tests/markdown.rs`, `pdf_markdown::tests`, `tests/office.rs` (ambos motores) |
| §3 Markdown → DOCX | ✅ Rust puro (notas al pie, fórmulas e imágenes no) | `md_docx.rs` | 15 unitarios + `conversions.rs::markdown_to_docx_keeps_the_content_when_read_back_by_an_independent_reader` |
| §3 DOCX → MD, TXT → DOCX/PDF | ⬜ | — | — |
| §8 Distribución (instalador) | 🟡 Windows: instalador normal de **16 MiB** con WordCraft, probado instalado con la interfaz real; la variante con LibreOffice y página de elección **sin reconstruir**; el instalador anterior (con página) lo probó **el usuario en otro PC (instalar → usar → desinstalar)**; sin firma ni auto-actualización | `tauri.bundle.conf.json`, `nsis/installer.nsi`, `scripts/build-installers.ps1`, `docs/DISTRIBUCION.md` | Release con LibreOffice incluido probado fuera del instalador |
| §1 Linux | 🟡 Compila, 148 tests, `.deb`/`.rpm`/AppImage con WordCraft construidos y la app arranca; DOCX/Markdown/DOC → PDF en ~0,35 s desde la disposición del paquete y las 5 conversiones con LibreOffice 7.3.7; **sin** lectura de la selección ni atajo probado ni ventana vista en un escritorio real | `tauri.bundle.linux.conf.json`, `linux/converter.desktop`, `scripts/fetch-pdfium.sh`, `pdf.rs::packaged_pdfium_dirs` | Ubuntu 22.04 desechable (WSL2); `pdf::tests::looks_for_pdfium_where_each_package_leaves_it` |
| §1 macOS | ⬜ Configuración escrita, **nunca compilado** | `tauri.bundle.macos.conf.json`, `.github/workflows/` | — (la CI lo dirá) |
| §1 Integración continua en tres sistemas | ⬜ Escrita, **sin ejecutar** | `.github/workflows/ci.yml`, `release.yml`, `deny.toml` | `cargo deny check` en local |
| §3 Varias imágenes → un solo PDF | ⬜ | `pdf_write::build_pdf` ya admite varias páginas; falta la API de varias entradas | — |
| §4 Ranking por coherencia | ✅ | `ranking.rs`, `engine.rs` | `targets_are_ranked_*` |
| §4 Rutas de varios saltos, aprendizaje local, "combinar en PDF" | ⬜ | — | — |
| §5 OCR (Tesseract local) | ⬜ | Interruptor deshabilitado en la paleta | — |
| §7 WEBP con pérdida | ✅ | `image_conv.rs` | 5 tests WEBP |
| §9 Tests con archivos reales por cada par publicado | ✅ | `tests/conversions.rs::every_published_pair_converts_a_real_file` (47 pares) | — |
| §2 Ajustes del usuario (tuerca), con mantenimiento (quitar LibreOffice, desinstalar) | ✅ (atajo solo lectura; OCR pendiente; desinstalador real probado por el usuario en otro PC) | `palette-app/src/settings.rs`, `ui/palette.js` | 6 tests de ajustes; verificado en la app |
| §9 No ocultar archivos dañados | ✅ | `image_conv.rs::jpeg_is_complete`, errores `Decode` | `corrupt_*`, `truncated_*` |

---

## Pendientes

- Hacer el atajo configurable; definir atajos y lectura de selección en macOS y Linux.
- Probar la rama del Escritorio con archivos seleccionados.
- Instalador (`tauri build`), arranque con Windows, firma de código y auto-actualización.
- Tests automáticos de `os-shell` y de la ventana (hoy solo manuales; ver el método en `CLAUDE.md`).
- Metadatos (EXIF, perfiles de color) no se conservan al convertir imágenes.
- **Ejecutar la CI en GitHub** (`ci.yml` y `release.yml` están escritas pero nunca se han lanzado) y corregir lo que diga macOS: es la primera compilación del proyecto allí.
- **Probar Linux en un escritorio real** (ventana, bandeja, atajo en X11 y Wayland, «Abrir con» en un gestor de archivos, instalar el `.rpm`, AppImage en otras distribuciones) y leer la selección del gestor (hoy `os-shell` devuelve `Unsupported`).
- **macOS**: compilar, firmar y notarizar (necesita cuenta de Apple Developer); universal con `lipo` sobre `libpdfium.dylib`; LibreOffice dentro del `.app`. Ver [DISTRIBUCION.md](DISTRIBUCION.md).
- **WordCraft (D-41, aceptada)**: probarlo con documentos reales y complejos del usuario (tablas anchas, imágenes flotantes, notas al pie, fuentes que no tenga) y, si algo sale mal, anotarlo aquí; valorar fuzzing; y los formatos que da gratis (ODT, RTF, HTML, TeX, DOCX → Markdown/HTML) con su fixture y su test cada uno.
- Variante grande del instalador: elegir «Con LibreOffice» desde la ventana y reinstalar de una opción a otra (el resto ya está probado).
- Investigar el aviso `Gtk-CRITICAL … gtk_widget_get_scale_factor` al arrancar la app en Linux.
- **Markdown → DOCX**: abrir el resultado en Microsoft Word y en LibreOffice (solo se leyó con WordCraft y con el lector del proyecto), comprobar el módulo en Linux/macOS (la CI), y valorar notas al pie, imágenes locales y listas de definiciones.
- Si WordCraft publica una versión nueva: cambiar etiqueta y commit en `build-wordcraft.*` y repetir las pruebas `wordcraft_*` en Windows y Linux.
- Avisos de copyright de `LICENSE-MIT`: hoy «Converter contributors»; poner el nombre que corresponda.
- Revisar el aviso del `zip` 8.6 (RUSTSEC-2026-0367/8/9) cuando `calamine` publique una versión con `zip 9`.
- Idea de WordCraft aún sin aplicar: lints `#![deny(clippy::unwrap_used, expect_used, panic…)]` en `converter-core` para hacer cumplir «sin pánico ante entradas malformadas».
- Recortar más LibreOffice (Calc/Impress/Draw/Base/Python, ~100 MB) si el tamaño importa; menos beneficio y más riesgo.
- Archivo `THIRD_PARTY_LICENSES` y firma de código en Windows.
- Tesseract bajo demanda con el mismo gestor (OCR), y que `NeedsOcr` lo ofrezca.
- Papel por defecto: sigue siendo A4 (ahora se puede cambiar a Carta en los ajustes); valorar elegirlo según la región del sistema.
- Desinstalador real: el usuario lo probó en otro PC (funcionó); falta anotar si con la casilla «borrar datos» se borran los ajustes y sin ella se conservan.
- Ajustes: hacer el atajo configurable, y añadir el interruptor de OCR cuando exista el motor.
- PDF → Markdown: tablas (columnas por posición) y enlaces.
- LibreOffice tarda ~20 s por conversión porque crea un perfil nuevo cada vez; valorar un perfil persistente con bloqueo.
- Word tarda 6-18 s por conversión (arranca una instancia cada vez); valorar reutilizar una instancia entre archivos de un mismo lote.
- Probar a mano el instalador normal en otro PC (el usuario ya probó el anterior) y, en la variante con LibreOffice, elegir «Con LibreOffice» en la ventana y pasar de una opción a otra reinstalando. Firmar el código.
- Al subir el CLI de Tauri, comparar `crates/palette-app/nsis/installer.nsi` con la plantilla nueva (los añadidos están marcados «Converter»).
- PDF protegido con contraseña: el error está traducido pero sin fixture ni test.
- Mensaje de resultado de la paleta con varias salidas: hoy dice "N archivos"; mejor "N páginas en la carpeta X" y un botón "mostrar en carpeta".
- API para convertir **varias entradas en una sola salida** (varias imágenes → un PDF).
- Si falta pdfium, la paleta igualmente ofrece los destinos de PDF y el error aparece al convertir; valorar ocultarlos o ofrecer la instalación.
- Orden de pétalos con varios archivos mezclados: hoy manda el ranking del primer archivo.

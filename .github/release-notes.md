**Converter** convierte archivos con una paleta radial: seleccionas un archivo, pulsas un atajo y eliges el formato de destino. Imágenes, datos (CSV/XLSX/JSON), PDF y documentos de Word y Markdown, sin enviar nada a internet.

> **Versión inicial (pre-1.0).** Funciona de extremo a extremo en Windows; en Linux y macOS compila y pasa los tests, pero todavía no lee la selección del gestor de archivos ni tiene atajo global. Los instaladores **no están firmados**.

## Qué descargar

| Sistema | Archivo | Notas |
|---|---|---|
| **Windows 11** (x64) | `…_Windows.exe` | Instalador de unos 16 MB; no pide administrador. Incluye todo lo necesario salvo PDF → Word, que descarga LibreOffice la primera vez |
| **Linux** (x86_64) | `…_Linux.deb` · `…_Linux.rpm` · `…_Linux.AppImage` | `.deb` para Debian/Ubuntu, `.rpm` para Fedora/openSUSE, AppImage para cualquier distribución. Necesitan glibc 2.35 o superior |
| **macOS** | `…_aarch64_macOS.dmg` (Apple silicon: M1, M2, M3…) · `…_x64_macOS.dmg` (Intel) | Sin firmar ni notarizar: macOS lo bloqueará hasta que lo permitas |

El nombre de cada archivo termina con el sistema al que corresponde (`Windows`, `Linux`, `macOS`).

Verifica la descarga comparando su suma con `SHA256SUMS.txt` (`Get-FileHash <archivo>` en Windows; `sha256sum <archivo>` en Linux; `shasum -a 256 <archivo>` en macOS).

## Avisos de los instaladores sin firmar

- **Windows**: SmartScreen puede mostrar «Windows protegió su PC»: pulsa *Más información → Ejecutar de todas formas*.
- **macOS**: abre el `.dmg`, arrastra la aplicación a *Aplicaciones* y la primera vez ábrela con clic derecho → *Abrir*.
- **Linux**: `sudo apt install ./<archivo>.deb`, o `chmod +x` al AppImage y ejecútalo.

La documentación completa está en el [README](../blob/main/README.md).

---

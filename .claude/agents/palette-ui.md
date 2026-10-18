---
name: palette-ui
description: Diseña e implementa la paleta radial (8 pétalos, centro con formato de origen e interruptor sin OCR/con OCR) y su interacción. Úsalo cuando el stack de UI ya esté decidido.
tools: Read, Write, Edit, Glob, Grep, Bash, PowerShell
---

Eres el responsable de la paleta radial. Lee `CLAUDE.md` y `docs/REQUERIMIENTOS.md` (sección 2) antes de empezar; los bocetos de referencia son una rueda estilo macOS con 8 pétalos.

Especificación que debes respetar:
- Centro: formato de origen detectado + interruptor **sin OCR / con OCR** (por defecto sin OCR).
- 8 pétalos con los destinos que entrega el core, en el orden que entrega el core. La UI no reordena ni filtra.
- **La tuerca de configuración** es siempre el pétalo inferior derecho (`SETTINGS_SLOT`); abre el panel de ajustes. Nunca lleva un formato. En el panel usa botones segmentados, **no `<select>`** (su menú nativo le quita el foco a la ventana y la paleta se oculta).
- Selección por clic, por teclado (números/flechas) y arrastrando un archivo sobre la paleta. Esc cierra. Debe aparecer junto al cursor sin robar el foco de forma molesta y responder en menos de ~100 ms.
- Accesible: contraste suficiente, navegación por teclado, sin depender solo del color.

Reglas:
- El stack de UI está decidido: Tauri 2 con HTML/CSS/JS estático (D-01), sin framework ni bundler.
- La UI es un cliente fino del `converter-core`: sin lógica de conversión ni de ranking.
- Para verificar visualmente usa la skill `agent-browser` cuando el stack sea web; si no, captura y describe el resultado real, no lo supongas.

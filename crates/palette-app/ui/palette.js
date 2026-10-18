"use strict";

// La interfaz solo pinta lo que el backend decide: no ordena, filtra ni convierte.
const tauri = window.__TAURI__;
const invoke = tauri?.core?.invoke;
const NS = "http://www.w3.org/2000/svg";

const C = 230;        // centro del viewBox
const R_OUT = 212;
const R_IN = 104;
const GAP = 4;        // grados entre pétalos
const INSET = 5;      // el trazo redondeado de 10px añade 5px por lado

const ring = document.getElementById("ring");
const wheel = document.getElementById("wheel");
const center = document.getElementById("center");
const srcLabel = document.getElementById("src-label");
const srcName = document.getElementById("src-name");
const actions = document.getElementById("actions");
const actYes = document.getElementById("act-yes");
const actNo = document.getElementById("act-no");
const bar = document.getElementById("bar");
const barFill = document.getElementById("bar-fill");
const settingsPanel = document.getElementById("settings");
const settingsBody = document.getElementById("settings-body");
const settingsClose = document.getElementById("settings-close");

let busy = false;
let hideTimer = null;

const rad = (deg) => (deg * Math.PI) / 180;
const deg = (radians) => (radians * 180) / Math.PI;
const polar = (r, a) => [C + r * Math.cos(rad(a)), C + r * Math.sin(rad(a))];

/** Sector anular del pétalo `i` (0 = arriba, sentido horario). */
function sectorPath(i) {
  const mid = -90 + i * 45;
  const start = mid - 22.5 + GAP / 2;
  const end = mid + 22.5 - GAP / 2;
  const ro = R_OUT - INSET;
  const ri = R_IN + INSET;
  const dOut = deg(INSET / ro);
  const dIn = deg(INSET / ri);
  const [x0, y0] = polar(ro, start + dOut);
  const [x1, y1] = polar(ro, end - dOut);
  const [x2, y2] = polar(ri, end - dIn);
  const [x3, y3] = polar(ri, start + dIn);
  return `M${x0} ${y0}A${ro} ${ro} 0 0 1 ${x1} ${y1}L${x2} ${y2}A${ri} ${ri} 0 0 0 ${x3} ${y3}Z`;
}

function svg(tag, attrs = {}, text) {
  const el = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
  if (text !== undefined) el.textContent = text;
  return el;
}

/** Contorno de una rueda dentada (con el agujero central), para el icono de la tuerca. */
function gearPath(cx, cy, radius, teeth = 8) {
  const step = 360 / teeth;
  const inner = radius * 0.78;
  const point = (r, a) => [cx + r * Math.cos(rad(a)), cy + r * Math.sin(rad(a))];
  const pts = [];
  for (let k = 0; k < teeth; k++) {
    const a = k * step - 90;
    pts.push(point(inner, a - step * 0.3), point(radius, a - step * 0.17), point(radius, a + step * 0.17), point(inner, a + step * 0.3));
  }
  const hole = radius * 0.38;
  const outline = pts.map(([x, y], i) => `${i ? "L" : "M"}${x.toFixed(1)} ${y.toFixed(1)}`).join("") + "Z";
  return `${outline}M${cx + hole} ${cy}A${hole} ${hole} 0 1 0 ${cx - hole} ${cy}A${hole} ${hole} 0 1 0 ${cx + hole} ${cy}Z`;
}

function petalNode(i, petal, gear) {
  const g = svg("g", { class: gear ? "petal gear" : "petal", style: `--i:${i}` });
  g.append(svg("path", { class: "shape", d: sectorPath(i) }), svg("path", { class: "edge", d: sectorPath(i) }));
  const mid = -90 + i * 45;
  if (gear) {
    const [gx, gy] = polar((R_OUT + R_IN) / 2, mid);
    g.setAttribute("role", "button");
    g.setAttribute("tabindex", "0");
    g.setAttribute("aria-label", "Configuración. Tecla 0");
    g.append(svg("path", { class: "icon", d: gearPath(gx, gy, 17), "fill-rule": "evenodd" }));
    g.append(svg("title", {}, "Configuración"));
    g.addEventListener("click", () => openSettings());
    g.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        openSettings();
      }
    });
    return g;
  }
  const [lx, ly] = polar((R_OUT + R_IN) / 2, mid);
  const [kx, ky] = polar(R_OUT - 20, mid);
  g.setAttribute("role", "button");
  g.setAttribute("tabindex", "0");
  g.setAttribute("aria-label", `Convertir a ${petal.label}${petal.lossy ? " (con pérdida)" : ""}. Tecla ${petal.key}`);
  g.append(svg("text", { class: "label", x: lx, y: ly }, petal.label));
  g.append(svg("text", { class: "hint", x: kx, y: ky }, String(petal.key)));
  if (petal.lossy) {
    const [px, py] = polar((R_OUT + R_IN) / 2, mid);
    g.append(svg("text", { class: "lossy", x: px, y: py + 17 }, "con pérdida"));
    g.append(svg("title", {}, `${petal.label}: puede perder calidad o información`));
  }
  const choose = () => convert(petal);
  g.addEventListener("click", choose);
  g.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      choose();
    }
  });
  g.dataset.key = String(petal.key);
  return g;
}

// ---------------------------------------------------------------- ajustes

let view = null;        // última respuesta de get_settings
let saveTimer = null;

const settingsOpen = () => !settingsPanel.hidden;

/** Invoca al backend; en un navegador sin backend (vista previa) usa datos de muestra. */
async function call(name, args) {
  if (invoke) return invoke(name, args);
  if (name === "get_settings") {
    return {
      settings: { document_engine: "auto", jpeg_quality: 90, webp_quality: 80, pdf_dpi: 150, pdf_paper: "a4" },
      libreoffice: { none: null, managed: "descargado" }[new URLSearchParams(location.search).get("lo")] ?? "incluido",
      managed_mb: new URLSearchParams(location.search).get("lo") === "managed" ? 690 : null,
      can_uninstall_app: new URLSearchParams(location.search).has("app"),
      word: true,
      wordcraft: new URLSearchParams(location.search).get("wc") !== "none",
      can_install_libreoffice: true,
      install_hint: "",
      shortcut: "Ctrl+Alt+Espacio",
      offer: { id: "libreoffice", title: "LibreOffice", detail: "Hace falta solo para PDF → Word." },
    };
  }
  if (name === "save_settings") return args.settings;
  if (name === "uninstall_libreoffice") return 690;
  return null;
}

function el(tag, props = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (k === "class") node.className = v;
    else if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
    else node.setAttribute(k, v);
  }
  node.append(...children);
  return node;
}

function scheduleSave() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    try {
      view.settings = await call("save_settings", { settings: view.settings });
    } catch (e) {
      console.error(e);
    }
  }, 250);
}

/** Botones excluyentes (no se usan <select>: su menú del sistema le quita el foco a la paleta). */
function segmented(options, current, onPick) {
  const group = el("div", { class: "seg", role: "radiogroup" });
  for (const { value, label, disabled, title } of options) {
    const button = el("button", { type: "button", role: "radio", "aria-checked": String(value === current) }, label);
    if (disabled) button.disabled = true;
    if (title) button.title = title;
    if (value === current) button.classList.add("on");
    button.addEventListener("click", () => {
      onPick(value);
      for (const other of group.children) {
        other.classList.toggle("on", other === button);
        other.setAttribute("aria-checked", String(other === button));
      }
    });
    group.append(button);
  }
  return group;
}

function slider(min, max, current, onInput) {
  const output = el("span", { class: "val" }, String(current));
  const input = el("input", { type: "range", min: String(min), max: String(max), value: String(current) });
  input.addEventListener("input", () => {
    output.textContent = input.value;
    onInput(Number(input.value));
  });
  return el("div", { class: "slider" }, input, output);
}

/** Botón que pide confirmación: el primer clic lo «arma» durante 4 s, el segundo ejecuta. */
function confirmButton(label, confirmLabel, action) {
  const button = el("button", { type: "button", class: "act danger" }, label);
  let timer = null;
  button.addEventListener("click", async () => {
    if (timer === null) {
      button.textContent = confirmLabel;
      button.classList.add("armed");
      timer = setTimeout(() => {
        timer = null;
        button.textContent = label;
        button.classList.remove("armed");
      }, 4000);
      return;
    }
    clearTimeout(timer);
    timer = null;
    button.disabled = true;
    await action();
  });
  return button;
}

let maintenanceNote = null;   // { text, error } tras una acción de mantenimiento

async function removeLibreOffice() {
  try {
    const mb = await call("uninstall_libreoffice");
    maintenanceNote = { text: `Se liberaron ${mb} MB.` };
    view = await call("get_settings");
  } catch (e) {
    maintenanceNote = { text: String(e), error: true };
  }
  renderSettings();
}

async function uninstallApp() {
  try {
    await call("uninstall_app");
    maintenanceNote = { text: "Abriendo el desinstalador de Windows…" };
  } catch (e) {
    maintenanceNote = { text: String(e), error: true };
  }
  renderSettings();
}

const row = (label, control, hint) =>
  el("div", { class: "row" }, el("div", { class: "lab" }, label, hint ? el("small", {}, hint) : ""), control);

function renderSettings() {
  const s = view.settings;
  const set = (key) => (value) => {
    s[key] = value;
    scheduleSave();
  };
  const lo = view.libreoffice;
  const loText = lo === "incluido" ? "incluido en la aplicación" : lo === "descargado" ? "descargado por la aplicación" : lo === "sistema" ? "instalado en el equipo" : null;
  const dpi = [72, 150, 200, 300];
  if (!dpi.includes(s.pdf_dpi)) dpi.push(s.pdf_dpi);

  const sections = [];

  // LibreOffice ya no es imprescindible: WordCraft hace casi todo. Solo hace falta para PDF → Word.
  const installRow = lo
    ? el("div", { class: "status ok" }, `LibreOffice: ${loText}`)
    : view.can_install_libreoffice
      ? el("div", { class: "status" }, "LibreOffice: no instalado (solo hace falta para PDF → Word) ", el("button", { type: "button", class: "act primary", onclick: () => { closeSettings(); install(view.offer, null); } }, "Instalar"))
      : el("div", { class: "status" }, `LibreOffice: no instalado (solo hace falta para PDF → Word). ${view.install_hint}`);
  sections.push(
    el("h3", {}, "Documentos (Word, PDF, Markdown)"),
    row("Programa", segmented([
      { value: "auto", label: "Automático", title: "WordCraft si sabe hacerlo; si no, LibreOffice o Word" },
      { value: "wordcraft", label: "WordCraft", disabled: !view.wordcraft, title: view.wordcraft ? "Rápido; no hace PDF → Word" : "No se encontró wordcraft-cli" },
      { value: "libreoffice", label: "LibreOffice" },
      { value: "word", label: "Word", disabled: !view.word, title: view.word ? "" : "Word no está instalado" },
    ], s.document_engine, set("document_engine"))),
    el("div", { class: "status" + (view.wordcraft ? " ok" : "") }, `WordCraft: ${view.wordcraft ? "incluido en la aplicación" : "no encontrado"}`),
    installRow,
    el("div", { class: "status" + (view.word ? " ok" : "") }, `Word: ${view.word ? "disponible" : "no encontrado"}`),
  );

  sections.push(
    el("h3", {}, "Imágenes"),
    row("Calidad JPG", slider(30, 100, s.jpeg_quality, set("jpeg_quality"))),
    row("Calidad WEBP", slider(30, 100, s.webp_quality, set("webp_quality")), "con pérdida (desde JPG)"),
  );

  sections.push(
    el("h3", {}, "PDF"),
    row("Papel al crear PDF", segmented([
      { value: "a4", label: "A4" },
      { value: "letter", label: "Carta" },
    ], s.pdf_paper, set("pdf_paper"))),
    row("PDF a imagen", segmented(dpi.map((d) => ({ value: d, label: `${d} dpi` })), s.pdf_dpi, set("pdf_dpi"))),
  );

  sections.push(
    el("h3", {}, "Atajo"),
    el("div", { class: "status" }, el("kbd", {}, view.shortcut), " · poder cambiarlo llegará más adelante"),
    el("h3", {}, "Reconocimiento de texto (OCR)"),
    el("div", { class: "status dim" }, "Próximamente: leer texto de escaneos e imágenes."),
  );

  // Mantenimiento: quitar lo que se descargó y desinstalar el programa.
  sections.push(el("h3", {}, "Mantenimiento"));
  if (lo === "descargado" && view.managed_mb) {
    sections.push(row("LibreOffice descargado", confirmButton("Quitar", "¿Seguro? Otra vez", removeLibreOffice), `Ocupa ${view.managed_mb} MB; se puede volver a descargar`));
  } else if (lo === "incluido") {
    sections.push(el("div", { class: "status dim" }, "LibreOffice viene incluido: se quita al desinstalar Converter."));
  } else if (lo === "sistema") {
    sections.push(el("div", { class: "status dim" }, "LibreOffice está instalado en el equipo: Converter no lo gestiona."));
  } else {
    sections.push(el("div", { class: "status dim" }, "No hay LibreOffice descargado que quitar."));
  }
  if (view.can_uninstall_app) {
    sections.push(row("Desinstalar Converter", confirmButton("Desinstalar…", "¿Seguro? Otra vez", uninstallApp), "se abre el desinstalador de Windows"));
  } else {
    sections.push(el("div", { class: "status dim" }, "Esta copia no se instaló con el instalador: para quitarla, borra su carpeta."));
  }
  if (maintenanceNote) {
    sections.push(el("div", { class: "status" + (maintenanceNote.error ? " err" : " ok") }, maintenanceNote.text));
    maintenanceNote = null;
  }

  settingsBody.replaceChildren(...sections);
}

async function openSettings() {
  try {
    view = await call("get_settings");
  } catch (e) {
    setCenter({ label: "Error", labelSmall: true, name: String(e), nameClass: "err wrap", ocr: false });
    return;
  }
  clearTimeout(hideTimer);
  if (invoke) invoke("pin_palette", { pinned: true });
  renderSettings();
  settingsPanel.hidden = false;
  wheel.classList.add("settings-open");
  settingsClose.focus({ preventScroll: true });
}

function closeSettings() {
  if (settingsPanel.hidden) return;
  clearTimeout(saveTimer);
  if (view) call("save_settings", { settings: view.settings }); // lo pendiente se guarda al cerrar
  settingsPanel.hidden = true;
  wheel.classList.remove("settings-open");
  if (invoke) invoke("pin_palette", { pinned: false });
}

settingsClose.addEventListener("click", closeSettings);

function setCenter({ label = "", name = "", labelSmall = false, nameClass = "", spinner = false, ocr = true, showActions = false, progress = null }) {
  center.querySelector(".spinner")?.remove();
  if (spinner) {
    const s = document.createElement("div");
    s.className = "spinner";
    center.prepend(s);
  }
  srcLabel.textContent = label;
  srcLabel.classList.toggle("small", labelSmall);
  srcName.textContent = name;
  srcName.className = `src-name ${nameClass}`.trim();
  center.classList.toggle("hide-ocr", !ocr);
  actions.hidden = !showActions;
  bar.hidden = progress === null;
  bar.classList.toggle("indeterminate", progress === "indeterminate");
  barFill.style.width = typeof progress === "number" ? `${Math.round(progress * 100)}%` : "";
}

/** Oferta de instalar un componente (LibreOffice). `pending` es la conversión que la pidió. */
function showOffer(offer, pending, { yes = "Descargar", no = "Ahora no", detail = offer.detail } = {}) {
  clearTimeout(hideTimer);
  busy = false;
  wheel.classList.remove("busy");
  setCenter({ label: offer.title, labelSmall: true, name: detail, nameClass: "wrap", ocr: false, showActions: true });
  actYes.textContent = yes;
  actNo.textContent = no;
  actYes.onclick = () => install(offer, pending);
  actNo.onclick = () => hide();
  actYes.focus({ preventScroll: true });
}

const MB = 1_000_000;
function onProgress({ phase, done, total }) {
  if (phase === "downloading") {
    const pct = total ? Math.floor((done / total) * 100) : 0;
    setCenter({ label: "Descargando", labelSmall: true, name: `${pct} % · ${Math.round(done / MB)} de ${Math.round(total / MB)} MB`, ocr: false, progress: total ? done / total : "indeterminate" });
  } else if (phase === "verifying") {
    setCenter({ label: "Verificando", labelSmall: true, name: "Comprobando que la descarga está intacta", nameClass: "wrap", ocr: false, progress: "indeterminate" });
  } else if (phase === "extracting") {
    setCenter({ label: "Instalando", labelSmall: true, name: "Puede tardar un par de minutos", nameClass: "wrap", ocr: false, progress: "indeterminate" });
  }
}

async function install(offer, pending) {
  if (busy) return;
  busy = true;
  wheel.classList.add("busy");
  setCenter({ label: "Descargando", labelSmall: true, name: "Conectando…", ocr: false, progress: "indeterminate" });
  try {
    await invoke("install_component", { id: offer.id });
  } catch (e) {
    showOffer(offer, pending, { yes: "Reintentar", no: "Cerrar", detail: String(e) });
    return;
  }
  busy = false;
  if (pending) {
    convert(pending); // la conversión que lo necesitaba continúa sola
  } else {
    setCenter({ label: "Listo", labelSmall: true, name: `${offer.title} instalado`, nameClass: "ok", ocr: false });
    scheduleHide(2000);
  }
}

function render(payload) {
  clearTimeout(hideTimer);
  busy = false;
  wheel.classList.remove("busy");
  ring.replaceChildren();
  closeSettings();
  payload.slots.forEach((petal, i) => {
    if (i === payload.settings_slot) ring.append(petalNode(i, null, true));
    else if (petal) ring.append(petalNode(i, petal, false));
  });
  if (payload.offer) {
    showOffer(payload.offer, null);
  } else if (payload.source) {
    setCenter({ label: payload.source.label, name: payload.source.name });
  } else {
    setCenter({ label: payload.notice ?? "Suelta un archivo aquí", labelSmall: true, nameClass: "wrap", ocr: false });
  }
}

async function convert(petal) {
  if (busy) return;
  busy = true;
  wheel.classList.add("busy");
  setCenter({ label: "Convirtiendo", labelSmall: true, name: `a ${petal.label}`, spinner: true, ocr: false });
  try {
    const outcome = await invoke("convert", { format: petal.id });
    if (outcome.needs) {
      showOffer(outcome.needs, petal);
    } else if (outcome.failed.length === 0) {
      setCenter({ label: "Listo", labelSmall: true, name: outcome.summary, nameClass: "ok wrap", ocr: false });
      scheduleHide(1500);
    } else {
      const first = outcome.failed[0];
      const prefix = outcome.converted ? `${outcome.converted} bien, ${outcome.failed.length} con error. ` : "";
      setCenter({ label: "No se pudo convertir", labelSmall: true, name: `${prefix}${first.name}: ${first.error}`, nameClass: "err wrap", ocr: false });
      scheduleHide(7000);
    }
  } catch (e) {
    setCenter({ label: "Error", labelSmall: true, name: String(e), nameClass: "err wrap", ocr: false });
    scheduleHide(7000);
  }
}

function scheduleHide(ms) {
  clearTimeout(hideTimer);
  hideTimer = setTimeout(hide, ms);
}

function hide() {
  if (invoke) invoke("hide_palette");
}

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    if (settingsOpen()) closeSettings();
    else hide();
  } else if (e.key === "0" && !busy && !settingsOpen()) {
    openSettings();
  } else if (/^[1-7]$/.test(e.key) && !busy) {
    ring.querySelector(`.petal[data-key="${e.key}"]`)?.dispatchEvent(new Event("click"));
  }
});

if (tauri?.event) {
  tauri.event.listen("palette://open", (event) => render(event.payload));
  tauri.event.listen("component://progress", (event) => onProgress(event.payload));
  tauri.event.listen("palette://settings", () => openSettings());
  // Si el evento se emitió antes de que esta página cargara, el backend guarda el último estado.
  invoke("current_palette").then((payload) => payload && render(payload));
} else {
  // Vista previa en un navegador normal (sin backend): `index.html?demo=png|csv|empty`.
  const demo = new URLSearchParams(location.search).get("demo") ?? "png";
  const mk = (id, label, key, lossy = false) => ({ id, label, key, lossy });
  const samples = {
    png: { source: { label: "PNG", name: "captura de pantalla.png", count: 1 }, slots: [mk("webp", "WEBP", 1), mk("jpg", "JPG", 2, true), mk("bmp", "BMP", 3), null, mk("tiff", "TIFF", 4), mk("gif", "GIF", 5, true), null, null], settings_slot: 3, notice: null },
    csv: { source: { label: "CSV", name: "ventas 2026.csv", count: 1 }, slots: [mk("xlsx", "XLSX", 1), mk("json", "JSON", 2), null, null, null, null, null, null], settings_slot: 3, notice: null },
    empty: { source: null, slots: Array(8).fill(null), settings_slot: 3, notice: "Suelta un archivo aquí" },
  };
  render(samples[demo === "settings" ? "png" : demo] ?? samples.png);
  if (demo === "settings") openSettings().then(() => { if (new URLSearchParams(location.search).has("end")) settingsBody.scrollTop = 9999; });
}

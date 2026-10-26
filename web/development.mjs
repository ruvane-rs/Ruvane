const LIMIT = 32768, MAX_U64 = 18446744073709551615n;
function decimal(value) {
  if (!/^(0|[1-9][0-9]{0,19})$/.test(value) || BigInt(value) > MAX_U64) throw new Error("Invalid development status");
  return value;
}

export function parseStatus(text, session) {
  if (typeof text !== "string" || new TextEncoder().encode(text).length > LIMIT || /[\x00-\x08\x0b-\x1f\x7f-\x9f]/.test(text)) throw new Error("Invalid development status");
  const split = text.indexOf("\n\n"), lines = text.slice(0, split).split("\n");
  if (split < 0 || lines.length !== 5 || lines[0] !== "RVD1" || decimal(lines[1]) !== session || decimal(lines[2]) === "0" || !["building", "ready", "error"].includes(lines[3]) || decimal(lines[4]) === "0" && lines[3] === "ready") throw new Error("Invalid development status");
  return Object.freeze({ sequence: BigInt(lines[2]), state: lines[3], revision: lines[4], message: text.slice(split + 2) });
}

export function changedFields(fields) {
  return [...fields].some(field => {
    if (field.tagName === "SELECT") return [...field.options].some(option => option.selected !== option.defaultSelected);
    if (field.type === "checkbox" || field.type === "radio") return field.checked !== field.defaultChecked;
    if (field.type === "file") return field.files.length > 0;
    return field.value !== field.defaultValue;
  });
}

export function reloadSafety(state, { dirty = false, composing = false, nativeSubmit = false, hidden = false } = {}) {
  if (nativeSubmit || state?.actions?.pending > 0) return "pending";
  if (state?.actions?.uncertain) return "uncertain";
  if (!state?.ready || state.actions?.state !== "active" || state.navigationPending) return "unavailable";
  if (dirty || composing) return "edited";
  if (hidden) return "hidden";
  return "safe";
}

export async function readStatus(response) {
  if (!response.ok || response.headers.get("content-type") !== "text/plain; charset=utf-8") throw new Error("Development status unavailable");
  const declared = response.headers.get("content-length");
  if (declared !== null && (!/^[0-9]+$/.test(declared) || Number(declared) > LIMIT)) throw new Error("Development status too large");
  const reader = response.body.getReader(), chunks = [];
  let length = 0;
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      length += value.length;
      if (length > LIMIT) throw new Error("Development status too large");
      chunks.push(value);
    }
    const bytes = new Uint8Array(length);
    let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } finally { await reader.cancel().catch(() => {}); reader.releaseLock(); }
}

export function startDevelopment({ document, window, fetch, loadApplication = url => import(url) }) {
  const markers = document.querySelectorAll("#ruvane-development");
  if (markers.length !== 1) throw new Error("Invalid development configuration");
  const marker = markers[0], session = decimal(marker.dataset.session), revision = decimal(marker.dataset.revision);
  if (session === "0" || marker.getAttribute("src") !== "/_ruvane/dev/client.mjs") throw new Error("Invalid development configuration");
  const placeholder = revision === "0";
  let application = null, composing = false, nativeSubmit = false, editedContent = false;
  let stopped = false, timer = null, controller = null, latest = null, sequence = 0n, failures = 0, reloading = false, foreign = false;
  const listeners = [];
  const listen = (target, name, fn) => { target.addEventListener(name, fn); listeners.push(() => target.removeEventListener(name, fn)); };
  const style = document.createElement("link"); style.rel = "stylesheet"; style.href = "/_ruvane/dev/style.css"; document.head.append(style);
  const panel = document.createElement("aside"); panel.id = "ruvane-development-panel"; panel.setAttribute("aria-label", "Ruvane development"); panel.hidden = true;
  const title = document.createElement("p"); title.setAttribute("role", "status"); title.setAttribute("aria-live", "polite");
  const message = document.createElement("p"), diagnostics = document.createElement("pre"), reload = document.createElement("button");
  reload.type = "button"; reload.textContent = "Reload";
  panel.append(title, message, diagnostics, reload); document.body.append(panel);
  if (!placeholder) {
    const root = document.getElementById("ruvane-application"), wasm = root?.getAttribute("data-ruvane-module");
    if (root?.getAttribute("data-ruvane-revision") !== revision || !/^\/ruvane-runtime\/[0-9a-f]{16}\/app\.wasm$/.test(wasm ?? "")) throw new Error("Invalid development application");
    const hex = BigInt(revision).toString(16).padStart(16, "0");
    if (wasm !== `/ruvane-runtime/${hex}/app.wasm`) throw new Error("Invalid development revision");
    Promise.resolve().then(() => loadApplication(wasm.replace("app.wasm", "application.mjs"))).then(module => { if (!stopped && typeof module.applicationState === "function") application = module; }).catch(() => {});
  }
  function safety() {
    let state;
    try { state = placeholder ? { ready: true, actions: { state: "active", pending: 0, uncertain: false }, navigationPending: false } : application?.applicationState(); } catch { state = null; }
    return reloadSafety(state, { dirty: editedContent || changedFields(document.querySelectorAll("input,textarea,select")), composing, nativeSubmit, hidden: document.hidden });
  }
  function update() {
    if (stopped || reloading) return;
    reload.hidden = true; diagnostics.hidden = true;
    if (foreign) { panel.hidden = false; title.textContent = "Development session changed"; message.textContent = "Open the current development URL to reconnect."; return; }
    if (!latest) { panel.hidden = false; title.textContent = failures >= 3 ? "Development server disconnected" : "Connecting to development server…"; message.textContent = ""; return; }
    panel.hidden = false;
    if (latest.state === "error") {
      title.textContent = "Build failed"; message.textContent = "Fix the error and save to rebuild. The previous application remains available.";
      diagnostics.textContent = latest.message; diagnostics.hidden = false; return;
    }
    if (latest.state === "building") { title.textContent = "Building…"; message.textContent = ""; return; }
    if (latest.revision === revision) { panel.hidden = true; return; }
    const reason = safety();
    if (reason === "safe" && failures === 0) { reloading = true; window.location.reload(); return; }
    title.textContent = "New version available";
    message.textContent = reason === "pending" ? "Waiting for the current action or form submission to finish." : reason === "uncertain" ? "An action outcome is uncertain. Reload does not replay it. Unsaved changes will be lost." : "Reload when ready. Unsaved changes will be lost.";
    reload.hidden = false; reload.disabled = reason === "pending";
  }
  listen(reload, "click", () => { if (!stopped && latest?.state === "ready" && safety() !== "pending") { reloading = true; window.location.reload(); } });
  listen(document, "compositionstart", () => { composing = true; update(); });
  listen(document, "compositionend", () => { composing = false; update(); });
  listen(document, "input", event => { if (event.target?.isContentEditable) editedContent = true; update(); });
  listen(document, "change", update);
  listen(document, "submit", event => { if (!event.defaultPrevented) { nativeSubmit = true; update(); } });
  function schedule(delay = 1000) { if (!stopped && !foreign && !document.hidden) timer = window.setTimeout(poll, delay); }
  async function poll() {
    timer = null;
    if (stopped || controller || document.hidden || foreign || reloading) return;
    controller = new AbortController();
    const active = controller, timeout = window.setTimeout(() => active.abort(), 2000);
    try {
      const response = await fetch("/_ruvane/dev/status", { headers: { Accept: "text/plain", "Ruvane-Development": session }, credentials: "same-origin", mode: "same-origin", cache: "no-store", redirect: "error", signal: active.signal });
      if (response.status === 409) { foreign = true; throw new Error("Development session changed"); }
      const status = parseStatus(await readStatus(response), session);
      if (status.sequence < sequence) throw new Error("Development status went backwards");
      if (status.sequence === sequence && latest && (status.state !== latest.state || status.revision !== latest.revision || status.message !== latest.message)) throw new Error("Development status changed without a sequence");
      sequence = status.sequence; latest = status; failures = 0;
    } catch { failures++; }
    finally { window.clearTimeout(timeout); controller = null; if (!stopped) { update(); schedule(Math.min(8000, 1000 * 2 ** Math.min(failures, 3))); } }
  }
  listen(document, "visibilitychange", () => { window.clearTimeout(timer); timer = null; if (document.hidden) controller?.abort(); else if (!controller) poll(); });
  function stop() { if (stopped) return; stopped = true; window.clearTimeout(timer); controller?.abort(); for (const remove of listeners) remove(); panel.remove(); style.remove(); }
  listen(window, "pagehide", stop);
  update(); poll();
  return Object.freeze({ stop });
}

if (typeof document !== "undefined" && typeof window !== "undefined") {
  try { startDevelopment({ document, window, fetch: window.fetch.bind(window) }); } catch { /* Leave native navigation available. */ }
}

// DOM bridge for RVN1 commands.
export const ABI_VERSION = 1;
export const LIMITS = Object.freeze({ bytes: 65536, commands: 128, text: 1024, views: 16, nodes: 32, listeners: 16, moduleBytes: 1048576 });
const TAGS = Object.freeze({ 1: "section", 2: "h2", 3: "p", 4: "span", 5: "output", 6: "button" });
const FUNCTIONS = ["ruvane_mount", "ruvane_dispatch", "ruvane_dispose", "ruvane_commands_ptr", "ruvane_commands_len", "ruvane_commands_clear", "ruvane_last_error", "ruvane_live_instances", "ruvane_live_signals", "ruvane_live_effects"];
const invalid = () => new Error("Invalid Ruvane browser protocol");
function uint(value, zero = false) {
  if (!Number.isInteger(value) || value < (zero ? 0 : 1) || value > 0xffffffff) throw invalid();
  return value;
}
function result(value) {
  if (!Number.isInteger(value) || value < -2147483648 || value > 0xffffffff) throw invalid();
  return value >>> 0;
}

/** Checks the module's exports and ABI version. */
export function validateExports(exports) {
  if (!exports || !(exports.memory instanceof WebAssembly.Memory) || typeof exports.ruvane_abi_version !== "function") throw invalid();
  if (result(exports.ruvane_abi_version()) !== ABI_VERSION) throw new Error("Unsupported Ruvane browser ABI");
  if (FUNCTIONS.some(name => typeof exports[name] !== "function")) throw invalid();
  return exports;
}

/** Decodes a batch before touching the DOM. */
export function decodeCommands(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.length < 8 || bytes.length > LIMITS.bytes) throw invalid();
  if (bytes[0] !== 82 || bytes[1] !== 86 || bytes[2] !== 78 || bytes[3] !== 49) throw invalid();
  const data = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let offset = 4;
  const u8 = () => { if (offset >= bytes.length) throw invalid(); return bytes[offset++]; };
  const u32 = () => { if (offset + 4 > bytes.length) throw invalid(); const n = data.getUint32(offset, true); offset += 4; return n; };
  const count = u32();
  if (count > LIMITS.commands) throw invalid();
  const decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });
  const commands = [];
  for (let i = 0; i < count; i++) {
    const op = u8(), view = uint(u32());
    if (op === 5) { commands.push({ op, view }); continue; }
    if (![1, 2, 3, 4].includes(op)) throw invalid();
    const node = uint(u32());
    if (op === 1) {
      const parent = u32(), tag = u8();
      if (!Object.hasOwn(TAGS, tag)) throw invalid();
      commands.push({ op, view, node, parent, tag });
    } else if (op === 2) {
      const size = u32();
      if (size > LIMITS.text || offset + size > bytes.length) throw invalid();
      const text = decoder.decode(bytes.subarray(offset, offset + size));
      offset += size;
      commands.push({ op, view, node, text });
    } else if (op === 3) {
      commands.push({ op, view, node, event: uint(u32()) });
    } else {
      const disabled = u8();
      if (disabled > 1) throw invalid();
      commands.push({ op, view, node, disabled: disabled === 1 });
    }
  }
  if (offset !== bytes.length) throw invalid();
  return commands;
}

async function loadModule(url) {
  const target = new URL(url, location.href);
  if (target.origin !== location.origin || !["http:", "https:"].includes(target.protocol) || target.username || target.password || target.hash) throw invalid();
  const abort = new AbortController();
  const timer = setTimeout(() => abort.abort(), 10000);
  let bytes;
  try {
    const response = await fetch(target, { credentials: "same-origin", redirect: "error", signal: abort.signal });
    if (response.status !== 200 || response.headers.get("content-type")?.split(";")[0].trim().toLowerCase() !== "application/wasm") throw new Error("Invalid Ruvane Wasm response");
    const length = response.headers.get("content-length");
    if (length !== null && (!/^\d+$/.test(length) || Number(length) > LIMITS.moduleBytes)) throw invalid();
    if (!response.body) throw invalid();
    const reader = response.body.getReader(), parts = [];
    let total = 0;
    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        total += value.length;
        if (total > LIMITS.moduleBytes) { await reader.cancel(); throw invalid(); }
        parts.push(value);
      }
    } finally { reader.releaseLock(); }
    bytes = new Uint8Array(total);
    let offset = 0;
    for (const part of parts) { bytes.set(part, offset); offset += part.length; }
  } finally { clearTimeout(timer); }
  // Modules are application code. They receive no host imports.
  return validateExports((await WebAssembly.instantiate(bytes, {})).instance.exports);
}

/** Loads a module and manages its views. Disposal is final. */
export async function createRuntime(url) {
  let exports = await loadModule(url);
  const views = new Map(), roots = new WeakMap();
  let nextRoot = 0, busy = false, closed = false, faulted = false;
  let finalRust = null;

  function live() { if (closed) throw new Error("Ruvane browser runtime disposed"); }
  function forget(id) {
    const view = views.get(id);
    if (!view) return;
    for (const [node, entry] of view.listeners) view.nodes.get(node).element.removeEventListener("click", entry.listener);
    view.listeners.clear();
    // Leave host-owned siblings alone.
    for (const entry of view.nodes.values()) {
      if (entry.parent === 0 && entry.element.parentNode === view.root) entry.element.remove();
    }
    view.nodes.clear();
    roots.delete(view.root);
    views.delete(id);
  }
  function nativeStats() {
    const instances = result(exports.ruvane_live_instances());
    const signals = result(exports.ruvane_live_signals());
    const effects = result(exports.ruvane_live_effects());
    if (instances > LIMITS.views || signals > LIMITS.views * 2 || effects > LIMITS.views * 2) throw invalid();
    return { rustInstances: instances, rustSignals: signals, rustEffects: effects };
  }
  function stop(failed) {
    if (closed) return;
    let cleaned = true;
    try {
      if (result(exports.ruvane_commands_clear()) !== 1) throw invalid();
      for (const id of views.keys()) {
        if (result(exports.ruvane_dispose(id)) !== 1 || result(exports.ruvane_commands_clear()) !== 1) throw invalid();
      }
      finalRust = nativeStats();
    } catch { cleaned = false; }
    for (const id of [...views.keys()]) forget(id);
    faulted = failed || !cleaned;
    if (!cleaned) finalRust = null;
    closed = true;
    exports = null;
  }
  function prepare(commands, id, phase) {
    const view = views.get(id);
    if (!view || view.root.ownerDocument !== document) throw invalid();
    const simulated = new Map([...view.nodes].map(([key, entry]) => [key, { tag: entry.tag, depth: entry.depth }]));
    const listening = new Set(view.listeners.keys());
    for (const [node, entry] of view.nodes) {
      if (node === 0) continue;
      if (!view.root.contains(entry.element) || entry.element.parentNode !== view.nodes.get(entry.parent)?.element || entry.element.localName !== TAGS[entry.tag]) throw invalid();
    }
    for (const command of commands) {
      if (command.view !== id) throw invalid();
      if (phase === "dispose") {
        if (commands.length !== 1 || command.op !== 5) throw invalid();
        continue;
      }
      if (command.op === 1) {
        const parent = simulated.get(command.parent);
        if (phase !== "mount" || simulated.has(command.node) || simulated.size > LIMITS.nodes || !parent || ![0, 1, 3].includes(parent.tag) || parent.depth >= 8) throw invalid();
        simulated.set(command.node, { tag: command.tag, depth: parent.depth + 1 });
      } else if (command.op === 5) {
        throw invalid();
      } else {
        const node = simulated.get(command.node);
        if (!node) throw invalid();
        if (command.op === 2 && ![2, 4, 5, 6].includes(node.tag)) throw invalid();
        if (command.op === 3) {
          if (phase !== "mount" || node.tag !== 6 || listening.has(command.node) || listening.size === LIMITS.listeners) throw invalid();
          listening.add(command.node);
        }
        if (command.op === 4 && node.tag !== 6) throw invalid();
      }
    }
    if (phase === "mount" && simulated.size === 1) throw invalid();
    if (phase === "dispose" && commands.length !== 1) throw invalid();
  }
  function drain(id, phase) {
    const pointer = result(exports.ruvane_commands_ptr());
    const size = result(exports.ruvane_commands_len());
    // Wasm calls may grow memory, so read its buffer again.
    const memory = exports.memory.buffer;
    if (size < 8 || size > LIMITS.bytes || pointer + size > memory.byteLength) throw invalid();
    const commands = decodeCommands(new Uint8Array(memory, pointer, size).slice());
    prepare(commands, id, phase);
    if (result(exports.ruvane_commands_clear()) !== 1) throw invalid();
    const view = views.get(id);
    for (const command of commands) {
      if (command.op === 1) {
        const element = document.createElement(TAGS[command.tag]);
        if (command.tag === 6) element.type = "button";
        element.dataset.ruvaneNode = String(command.node);
        view.nodes.get(command.parent).element.appendChild(element);
        view.nodes.set(command.node, { element, tag: command.tag, parent: command.parent, depth: view.nodes.get(command.parent).depth + 1 });
      } else if (command.op === 2) {
        view.nodes.get(command.node).element.textContent = command.text;
      } else if (command.op === 3) {
        const listener = () => {
          if (!views.has(id) || closed) return;
          try { dispatch(id, command.event); }
          catch { stop(true); console.error("Ruvane browser runtime stopped"); }
        };
        view.nodes.get(command.node).element.addEventListener("click", listener);
        view.listeners.set(command.node, { listener, event: command.event });
      } else if (command.op === 4) {
        view.nodes.get(command.node).element.disabled = command.disabled;
      } else { forget(id); }
    }
  }
  function dispatch(id, event) {
    live();
    uint(id); uint(event);
    if (![1, 2, 3].includes(event)) throw new RangeError("Unknown Ruvane counter event");
    if (!views.has(id)) throw new Error("Ruvane view disposed");
    if (busy) throw new Error("Ruvane bridge busy");
    busy = true;
    try {
      if (result(exports.ruvane_dispatch(id, event)) !== 1) throw invalid();
      drain(id, "dispatch");
    } catch (error) { stop(true); throw error; }
    finally { busy = false; }
  }
  function disposeView(id) {
    if (!views.has(id) || closed) return false;
    if (busy) throw new Error("Ruvane bridge busy");
    busy = true;
    try {
      if (result(exports.ruvane_dispose(id)) !== 1) throw invalid();
      drain(id, "dispose");
      return true;
    } catch (error) { stop(true); throw error; }
    finally { busy = false; }
  }
  return Object.freeze({
    mount(root) {
      live();
      if (!(root instanceof HTMLElement) || root.ownerDocument !== document || !["div", "section", "main", "article", "aside"].includes(root.localName) || root.childNodes.length || roots.has(root)) throw new Error("Ruvane requires an empty, unclaimed host root");
      if (views.size === LIMITS.views || nextRoot === 0xffffffff) throw new RangeError("Ruvane view limit exceeded");
      if (busy) throw new Error("Ruvane bridge busy");
      busy = true;
      try {
        const id = result(exports.ruvane_mount(++nextRoot));
        if (!id || views.has(id)) throw invalid();
        views.set(id, { root, nodes: new Map([[0, { element: root, tag: 0, depth: 0 }]]), listeners: new Map() });
        roots.set(root, id);
        drain(id, "mount");
        return Object.freeze({ id, dispatch: event => dispatch(id, event), dispose: () => disposeView(id) });
      } catch (error) { stop(true); throw error; }
      finally { busy = false; }
    },
    stats() {
      let domHandles = 0, eventListeners = 0;
      for (const view of views.values()) { domHandles += view.nodes.size - 1; eventListeners += view.listeners.size; }
      const native = closed ? finalRust : nativeStats();
      return Object.freeze({ state: closed ? (faulted ? "faulted" : "disposed") : "active", views: views.size, domHandles, eventListeners,
        rustInstances: native?.rustInstances ?? null, rustSignals: native?.rustSignals ?? null, rustEffects: native?.rustEffects ?? null });
    },
    dispose() { if (busy) throw new Error("Ruvane bridge busy"); stop(false); },
  });
}

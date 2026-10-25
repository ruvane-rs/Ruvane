// DOM bridge for RVN2 commands.
export const ABI_VERSION = 3;
export const LIMITS = Object.freeze({ bytes: 65536, commands: 256, text: 1024, views: 16, nodes: 32, listeners: 16, moduleBytes: 1048576, signals: 16384, effects: 16384 });
const TAGS = Object.freeze({ 1: "section", 2: "h2", 3: "p", 4: "span", 5: "output", 6: "button", 7: "ul", 8: "li" });
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
  if (bytes[0] !== 82 || bytes[1] !== 86 || bytes[2] !== 78 || bytes[3] !== 50) throw invalid();
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
    if (![1, 2, 3, 4, 6, 7].includes(op)) throw invalid();
    const node = uint(u32());
    if (op === 1) {
      const parent = u32(), tag = u8(), key = u32();
      if (!Object.hasOwn(TAGS, tag)) throw invalid();
      commands.push({ op, view, node, parent, tag, key });
    } else if (op === 2) {
      const size = u32();
      if (size > LIMITS.text || offset + size > bytes.length) throw invalid();
      const text = decoder.decode(bytes.subarray(offset, offset + size));
      offset += size;
      commands.push({ op, view, node, text });
    } else if (op === 3) {
      commands.push({ op, view, node, event: uint(u32()) });
    } else if (op === 6) {
      commands.push({ op, view, node });
    } else if (op === 7) {
      const before = u32();
      if (before === node) throw invalid();
      commands.push({ op, view, node, before });
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

const CLAIMS = new WeakSet();
class HydrationMismatch extends Error {
  constructor() { super("Ruvane hydration mismatch"); }
}
class MountRejected extends Error {
  constructor() { super("Ruvane component props or mount rejected"); }
}
const mismatch = () => { throw new HydrationMismatch(); };
const container = tag => [0, 1, 3, 7, 8].includes(tag);
const leaf = tag => [2, 4, 5, 6].includes(tag);

/** Checks and copies public mount props. */
export function validateProps(props) {
  if (!props || !props.state) throw invalid();
  const component = uint(props.component);
  if (props.state.kind === "number") return Object.freeze({ component, kind: 1, value: uint(props.state.value, true) });
  if (props.state.kind === "boolean" && typeof props.state.value === "boolean") return Object.freeze({ component, kind: 2, value: Number(props.state.value) });
  throw invalid();
}
function decimal(value, zero = false) {
  if (typeof value !== "string" || value.length > 10 || !/^(0|[1-9][0-9]*)$/.test(value)) throw invalid();
  return uint(Number(value), zero);
}
function readProps(root) {
  if (root.getAttribute("data-ruvane-version") !== String(ABI_VERSION)) throw new Error("Unsupported Ruvane hydration version");
  const component = decimal(root.getAttribute("data-ruvane-component"));
  const state = root.getAttribute("data-ruvane-state");
  if (typeof state !== "string" || state.length > 12 || !/^[12]:/.test(state)) throw invalid();
  const kind = Number(state[0]), value = decimal(state.slice(2), true);
  if (kind === 2 && value > 1) throw invalid();
  return { component, kind, value };
}
function host(root) {
  if (!(root instanceof HTMLElement) || root.ownerDocument !== document || !["div", "section", "main", "article", "aside"].includes(root.localName) || CLAIMS.has(root)) throw new Error("Invalid or claimed Ruvane host root");
  let parent = root.parentElement, depth = 0;
  while (parent) {
    if (++depth > 128 || CLAIMS.has(parent)) throw new Error("Nested Ruvane root claim");
    parent = parent.parentElement;
  }
}
function accepts(parent, tag) {
  if (parent === 7) return tag === 8;
  if (tag === 8) return false;
  if (parent === 3) return [4, 5, 6].includes(tag);
  return [0, 1, 8].includes(parent);
}

/** Validates decoded commands against a copy of the live tree. */
export function planCommands(commands, id, previous, initial = false) {
  uint(id);
  if (!Array.isArray(commands) || commands.length > LIMITS.commands) throw invalid();
  const nodes = new Map([...previous.nodes].map(([key, node]) => [key, { ...node, children: [...node.children] }]));
  const fresh = new Set(), seen = new Set();
  let nextNode = uint(previous.nextNode, true);
  for (const command of commands) {
    if (command.view !== id) throw invalid();
    uint(command.node);
    const key = `${command.node}:${command.op}`;
    if (seen.has(key)) throw invalid();
    seen.add(key);
    if (command.op === 1) {
      const parent = nodes.get(uint(command.parent, true));
      uint(command.key, true);
      if (!Object.hasOwn(TAGS, command.tag) || command.node <= nextNode || nodes.size > LIMITS.nodes || !parent || !accepts(parent.tag, command.tag) || parent.depth >= 8) throw invalid();
      if (command.key && parent.children.some(id => nodes.get(id).key === command.key)) throw invalid();
      nodes.set(command.node, { element: null, key: command.key, tag: command.tag, parent: command.parent, depth: parent.depth + 1, children: [], text: null, event: null, disabled: null });
      fresh.add(command.node); nextNode = command.node; parent.children.push(command.node);
      continue;
    }
    const node = nodes.get(command.node);
    if (!node) throw invalid();
    if (command.op === 2 && leaf(node.tag)) {
      if (typeof command.text !== "string" || command.text.length > LIMITS.text || new TextEncoder().encode(command.text).length > LIMITS.text) throw invalid();
      node.text = command.text;
    } else if (command.op === 3 && node.tag === 6 && fresh.has(command.node) && node.event === null) {
      node.event = uint(command.event);
    } else if (command.op === 4 && node.tag === 6 && typeof command.disabled === "boolean") {
      node.disabled = command.disabled;
    } else if (command.op === 6 && !initial && !fresh.has(command.node)) {
      const siblings = nodes.get(node.parent).children;
      siblings.splice(siblings.indexOf(command.node), 1);
      const pending = [command.node];
      while (pending.length) {
        const id = pending.pop(); pending.push(...nodes.get(id).children); nodes.delete(id);
      }
    } else if (command.op === 7 && !initial) {
      const before = uint(command.before, true);
      if (before === command.node || (before && nodes.get(before)?.parent !== node.parent)) throw invalid();
      const siblings = nodes.get(node.parent).children;
      siblings.splice(siblings.indexOf(command.node), 1);
      siblings.splice(before ? siblings.indexOf(before) : siblings.length, 0, command.node);
    } else throw invalid();
  }
  for (const command of commands) {
    if (command.op !== 6 && (!nodes.has(command.node) || (command.op === 7 && command.before && !nodes.has(command.before)))) throw invalid();
  }
  if (nodes.size < 2) throw invalid();
  let listeners = 0;
  for (const [id, node] of nodes) {
    if (id && leaf(node.tag) && node.text === null) throw invalid();
    if (node.tag === 6 && (node.event === null || node.disabled === null || ++listeners > LIMITS.listeners)) throw invalid();
  }
  return { nodes, nextNode };
}
function matchDom(nodes, root, inert) {
  function children(parent, element) {
    if (element.childNodes.length !== parent.children.length) mismatch();
    for (let index = 0; index < parent.children.length; index++) {
      const id = parent.children[index], node = nodes.get(id), actual = element.childNodes[index];
      if (!(actual instanceof HTMLElement) || actual.ownerDocument !== document || actual.namespaceURI !== "http://www.w3.org/1999/xhtml" || actual.localName !== TAGS[node.tag] || CLAIMS.has(actual)) mismatch();
      if (node.element && node.element !== actual) mismatch();
      const attributes = { "data-ruvane-node": String(id) };
      if (node.key) attributes["data-ruvane-key"] = String(node.key);
      if (node.tag === 6) {
        attributes.type = "button";
        attributes["data-ruvane-click"] = String(node.event);
        if (inert || node.disabled) attributes.disabled = "";
        if (actual.disabled !== (inert || node.disabled)) mismatch();
      }
      if (actual.attributes.length !== Object.keys(attributes).length || Object.entries(attributes).some(([key, value]) => actual.getAttribute(key) !== value)) mismatch();
      node.element = actual;
      if (container(node.tag)) children(node, actual);
      else if (node.text === "") { if (actual.childNodes.length !== 0) mismatch(); }
      else if (actual.childNodes.length !== 1 || actual.firstChild.nodeType !== 3 || actual.firstChild.data !== node.text) mismatch();
    }
  }
  children(nodes.get(0), root);
}
/** Loads a module and manages mounted or hydrated views. Disposal is final. */
export async function createRuntime(url) {
  let exports = await loadModule(url);
  const views = new Map();
  let nextRoot = 0, busy = false, closed = false, faulted = false, finalRust = null;

  function live() { if (closed) throw new Error("Ruvane browser runtime disposed"); }
  function forget(id, preserve = false) {
    const view = views.get(id);
    if (!view) return;
    for (const [node, listener] of view.listeners) view.nodes.get(node).element.removeEventListener("click", listener);
    view.listeners.clear();
    // A failed hydration leaves the server's content in place.
    for (const node of view.nodes.values()) {
      if (preserve) {
        if (node.tag === 6 && view.root.contains(node.element)) node.element.disabled = true;
      } else if (node.parent === 0 && node.element?.parentNode === view.root) node.element.remove();
    }
    view.nodes.clear(); view.events.clear(); CLAIMS.delete(view.root); views.delete(id);
  }
  function nativeStats() {
    const instances = result(exports.ruvane_live_instances());
    const signals = result(exports.ruvane_live_signals());
    const effects = result(exports.ruvane_live_effects());
    if (instances > LIMITS.views || signals > LIMITS.signals || effects > LIMITS.effects) throw invalid();
    return { rustInstances: instances, rustSignals: signals, rustEffects: effects };
  }
  function releaseNative(id) {
    if (result(exports.ruvane_commands_clear()) !== 1) throw invalid();
    const disposed = result(exports.ruvane_dispose(id));
    if (disposed !== 1 && result(exports.ruvane_last_error()) !== 3) throw invalid();
    if (result(exports.ruvane_commands_clear()) !== 1) throw invalid();
  }
  function stop(failed) {
    if (closed) return;
    let cleaned = true;
    for (const id of views.keys()) {
      try { releaseNative(id); } catch { cleaned = false; }
    }
    try {
      finalRust = nativeStats();
      if (finalRust.rustInstances || finalRust.rustSignals || finalRust.rustEffects) cleaned = false;
    } catch { cleaned = false; }
    for (const [id, view] of [...views]) forget(id, failed && view.hydrated);
    faulted = failed || !cleaned;
    if (!cleaned) finalRust = null;
    closed = true; exports = null;
  }
  function commands() {
    const pointer = result(exports.ruvane_commands_ptr()), size = result(exports.ruvane_commands_len());
    // Wasm calls may grow memory, so read its buffer again.
    const memory = exports.memory.buffer;
    if (size < 8 || size > LIMITS.bytes || pointer + size > memory.byteLength) throw invalid();
    return decodeCommands(new Uint8Array(memory, pointer, size).slice());
  }
  function bind(view, id, nodeId, node) {
    const listener = () => {
      if (closed || views.get(id)?.nodes.get(nodeId)?.element !== node.element) return;
      try { dispatchNode(id, nodeId, node.event); }
      catch { stop(true); console.error("Ruvane browser runtime stopped"); }
    };
    node.element.addEventListener("click", listener);
    view.listeners.set(nodeId, listener); view.events.add(node.event);
  }
  function createElement(nodes, command) {
    const node = nodes.get(command.node);
    node.element = document.createElement(TAGS[node.tag]);
    node.element.setAttribute("data-ruvane-node", String(command.node));
    if (node.key) node.element.setAttribute("data-ruvane-key", String(node.key));
    if (node.tag === 6) { node.element.type = "button"; node.element.disabled = true; }
    nodes.get(node.parent).element.appendChild(node.element);
  }
  function drain(id, phase) {
    const batch = commands(), view = views.get(id);
    if (phase === "dispose") {
      matchDom(view.nodes, view.root, false);
      if (batch.length !== 1 || batch[0].op !== 5 || batch[0].view !== id) throw invalid();
      if (result(exports.ruvane_commands_clear()) !== 1) throw invalid();
      forget(id); return;
    }
    const initial = phase !== "dispatch";
    if (!initial) matchDom(view.nodes, view.root, false);
    const plan = planCommands(batch, id, view, initial), nodes = plan.nodes;
    if (phase === "hydrate") matchDom(nodes, view.root, true);
    if (phase === "mount" && view.root.childNodes.length) throw invalid();
    if (result(exports.ruvane_commands_clear()) !== 1) throw invalid();
    const old = view.nodes;
    const focused = view.root.contains(document.activeElement) ? document.activeElement : null;
    for (const [nodeId, listener] of view.listeners) {
      if (!nodes.has(nodeId)) { old.get(nodeId).element.removeEventListener("click", listener); view.listeners.delete(nodeId); }
    }
    view.nodes = nodes; view.nextNode = plan.nextNode;
    if (phase !== "hydrate") {
      for (const command of batch) {
        if (command.op === 6) { old.get(command.node).element.remove(); continue; }
        if (command.op === 1) { createElement(nodes, command); continue; }
        const node = nodes.get(command.node);
        if (command.op === 2) node.element.textContent = command.text;
        else if (command.op === 3) node.element.setAttribute("data-ruvane-click", String(command.event));
        else if (command.op === 7) {
          const before = command.before ? nodes.get(command.before).element : null;
          if (node.element.nextSibling !== before) nodes.get(node.parent).element.insertBefore(node.element, before);
        }
      }
    }
    view.events.clear();
    for (const [nodeId, node] of nodes) {
      if (node.tag === 6) {
        node.element.disabled = node.disabled;
        view.events.add(node.event);
      }
    }
    matchDom(nodes, view.root, false);
    for (const [nodeId, node] of nodes) {
      if (node.tag === 6 && !view.listeners.has(nodeId)) bind(view, id, nodeId, node);
    }
    // insertBefore can blur a moved button. Restore focus only if it survived.
    if (focused && view.root.contains(focused) && !focused.disabled && document.activeElement !== focused) focused.focus({ preventScroll: true });
  }
  function start(root, props, hydrated) {
    live(); host(root);
    if (!hydrated && root.childNodes.length) throw new Error("Ruvane mount requires an empty root");
    if (views.size === LIMITS.views || nextRoot === 0xffffffff) throw new RangeError("Ruvane view limit exceeded");
    if (busy) throw new Error("Ruvane bridge busy");
    busy = true;
    let id = 0;
    try {
      id = result(exports.ruvane_mount(++nextRoot, props.component, props.kind, props.value));
      if (!id) {
        if ([1, 2, 7].includes(result(exports.ruvane_last_error()))) throw new MountRejected();
        throw invalid();
      }
      if (views.has(id)) throw invalid();
      views.set(id, { root, hydrated, nodes: new Map([[0, { element: root, tag: 0, depth: 0, children: [], key: 0 }]]), nextNode: 0, listeners: new Map(), events: new Set() });
      CLAIMS.add(root);
      drain(id, hydrated ? "hydrate" : "mount");
      return Object.freeze({ id, dispatch: event => dispatch(id, event), dispose: () => disposeView(id) });
    } catch (error) {
      if (error instanceof HydrationMismatch || error instanceof MountRejected) {
        try { if (id) { releaseNative(id); forget(id, true); } }
        catch { stop(true); }
      } else stop(true);
      throw error;
    } finally { busy = false; }
  }
  function dispatch(id, event) {
    live(); uint(id); uint(event);
    const view = views.get(id);
    if (!view) throw new Error("Ruvane view disposed");
    const found = [...view.nodes].find(([, node]) => node.event === event);
    if (!found) throw new RangeError("Unknown Ruvane component event");
    return dispatchNode(id, found[0], event);
  }
  function dispatchNode(id, nodeId, event) {
    live(); uint(id); uint(nodeId); uint(event);
    const view = views.get(id), node = view?.nodes.get(nodeId);
    if (!node || node.event !== event) throw new RangeError("Expired Ruvane event binding");
    if (busy) throw new Error("Ruvane bridge busy");
    busy = true;
    try {
      if (result(exports.ruvane_dispatch(id, nodeId, event)) !== 1) throw invalid();
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
      drain(id, "dispose"); return true;
    } catch (error) { stop(true); throw error; }
    finally { busy = false; }
  }
  return Object.freeze({
    mount(root, props) { return start(root, validateProps(props), false); },
    hydrate(root) { live(); host(root); return start(root, readProps(root), true); },
    stats() {
      let domHandles = 0, eventListeners = 0;
      for (const view of views.values()) { domHandles += Math.max(0, view.nodes.size - 1); eventListeners += view.listeners.size; }
      const native = closed ? finalRust : nativeStats();
      return Object.freeze({ state: closed ? (faulted ? "faulted" : "disposed") : "active", views: views.size, domHandles, eventListeners,
        rustInstances: native?.rustInstances ?? null, rustSignals: native?.rustSignals ?? null, rustEffects: native?.rustEffects ?? null });
    },
    dispose() { if (busy) throw new Error("Ruvane bridge busy"); stop(false); },
  });
}

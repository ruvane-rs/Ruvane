// DOM and action bridge for RVN6 commands.
export const ABI_VERSION = 11;
export const LIMITS = Object.freeze({ bytes: 65536, commands: 256, text: 1024, views: 16, nodes: 32, listeners: 16, moduleBytes: 1048576, signals: 16384, effects: 16384 });
const TAGS = Object.freeze({ 1: "section", 2: "h2", 3: "p", 4: "span", 5: "output", 6: "button", 7: "ul", 8: "li", 9: "form", 10: "label", 11: "input", 12: "input", 13: "textarea", 14: "button", 15: "a" });
const FUNCTIONS = ["ruvane_mount", "ruvane_dispatch", "ruvane_dispose", "ruvane_commands_ptr", "ruvane_commands_len", "ruvane_commands_clear", "ruvane_last_error", "ruvane_live_instances", "ruvane_live_signals", "ruvane_live_effects", "ruvane_event_begin", "ruvane_event_chunk", "ruvane_action_begin", "ruvane_action_chunk", "ruvane_action_complete", "ruvane_page_begin", "ruvane_page_chunk", "ruvane_page_mount", "ruvane_application_revision", "ruvane_page_validate", "ruvane_boundary_mount"];
export const ACTION_MEDIA_TYPE = "application/vnd.ruvane.action";
export const NAVIGATION_MEDIA_TYPE = "application/vnd.ruvane.page";
export const NAVIGATION_STREAM_MEDIA_TYPE = "application/vnd.ruvane.page-stream";
/** Validates a complete origin-relative target, without URL normalization. */
export function internalTarget(target) {
  if (typeof target !== "string" || target.length > 4096 || !target.startsWith("/") || /[^A-Za-z0-9\-._~!$&'()*+,;=:@/%?]/.test(target)) throw invalid();
  const index = target.indexOf("?"), path = index < 0 ? target : target.slice(0, index), query = index < 0 ? "" : target.slice(index + 1);
  if (path.includes("//")) throw invalid();
  const decode = part => { let value; try { value = decodeURIComponent(part); } catch { throw invalid(); } if (new TextEncoder().encode(value).length > 1024 || /[\u0000-\u001f\u007f-\u009f]/.test(value)) throw invalid(); return value; };
  for (const raw of path.slice(1).split("/")) { const value = decode(raw); if (value === "." || value === ".." || /[/\\]/.test(value)) throw invalid(); }
  const keys = new Set();
  if (query) for (const pair of query.split("&")) {
    const split = pair.indexOf("="), key = decode(split < 0 ? pair : pair.slice(0, split));
    decode(split < 0 ? "" : pair.slice(split + 1));
    if (!key || keys.has(key) || keys.size === 32) throw invalid(); keys.add(key);
  }
  return target;
}
/** Reads routing metadata. Rust validates the complete typed seed and application schema. */
export function decodePage(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.length < 34 || bytes.length > 65536) throw invalid();
  const version = String.fromCharCode(...bytes.slice(0,4)); if (!["RVP1","RVP2","RVP3","RVP4"].includes(version)) throw invalid();
  const data = new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength), decoder = new TextDecoder("utf-8",{fatal:true,ignoreBOM:true}); let pos=4;
  const take = size => { if (size > bytes.length-pos) throw invalid(); const value=bytes.subarray(pos,pos+size);pos+=size;return value; };
  const u32 = () => {take(4);return data.getUint32(pos-4,true);};
  const string = limit => {const size=u32();if(size>limit)throw invalid();return decoder.decode(take(size));};
  const selected = version !== "RVP1" ? take(1)[0] : 0;
  const failed=version==="RVP3"?take(1)[0]:null;
  const pending=version==="RVP4"?take(1)[0]:null;
  take(2);const status=data.getUint16(pos-2,true);if(![200,400,404,500].includes(status))throw invalid();
  const url=internalTarget(string(4096)),title=string(1024);
  if(!title.trim()||/[\u0000-\u001f\u007f-\u009f]/.test(title))throw invalid();
  const componentSeed = () => {
    const component=uint(u32()),kind=u32(),value=u32();
    if(![1,2].includes(kind)||(kind===2&&value>1))throw invalid();
    const seed=take(u32());if(seed.length<7||String.fromCharCode(...seed.slice(0,4))!=="RVA1"||seed[4]!==0)throw invalid();
    return {component,kind,value,seed:seed.slice()};
  };
  const leaf=componentSeed(),layouts=[];
  if(version!=="RVP1"){
    const count=take(1)[0];if(count>6||selected>count)throw invalid();
    if(version==="RVP4"?(status!==200||pending>6||count>pending||leaf.seed.length!==7||leaf.seed[5]!==0||leaf.seed[6]!==0):version==="RVP2"?(count<1||status!==200):(status===200||failed>6||count>failed))throw invalid();
    for(let n=0;n<count;n++){
      const name=string(48),key=internalTarget(string(4096));
      if(!/^[A-Za-z][A-Za-z0-9_-]*$/.test(name)||layouts.some(layout=>layout.name===name))throw invalid();
      layouts.push(Object.freeze({name,key,...componentSeed()}));
    }
  }
  if(pos!==bytes.length)throw invalid();
  return Object.freeze({url,title,status,component:leaf.component,kind:leaf.kind,value:leaf.value,selected,failed,pending,layouts:Object.freeze(layouts)});
}
/** Decodes an inert initial-page envelope. Rust still checks ownership and schemas. */
export function decodeNavigationSnapshot(bytes) {
  if(!(bytes instanceof Uint8Array)||bytes.length<16||bytes.length>65552||String.fromCharCode(...bytes.slice(0,4))!=="RVI1")throw invalid();
  const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
  if(view.getUint32(12,true)!==bytes.length-16)throw invalid();
  const packet=bytes.slice(16),page=decodePage(packet);
  if(page.pending!==null||page.selected!==0)throw invalid();
  return Object.freeze({revision:view.getBigUint64(4,true).toString(),bytes:packet,page});
}
function actionPath(path) {
  if (typeof path !== "string" || path.length > 1024 || !path.startsWith("/") || path.includes("//") || /[^A-Za-z0-9\-._~!$&'()*+,;=:@/%]/.test(path)) throw invalid();
  for (const part of path.slice(1).split("/")) {
    let decoded; try { decoded = decodeURIComponent(part); } catch { throw invalid(); }
    if (decoded === "." || decoded === ".." || /[/\\\u0000-\u001f\u007f-\u009f]/.test(decoded)) throw invalid();
  }
  return path;
}
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
  const revision = exports.ruvane_application_revision();
  if (typeof revision !== "bigint" || revision < 0n || revision > 0xffffffffffffffffn) throw invalid();
  return exports;
}

/** Decodes a batch before touching the DOM. */
export function decodeCommands(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.length < 8 || bytes.length > LIMITS.bytes) throw invalid();
  if (bytes[0] !== 82 || bytes[1] !== 86 || bytes[2] !== 78 || bytes[3] !== 54) throw invalid();
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
    if (op === 11) {
      const request = uint(u32()), action = uint(u32()), length = u32();
      if (length > 1024 || offset + length > bytes.length) throw invalid();
      const url = actionPath(decoder.decode(bytes.subarray(offset, offset + length))); offset += length;
      const size = u32(); if (size > 16384 || offset + size > bytes.length) throw invalid();
      const body = decoder.decode(bytes.subarray(offset, offset + size)); offset += size;
      if (/[^\x21-\x7e]/.test(body)) throw invalid();
      commands.push({ op, view, request, action, url, body }); continue;
    }
    if (![1, 2, 3, 4, 6, 7, 8, 9, 10, 12, 13].includes(op)) throw invalid();
    const node = uint(u32());
    if (op === 12) {
      const size = u32(); if (size > 4096 || offset + size > bytes.length) throw invalid();
      const url = internalTarget(decoder.decode(bytes.subarray(offset, offset + size))); offset += size;
      commands.push({ op, view, node, url });
    } else if (op === 1) {
      const parent = u32(), tag = u8(), key = u32();
      if (!Object.hasOwn(TAGS, tag)) throw invalid();
      commands.push({ op, view, node, parent, tag, key });
    } else if (op === 2 || op === 8) {
      const epoch = op === 8 ? u32() : null;
      const size = u32();
      if (size > LIMITS.text || offset + size > bytes.length) throw invalid();
      const text = decoder.decode(bytes.subarray(offset, offset + size));
      offset += size;
      commands.push(op === 8 ? { op, view, node, value: text, epoch } : { op, view, node, text });
    } else if (op === 3) {
      commands.push({ op, view, node, event: uint(u32()) });
    } else if (op === 6) {
      commands.push({ op, view, node });
    } else if (op === 7) {
      const before = u32();
      if (before === node) throw invalid();
      commands.push({ op, view, node, before });
    } else {
      const epoch = op === 9 ? u32() : null;
      const disabled = u8();
      if (disabled > 1) throw invalid();
      commands.push(op === 13 ? { op, view, node, prefetch: disabled === 1 } : op === 9 ? { op, view, node, checked: disabled === 1, epoch } : op === 10 ? { op, view, node, invalid: disabled === 1 } : { op, view, node, disabled: disabled === 1 });
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
  } finally { abort.abort(); clearTimeout(timer); }
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
const container = tag => [0, 1, 3, 7, 8, 9, 10].includes(tag);
const field = tag => [11, 12, 13].includes(tag);
const textField = tag => tag === 11 || tag === 13;
const disableable = tag => field(tag) || tag === 6 || tag === 14;
const eventName = tag => tag === 9 ? "submit" : tag === 12 ? "change" : textField(tag) ? "input" : "click";
const eventAttribute = tag => `data-ruvane-${eventName(tag)}`;
class PayloadRejected extends Error { constructor() { super("Invalid Ruvane event payload"); } }
function validText(tag, value) {
  if (typeof value !== "string" || value.length > LIMITS.text || new TextEncoder().encode(value).length > LIMITS.text) return false;
  for (const character of value) {
    const code = character.codePointAt(0);
    if (code >= 0xd800 && code <= 0xdfff) return false;
  }
  return !(tag === 13 ? /[\u0000-\u0008\u000b-\u001f\u007f-\u009f]/ : /[\u0000-\u001f\u007f-\u009f]/).test(value);
}
const leaf = tag => [2, 4, 5, 6, 14, 15].includes(tag);

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
  if (parent === 3) return [4, 5, 6, 15].includes(tag);
  if (parent === 10) return tag === 4 || field(tag);
  if (field(tag)) return false;
  return [0, 1, 8, 9].includes(parent);
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
      let ancestor = command.parent, inForm = false;
      while (ancestor) { inForm ||= nodes.get(ancestor).tag === 9; ancestor = nodes.get(ancestor).parent; }
      if ((command.tag === 9 && inForm) || (command.tag === 14 && !inForm)) throw invalid();
      nodes.set(command.node, { prefetch: null, url: null, value: null, checked: null, invalid: null, epoch: null, seed: null, element: null, key: command.key, tag: command.tag, parent: command.parent, depth: parent.depth + 1, children: [], text: null, event: null, disabled: null });
      fresh.add(command.node); nextNode = command.node; parent.children.push(command.node);
      continue;
    }
    const node = nodes.get(command.node);
    if (!node) throw invalid();
    if (command.op === 2 && leaf(node.tag)) {
      if (typeof command.text !== "string" || command.text.length > LIMITS.text || new TextEncoder().encode(command.text).length > LIMITS.text) throw invalid();
      node.text = command.text;
    } else if (command.op === 12 && node.tag === 15) {
      node.url = internalTarget(command.url);
    } else if (command.op === 13 && node.tag === 15 && typeof command.prefetch === "boolean") {
      node.prefetch = command.prefetch;
    } else if (command.op === 3 && (node.tag === 6 || node.tag === 9 || field(node.tag)) && fresh.has(command.node) && node.event === null) {
      node.event = uint(command.event);
    } else if (command.op === 4 && disableable(node.tag) && typeof command.disabled === "boolean") {
      node.disabled = command.disabled;
    } else if (command.op === 8 && textField(node.tag)) {
      if (!validText(node.tag, command.value)) throw invalid();
      const epoch = uint(command.epoch, true);
      if (node.epoch !== null && epoch < node.epoch) throw invalid();
      node.value = command.value; node.epoch = epoch;
      if (fresh.has(command.node)) node.seed = command.value;
    } else if (command.op === 9 && node.tag === 12 && typeof command.checked === "boolean") {
      const epoch = uint(command.epoch, true);
      if (node.epoch !== null && epoch < node.epoch) throw invalid();
      node.checked = command.checked; node.epoch = epoch;
      if (fresh.has(command.node)) node.seed = command.checked;
    } else if (command.op === 10 && field(node.tag) && typeof command.invalid === "boolean") {
      node.invalid = command.invalid;
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
    if (node.tag === 15 && (node.url === null || node.prefetch === null)) throw invalid();
    if (id && leaf(node.tag) && node.text === null) throw invalid();
    if ((node.tag === 6 || node.tag === 9 || field(node.tag)) && (node.event === null || ++listeners > LIMITS.listeners)) throw invalid();
    if (disableable(node.tag) && node.disabled === null) throw invalid();
    if (field(node.tag) && (node.epoch === null || node.invalid === null || (textField(node.tag) ? node.value === null : node.checked === null))) throw invalid();
    if (node.tag === 10 && (node.children.filter(id => field(nodes.get(id).tag)).length !== 1 || !node.children.some(id => nodes.get(id).tag === 4 && nodes.get(id).text?.trim()))) throw invalid();
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
      if (node.event !== null) attributes[eventAttribute(node.tag)] = String(node.event);
      if (node.tag === 6 || node.tag === 14) attributes.type = node.tag === 6 ? "button" : "submit";
      if (node.tag === 9) attributes.novalidate = "";
      if (node.tag === 15) { attributes.href = node.url; attributes["data-ruvane-nav"] = ""; if (node.prefetch) attributes["data-ruvane-prefetch"] = ""; }
      if (field(node.tag)) {
        attributes["aria-invalid"] = String(node.invalid); attributes["data-ruvane-epoch"] = String(node.epoch);
        if (textField(node.tag)) attributes.maxlength = "1024";
        if (node.tag === 11) { attributes.type = "text"; attributes.value = node.seed; }
        if (node.tag === 12) { attributes.type = "checkbox"; if (node.seed) attributes.checked = ""; }
      }
      if (disableable(node.tag)) {
        const disabled = inert && field(node.tag) && !node.disabled ? actual.disabled : inert || node.disabled;
        if (disabled) attributes.disabled = "";
        if (actual.disabled !== disabled) mismatch();
      }
      if (actual.attributes.length !== Object.keys(attributes).length || Object.entries(attributes).some(([key, value]) => actual.getAttribute(key) !== value)) mismatch();
      node.element = actual;
      if (container(node.tag)) children(node, actual);
      else if (node.tag === 11 || node.tag === 12) { if (actual.childNodes.length) mismatch(); }
      else {
        const expected = node.tag === 13 ? node.seed : node.text;
        if (expected === "") { if (actual.childNodes.length) mismatch(); }
        else if (actual.childNodes.length !== 1 || actual.firstChild.nodeType !== 3 || actual.firstChild.data !== expected) mismatch();
      }
    }
  }
  children(nodes.get(0), root);
}
/** Loads a module and manages mounted or hydrated views. Disposal is final. */
export async function createRuntime(url, options = {}) {
  if (!options || typeof options !== "object" || Object.keys(options).some(key => key !== "actionTimeoutMs" && key !== "applicationRevision")) throw invalid();
  const actionTimeout = options.actionTimeoutMs ?? 10000;
  if (!Number.isInteger(actionTimeout) || actionTimeout < 1 || actionTimeout > 10000) throw invalid();
  const applicationRevision = options.applicationRevision ?? null;
  if (applicationRevision !== null && (typeof applicationRevision !== "string" || !/^(0|[1-9][0-9]{0,19})$/.test(applicationRevision) || BigInt(applicationRevision) > 0xffffffffffffffffn)) throw invalid();
  let exports = await loadModule(url);
  if (applicationRevision !== null && exports.ruvane_application_revision() !== BigInt(applicationRevision)) throw new Error("Ruvane application revision mismatch");
  const views = new Map(), actionObservers = new Set();
  let actionEpoch = 0, uncertainActions = false;
  function actionState() { return Object.freeze({ state: closed ? faulted ? "faulted" : "disposed" : "active", epoch: actionEpoch, uncertain: uncertainActions, pending: [...views.values()].filter(view => view.action).length, subscribers: actionObservers.size }); }
  function actionChanged() {
    actionEpoch = actionEpoch === null || actionEpoch === Number.MAX_SAFE_INTEGER ? null : actionEpoch + 1;
    const state = actionState(); for (const observer of [...actionObservers]) { try { observer(state); } catch { /* Observers do not control delivery. */ } }
  }
  function endAction(operation, uncertain = false) { if (!operation.ended) { operation.ended = true; uncertainActions ||= uncertain; actionChanged(); } }
  let nextRoot = 0, busy = false, closed = false, faulted = false, finalRust = null, hydratingBranch = false;

  function live() { if (closed) throw new Error("Ruvane browser runtime disposed"); }
  function forget(id, preserve = false, disable = true) {
    const view = views.get(id);
    if (!view) return;
    if (view.action) { const operation = view.action; clearTimeout(operation.timer); operation.controller.abort(); view.action = null; endAction(operation,true); }
    view.resolveIdle?.("disposed"); view.idlePromise = null; view.resolveIdle = null;
    for (const [node, listeners] of view.listeners) for (const [type, listener] of listeners) view.nodes.get(node).element.removeEventListener(type, listener);
    view.listeners.clear();
    // A failed hydration leaves the server's content in place.
    for (const node of view.nodes.values()) {
      if (preserve) {
        if (disable && disableable(node.tag) && view.root.contains(node.element)) node.element.disabled = true;
      } else if (node.parent === 0 && node.element?.parentNode === view.root) node.element.remove();
    }
    view.nodes.clear(); view.events.clear(); view.edits.clear(); view.ingress.clear(); CLAIMS.delete(view.root); views.delete(id);
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
    closed = true; exports = null; actionChanged(); actionObservers.clear();
  }
  function commands() {
    const pointer = result(exports.ruvane_commands_ptr()), size = result(exports.ruvane_commands_len());
    // Wasm calls may grow memory, so read its buffer again.
    const memory = exports.memory.buffer;
    if (size < 8 || size > LIMITS.bytes || pointer + size > memory.byteLength) throw invalid();
    return decodeCommands(new Uint8Array(memory, pointer, size).slice());
  }
  function settle(view, outcome) {
    view.lastOutcome = outcome;
    if (!view.action) { view.resolveIdle?.(outcome); view.idlePromise = null; view.resolveIdle = null; }
  }
  function whenIdle(id) {
    const view = views.get(id);
    if (!view || closed) return Promise.resolve("disposed");
    if (!view.action) return Promise.resolve(view.lastOutcome);
    if (!view.idlePromise) view.idlePromise = new Promise(resolve => { view.resolveIdle = resolve; });
    return view.idlePromise;
  }
  function deliver(id, view, operation, bytes, kind) {
    if (closed || views.get(id) !== view || view.action !== operation) return;
    if (busy) { queueMicrotask(() => deliver(id, view, operation, bytes, kind)); return; }
    busy = true; view.action = null; let delivered = false;
    try {
      if (bytes) {
        if (result(exports.ruvane_action_begin(id, operation.request, bytes.length)) !== 1) throw invalid();
        for (let offset = 0; offset < bytes.length; offset += 4) {
          const count = Math.min(4, bytes.length - offset); let word = 0;
          for (let n = 0; n < count; n++) word |= bytes[offset + n] << (8 * n);
          if (result(exports.ruvane_action_chunk(word >>> 0, count)) !== 1) throw invalid();
        }
      }
      const completed = result(exports.ruvane_action_complete(id, operation.request, kind));
      if (![1, 2].includes(completed)) throw invalid();
      if (completed === 2) kind = 3;
      drain(id, "dispatch");
      settle(view, kind ? ["", "network", "timeout", "protocol", "rejected"][kind] : bytes[4] === 1 ? "validation" : "success"); delivered = true;
    } catch { stop(true); }
    finally { busy = false; endAction(operation,!delivered || kind !== 0); }
  }
  class TransportError extends Error { constructor(kind) { super("Ruvane action transport failed"); this.kind = kind; } }
  async function perform(id, view, operation) {
    let bytes = null, kind = 0;
    try {
      const response = await fetch(operation.url, { method: "POST", mode: "same-origin", credentials: "same-origin", redirect: "error", cache: "no-store", headers: { "Content-Type": "application/x-www-form-urlencoded;charset=UTF-8", "Accept": ACTION_MEDIA_TYPE, ...(applicationRevision === null ? {} : { "Ruvane-Application-Revision": applicationRevision }) }, body: operation.body, signal: operation.controller.signal });
      if (![200, 400].includes(response.status)) { await response.body?.cancel(); throw new TransportError(4); }
      if (response.headers.get("content-type")?.trim().toLowerCase() !== ACTION_MEDIA_TYPE || !response.body) { await response.body?.cancel(); throw new TransportError(3); }
      const length = response.headers.get("content-length");
      if (length !== null && (!/^\d+$/.test(length) || Number(length) > 65536)) { await response.body.cancel(); throw new TransportError(3); }
      const reader = response.body.getReader(), parts = []; let total = 0;
      try {
        while (true) {
          const { done, value } = await reader.read(); if (done) break;
          total += value.length; if (total > 65536) { await reader.cancel(); throw new TransportError(3); } parts.push(value);
        }
      } finally { reader.releaseLock(); }
      bytes = new Uint8Array(total); let offset = 0;
      for (const part of parts) { bytes.set(part, offset); offset += part.length; }
      if (bytes.length < 7 || bytes[0] !== 82 || bytes[1] !== 86 || bytes[2] !== 65 || bytes[3] !== 49 || bytes[4] !== (response.status === 400 ? 1 : 0)) throw new TransportError(3);
    } catch (error) { bytes = null; kind = operation.timedOut ? 2 : error instanceof TransportError ? error.kind : 1; }
    finally { clearTimeout(operation.timer); }
    deliver(id, view, operation, bytes, kind);
  }
  function launch(id, view, command) {
    const operation = { ...command, controller: new AbortController(), timer: null, timedOut: false, ended: false };
    view.action = operation; view.lastAction = command.request; actionChanged();
    operation.timer = setTimeout(() => { operation.timedOut = true; operation.controller.abort(); }, actionTimeout);
    void perform(id, view, operation);
  }
  function context(view, id) {
    if (!view.edits.has(id)) view.edits.set(id, { composing: false, pending: null });
    return view.edits.get(id);
  }
  function selection(element) {
    if (!element || !["input", "textarea"].includes(element.localName) || element.type === "checkbox") return null;
    return { start: element.selectionStart, end: element.selectionEnd, direction: element.selectionDirection };
  }
  function restoreSelection(element, saved) {
    if (!saved || saved.start === null || !element.isConnected) return;
    const start = Math.min(saved.start, element.value.length), end = Math.min(saved.end, element.value.length);
    if (element.selectionStart !== start || element.selectionEnd !== end || element.selectionDirection !== saved.direction) element.setSelectionRange(start, end, saved.direction);
  }
  function setValue(element, value) {
    if (element.value === value) return;
    const saved = element === document.activeElement ? selection(element) : null;
    element.value = value; restoreSelection(element, saved);
  }
  function restoreField(view, id) {
    const node = view.nodes.get(id);
    if (!node) return;
    if (textField(node.tag)) setValue(node.element, node.value);
    if (node.tag === 12) node.element.checked = node.checked;
  }
  function input(view, id, nodeId, type, event) {
    const node = view.nodes.get(nodeId);
    if (!node || closed) return;
    const edit = context(view, nodeId);
    if (type === "compositionstart") { edit.composing = true; return; }
    if (type === "input" && (event.isComposing || edit.composing)) { edit.composing = true; return; }
    if (type === "compositionend") {
      edit.composing = false;
      if (edit.pending !== null) { setValue(node.element, edit.pending); edit.pending = null; return; }
    }
    const value = node.tag === 12 ? node.element.checked : node.element.value;
    if (value === (node.tag === 12 ? node.checked : node.value)) return;
    try { dispatchNode(id, nodeId, node.event, value); }
    catch (error) {
      if (error instanceof PayloadRejected) { restoreField(view, nodeId); console.warn("Ruvane input rejected"); }
      else { stop(true); console.error("Ruvane browser runtime stopped"); }
    }
  }
  function adopt(view, id, edits) {
    view.ingress = new Set(edits.map(([key]) => key));
    try {
      for (const [key, value] of edits) {
        view.ingress.delete(key); const node = view.nodes.get(key); if (!node) continue;
        try { dispatchNode(id, key, node.event, value); }
        catch (error) { if (error instanceof PayloadRejected) restoreField(view, key); throw error; }
      }
    } finally { view.ingress.clear(); }
  }
  function submit(view, id, nodeId) {
    if ([...view.edits.values()].some(edit => edit.composing)) return;
    const edits = [];
    for (const [key, node] of view.nodes) {
      if (!field(node.tag) || node.disabled) continue;
      const value = node.tag === 12 ? node.element.checked : node.element.value;
      if (value !== (node.tag === 12 ? node.checked : node.value)) edits.push([key, value]);
    }
    adopt(view, id, edits);
    const node = view.nodes.get(nodeId);
    if (node) dispatchNode(id, nodeId, node.event);
  }
  function bind(view, id, nodeId, node) {
    const types = textField(node.tag) ? ["input", "compositionstart", "compositionend"] : [eventName(node.tag)];
    const listeners = types.map(type => {
      const listener = event => {
        if (closed || views.get(id)?.nodes.get(nodeId)?.element !== node.element) return;
        if (node.tag === 9) event.preventDefault();
        if (field(node.tag)) { input(view, id, nodeId, type, event); return; }
        try { if (node.tag === 9) submit(view, id, nodeId); else dispatchNode(id, nodeId, node.event); }
        catch (error) {
          if (error instanceof PayloadRejected) console.warn("Ruvane input rejected");
          else { stop(true); console.error("Ruvane browser runtime stopped"); }
        }
      };
      node.element.addEventListener(type, listener); return [type, listener];
    });
    view.listeners.set(nodeId, listeners); view.events.add(node.event);
  }
  function createElement(nodes, command) {
    const node = nodes.get(command.node);
    node.element = document.createElement(TAGS[node.tag]);
    node.element.setAttribute("data-ruvane-node", String(command.node));
    if (node.key) node.element.setAttribute("data-ruvane-key", String(node.key));
    if (node.tag === 6 || node.tag === 14) node.element.type = node.tag === 6 ? "button" : "submit";
    if (node.tag === 9) node.element.noValidate = true;
    if (textField(node.tag)) { node.element.maxLength = LIMITS.text; if (node.tag === 11) node.element.type = "text"; node.element.defaultValue = node.seed; }
    if (node.tag === 12) { node.element.type = "checkbox"; node.element.defaultChecked = node.seed; }
    if (disableable(node.tag)) node.element.disabled = true;
    nodes.get(node.parent).element.appendChild(node.element);
  }
  function order(nodes, focused) {
    for (const parent of nodes.values()) {
      if (!parent.children.length) continue;
      const children = parent.children.map(id => nodes.get(id).element);
      const anchor = children.find(element => focused && element.contains(focused));
      const place = (elements, before) => {
        for (const element of [...elements].reverse()) {
          if (element.nextSibling !== before) parent.element.insertBefore(element, before);
          before = element;
        }
      };
      if (anchor) { const index = children.indexOf(anchor); place(children.slice(0, index), anchor); place(children.slice(index + 1), null); }
      else place(children, null);
    }
  }
  function drain(id, phase, decoded = commands()) {
    const view = views.get(id);
    const actions = decoded.filter(command => command.op === 11), batch = decoded.filter(command => command.op !== 11);
    if(hydratingBranch&&actions.length)throw new HydrationMismatch();
    if (actions.length > 1 || (phase === "dispose" && actions.length) || actions.some(command => command.view !== id || command.request <= view.lastAction || view.action)) throw invalid();
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
    const focused = view.root.contains(document.activeElement) ? document.activeElement : null, saved = selection(focused);
    for (const [nodeId, listeners] of view.listeners) {
      if (!nodes.has(nodeId)) { for (const [type, listener] of listeners) old.get(nodeId).element.removeEventListener(type, listener); view.listeners.delete(nodeId); view.edits.delete(nodeId); }
    }
    view.nodes = nodes; view.nextNode = plan.nextNode;
    if (phase !== "hydrate") {
      for (const command of batch) {
        if (command.op === 6) { old.get(command.node).element.remove(); continue; }
        if (command.op === 1) { createElement(nodes, command); continue; }
        const node = nodes.get(command.node);
        if (command.op === 13) { if (node.prefetch) node.element.setAttribute("data-ruvane-prefetch", ""); else node.element.removeAttribute("data-ruvane-prefetch"); }
        if (command.op === 12) { node.element.setAttribute("href", node.url); node.element.setAttribute("data-ruvane-nav", ""); }
        if (command.op === 2) node.element.textContent = command.text;
        else if (command.op === 3) node.element.setAttribute(eventAttribute(node.tag), String(command.event));
      }
      order(nodes, focused);
    }
    view.events.clear();
    const edits = [];
    for (const [nodeId, node] of nodes) {
      if (disableable(node.tag)) node.element.disabled = node.disabled;
      if (node.event !== null && node.event !== undefined) view.events.add(node.event);
      if (!field(node.tag)) continue;
      node.element.setAttribute("aria-invalid", String(node.invalid)); node.element.setAttribute("data-ruvane-epoch", String(node.epoch));
      const edit = context(view, nodeId), previous = old.get(nodeId);
      if (phase === "hydrate" && !node.disabled) {
        const value = node.tag === 12 ? node.element.checked : node.element.value;
        if (value !== (node.tag === 12 ? node.checked : node.value)) edits.push([nodeId, value]);
      } else if (view.ingress.has(nodeId)) { continue; }
      else if (textField(node.tag)) {
        if (edit.composing) {
          if (previous && (previous.value !== node.value || previous.epoch !== node.epoch)) edit.pending = node.value;
        } else setValue(node.element, node.value);
      } else node.element.checked = node.checked;
    }
    matchDom(nodes, view.root, false);
    for (const [nodeId, node] of nodes) {
      if (node.event !== null && node.event !== undefined && !view.listeners.has(nodeId)) bind(view, id, nodeId, node);
    }
    view.hydrationEdits = edits;
    if (focused && view.root.contains(focused) && !focused.disabled) {
      if (document.activeElement !== focused) focused.focus({ preventScroll: true });
      const active = [...nodes].find(([, node]) => node.element === focused);
      if (!active || !view.edits.get(active[0])?.composing) restoreSelection(focused, saved);
    }
    if (actions.length) launch(id, view, actions[0]);
  }
  function stagePage(page) {
    if (result(exports.ruvane_page_begin(page.length)) !== 1) throw invalid();
    for (let offset = 0; offset < page.length; offset += 4) {
      const count = Math.min(4, page.length - offset); let word = 0;
      for (let index = 0; index < count; index++) word |= page[offset + index] << (index * 8);
      if (result(exports.ruvane_page_chunk(word >>> 0, count)) !== 1) throw invalid();
    }
  }
  function validatePage(bytes) {
    live(); decodePage(bytes); if (busy) throw new Error("Ruvane bridge busy"); busy = true;
    try {
      stagePage(bytes);
      if (result(exports.ruvane_page_validate()) !== 1) {
        if ([2,6,7,8,9,11].includes(result(exports.ruvane_last_error()))) throw new MountRejected();
        throw invalid();
      }
      if (commands().length !== 0) throw invalid();
      return true;
    } catch (error) {
      if (error instanceof MountRejected) { if (result(exports.ruvane_commands_clear()) !== 1) stop(true); }
      else stop(true);
      throw error;
    } finally { busy = false; }
  }
  function start(root, props, hydrated, page = null, boundary = false, prepare = false) {
    live(); host(root);
    if (!hydrated && root.childNodes.length) throw new Error("Ruvane mount requires an empty root");
    if (views.size === LIMITS.views || nextRoot === 0xffffffff) throw new RangeError("Ruvane view limit exceeded");
    if (busy) throw new Error("Ruvane bridge busy");
    busy = true;
    let id = 0;
    try {
      if (page) {
        stagePage(page);
        id = result(exports.ruvane_page_mount(++nextRoot));
      } else id = result((boundary ? exports.ruvane_boundary_mount : exports.ruvane_mount)(++nextRoot, props.component, props.kind, props.value));
      if (!id) {
        if ((boundary ? [1,2,6,7,8,9,11] : [1, 2, 7, 11]).includes(result(exports.ruvane_last_error()))) throw new MountRejected();
        throw invalid();
      }
      if (views.has(id)) throw invalid();
      views.set(id, { root, hydrated, nodes: new Map([[0, { element: root, tag: 0, depth: 0, children: [], key: 0 }]]), nextNode: 0, listeners: new Map(), events: new Set(), edits: new Map(), ingress: new Set(), hydrationEdits: [], action: null, lastAction: 0, lastOutcome: "success", idlePromise: null, resolveIdle: null });
      CLAIMS.add(root);
      if(prepare){
        const decoded=commands(),view=views.get(id);
        if(decoded.some(command=>command.op===11))throw new HydrationMismatch();
        const plan=planCommands(decoded,id,view,true);matchDom(plan.nodes,root,true);
        for(const node of plan.nodes.values())if(field(node.tag)){const value=node.tag===12?node.element.checked:node.element.value;if(node.tag!==12&&!validText(node.tag,value))throw new HydrationMismatch();}
        if(result(exports.ruvane_commands_clear())!==1)throw invalid();
        view.prepared=decoded;
      }else drain(id, hydrated ? "hydrate" : "mount");
      return Object.freeze({ id, dispatch: (event, value) => dispatch(id, event, value), whenIdle: () => whenIdle(id), dispose: () => disposeView(id) });
    } catch (error) {
      if (error instanceof HydrationMismatch || error instanceof MountRejected) {
        try { if (id) { releaseNative(id); forget(id, true); } }
        catch { stop(true); }
      } else stop(true);
      throw error;
    } finally { busy = false; }
  }
  function dispatch(id, event, value) {
    live(); uint(id); uint(event);
    const view = views.get(id);
    if (!view) throw new Error("Ruvane view disposed");
    const found = [...view.nodes].find(([, node]) => node.event === event);
    if (!found) throw new RangeError("Unknown Ruvane component event");
    if (found[1].tag === 9 && value === undefined) return submit(view, id, found[0]);
    return dispatchNode(id, found[0], event, value);
  }
  function dispatchNode(id, nodeId, event, value) {
    live(); uint(id); uint(nodeId); uint(event);
    const view = views.get(id), node = view?.nodes.get(nodeId);
    if (!node || node.event !== event) throw new RangeError("Expired Ruvane event binding");
    let kind = 0, scalar = 0, bytes;
    if (textField(node.tag)) { if (!validText(node.tag, value) || node.disabled) throw new PayloadRejected(); kind = 1; bytes = new TextEncoder().encode(value); }
    else if (node.tag === 12) { if (typeof value !== "boolean" || node.disabled) throw new PayloadRejected(); kind = 2; scalar = Number(value); }
    else if (value !== undefined) throw new PayloadRejected();
    if (busy) throw new Error("Ruvane bridge busy");
    busy = true;
    try {
      if (bytes) {
        if (result(exports.ruvane_event_begin(id, nodeId, event, bytes.length)) !== 1) throw new PayloadRejected();
        for (let offset = 0; offset < bytes.length; offset += 4) {
          const count = Math.min(4, bytes.length - offset); let word = 0;
          for (let i = 0; i < count; i++) word |= bytes[offset + i] << (i * 8);
          if (result(exports.ruvane_event_chunk(word >>> 0, count)) !== 1) throw new PayloadRejected();
        }
      }
      if (result(exports.ruvane_dispatch(id, nodeId, event, kind, scalar)) !== 1) {
        if (result(exports.ruvane_last_error()) === 9) throw new PayloadRejected();
        throw invalid();
      }
      drain(id, "dispatch");
    } catch (error) {
      if (error instanceof PayloadRejected || hydratingBranch && error instanceof HydrationMismatch) { if (result(exports.ruvane_commands_clear()) !== 1) stop(true); }
      else stop(true);
      throw error;
    } finally { busy = false; }
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
  function preparePageHydration(roots,bytes) {
    live();const page=decodePage(bytes);
    if(page.pending!==null||page.selected!==0||!Array.isArray(roots)||roots.length!==page.layouts.length+1||views.size+roots.length>LIMITS.views)throw new MountRejected();
    const controllers=[],undo=[];let activated=false,finished=false;
    const focused=document.activeElement,savedSelection=selection(focused),point={x:scrollX,y:scrollY};
    function remember(root){
      const records=[];
      function visit(node){
        if(node.nodeType===3){records.push({node,text:node.data});return;}
        const item={node,children:[...node.childNodes],attributes:[...node.attributes].map(a=>[a.name,a.value])};
        if(node.localName==="input"||node.localName==="textarea"){item.value=node.value;item.checked=node.checked;}
        records.push(item);for(const child of item.children)visit(child);
      }
      visit(root);
      return()=>{for(const item of records){if("text"in item){item.node.data=item.text;continue;}item.node.replaceChildren(...item.children);for(const a of [...item.node.attributes])item.node.removeAttribute(a.name);for(const [name,value]of item.attributes)item.node.setAttribute(name,value);if("value"in item){item.node.value=item.value;if(item.checked!==undefined)item.node.checked=item.checked;}}};
    }
    function rollback(){
      if(finished)return false;finished=true;
      for(const controller of [...controllers].reverse()){
        if(!views.has(controller.id))continue;
        try{releaseNative(controller.id);forget(controller.id,true,false);}catch{stop(true);}
      }
      for(const restore of undo)restore();
      if(focused?.isConnected){focused.focus({preventScroll:true});restoreSelection(focused,savedSelection);}
      window.scrollTo(point.x,point.y);undo.length=0;controllers.length=0;return true;
    }
    try{
      for(let index=0;index<roots.length;index++){
        const root=roots[index],expected=page.layouts[index]??page,props=readProps(root);
        if(root.localName!=="div"||root.attributes.length!==3||props.component!==expected.component||props.kind!==expected.kind||props.value!==expected.value)throw new HydrationMismatch();
        const selected=bytes.slice();if(index<page.layouts.length)selected[4]=index+1;
        controllers.push(start(root,null,true,selected,false,true));
      }
      for(const root of roots)undo.push(remember(root));
    }catch(error){rollback();throw error;}
    return Object.freeze({
      activate(){
        if(finished||activated||closed||busy)throw invalid();activated=true;hydratingBranch=true;
        try{
          for(const controller of controllers){
            const view=views.get(controller.id),decoded=view.prepared;delete view.prepared;
            busy=true;try{drain(controller.id,"hydrate",decoded);}finally{busy=false;}
            const edits=view.hydrationEdits;view.hydrationEdits=[];
            adopt(view,controller.id,edits);
          }
          return Object.freeze([...controllers]);
        }catch(error){rollback();throw error;}
        finally{hydratingBranch=false;busy=false;}
      },
      commit(){if(finished||!activated)throw invalid();finished=true;undo.length=0;controllers.length=0;},
      rollback,
    });
  }
  return Object.freeze({
    applicationRevision,
    validatePage,
    preparePageHydration,
    actionState,
    subscribeActions(observer) {
      live(); if (typeof observer !== "function" || actionObservers.size >= 16) throw new Error("Invalid Ruvane action observer");
      actionObservers.add(observer); return () => actionObservers.delete(observer);
    },
    mountPage(root, bytes) { const page=decodePage(bytes); if(page.selected!==0||page.pending!==null)throw invalid(); return start(root, null, false, bytes); },
    mountPageSegment(root, bytes, index) {
      const page=decodePage(bytes);if(page.selected!==0||!Number.isInteger(index)||index<1||index>page.layouts.length)throw invalid();
      const selected=bytes.slice();selected[4]=index;return start(root,null,false,selected);
    },
    mount(root, props) { return start(root, validateProps(props), false); },
    mountBoundary(root,props) {return start(root,validateProps(props),false,null,true);},
    hydrate(root) {
      live(); host(root); const controller = start(root, readProps(root), true), view = views.get(controller.id);
      const edits = view.hydrationEdits; view.hydrationEdits = [];
      const valid = [];
      for (const [id, value] of edits) {
        const node = view.nodes.get(id);
        if (node && (node.tag === 12 ? typeof value === "boolean" : validText(node.tag, value))) valid.push([id, value]);
        else restoreField(view, id);
      }
      adopt(view, controller.id, valid);
      return controller;
    },
    stats() {
      let domHandles = 0, eventListeners = 0;
      for (const view of views.values()) { domHandles += Math.max(0, view.nodes.size - 1); eventListeners += [...view.listeners.values()].reduce((sum, listeners) => sum + listeners.length, 0); }
      const native = closed ? finalRust : nativeStats();
      const pendingActions = [...views.values()].filter(view => view.action).length;
      return Object.freeze({ state: closed ? (faulted ? "faulted" : "disposed") : "active", views: views.size, domHandles, eventListeners, pendingActions,
        rustInstances: native?.rustInstances ?? null, rustSignals: native?.rustSignals ?? null, rustEffects: native?.rustEffects ?? null });
    },
    dispose() { if (busy) throw new Error("Ruvane bridge busy"); stop(false); },
  });
}

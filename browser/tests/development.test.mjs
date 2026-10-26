import test from "node:test";
import assert from "node:assert/strict";
import { parseStatus, changedFields, reloadSafety, readStatus, startDevelopment } from "../../web/development.mjs";

const wire = (state = "ready", revision = "13", sequence = "1", message = "") => `RVD1\n9\n${sequence}\n${state}\n${revision}\n\n${message}`;
const active = () => ({ ready: true, actions: { state: "active", pending: 0, uncertain: false }, navigationPending: false });
test("development status uses exact u64 identifiers and bounded UTF-8 diagnostics", () => {
  assert.equal(parseStatus(wire("ready", "18446744073709551615"), "9").revision, "18446744073709551615");
  assert.equal(parseStatus(wire("error", "0", "2", "<script> 🦀"), "9").message, "<script> 🦀");
  for (const text of [wire().replace("\n9\n", "\n8\n"), wire("ready", "0"), wire("other"), wire("ready", "18446744073709551616"), wire("ready", "01"), wire("ready", "1", "0"), wire("error", "0", "1", "\x1b[31m"), wire("error", "0", "1", "é".repeat(32768))]) assert.throws(() => parseStatus(text, "9"));
});
test("response reading checks MIME, size, UTF-8 and cancels oversized streams", async () => {
  assert.equal(await readStatus(new Response(wire(), { headers: { "Content-Type": "text/plain; charset=utf-8" } })), wire());
  for (const response of [new Response("x", { status: 403 }), new Response("x", { headers: { "Content-Type": "text/html" } }), new Response("x", { headers: { "Content-Type": "text/plain; charset=utf-8", "Content-Length": "32769" } }), new Response(new Uint8Array([255]), { headers: { "Content-Type": "text/plain; charset=utf-8" } })]) await assert.rejects(readStatus(response));
  let cancelled = false;
  const stream = new ReadableStream({ pull(controller) { controller.enqueue(new Uint8Array(32769)); }, cancel() { cancelled = true; } });
  await assert.rejects(readStatus(new Response(stream, { headers: { "Content-Type": "text/plain; charset=utf-8" } })));
  assert.equal(cancelled, true);
});
test("draft detection covers text, checks, files and selects without recording values", () => {
  const text = { tagName: "INPUT", type: "text", value: "", defaultValue: "" }, check = { tagName: "INPUT", type: "checkbox", checked: false, defaultChecked: false }, select = { tagName: "SELECT", options: [{ selected: true, defaultSelected: true }] }, file = { tagName: "INPUT", type: "file", files: [] };
  assert.equal(changedFields([text, check, select, file]), false);
  for (const field of [{ ...text, value: "draft" }, { ...check, checked: true }, { ...check, type: "radio", checked: true }, { ...select, options: [{ selected: true, defaultSelected: false }] }, { ...file, files: [{}] }, { ...text, tagName: "TEXTAREA", value: "draft" }]) assert.equal(changedFields([field]), true);
});
test("automatic reload waits for hydration, navigation, actions, IME and visibility", () => {
  assert.equal(reloadSafety(active()), "safe");
  for (const [state, options, result] of [[null, {}, "unavailable"], [{ ...active(), ready: false }, {}, "unavailable"], [{ ...active(), navigationPending: true }, {}, "unavailable"], [{ ...active(), actions: { state: "active", pending: 1 } }, {}, "pending"], [{ ...active(), actions: { state: "active", uncertain: true } }, {}, "uncertain"], [active(), { nativeSubmit: true }, "pending"], [active(), { composing: true }, "edited"], [active(), { dirty: true }, "edited"], [active(), { hidden: true }, "hidden"]]) assert.equal(reloadSafety(state, options), result);
});

class Target {
  constructor() { this.listeners = new Map(); }
  addEventListener(name, fn) { if (!this.listeners.has(name)) this.listeners.set(name, new Set()); this.listeners.get(name).add(fn); }
  removeEventListener(name, fn) { this.listeners.get(name)?.delete(fn); }
  emit(name, event = {}) { for (const fn of [...(this.listeners.get(name) ?? [])]) fn(event); }
  listenerCount() { return [...this.listeners.values()].reduce((sum, set) => sum + set.size, 0); }
}
class Element extends Target {
  constructor(tag) { super(); this.tagName = tag.toUpperCase(); this.children = []; this.attributes = new Map(); this.hidden = false; this.textContent = ""; }
  append(...children) { for (const child of children) { child.parent = this; this.children.push(child); } }
  remove() { if (this.parent) this.parent.children = this.parent.children.filter(child => child !== this); }
  setAttribute(name, value) { this.attributes.set(name, value); }
  getAttribute(name) { return this.attributes.get(name) ?? null; }
  set innerHTML(_) { throw new Error("Executable markup is forbidden"); }
}
const flush = async () => { for (let n = 0; n < 12; n++) await new Promise(resolve => setImmediate(resolve)); };
async function fixture({ placeholder = false, state = active(), dirty = false, status = wire(), delay = false } = {}) {
  const document = new Target(), window = new Target(), timers = new Map(), fields = dirty ? [{ type: "text", tagName: "INPUT", value: "draft", defaultValue: "" }] : [];
  const marker = new Element("script"), root = new Element("div");
  marker.dataset = { session: "9", revision: placeholder ? "0" : "12" }; marker.setAttribute("src", "/_ruvane/dev/client.mjs");
  root.setAttribute("data-ruvane-revision", "12"); root.setAttribute("data-ruvane-module", "/ruvane-runtime/000000000000000c/app.wasm");
  document.head = new Element("head"); document.body = new Element("body"); document.hidden = false;
  document.createElement = tag => new Element(tag);
  document.querySelectorAll = query => query === "#ruvane-development" ? [marker] : query === "input,textarea,select" ? fields : [];
  document.getElementById = id => id === "ruvane-application" ? root : null;
  let timerId = 0, reloads = 0, calls = 0, pendingResponse = null;
  window.setTimeout = (fn, ms) => { const id = ++timerId; timers.set(id, { fn, ms }); return id; };
  window.clearTimeout = id => timers.delete(id); window.location = { reload() { reloads++; } };
  const request = async (_url, options) => {
    calls++; assert.equal(options.headers["Ruvane-Development"], "9"); assert.equal(options.mode, "same-origin"); assert.equal(options.cache, "no-store"); assert.equal(options.redirect, "error"); assert.equal(options.method, undefined);
    if (delay) await new Promise((resolve, reject) => { pendingResponse = resolve; options.signal.addEventListener("abort", () => reject(new Error("Aborted"))); });
    if (status instanceof Error) throw status;
    return new Response(status, { status: typeof status === "number" ? status : 200, headers: { "Content-Type": "text/plain; charset=utf-8" } });
  };
  const client = startDevelopment({ document, window, fetch: request, loadApplication: async () => ({ applicationState: () => state }) });
  await flush();
  const panel = document.body.children[0], [title, message, diagnostics, button] = panel.children;
  return { document, window, client, panel, title, message, diagnostics, button, fields, get reloads() { return reloads; }, get calls() { return calls; }, get timers() { return timers; }, setState(value) { state = value; }, setStatus(value) { status = value; }, resolve() { pendingResponse?.(); }, async tick() { const next = [...timers].sort((a,b) => a[1].ms - b[1].ms)[0]; if (next) { timers.delete(next[0]); next[1].fn(); await flush(); } } };
}
test("a clean ready document and first-build placeholder reload once", async () => {
  for (const placeholder of [false, true]) { const f = await fixture({ placeholder }); assert.equal(f.reloads, 1); await f.tick(); assert.equal(f.reloads, 1); f.client.stop(); }
});
test("drafts keep the document and explicit reload requires a user click", async () => {
  const f = await fixture({ dirty: true }); assert.equal(f.reloads, 0); assert.equal(f.title.textContent, "New version available"); assert.equal(f.fields[0].value, "draft"); assert.equal(f.button.disabled, false); f.button.emit("click"); assert.equal(f.reloads, 1); f.client.stop();
});
test("pending actions disable reload and uncertain outcomes require explicit reload", async () => {
  const f = await fixture({ state: { ...active(), actions: { state: "active", pending: 1, uncertain: false } } });
  assert.equal(f.reloads, 0); assert.equal(f.button.disabled, true); f.button.emit("click"); assert.equal(f.reloads, 0);
  f.setState({ ...active(), actions: { state: "active", pending: 0, uncertain: true } }); await f.tick(); assert.equal(f.reloads, 0); assert.match(f.message.textContent, /does not replay/); f.button.emit("click"); assert.equal(f.reloads, 1); assert.equal(f.calls, 2); f.client.stop();
});
test("composition and native submissions block a later ready version", async () => {
  const f = await fixture({ status: wire("building", "12") });
  f.document.emit("compositionstart"); f.setStatus(wire("ready", "13", "2")); await f.tick(); assert.equal(f.reloads, 0);
  f.document.emit("submit", { defaultPrevented: false }); f.document.emit("compositionend"); assert.equal(f.button.disabled, true); f.button.emit("click"); assert.equal(f.reloads, 0); f.client.stop();
  const g = await fixture({ status: wire("building", "12") }); g.document.emit("submit", { defaultPrevented: true }); g.setStatus(wire("ready", "13", "2")); await g.tick(); assert.equal(g.reloads, 1); g.client.stop();
});
test("compiler errors are plain text and preserve the previous app", async () => {
  const f = await fixture({ status: wire("error", "12", "1", "error: <img src=x onerror=alert(1)> 🦀") });
  assert.equal(f.reloads, 0); assert.equal(f.title.textContent, "Build failed"); assert.equal(f.diagnostics.textContent, "error: <img src=x onerror=alert(1)> 🦀"); assert.equal(f.button.hidden, true);
  f.setStatus(wire("ready", "13", "2")); await f.tick(); assert.equal(f.reloads, 1); f.client.stop();
});
test("malformed, foreign, stale and contradictory status never triggers reload", async () => {
  for (const bad of ["garbage", wire().replace("\n9\n", "\n8\n"), 409]) { const f = await fixture({ status: bad }); assert.equal(f.reloads, 0); if (bad === 409) { assert.equal(f.title.textContent, "Development session changed"); assert.equal(f.timers.size, 0); } f.client.stop(); }
  const f = await fixture({ status: wire("ready", "12", "3") });
  for (const bad of [wire("ready", "13", "2"), wire("ready", "13", "3")]) { f.setStatus(bad); await f.tick(); assert.equal(f.reloads, 0); }
  f.client.stop();
});
test("hidden pages pause polling and pagehide aborts requests and removes listeners", async () => {
  const f = await fixture({ status: wire("building", "12") });
  f.document.hidden = true; f.document.emit("visibilitychange"); assert.equal(f.timers.size, 0);
  f.setStatus(wire("ready", "13", "2")); f.document.hidden = false; f.document.emit("visibilitychange"); await flush(); assert.equal(f.reloads, 1);
  f.window.emit("pagehide"); assert.equal(f.document.listenerCount(), 0); assert.equal(f.window.listenerCount(), 0); assert.equal(f.document.body.children.length, 0);
  const g = await fixture({ delay: true }); g.window.emit("pagehide"); await flush(); assert.equal(g.reloads, 0); assert.equal(g.timers.size, 0); assert.equal(g.calls, 1);
});
test("connection loss backs off without overlapping requests or replacing diagnostics", async () => {
  const f = await fixture({ status: wire("error", "12", "1", "compiler failure") });
  f.setStatus(new Error("offline")); for (let n = 0; n < 5; n++) await f.tick();
  assert.equal(f.diagnostics.textContent, "compiler failure"); assert.equal(f.reloads, 0); assert.equal(f.timers.size, 1); assert.equal([...f.timers.values()][0].ms, 8000); f.client.stop();
});

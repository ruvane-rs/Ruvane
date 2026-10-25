import { decodePage, internalTarget, NAVIGATION_MEDIA_TYPE } from "./bridge.mjs";

const OWNERS = new WeakSet(), KEY = "__ruvane_navigation_v1";
const failure = () => new Error("Ruvane navigation failed");
const position = value => Number.isFinite(value) && value >= 0 && value <= 100000000 ? value : 0;
const urlTarget = url => url.pathname + url.search + (!url.search && url.href.split("#")[0].endsWith("?") ? "?" : "");
const target = () => urlTarget(location);
function stateObject(value) {
  if (value === null) return {};
  if (typeof value !== "object" || Array.isArray(value) || Object.getPrototypeOf(value) !== Object.prototype) throw failure();
  return value;
}
function entry(url, x, y) { return { version: 1, url, x: position(x), y: position(y) }; }
function saved(value, url) {
  const item = value?.[KEY];
  return item?.version === 1 && item.url === url ? { x: position(item.x), y: position(item.y) } : null;
}
async function pageBytes(response, signal) {
  if (![200, 400, 404].includes(response.status) || response.headers.get("content-type")?.split(";")[0].trim().toLowerCase() !== NAVIGATION_MEDIA_TYPE || !response.body) throw failure();
  const length = response.headers.get("content-length");
  if (length !== null && (!/^\d+$/.test(length) || Number(length) > 65536)) throw failure();
  const reader = response.body.getReader(), parts = []; let total = 0;
  try {
    while (true) {
      const { done, value } = await reader.read(); if (signal.aborted) throw failure(); if (done) break;
      total += value.length; if (total > 65536) { await reader.cancel(); throw failure(); } parts.push(value);
    }
  } finally { reader.releaseLock(); }
  const bytes = new Uint8Array(total); let offset = 0;
  for (const part of parts) { bytes.set(part, offset); offset += part.length; }
  return bytes;
}

/** Replaces only the page outlet after a typed page has been prepared successfully. */
export function createNavigator(runtime, { outlet, scope = document, status, timeoutMs = 10000 } = {}) {
  if (!runtime || typeof runtime.mountPage !== "function" || !(outlet instanceof HTMLElement) || outlet.ownerDocument !== document || !(scope === document || scope instanceof HTMLElement && scope.ownerDocument === document) || !scope.contains(outlet) || OWNERS.has(window) || !(status instanceof HTMLElement) || status.ownerDocument !== document || outlet.contains(status) || !Number.isInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 10000 || !["http:", "https:"].includes(location.protocol)) throw failure();
  stateObject(history.state);
  const previousRestoration = history.scrollRestoration;
  let current = null, activeURL = target(), pending = null, disposed = false, started = false, frame = 0, idle = Promise.resolve("idle");
  OWNERS.add(window);
  const previousRole = status.getAttribute("role"), previousLive = status.getAttribute("aria-live");
  status.setAttribute("role", "status"); status.setAttribute("aria-live", "polite");
  function save() {
    if (!disposed && started && activeURL === target()) history.replaceState({ ...stateObject(history.state), [KEY]: entry(target() + location.hash, scrollX, scrollY) }, "", location.href);
  }
  function scroll() {
    if (!started || disposed || frame || pending?.mode === "pop") return;
    frame = requestAnimationFrame(() => { frame = 0; try { save(); } catch { /* History storage can be full. */ } });
  }
  function restore(point) { window.scrollTo(position(point.x), position(point.y)); }
  function fallback(url) { if (!disposed) location.replace(url); }
  function run(url, mode, point = { x: 0, y: 0 }, accept) {
    if (disposed) return Promise.reject(new Error("Ruvane navigator disposed"));
    internalTarget(url);
    const absolute = new URL(url, location.href);
    if (absolute.origin !== location.origin || urlTarget(absolute) !== url) return Promise.reject(failure());
    if (mode !== "start" && !started) return Promise.reject(failure());
    pending?.abort.abort();
    const operation = { mode, abort: new AbortController(), expired: false, updated: false }; pending = operation;
    status.textContent = "Loading…"; outlet.setAttribute("aria-busy", "true");
    const timer = setTimeout(() => { operation.expired = true; operation.abort.abort(); }, timeoutMs);
    idle = (async () => {
      let prepared = null, committing = false;
      try {
        const response = await fetch(absolute.href, { method: "GET", mode: "same-origin", credentials: "same-origin", redirect: "error", headers: { Accept: NAVIGATION_MEDIA_TYPE, ...(runtime.applicationRevision ? { "Ruvane-Application-Revision": runtime.applicationRevision } : {}) }, signal: operation.abort.signal, cache: "no-store" });
        operation.updated = response.status === 409;
        const bytes = await pageBytes(response, operation.abort.signal), page = decodePage(bytes);
        if (page.url !== url || page.status !== response.status || response.redirected) throw failure();
        if (disposed || pending !== operation) return disposed ? "disposed" : "superseded";
        const host = document.createElement("div"); host.tabIndex = -1;
        prepared = runtime.mountPage(host, bytes);
        if (mode === "start" && accept && !accept()) { prepared.dispose(); prepared = null; status.textContent = "Native forms are ready."; return "native"; }
        committing = true;
        if (mode === "push") {
          save(); history.pushState({ [KEY]: entry(url, 0, 0) }, "", url);
        } else if (mode === "start" || mode === "replace") {
          history.replaceState({ ...stateObject(history.state), [KEY]: entry(mode === "start" ? url + location.hash : url, point.x, point.y) }, "", mode === "start" ? location.href : url);
        }
        current?.dispose(); current = prepared; prepared = null;
        outlet.replaceChildren(host); activeURL = url; started = true; history.scrollRestoration = "manual";
        document.title = page.title; status.textContent = page.status === 200 ? "Ready." : `Page ${page.status}.`;
        if (mode !== "start") host.focus({ preventScroll: true });
        restore(point); return "success";
      } catch {
        try { prepared?.dispose(); } catch { fallback(url); }
        if (disposed || pending !== operation) return disposed ? "disposed" : "superseded";
        status.textContent = operation.updated ? "Application updated. Reload to continue." : operation.expired ? "Navigation timed out. Try the link again." : "Navigation failed. Try the link again or reload.";
        if (committing || mode === "pop" || runtime.stats().state !== "active" || activeURL !== target()) fallback(url);
        return "failed";
      } finally {
        operation.abort.abort(); clearTimeout(timer);
        if (pending === operation) { pending = null; outlet.removeAttribute("aria-busy"); }
      }
    })();
    return idle;
  }
  function click(event) {
    if (!started || disposed || event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    const anchor = event.target instanceof Element ? event.target.closest("a[data-ruvane-nav]") : null;
    if (!anchor || !scope.contains(anchor) || anchor.hasAttribute("target") || anchor.hasAttribute("download") || anchor.hasAttribute("ping") || anchor.relList.contains("external")) return;
    const href = anchor.getAttribute("href");
    try { internalTarget(href); } catch { return; }
    event.preventDefault(); void run(href, "push").catch(() => { status.textContent = "Navigation failed. Reload to continue."; });
  }
  function pop(event) {
    if (disposed || !started) return;
    if (frame) { cancelAnimationFrame(frame); frame = 0; }
    const url = target();
    if (url === activeURL) { pending?.abort.abort(); pending = null; outlet.removeAttribute("aria-busy"); status.textContent = "Ready."; const point = saved(event.state, url + location.hash); if (point) restore(point); return; }
    try { void run(url, "pop", saved(event.state, url + location.hash) ?? { x: 0, y: 0 }).catch(() => fallback(url)); } catch { fallback(url); }
  }
  scope.addEventListener("click", click); window.addEventListener("popstate", pop); window.addEventListener("scroll", scroll, { passive: true });
  return Object.freeze({
    start({ accept } = {}) { if (started || pending || disposed || accept !== undefined && typeof accept !== "function") throw failure(); return run(target(), "start", { x: scrollX, y: scrollY }, accept); },
    navigate(url, { replace = false } = {}) { return run(url, replace ? "replace" : "push"); },
    current() { return current; },
    whenIdle() { return idle; },
    stats() { return Object.freeze({ state: disposed ? "disposed" : started ? "active" : "idle", pending: pending !== null, url: activeURL, listeners: disposed ? 0 : 3 }); },
    dispose() {
      if (disposed) return;
      try { save(); } catch { /* Continue cleanup. */ }
      disposed = true; pending?.abort.abort(); pending = null; if (frame) cancelAnimationFrame(frame);
      scope.removeEventListener("click", click); window.removeEventListener("popstate", pop); window.removeEventListener("scroll", scroll);
      for (const [name, value] of [["role", previousRole], ["aria-live", previousLive]]) { if (value === null) status.removeAttribute(name); else status.setAttribute(name, value); }
      history.scrollRestoration = previousRestoration; OWNERS.delete(window); outlet.removeAttribute("aria-busy"); current?.dispose(); current = null;
    },
  });
}

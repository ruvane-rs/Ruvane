import { decodePage, internalTarget, NAVIGATION_MEDIA_TYPE } from "./bridge.mjs";

export const CACHE_LIMITS = Object.freeze({ entries: 32, bytes: 2097152, prefetches: 2, jobs: 32, ttlMs: 300000 });
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
function saved(value, url) { const item = value?.[KEY]; return item?.version === 1 && item.url === url ? { x: position(item.x), y: position(item.y) } : null; }
/** Returns a conservative expiration only for the explicit public response profile. */
export function cacheFreshness(response, ttlMs, startedAt) {
  if (response.status !== 200 || response.redirected) return null;
  const control = response.headers.get("cache-control")?.toLowerCase().split(",").map(part => part.trim());
  if (!control || control.length !== 3 || control.filter(part => part === "public").length !== 1 || control.filter(part => part === "must-revalidate").length !== 1) return null;
  const ages = control.filter(part => /^max-age=[1-9][0-9]{0,2}$/.test(part)); if (ages.length !== 1) return null;
  const seconds = Number(ages[0].slice(8)); if (seconds > 300) return null;
  const vary = response.headers.get("vary")?.toLowerCase().split(",").map(part => part.trim()).sort();
  if (JSON.stringify(vary) !== JSON.stringify(["accept", "ruvane-application-revision"])) return null;
  const age = response.headers.get("age"); if (age !== null && !/^(0|[1-9][0-9]{0,9})$/.test(age)) return null;
  const expires = startedAt + Math.min(ttlMs, (seconds - Number(age ?? 0)) * 1000);
  return expires > performance.now() ? expires : null;
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

/** Keeps page data in a bounded cache; live views belong to the runtime. */
export function createNavigator(runtime, { outlet, scope = document, status, timeoutMs = 10000, maxCacheEntries = 32, maxCacheBytes = 2097152, cacheTtlMs = 30000, prefetchConcurrency = 2 } = {}) {
  if (!runtime || !["mountPage", "validatePage", "actionState", "subscribeActions"].every(name => typeof runtime[name] === "function") || !(outlet instanceof HTMLElement) || outlet.ownerDocument !== document || !(scope === document || scope instanceof HTMLElement && scope.ownerDocument === document) || !scope.contains(outlet) || OWNERS.has(window) || !(status instanceof HTMLElement) || status.ownerDocument !== document || outlet.contains(status) || !Number.isInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 10000 || !["http:", "https:"].includes(location.protocol)) throw failure();
  for (const [value, min, max] of [[maxCacheEntries,0,32],[maxCacheBytes,0,2097152],[cacheTtlMs,1,300000],[prefetchConcurrency,1,2]]) if (!Number.isInteger(value) || value < min || value > max) throw failure();
  if (runtime.actionState().state !== "active") throw failure(); stateObject(history.state);
  const previousRestoration = history.scrollRestoration, previousRole = status.getAttribute("role"), previousLive = status.getAttribute("aria-live");
  const cache = new Map(), jobs = new Map(), activeJobs = new Set();
  const identity = `${location.origin}\n${runtime.applicationRevision ?? "unversioned"}\n`;
  let current = null, activeURL = target(), pending = null, disposed = false, started = false, frame = 0, idle = Promise.resolve("idle");
  let generation = 0, bytesHeld = 0, blocked = false, pumpQueued = false, hits = 0, misses = 0;
  const key = url => identity + url;
  const eligible = () => !disposed && !blocked && generation !== null && maxCacheEntries > 0 && maxCacheBytes > 0 && runtime.actionState().state === "active" && runtime.actionState().epoch !== null && !runtime.actionState().uncertain && runtime.actionState().pending === 0;
  function safeURL(url) { internalTarget(url); const absolute = new URL(url, location.href); if (absolute.origin !== location.origin || urlTarget(absolute) !== url) throw failure(); return absolute; }
  function remove(url) { const item = cache.get(key(url)); if (item) { bytesHeld -= item.bytes.length; cache.delete(key(url)); } }
  function sweep() { const now = performance.now(); for (const [id,item] of cache) if (item.expires <= now) { bytesHeld -= item.bytes.length; cache.delete(id); } }
  function lookup(url) {
    sweep(); if (!eligible()) return null; const item = cache.get(key(url));
    if (item) { cache.delete(key(url)); cache.set(key(url),item); } return item ?? null;
  }
  function store(url, bytes, expires, version, epoch) {
    remove(url); sweep();
    if (!eligible() || version !== generation || epoch !== runtime.actionState().epoch || expires === null || expires <= performance.now() || bytes.length > maxCacheBytes) return false;
    while (cache.size >= maxCacheEntries || bytesHeld + bytes.length > maxCacheBytes) { const [id,item] = cache.entries().next().value; bytesHeld -= item.bytes.length; cache.delete(id); }
    cache.set(key(url),{ bytes: bytes.slice(), expires }); bytesHeld += bytes.length; return true;
  }
  function finishJob(job, outcome) {
    if (job.finished) return; job.finished = true; job.controller?.abort();
    if (jobs.get(job.url) === job) jobs.delete(job.url); activeJobs.delete(job); job.resolve(outcome); schedule();
  }
  function cancelJobs(reason, url) { for (const job of [...jobs.values()]) if (url === undefined || job.url === url) { job.reason = reason; finishJob(job,reason); } }
  function cancelNavigation(reason, url) { if (pending && !pending.committing && (url === undefined || pending.url === url)) { pending.reason = reason; pending.abort.abort(); } }
  function invalidate(url, reason = "invalidated") {
    if (disposed) return;
    if (url !== undefined) safeURL(url);
    generation = generation === null || generation === Number.MAX_SAFE_INTEGER ? null : generation + 1;
    if (url === undefined) { cache.clear(); bytesHeld = 0; } else remove(url);
    cancelJobs(reason,url); cancelNavigation(reason,url);
  }
  function updated() { blocked = true; invalidate(undefined,"invalidated"); status.textContent = "Application updated. Reload to continue."; }
  function schedule() { if (!pumpQueued) { pumpQueued = true; queueMicrotask(() => { pumpQueued = false; pump(); }); } }
  const headers = () => ({ Accept: NAVIGATION_MEDIA_TYPE, ...(runtime.applicationRevision ? { "Ruvane-Application-Revision": runtime.applicationRevision } : {}) });
  const request = (url, signal) => fetch(safeURL(url).href,{ method:"GET", mode:"same-origin", credentials:"same-origin", redirect:"error", headers:headers(), signal, cache:"no-cache" });
  function verify(bytes,response,url) { const page = decodePage(bytes); if (page.url !== url || page.status !== response.status || response.redirected) throw failure(); return page; }
  async function performPrefetch(job) {
    const version = generation, epoch = runtime.actionState().epoch, began = performance.now();
    const timer = setTimeout(() => job.controller.abort(),timeoutMs);
    try {
      const response = await request(job.url,job.controller.signal);
      if (response.status === 409 && jobs.get(job.url) === job) { updated(); return; }
      const expires = cacheFreshness(response,cacheTtlMs,began);
      if (expires === null) { finishJob(job,"skipped"); return; }
      const bytes = await pageBytes(response,job.controller.signal); verify(bytes,response,job.url);
      if (job.finished || version !== generation || epoch !== runtime.actionState().epoch || job.controller.signal.aborted) { finishJob(job,job.reason ?? "invalidated"); return; }
      runtime.validatePage(bytes);
      finishJob(job,store(job.url,bytes,expires,version,epoch) ? "prefetched" : "skipped");
    } catch { finishJob(job,job.reason ?? (disposed ? "disposed" : "failed")); }
    finally { clearTimeout(timer); job.controller.abort(); }
  }
  function pump() {
    if (disposed || pending || !eligible()) return;
    for (const job of jobs.values()) {
      if (activeJobs.size >= prefetchConcurrency) break;
      if (!job.controller) { job.controller = new AbortController(); activeJobs.add(job); void performPrefetch(job); }
    }
  }
  function prefetch(url) {
    try { safeURL(url); } catch (error) { return Promise.reject(error); }
    if (disposed) return Promise.resolve("disposed");
    if (!started || pending || !eligible()) return Promise.resolve("skipped");
    if (lookup(url)) return Promise.resolve("cached");
    if (jobs.has(url)) return jobs.get(url).promise;
    if (jobs.size >= CACHE_LIMITS.jobs) return Promise.resolve("skipped");
    const job = { url, controller:null, finished:false, reason:null }; job.promise = new Promise(resolve => { job.resolve = resolve; }); jobs.set(url,job); pump(); return job.promise;
  }
  function actionChanged(state) { invalidate(); if (state.state !== "active") cancelJobs("disposed"); schedule(); }
  const unsubscribe = runtime.subscribeActions(actionChanged);
  OWNERS.add(window); status.setAttribute("role","status"); status.setAttribute("aria-live","polite");
  function save() { if (!disposed && started && activeURL === target()) history.replaceState({ ...stateObject(history.state), [KEY]:entry(target()+location.hash,scrollX,scrollY) },"",location.href); }
  function scroll() {
    if (!started || disposed || frame || pending?.mode === "pop") return;
    frame = requestAnimationFrame(() => { frame = 0; try { save(); } catch { /* History storage can be full. */ } });
  }
  const restore = point => window.scrollTo(position(point.x),position(point.y));
  const fallback = url => { if (!disposed) location.replace(url); };
  function run(url,mode,point={x:0,y:0},accept,fresh=false) {
    if (disposed) return Promise.reject(new Error("Ruvane navigator disposed")); safeURL(url);
    if (mode !== "start" && !started) return Promise.reject(failure());
    cancelNavigation("superseded"); cancelJobs("superseded");
    const operation = { url, mode, abort:new AbortController(), expired:false, updated:false, reason:null, committing:false }; pending = operation;
    const version = generation, epoch = runtime.actionState().epoch;
    status.textContent = "Loading…"; outlet.setAttribute("aria-busy","true");
    const timer = setTimeout(() => { operation.expired = true; operation.abort.abort(); },timeoutMs);
    idle = (async () => {
      let prepared = null;
      try {
        const cached = fresh ? null : lookup(url); let bytes, page, expires = null;
        if (cached) { hits = Math.min(Number.MAX_SAFE_INTEGER,hits+1); bytes = cached.bytes; page = decodePage(bytes); }
        else {
          misses = Math.min(Number.MAX_SAFE_INTEGER,misses+1); const began = performance.now();
          const response = await request(url,operation.abort.signal); operation.updated = response.status === 409;
          if (operation.updated) { blocked = true; invalidate(); }
          bytes = await pageBytes(response,operation.abort.signal); page = verify(bytes,response,url); expires = cacheFreshness(response,cacheTtlMs,began);
        }
        if (disposed || pending !== operation) return disposed ? "disposed" : "superseded";
        if (operation.reason || operation.abort.signal.aborted) { status.textContent = "Data changed. Try the link again."; if (mode === "pop") fallback(url); return operation.reason ?? "invalidated"; }
        const host = document.createElement("div"); host.tabIndex = -1; prepared = runtime.mountPage(host,bytes);
        if (mode === "start" && accept && !accept()) { prepared.dispose(); prepared = null; status.textContent = "Native forms are ready."; return "native"; }
        operation.committing = true;
        if (mode === "push") { save(); history.pushState({ [KEY]:entry(url,0,0) },"",url); }
        else if (mode === "start" || mode === "replace") history.replaceState({ ...stateObject(history.state), [KEY]:entry(mode === "start" ? url+location.hash : url,point.x,point.y) },"",mode === "start" ? location.href : url);
        current?.dispose(); current = prepared; prepared = null; outlet.replaceChildren(host); activeURL = url; started = true; history.scrollRestoration = "manual";
        document.title = page.title; status.textContent = page.status === 200 ? "Ready." : `Page ${page.status}.`;
        if (mode !== "start") host.focus({ preventScroll:true }); restore(point);
        if (!cached) store(url,bytes,expires,version,epoch); return "success";
      } catch {
        remove(url); try { prepared?.dispose(); } catch { fallback(url); }
        if (disposed || pending !== operation) return disposed ? "disposed" : "superseded";
        if (operation.reason && !operation.updated && runtime.actionState().state === "active") { status.textContent = "Data changed. Try the link again."; if (mode === "pop") fallback(url); return operation.reason; }
        status.textContent = operation.updated ? "Application updated. Reload to continue." : operation.expired ? "Navigation timed out. Try the link again." : "Navigation failed. Try the link again or reload.";
        if (operation.committing || mode === "pop" || runtime.stats().state !== "active" || activeURL !== target()) fallback(url); return "failed";
      } finally { operation.abort.abort(); clearTimeout(timer); if (pending === operation) { pending = null; outlet.removeAttribute("aria-busy"); } schedule(); }
    })(); return idle;
  }
  function selected(event,marked=false) {
    const node = event.target instanceof Element ? event.target.closest(marked ? "a[data-ruvane-nav][data-ruvane-prefetch]" : "a[data-ruvane-nav]") : null;
    if (!node || !scope.contains(node) || ["target","download","ping"].some(name=>node.hasAttribute(name)) || node.relList.contains("external")) return null;
    const href = node.getAttribute("href"); try { safeURL(href); return href; } catch { return null; }
  }
  function click(event) {
    if (!started || disposed || event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    const href = selected(event); if (href === null) return; event.preventDefault(); void run(href,"push").catch(() => { status.textContent = "Navigation failed. Reload to continue."; });
  }
  function intend(event) { if (!started || disposed || event.defaultPrevented) return; const href = selected(event,true); if (href !== null) void prefetch(href).catch(()=>{}); }
  function pop(event) {
    if (disposed || !started) return; if (frame) { cancelAnimationFrame(frame); frame = 0; }
    const url = target();
    if (url === activeURL) { cancelNavigation("superseded"); pending = null; outlet.removeAttribute("aria-busy"); status.textContent = "Ready."; const point = saved(event.state,url+location.hash); if (point) restore(point); return; }
    try { void run(url,"pop",saved(event.state,url+location.hash) ?? {x:0,y:0}).catch(()=>fallback(url)); } catch { fallback(url); }
  }
  scope.addEventListener("click",click); scope.addEventListener("pointerover",intend); scope.addEventListener("focusin",intend); window.addEventListener("popstate",pop); window.addEventListener("scroll",scroll,{passive:true});
  return Object.freeze({
    start({accept}={}) { if (started || pending || disposed || accept !== undefined && typeof accept !== "function") throw failure(); return run(target(),"start",{x:scrollX,y:scrollY},accept); },
    navigate(url,{replace=false}={}) { return run(url,replace ? "replace" : "push"); },
    prefetch,
    invalidate(url) { invalidate(url); },
    refresh() { if (!started || disposed) return Promise.reject(failure()); invalidate(activeURL); return run(activeURL,"refresh",{x:scrollX,y:scrollY},undefined,true); },
    current() { return current; },
    whenIdle() { return idle; },
    stats() { sweep(); return Object.freeze({ state:disposed ? "disposed" : started ? "active" : "idle", pending:pending !== null, url:activeURL, listeners:disposed ? 0 : 5, cachePaused:blocked || generation === null || runtime.actionState().uncertain || runtime.actionState().pending > 0, cacheEntries:cache.size, cacheBytes:bytesHeld, pendingPrefetches:activeJobs.size, queuedPrefetches:jobs.size-activeJobs.size, hits, misses }); },
    dispose() {
      if (disposed) return; try { save(); } catch { /* Continue cleanup. */ } disposed = true; unsubscribe(); cancelNavigation("disposed"); pending = null; cancelJobs("disposed"); cache.clear(); bytesHeld = 0; if (frame) cancelAnimationFrame(frame);
      scope.removeEventListener("click",click); scope.removeEventListener("pointerover",intend); scope.removeEventListener("focusin",intend); window.removeEventListener("popstate",pop); window.removeEventListener("scroll",scroll);
      for (const [name,value] of [["role",previousRole],["aria-live",previousLive]]) { if (value === null) status.removeAttribute(name); else status.setAttribute(name,value); }
      history.scrollRestoration = previousRestoration; OWNERS.delete(window); outlet.removeAttribute("aria-busy"); current?.dispose(); current = null;
    },
  });
}

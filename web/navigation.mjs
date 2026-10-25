import { decodePage, decodeNavigationSnapshot, internalTarget, NAVIGATION_MEDIA_TYPE, NAVIGATION_STREAM_MEDIA_TYPE } from "./bridge.mjs";

export const CACHE_LIMITS = Object.freeze({ entries: 32, bytes: 2097152, prefetches: 2, jobs: 32, ttlMs: 300000 });
const OWNERS = new WeakSet(), KEY = "__ruvane_navigation_v1";
let nextStreamToken = 0;
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
  if (![200, 400, 404, 500].includes(response.status) || response.headers.get("content-type")?.split(";")[0].trim().toLowerCase() !== NAVIGATION_MEDIA_TYPE || !response.body) throw failure();
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

const sameDescriptor = (a,b) => a.name===b.name && a.key===b.key && a.component===b.component && a.kind===b.kind && a.value===b.value && a.seed.length===b.seed.length && a.seed.every((value,n)=>value===b.seed[n]);
/** Reads bounded ordered packets. A final packet is usable only after terminal and EOF. */
export async function readNavigationStream(response, { signal, url, revision, token, prefix }) {
  if(response.status!==200||response.redirected||!response.body||response.headers.get("content-type")!==NAVIGATION_STREAM_MEDIA_TYPE||response.headers.get("content-length")!==null||response.headers.get("cache-control")!=="no-store")throw failure();
  const reader=response.body.getReader();let part=new Uint8Array(),offset=0,total=0,emptyReads=0;
  const abort=()=>{void reader.cancel().catch(()=>{});};
  signal.addEventListener("abort",abort,{once:true});
  const check=()=>{if(signal.aborted)throw failure();};
  async function take(size){
    const result=new Uint8Array(size);let written=0;
    while(written<size){
      check();
      if(offset===part.length){const read=await reader.read();check();if(read.done||!(read.value instanceof Uint8Array))throw failure();part=read.value;offset=0;emptyReads=part.length?0:emptyReads+1;total+=part.length;if(total>524288||emptyReads>64)throw failure();}
      const count=Math.min(size-written,part.length-offset);result.set(part.subarray(offset,offset+count),written);offset+=count;written+=count;
    }
    return result;
  }
  const number=async()=>new DataView((await take(4)).buffer).getUint32(0,true);
  let previous=null,final=null,bytes=null;
  try{
    const header=await take(20),view=new DataView(header.buffer);
    if(String.fromCharCode(...header.slice(0,4))!=="RVS1"||view.getBigUint64(4,true).toString()!==revision||view.getBigUint64(12,true).toString()!==token)throw failure();
    for(let count=0;count<9;count++){
      const length=await number();
      if(length===0){
        if(!final||offset!==part.length)throw failure();
        const end=await reader.read();check();if(!end.done)throw failure();
        return {bytes,page:final};
      }
      if(final||length>65532)throw failure();
      bytes=await take(length);const page=decodePage(bytes);
      if(page.url!==url||page.selected!==0)throw failure();
      if(!previous){if(page.pending===null||page.layouts.length!==0)throw failure();}
      else {
        if(page.pending!==null&&(page.pending!==previous.pending||page.component!==previous.component||page.kind!==previous.kind||page.value!==previous.value||page.layouts.length!==previous.layouts.length+1))throw failure();
        if(page.pending===null&&(page.failed===null?(page.status!==200||previous.layouts.length!==previous.pending||page.layouts.length!==previous.pending||page.component!==previous.component||page.kind!==previous.kind||page.value!==previous.value):(page.failed!==previous.layouts.length||page.layouts.length>previous.layouts.length)))throw failure();
        if(!page.layouts.every((item,index)=>index>=previous.layouts.length||sameDescriptor(item,previous.layouts[index])))throw failure();
      }
      if(page.pending===null)final=page;
      else {await prefix(bytes,page);check();previous=page;}
    }
    throw failure();
  } catch(error) {try{await reader.cancel();}catch{}throw error;}
  finally{signal.removeEventListener("abort",abort);reader.releaseLock();}
}

const identifier=value=>typeof value==="string"&&/^[A-Za-z][A-Za-z0-9_-]{0,47}$/.test(value);
const pathParts=url=>url.split("?")[0]==="/"?[]:url.split("?")[0].slice(1).split("/").map(value=>decodeURIComponent(value));
function patternParts(pattern){
  if(typeof pattern!=="string"||pattern.length>4096)throw failure();
  const names=new Set();
  internalTarget(pattern.replace(/\{([A-Za-z_][A-Za-z0-9_]*)\}/g,(_,name)=>{if(names.has(name))throw failure();names.add(name);return "1";}));
  return pattern==="/"?[]:pattern.slice(1).split("/").map(value=>/^\{[A-Za-z_][A-Za-z0-9_]*\}$/.test(value)?null:decodeURIComponent(value));
}
function boundaryConfig(entries){
  if(!Array.isArray(entries)||entries.length>160)throw failure();
  const used=new Set(),records=entries.map(item=>{
    if(!item||!["page","layout"].includes(item.kind)||!identifier(item.name)||!Array.isArray(item.chain)||item.chain.length>6||new Set(item.chain).size!==item.chain.length||!item.chain.every(identifier))throw failure();
    for(const id of [item.loading,item.error])if(!Number.isInteger(id)||id<0||id>0xffffffff)throw failure();
    if((item.loading===0)!==(item.error===0))throw failure();
    const key=item.kind+":"+item.name;if(used.has(key))throw failure();used.add(key);
    const parts=patternParts(item.pattern);if(parts.length>17)throw failure();
    return Object.freeze({kind:item.kind,name:item.name,pattern:item.pattern,chain:Object.freeze([...item.chain]),parts:Object.freeze(parts),loading:item.loading,error:item.error});
  });
  const layouts=new Map(records.filter(item=>item.kind==="layout").map(item=>[item.name,item]));
  for(const item of records){
    if(item.kind==="layout"&&item.chain.at(-1)!==item.name)throw failure();
    for(let n=0;n<item.chain.length;n++){
      const parent=layouts.get(item.chain[n]);if(!parent||parent.chain.length!==n+1||!parent.chain.every((name,i)=>name===item.chain[i])||parent.parts.length>item.parts.length||!parent.parts.every((part,i)=>part===null||part===item.parts[i]))throw failure();
    }
  }
  return Object.freeze(records);
}
/** Reads the application's bounded public presentation declarations. */
export function readBoundaries(root){
  const manifests=root.querySelectorAll("#ruvane-boundaries");if(!manifests.length)return [];
  if(manifests.length!==1||manifests[0].children.length>160)throw failure();
  const items=[...manifests[0].children].map(node=>{
    if(node.localName!=="span")throw failure();
    const value=name=>node.getAttribute("data-rvn-"+name);
    const number=name=>{const raw=value(name);if(!/^(0|[1-9][0-9]{0,9})$/.test(raw??""))throw failure();return Number(raw);};
    return {kind:value("kind"),name:value("name"),pattern:value("pattern"),chain:value("chain")?value("chain").split(","):[],loading:number("loading"),error:number("error")};
  });
  return boundaryConfig(items);
}

/** Reads only the document's inert, bounded RVI1 metadata. No loader is called. */
export function readInitialSnapshot(root){
  if(!(root instanceof HTMLElement)||root.ownerDocument!==document)throw failure();
  const items=root.querySelectorAll("#ruvane-initial"),node=items[0];
  if(items.length!==1||document.querySelectorAll("#ruvane-initial").length!==1||node.parentNode!==root||node.localName!=="div"||node.attributes.length!==2||!node.hasAttribute("hidden")||node.childNodes.length!==1||node.firstChild.nodeType!==3)throw failure();
  const profile=root.getAttribute("data-ruvane-document");
  if(profile!==null){
    const markers=root.querySelectorAll("#ruvane-complete"),end=markers[0];
    if(profile!=="stream"||document.readyState==="loading"||markers.length!==1||document.querySelectorAll("#ruvane-complete").length!==1||end.parentNode!==root||end!==root.lastElementChild||node.nextElementSibling!==end||end.localName!=="div"||end.attributes.length!==2||!end.hasAttribute("hidden")||end.childNodes.length!==0)throw failure();
  }
  const text=node.textContent;
  if(text.length>131104||text.length%2||!/^[0-9a-f]+$/.test(text))throw failure();
  const bytes=new Uint8Array(text.length/2);for(let n=0;n<bytes.length;n++)bytes[n]=parseInt(text.slice(n*2,n*2+2),16);
  const snapshot=decodeNavigationSnapshot(bytes);
  if(snapshot.revision!==root.getAttribute("data-ruvane-revision")||snapshot.page.url!==target())throw failure();
  return bytes;
}

/** Keeps page data in a bounded cache; live views belong to the runtime. */
export function createNavigator(runtime, { outlet, scope = document, status, timeoutMs = 10000, maxCacheEntries = 32, maxCacheBytes = 2097152, cacheTtlMs = 30000, prefetchConcurrency = 2, boundaries = [], progressive = false } = {}) {
  if(typeof progressive!=="boolean"||progressive&&!runtime?.applicationRevision)throw failure();
  if (!runtime || !["mountPage", "mountPageSegment", "validatePage", "actionState", "subscribeActions"].every(name => typeof runtime[name] === "function") || !(outlet instanceof HTMLElement) || outlet.ownerDocument !== document || !(scope === document || scope instanceof HTMLElement && scope.ownerDocument === document) || !scope.contains(outlet) || OWNERS.has(window) || !(status instanceof HTMLElement) || status.ownerDocument !== document || outlet.contains(status) || !Number.isInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 10000 || !["http:", "https:"].includes(location.protocol)) throw failure();
  for (const [value, min, max] of [[maxCacheEntries,0,32],[maxCacheBytes,0,2097152],[cacheTtlMs,1,300000],[prefetchConcurrency,1,2]]) if (!Number.isInteger(value) || value < min || value > max) throw failure();
  const presentation=boundaryConfig(boundaries), layoutPresentation=new Map(presentation.filter(item=>item.kind==="layout").map(item=>[item.name,item]));
  if(presentation.length&&typeof runtime.mountBoundary!=="function")throw failure();
  if (runtime.actionState().state !== "active") throw failure(); stateObject(history.state);
  const previousRestoration = history.scrollRestoration, previousRole = status.getAttribute("role"), previousLive = status.getAttribute("aria-live");
  const cache = new Map(), jobs = new Map(), activeJobs = new Set();
  const identity = `${location.origin}\n${runtime.applicationRevision ?? "unversioned"}\n`;
  let current = null, branch = [], pageHost = null, activeURL = target(), pending = null, disposed = false, started = false, frame = 0, idle = Promise.resolve("idle");
  let feedback=null, retryButton=null, lastFailure=null;
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
  function cancelNavigation(reason, url) { if (pending && !pending.committing && (url === undefined || pending.url === url)) { pending.reason = reason; pending.abort.abort(); rollbackPreview(pending); } }
  function invalidate(url, reason = "invalidated") {
    if (disposed) return;
    if (url !== undefined) safeURL(url);
    if(lastFailure&&(url===undefined||lastFailure.url===url)){clearFeedback();lastFailure=null;}
    generation = generation === null || generation === Number.MAX_SAFE_INTEGER ? null : generation + 1;
    if (url === undefined) { cache.clear(); bytesHeld = 0; } else remove(url);
    cancelJobs(reason,url); cancelNavigation(reason,url);
  }
  function updated() { clearFeedback();lastFailure=null;blocked = true; invalidate(undefined,"invalidated"); status.textContent = "Application updated. Reload to continue."; }
  function schedule() { if (!pumpQueued) { pumpQueued = true; queueMicrotask(() => { pumpQueued = false; pump(); }); } }
  const headers = token => ({ Accept: token ? NAVIGATION_STREAM_MEDIA_TYPE : NAVIGATION_MEDIA_TYPE, ...(token ? {"Ruvane-Stream-Token":token} : {}), ...(runtime.applicationRevision ? { "Ruvane-Application-Revision": runtime.applicationRevision } : {}) });
  const request = (url, signal, token=null) => fetch(safeURL(url).href,{ method:"GET", mode:"same-origin", credentials:"same-origin", redirect:"error", headers:headers(token), signal, cache:"no-cache" });
  function verify(bytes,response,url) { const page = decodePage(bytes); if (page.pending!==null||page.url !== url || page.status !== response.status || page.selected !== 0 || response.redirected) throw failure(); return page; }
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
  function actionChanged(state) { clearFeedback();lastFailure=null;invalidate(); if (state.state !== "active") cancelJobs("disposed"); schedule(); }
  const unsubscribe = runtime.subscribeActions(actionChanged);
  OWNERS.add(window); status.setAttribute("role","status"); status.setAttribute("aria-live","polite");
  function save() { if (!disposed && started && activeURL === target()) {const point=pending?.preview?.point??{x:scrollX,y:scrollY};history.replaceState({ ...stateObject(history.state), [KEY]:entry(target()+location.hash,point.x,point.y) },"",location.href);} }
  function scroll() {
    if (!started || disposed || frame || pending?.mode === "pop" || pending?.preview) return;
    frame = requestAnimationFrame(() => { frame = 0; try { save(); } catch { /* History storage can be full. */ } });
  }
  const restore = point => window.scrollTo(position(point.x),position(point.y));
  const fallback = url => { if (!disposed) location.replace(url); };
  const sameLayout = sameDescriptor;
  function checkBranch(items=branch,leaf=current!==null) {
    let slot=outlet;
    for(const layout of items) {
      if(slot.childNodes.length!==1 || slot.firstChild!==layout.host || layout.host.childNodes.length!==2 || layout.host.firstChild!==layout.own || layout.host.lastChild!==layout.outlet || layout.host.getAttribute("data-ruvane-layout")!==layout.descriptor.name)throw failure();
      slot=layout.outlet;
    }
    if(leaf && (slot.childNodes.length!==1 || slot.firstChild!==pageHost))throw failure();
  }
  function releaseBranch(items) { for(const item of [...items].reverse())item.controller.dispose(); }
  function feedbackConfig(url){
    const parts=pathParts(url),matches=item=>item.kind==="page"&&item.parts.length===parts.length&&item.parts.every((part,n)=>part===null||part===parts[n]);
    const pages=presentation.filter(matches);pages.sort((a,b)=>Number(a.parts.includes(null))-Number(b.parts.includes(null)));
    const page=pages[0];if(!page)return null;
    let declaration=page.loading?page:null,depth=page.chain.length;
    if(!declaration)for(let n=page.chain.length-1;n>=0;n--){const parent=layoutPresentation.get(page.chain[n]);if(parent.loading){declaration=parent;depth=n+1;break;}}
    if(!declaration)return null;
    let shared=0;
    while(shared<depth&&shared<branch.length&&branch[shared].descriptor.name===page.chain[shared]){
      const old=pathParts(branch[shared].descriptor.key);if(!old.every((part,n)=>part===parts[n]))break;shared++;
    }
    return {declaration,slot:shared?branch[shared-1].outlet:outlet};
  }
  function clearFeedback(){
    const previous=feedback;feedback=null;retryButton=null;if(!previous)return;
    previous.host.remove();if(previous.busy) {if(previous.previousBusy===null)previous.slot.removeAttribute("aria-busy");else previous.slot.setAttribute("aria-busy",previous.previousBusy);}
    if(previous.controller)try{previous.controller.dispose();}catch{queueMicrotask(()=>{try{previous.controller.dispose();}catch{ /* Runtime shutdown owns remaining views. */ }});}
  }
  function showFeedback(url,kind,component=true,selectedSlot=null){
    const config=feedbackConfig(url);if(!config&&!selectedSlot)return;
    const slot=selectedSlot??(component?config.slot:(branch.length?branch.at(-1).outlet:outlet)),host=document.createElement("div"),own=document.createElement("div");
    host.setAttribute("data-ruvane-feedback",kind);host.setAttribute("role",kind==="loading"?"status":"alert");host.setAttribute("aria-live",kind==="loading"?"polite":"assertive");host.append(own);
    const previousBusy=slot.getAttribute("aria-busy");if(kind==="loading")slot.setAttribute("aria-busy","true");
    let controller=null;
    if(component&&config)try{controller=runtime.mountBoundary(own,{component:kind==="loading"?config.declaration.loading:config.declaration.error,state:{kind:"number",value:kind==="timeout"?408:0}});}catch{own.textContent=kind==="loading"?"Loading…":"This page could not be loaded.";}
    else own.textContent=kind==="loading"?"Loading…":"This page could not be loaded.";
    if(kind!=="loading") {retryButton=document.createElement("button");retryButton.type="button";retryButton.textContent="Try again";host.append(retryButton);}
    slot.append(host);feedback={host,slot,controller,busy:kind==="loading",previousBusy,kind};
  }
  function rollbackPreview(operation){
    const preview=operation.preview;if(!preview)return;
    operation.preview=null;clearFeedback();preview.mountPoint.replaceChildren(preview.saved);
    if(preview.focus?.isConnected&&document.activeElement===document.body)preview.focus.focus({preventScroll:true});
    restore(preview.point);
    for(const item of [...preview.layouts].reverse())try{item.controller.dispose();}catch{queueMicrotask(()=>{try{item.controller.dispose();}catch{ /* A stopped runtime owns remaining views. */ }});}
  }
  function previewPrefix(operation,bytes,page,resetLayouts){
    if(disposed||pending!==operation||operation.reason||operation.abort.signal.aborted)throw failure();
    runtime.validatePage(bytes);
    if(page.layouts.length===0)return;
    clearFeedback();
    let preview=operation.preview;
    if(!preview){
      checkBranch();let shared=0;
      if(!resetLayouts)while(shared<branch.length&&shared<page.layouts.length&&sameLayout(branch[shared].descriptor,page.layouts[shared]))shared++;
      if(shared===page.layouts.length){showFeedback(operation.url,"loading");return;}
      const mountPoint=shared?branch[shared-1].outlet:outlet,saved=document.createDocumentFragment(),focus=mountPoint.contains(document.activeElement)?document.activeElement:null,point={x:scrollX,y:scrollY};
      while(mountPoint.firstChild)saved.append(mountPoint.firstChild);
      preview={shared,mountPoint,saved,layouts:[],focus,point};operation.preview=preview;
    }
    const candidate=branch.slice(0,preview.shared).concat(preview.layouts);
    checkBranch(candidate,false);
    let slot=preview.layouts.at(-1)?.outlet??preview.mountPoint;
    for(let index=candidate.length;index<page.layouts.length;index++){
      if(slot.childNodes.length)throw failure();
      const descriptor=page.layouts[index],host=document.createElement("section"),own=document.createElement("div"),child=document.createElement("div");
      host.setAttribute("data-ruvane-layout",descriptor.name);own.setAttribute("data-ruvane-layout-view","");child.setAttribute("data-ruvane-child-outlet","");host.append(own,child);
      const controller=runtime.mountPageSegment(own,bytes,index+1);preview.layouts.push({descriptor,host,own,outlet:child,controller});slot.append(host);slot=child;
    }
    showFeedback(operation.url,"loading",true,slot);
  }
  function run(url,mode,point={x:0,y:0},accept,fresh=false,resetLayouts=false) {
    if (disposed) return Promise.reject(new Error("Ruvane navigator disposed")); safeURL(url);
    if (mode !== "start" && !started) return Promise.reject(failure());
    cancelNavigation("superseded"); cancelJobs("superseded");clearFeedback();lastFailure=null;
    const operation = { url, mode, abort:new AbortController(), expired:false, updated:false, reason:null, committing:false }; pending = operation;
    const version = generation, epoch = runtime.actionState().epoch;
    status.textContent = "Loading…"; outlet.setAttribute("aria-busy","true");
    showFeedback(url,"loading");
    const timer = setTimeout(() => { operation.expired = true; operation.abort.abort(); },timeoutMs);
    idle = (async () => {
      let prepared = null, preparedLayouts = [];
      try {
        const cached = fresh ? null : lookup(url); let bytes, page, expires = null;
        if (cached) { hits = Math.min(Number.MAX_SAFE_INTEGER,hits+1); bytes = cached.bytes; page = decodePage(bytes); }
        else {
          misses = Math.min(Number.MAX_SAFE_INTEGER,misses+1); const began = performance.now();
          let token=null;
          if(progressive&&mode!=="start"){if(nextStreamToken===Number.MAX_SAFE_INTEGER)throw failure();token=String(++nextStreamToken);}
          const response = await request(url,operation.abort.signal,token); operation.updated = response.status === 409;
          if (operation.updated) { blocked = true; invalidate(); }
          if(response.headers.get("content-type")===NAVIGATION_STREAM_MEDIA_TYPE){
            if(!token)throw failure();
            ({bytes,page}=await readNavigationStream(response,{signal:operation.abort.signal,url,revision:runtime.applicationRevision,token,prefix:(bytes,page)=>previewPrefix(operation,bytes,page,resetLayouts)}));
          } else {bytes = await pageBytes(response,operation.abort.signal); page = verify(bytes,response,url); expires = cacheFreshness(response,cacheTtlMs,began);}
        }
        if (disposed || pending !== operation) return disposed ? "disposed" : "superseded";
        if (operation.reason || operation.abort.signal.aborted) { status.textContent = "Data changed. Try the link again."; if (mode === "pop") fallback(url); return operation.reason ?? "invalidated"; }
        clearFeedback();runtime.validatePage(bytes);
        const previousBranch=branch,preview=operation.preview,source=preview?branch.slice(0,preview.shared).concat(preview.layouts):branch;
        checkBranch(source,!preview&&current!==null);
        let shared=0;
        if(!resetLayouts||preview)while(shared<source.length && shared<page.layouts.length && sameLayout(source[shared].descriptor,page.layouts[shared]))shared++;
        const mountPoint = shared ? source[shared-1].outlet : outlet;
        const fragment=document.createDocumentFragment();let slot=fragment;
        for(let index=shared;index<page.layouts.length;index++){
          const descriptor=page.layouts[index],host=document.createElement("section"),own=document.createElement("div"),child=document.createElement("div");
          host.setAttribute("data-ruvane-layout",descriptor.name);own.setAttribute("data-ruvane-layout-view","");child.setAttribute("data-ruvane-child-outlet","");host.append(own,child);slot.append(host);
          const controller=runtime.mountPageSegment(own,bytes,index+1);preparedLayouts.push({descriptor,host,own,outlet:child,controller});slot=child;
        }
        const host = document.createElement("div"); host.tabIndex = -1; prepared = runtime.mountPage(host,bytes);slot.append(host);
        if (mode === "start" && accept && !accept()) { prepared.dispose(); prepared = null; releaseBranch(preparedLayouts);preparedLayouts=[]; status.textContent = "Native forms are ready."; return "native"; }
        operation.committing = true;
        if (mode === "push") { save(); history.pushState({ [KEY]:entry(url,0,0) },"",url); }
        else if (mode === "start" || mode === "replace") history.replaceState({ ...stateObject(history.state), [KEY]:entry(mode === "start" ? url+location.hash : url,point.x,point.y) },"",mode === "start" ? location.href : url);
        const retained=new Set(source.slice(0,shared).map(item=>item.controller)),retired=new Set();
        current?.dispose();
        for(const item of [...previousBranch,...source].reverse())if(!retained.has(item.controller)&&!retired.has(item.controller)){item.controller.dispose();retired.add(item.controller);}
        mountPoint.replaceChildren(fragment); branch=source.slice(0,shared).concat(preparedLayouts);preparedLayouts=[];current=prepared;prepared=null;operation.preview=null;pageHost=host; activeURL = url; started = true; history.scrollRestoration = "manual";
        document.title = page.title; status.textContent = page.status === 200 ? "Ready." : `Page ${page.status}.`;
        if (mode !== "start") host.focus({ preventScroll:true }); restore(point);
        if (!cached) store(url,bytes,expires,version,epoch);
        if(page.failed!==null){lastFailure={url,mode:"replace",point:{x:scrollX,y:scrollY},kind:"server",accept:null};showFeedback(url,"error",false);}
        return "success";
      } catch {
        if(pending===operation){clearFeedback();rollbackPreview(operation);}
        remove(url); try { prepared?.dispose(); releaseBranch(preparedLayouts); } catch { fallback(url); }
        if (disposed || pending !== operation) return disposed ? "disposed" : "superseded";
        if (operation.reason && !operation.updated && runtime.actionState().state === "active") { status.textContent = "Data changed. Try the link again."; if (mode === "pop") fallback(url); return operation.reason; }
        status.textContent = operation.updated ? "Application updated. Reload to continue." : operation.expired ? "Navigation timed out. Try the link again." : "Navigation failed. Try the link again or reload.";
        if (operation.committing || mode === "pop" || runtime.stats().state !== "active" || activeURL !== target()) fallback(url);
        else if(!operation.updated){lastFailure={url,mode,point,kind:operation.expired?"timeout":"network",accept};showFeedback(url,lastFailure.kind);}
        return "failed";
      } finally { operation.abort.abort(); clearTimeout(timer); if (pending === operation) { if(feedback?.kind==="loading")clearFeedback();pending = null; outlet.removeAttribute("aria-busy"); } schedule(); }
    })(); return idle;
  }
  function selected(event,marked=false) {
    const node = event.target instanceof Element ? event.target.closest(marked ? "a[data-ruvane-nav][data-ruvane-prefetch]" : "a[data-ruvane-nav]") : null;
    if (!node || !scope.contains(node) || ["target","download","ping"].some(name=>node.hasAttribute(name)) || node.relList.contains("external")) return null;
    const href = node.getAttribute("href"); try { safeURL(href); return href; } catch { return null; }
  }
  function click(event) {
    if (!started || disposed || event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    if(retryButton && (event.target===retryButton || retryButton.contains(event.target))){event.preventDefault();void retry();return;}
    const href = selected(event); if (href === null) return; event.preventDefault(); void run(href,"push").catch(() => { status.textContent = "Navigation failed. Reload to continue."; });
  }
  function intend(event) { if (!started || disposed || event.defaultPrevented) return; const href = selected(event,true); if (href !== null) void prefetch(href).catch(()=>{}); }
  function pop(event) {
    if (disposed || !started) return; if (frame) { cancelAnimationFrame(frame); frame = 0; }
    const url = target();
    if (url === activeURL) { clearFeedback();lastFailure=null;cancelNavigation("superseded"); pending = null; outlet.removeAttribute("aria-busy"); status.textContent = "Ready."; const point = saved(event.state,url+location.hash); if (point) restore(point); return; }
    try { void run(url,"pop",saved(event.state,url+location.hash) ?? {x:0,y:0}).catch(()=>fallback(url)); } catch { fallback(url); }
  }
  function retry(){
    if(disposed||pending||!lastFailure||blocked||runtime.actionState().state!=="active")return Promise.resolve("skipped");
    const attempt=lastFailure;invalidate(attempt.url);return run(attempt.url,attempt.mode,attempt.point,attempt.accept,true,false);
  }
  function initialDOM(page){
    const fallback=outlet.lastChild,layouts=[],roots=[];
    if(outlet.childNodes.length!==2||fallback.nodeType!==1||fallback.localName!=="div"||fallback.id!=="ruvane-fallback"||fallback.attributes.length!==1||outlet.querySelectorAll("#ruvane-fallback").length!==1)throw failure();
    let slot=outlet;
    for(const descriptor of page.layouts){
      const host=slot.firstChild,own=host?.firstChild,child=host?.lastChild;
      if(slot.childNodes.length!==(slot===outlet?2:1)||host?.nodeType!==1||host.localName!=="section"||host.attributes.length!==1||host.getAttribute("data-ruvane-layout")!==descriptor.name||host.childNodes.length!==2||own.localName!=="div"||own.attributes.length!==1||!own.hasAttribute("data-ruvane-layout-view")||own.childNodes.length!==1||child.localName!=="div"||child.attributes.length!==1||!child.hasAttribute("data-ruvane-child-outlet"))throw failure();
      layouts.push({descriptor,host,own,outlet:child});roots.push(own.firstChild);slot=child;
    }
    const host=slot.firstChild;
    if(slot.childNodes.length!==(slot===outlet?2:1)||host?.nodeType!==1||host.localName!=="div"||host.attributes.length!==2||!host.hasAttribute("data-ruvane-page")||host.getAttribute("tabindex")!=="-1"||host.childNodes.length!==1)throw failure();
    roots.push(host.firstChild);return{layouts,roots,host,fallback};
  }
  function initialize(snapshotBytes,accept){
    let transaction=null;
    try{
      const {revision,bytes,page}=decodeNavigationSnapshot(snapshotBytes),epoch=runtime.actionState().epoch;
      if(revision!==runtime.applicationRevision||page.url!==target()||runtime.actionState().pending||runtime.actionState().uncertain||typeof runtime.preparePageHydration!=="function")throw failure();
      runtime.validatePage(bytes);
      if(accept&&!accept()){status.textContent="Native forms are ready.";return "native";}
      if(disposed||pending||started)throw failure();
      const server=initialDOM(page);transaction=runtime.preparePageHydration(server.roots,bytes);
      const controllers=transaction.activate(),after=initialDOM(page);
      if(after.host!==server.host||after.fallback!==server.fallback||!after.roots.every((root,n)=>root===server.roots[n])||page.url!==target()||runtime.actionState().epoch!==epoch||accept&&!accept()||disposed||pending||started)throw failure();
      history.replaceState({...stateObject(history.state),[KEY]:entry(target()+location.hash,scrollX,scrollY)},"",location.href);
      if(disposed||pending||started||page.url!==target())throw failure();
      server.fallback.remove();transaction.commit();transaction=null;
      branch=server.layouts.map((item,n)=>({...item,controller:controllers[n]}));current=controllers.at(-1);pageHost=server.host;activeURL=page.url;started=true;history.scrollRestoration="manual";
      status.textContent=page.status===200?"Ready.":`Page ${page.status}.`;
      if(page.failed!==null){lastFailure={url:page.url,mode:"replace",point:{x:scrollX,y:scrollY},kind:"server",accept:null};showFeedback(page.url,"error",false);}
      return "success";
    }catch{
      transaction?.rollback();status.textContent="Interactive mode is unavailable. Native links and forms are ready.";return "native";
    }
  }
  scope.addEventListener("click",click); scope.addEventListener("pointerover",intend); scope.addEventListener("focusin",intend); window.addEventListener("popstate",pop); window.addEventListener("scroll",scroll,{passive:true});
  return Object.freeze({
    start({accept,snapshot}={}) { if (started || pending || disposed || accept !== undefined && typeof accept !== "function") throw failure(); if(snapshot!==undefined){idle=Promise.resolve(initialize(snapshot,accept));return idle;} return run(target(),"start",{x:scrollX,y:scrollY},accept); },
    navigate(url,{replace=false}={}) { return run(url,replace ? "replace" : "push"); },
    prefetch,
    retry,
    failure() {return lastFailure?Object.freeze({url:lastFailure.url,kind:lastFailure.kind}):null;},
    invalidate(url) { invalidate(url); },
    refresh() { if (!started || disposed) return Promise.reject(failure()); invalidate(activeURL); return run(activeURL,"refresh",{x:scrollX,y:scrollY},undefined,true,true); },
    current() { return current; },
    whenIdle() { return idle; },
    stats() { sweep(); return Object.freeze({ state:disposed ? "disposed" : started ? "active" : "idle", pending:pending !== null, url:activeURL, listeners:disposed ? 0 : 5, layouts:branch.length, feedback:feedback?.kind??null, retryable:lastFailure!==null, cachePaused:blocked || generation === null || runtime.actionState().uncertain || runtime.actionState().pending > 0, cacheEntries:cache.size, cacheBytes:bytesHeld, pendingPrefetches:activeJobs.size, queuedPrefetches:jobs.size-activeJobs.size, hits, misses }); },
    dispose() {
      if (disposed) return; try { save(); } catch { /* Continue cleanup. */ } disposed = true;clearFeedback();lastFailure=null; unsubscribe(); cancelNavigation("disposed"); pending = null; cancelJobs("disposed"); cache.clear(); bytesHeld = 0; if (frame) cancelAnimationFrame(frame);
      scope.removeEventListener("click",click); scope.removeEventListener("pointerover",intend); scope.removeEventListener("focusin",intend); window.removeEventListener("popstate",pop); window.removeEventListener("scroll",scroll);
      for (const [name,value] of [["role",previousRole],["aria-live",previousLive]]) { if (value === null) status.removeAttribute(name); else status.setAttribute(name,value); }
      history.scrollRestoration = previousRestoration; OWNERS.delete(window); outlet.removeAttribute("aria-busy"); current?.dispose(); releaseBranch(branch); branch=[]; current = null;pageHost=null;
    },
  });
}

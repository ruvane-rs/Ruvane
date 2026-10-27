import { decodePage, decodeNavigationSnapshot, decodeLayoutSnapshot, internalTarget, NAVIGATION_MEDIA_TYPE, NAVIGATION_STREAM_MEDIA_TYPE } from "./bridge.mjs";

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
  if (response.status !== 200 || response.redirected || response.headers.get("ruvane-cache-reset") !== null) return null;
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
  if (![200, 400, 403, 404, 500, 503].includes(response.status) || response.headers.get("content-type")?.split(";")[0].trim().toLowerCase() !== NAVIGATION_MEDIA_TYPE || !response.body) throw failure();
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
const HEAD_FIELDS='title,meta[name="description" i]:not([http-equiv]),meta[name="robots" i]:not([http-equiv]),link[rel="canonical" i],link[rel="icon" i][type="image/png" i]';
function preparePageHead(page,managed){
  const metadata=page.metadata??(managed?{description:null,robots:page.status===200?null:"noindex, follow",canonical:null,favicon:null}:null);
  if(metadata===null){
    const canonical=page.status===200?[]:[...document.head.querySelectorAll('link[rel="canonical" i]')];if(canonical.length>32)throw failure();
    return()=>{document.title=page.title;for(const node of canonical)node.remove();errorMetadata(page.status);};
  }
  const head=document.head,old=[...head.querySelectorAll(HEAD_FIELDS)];if(old.length>32)throw failure();
  const original=old.map(node=>node.outerHTML),fresh=[];
  const title=document.createElement("title");title.textContent=page.title;fresh.push(title);
  for(const [name,value] of [["description",metadata.description],["robots",metadata.robots]])if(value!==null){
    const node=document.createElement("meta");node.setAttribute("name",name);node.setAttribute("content",value);node.setAttribute("data-ruvane-head",name);fresh.push(node);
  }
  for(const [rel,value] of [["canonical",metadata.canonical],["icon",metadata.favicon]])if(value!==null){
    const node=document.createElement("link");node.setAttribute("rel",rel);node.setAttribute("href",value);node.setAttribute("data-ruvane-head",rel);if(rel==="icon")node.setAttribute("type","image/png");fresh.push(node);
  }
  return()=>{
    const current=[...head.querySelectorAll(HEAD_FIELDS)];
    if(document.head!==head||current.length!==old.length||current.some((node,n)=>node!==old[n]||node.outerHTML!==original[n]))throw failure();
    for(const node of old)node.remove();head.append(...fresh);
  };
}
const ERROR_ROBOTS=new WeakMap();
function errorMetadata(status){
  const head=document.head;
  let node=head.querySelector('meta[data-ruvane-error-status]');
  if(status===200){
    if(node){
      if(ERROR_ROBOTS.has(node)){
        const value=ERROR_ROBOTS.get(node);ERROR_ROBOTS.delete(node);
        if(value===null)node.removeAttribute("content");else node.setAttribute("content",value);
        node.removeAttribute("data-ruvane-error-status");
      }else node.remove();
    }
    return;
  }
  if(!node){
    node=head.querySelector('meta[name="robots"]');
    const existing=node?.getAttribute("content")?.toLowerCase().split(/[\s,]+/)??[];
    if(existing.includes("noindex")||existing.includes("none"))return;
    if(node)ERROR_ROBOTS.set(node,node.getAttribute("content"));
    else {node=document.createElement("meta");node.setAttribute("name","robots");head.append(node);}
    node.setAttribute("data-ruvane-error-status","");
  }
  const directives=(node.getAttribute("content")??"").toLowerCase().split(/[\s,]+/).filter(value=>value&&value!=="index"&&value!=="noindex");
  node.setAttribute("content",["noindex",...(directives.length?directives:["follow"])].join(", "));
}
function boundaryConfig(entries){
  if(!Array.isArray(entries)||entries.length>161)throw failure();
  const used=new Set(),records=entries.map(item=>{
    if(!item||!["app","page","layout"].includes(item.kind)||!identifier(item.name)||!Array.isArray(item.chain)||item.chain.length>6||new Set(item.chain).size!==item.chain.length||!item.chain.every(identifier))throw failure();
    for(const id of [item.loading,item.error])if(!Number.isInteger(id)||id<0||id>0xffffffff)throw failure();
    if((item.loading===0)!==(item.error===0)||item.kind==="app"&&(item.name!=="app"||item.pattern!=="/"||item.chain.length||!item.loading))throw failure();
    const key=item.kind+":"+item.name;if(used.has(key))throw failure();used.add(key);
    const parts=patternParts(item.pattern);if(parts.length>17)throw failure();
    return Object.freeze({kind:item.kind,name:item.name,pattern:item.pattern,chain:Object.freeze([...item.chain]),parts:Object.freeze(parts),loading:item.loading,error:item.error});
  });
  if(records.filter(item=>item.kind!=="app").length>160)throw failure();
  const layouts=new Map(records.filter(item=>item.kind==="layout").map(item=>[item.name,item]));
  for(const item of records){
    if(item.kind==="layout"&&item.chain.at(-1)!==item.name)throw failure();
    for(let n=0;n<item.chain.length;n++){
      const parent=layouts.get(item.chain[n]);if(!parent||parent.chain.length!==n+1||!parent.chain.every((name,i)=>name===item.chain[i])||parent.parts.length>item.parts.length||!parent.parts.every((part,i)=>part===null||part===item.parts[i]))throw failure();
    }
  }
  return Object.freeze(records);
}
function clientDeclaration(entries,url){
  const parts=pathParts(url),matches=entries.filter(item=>item.kind==="page"&&item.parts.length===parts.length&&item.parts.every((part,n)=>part===null||part===parts[n]));
  matches.sort((a,b)=>Number(a.parts.includes(null))-Number(b.parts.includes(null)));return matches[0]??null;
}
/** Reads the application's bounded public presentation declarations. */
export function readBoundaries(root){
  const manifests=root.querySelectorAll("#ruvane-boundaries");if(!manifests.length)return [];
  if(manifests.length!==1||manifests[0].children.length>161)throw failure();
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
  if(!(root instanceof HTMLElement)||root.ownerDocument!==document||root.querySelector("[data-ruvane-stream-error]"))throw failure();
  const items=root.querySelectorAll("#ruvane-initial"),node=items[0];
  if(items.length!==1||document.querySelectorAll("#ruvane-initial").length!==1||node.parentNode!==root||node.localName!=="div"||node.attributes.length!==2||!node.hasAttribute("hidden")||node.childNodes.length!==1||node.firstChild.nodeType!==3)throw failure();
  const profile=root.getAttribute("data-ruvane-document");
  if(profile!==null){
    const markers=root.querySelectorAll("#ruvane-complete"),end=markers[0];
    if(!["stream","segments"].includes(profile)||document.readyState==="loading"||markers.length!==1||document.querySelectorAll("#ruvane-complete").length!==1||end.parentNode!==root||end!==root.lastElementChild||node.nextElementSibling!==end||end.localName!=="div"||end.attributes.length!==2||!end.hasAttribute("hidden")||end.childNodes.length!==0)throw failure();
  }
  const text=node.textContent;
  if(text.length>131104||text.length%2||!/^[0-9a-f]+$/.test(text))throw failure();
  const bytes=new Uint8Array(text.length/2);for(let n=0;n<bytes.length;n++)bytes[n]=parseInt(text.slice(n*2,n*2+2),16);
  const snapshot=decodeNavigationSnapshot(bytes);
  if(snapshot.revision!==root.getAttribute("data-ruvane-revision")||snapshot.page.url!==target())throw failure();
  readRetiredBranch(root,snapshot.page);
  return bytes;
}

function readRetiredBranch(root,page){
  const items=root.querySelectorAll("#ruvane-retired"),profile=root.getAttribute?.("data-ruvane-document"),short=page.failed!==null&&page.failed>page.layouts.length;
  if(!items.length){if(short&&["stream","segments"].includes(profile))throw failure();return null;}
  const node=items[0],initial=root.querySelector("#ruvane-initial");
  if(!short||!["stream","segments"].includes(profile)||items.length!==1||document.querySelectorAll("#ruvane-retired").length!==1||node.parentNode!==root||node.nextElementSibling!==initial||node.localName!=="div"||node.attributes.length!==2||!node.hasAttribute("hidden")||node.childNodes.length!==1||node.firstChild.nodeType!==3)throw failure();
  const text=node.textContent;
  if(text.length>131104||text.length%2||!/^[0-9a-f]+$/.test(text))throw failure();
  const raw=new Uint8Array(text.length/2);for(let n=0;n<raw.length;n++)raw[n]=parseInt(text.slice(n*2,n*2+2),16);
  const prefix=decodeLayoutSnapshot(raw);
  if(prefix.revision!==root.getAttribute("data-ruvane-revision")||prefix.page.url!==page.url||prefix.page.title!==page.title||prefix.page.layouts.length!==page.failed||!page.layouts.every((item,n)=>sameDescriptor(item,prefix.page.layouts[n])))throw failure();
  return{node,text,bytes:prefix.bytes,page:prefix.page};
}

const DOCUMENT_OWNERS=new WeakSet();
/** Reads one complete inert RVH1 record; the ready child is emitted after its bytes. */
export function readLayoutSnapshot(root,node){
  if(!(root instanceof HTMLElement)||root.ownerDocument!==document||root.getAttribute("data-ruvane-document")!=="segments"||!(node instanceof HTMLElement)||!root.contains(node))throw failure();
  const ready=node.lastChild;
  if(node.localName!=="div"||node.attributes.length!==2||!node.hasAttribute("hidden")||node.getAttribute("data-ruvane-segment")!==""||node.childNodes.length!==2||node.firstChild.nodeType!==3||ready.localName!=="br"||ready.attributes.length!==1||ready.getAttribute("data-ruvane-ready")!==""||ready.childNodes.length)throw failure();
  const text=node.firstChild.data;
  if(text.length>131104||text.length%2||!/^[0-9a-f]+$/.test(text))throw failure();
  const bytes=new Uint8Array(text.length/2);for(let n=0;n<bytes.length;n++)bytes[n]=parseInt(text.slice(n*2,n*2+2),16);
  const snapshot=decodeLayoutSnapshot(bytes);
  if(snapshot.revision!==root.getAttribute("data-ruvane-revision")||snapshot.page.url!==target())throw failure();
  return snapshot;
}
/** Owns early layout hydration until the complete branch adopts it or startup retires. */
export function createDocumentHydration(runtime,root,{timeoutMs}={}){
  if(!(root instanceof HTMLElement)||root.ownerDocument!==document||root.getAttribute("data-ruvane-document")!=="segments"||DOCUMENT_OWNERS.has(root)||typeof runtime?.hydrateLayout!=="function"||typeof runtime?.retireLayout!=="function")throw failure();
  const raw=root.getAttribute("data-ruvane-layouts"),rawTime=root.getAttribute("data-ruvane-timeout");
  if(!/^[0-6]$/.test(raw??"")||! /^[1-9][0-9]{0,4}$/.test(rawTime??"")||Number(rawTime)>60000)throw failure();
  const shellNames=root.getAttribute("data-ruvane-shells"),layoutMode=root.getAttribute("data-ruvane-layout-hydration"),earlyLayouts=layoutMode!=="off";
  const names=shellNames===null?[]:shellNames===""?[]:shellNames.split(",");
  if(names.length>8||new Set(names).size!==names.length||!names.every(name=>/^[A-Za-z_][A-Za-z0-9_]{0,31}$/.test(name))||layoutMode!==null&&(layoutMode!=="off"||shellNames===null)||shellNames!==null&&!["hydrateShell","validateShells","commitShells","retireShell"].every(name=>typeof runtime[name]==="function"))throw failure();
  const expected=Number(raw),budget=timeoutMs??Number(rawTime);
  if(!Number.isFinite(budget)||budget<1||budget>Number(rawTime)||runtime.applicationRevision!==root.getAttribute("data-ruvane-revision"))throw failure();
  const url=target(),moduleURL=root.getAttribute("data-ruvane-module"),rows=[],shellRows=[];let snapshotBytes=null,state="waiting",queued=false,title=null,resolve,reject;
  const ready=new Promise((yes,no)=>{resolve=yes;reject=no;});
  DOCUMENT_OWNERS.add(root);
  const observer=new MutationObserver(schedule),timer=setTimeout(()=>retire(),budget);
  function cleanup(final=false){observer.disconnect();clearTimeout(timer);document.removeEventListener("readystatechange",schedule);if(final){window.removeEventListener("pagehide",leave);window.removeEventListener("popstate",leave);DOCUMENT_OWNERS.delete(root);}}
  function leave(){retire();}
  function retire(){
    if(state==="retired"||state==="committed")return false;state="retired";cleanup(true);
    for(const row of rows.reverse())try{runtime.retireLayout(row.controller);}catch{ /* Runtime failure owns any remaining handles. */ }
    for(const row of shellRows.reverse())try{runtime.retireShell(row.controller);}catch{ /* Runtime shutdown owns remaining views. */ }
    rows.length=0;shellRows.length=0;const status=root.querySelector("#ruvane-status");if(status)status.textContent="Interactive mode is unavailable. Native links and forms are ready.";
    reject(failure());return true;
  }
  function shape(){
    if(root.querySelector("[data-ruvane-stream-error]")||!root.isConnected||document.querySelectorAll("#ruvane-application").length!==1||document.getElementById("ruvane-application")!==root||root.getAttribute("data-ruvane-document")!=="segments"||root.getAttribute("data-ruvane-shells")!==shellNames||root.getAttribute("data-ruvane-layout-hydration")!==layoutMode||root.getAttribute("data-ruvane-layouts")!==raw||root.getAttribute("data-ruvane-timeout")!==rawTime||root.getAttribute("data-ruvane-revision")!==runtime.applicationRevision||root.getAttribute("data-ruvane-module")!==moduleURL||target()!==url||runtime.actionState().state!=="active"||runtime.actionState().pending||runtime.actionState().uncertain)throw failure();
    const outlet=root.querySelector("#ruvane-outlet"),status=root.querySelector("#ruvane-status");
    if(root.querySelectorAll("#ruvane-outlet").length>1||root.querySelectorAll("#ruvane-status").length>1)throw failure();
    if(shellNames!==null){
      const hosts=[...root.querySelectorAll("[data-ruvane-shell]")];
      if(hosts.length>names.length||!hosts.every((host,n)=>host.getAttribute("data-ruvane-shell")===names[n]&&!outlet?.contains(host)&&!host.contains(outlet)&&!host.parentElement?.closest("[data-ruvane-shell]")&&document.querySelectorAll(`[data-ruvane-shell="${names[n]}"]`).length===1))throw failure();
      for(const row of shellRows)if(row.host.parentNode!==row.parent||row.host.localName!=="div"||row.host.attributes.length!==1||row.host.getAttribute("data-ruvane-shell")!==row.name||row.host.childNodes.length!==1||row.host.firstChild!==row.root||!root.contains(row.host)||outlet?.contains(row.host))throw failure();
      runtime.validateShells(shellRows.map(row=>row.controller),url);
    }
    return{outlet,status};
  }
  function pump(){
    queued=false;if(state==="ready"){try{shape();}catch{retire();}return;}if(state!=="waiting")return;
    try{
      const {outlet,status}=shape();
      if(!outlet||!status){if(document.readyState!=="loading")throw failure();return;}
      if(shellNames!==null){
        for(const host of root.querySelectorAll("[data-ruvane-shell]")){
          if(shellRows.some(row=>row.host===host))continue;
          const name=host.getAttribute("data-ruvane-shell"),marker=host.nextSibling;
          if(!marker){if(document.readyState!=="loading")throw failure();break;}
          if(name!==names[shellRows.length]||host.localName!=="div"||host.attributes.length!==1||host.childNodes.length!==1||marker.localName!=="br"||marker.attributes.length!==2||!marker.hasAttribute("hidden")||marker.getAttribute("data-ruvane-shell-ready")!==name||marker.childNodes.length)throw failure();
          const view=host.firstChild,controller=runtime.hydrateShell(view,name,url);marker.remove();shellRows.push({name,host,root:view,parent:host.parentNode,controller});
          status.textContent="Shell ready. Loading page…";
        }
        const markers=[...root.querySelectorAll("[data-ruvane-shell-ready]")];
        if(markers.length>names.length-shellRows.length||markers.some(node=>!names.slice(shellRows.length).includes(node.getAttribute("data-ruvane-shell-ready"))))throw failure();
      }
      let added=false;
      for(const node of root.querySelectorAll("[data-ruvane-segment]")){
        if(!earlyLayouts)throw failure();
        if(rows.length>=expected)throw failure();
        const own=node.parentElement,host=own?.parentElement,slot=rows.at(-1)?.outlet??outlet,child=host?.lastChild;
        if(!node.querySelector("[data-ruvane-ready]")||host?.childNodes.length===1){if(document.readyState!=="loading")throw failure();break;}
        const snapshot=readLayoutSnapshot(root,node),page=snapshot.page,descriptor=page.layouts.at(-1);
        if(page.pending!==expected||page.layouts.length!==rows.length+1||title!==null&&title!==page.title||!page.layouts.slice(0,-1).every((item,n)=>sameDescriptor(item,rows[n].descriptor)))throw failure();
        if(slot.firstChild!==host||host.localName!=="section"||host.attributes.length!==1||host.getAttribute("data-ruvane-layout")!==descriptor.name||host.childNodes.length!==2||host.firstChild!==own||own.localName!=="div"||own.attributes.length!==1||!own.hasAttribute("data-ruvane-layout-view")||own.childNodes.length!==2||own.lastChild!==node||child.localName!=="div"||child.attributes.length!==1||!child.hasAttribute("data-ruvane-child-outlet"))throw failure();
        title=page.title;node.remove();
        const controller=runtime.hydrateLayout(own.firstChild,snapshot.bytes,rows.length+1);rows.push({descriptor,host,own,outlet:child,controller});added=true;
      }
      if(added)status.textContent="Layouts ready. Loading page…";
      if(document.readyState!=="loading"){
        const snapshot=readInitialSnapshot(root),page=decodeNavigationSnapshot(snapshot).page;
        const retired=readRetiredBranch(root,page),shorter=rows.length<expected&&page.failed===rows.length;
        if(!earlyLayouts||!rows.length&&page.failed===0){const declaration=clientDeclaration(readBoundaries(root),page.url);if(!declaration||declaration.chain.length!==expected)throw failure();}
        if(earlyLayouts&&(rows.length!==expected&&!shorter)||title!==null&&title!==page.title||root.querySelector("[data-ruvane-segment]")||shellRows.length!==names.length||shellNames!==null&&root.querySelector("[data-ruvane-shell-ready]"))throw failure();
        if(retired){
          if(retired.page.pending!==expected||earlyLayouts&&(rows.length!==page.failed||!retired.page.layouts.every((item,n)=>sameDescriptor(item,rows[n].descriptor))))throw failure();
          runtime.validatePage(retired.bytes);
        }else if(earlyLayouts&&(page.layouts.length!==rows.length||!page.layouts.every((item,n)=>sameDescriptor(item,rows[n].descriptor))))throw failure();
        runtime.validatePage(decodeNavigationSnapshot(snapshot).bytes);
        snapshotBytes=decodeNavigationSnapshot(snapshot).bytes;state="ready";resolve(Object.freeze({snapshot,shells:Object.freeze(shellRows.map(row=>Object.freeze({root:row.root,controller:row.controller}))),parents:Object.freeze(rows.map(row=>row.controller)),accept(){if(state!=="ready")return false;try{shape();return true;}catch{retire();return false;}}}));
      }
    }catch{retire();}
  }
  function schedule(){if(!["waiting","ready"].includes(state)||queued)return;queued=true;queueMicrotask(pump);}
  observer.observe(document.documentElement,{subtree:true,childList:true,characterData:true,attributes:true});document.addEventListener("readystatechange",schedule);window.addEventListener("pagehide",leave,{once:true});window.addEventListener("popstate",leave);schedule();
  return Object.freeze({ready,retire,commit(){if(state!=="ready")throw failure();shape();if(shellNames!==null)runtime.commitShells(shellRows.map(row=>row.controller),snapshotBytes);state="committed";cleanup(true);rows.length=0;shellRows.length=0;}});
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
  let metadataManaged=!!document.head.querySelector("[data-ruvane-head]");
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
  function cookiesChanged(response, keep=null) {
    if(response.headers.get("ruvane-cache-reset")===null)return false;
    generation = generation === null || generation === Number.MAX_SAFE_INTEGER ? null : generation + 1;
    cache.clear();bytesHeld=0;cancelJobs("invalidated");
    if(pending!==keep)cancelNavigation("invalidated");
    clearFeedback();lastFailure=null;
    return true;
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
      if(cookiesChanged(response))return;
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
    const parts=pathParts(url),page=clientDeclaration(presentation,url);if(!page)return null;
    let declaration=page.loading?page:null,depth=page.chain.length;
    if(!declaration)for(let n=page.chain.length-1;n>=0;n--){const parent=layoutPresentation.get(page.chain[n]);if(parent.loading){declaration=parent;depth=n+1;break;}}
    if(!declaration){declaration=presentation.find(item=>item.kind==="app");depth=0;}
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
  function run(url,mode,point={x:0,y:0},accept,fresh=false,resetLayouts=false,preserveLayoutFocus=false) {
    if (disposed) return Promise.reject(new Error("Ruvane navigator disposed")); safeURL(url);
    if (mode !== "start" && !started) return Promise.reject(failure());
    cancelNavigation("superseded"); cancelJobs("superseded");clearFeedback();lastFailure=null;
    const operation = { url, mode, abort:new AbortController(), expired:false, updated:false, reason:null, committing:false, changed:false }; pending = operation;
    let version = generation;const epoch = runtime.actionState().epoch;
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
          const current = !disposed && pending===operation && !operation.abort.signal.aborted && version===generation && epoch===runtime.actionState().epoch;
          if(cookiesChanged(response,current?operation:null)&&current){version=generation;resetLayouts=true;}
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
        const keepFocus=preserveLayoutFocus&&(source.slice(0,shared).some(item=>item.own.contains(document.activeElement))||scope.contains(document.activeElement)&&!outlet.contains(document.activeElement));
        if(keepFocus)point={x:scrollX,y:scrollY};
        const commitHead=preparePageHead(page,metadataManaged);
        operation.committing = true;
        if (mode === "push") { save(); history.pushState({ [KEY]:entry(url,0,0) },"",url); }
        else if (mode === "start" || mode === "replace") history.replaceState({ ...stateObject(history.state), [KEY]:entry(mode === "start" ? url+location.hash : url,point.x,point.y) },"",mode === "start" ? location.href : url);
        const retained=new Set(source.slice(0,shared).map(item=>item.controller)),retired=new Set();
        operation.changed=true;current?.dispose();
        for(const item of [...previousBranch,...source].reverse())if(!retained.has(item.controller)&&!retired.has(item.controller)){item.controller.dispose();retired.add(item.controller);}
        mountPoint.replaceChildren(fragment); branch=source.slice(0,shared).concat(preparedLayouts);preparedLayouts=[];current=prepared;prepared=null;operation.preview=null;pageHost=host; activeURL = url; started = true; history.scrollRestoration = "manual";
        commitHead();metadataManaged ||= page.metadata!==undefined; status.textContent = page.status === 200 ? "Ready." : `Page ${page.status}.`;
        if (mode !== "start"&&!keepFocus) host.focus({ preventScroll:true }); restore(point);
        if (!cached) store(url,bytes,expires,version,epoch);
        if(page.failed!==null){lastFailure={url,mode:"replace",point:{x:scrollX,y:scrollY},kind:"server",accept:null};showFeedback(url,"error",false);}
        return "success";
      } catch {
        if(pending===operation){clearFeedback();rollbackPreview(operation);}
        remove(url); try { prepared?.dispose(); releaseBranch(preparedLayouts); } catch { fallback(url); }
        if (disposed || pending !== operation) return disposed ? "disposed" : "superseded";
        if (operation.reason && !operation.updated && runtime.actionState().state === "active") { status.textContent = "Data changed. Try the link again."; if (mode === "pop") fallback(url); return operation.reason; }
        status.textContent = operation.updated ? "Application updated. Reload to continue." : operation.expired ? "Navigation timed out. Try the link again." : "Navigation failed. Try the link again or reload.";
        if(preserveLayoutFocus&&!operation.changed&&activeURL===target())operation.committing=false;
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
    const attempt=lastFailure,preserve=started&&attempt.mode==="replace"&&attempt.url===activeURL&&activeURL===target();
    invalidate(attempt.url);return run(attempt.url,attempt.mode,preserve?{x:scrollX,y:scrollY}:attempt.point,attempt.accept,true,false,preserve);
  }
  function initialDOM(page,retired=null){
    if(!outlet.isConnected||!scope.contains(outlet))throw failure();
    const documentRoot=outlet.closest("#ruvane-application");
    if(page.failed!==null&&!page.layouts.length&&documentRoot){
      const declarations=readBoundaries(documentRoot),declaration=clientDeclaration(declarations,page.url);
      const localZero=declaration?.chain.length===0&&page.failed===0&&declaration.error===page.component;
      if(!declaration||!localZero&&!declarations.some(item=>item.kind==="app"&&item.error===page.component))throw failure();
      if(documentRoot.getAttribute("data-ruvane-document")==="segments"&&documentRoot.getAttribute("data-ruvane-layouts")!==String(declaration.chain.length))throw failure();
    }
    const fallback=outlet.lastChild,layouts=[],roots=[],retiringRoots=[];
    if(outlet.childNodes.length!==(retired&&!page.layouts.length?3:2)||fallback.nodeType!==1||fallback.localName!=="div"||fallback.id!=="ruvane-fallback"||fallback.attributes.length!==1||outlet.querySelectorAll("#ruvane-fallback").length!==1)throw failure();
    function layout(host,descriptor){
      const own=host?.firstChild,child=host?.lastChild;
      if(host?.nodeType!==1||host.localName!=="section"||host.attributes.length!==1||host.getAttribute("data-ruvane-layout")!==descriptor.name||host.childNodes.length!==2||own.localName!=="div"||own.attributes.length!==1||!own.hasAttribute("data-ruvane-layout-view")||own.childNodes.length!==1||child.localName!=="div"||child.attributes.length!==1||!child.hasAttribute("data-ruvane-child-outlet"))throw failure();
      return{descriptor,host,own,outlet:child};
    }
    let slot=outlet;
    for(const descriptor of page.layouts){
      if(slot.childNodes.length!==(slot===outlet?2:1))throw failure();
      const row=layout(slot.firstChild,descriptor);layouts.push(row);roots.push(row.own.firstChild);slot=row.outlet;
    }
    let departing=null,host=slot.firstChild;
    if(retired){
      if(slot.childNodes.length!==(slot===outlet?3:2))throw failure();
      departing=host;host=departing.nextSibling;let child=departing;
      for(const descriptor of retired.page.layouts.slice(page.layouts.length)){
        const row=layout(child,descriptor);retiringRoots.push(row.own.firstChild);
        const last=retiringRoots.length===retired.page.layouts.length-page.layouts.length;
        if(row.outlet.childNodes.length!==(last?0:1))throw failure();child=row.outlet.firstChild;
      }
    }else if(slot.childNodes.length!==(slot===outlet?2:1))throw failure();
    if(host?.nodeType!==1||host.localName!=="div"||host.attributes.length!==2||!host.hasAttribute("data-ruvane-page")||host.getAttribute("tabindex")!=="-1"||host.childNodes.length!==1)throw failure();
    roots.push(host.firstChild);return{layouts,roots,host,fallback,departing,retiringRoots};
  }
  function sameInitialDOM(a,b){
    return a.host===b.host&&a.fallback===b.fallback&&a.departing===b.departing&&a.roots.length===b.roots.length&&a.roots.every((root,n)=>root===b.roots[n])&&a.layouts.every((row,n)=>row.host===b.layouts[n]?.host&&row.own===b.layouts[n].own&&row.outlet===b.layouts[n].outlet)&&a.retiringRoots.length===b.retiringRoots.length&&a.retiringRoots.every((root,n)=>root===b.retiringRoots[n]);
  }
  function initialize(snapshotBytes,accept,parents=[],shells=[]){
    let transaction=null;
    try{
      const {revision,bytes,page}=decodeNavigationSnapshot(snapshotBytes),epoch=runtime.actionState().epoch;
      if(revision!==runtime.applicationRevision||page.url!==target()||runtime.actionState().pending||runtime.actionState().uncertain||typeof runtime.preparePageHydration!=="function")throw failure();
      runtime.validatePage(bytes);
      const checkShells=()=>{if(shells.length){const roots=shells.map(row=>{if(!(row?.root instanceof HTMLElement)||!scope.contains(row.root)||outlet.contains(row.root))throw failure();return row.root;});runtime.validateShells(shells.map(row=>row.controller),page.url,roots);}};
      checkShells();
      if(accept&&!accept()){status.textContent="Native forms are ready.";return "native";}
      if(disposed||pending||started)throw failure();
      const root=outlet.closest("#ruvane-application")??scope,retired=readRetiredBranch(root,page);
      if(retired)runtime.validatePage(retired.bytes);
      const server=initialDOM(page,retired);transaction=runtime.preparePageHydration(server.roots,bytes,parents,retired?{bytes:retired.bytes,roots:server.retiringRoots}:null);
      const controllers=transaction.activate(),afterRetired=readRetiredBranch(root,page),after=initialDOM(page,afterRetired);
      if(retired&&(afterRetired?.node!==retired.node||afterRetired.text!==retired.text)||!sameInitialDOM(server,after)||page.url!==target()||runtime.actionState().epoch!==epoch||accept&&!accept()||disposed||pending||started)throw failure();
      history.replaceState({...stateObject(history.state),[KEY]:entry(target()+location.hash,scrollX,scrollY)},"",location.href);
      if(disposed||pending||started||page.url!==target()||runtime.actionState().epoch!==epoch||runtime.actionState().pending||runtime.actionState().uncertain)throw failure();
      if(accept&&!accept())throw failure();
      checkShells();
      const finalRetired=readRetiredBranch(root,page);
      if(retired&&(finalRetired?.node!==retired.node||finalRetired.text!==retired.text)||!sameInitialDOM(server,initialDOM(page,finalRetired))||disposed||pending||started||page.url!==target()||runtime.actionState().state!=="active"||runtime.actionState().epoch!==epoch||runtime.actionState().pending||runtime.actionState().uncertain)throw failure();
      const focusRemoved=server.departing?.contains(document.activeElement);
      const commitHead=preparePageHead(page,metadataManaged);
      transaction.commit();transaction=null;server.departing?.remove();server.fallback.remove();retired?.node.remove();
      if(focusRemoved)server.host.focus({preventScroll:true});
      branch=server.layouts.map((item,n)=>({...item,controller:controllers[n]}));current=controllers.at(-1);pageHost=server.host;activeURL=page.url;started=true;history.scrollRestoration="manual";
      commitHead();metadataManaged ||= page.metadata!==undefined;
      status.textContent=page.status===200?"Ready.":`Page ${page.status}.`;
      if(page.failed!==null){lastFailure={url:page.url,mode:"replace",point:{x:scrollX,y:scrollY},kind:"server",accept:null};showFeedback(page.url,"error",false);}
      return "success";
    }catch{
      transaction?.rollback();status.textContent="Interactive mode is unavailable. Native links and forms are ready.";return "native";
    }
  }
  scope.addEventListener("click",click); scope.addEventListener("pointerover",intend); scope.addEventListener("focusin",intend); window.addEventListener("popstate",pop); window.addEventListener("scroll",scroll,{passive:true});
  return Object.freeze({
    start({accept,snapshot,parents=[],shells=[]}={}) { if (started || pending || disposed || accept !== undefined && typeof accept !== "function" || !Array.isArray(parents) || parents.length>6 || !Array.isArray(shells) || shells.length>8 || snapshot===undefined&&(parents.length||shells.length)) throw failure(); if(snapshot!==undefined){idle=Promise.resolve(initialize(snapshot,accept,parents,shells));return idle;} return run(target(),"start",{x:scrollX,y:scrollY},accept); },
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

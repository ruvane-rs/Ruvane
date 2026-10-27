const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const check=(ok,message)=>{if(!ok)throw Error(message);};
async function until(fn){for(let n=0;n<300;n++){if(fn())return;await pause(20);}throw Error("Browser state did not settle");}
const originalFetch=globalThis.fetch.bind(globalThis);
const response=await originalFetch("/session",{cache:"no-store"}),parsed=new DOMParser().parseFromString(await response.text(),"text/html");
for(const script of parsed.querySelectorAll("script"))script.remove();
history.replaceState(null,"","/session");document.head.replaceChildren(...parsed.head.childNodes);document.body.replaceChildren(...parsed.body.childNodes);
const root=document.getElementById("ruvane-application"),outlet=document.getElementById("ruvane-outlet"),moduleURL=root.getAttribute("data-ruvane-module"),revision=root.getAttribute("data-ruvane-revision");
const bridge=await import(new URL("./bridge.mjs",new URL(moduleURL,location.href))),nav=await import(new URL("./navigation.mjs",new URL(moduleURL,location.href)));
const runtime=await bridge.createRuntime(moduleURL,{applicationRevision:revision,csrfEndpoint:root.getAttribute("data-ruvane-csrf"),actionTimeoutMs:500});
for(const element of root.querySelectorAll('[data-ruvane-version]'))if(!outlet.contains(element))runtime.hydrate(element);
const navigator=nav.createNavigator(runtime,{outlet,scope:root,status:document.getElementById("ruvane-status"),boundaries:nav.readBoundaries(root),progressive:true});
await navigator.start({snapshot:nav.readInitialSnapshot(root)});root.querySelector("#ruvane-initial").remove();
window.addEventListener("pagehide",()=>{navigator.dispose();runtime.dispose();},{once:true});
const panel=document.createElement("section"),heading=document.createElement("h1"),summary=document.createElement("p"),list=document.createElement("ol"),report=document.createElement("pre");
heading.textContent="Ruvane alpha.49 — Session CSRF";summary.id="summary";summary.textContent="Running browser checks…";report.id="report";report.hidden=true;panel.append(heading,summary,list,report);document.body.prepend(panel);
const rows=[];async function test(name,fn){const item=document.createElement("li");list.append(item);try{await fn();rows.push({name,passed:true});item.textContent="PASS — "+name;}catch(error){rows.push({name,passed:false,error:String(error)});item.textContent="FAIL — "+name+": "+error;}report.textContent=JSON.stringify(rows,null,2);}
let posts=0,gets=0,preflight=null,transportValid=true;
globalThis.fetch=async (url,options={})=>{
  if(options.method==="POST") {
    posts++;transportValid&&=options.mode==="same-origin"&&options.credentials==="same-origin"&&options.cache==="no-store"&&options.redirect==="error";
    if(options.headers[bridge.CSRF_HEADER])transportValid&&=/^[a-f0-9]{64}$/.test(options.headers[bridge.CSRF_HEADER])&&!options.body.includes(options.headers[bridge.CSRF_HEADER]);
  }
  if(url==="/csrf-token") {
    gets++;transportValid&&=options.method==="GET"&&options.mode==="same-origin"&&options.credentials==="same-origin"&&options.cache==="no-store"&&options.redirect==="error"&&options.headers["Ruvane-Application-Revision"]===revision;
    if(preflight)return preflight(options);
  }
  return originalFetch(url,options);
};
const calls=async()=>Number(await(await originalFetch("/csrf-stats",{cache:"no-store"})).text());
async function click(label){const button=[...outlet.querySelectorAll("button")].find(node=>node.textContent===label);check(button&&!button.disabled,"action button ready");button.click();await until(()=>runtime.actionState().pending===0);check(runtime.actionState().state==="active","runtime remains active");}
const token=async()=>{const response=await originalFetch("/csrf-token",{cache:"no-store"});return response.headers.get(bridge.CSRF_HEADER);};
const rawPost=async(mode,claim,body)=>originalFetch("/session-action",{method:"POST",headers:{"Content-Type":"application/x-www-form-urlencoded","Ruvane-Application-Revision":revision,...(claim?{[bridge.CSRF_HEADER]:claim}:{})},body:body??`mode=${mode}`});
await test("initial SSR hydrates and automatic preflight opens a public demo session",async()=>{
  check(outlet.textContent.includes("Anonymous"),"anonymous initial state");const before=await calls();await click("Open demo session");check(outlet.textContent.includes("Session active"),"session opened");check(posts===1&&gets===1&&(await calls())===before+1,"one token fetch then one POST");check(!document.cookie.includes("ruvane_alpha49_session="),"session identifier remains HttpOnly");check(!runtime.actionState().uncertain,"known success");
});
const oldToken=await token();
await test("each Wasm action fetches again and rotation retires the previous form token",async()=>{
  const before=await calls();await click("Rotate session");check(gets===2&&posts===2&&(await calls())===before+1,"fresh fetch for rotation");const current=await token();check(current!==oldToken,"token rotated");const stale=await rawPost("rotate",null,`mode=rotate&_ruvane_csrf=${oldToken}`);check(stale.status===403,"old native form rejected");await stale.arrayBuffer();check((await calls())===before+1,"stale form did not reach handler");
});
await test("two independent same-origin fetches share a reusable generation token",async()=>{
  const [first,second]=await Promise.all([token(),token()]);check(first===second&&/^[a-f0-9]{64}$/.test(first),"stable token for both tabs");
});
const tokenResponse=(changes={},body="")=>new Response(body,{status:changes.status??200,headers:{"Content-Type":bridge.CSRF_MEDIA_TYPE,"Cache-Control":"no-store","Content-Length":"0",[bridge.CSRF_HEADER]:"a".repeat(64),...(changes.headers??{})}});
for(const [name,handler] of [
  ["HTTP failure",()=>tokenResponse({status:503})],
  ["wrong MIME",()=>tokenResponse({headers:{"Content-Type":"text/plain"}})],
  ["public cache directive",()=>tokenResponse({headers:{"Cache-Control":"public, max-age=60"}})],
  ["nonzero declared body",()=>tokenResponse({headers:{"Content-Length":"1"}})],
  ["missing declared length",()=>new Response("",{headers:{"Content-Type":bridge.CSRF_MEDIA_TYPE,"Cache-Control":"no-store"}})],
  ["malformed token",()=>tokenResponse({headers:{[bridge.CSRF_HEADER]:"forged"}})],
  ["duplicate token value",()=>tokenResponse({headers:{[bridge.CSRF_HEADER]:"a".repeat(64)+", "+"b".repeat(64)}})],
  ["unexpected body",()=>tokenResponse({},"x")],
  ["network failure",()=>Promise.reject(Error("fixture connection failure"))],
  ["timeout",options=>new Promise((_,reject)=>options.signal.addEventListener("abort",()=>reject(Error("fixture aborted")),{once:true}))],
])await test(`preflight ${name} sends no POST and leaves no uncertain mutation`,async()=>{
  const before=await calls(),sent=posts;preflight=handler;try{await click("Rotate session");}finally{preflight=null;}
  check(posts===sent&&(await calls())===before,"no mutation sent");check(!runtime.actionState().uncertain,"no uncertain mutation");
});
await test("a later valid preflight permits action after rejected attempts",async()=>{
  const before=await calls();await click("Rotate session");check((await calls())===before+1,"rotation resumed");check(!runtime.actionState().uncertain&&transportValid,"transport restrictions retained");
});
await test("private navigation keeps token out of snapshots and shared caches",async()=>{
  check(await navigator.navigate("/session-private")==="success","private navigation");check(outlet.textContent.includes("Private demo content"),"private result");check(navigator.stats().cacheEntries===0,"no browser cache entry");
  const claim=await token(),response=await originalFetch("/session",{headers:{Accept:bridge.NAVIGATION_MEDIA_TYPE,"Ruvane-Application-Revision":revision},cache:"no-store"});const body=await response.text();check(!body.includes(claim),"token absent from packet");
  check(await navigator.navigate("/session")==="success","controls restored");
});
await test("native hidden field submits through 303 and stale field is denied",async()=>{
  const claim=await token();let response=await originalFetch("/session-native",{method:"POST",headers:{"Content-Type":"application/x-www-form-urlencoded"},body:`mode=rotate&_ruvane_csrf=${claim}`});check(response.status===200&&response.redirected,"native 303 followed");await response.arrayBuffer();
  response=await originalFetch("/session-native",{method:"POST",headers:{"Content-Type":"application/x-www-form-urlencoded"},body:`mode=rotate&_ruvane_csrf=${claim}`});check(response.status===403,"old native form denied");await response.arrayBuffer();
});
await test("Wasm refetches after native rotation and logout invalidates the token",async()=>{
  const claim=await token();await click("Close session");check(outlet.textContent.includes("Anonymous"),"logout delivered");check(await token()===null,"no live token");const response=await rawPost("rotate",claim);check(response.status===403,"retired token denied");await response.arrayBuffer();check(!runtime.actionState().uncertain,"known logout outcome");
});
globalThis.fetch=originalFetch;
summary.textContent=`${rows.filter(row=>row.passed).length}/${rows.length} checks passed`;
report.textContent=JSON.stringify({revision,rows,posts,gets,transportValid,actions:runtime.actionState(),cache:navigator.stats()},null,2);document.documentElement.dataset.checksComplete="true";

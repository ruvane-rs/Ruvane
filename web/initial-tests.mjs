const report=document.getElementById("initial-report"),summary=document.getElementById("initial-summary"),results=[];
const assert=(value,message)=>{if(!value)throw new Error(message);};
const equal=(a,b,message)=>assert(JSON.stringify(a)===JSON.stringify(b),`${message}: ${JSON.stringify(a)}`);
async function test(name,fn){try{await fn();results.push({name,passed:true});}catch(error){results.push({name,passed:false,error:String(error.message)});}report.textContent=JSON.stringify(results,null,2);}
const nativeFetch=window.fetch;
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(check){const end=performance.now()+5000;while(!check()){if(performance.now()>end)throw new Error("Initial hydration check timed out");await wait(5);}}
const source=async url=>new DOMParser().parseFromString(await(await nativeFetch(url)).text(),"text/html").getElementById("ruvane-application");
const configuration=await source("/"),moduleURL=configuration.getAttribute("data-ruvane-module"),revision=configuration.getAttribute("data-ruvane-revision");
const {createRuntime}=await import(new URL("./bridge.mjs",new URL(moduleURL,location.href)));
const {createNavigator,readBoundaries,readInitialSnapshot}=await import(new URL("./navigation.mjs",new URL(moduleURL,location.href)));
let serial=0;
function insert(template,url){history.replaceState(null,"",url);const root=document.importNode(template,true);for(const script of root.querySelectorAll("script"))script.remove();document.body.append(root);return root;}
function references(root){const nodes=[];function visit(node){nodes.push(node);for(const child of node.childNodes)visit(child);}visit(root);return nodes;}
function sameNodes(before,root){equal(references(root).length,before.length,"same node count");assert(references(root).every((node,n)=>node===before[n]),"same element and text nodes");}
async function setup(url="/tasks",options={}){
  const root=insert(await source(url),url),outlet=root.querySelector("#ruvane-outlet"),status=root.querySelector("#ruvane-status"),runtime=await createRuntime(moduleURL,{applicationRevision:revision});
  const shell=root.querySelector("[data-ruvane-version]"),counter=runtime.hydrate(shell),navigator=createNavigator(runtime,{scope:root,outlet,status,boundaries:readBoundaries(root),progressive:true,...options});
  let reads=0,posts=0;
  window.fetch=(url,options={})=>{if(options.method==="POST"){posts++;throw new Error("Unexpected activation POST");}reads++;throw new Error("Unexpected activation GET");};
  return{root,outlet,status,runtime,counter,navigator,reads:()=>reads,posts:()=>posts,packet:()=>readInitialSnapshot(root),page:()=>outlet.querySelector("[data-ruvane-page]"),close(){window.fetch=nativeFetch;navigator.dispose();runtime.dispose();root.remove();}};
}
const own=layout=>layout.querySelector(":scope > [data-ruvane-layout-view]");
const layout=ctx=>ctx.outlet.querySelector('[data-ruvane-layout="workspace"]');
async function createTask(title){await nativeFetch("/tasks",{method:"POST",headers:{"Content-Type":"application/x-www-form-urlencoded"},body:new URLSearchParams({title,notes:"Initial snapshot fixture"})});const root=await source("/tasks");return [...root.querySelectorAll('a[data-ruvane-nav]')].find(a=>/^\/tasks\/[1-9][0-9]*$/.test(a.getAttribute("href"))&&a.closest("li")?.textContent.includes(title))?.getAttribute("href");}
async function removeTask(url){await nativeFetch(url+"/remove",{method:"POST",headers:{"Content-Type":"application/x-www-form-urlencoded"},body:""});}

await test("the real application bootstrap adopts server nodes without another page GET",async()=>{
  const url="/?fixture=bootstrap",root=insert(await source(url),url),outlet=root.querySelector("#ruvane-outlet"),page=outlet.querySelector("[data-ruvane-page]"),before=references(page),fallback=root.querySelector("#ruvane-fallback");let dataReads=0,posts=0;
  window.fetch=(url,options={})=>{if(options.method==="POST"){posts++;throw new Error("Unexpected POST");}if(!String(url).includes("/ruvane-runtime/")){dataReads++;throw new Error("Unexpected data GET");}return nativeFetch(url,options);};
  try{await import(new URL(`./application.mjs?initial=${++serial}`,new URL(moduleURL,location.href)));equal(root.querySelector("#ruvane-status").textContent,"Ready.","bootstrap ready");sameNodes(before,page);assert(!fallback.isConnected&&!root.querySelector("#ruvane-initial"),"inert metadata and pristine fallback retired");equal(dataReads,0,"no second page GET");equal(posts,0,"no activation POST");}finally{window.dispatchEvent(new Event("pagehide"));window.fetch=nativeFetch;root.remove();}
});
await test("complete page and layout hydration preserve every server element and text node",async()=>{
  for(const url of ["/","/tasks","/tasks/project"]){const ctx=await setup(url);try{const page=ctx.page(),before=references(page),parent=layout(ctx),parentNodes=parent?references(own(parent)):null;equal(await ctx.navigator.start({snapshot:ctx.packet()}),"success","activated");sameNodes(before,page);if(parent)sameNodes(parentNodes,own(parent));equal(ctx.navigator.stats().layouts,url==="/"?0:1,"adopted layout count");equal(ctx.reads(),0,"no reads");equal(ctx.posts(),0,"no writes");equal(ctx.navigator.stats().cacheEntries,0,"document snapshot is no-store");}finally{ctx.close();}}
});
await test("the real bootstrap preserves SSR and native drafts when metadata is corrupt",async()=>{
  const url="/tasks?fixture=bad-bootstrap",root=insert(await source(url),url),outlet=root.querySelector("#ruvane-outlet"),input=root.querySelector("#ruvane-fallback input"),before=references(outlet);let reads=0;input.value="Native draft";input.focus();root.querySelector("#ruvane-initial").textContent="not-a-snapshot";window.fetch=()=>{reads++;throw new Error("Unexpected recovery fetch");};
  try{await import(new URL(`./application.mjs?initial=${++serial}`,new URL(moduleURL,location.href)));sameNodes(before,outlet);assert(input.value==="Native draft"&&document.activeElement===input,"draft retained");assert(root.querySelector("#ruvane-status").textContent.includes("Native links and forms"),"native recovery");equal(reads,0,"no implicit loader or module fetch");}finally{window.dispatchEvent(new Event("pagehide"));window.fetch=nativeFetch;root.remove();}
});
await test("leaving the document while Wasm loads prevents late activation",async()=>{
  const url="/tasks?fixture=retired",root=insert(await source(url),url),outlet=root.querySelector("#ruvane-outlet"),before=references(outlet);let release,reads=0;
  window.fetch=(url,options)=>{if(String(url).endsWith("app.wasm"))return new Promise(resolve=>{release=()=>resolve(nativeFetch(url,options));});reads++;throw new Error("Unexpected data request");};
  try{const boot=import(new URL(`./application.mjs?initial=${++serial}`,new URL(moduleURL,location.href)));await until(()=>release);window.dispatchEvent(new Event("pagehide"));release();await boot;sameNodes(before,outlet);assert(root.querySelector("button").disabled,"shell never activated");equal(reads,0,"no data fetch");}finally{window.fetch=nativeFetch;root.remove();}
});
await test("early text, textarea values, focus and UTF-16 selection survive activation",async()=>{
  const ctx=await setup();try{const input=ctx.page().querySelector('input[type="text"]'),notes=ctx.page().querySelector("textarea");input.value="Early 🦀 <script>";notes.value="Written before activation\nSecond line";input.focus();input.setSelectionRange(2,9,"backward");const before=references(ctx.page());equal(await ctx.navigator.start({snapshot:ctx.packet()}),"success","activated");sameNodes(before,ctx.page());equal(input.value,"Early 🦀 <script>","text adopted");equal(notes.value,"Written before activation\nSecond line","textarea adopted");assert(document.activeElement===input,"same focused field");equal([input.selectionStart,input.selectionEnd,input.selectionDirection],[2,9,"backward"],"selection preserved");equal(ctx.posts(),0,"no form submission");}finally{ctx.close();}
});
await test("an oversized UTF-8 draft is preserved by declining activation",async()=>{
  const ctx=await setup();try{const input=ctx.page().querySelector("input"),draft="🦀".repeat(257),stats=ctx.runtime.stats();input.value=draft;input.focus();equal(await ctx.navigator.start({snapshot:ctx.packet()}),"native","oversized input refused");assert(input.value===draft&&document.activeElement===input,"draft and focus retained");equal(ctx.runtime.stats(),stats,"no retained page owners");equal(ctx.posts(),0,"no POST");}finally{ctx.close();}
});
await test("dirty native fallback forms can decline activation without data requests",async()=>{
  const ctx=await setup();try{const fallback=ctx.root.querySelector("#ruvane-fallback"),input=fallback.querySelector("input"),before=references(ctx.outlet),stats=ctx.runtime.stats();input.value="Native draft";input.focus();equal(await ctx.navigator.start({snapshot:ctx.packet(),accept:()=>input.value===input.defaultValue}),"native","native guard");sameNodes(before,ctx.outlet);assert(fallback.isConnected&&input.value==="Native draft"&&document.activeElement===input,"native form retained");equal(ctx.runtime.stats(),stats,"no page owners");equal(ctx.reads(),0,"no reads");}finally{ctx.close();}
});
await test("a late DOM mismatch leaves all prepared parents inert and the shell usable",async()=>{
  const ctx=await setup("/tasks/project");try{ctx.page().querySelector("h2").textContent="Changed before activation";const before=references(ctx.outlet),markup=ctx.outlet.innerHTML,stats=ctx.runtime.stats();equal(await ctx.navigator.start({snapshot:ctx.packet()}),"native","mismatch rejected");sameNodes(before,ctx.outlet);equal(ctx.outlet.innerHTML,markup,"server markup preserved");equal(ctx.runtime.stats(),stats,"prepared owners released");ctx.counter.dispatch(1);assert(ctx.root.querySelector("output").textContent==="6","independent shell stays active");}finally{ctx.close();}
});
await test("a failed history commit rolls back enabled controls, adopted values and listeners",async()=>{
  const ctx=await setup(),original=history.replaceState;try{const input=ctx.page().querySelector("input"),button=ctx.page().querySelector("button"),page=ctx.page();input.value="Saved draft 🦀";input.focus();input.setSelectionRange(1,6);const before=references(page),markup=page.innerHTML,stats=ctx.runtime.stats();history.replaceState=()=>{throw new Error("History fixture");};equal(await ctx.navigator.start({snapshot:ctx.packet()}),"native","commit rejected");sameNodes(before,page);equal(page.innerHTML,markup,"attributes and text rolled back");assert(input.value==="Saved draft 🦀"&&document.activeElement===input,"draft and focus retained");equal([input.selectionStart,input.selectionEnd],[1,6],"selection retained");assert(button.disabled,"control inert again");equal(ctx.runtime.stats(),stats,"all temporary owners gone");equal(ctx.posts(),0,"no POST");}finally{history.replaceState=original;ctx.close();}
});
await test("duplicate, executable-looking or oversized metadata is rejected before hydration",async()=>{
  for(const mode of ["duplicate","text","size"]){const ctx=await setup();try{const node=ctx.root.querySelector("#ruvane-initial"),stats=ctx.runtime.stats();if(mode==="duplicate")ctx.root.append(node.cloneNode(true));else node.textContent=mode==="text"?"</div><script>alert(1)</script>":"0".repeat(131106);let rejected=false;try{ctx.packet();}catch{rejected=true;}assert(rejected,"metadata rejected");equal(ctx.runtime.stats(),stats,"no allocations");equal(ctx.reads(),0,"no requests");}finally{ctx.close();}}
});
await test("wrong revisions, destinations, selected packets and pending leaves fail closed",async()=>{
  for(const mode of ["revision","target","selected","pending"]){const ctx=await setup();try{const snapshot=ctx.packet(),stats=ctx.runtime.stats();if(mode==="revision")snapshot[4]^=1;else if(mode==="target")history.replaceState(null,"","/tasks?different=yes");else if(mode==="selected")snapshot[20]=1;else{snapshot[19]=52;snapshot[21]=1;}equal(await ctx.navigator.start({snapshot}),"native","snapshot refused");equal(ctx.runtime.stats(),stats,"no page retained");equal(ctx.reads(),0,"no recovery fetch");}finally{ctx.close();}}
});
await test("invalid final component schemas do not allocate owners or replace SSR",async()=>{
  const ctx=await setup();try{const snapshot=ctx.packet(),view=new DataView(snapshot.buffer);let pos=16+5+2;for(let n=0;n<2;n++)pos+=4+view.getUint32(pos,true);pos+=12+4;snapshot[pos+13]=99;const stats=ctx.runtime.stats(),before=references(ctx.outlet);equal(await ctx.navigator.start({snapshot}),"native","bad seed");sameNodes(before,ctx.outlet);equal(ctx.runtime.stats(),stats,"healthy shell and no page owners");}finally{ctx.close();}
});
await test("view budget rejection leaves the entire server branch and native fallback intact",async()=>{
  const ctx=await setup("/tasks/project");try{for(let n=0;n<14;n++){const host=document.createElement("div");ctx.root.append(host);ctx.runtime.mount(host,{component:1,state:{kind:"number",value:0}});}const stats=ctx.runtime.stats(),before=references(ctx.outlet);equal(await ctx.navigator.start({snapshot:ctx.packet()}),"native","budget rejected");sameNodes(before,ctx.outlet);equal(ctx.runtime.stats(),stats,"existing owners retained");}finally{ctx.close();}
});
await test("disposal inside the activation guard cannot leave orphaned page owners",async()=>{
  const ctx=await setup();try{const stats=ctx.runtime.stats(),before=references(ctx.outlet);equal(await ctx.navigator.start({snapshot:ctx.packet(),accept:()=>{ctx.navigator.dispose();return true;}}),"native","retired activation");sameNodes(before,ctx.outlet);equal(ctx.runtime.stats(),stats,"shell only");equal(ctx.reads(),0,"no reads");}finally{ctx.close();}
});
await test("adopting an early input cannot automatically submit a server action",async()=>{
  const ctx=await setup();try{const input=ctx.page().querySelector("input"),stats=ctx.runtime.stats();input.value="Autosave fixture draft";input.focus();const before=references(ctx.page());equal(await ctx.navigator.start({snapshot:ctx.packet()}),"native","automatic action refused");sameNodes(before,ctx.page());assert(input.value==="Autosave fixture draft"&&document.activeElement===input,"draft retained");equal(ctx.posts(),0,"no POST sent");equal(ctx.runtime.stats(),stats,"temporary owners released and shell healthy");}finally{ctx.close();}
});
await test("initial scoped 500 errors hydrate their validated parents and retry only explicitly",async()=>{
  const ctx=await setup("/tasks/project?unavailable=yes");try{const parent=layout(ctx),before=references(ctx.page());equal(await ctx.navigator.start({snapshot:ctx.packet()}),"success","typed error hydrated");sameNodes(before,ctx.page());assert(layout(ctx)===parent&&ctx.status.textContent==="Page 500.","retained parents and logical status");assert(ctx.navigator.failure()?.kind==="server","explicit retry available");equal(ctx.reads(),0,"no implicit error reload");window.fetch=nativeFetch;const length=history.length;equal(await ctx.navigator.retry(),"success","fresh typed error");equal(history.length,length,"retry replaces entry");assert(layout(ctx)===parent,"parent retained across retry");}finally{ctx.close();}
});
await test("hydrated parents retain state across client navigation and refresh replaces them",async()=>{
  const ctx=await setup();try{equal(await ctx.navigator.start({snapshot:ctx.packet()}),"success","activated");const parent=layout(ctx);own(parent).querySelector("button").click();window.fetch=nativeFetch;equal(await ctx.navigator.navigate("/tasks/settings"),"success","sibling");assert(layout(ctx)===parent,"same server parent");equal(own(parent).querySelector("output").textContent,"1","server parent state retained");equal(await ctx.navigator.refresh(),"success","refresh");assert(layout(ctx)!==parent,"fresh parent");equal(own(layout(ctx)).querySelector("output").textContent,"0","fresh state");}finally{ctx.close();}
});
await test("nested task contexts hydrate from their own typed props and native data",async()=>{
  const title="Hydration fixture 🦀 <script>",url=await createTask(title);assert(url,"fixture created");let ctx;try{ctx=await setup(url);const parents=[...ctx.outlet.querySelectorAll("[data-ruvane-layout]")],before=references(ctx.page());equal(await ctx.navigator.start({snapshot:ctx.packet()}),"success","nested hydration");equal(ctx.navigator.stats().layouts,2,"two layouts");sameNodes(before,ctx.page());assert(ctx.page().textContent.includes(title)&&!ctx.page().querySelector("script"),"typed text remains inert");assert(parents.every((node,n)=>node===[...ctx.outlet.querySelectorAll("[data-ruvane-layout]")][n]),"same nested hosts");}finally{ctx?.close();await removeTask(url);}
});
await test("startup uses the original server snapshot even when data changes before activation",async()=>{
  const ctx=await setup(),title="Created after SSR",url=await createTask(title);try{equal(await ctx.navigator.start({snapshot:ctx.packet()}),"success","original seed hydrated");assert(!ctx.page().textContent.includes(title),"original document seed");equal(ctx.reads(),0,"no repeated load");window.fetch=nativeFetch;equal(await ctx.navigator.refresh(),"success","explicit new read");assert(ctx.outlet.textContent.includes(title),"fresh data after refresh");}finally{ctx.close();await removeTask(url);}
});
await test("one hundred hydration and disposal cycles release page, layout and event owners",async()=>{
  const url="/tasks/project",template=await source(url),runtime=await createRuntime(moduleURL,{applicationRevision:revision}),before=runtime.stats();window.fetch=()=>{throw new Error("Unexpected data load");};
  try{for(let n=0;n<100;n++){const root=insert(template,url),outlet=root.querySelector("#ruvane-outlet"),navigator=createNavigator(runtime,{scope:root,outlet,status:root.querySelector("#ruvane-status"),boundaries:readBoundaries(root)});equal(await navigator.start({snapshot:readInitialSnapshot(root)}),"success","cycle hydration");navigator.dispose();root.remove();equal(runtime.stats(),before,"resource baseline");}}finally{window.fetch=nativeFetch;runtime.dispose();}
});
window.fetch=nativeFetch;
const result={passed:results.filter(row=>row.passed).length,failed:results.filter(row=>!row.passed).length,results};report.textContent=JSON.stringify(result,null,2);summary.textContent=`${result.passed}/${results.length} initial hydration checks passed`;

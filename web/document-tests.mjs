const report=document.getElementById("document-report"),summary=document.getElementById("document-summary"),results=[];
const assert=(value,message)=>{if(!value)throw new Error(message);};
const equal=(a,b,message)=>assert(JSON.stringify(a)===JSON.stringify(b),`${message}: ${JSON.stringify(a)}`);
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(check){const end=performance.now()+55000;while(!check()){if(performance.now()>end)throw new Error("Document check timed out");await wait(5);}}
async function test(name,fn){try{await fn();results.push({name,passed:true});}catch(error){results.push({name,passed:false,error:String(error.message)});}report.textContent=JSON.stringify(results,null,2);}
const nativeFetch=window.fetch,control=path=>nativeFetch(path,{method:"POST",headers:{"Content-Type":"application/x-www-form-urlencoded"},body:""});
const source=async url=>new DOMParser().parseFromString(await(await nativeFetch(url)).text(),"text/html").getElementById("ruvane-application");
const configuration=await source("/tasks/document"),moduleURL=configuration.getAttribute("data-ruvane-module"),revision=configuration.getAttribute("data-ruvane-revision");
const {createRuntime}=await import(new URL("./bridge.mjs",new URL(moduleURL,location.href)));
const {createNavigator,readBoundaries,readInitialSnapshot}=await import(new URL("./navigation.mjs",new URL(moduleURL,location.href)));
const own=layout=>layout.querySelector(":scope > [data-ruvane-layout-view]");
function references(root){const nodes=[];function visit(node){nodes.push(node);for(const child of node.childNodes)visit(child);}visit(root);return nodes;}
function sameNodes(before,root){const after=references(root);assert(after.length===before.length&&after.every((node,n)=>node===before[n]),"same server elements and text nodes");}
function insert(template,url){history.replaceState(null,"",url);const root=document.importNode(template,true);for(const script of root.querySelectorAll("script"))script.remove();document.body.append(root);return root;}
async function setup(url="/tasks/document"){
  const root=insert(await source(url),url),outlet=root.querySelector("#ruvane-outlet"),status=root.querySelector("#ruvane-status"),runtime=await createRuntime(moduleURL,{applicationRevision:revision});
  const navigator=createNavigator(runtime,{scope:root,outlet,status,boundaries:readBoundaries(root),progressive:true});let reads=0;
  window.fetch=()=>{reads++;throw new Error("Unexpected document data request");};
  return{root,outlet,status,runtime,navigator,reads:()=>reads,close(){window.fetch=nativeFetch;navigator.dispose();runtime.dispose();root.remove();}};
}
const run=document.createElement("button"),release=document.createElement("button");run.textContent="Run document checks";release.textContent="Release initial App page";release.disabled=true;document.body.prepend(run,release);
release.addEventListener("click",async()=>{release.disabled=true;await control("/fixtures/release");});
run.addEventListener("click",async()=>{
  run.disabled=true;const child=window.open("about:blank","ruvane-initial-document");if(!child){summary.textContent="The browser blocked the test window.";return;}
  try{
    await control("/fixtures/prepare");child.location.href="/tasks?document-gate=yes";
    let parent,parentNodes,releaseWasm,earlyPage,earlyNodes,input,textarea,reads=0,posts=0;
    await test("a real initial document displays its inert layout before the blocked leaf",async()=>{
      await until(()=>child.document.querySelector('[data-ruvane-layout="workspace"]'));
      parent=child.document.querySelector('[data-ruvane-layout="workspace"]');parentNodes=references(own(parent));
      assert(parent.getBoundingClientRect().height>0&&child.document.readyState==="loading","visible before completion");
      assert(!child.document.querySelector("[data-ruvane-page]")&&!child.document.getElementById("ruvane-initial"),"leaf and snapshot deferred");
      assert(own(parent).querySelector("button").disabled,"layout remains inert");
      const fetch=child.fetch.bind(child);child.fetch=(url,options={})=>{if(String(url).endsWith("app.wasm"))return new Promise(resolve=>{releaseWasm=()=>resolve(fetch(url,options));});if(options.method==="POST")posts++;else reads++;throw new Error("Unexpected activation request");};
      summary.textContent="Initial App layout is visible. Release the page.";release.disabled=false;
    });
    await test("the leaf and completion snapshot arrive before Wasm activation",async()=>{
      await until(()=>releaseWasm&&child.document.getElementById("ruvane-complete"));
      const doc=child.document;earlyPage=doc.querySelector("[data-ruvane-page]");earlyNodes=references(earlyPage);input=earlyPage.querySelector("input");textarea=earlyPage.querySelector("textarea");
      assert(earlyPage&&doc.readyState!=="loading"&&doc.querySelector('[data-ruvane-layout="workspace"]')===parent,"same parent and completed parsing");
      assert(!input.disabled&&!textarea.disabled&&earlyPage.querySelector("button").disabled,"only early text controls editable");
      input.value="Early document draft 🦀";textarea.value="Notes before activation";input.focus();input.setSelectionRange(2,8);releaseWasm();
      await until(()=>doc.getElementById("ruvane-status").textContent==="Ready.");
    });
    await test("the real bootstrap keeps nodes, drafts, focus and selection without another GET",async()=>{
      const doc=child.document;sameNodes(parentNodes,own(parent));sameNodes(earlyNodes,earlyPage);
      assert(doc.querySelector("[data-ruvane-page]")===earlyPage&&doc.activeElement===input,"same page and focus");
      equal([input.value,textarea.value,input.selectionStart,input.selectionEnd],["Early document draft 🦀","Notes before activation",2,8],"early fields");
      equal([reads,posts],[0,0],"no extra GET or POST");assert(!doc.getElementById("ruvane-initial")&&!doc.getElementById("ruvane-complete"),"metadata retired");
      const button=own(parent).querySelector("button");assert(!button.disabled,"layout activated only after completion");button.click();equal(own(parent).querySelector("output").textContent,"1","working shared layout");
    });
  }finally{await control("/fixtures/release");release.disabled=true;child.close();}
  await test("streamed server HTML hydrates the same complete branch transactionally",async()=>{
    const ctx=await setup();try{const page=ctx.outlet.querySelector("[data-ruvane-page]"),parent=ctx.outlet.querySelector("[data-ruvane-layout]"),before=references(page),parents=references(own(parent));equal(await ctx.navigator.start({snapshot:readInitialSnapshot(ctx.root)}),"success","startup");sameNodes(before,page);sameNodes(parents,own(parent));equal(ctx.reads(),0,"no repeated data request");equal(ctx.navigator.stats().cacheEntries,0,"no snapshot caching");}finally{ctx.close();}
  });
  await test("missing, duplicate, moved and nonempty completion markers are rejected",async()=>{
    for(const mode of ["missing","duplicate","moved","text","attribute","order","profile"]){const ctx=await setup();try{const end=ctx.root.querySelector("#ruvane-complete"),stats=ctx.runtime.stats();
      if(mode==="missing")end.remove();else if(mode==="duplicate")ctx.root.append(end.cloneNode());else if(mode==="moved")ctx.outlet.append(end);else if(mode==="text")end.textContent="incomplete";else if(mode==="attribute")end.setAttribute("title","extra");else if(mode==="order")ctx.root.prepend(end);else ctx.root.setAttribute("data-ruvane-document","unknown");
      let rejected=false;try{readInitialSnapshot(ctx.root);}catch{rejected=true;}assert(rejected,`${mode} rejected`);equal(ctx.runtime.stats(),stats,"no allocated page owners");equal(ctx.reads(),0,"no recovery GET");}finally{ctx.close();}}
  });
  await test("incomplete metadata cannot activate the real bootstrap or request Wasm",async()=>{
    const root=insert(await source("/tasks/document"),"/tasks/document"),before=references(root.querySelector("#ruvane-outlet"));root.querySelector("#ruvane-complete").remove();let reads=0;window.fetch=()=>{reads++;throw new Error("Unexpected startup fetch");};
    try{await import(new URL("./application.mjs?document=incomplete",new URL(moduleURL,location.href)));sameNodes(before,root.querySelector("#ruvane-outlet"));assert(root.querySelector("#ruvane-status").textContent.includes("Native links and forms"),"native recovery");equal(reads,0,"no module or page GET");}finally{window.dispatchEvent(new Event("pagehide"));window.fetch=nativeFetch;root.remove();}
  });
  await test("DOM mismatch preserves received HTML and releases prepared layout owners",async()=>{
    const ctx=await setup();try{ctx.outlet.querySelector("[data-ruvane-page] h2").textContent="Tampered fixture";const before=references(ctx.outlet),stats=ctx.runtime.stats();equal(await ctx.navigator.start({snapshot:readInitialSnapshot(ctx.root)}),"native","rejected branch");sameNodes(before,ctx.outlet);equal(ctx.runtime.stats(),stats,"owners released");equal(ctx.reads(),0,"no retry");}finally{ctx.close();}
  });
  await test("adopted streamed layouts persist during later client navigation",async()=>{
    const ctx=await setup();try{equal(await ctx.navigator.start({snapshot:readInitialSnapshot(ctx.root)}),"success","startup");const parent=ctx.outlet.querySelector('[data-ruvane-layout="workspace"]');own(parent).querySelector("button").click();window.fetch=nativeFetch;equal(await ctx.navigator.navigate("/tasks/settings"),"success","sibling");assert(ctx.outlet.querySelector('[data-ruvane-layout="workspace"]')===parent,"retained layout");equal(own(parent).querySelector("output").textContent,"1","retained state");}finally{ctx.close();}
  });
  await test("native-only requests remain buffered with scripts blocked",async()=>{
    const response=await nativeFetch("/tasks/document?native=yes"),html=await response.text();assert(!response.headers.has("transfer-encoding")&&response.headers.get("content-security-policy").includes("script-src 'none'"),"buffered native policy");assert(!html.includes("ruvane-initial")&&!html.includes("application.mjs"),"native SSR");
  });
  await test("HEAD sends framing without a document body",async()=>{
    const response=await nativeFetch("/tasks/document",{method:"HEAD"});assert(response.status===200&&response.headers.get("transfer-encoding")==="chunked","stream HEAD");equal((await response.arrayBuffer()).byteLength,0,"no body");
  });
  await test("late loader failures cannot complete a successful document fetch",async()=>{
    const response=await nativeFetch("/tasks/document?unavailable=yes");equal(response.status,200,"already committed status");let failed=false;try{await response.text();}catch{failed=true;}assert(failed,"incomplete transport");assert((await nativeFetch("/tasks/document")).ok,"server recovered");
  });
  await test("one hundred streamed snapshot activations release all temporary resources",async()=>{
    const url="/tasks/document",template=await source(url),runtime=await createRuntime(moduleURL,{applicationRevision:revision}),baseline=runtime.stats();window.fetch=()=>{throw new Error("Unexpected data GET");};
    try{for(let n=0;n<100;n++){const root=insert(template,url),navigator=createNavigator(runtime,{scope:root,outlet:root.querySelector("#ruvane-outlet"),status:root.querySelector("#ruvane-status"),boundaries:readBoundaries(root)});equal(await navigator.start({snapshot:readInitialSnapshot(root)}),"success","cycle");navigator.dispose();root.remove();equal(runtime.stats(),baseline,"resource baseline");}}finally{window.fetch=nativeFetch;runtime.dispose();}
  });
  const result={passed:results.filter(row=>row.passed).length,failed:results.filter(row=>!row.passed).length,results};report.textContent=JSON.stringify(result,null,2);summary.textContent=`${result.passed}/${results.length} document streaming checks passed`;
});

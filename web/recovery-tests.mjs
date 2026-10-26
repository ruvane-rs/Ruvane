const report=document.getElementById("recovery-report"),summary=document.getElementById("recovery-summary"),results=[];
const assert=(value,message)=>{if(!value)throw new Error(message);};
const equal=(a,b,message)=>assert(JSON.stringify(a)===JSON.stringify(b),`${message}: ${JSON.stringify(a)}`);
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(check){const end=performance.now()+55000;while(!check()){if(performance.now()>end)throw new Error("Recovery check timed out");await wait(5);}}
async function test(name,fn){try{await fn();results.push({name,passed:true});}catch(error){results.push({name,passed:false,error:String(error.message)});}report.textContent=JSON.stringify(results,null,2);}
const nativeFetch=window.fetch,control=(path,fields={})=>nativeFetch(path,{method:"POST",headers:{"Content-Type":"application/x-www-form-urlencoded"},body:new URLSearchParams(fields)});
const mode=(value,revision=0)=>control("/fixtures/recovery-mode",{mode:String(value),revision:String(revision)});
const stats=async()=> (await(await nativeFetch("/fixtures/recovery-stats")).text()).split(",").map(Number);
const source=async url=>new DOMParser().parseFromString(await(await nativeFetch(url)).text(),"text/html").getElementById("ruvane-application");
const configuration=await source("/tasks/recovery"),moduleURL=configuration.getAttribute("data-ruvane-module"),revision=configuration.getAttribute("data-ruvane-revision");
const {createRuntime}=await import(new URL("./bridge.mjs",new URL(moduleURL,location.href)));
const {createNavigator,createDocumentHydration,readBoundaries}=await import(new URL("./navigation.mjs",new URL(moduleURL,location.href)));
const own=host=>host.querySelector(":scope > [data-ruvane-layout-view]"),layout=(root,name)=>root.querySelector(`[data-ruvane-layout="${name}"]`);
const button=(root,label)=>[...root.querySelectorAll("button")].find(node=>node.textContent===label);
const type=(field,value)=>{field.value=value;field.dispatchEvent(new InputEvent("input",{bubbles:true}));};
function references(root){const nodes=[];function visit(node){nodes.push(node);for(const child of node.childNodes)visit(child);}visit(root);return nodes;}
function sameNodes(before,root){const after=references(root);assert(after.length===before.length&&after.every((node,n)=>node===before[n]),"same parent elements and text nodes");}
function insert(template,url){history.replaceState({fixture:"recovery"},"",url);const root=document.importNode(template,true);for(const script of root.querySelectorAll("script"))script.remove();document.body.append(root);return root;}
async function setup({status=500,progressive=true}={}){
  await mode(status);const root=insert(await source("/tasks/recovery"),"/tasks/recovery"),outlet=root.querySelector("#ruvane-outlet"),feedback=root.querySelector("#ruvane-status"),runtime=await createRuntime(moduleURL,{applicationRevision:revision});
  let reads=0,posts=0,transport=()=>{throw new Error("Unexpected startup fetch");};
  window.fetch=(url,options={})=>{if(options.method==="POST")posts++;else reads++;return transport(url,options);};
  const session=createDocumentHydration(runtime,root),initial=await session.ready,navigator=createNavigator(runtime,{scope:root,outlet,status:feedback,boundaries:readBoundaries(root),progressive});
  equal(await navigator.start(initial),"success","error branch activated");session.commit();
  return{root,outlet,status:feedback,runtime,navigator,initial,reads:()=>reads,posts:()=>posts,enable(){transport=nativeFetch;},transport(fn){transport=fn;},close(){window.fetch=nativeFetch;session.retire();navigator.dispose();runtime.dispose();root.remove();}};
}
function hold(ctx){let release,held=false;ctx.transport(async(url,options)=>{const response=await nativeFetch(url,options);if(held||!String(url).includes("/tasks/recovery"))return response;held=true;const bytes=new Uint8Array(await response.arrayBuffer());return new Promise(resolve=>{release=()=>resolve(new Response(bytes,{status:response.status,headers:response.headers}));});});return{get ready(){return !!release;},release(){release?.();}};}
const run=document.createElement("button"),release=document.createElement("button");run.textContent="Run recovery checks";release.textContent="Release failing leaf";release.disabled=true;document.body.prepend(run,release);
release.addEventListener("click",async()=>{release.disabled=true;await control("/fixtures/release");});
run.addEventListener("click",async()=>{
  run.disabled=true;const child=window.open("about:blank","ruvane-leaf-recovery");if(!child){summary.textContent="The browser blocked the test window.";return;}
  try{
    await mode(500);const before=await stats();await control("/fixtures/prepare");child.location.href="/tasks/recovery?document-gate=yes";
    let workspace,draft,input,notes,parentNodes,draftNodes,reads=0,posts=0;
    await test("healthy layouts accept local edits before a blocked leaf fails",async()=>{
      await until(()=>{const draft=layout(child.document,"recoveryDraft");return draft&&!button(own(draft),"Increment draft layout").disabled;});workspace=layout(child.document,"workspace");draft=layout(child.document,"recoveryDraft");assert(child.document.readyState==="loading"&&!child.document.querySelector('[tabindex="-1"]'),"leaf still pending");
      own(workspace).querySelector("button").click();button(own(draft),"Increment draft layout").click();input=own(draft).querySelector("input");notes=own(draft).querySelector("textarea");input.value="Recovery draft 🦀";input.dispatchEvent(new child.InputEvent("input",{bubbles:true}));notes.value="Keep these notes";notes.dispatchEvent(new child.InputEvent("input",{bubbles:true}));input.focus();input.setSelectionRange(2,8);parentNodes=references(own(workspace).firstChild);draftNodes=references(own(draft).firstChild);
      const fetch=child.fetch.bind(child);child.fetch=(url,options={})=>{if(options.method==="POST")posts++;else reads++;return fetch(url,options);};summary.textContent="Parents contain drafts while the leaf is blocked. Release the failing leaf.";release.disabled=false;
    });
    await control("/fixtures/release");release.disabled=true;
    await test("a typed initial error keeps parents interactive without another GET",async()=>{
      await until(()=>child.document.getElementById("ruvane-status")?.textContent==="Page 500.");sameNodes(parentNodes,own(workspace).firstChild);sameNodes(draftNodes,own(draft).firstChild);assert(!input.disabled&&!own(workspace).querySelector("button").disabled,"parents remain active");assert(child.document.querySelector('[tabindex="-1"]').textContent.includes("Page unavailable")&&button(child.document,"Try again"),"typed error and retry");equal([input.value,notes.value,input.selectionStart,input.selectionEnd],["Recovery draft 🦀","Keep these notes",2,8],"draft and selection");assert(child.document.activeElement===input,"focus retained");equal([reads,posts],[0,0],"no implicit request");const after=await stats();equal(after.map((value,n)=>value-before[n]),[1,1],"one layout and leaf load");
    });
    await test("Try again issues one GET and adopts the same parents on success",async()=>{
      await mode(0);const doc=child.document,identity=doc.documentElement;button(doc,"Try again").click();await until(()=>doc.getElementById("ruvane-status")?.textContent==="Ready.");assert(doc.documentElement===identity&&layout(doc,"workspace")===workspace&&layout(doc,"recoveryDraft")===draft,"same document and parents");sameNodes(parentNodes,own(workspace).firstChild);sameNodes(draftNodes,own(draft).firstChild);equal([own(workspace).querySelector("output").textContent,own(draft).querySelector("output").textContent],["1","1"],"counter state");equal([input.value,notes.value,input.selectionStart,input.selectionEnd],["Recovery draft 🦀","Keep these notes",2,8],"draft and selection");assert(doc.activeElement===input,"retained parent focus");equal([reads,posts],[1,0],"one GET and no POST");assert(doc.querySelector('[tabindex="-1"]').textContent.includes("Project overview"),"healthy leaf");
    });
  }finally{await control("/fixtures/release");release.disabled=true;child.close();}
  await test("400, 404, panic, schema and title errors use a readonly typed leaf",async()=>{
    for(const status of [400,404,500,600,700,800]){const ctx=await setup({status});try{equal(ctx.status.textContent,`Page ${status<600?status:500}.`,"logical status");equal(ctx.navigator.failure()?.kind,"server","typed failure");assert(ctx.outlet.querySelector('[tabindex="-1"]').querySelectorAll("button,input,textarea").length===0,"readonly error");equal([ctx.reads(),ctx.posts()],[0,0],"no automatic request");equal(ctx.navigator.stats().cacheEntries,0,"error not cached");}finally{ctx.close();}}
  });
  await test("buffered retry preserves drafts, selection and existing history fields",async()=>{
    const ctx=await setup({progressive:false});try{const parent=layout(ctx.root,"recoveryDraft"),input=own(parent).querySelector("input");type(input,"Buffered retry draft");input.focus();input.setSelectionRange(1,7);ctx.initial.parents[0].dispatch(1);const before=references(own(parent).firstChild),length=history.length;await mode(0);ctx.enable();equal(await ctx.navigator.retry(),"success","retry");sameNodes(before,own(parent).firstChild);assert(layout(ctx.root,"recoveryDraft")===parent&&document.activeElement===input,"parent and focus");equal([input.value,input.selectionStart,input.selectionEnd],["Buffered retry draft",1,7],"selection");equal([ctx.reads(),ctx.posts(),history.length],[1,0,length],"one GET and replacement");equal(history.state.fixture,"recovery","history fields");}finally{ctx.close();}
  });
  await test("repeated explicit failures preserve parents and never cache errors",async()=>{
    const ctx=await setup();try{const parent=layout(ctx.root,"recoveryDraft"),input=own(parent).querySelector("input");type(input,"Repeated failure draft");ctx.enable();for(let n=0;n<5;n++){equal(await ctx.navigator.retry(),"success","typed error committed");assert(layout(ctx.root,"recoveryDraft")===parent,"same parent");equal(ctx.navigator.stats().cacheEntries,0,"no error cache");equal(ctx.runtime.stats().views,3,"two parents and error");}equal([input.value,ctx.reads(),ctx.posts()],["Repeated failure draft",5,0],"no hidden retry or POST");}finally{ctx.close();}
  });
  await test("changed seeds replace only the incompatible parent",async()=>{
    const ctx=await setup();try{const root=layout(ctx.root,"workspace"),draft=layout(ctx.root,"recoveryDraft");ctx.initial.parents[0].dispatch(1);await mode(0,1);ctx.enable();equal(await ctx.navigator.retry(),"success","retry");assert(layout(ctx.root,"workspace")===root&&layout(ctx.root,"recoveryDraft")!==draft,"compatible ancestor retained");equal(own(root).querySelector("output").textContent,"1","state retained");equal(ctx.runtime.stats().views,3,"departing owner released");equal(ctx.posts(),0,"no POST");}finally{ctx.close();}
  });
  await test("history failure preserves the current error and parent drafts",async()=>{
    const ctx=await setup(),replace=history.replaceState;try{const parent=layout(ctx.root,"recoveryDraft"),input=own(parent).querySelector("input"),leaf=ctx.outlet.querySelector('[tabindex="-1"]');type(input,"History draft");await mode(0);ctx.enable();history.replaceState=()=>{throw new Error("History fixture");};equal(await ctx.navigator.retry(),"failed","commit refused");assert(ctx.outlet.querySelector('[tabindex="-1"]')===leaf&&layout(ctx.root,"recoveryDraft")===parent,"current branch retained");equal([input.value,ctx.reads(),ctx.posts()],["History draft",1,0],"no reload or POST");}finally{history.replaceState=replace;ctx.close();}
  });
  await test("malformed replies cannot replace the displayed error or its parents",async()=>{
    const ctx=await setup();try{const parent=layout(ctx.root,"recoveryDraft"),leaf=ctx.outlet.querySelector('[tabindex="-1"]');ctx.transport(async(url,options)=>{const response=await nativeFetch(url,options),bytes=new Uint8Array(await response.arrayBuffer());bytes[0]^=255;return new Response(bytes,{status:response.status,headers:response.headers});});await mode(0);equal(await ctx.navigator.retry(),"failed","reply rejected");assert(layout(ctx.root,"recoveryDraft")===parent&&ctx.outlet.querySelector('[tabindex="-1"]')===leaf,"branch retained");equal(ctx.navigator.stats().cacheEntries,0,"no invalid cache");equal(ctx.posts(),0,"no POST");}finally{ctx.close();}
  });
  await test("a newer navigation retires a late retry response",async()=>{
    const ctx=await setup();let gate;try{await mode(0);gate=hold(ctx);const retry=ctx.navigator.retry();await until(()=>gate.ready);equal(await ctx.navigator.navigate("/tasks/settings"),"success","new destination");gate.release();equal(await retry,"superseded","stale retry");assert(ctx.outlet.textContent.includes("Task settings"),"new page remains");equal(ctx.posts(),0,"no POST");}finally{gate?.release();ctx.close();}
  });
  await test("an explicit parent action invalidates retry without replaying its POST",async()=>{
    const ctx=await setup();let gate;try{await mode(0);gate=hold(ctx);const retry=ctx.navigator.retry();await until(()=>gate.ready);ctx.initial.parents[1].dispatch(4);equal(await ctx.initial.parents[1].whenIdle(),"success","explicit action");gate.release();equal(await retry,"invalidated","old read retired");equal([ctx.reads(),ctx.posts()],[1,1],"one GET and one explicit POST");assert(ctx.outlet.querySelector('[tabindex="-1"]').textContent.includes("Page unavailable"),"error remains");equal(await ctx.navigator.retry(),"skipped","no restart");}finally{gate?.release();ctx.close();}
  });
  await test("duplicate retry requests share no extra GET",async()=>{
    const ctx=await setup();let gate;try{await mode(0);gate=hold(ctx);const retry=ctx.navigator.retry();await until(()=>gate.ready);equal(await ctx.navigator.retry(),"skipped","pending retry refused");gate.release();equal(await retry,"success","first retry");equal([ctx.reads(),ctx.posts()],[1,0],"one GET only");}finally{gate?.release();ctx.close();}
  });
  await test("disposal releases every owner and ignores late retry bytes",async()=>{
    const ctx=await setup();let gate;try{await mode(0);gate=hold(ctx);const retry=ctx.navigator.retry();await until(()=>gate.ready);ctx.navigator.dispose();equal(ctx.runtime.stats().views,0,"owners released");gate.release();equal(await retry,"disposed","late response ignored");equal(ctx.posts(),0,"no POST");}finally{gate?.release();ctx.close();}
  });
  await test("a corrupt final error snapshot cannot release parent action gates",async()=>{
    await mode(500);const root=insert(await source("/tasks/recovery"),"/tasks/recovery"),runtime=await createRuntime(moduleURL,{applicationRevision:revision}),session=createDocumentHydration(runtime,root);let navigator;try{const initial=await session.ready;initial.snapshot[4]^=1;navigator=createNavigator(runtime,{scope:root,outlet:root.querySelector("#ruvane-outlet"),status:root.querySelector("#ruvane-status"),boundaries:readBoundaries(root)});equal(await navigator.start(initial),"native","revision refused");session.retire();equal(runtime.stats().views,0,"early owners released");assert(own(layout(root,"recoveryDraft")).querySelector("input").disabled,"controls retired");}finally{session.retire();navigator?.dispose();runtime.dispose();root.remove();}
  });
  await test("native requests retain real error statuses and blocking CSP",async()=>{
    await mode(404);const response=await nativeFetch("/tasks/recovery?native=yes"),html=await response.text();equal(response.status,404,"native status");assert(response.headers.get("content-security-policy").includes("script-src 'none'")&&!html.includes("ruvane-initial"),"native policy");
  });
  await test("one hundred typed-error startups release all resources",async()=>{
    await mode(500);const url="/tasks/recovery",template=await source(url),runtime=await createRuntime(moduleURL,{applicationRevision:revision}),baseline=runtime.stats();window.fetch=()=>{throw new Error("Unexpected data request");};
    try{for(let n=0;n<100;n++){const root=insert(template,url),session=createDocumentHydration(runtime,root),initial=await session.ready,navigator=createNavigator(runtime,{scope:root,outlet:root.querySelector("#ruvane-outlet"),status:root.querySelector("#ruvane-status"),boundaries:readBoundaries(root)});initial.parents[0].dispatch(1);equal(await navigator.start(initial),"success","typed error startup");session.commit();equal(navigator.failure()?.kind,"server","explicit retry");navigator.dispose();root.remove();equal(runtime.stats(),baseline,"no retained resources");}}finally{window.fetch=nativeFetch;runtime.dispose();}
  });
  window.fetch=nativeFetch;await mode(500);const result={passed:results.filter(row=>row.passed).length,failed:results.filter(row=>!row.passed).length,results};report.textContent=JSON.stringify(result,null,2);summary.textContent=`${result.passed}/${results.length} recovery checks passed`;
});

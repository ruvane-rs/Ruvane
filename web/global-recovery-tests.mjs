const report=document.getElementById("global-recovery-report"),summary=document.getElementById("global-recovery-summary"),results=[];
const assert=(value,message)=>{if(!value)throw new Error(message);},equal=(a,b,message)=>assert(JSON.stringify(a)===JSON.stringify(b),`${message}: ${JSON.stringify(a)}`);
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(check){const end=performance.now()+55000;while(!check()){if(performance.now()>end)throw new Error("Global recovery check timed out");await wait(5);}}
async function test(name,fn){try{await fn();results.push({name,passed:true});}catch(error){results.push({name,passed:false,error:String(error.message)});}report.textContent=JSON.stringify(results,null,2);summary.textContent=`Completed ${results.length}: ${name}`;}
const nativeFetch=window.fetch,control=async(path,fields={})=>{const response=await nativeFetch(path,{method:"POST",headers:{"Content-Type":"application/x-www-form-urlencoded"},body:new URLSearchParams(fields)});assert(response.ok,`Control ${path}: ${response.status}`);return response;};
const mode=(stage=0,code=500,bad=0)=>control("/fixtures/global-mode",{stage:String(stage),code:String(code),bad:String(bad)});
const base="/tasks/global-fixture",source=async url=>new DOMParser().parseFromString(await(await nativeFetch(url)).text(),"text/html").getElementById("ruvane-application");
const configuration=await source(base),moduleURL=configuration.getAttribute("data-ruvane-module"),revision=configuration.getAttribute("data-ruvane-revision");
const {createRuntime}=await import(new URL("./bridge.mjs",new URL(moduleURL,location.href)));
const {createNavigator,createDocumentHydration,readBoundaries,readInitialSnapshot}=await import(new URL("./navigation.mjs",new URL(moduleURL,location.href)));
const layout=(root,name)=>root.querySelector(`[data-ruvane-layout="${name}"]`),own=host=>host.querySelector(":scope > [data-ruvane-layout-view]"),editor=root=>root.querySelector("#fixture-shell");
const button=(root,label)=>[...root.querySelectorAll("button")].find(node=>node.textContent===label),type=(field,value)=>{field.value=value;field.dispatchEvent(new InputEvent("input",{bubbles:true}));};
function references(root){const nodes=[];function visit(node){nodes.push(node);for(const child of node.childNodes)visit(child);}visit(root);return nodes;}
function sameNodes(nodes,root){const after=references(root);assert(nodes.length===after.length&&nodes.every((node,n)=>node===after[n]),"same shell nodes");}
function insert(template,url){history.replaceState({fixture:"global-recovery"},"",url);const root=document.importNode(template,true);for(const script of root.querySelectorAll("script"))script.remove();document.body.append(root);return root;}
function shellControllers(runtime,root){const outlet=root.querySelector("#ruvane-outlet");return [...root.querySelectorAll("[data-ruvane-version]")].filter(node=>!outlet.contains(node)).map(node=>runtime.hydrate(node));}
async function setup({stage=0,code=500,early=true,start=true,path=base,progressive=true,template=null,beforeShell=null}={}){
  await mode(stage,code);const url=path+(early?"":"?hydrate=no"),root=insert(template??await source(url),url),runtime=await createRuntime(moduleURL,{applicationRevision:revision}),shell=(beforeShell?.(root),shellControllers(runtime,root));
  let reads=0,posts=0,transport=()=>{throw new Error("Unexpected startup fetch");};window.fetch=(url,options={})=>{if(options.method==="POST")posts++;else reads++;return transport(url,options);};
  const session=early?createDocumentHydration(runtime,root):null,initial={...(session?await session.ready:{snapshot:readInitialSnapshot(root),parents:[]})},navigator=createNavigator(runtime,{scope:root,outlet:root.querySelector("#ruvane-outlet"),status:root.querySelector("#ruvane-status"),boundaries:readBoundaries(root),progressive});
  const ctx={root,runtime,navigator,shell,session,initial,reads:()=>reads,posts:()=>posts,enable(){transport=nativeFetch;},transport(fn){transport=fn;},async start(options={}){const result=await navigator.start({...initial,...options,accept:()=> (!initial.accept||initial.accept())&&(!options.accept||options.accept())});if(result==="success")session?.commit();return result;},close(){window.fetch=nativeFetch;session?.retire();navigator.dispose();runtime.dispose();root.remove();}};
  if(start)equal(await ctx.start(),"success","global error activated");return ctx;
}
const run=document.createElement("button");run.textContent="Run global recovery checks";document.body.prepend(run);
run.addEventListener("click",async()=>{
  run.disabled=true;
  for(const stage of [0,2]){
    let response,reader,html="",ctx,shell,input,notes,nodes;const decoder=new TextDecoder();
    try{
      await mode(stage);await control("/fixtures/prepare");
      await test(`${stage===0?"root":"branch"} failure streams the shell before completion`,async()=>{
        response=await nativeFetch(base+"?document-gate=yes");reader=response.body.getReader();
        while(!html.includes(stage===0?'id="fixture-shell"':'data-ruvane-layout="fixtureInner"')){const item=await reader.read();assert(!item.done,"stream held before completion");html+=decoder.decode(item.value,{stream:true});}
        assert(response.status===200&&html.includes("Layout title")&&!html.includes("ruvane-complete"),"native shell fields received before completion");
      });
      await control("/fixtures/release");
      await test(`${stage===0?"root":"branch"} error activates globally and keeps the shell interactive`,async()=>{
        while(true){const item=await reader.read();if(item.done)break;html+=decoder.decode(item.value,{stream:true});}
        const template=new DOMParser().parseFromString(html,"text/html").getElementById("ruvane-application");
        ctx=await setup({stage,path:base+"?document-gate=yes",start:false,template,beforeShell(root){shell=editor(root);input=shell.querySelector("input");notes=shell.querySelector("textarea");input.value="Shell 🦀 draft";notes.value="Shell notes";}});input.focus();input.setSelectionRange(2,8);equal(await ctx.start(),"success","global startup");
        assert(editor(ctx.root)===shell&&!ctx.root.querySelector("[data-ruvane-layout]"),"shell retained and outlet branch retired");
        equal([input.value,notes.value,input.selectionStart,input.selectionEnd],["Shell 🦀 draft","Shell notes",2,8],"shell edits adopted");assert(document.activeElement===input,"shell focus");
        button(shell,"Increment draft layout").click();equal(shell.querySelector("output").textContent,"1","shell event works");nodes=references(shell);equal([ctx.reads(),ctx.posts()],[0,0],"no automatic request");
      });
      await test(`${stage===0?"root":"branch"} retry reconstructs the full branch without resetting the shell`,async()=>{
        await mode(stage,0);ctx.enable();equal(await ctx.navigator.retry(),"success","explicit retry");
        assert(editor(ctx.root)===shell&&layout(ctx.root,"fixtureRoot")&&layout(ctx.root,"fixtureInner"),"full branch restored");sameNodes(nodes,shell);
        equal([input.value,notes.value,shell.querySelector("output").textContent,ctx.reads(),ctx.posts()],["Shell 🦀 draft","Shell notes","1",1,0],"shell state and one GET");
      });
    }finally{await control("/fixtures/release");reader?.releaseLock();ctx?.close();}
  }
  for(const early of [true,false])for(const [stage,code] of [[0,400],[0,404],[0,500],[0,600],[0,700],[1,500],[2,500],[2,600],[2,800]])await test(`${early?"early":"default"} global error at stage ${stage} with code ${code} has no retained layouts`,async()=>{
    const ctx=await setup({stage,code,early});try{equal([ctx.initial.snapshot[21],new DataView(ctx.initial.snapshot.buffer,ctx.initial.snapshot.byteOffset).getUint16(22,true)],[stage,[400,404].includes(code)?code:500],"failure scope and status");equal(ctx.runtime.stats().views,3,"two shell islands and error");equal(ctx.navigator.failure()?.kind,"server","typed failure");assert(!ctx.root.querySelector("[data-ruvane-layout]")&&!ctx.root.querySelector("#ruvane-retired"),"only global error remains");equal([ctx.reads(),ctx.posts()],[0,0],"no startup request");equal(ctx.navigator.failure()?.kind,"server","explicit retry");}finally{ctx.close();}
  });
  await test("a page without layouts can use the App boundary",async()=>{const ctx=await setup({stage:2,path:"/tasks/global-bare"});try{equal([ctx.initial.parents.length,ctx.runtime.stats().views],[0,3],"shell and global error");}finally{ctx.close();}});
  for(const [path,depth] of [["/tasks/global-local",2],["/tasks/global-parent",1]])await test(`local boundary at depth ${depth} takes precedence over the App boundary`,async()=>{const ctx=await setup({stage:2,path});try{equal(ctx.navigator.stats().layouts,depth,"local scope retained");assert(ctx.root.querySelector("#ruvane-outlet").textContent.includes("Page unavailable"),"local error component");}finally{ctx.close();}});
  for(const progressive of [true,false])await test(`${progressive?"progressive":"buffered"} global retry preserves focused shell drafts through repeated errors`,async()=>{
    const ctx=await setup({progressive});try{const shell=editor(ctx.root),input=shell.querySelector("input");type(input,"Focused shell draft");input.focus();input.setSelectionRange(2,7);const nodes=references(shell);ctx.enable();for(let n=0;n<3;n++){equal(await ctx.navigator.retry(),"success","error retry");equal(ctx.navigator.stats().cacheEntries,0,"no error cache");equal(ctx.runtime.stats().views,3,"old error released");}await mode(0,0);equal(await ctx.navigator.retry(),"success","branch restored");sameNodes(nodes,shell);assert(document.activeElement===input,"shell focus retained");equal([input.value,input.selectionStart,input.selectionEnd,ctx.reads(),ctx.posts()],["Focused shell draft",2,7,4,0],"selection and explicit GETs");}finally{ctx.close();}
  });
  for(const corruption of ["depth","manifest","component","failure","prefix"])await test(`invalid global ${corruption} cannot clear the received branch`,async()=>{
    const ctx=await setup({stage:2,start:false});try{const root=layout(ctx.root,"fixtureRoot"),initial=ctx.root.querySelector("#ruvane-initial");
      if(corruption==="depth")ctx.root.setAttribute("data-ruvane-layouts","1");
      if(corruption==="manifest"){const app=ctx.root.querySelector('[data-rvn-kind="app"]');app.before(app.cloneNode(true));}
      if(corruption==="component"||corruption==="failure"){const bytes=ctx.initial.snapshot.slice(),view=new DataView(bytes.buffer);if(corruption==="failure")bytes[21]=0;else{let p=24;p+=4+view.getUint32(p,true);p+=4+view.getUint32(p,true);view.setUint32(p,16,true);}ctx.initial.snapshot=bytes;}
      if(corruption==="prefix")ctx.root.querySelector("#ruvane-retired").remove();
      equal(await ctx.start(),"native","invalid global branch refused");assert(root.isConnected,"received root retained");ctx.session.retire();equal(ctx.runtime.stats().views,2,"only independent shell owners remain");equal(ctx.posts(),0,"no POST");
    }finally{ctx.close();}
  });
  await test("a shell owner cannot be substituted for a retiring layout",async()=>{
    const ctx=await setup({stage:2,start:false});try{const root=layout(ctx.root,"fixtureRoot");equal(await ctx.start({parents:[ctx.shell[1],...ctx.initial.parents.slice(1)]}),"native","foreign owner refused");assert(root.isConnected,"root retained");ctx.session.retire();ctx.shell[1].dispatch(1);equal(editor(ctx.root).querySelector("output").textContent,"1","shell owner stays active");}finally{ctx.close();}
  });
  for(const accept of [false,true])await test(`${accept?"history failure":"native guard"} refuses whole-branch retirement`,async()=>{
    const ctx=await setup({stage:2,start:false}),replace=history.replaceState;try{const root=layout(ctx.root,"fixtureRoot");if(accept)history.replaceState=()=>{throw new Error("History fixture");};equal(await ctx.start({accept:()=>accept}),"native","commit refused");assert(root.isConnected,"no partial removal");equal(ctx.posts(),0,"no POST");}finally{history.replaceState=replace;ctx.close();}
  });
  await test("a cancelled root session cannot activate a late global error",async()=>{const ctx=await setup({start:false});try{ctx.session.retire();equal(await ctx.start(),"native","retired root completion refused");equal(ctx.runtime.stats().views,2,"only shell survives");}finally{ctx.close();}});
  await test("a newer destination cannot retire the old global branch",async()=>{const ctx=await setup({stage:2,start:false});try{const root=layout(ctx.root,"fixtureRoot");history.replaceState({},"","/tasks/settings");equal(await ctx.start(),"native","stale destination refused");assert(root.isConnected,"branch retained");}finally{ctx.close();}});
  await test("an explicit shell action invalidates a late retry without replaying its POST",async()=>{
    const ctx=await setup();let release,ready=false;try{await mode(0,0);ctx.transport(async(url,options)=>{const response=await nativeFetch(url,options);if(options.method==="POST")return response;const bytes=await response.arrayBuffer();ready=true;return new Promise(resolve=>{release=()=>resolve(new Response(bytes,{status:response.status,headers:response.headers}));});});const retry=ctx.navigator.retry();await until(()=>ready);ctx.shell[1].dispatch(4);equal(await ctx.shell[1].whenIdle(),"success","explicit shell action");release();equal(await retry,"invalidated","stale GET retired");equal([ctx.reads(),ctx.posts()],[1,1],"one GET and one explicit POST");}finally{release?.();ctx.close();}
  });
  await test("invalid global presentations use minimal fallback without diagnostics or actions",async()=>{
    try{for(const bad of [4,5,6]){await mode(0,500,bad);for(const suffix of ["","?hydrate=no"]){const response=await nativeFetch(base+suffix),reader=response.body.getReader(),decoder=new TextDecoder();let html="",complete=false;try{while(true){const item=await reader.read();if(item.done){complete=true;break;}html+=decoder.decode(item.value,{stream:true});}}catch(error){if(!suffix)throw error;}finally{reader.releaseLock();}if(suffix)assert(!complete&&!html.includes("ruvane-complete"),"default stream incomplete");else assert(complete&&html.includes("data-ruvane-stream-error"),"early generic fallback");assert(!html.includes("ruvane-initial")&&!html.includes("private global"),"no typed activation or diagnostics");}}}finally{await mode();}
  });
  await test("one hundred global startups per document profile release all owners and listeners",async()=>{
    await mode(2);const templates=[await source(base),await source(base+"?hydrate=no")],runtime=await createRuntime(moduleURL,{applicationRevision:revision}),baseline=runtime.stats(),replace=history.replaceState;window.fetch=()=>{throw new Error("Unexpected startup fetch");};
    // Skip redundant history writes in the cleanup loop.
    history.replaceState=function(state,title,url){if(url!==undefined&&new URL(url,location.href).href!==location.href)return replace.call(this,state,title,url);};
    try{for(const early of [true,false])for(let n=0;n<100;n++){
      const url=base+(early?"":"?hydrate=no"),root=insert(templates[early?0:1],url),shell=shellControllers(runtime,root),session=early?createDocumentHydration(runtime,root):null;let navigator;
      try{const initial=session?await session.ready:{snapshot:readInitialSnapshot(root),parents:[]};navigator=createNavigator(runtime,{scope:root,outlet:root.querySelector("#ruvane-outlet"),status:root.querySelector("#ruvane-status"),boundaries:readBoundaries(root)});equal(await navigator.start(initial),"success","global startup");session?.commit();shell[1].dispatch(1);}
      finally{session?.retire();navigator?.dispose();for(const controller of shell)controller.dispose();root.remove();}
      equal(runtime.stats(),baseline,"all resources released");
    }}finally{history.replaceState=replace;window.fetch=nativeFetch;runtime.dispose();}
  });
  window.fetch=nativeFetch;await mode();const result={passed:results.filter(row=>row.passed).length,failed:results.filter(row=>!row.passed).length,results};report.textContent=JSON.stringify(result,null,2);summary.textContent=`${result.passed}/${results.length} global recovery checks passed`;
});

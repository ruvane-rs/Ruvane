const report=document.getElementById("parent-recovery-report"),summary=document.getElementById("parent-recovery-summary"),results=[];
const assert=(value,message)=>{if(!value)throw new Error(message);};
const equal=(a,b,message)=>assert(JSON.stringify(a)===JSON.stringify(b),`${message}: ${JSON.stringify(a)}`);
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(check){const end=performance.now()+55000;while(!check()){if(performance.now()>end)throw new Error("Parent recovery check timed out");await wait(5);}}
async function test(name,fn){try{await fn();results.push({name,passed:true});}catch(error){results.push({name,passed:false,error:String(error.message)});}report.textContent=JSON.stringify(results,null,2);}
const nativeFetch=window.fetch,control=(path,fields={})=>nativeFetch(path,{method:"POST",headers:{"Content-Type":"application/x-www-form-urlencoded"},body:new URLSearchParams(fields)});
const mode=value=>control("/fixtures/parent-mode",{mode:String(value)}),stats=async()=>Number(await(await nativeFetch("/fixtures/parent-stats")).text());
const path="/tasks/parent-recovery",source=async url=>new DOMParser().parseFromString(await(await nativeFetch(url)).text(),"text/html").getElementById("ruvane-application");
const configuration=await source(path),moduleURL=configuration.getAttribute("data-ruvane-module"),revision=configuration.getAttribute("data-ruvane-revision");
const {createRuntime}=await import(new URL("./bridge.mjs",new URL(moduleURL,location.href)));
const {createNavigator,createDocumentHydration,readBoundaries,readInitialSnapshot}=await import(new URL("./navigation.mjs",new URL(moduleURL,location.href)));
const own=host=>host.querySelector(":scope > [data-ruvane-layout-view]"),layout=(root,name)=>root.querySelector(`[data-ruvane-layout="${name}"]`);
const button=(root,label)=>[...root.querySelectorAll("button")].find(node=>node.textContent===label),type=(field,value)=>{field.value=value;field.dispatchEvent(new InputEvent("input",{bubbles:true}));};
function references(root){const nodes=[];function visit(node){nodes.push(node);for(const child of node.childNodes)visit(child);}visit(root);return nodes;}
function sameNodes(before,root){const after=references(root);assert(after.length===before.length&&after.every((node,n)=>node===before[n]),"same ancestor nodes");}
function insert(template,url){history.replaceState({fixture:"parent-recovery"},"",url);const root=document.importNode(template,true);for(const script of root.querySelectorAll("script"))script.remove();document.body.append(root);return root;}
async function setup({status=500,early=true,start=true,route=path,progressive=true}={}){
  await mode(status);const url=route+(early?"":"?hydrate=no"),root=insert(await source(url),url),runtime=await createRuntime(moduleURL,{applicationRevision:revision});
  let reads=0,posts=0,transport=()=>{throw new Error("Unexpected startup fetch");};window.fetch=(url,options={})=>{if(options.method==="POST")posts++;else reads++;return transport(url,options);};
  const session=early?createDocumentHydration(runtime,root):null,initial=session?await session.ready:{snapshot:readInitialSnapshot(root),parents:[]};
  const navigator=createNavigator(runtime,{scope:root,outlet:root.querySelector("#ruvane-outlet"),status:root.querySelector("#ruvane-status"),boundaries:readBoundaries(root),progressive});
  const ctx={root,runtime,navigator,session,initial,reads:()=>reads,posts:()=>posts,enable(){transport=nativeFetch;},async start(accept){const result=await navigator.start({...initial,accept});if(result==="success")session?.commit();return result;},close(){window.fetch=nativeFetch;session?.retire();navigator.dispose();runtime.dispose();root.remove();}};
  if(start)equal(await ctx.start(),"success","ancestor error activated");return ctx;
}
const run=document.createElement("button");run.textContent="Run parent recovery checks";document.body.prepend(run);
run.addEventListener("click",async()=>{
  run.disabled=true;const child=window.open("about:blank","ruvane-parent-recovery");if(!child){summary.textContent="The browser blocked the test window.";return;}
  try{
    await mode(500);await control("/fixtures/prepare");child.location.href=path+"?document-gate=yes";
    let parent,middle,tail,input,notes,nodes,reads=0,posts=0;
    await test("three streamed layouts accept local edits before the leaf fails",async()=>{
      await until(()=>{const tail=layout(child.document,"propTail");return tail&&button(own(tail),"Increment draft layout")?.disabled===false&&child.document.querySelector("#ruvane-status")?.textContent==="Layouts ready. Loading page…";});
      parent=layout(child.document,"propRoot");middle=layout(child.document,"propMiddle");tail=layout(child.document,"propTail");input=own(parent).querySelector("input");notes=own(parent).querySelector("textarea");
      const fetch=child.fetch.bind(child);child.fetch=(url,options={})=>{if(options.method==="POST")posts++;else reads++;return fetch(url,options);};
      button(own(parent),"Increment draft layout").click();type(input,"Ancestor 🦀 draft");type(notes,"Ancestor notes");type(own(middle).querySelector("input"),"Removed middle draft");
      button(own(tail),"Send layout action").click();await until(()=>own(tail).textContent.includes("Action blocked"));equal(posts,0,"prepared child action refused");
      input.focus();input.setSelectionRange(2,8);nodes=references(own(parent));
    });
    await control("/fixtures/release");
    await test("the ancestor boundary retires both descendants and retains its own live DOM",async()=>{
      await until(()=>child.document.querySelector("#ruvane-status")?.textContent==="Page 500.");
      assert(layout(child.document,"propRoot")===parent&&!middle.isConnected&&!tail.isConnected,"only ancestor retained");sameNodes(nodes,own(parent));
      equal([input.value,notes.value,input.selectionStart,input.selectionEnd],["Ancestor 🦀 draft","Ancestor notes",2,8],"draft and selection");assert(child.document.activeElement===input,"ancestor focus");
      const old=button(own(middle),"Increment draft layout"),value=own(middle).querySelector("output").textContent;assert(old.disabled,"departed controls disabled");old.click();equal(own(middle).querySelector("output").textContent,value,"detached control inert");equal([reads,posts],[0,0],"no automatic request");
    });
    await test("Try again reconstructs descendants with one GET and the same ancestor state",async()=>{
      await mode(0);button(child.document,"Try again").click();await until(()=>child.document.querySelector("#ruvane-status")?.textContent==="Ready.");
      assert(layout(child.document,"propRoot")===parent&&layout(child.document,"propMiddle")!==middle&&layout(child.document,"propTail")!==tail,"new descendants");sameNodes(nodes,own(parent));
      equal([input.value,notes.value,own(parent).querySelector("output").textContent,reads,posts],["Ancestor 🦀 draft","Ancestor notes","1",1,0],"state preserved and no POST replay");
    });
  }finally{await control("/fixtures/release");child.close();}
  for(const early of [false,true])for(const status of [400,404,500,600,700,800])await test(`${early?"early":"default"} document propagates error ${status} without a data request`,async()=>{
    const ctx=await setup({status,early});try{equal(ctx.runtime.stats().views,2,"ancestor and error only");assert(!layout(ctx.root,"propMiddle")&&!layout(ctx.root,"propTail")&&!ctx.root.querySelector("#ruvane-retired"),"subtree and metadata retired");equal([ctx.reads(),ctx.posts()],[0,0],"no startup fetch");equal(ctx.navigator.failure()?.kind,"server","explicit retry available");}finally{ctx.close();}
  });
  await test("a deeper layout failure retires the emitted middle and never calls the leaf",async()=>{
    const before=await stats(),ctx=await setup({status:900});try{equal(ctx.initial.parents.length,2,"only emitted prefix hydrated");equal(await stats(),before,"leaf not executed");equal(ctx.runtime.stats().views,2,"one ancestor and error");assert(!layout(ctx.root,"propMiddle")&&!layout(ctx.root,"propTail"),"missing and retired descendants absent");}finally{ctx.close();}
  });
  await test("the nearest of multiple ancestor boundaries keeps both surviving parents",async()=>{
    const ctx=await setup({start:false,route:"/tasks/nearest-recovery"});try{const root=layout(ctx.root,"propRoot"),nearest=layout(ctx.root,"propNearest"),tail=layout(ctx.root,"propNearestTail"),input=own(nearest).querySelector("input");type(input,"Nearest parent draft");input.focus();input.setSelectionRange(1,7);equal(await ctx.start(),"success","nearest boundary");assert(layout(ctx.root,"propRoot")===root&&layout(ctx.root,"propNearest")===nearest&&!tail.isConnected,"nearest scope");equal([input.value,input.selectionStart,input.selectionEnd],["Nearest parent draft",1,7],"nearest state");equal(ctx.runtime.stats().views,3,"two parents and error");}finally{ctx.close();}
  });
  await test("focus inside a retired descendant moves to the error host",async()=>{
    const ctx=await setup({start:false});try{own(layout(ctx.root,"propTail")).querySelector("input").focus();equal(await ctx.start(),"success","error committed");assert(document.activeElement===ctx.root.querySelector("[data-ruvane-page]"),"error host focused");}finally{ctx.close();}
  });
  await test("default startup adopts surviving edits without adopting removed autosave fields",async()=>{
    const ctx=await setup({start:false,early:false});try{const parent=layout(ctx.root,"propRoot"),input=own(parent).querySelector("input"),tail=layout(ctx.root,"propTail");input.value="Default parent draft";own(tail).querySelector("input").value="Autosave fixture removed";input.focus();input.setSelectionRange(2,6);equal(await ctx.start(),"success","default error startup");assert(layout(ctx.root,"propRoot")===parent&&!tail.isConnected,"subtree retired");equal([input.value,input.selectionStart,input.selectionEnd,ctx.posts()],["Default parent draft",2,6,0],"only surviving edit adopted");}finally{ctx.close();}
  });
  for(const progressive of [false,true])await test(`${progressive?"progressive":"buffered"} retry keeps the ancestor through repeated errors then success`,async()=>{
    const ctx=await setup({progressive});try{const parent=layout(ctx.root,"propRoot"),input=own(parent).querySelector("input");type(input,"Retry draft");ctx.enable();for(let n=0;n<3;n++){equal(await ctx.navigator.retry(),"success","error retry");assert(layout(ctx.root,"propRoot")===parent&&!layout(ctx.root,"propMiddle"),"ancestor retained");equal(ctx.navigator.stats().cacheEntries,0,"no cached error");equal(ctx.runtime.stats().views,2,"no departed owner");}await mode(0);equal(await ctx.navigator.retry(),"success","success retry");equal([input.value,ctx.reads(),ctx.posts(),ctx.runtime.stats().views],["Retry draft",4,0,4],"one GET per explicit retry");}finally{ctx.close();}
  });
  for(const corruption of ["missing","duplicate","revision","depth","seed","dom"])await test(`invalid ${corruption} retirement keeps received descendants in place`,async()=>{
    const ctx=await setup({start:false});try{const middle=layout(ctx.root,"propMiddle"),record=ctx.root.querySelector("#ruvane-retired");
      if(corruption==="missing")record.remove();else if(corruption==="duplicate")record.before(record.cloneNode(true));else if(corruption==="dom")own(middle).setAttribute("data-extra","invalid");else{const bytes=Uint8Array.from(record.textContent.match(/../g),value=>parseInt(value,16));if(corruption==="revision")bytes[4]^=1;if(corruption==="depth")bytes[21]=2;if(corruption==="seed")bytes[bytes.length-1]^=1;record.textContent=[...bytes].map(value=>value.toString(16).padStart(2,"0")).join("");}
      equal(await ctx.start(),"native","invalid retirement refused");assert(middle.isConnected&&layout(ctx.root,"propMiddle")===middle,"no partial removal");ctx.session.retire();equal([ctx.runtime.stats().views,ctx.posts()],[0,0],"early owners released");
    }finally{ctx.close();}
  });
  await test("history failure leaves the full received subtree and ancestor draft intact",async()=>{
    const ctx=await setup({start:false}),replace=history.replaceState;try{const middle=layout(ctx.root,"propMiddle"),input=own(layout(ctx.root,"propRoot")).querySelector("input");type(input,"History draft");history.replaceState=()=>{throw new Error("History fixture");};equal(await ctx.start(),"native","history refused");assert(middle.isConnected,"subtree not removed");equal(input.value,"History draft","draft retained");ctx.session.retire();equal(ctx.runtime.stats().views,0,"all early owners retired");}finally{history.replaceState=replace;ctx.close();}
  });
  await test("history side effects cannot prune a changed received subtree",async()=>{
    const ctx=await setup({start:false}),replace=history.replaceState;try{const middle=layout(ctx.root,"propMiddle");history.replaceState=function(...args){own(middle).setAttribute("data-extra","changed");return replace.apply(this,args);};equal(await ctx.start(),"native","changed DOM refused");assert(middle.isConnected,"subtree retained");ctx.session.retire();equal([ctx.runtime.stats().views,ctx.posts()],[0,0],"no retired owner or POST");}finally{history.replaceState=replace;ctx.close();}
  });
  await test("a native form guard refuses pruning before any owner is retired",async()=>{
    const ctx=await setup({start:false});try{const middle=layout(ctx.root,"propMiddle");equal(await ctx.start(()=>false),"native","guard declined");assert(middle.isConnected,"received subtree retained");equal(ctx.runtime.stats().views,3,"owners still gated until retirement");}finally{ctx.close();}
  });
  await test("a disposed descendant owner cannot commit retirement",async()=>{
    const ctx=await setup({start:false});try{const middle=layout(ctx.root,"propMiddle");ctx.initial.parents[2].dispose();equal(await ctx.start(),"native","stale owner refused");assert(middle.isConnected,"no subtree removal");equal(ctx.posts(),0,"no action");}finally{ctx.close();}
  });
  await test("view budget exhaustion cannot remove the received subtree",async()=>{
    const ctx=await setup({start:false}),hosts=[];try{while(ctx.runtime.stats().views<16){const host=document.createElement("div");document.body.append(host);hosts.push(host);ctx.runtime.mount(host,{component:1,state:{kind:"number",value:0}});}const middle=layout(ctx.root,"propMiddle");equal(await ctx.start(),"native","budget refused");assert(middle.isConnected,"subtree still present");equal(ctx.posts(),0,"no POST");}finally{ctx.close();hosts.forEach(host=>host.remove());}
  });
  await test("a newer destination refuses retirement of the old received branch",async()=>{
    const ctx=await setup({start:false});try{const middle=layout(ctx.root,"propMiddle");history.replaceState({},"","/tasks/settings");equal(await ctx.start(),"native","stale destination refused");assert(middle.isConnected,"old subtree retained");equal(ctx.posts(),0,"no POST");}finally{ctx.close();}
  });
  await test("a retired initial session cannot remove its received descendants",async()=>{
    const ctx=await setup({start:false});try{const middle=layout(ctx.root,"propMiddle");ctx.session.retire();equal(await ctx.start(),"native","session cancelled");assert(middle.isConnected,"received DOM retained");equal(ctx.runtime.stats().views,0,"no late owner");}finally{ctx.close();}
  });
  await test("an incomplete retirement record refuses default document activation",async()=>{
    const ctx=await setup({start:false,early:false});try{const middle=layout(ctx.root,"propMiddle"),record=ctx.root.querySelector("#ruvane-retired");record.textContent=record.textContent.slice(0,-2);equal(await ctx.start(),"native","truncated prefix refused");assert(middle.isConnected,"no partial removal");equal([ctx.runtime.stats().views,ctx.posts()],[0,0],"no owner or POST");}finally{ctx.close();}
  });
  await test("a successful branch rejects unnecessary retirement metadata",async()=>{
    await mode(500);const record=(await source(path)).querySelector("#ruvane-retired"),ctx=await setup({start:false,status:0});try{const middle=layout(ctx.root,"propMiddle");ctx.root.querySelector("#ruvane-initial").before(document.importNode(record,true));equal(await ctx.start(),"native","unexpected retirement refused");assert(middle.isConnected,"healthy subtree retained");equal(ctx.posts(),0,"no POST");}finally{ctx.close();}
  });
  await test("one hundred startups of each document profile return runtime counts to baseline",async()=>{
    await mode(500);const templates=[await source(path),await source(path+"?hydrate=no")],runtime=await createRuntime(moduleURL,{applicationRevision:revision}),baseline=runtime.stats(),replace=history.replaceState;window.fetch=()=>{throw new Error("Unexpected data request");};
    // Skip redundant history writes in the cleanup loop.
    history.replaceState=function(state,title,url){if(url!==undefined&&new URL(url,location.href).href!==location.href)return replace.call(this,state,title,url);};
    try{for(const early of [true,false])for(let n=0;n<100;n++){
      const url=path+(early?"":"?hydrate=no");let root,session,navigator,stage="insert";
      try{root=insert(templates[early?0:1],url);stage="snapshot";session=early?createDocumentHydration(runtime,root):null;const initial=session?await session.ready:{snapshot:readInitialSnapshot(root),parents:[]};stage="start";navigator=createNavigator(runtime,{scope:root,outlet:root.querySelector("#ruvane-outlet"),status:root.querySelector("#ruvane-status"),boundaries:readBoundaries(root)});equal(await navigator.start(initial),"success","error adopted");session?.commit();}
      catch(error){throw new Error(`${early?"early":"default"} cycle ${n} ${stage}: ${error.message}; URL ${location.pathname+location.search}`);}
      finally{session?.retire();navigator?.dispose();root?.remove();}
      equal(runtime.stats(),baseline,"resources released");
    }}finally{history.replaceState=replace;window.fetch=nativeFetch;runtime.dispose();}
  });
  window.fetch=nativeFetch;await mode(500);const result={passed:results.filter(row=>row.passed).length,failed:results.filter(row=>!row.passed).length,results};report.textContent=JSON.stringify(result,null,2);summary.textContent=`${result.passed}/${results.length} parent recovery checks passed`;
});

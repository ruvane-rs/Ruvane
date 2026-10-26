const report=document.getElementById("shell-hydration-report"),summary=document.getElementById("shell-hydration-summary"),results=[];
const assert=(value,message)=>{if(!value)throw new Error(message);},equal=(a,b,message)=>assert(JSON.stringify(a)===JSON.stringify(b),`${message}: ${JSON.stringify(a)}`);
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(check){const end=performance.now()+55000;while(!check()){if(performance.now()>end)throw new Error("Shell check timed out");await wait(5);}}
async function test(name,fn){try{await fn();results.push({name,passed:true});}catch(error){results.push({name,passed:false,error:String(error.message)});}report.textContent=JSON.stringify(results,null,2);summary.textContent=`Completed ${results.length}: ${name}`;}
const nativeFetch=window.fetch;
async function control(path,fields={}){const response=await nativeFetch(path,{method:"POST",headers:{"Content-Type":"application/x-www-form-urlencoded"},body:new URLSearchParams(fields)});assert(response.ok,`Control ${path}: ${response.status}`);return response;}
const mode=(stage=2,code=0,bad=0)=>control("/fixtures/global-mode",{stage:String(stage),code:String(code),bad:String(bad)}),base="/tasks/global-fixture",source=async url=>new DOMParser().parseFromString(await(await nativeFetch(url)).text(),"text/html").getElementById("ruvane-application");
await mode();const configuration=await source(base),moduleURL=configuration.getAttribute("data-ruvane-module"),revision=configuration.getAttribute("data-ruvane-revision");
const {createRuntime}=await import(new URL("./bridge.mjs",new URL(moduleURL,location.href)));
const {createNavigator,createDocumentHydration,readBoundaries}=await import(new URL("./navigation.mjs",new URL(moduleURL,location.href)));
const island=(root,name)=>root.querySelector(`[data-ruvane-shell="${name}"]`),editor=root=>island(root,"fixtureEditor"),button=(root,label)=>[...root.querySelectorAll("button")].find(node=>node.textContent===label);
const type=(field,value)=>{field.value=value;field.dispatchEvent(new InputEvent("input",{bubbles:true}));};
function references(root){const nodes=[];function visit(node){nodes.push(node);for(const child of node.childNodes)visit(child);}visit(root);return nodes;}
function sameNodes(nodes,root){const after=references(root);assert(nodes.length===after.length&&nodes.every((node,n)=>node===after[n]),"shell nodes retained");}
async function setup({layouts=true,stage=2,code=0,start=true,beforeSession=null,shared=null,template=null,bad=0,timeoutMs}={}){
  await mode(stage,code,bad);const url=base+"?hydrate="+(layouts?"yes":"no");history.replaceState({},"",url);
  const root=document.importNode(template??await source(url),true);for(const script of root.querySelectorAll("script"))script.remove();document.body.append(root);
  const runtime=shared??await createRuntime(moduleURL,{applicationRevision:revision});let session,initial,navigator,reads=0,posts=0,transport=()=>{throw new Error("Unexpected initial fetch");};
  window.fetch=(url,options={})=>{if(options.method==="POST")posts++;else reads++;return transport(url,options);};
  const ctx={root,runtime,get session(){return session;},get initial(){return initial;},get navigator(){return navigator;},reads:()=>reads,posts:()=>posts,enable(){transport=nativeFetch;},async prepare(){beforeSession?.(root);session=createDocumentHydration(runtime,root,{timeoutMs});initial={...await session.ready};navigator=createNavigator(runtime,{scope:root,outlet:root.querySelector("#ruvane-outlet"),status:root.querySelector("#ruvane-status"),boundaries:readBoundaries(root),progressive:true});return initial;},async start(options={}){const result=await navigator.start({...initial,...options,accept:()=>initial.accept()&&(!options.accept||options.accept())});if(result==="success")session.commit();return result;},close(){window.fetch=nativeFetch;session?.retire();navigator?.dispose();for(const row of initial?.shells??[])row.controller.dispose();if(!shared)runtime.dispose();root.remove();}};
  if(start){try{await ctx.prepare();equal(await ctx.start(),"success","document activated");}catch(error){ctx.close();throw error;}}return ctx;
}
const run=document.createElement("button");run.textContent="Run shell hydration checks";document.body.prepend(run);
run.addEventListener("click",async()=>{
  run.disabled=true;
  const child=window.open("about:blank","ruvane-shell-hydration-"+revision);if(!child){summary.textContent="The browser blocked the test window.";return;}
  await test("independent shell views validate through the gated runtime",async()=>{
    const ctx=await setup({start:false}),owners=[];try{for(const host of ctx.root.querySelectorAll("[data-ruvane-shell]"))owners.push(ctx.runtime.hydrateShell(host.firstChild,host.getAttribute("data-ruvane-shell"),base+"?hydrate=yes"));equal(ctx.runtime.stats().views,3,"three gated views");owners[0].dispatch(1);equal(island(ctx.root,"toolbar").querySelector("output").textContent,"6","counter increment");owners[0].dispatch(4);assert(button(island(ctx.root,"toolbar"),"Hide tools"),"menu toggled");equal(ctx.runtime.stats().state,"active","runtime active after local events");ctx.runtime.validateShells(owners,base+"?hydrate=yes");}finally{for(const owner of owners)ctx.runtime.retireShell(owner);ctx.close();}
  });
  try{for(const [layouts,stage,code] of [[false,2,0],[true,2,0],[true,0,500]]){
    await mode(stage,code);await control("/fixtures/prepare");const previousDocument=child.document;child.location.href=new URL(base+`?document-gate=yes&hydrate=${layouts?"yes":"no"}`,location.href).href;
    let shell,toolbar,input,notes,nodes,reads=0,posts=0;
    await test(`${layouts?"layouts and shell":"shell only"}, status ${code}: local events work before completion`,async()=>{
      await until(()=>{return child.document!==previousDocument&&editor(child.document)&&button(editor(child.document),"Increment draft layout")?.disabled===false;});
      shell=editor(child.document);toolbar=island(child.document,"toolbar");input=shell.querySelector("input");notes=shell.querySelector("textarea");
      assert(child.document.readyState==="loading"&&!child.document.querySelector("#ruvane-complete")&&!island(child.document,"fixtureFooter"),"page and footer still pending");
      assert(button(child.document.querySelector("#fixture-unmarked"),"Increment draft layout").disabled,"unmarked view remains inert");
      const fetch=child.fetch.bind(child);child.fetch=(url,options={})=>{if(options.method==="POST")posts++;else if(String(url).includes(base))reads++;return fetch(url,options);};
      button(toolbar,"Increment").click();button(toolbar,"Show tools").click();button(shell,"Increment draft layout").click();
      type(input,"Shell 🦀 draft");type(notes,"Keep shell notes");button(shell,"Send layout action").click();assert(shell.textContent.includes("Action blocked"),`gate result: ${shell.textContent}; status=${child.document.querySelector("#ruvane-status")?.textContent}; posts=${posts}`);input.focus();input.setSelectionRange(2,8);nodes=references(shell);
      equal([toolbar.querySelector("output").textContent,shell.querySelector("output").textContent,reads,posts],["6","1",0,0],"local state and blocked POST");
    });
    await control("/fixtures/release");
    await test(`${layouts?"layouts and shell":"shell only"}, status ${code}: completion adopts existing shell owners`,async()=>{
      await until(()=>{const status=child.document.querySelector("#ruvane-status")?.textContent;assert(!status?.startsWith("Interactive mode is unavailable"),"real bootstrap refused the branch");return status===(code?"Page 500.":"Ready.");});
      sameNodes(nodes,shell);assert(editor(child.document)===shell&&input===shell.querySelector("input")&&child.document.activeElement===input,"same nodes and focused input");
      equal([input.value,notes.value,input.selectionStart,input.selectionEnd],["Shell 🦀 draft","Keep shell notes",2,8],"draft and UTF-16 selection");
      assert(island(child.document,"fixtureFooter")&&!button(island(child.document,"fixtureFooter"),"Increment draft layout").disabled,"footer activated after delivery");
      const unmarked=child.document.querySelector("#fixture-unmarked");button(unmarked,"Increment draft layout").click();equal(unmarked.querySelector("output").textContent,"1","unmarked view activated once at completion");
      equal([reads,posts],[0,0],"no request replay at commit");
    });
    await test(`${layouts?"layouts and shell":"shell only"}, status ${code}: retry or navigation preserves the independent shell`,async()=>{
      input.focus();input.setSelectionRange(1,7);if(code){await mode(stage,0);button(child.document,"Try again").click();await until(()=>child.document.querySelector("#ruvane-status")?.textContent==="Ready.");equal(reads,1,"one explicit retry GET");}
      else{const link=[...child.document.querySelectorAll("a")].find(node=>node.getAttribute("href")==="/tasks");link.click();await until(()=>child.document.querySelector("#ruvane-status")?.textContent==="Ready."&&child.document.querySelector("#ruvane-outlet")?.textContent.includes("Server tasks"));}
      sameNodes(nodes,shell);equal([input.value,notes.value,toolbar.querySelector("output").textContent,posts],["Shell 🦀 draft","Keep shell notes","6",0],"shell state retained");
    });
    await test(`${layouts?"layouts and shell":"shell only"}, status ${code}: only an explicit post-commit action sends a POST`,async()=>{
      assert(!button(shell,"Send layout action").disabled,"shell action enabled after commit");button(shell,"Send layout action").click();await until(()=>shell.textContent.includes("Action sent"));equal(posts,1,"one explicit POST");nodes=references(shell);
    });
  }}finally{await control("/fixtures/release");child.close();}
  for(const layouts of [true,false])for(const [stage,code] of [[2,0],[0,500],[1,404],[2,400],[2,600]])await test(`${layouts?"early layouts":"shell only"} validates completion at stage ${stage}, code ${code}`,async()=>{
    const ctx=await setup({layouts,stage,code});try{equal(ctx.initial.shells.length,3,"three shell owners");equal(ctx.initial.parents.length,layouts?stage===0?0:stage===1?1:2:0,"early parent count");equal(ctx.runtime.stats().views,code?4:6,"shell owners and committed branch");assert(!ctx.root.querySelector("[data-ruvane-shell-ready]"),"markers consumed");equal([ctx.reads(),ctx.posts()],[0,0],"no startup GET or POST");}finally{ctx.close();}
  });
  for(const mutation of ["names","duplicate","missing","marker","nested","component","dom","oversized","mode","revision","completion"])await test(`invalid shell ${mutation} retires every early owner`,async()=>{
    const ctx=await setup({start:false,beforeSession(root){const host=editor(root);if(mutation==="names")root.setAttribute("data-ruvane-shells","toolbar,toolbar");if(mutation==="duplicate")host.before(host.cloneNode(true));if(mutation==="missing")host.nextSibling.remove();if(mutation==="marker")host.nextSibling.setAttribute("data-ruvane-shell-ready","foreign");if(mutation==="nested")root.querySelector("#ruvane-outlet").append(host,host.nextSibling);if(mutation==="component")host.firstChild.setAttribute("data-ruvane-component","999");if(mutation==="dom")host.querySelector("h2").textContent="Foreign shell";if(mutation==="oversized")host.firstChild.append(...Array.from({length:1024},()=>document.createElement("span")));if(mutation==="mode")root.setAttribute("data-ruvane-layout-hydration","yes");if(mutation==="revision")root.setAttribute("data-ruvane-revision","0");if(mutation==="completion")root.querySelector("#ruvane-complete").remove();}});
    try{let rejected=false;try{await ctx.prepare();}catch{rejected=true;}assert(rejected,"invalid document refused");equal(ctx.runtime.stats().views,0,"all early owners released");equal(ctx.posts(),0,"no POST");assert(ctx.root.querySelector("#ruvane-outlet").isConnected,"native HTML retained");}finally{ctx.close();}
  });
  await test("adopting a shell field cannot automatically send a server action",async()=>{
    const ctx=await setup({start:false,beforeSession(root){editor(root).querySelector("input").value="Autosave fixture rejected";}});try{let rejected=false;try{await ctx.prepare();}catch{rejected=true;}assert(rejected,"action during adoption refused");equal([ctx.runtime.stats().views,ctx.posts()],[0,0],"no owner or POST");equal(editor(ctx.root).querySelector("input").value,"Autosave fixture rejected","draft retained");}finally{ctx.close();}
  });
  await test("a shell component with an initial action cannot activate",async()=>{const ctx=await setup({start:false,bad:21});try{let rejected=false;try{await ctx.prepare();}catch{rejected=true;}assert(rejected,"initial action refused");equal([ctx.runtime.stats().views,ctx.posts()],[0,0],"no owner or POST");}finally{ctx.close();await mode();}});
  for(const guard of ["native","history","history-shell","late-shell"])await test(`${guard} refusal preserves native HTML and releases shell gates`,async()=>{
    const ctx=await setup({start:false}),replace=history.replaceState;let calls=0;try{await ctx.prepare();const input=editor(ctx.root).querySelector("input");type(input,"Guarded shell draft");if(guard==="history"||guard==="history-shell")history.replaceState=function(...args){if(guard==="history")throw new Error("History fixture");editor(ctx.root).setAttribute("data-ruvane-shell","changed");return replace.apply(this,args);};equal(await ctx.start({accept:()=>{if(guard==="late-shell"&&++calls===3)editor(ctx.root).setAttribute("data-ruvane-shell","changed");return guard!=="native";}}),"native","activation refused");ctx.session.retire();equal(ctx.runtime.stats().views,0,"all early owners retired");equal([input.value,ctx.posts()],["Guarded shell draft",0],"draft preserved without POST");assert(ctx.root.querySelector("#ruvane-fallback"),"native fallback retained");}finally{history.replaceState=replace;ctx.close();}
  });
  for(const reason of ["cancel","detach","destination","owner"])await test(`${reason} invalidates shell handoff before branch activation`,async()=>{
    const ctx=await setup({start:false});try{await ctx.prepare();if(reason==="cancel")ctx.session.retire();if(reason==="detach")ctx.root.remove();if(reason==="destination")history.replaceState({},"","/tasks/settings");if(reason==="owner")ctx.initial.shells[0].controller.dispose();await wait(0);equal(await ctx.start(),"native","late activation refused");ctx.session.retire();equal([ctx.runtime.stats().views,ctx.posts()],[0,0],"no retained owner or POST");}finally{ctx.close();}
  });
  await test("a rejected final packet cannot release shell action gates",async()=>{
    const ctx=await setup({start:false});try{await ctx.prepare();const bytes=ctx.initial.snapshot.slice(),view=new DataView(bytes.buffer);let p=23;p+=4+view.getUint32(p,true);p+=4+view.getUint32(p,true);view.setUint32(p,999,true);ctx.initial.snapshot=bytes;
      equal(await ctx.start(),"native","forged final page refused");button(editor(ctx.root),"Send layout action").click();equal(ctx.posts(),0,"shell action still gated");assert(editor(ctx.root).textContent.includes("Action blocked"),"local rejection");ctx.session.retire();equal(ctx.runtime.stats().views,0,"owners retired");
    }finally{ctx.close();}
  });
  await test("a foreign owner cannot validate, release or retire a shell gate",async()=>{
    const ctx=await setup({start:false});let normal;const root=document.createElement("div");document.body.append(root);try{await ctx.prepare();normal=ctx.runtime.mount(root,{component:1,state:{kind:"number",value:5}});for(const controllers of [[normal],[ctx.initial.shells[0].controller,ctx.initial.shells[0].controller]]){let rejected=false;try{ctx.runtime.validateShells(controllers,base+"?hydrate=yes");}catch{rejected=true;}assert(rejected,"foreign or duplicate owner rejected");}assert(!ctx.runtime.retireShell(normal),"ordinary owner retained");equal(await ctx.start(),"success","real shell handoff");normal.dispatch(1);equal(root.querySelector("output").textContent,"6","ordinary owner active");}finally{normal?.dispose();root.remove();ctx.close();}
  });
  await test("exhausting the view budget releases partial shell activation",async()=>{
    const ctx=await setup({start:false}),roots=[],owners=[];try{for(let n=0;n<14;n++){const root=document.createElement("div");document.body.append(root);roots.push(root);owners.push(ctx.runtime.mount(root,{component:1,state:{kind:"number",value:0}}));}let rejected=false;try{await ctx.prepare();}catch{rejected=true;}assert(rejected,"budget refused");equal(ctx.runtime.stats().views,14,"ordinary owners retained");owners[0].dispatch(1);equal(roots[0].querySelector("output").textContent,"1","ordinary owner active");equal(ctx.posts(),0,"no POST");}finally{for(const owner of owners)owner.dispose();for(const root of roots)root.remove();ctx.close();}
  });
  await test("a ready shell session expires without leaking owners or replaying actions",async()=>{
    const ctx=await setup({start:false,timeoutMs:80});try{await ctx.prepare();await wait(110);assert(!ctx.initial.accept(),"expired handoff refused");equal([ctx.runtime.stats().views,ctx.posts()],[0,0],"all owners released");}finally{ctx.close();}
  });
  await test("one hundred activations per profile release shell, branch, listeners and Wasm owners",async()=>{
    await mode();const runtime=await createRuntime(moduleURL,{applicationRevision:revision}),baseline=runtime.stats(),replace=history.replaceState,templates=[await source(base+"?hydrate=yes"),await source(base+"?hydrate=no")];
    // Skip redundant history writes in this resource loop.
    history.replaceState=function(state,title,url){if(url!==undefined&&new URL(url,location.href).href!==location.href)return replace.call(this,state,title,url);};
    try{for(const layouts of [true,false])for(let n=0;n<100;n++){const ctx=await setup({layouts,shared:runtime,template:templates[layouts?0:1]});try{const shell=editor(ctx.root);button(shell,"Increment draft layout").click();}finally{ctx.close();}equal(runtime.stats(),baseline,"resources returned to baseline");}}finally{history.replaceState=replace;runtime.dispose();}
  });
  window.fetch=nativeFetch;await mode();const result={passed:results.filter(row=>row.passed).length,failed:results.filter(row=>!row.passed).length,results};report.textContent=JSON.stringify(result,null,2);summary.textContent=`${result.passed}/${results.length} shell hydration checks passed`;
});

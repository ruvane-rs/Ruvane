const report=document.getElementById("layout-hydration-report"),summary=document.getElementById("layout-hydration-summary"),results=[];
const assert=(value,message)=>{if(!value)throw new Error(message);};
const equal=(a,b,message)=>assert(JSON.stringify(a)===JSON.stringify(b),`${message}: ${JSON.stringify(a)}`);
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(check){const end=performance.now()+55000;while(!check()){if(performance.now()>end)throw new Error("Layout hydration check timed out");await wait(5);}}
async function test(name,fn){try{await fn();results.push({name,passed:true});}catch(error){results.push({name,passed:false,error:String(error.message)});}report.textContent=JSON.stringify(results,null,2);}
const nativeFetch=window.fetch,control=path=>nativeFetch(path,{method:"POST",headers:{"Content-Type":"application/x-www-form-urlencoded"},body:""});
const source=async url=>new DOMParser().parseFromString(await(await nativeFetch(url)).text(),"text/html").getElementById("ruvane-application");
const configuration=await source("/tasks/live"),moduleURL=configuration.getAttribute("data-ruvane-module"),revision=configuration.getAttribute("data-ruvane-revision");
const {createRuntime}=await import(new URL("./bridge.mjs",new URL(moduleURL,location.href)));
const {createNavigator,createDocumentHydration,readBoundaries}=await import(new URL("./navigation.mjs",new URL(moduleURL,location.href)));
const own=layout=>layout.querySelector(":scope > [data-ruvane-layout-view]");
const layout=(root,name)=>root.querySelector(`[data-ruvane-layout="${name}"]`);
const button=(root,label)=>[...root.querySelectorAll("button")].find(node=>node.textContent===label);
const type=(field,value)=>{field.value=value;field.dispatchEvent(new InputEvent("input",{bubbles:true}));};
function references(root){const nodes=[];function visit(node){nodes.push(node);for(const child of node.childNodes)visit(child);}visit(root);return nodes;}
function sameNodes(before,root){const after=references(root);assert(after.length===before.length&&after.every((node,n)=>node===before[n]),"same element and text nodes");}
function insert(template,url){history.replaceState(null,"",url);const root=document.importNode(template,true);for(const script of root.querySelectorAll("script"))script.remove();document.body.append(root);return root;}
async function setup(url="/tasks/live"){
  const root=insert(await source(url),url),outlet=root.querySelector("#ruvane-outlet"),status=root.querySelector("#ruvane-status"),runtime=await createRuntime(moduleURL,{applicationRevision:revision});
  const navigator=createNavigator(runtime,{scope:root,outlet,status,boundaries:readBoundaries(root),progressive:true});let session,initial,reads=0,posts=0,allow=false;
  window.fetch=(url,options={})=>{if(options.method==="POST")posts++;else reads++;if(allow)return nativeFetch(url,options);throw new Error("Unexpected startup request");};
  return{root,outlet,status,runtime,navigator,reads:()=>reads,posts:()=>posts,enable(){allow=true;},get initial(){return initial;},get session(){return session;},async prepare(){session=createDocumentHydration(runtime,root);initial=await session.ready;return initial;},async start(){const outcome=await navigator.start(initial);if(outcome==="success")session.commit();else session.retire();return outcome;},close(){window.fetch=nativeFetch;session?.retire();navigator.dispose();runtime.dispose();root.remove();}};
}
const run=document.createElement("button"),release=document.createElement("button");run.textContent="Run layout hydration checks";release.textContent="Release streamed leaf";release.disabled=true;document.body.prepend(run,release);
release.addEventListener("click",async()=>{release.disabled=true;await control("/fixtures/release");});
run.addEventListener("click",async()=>{
  run.disabled=true;let child=window.open("about:blank","ruvane-layout-hydration");if(!child){summary.textContent="The browser blocked the test window.";return;}
  try{
    await control("/fixtures/prepare");child.location.href="/tasks/live?document-gate=yes";
    let workspace,draft,workspaceNodes,draftNodes,input,textarea,reads=0,posts=0;
    await test("a real initial document activates its layouts while the leaf is still blocked",async()=>{
      await until(()=>{const draft=layout(child.document,"draft");return draft&&button(own(draft),"Increment draft layout")?.disabled===false;});
      workspace=layout(child.document,"workspace");draft=layout(child.document,"draft");
      assert(child.document.readyState==="loading"&&!child.document.querySelector("[data-ruvane-page]"),"leaf still loading");assert(workspace.getBoundingClientRect().height>0&&!own(workspace).querySelector("button").disabled,"visible interactive workspace");
      assert(!child.document.querySelector("[data-ruvane-segment]")&&!child.document.getElementById("ruvane-initial"),"layout records consumed before the final snapshot");
      const fetch=child.fetch.bind(child);child.fetch=(url,options={})=>{if(options.method==="POST")posts++;else reads++;return fetch(url,options);};
    });
    await test("local events work and server actions are refused before branch commitment",async()=>{
      own(workspace).querySelector("button").click();button(own(draft),"Increment draft layout").click();button(own(draft),"Send layout action").click();
      equal(own(workspace).querySelector("output").textContent,"1","workspace counter");equal(own(draft).querySelector("output").textContent,"1","draft counter");assert(own(draft).textContent.includes("Action blocked"),"local action rejection");equal(posts,0,"no POST before completion");
      input=own(draft).querySelector("input");textarea=own(draft).querySelector("textarea");input.value="Early layout draft 🦀";input.dispatchEvent(new child.InputEvent("input",{bubbles:true}));textarea.value="Notes during streaming";textarea.dispatchEvent(new child.InputEvent("input",{bubbles:true}));input.focus();input.setSelectionRange(2,8);
      workspaceNodes=references(own(workspace).firstChild);draftNodes=references(own(draft).firstChild);summary.textContent="Layouts are interactive while the leaf is blocked. Release the leaf.";release.disabled=false;
    });
    await test("the final branch adopts live parents with their state, DOM, focus and selection",async()=>{
      await until(()=>child.document.getElementById("ruvane-status")?.textContent==="Ready.");
      assert(layout(child.document,"workspace")===workspace&&layout(child.document,"draft")===draft,"same parent hosts");sameNodes(workspaceNodes,own(workspace).firstChild);sameNodes(draftNodes,own(draft).firstChild);
      equal([own(workspace).querySelector("output").textContent,own(draft).querySelector("output").textContent],["1","1"],"counter state retained");equal([input.value,textarea.value,input.selectionStart,input.selectionEnd],["Early layout draft 🦀","Notes during streaming",2,8],"draft and selection retained");assert(child.document.activeElement===input,"focus retained");equal([reads,posts],[0,0],"no repeated GET or activation POST");
      button(own(draft),"Send layout action").click();await until(()=>own(draft).textContent.includes("Action sent"));equal(posts,1,"an explicit action is available after commit");
    });
  }finally{await control("/fixtures/release");release.disabled=true;child.close();}
  await test("complete snapshots adopt prepared parents without a second data GET",async()=>{
    const ctx=await setup();try{const root=own(layout(ctx.root,"workspace")).firstChild;await ctx.prepare();equal(ctx.runtime.stats().views,2,"only parents hydrated");ctx.initial.parents[0].dispatch(1);const before=references(root);equal(await ctx.start(),"success","startup");sameNodes(before,root);equal(root.querySelector("output").textContent,"1","parent state retained");equal(ctx.runtime.stats().views,3,"two parents and leaf");equal([ctx.reads(),ctx.posts()],[0,0],"no extra request");assert(!ctx.runtime.retireLayout(ctx.initial.parents[0]),"parent ownership transferred");}finally{ctx.close();}
  });
  await test("early layout text edits are adopted before the leaf becomes interactive",async()=>{
    const ctx=await setup();try{const ownRoot=own(layout(ctx.root,"draft")),input=ownRoot.querySelector("input"),notes=ownRoot.querySelector("textarea");input.value="Before layout activation 🦀";notes.value="Existing notes";input.focus();input.setSelectionRange(1,7);await ctx.prepare();equal(await ctx.start(),"success","startup");assert(document.activeElement===input,"focus retained");equal([input.value,notes.value,input.selectionStart,input.selectionEnd],["Before layout activation 🦀","Existing notes",1,7],"early text retained");equal(ctx.posts(),0,"no automatic action");}finally{ctx.close();}
  });
  await test("input adoption cannot automatically send an action from a newly hydrated layout",async()=>{
    const ctx=await setup();try{const input=own(layout(ctx.root,"draft")).querySelector("input");input.value="Autosave fixture draft";let failed=false;try{await ctx.prepare();}catch{failed=true;}assert(failed,"automatic action refused");equal(ctx.posts(),0,"no POST");equal(ctx.runtime.stats().views,0,"all early owners released");equal(input.value,"Autosave fixture draft","draft retained");}finally{ctx.close();}
  });
  await test("incomplete, duplicate, oversized and selected layout records fail closed",async()=>{
    for(const mode of ["ready","duplicate","hex","size","selected","count","unknown"]){const ctx=await setup();try{const node=ctx.root.querySelector("[data-ruvane-segment]");if(mode==="ready")node.lastChild.remove();else if(mode==="duplicate")node.parentElement.append(node.cloneNode(true));else if(mode==="hex")node.firstChild.data="invalid";else if(mode==="size")node.firstChild.data="0".repeat(131106);else if(mode==="selected")node.firstChild.data=node.firstChild.data.slice(0,40)+"01"+node.firstChild.data.slice(42);else if(mode==="count")ctx.root.setAttribute("data-ruvane-layouts","3");else ctx.root.setAttribute("data-ruvane-document","unknown");let failed=false;try{await ctx.prepare();}catch{failed=true;}assert(failed,`${mode} refused`);equal(ctx.runtime.stats().views,0,"no retained owners");equal([ctx.reads(),ctx.posts()],[0,0],"no recovery request");}finally{ctx.close();}}
  });
  await test("out-of-order records cannot hydrate a descendant before its parent",async()=>{
    const ctx=await setup();try{const records=ctx.root.querySelectorAll("[data-ruvane-segment]");records[0].firstChild.data=records[1].firstChild.data;let failed=false;try{await ctx.prepare();}catch{failed=true;}assert(failed,"order checked");equal(ctx.runtime.stats().views,0,"no retained owner");}finally{ctx.close();}
  });
  await test("an invalid final leaf schema retires all previously validated parents",async()=>{
    const ctx=await setup();try{const node=ctx.root.querySelector("#ruvane-initial"),hex=node.textContent,bytes=Uint8Array.from(hex.match(/../g),x=>parseInt(x,16)),view=new DataView(bytes.buffer);let offset=16+5+2;offset+=4+view.getUint32(offset,true);offset+=4+view.getUint32(offset,true);view.setUint32(offset,999,true);node.textContent=[...bytes].map(x=>x.toString(16).padStart(2,"0")).join("");let failed=false;try{await ctx.prepare();}catch{failed=true;}assert(failed,"schema rejected");equal(ctx.runtime.stats().views,0,"parents released");equal(ctx.posts(),0,"no POST");}finally{ctx.close();}
  });
  await test("stale layout revisions and changed destinations cannot activate",async()=>{
    for(const mode of ["revision","target"]){const ctx=await setup();try{if(mode==="revision"){const node=ctx.root.querySelector("[data-ruvane-segment]");node.firstChild.data=node.firstChild.data.slice(0,8)+"00"+node.firstChild.data.slice(10);}else history.replaceState(null,"","/tasks/live?changed=yes");let failed=false;try{await ctx.prepare();}catch{failed=true;}assert(failed,"identity rejected");equal(ctx.runtime.stats().views,0,"no owner");}finally{ctx.close();}}
  });
  await test("a rejected final snapshot preserves edits and releases early event owners",async()=>{
    const ctx=await setup();try{await ctx.prepare();const input=own(layout(ctx.root,"draft")).querySelector("input");type(input,"Keep this draft");ctx.initial.snapshot[4]^=1;equal(await ctx.start(),"native","final revision rejected");equal(input.value,"Keep this draft","draft retained");equal(ctx.runtime.stats().views,0,"early owners released");equal([ctx.reads(),ctx.posts()],[0,0],"no hidden recovery");}finally{ctx.close();}
  });
  await test("history failure rolls back the leaf while retaining the received parent DOM",async()=>{
    const ctx=await setup(),replace=history.replaceState;try{await ctx.prepare();const parent=own(layout(ctx.root,"draft")).firstChild,input=parent.querySelector("input"),before=references(parent);type(input,"History draft");ctx.initial.parents[0].dispatch(1);history.replaceState=()=>{throw new Error("History fixture");};equal(await ctx.start(),"native","commit refused");sameNodes(before,parent);equal(input.value,"History draft","draft retained");equal(ctx.runtime.stats().views,0,"all temporary owners released");equal(ctx.posts(),0,"no POST");}finally{history.replaceState=replace;ctx.close();}
  });
  await test("a dirty native fallback declines leaf activation and retires early parents",async()=>{
    const ctx=await setup("/tasks/live?native-fallback=yes");try{await ctx.prepare();const field=ctx.root.querySelector("#ruvane-fallback input");assert(field,"native fixture present");field.value="Native draft";field.focus();equal(await ctx.navigator.start({...ctx.initial,accept:()=>false}),"native","native fallback retained");ctx.session.retire();equal(field.value,"Native draft","native draft retained");equal(ctx.runtime.stats().views,0,"early parents released");equal(ctx.posts(),0,"no POST");}finally{ctx.close();}
  });
  await test("view budget exhaustion cannot leak the prepared leaf or early parents",async()=>{
    const ctx=await setup(),islands=[];try{for(let n=0;n<14;n++){const host=document.createElement("div");document.body.append(host);islands.push([host,ctx.runtime.mount(host,{component:1,state:{kind:"number",value:5}})]);}await ctx.prepare();equal(ctx.runtime.stats().views,16,"view budget full");equal(await ctx.start(),"native","leaf rejected");equal(ctx.runtime.stats().views,14,"temporary parents released");equal(ctx.posts(),0,"no POST");}finally{for(const [host,controller]of islands){controller.dispose();host.remove();}ctx.close();}
  });
  await test("adopted workspace state persists during subsequent client navigation",async()=>{
    const ctx=await setup();try{await ctx.prepare();const workspace=layout(ctx.root,"workspace");ctx.initial.parents[0].dispatch(1);equal(await ctx.start(),"success","startup");ctx.enable();equal(await ctx.navigator.navigate("/tasks/settings"),"success","sibling navigation");assert(layout(ctx.root,"workspace")===workspace,"same workspace");equal(own(workspace).querySelector("output").textContent,"1","state retained");equal(ctx.runtime.stats().views,2,"departing draft layout released");}finally{ctx.close();}
  });
  await test("a late server failure retires descendants and keeps its parent boundary active",async()=>{
    await control("/fixtures/prepare");child=window.open("/tasks/live?document-gate=yes&unavailable=yes","ruvane-layout-failure");try{assert(child,"test window");await until(()=>{const draft=layout(child.document,"draft");return draft&&button(own(draft),"Increment draft layout")?.disabled===false;});const draft=layout(child.document,"draft"),input=own(draft).querySelector("input");input.value="Draft before failure";input.dispatchEvent(new child.InputEvent("input",{bubbles:true}));await control("/fixtures/release");await until(()=>child.document.getElementById("ruvane-status")?.textContent==="Page 500.");equal(input.value,"Draft before failure","detached draft retained");assert(!draft.isConnected&&button(own(draft),"Increment draft layout").disabled&&child.document.querySelector("[data-ruvane-page]")&&!child.document.querySelector("[data-ruvane-stream-error]")&&!button(own(layout(child.document,"workspace")),"Increment layout").disabled,"descendants retired and parent active");}finally{await control("/fixtures/release");child?.close();}
  });
  await test("page departure prevents late leaf activation after a streamed prefix",async()=>{
    await control("/fixtures/prepare");child=window.open("/tasks/live?document-gate=yes","ruvane-layout-departure");try{assert(child,"test window");await until(()=>{const draft=layout(child.document,"draft");return draft&&button(own(draft),"Increment draft layout")?.disabled===false;});const draft=layout(child.document,"draft");child.dispatchEvent(new child.Event("pagehide"));await control("/fixtures/release");await until(()=>child.document.getElementById("ruvane-complete"));assert(button(own(draft),"Increment draft layout").disabled&&child.document.getElementById("ruvane-status").textContent!=="Ready.","late completion cannot reactivate owners");}finally{await control("/fixtures/release");child?.close();}
  });
  await test("removing a ready document root retires its pending owners",async()=>{
    const ctx=await setup();try{await ctx.prepare();const input=own(layout(ctx.root,"draft")).querySelector("input");type(input,"Detached draft");ctx.root.remove();await until(()=>ctx.runtime.stats().views===0);equal(input.value,"Detached draft","draft retained");equal(ctx.posts(),0,"no POST");}finally{ctx.close();}
  });
  await test("an uncommitted ready session expires without leaving listeners or Rust owners",async()=>{
    const ctx=await setup();let session;try{session=createDocumentHydration(ctx.runtime,ctx.root,{timeoutMs:30});await session.ready;await until(()=>ctx.runtime.stats().views===0);equal(ctx.runtime.stats().eventListeners,0,"listeners released");equal(ctx.posts(),0,"no POST");}finally{session?.retire();ctx.close();}
  });
  await test("one hundred early hydration, adoption and disposal cycles return to baseline",async()=>{
    const url="/tasks/live",template=await source(url),runtime=await createRuntime(moduleURL,{applicationRevision:revision}),baseline=runtime.stats();window.fetch=()=>{throw new Error("Unexpected data request");};
    try{for(let n=0;n<100;n++){const root=insert(template,url),session=createDocumentHydration(runtime,root),initial=await session.ready,navigator=createNavigator(runtime,{scope:root,outlet:root.querySelector("#ruvane-outlet"),status:root.querySelector("#ruvane-status"),boundaries:readBoundaries(root)});initial.parents[0].dispatch(1);equal(await navigator.start(initial),"success","cycle");session.commit();navigator.dispose();root.remove();equal(runtime.stats(),baseline,"all owners and listeners released");}}finally{window.fetch=nativeFetch;runtime.dispose();}
  });
  const result={passed:results.filter(row=>row.passed).length,failed:results.filter(row=>!row.passed).length,results};report.textContent=JSON.stringify(result,null,2);summary.textContent=`${result.passed}/${results.length} layout hydration checks passed`;
});

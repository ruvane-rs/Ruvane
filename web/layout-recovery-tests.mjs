const report=document.getElementById("layout-recovery-report"),summary=document.getElementById("layout-recovery-summary"),results=[];
const assert=(value,message)=>{if(!value)throw new Error(message);};
const equal=(a,b,message)=>assert(JSON.stringify(a)===JSON.stringify(b),`${message}: ${JSON.stringify(a)}`);
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(check){const end=performance.now()+55000;while(!check()){if(performance.now()>end)throw new Error("Layout recovery check timed out");await wait(5);}}
async function test(name,fn){try{await fn();results.push({name,passed:true});}catch(error){results.push({name,passed:false,error:String(error.message)});}report.textContent=JSON.stringify(results,null,2);}
const nativeFetch=window.fetch,control=(path,fields={})=>nativeFetch(path,{method:"POST",headers:{"Content-Type":"application/x-www-form-urlencoded"},body:new URLSearchParams(fields)});
const mode=value=>control("/fixtures/layout-mode",{mode:String(value)});
const stats=async()=> (await(await nativeFetch("/fixtures/layout-stats")).text()).split(",").map(Number);
const source=async url=>new DOMParser().parseFromString(await(await nativeFetch(url)).text(),"text/html").getElementById("ruvane-application");
const path="/tasks/layout-recovery",configuration=await source(path),moduleURL=configuration.getAttribute("data-ruvane-module"),revision=configuration.getAttribute("data-ruvane-revision");
const {createRuntime}=await import(new URL("./bridge.mjs",new URL(moduleURL,location.href)));
const {createNavigator,createDocumentHydration,readBoundaries}=await import(new URL("./navigation.mjs",new URL(moduleURL,location.href)));
const own=host=>host.querySelector(":scope > [data-ruvane-layout-view]"),layout=(root,name)=>root.querySelector(`[data-ruvane-layout="${name}"]`);
const button=(root,label)=>[...root.querySelectorAll("button")].find(node=>node.textContent===label);
const type=(field,value)=>{field.value=value;field.dispatchEvent(new InputEvent("input",{bubbles:true}));};
function references(root){const nodes=[];function visit(node){nodes.push(node);for(const child of node.childNodes)visit(child);}visit(root);return nodes;}
function sameNodes(before,root){const after=references(root);assert(after.length===before.length&&after.every((node,n)=>node===before[n]),"same ancestor nodes");}
function insert(template,url=path){history.replaceState({fixture:"layout-recovery"},"",url);const root=document.importNode(template,true);for(const script of root.querySelectorAll("script"))script.remove();document.body.append(root);return root;}
async function setup({status=500,progressive=true}={}){
  await mode(status);const root=insert(await source(path)),outlet=root.querySelector("#ruvane-outlet"),feedback=root.querySelector("#ruvane-status"),runtime=await createRuntime(moduleURL,{applicationRevision:revision});
  let reads=0,posts=0,transport=()=>{throw new Error("Unexpected startup fetch");};
  window.fetch=(url,options={})=>{if(options.method==="POST")posts++;else reads++;return transport(url,options);};
  const session=createDocumentHydration(runtime,root),initial=await session.ready,navigator=createNavigator(runtime,{scope:root,outlet,status:feedback,boundaries:readBoundaries(root),progressive});
  equal(await navigator.start(initial),"success","short error adopted");session.commit();
  return{root,outlet,runtime,navigator,initial,reads:()=>reads,posts:()=>posts,enable(){transport=nativeFetch;},close(){window.fetch=nativeFetch;session.retire();navigator.dispose();runtime.dispose();root.remove();}};
}
const run=document.createElement("button");run.textContent="Run layout recovery checks";document.body.prepend(run);
run.addEventListener("click",async()=>{
  run.disabled=true;const child=window.open("about:blank","ruvane-layout-recovery");if(!child){summary.textContent="The browser blocked the test window.";return;}
  try{
    await mode(500);const before=await stats();await control("/fixtures/prepare");child.location.href=path+"?document-gate=yes";
    let parent,input,notes,nodes;
    await test("ancestor fields and counters work before the nested layout fails",async()=>{
      await until(()=>{const host=layout(child.document,"editableRoot");return host&&button(own(host),"Increment draft layout")?.disabled===false&&child.document.querySelector("#ruvane-status")?.textContent==="Layouts ready. Loading page…";});
      parent=layout(child.document,"editableRoot");const view=own(parent);input=view.querySelector("input");notes=view.querySelector("textarea");
      button(view,"Increment draft layout").click();type(input,"Ancestor 🦀 draft");type(notes,"Notes before layout failure");input.focus();input.setSelectionRange(2,8);nodes=references(view);
      equal(view.querySelector("output").textContent,"1","local counter");assert(!layout(child.document,"fragile"),"failed layout has no DOM");
    });
    await control("/fixtures/release");
    await test("a shorter error keeps the ancestor DOM, draft, focus and selection",async()=>{
      await until(()=>child.document.querySelector("#ruvane-status")?.textContent==="Page 500.");
      assert(layout(child.document,"editableRoot")===parent,"same ancestor");sameNodes(nodes,own(parent));
      equal([input.value,notes.value,input.selectionStart,input.selectionEnd],["Ancestor 🦀 draft","Notes before layout failure",2,8],"fields and selection");assert(child.document.activeElement===input,"focus retained");
      equal((await stats())[1]-before[1],0,"leaf loader not called");assert(!layout(child.document,"fragile"),"failed layout inactive");
    });
    await test("Try again creates the missing layout and page without resetting the ancestor",async()=>{
      await mode(0);button(child.document,"Try again").click();await until(()=>child.document.querySelector("#ruvane-status")?.textContent==="Ready.");
      assert(layout(child.document,"editableRoot")===parent&&layout(child.document,"fragile"),"missing layout rebuilt");sameNodes(nodes,own(parent));
      equal([input.value,notes.value,own(parent).querySelector("output").textContent],["Ancestor 🦀 draft","Notes before layout failure","1"],"live state retained");equal((await stats())[1]-before[1],1,"leaf loaded once");
    });
  }finally{await control("/fixtures/release");child.close();}
  for(const status of [400,404,500,600,700])await test(`short layout error ${status} validates and activates with no data fetch`,async()=>{
    const ctx=await setup({status});try{equal(ctx.initial.parents.length,1,"one surviving parent");equal(ctx.navigator.failure()?.kind,"server","retry available");equal([ctx.reads(),ctx.posts(),ctx.runtime.stats().views],[0,0,2],"one ancestor and error only");assert(!layout(ctx.root,"fragile"),"no failed layout");}finally{ctx.close();}
  });
  for(const progressive of [false,true])await test(`${progressive?"progressive":"buffered"} retry preserves focused ancestor fields`,async()=>{
    const ctx=await setup({progressive});try{const parent=layout(ctx.root,"editableRoot"),input=own(parent).querySelector("input"),nodes=references(own(parent));type(input,"Retry draft 🦀");input.focus();input.setSelectionRange(1,5);const length=history.length;await mode(0);ctx.enable();equal(await ctx.navigator.retry(),"success","retry succeeded");assert(layout(ctx.root,"editableRoot")===parent&&layout(ctx.root,"fragile"),"branch completed");sameNodes(nodes,own(parent));assert(document.activeElement===input,"focus retained");equal([input.value,input.selectionStart,input.selectionEnd,ctx.reads(),ctx.posts(),history.length],["Retry draft 🦀",1,5,1,0,length],"one GET and retained selection");equal(ctx.runtime.stats().views,3,"three owners");}finally{ctx.close();}
  });
  await test("repeated layout errors keep the ancestor and stay uncached",async()=>{
    const ctx=await setup();try{const parent=layout(ctx.root,"editableRoot"),input=own(parent).querySelector("input");type(input,"Repeated draft");ctx.enable();for(let n=0;n<4;n++){equal(await ctx.navigator.retry(),"success","error retry");assert(layout(ctx.root,"editableRoot")===parent&&!layout(ctx.root,"fragile"),"only ancestor survives");equal(ctx.navigator.stats().cacheEntries,0,"error not cached");equal(ctx.runtime.stats().views,2,"no leaked owners");}equal([input.value,ctx.reads(),ctx.posts()],["Repeated draft",4,0],"no hidden retry");}finally{ctx.close();}
  });
  for(const corruption of ["success","index","depth","seed"])await test(`inconsistent ${corruption} cannot adopt a shorter branch`,async()=>{
    await mode(500);const root=insert(await source(path)),record=root.querySelector("#ruvane-initial"),hex=record.textContent;let bytes=Uint8Array.from(hex.match(/../g),x=>parseInt(x,16));
    if(corruption==="success"){
      const prefix=Uint8Array.from(root.querySelector("[data-ruvane-segment]").textContent.match(/../g),x=>parseInt(x,16)),packet=prefix.slice(16);
      packet[3]=50;bytes=new Uint8Array([...prefix.slice(0,16),...packet.slice(0,5),...packet.slice(6)]);bytes[2]=73;new DataView(bytes.buffer).setUint32(12,bytes.length-16,true);
    }
    if(corruption==="index")bytes[21]=0;
    if(corruption==="depth")root.setAttribute("data-ruvane-layouts","3");
    if(corruption==="seed")bytes[bytes.length-1]^=1;
    record.textContent=[...bytes].map(value=>value.toString(16).padStart(2,"0")).join("");
    const runtime=await createRuntime(moduleURL,{applicationRevision:revision}),session=createDocumentHydration(runtime,root);let rejected=false;
    try{await session.ready;}catch{rejected=true;}finally{session.retire();equal(runtime.stats().views,0,"owners released");runtime.dispose();root.remove();}assert(rejected,"invalid shorter branch refused");
  });
  await test("short error startup and disposal release every owner over one hundred cycles",async()=>{
    await mode(500);const template=await source(path),runtime=await createRuntime(moduleURL,{applicationRevision:revision}),baseline=runtime.stats();window.fetch=()=>{throw new Error("Unexpected data request");};
    try{for(let n=0;n<100;n++){const root=insert(template),session=createDocumentHydration(runtime,root),initial=await session.ready,navigator=createNavigator(runtime,{scope:root,outlet:root.querySelector("#ruvane-outlet"),status:root.querySelector("#ruvane-status"),boundaries:readBoundaries(root)});initial.parents[0].dispatch(1);equal(await navigator.start(initial),"success","error activation");session.commit();navigator.dispose();root.remove();equal(runtime.stats(),baseline,"all handles released");}}finally{window.fetch=nativeFetch;runtime.dispose();}
  });
  window.fetch=nativeFetch;await mode(500);const result={passed:results.filter(row=>row.passed).length,failed:results.filter(row=>!row.passed).length,results};report.textContent=JSON.stringify(result,null,2);summary.textContent=`${result.passed}/${results.length} layout recovery checks passed`;
});

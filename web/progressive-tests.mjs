const report=document.getElementById("progressive-report"),summary=document.getElementById("progressive-summary"),results=[];
const assert=(value,message)=>{if(!value)throw new Error(message);};
const equal=(a,b,message)=>assert(JSON.stringify(a)===JSON.stringify(b),`${message}: ${JSON.stringify(a)}`);
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(check){const end=performance.now()+5000;while(!check()){if(performance.now()>end)throw new Error("Progressive check timed out");await wait(5);}}
async function test(name,fn){try{await fn();results.push({name,passed:true});}catch(error){results.push({name,passed:false,error:String(error.message)});}report.textContent=JSON.stringify(results,null,2);}
const originalPush=history.pushState;let pushes=0;history.pushState=function(...args){pushes++;return originalPush.apply(this,args);};
const nativeFetch=window.fetch,source=new DOMParser().parseFromString(await(await nativeFetch("/tasks")).text(),"text/html"),configuration=source.getElementById("ruvane-application"),moduleURL=configuration.getAttribute("data-ruvane-module"),revision=configuration.getAttribute("data-ruvane-revision");
const {createRuntime,NAVIGATION_STREAM_MEDIA_TYPE}=await import(new URL("./bridge.mjs",new URL(moduleURL,location.href)));
const {createNavigator,readBoundaries}=await import(new URL("./navigation.mjs",new URL(moduleURL,location.href)));
let transport=(url,options)=>nativeFetch(url,options);
window.fetch=(url,options)=>transport(url,options?.headers?.Accept==="application/vnd.ruvane.page"?{...options,credentials:"omit"}:options);
async function setup(options={}){
  history.replaceState(null,"","/");const scope=document.createElement("main"),shell=document.createElement("div"),outlet=document.createElement("div"),status=document.createElement("p");scope.append(shell,status,outlet);document.body.append(scope);
  const runtime=await createRuntime(moduleURL,{applicationRevision:revision}),counter=runtime.mount(shell,{component:1,state:{kind:"number",value:5}}),navigator=createNavigator(runtime,{scope,outlet,status,boundaries:readBoundaries(configuration),progressive:true,...options});
  equal(await navigator.start(),"success","initial page");
  return{scope,shell,outlet,status,runtime,counter,navigator,close(){navigator.dispose();runtime.dispose();scope.remove();transport=(url,options)=>nativeFetch(url,options);}};
}
const workspace=ctx=>ctx.outlet.querySelector('[data-ruvane-layout="workspace"]');
const own=layout=>layout.querySelector(":scope > [data-ruvane-layout-view]");
function gate(url,{truncate=false,transform=bytes=>bytes}={}){
  let release,cancelled=false;
  transport=async(target,options)=>{
    const response=await nativeFetch(target,options);
    if(!String(target).endsWith(url)||options.headers.Accept!==NAVIGATION_STREAM_MEDIA_TYPE)return response;
    const bytes=transform(new Uint8Array(await response.arrayBuffer())),view=new DataView(bytes.buffer);let cut=20;for(let n=0;n<2;n++)cut+=4+view.getUint32(cut,true);
    return new Response(new ReadableStream({start(controller){controller.enqueue(bytes.slice(0,cut));release=()=>{if(cancelled)return;controller.enqueue(bytes.slice(cut,truncate?-4:undefined));controller.close();};},cancel(){cancelled=true;}}),{headers:response.headers,status:response.status});
  };
  return{release(){assert(release,"gate ready");release();},ready:()=>typeof release==="function",cancelled:()=>cancelled};
}

await test("a real delayed GET exposes an interactive layout before its page completes",async()=>{
  const ctx=await setup();try{const pending=ctx.navigator.navigate("/tasks/project?delay=yes&fixture=real");await until(()=>workspace(ctx)&&!ctx.outlet.textContent.includes("Project overview"));const parent=workspace(ctx);assert(ctx.navigator.stats().pending,"page still pending");own(parent).querySelector("button").click();equal(own(parent).querySelector("output").textContent,"1","counter works while loading");equal(await pending,"success","completed");assert(workspace(ctx)===parent&&ctx.outlet.textContent.includes("Project overview"),"same layout and final page");equal(own(parent).querySelector("output").textContent,"1","state retained");}finally{ctx.close();}
});
await test("history and cache wait for the final packet, terminal marker and EOF",async()=>{
  const ctx=await setup();try{const url="/tasks/project?fixture=end",length=history.length,pushed=pushes,oldURL=location.pathname,cached=ctx.navigator.stats().cacheEntries,g=gate(url),pending=ctx.navigator.navigate(url);await until(()=>workspace(ctx));assert(location.pathname===oldURL&&history.length===length&&pushes===pushed,"no early history commit");equal(ctx.navigator.stats().cacheEntries,cached,"no partial storage");const parent=workspace(ctx);own(parent).querySelector("button").click();g.release();equal(await pending,"success","completion");equal(pushes,pushed+1,"one push");assert(workspace(ctx)===parent,"preview adopted");equal(own(parent).querySelector("output").textContent,"1","preview state adopted");}finally{ctx.close();}
});
await test("truncation restores the previous owner and an explicit fresh GET can recover",async()=>{
  const ctx=await setup();try{const before=ctx.navigator.current(),node=ctx.outlet.firstChild,url="/tasks/project?fixture=truncated",length=history.length,pushed=pushes,g=gate(url,{truncate:true}),pending=ctx.navigator.navigate(url);await until(()=>workspace(ctx));g.release();equal(await pending,"failed","incomplete response");assert(ctx.navigator.current()===before&&ctx.outlet.firstChild===node,"old owner and DOM restored");equal(history.length,length,"no failed history push");assert(ctx.navigator.failure()?.kind==="network","local failure");transport=(url,options)=>nativeFetch(url,options);equal(await ctx.navigator.retry(),"success","fresh retry");equal(pushes,pushed+1,"one successful push");}finally{ctx.close();}
});
await test("a newer navigation retires a partial branch and ignores late bytes",async()=>{
  const ctx=await setup();try{const url="/tasks/project?fixture=superseded",g=gate(url),pending=ctx.navigator.navigate(url);await until(()=>workspace(ctx));const old=workspace(ctx);equal(await ctx.navigator.navigate("/tasks/settings"),"success","newer page");equal(await pending,"superseded","old request retired");g.release();await wait(0);assert(workspace(ctx)!==old&&ctx.outlet.textContent.includes("settings"),"no resurrection");assert(g.cancelled(),"reader cancelled");}finally{ctx.close();}
});
await test("invalidation restores the page and releases all speculative layout owners",async()=>{
  const ctx=await setup();try{const before=ctx.runtime.stats(),owner=ctx.navigator.current(),url="/tasks/project?fixture=invalidate",g=gate(url),pending=ctx.navigator.navigate(url);await until(()=>workspace(ctx));ctx.navigator.invalidate();equal(await pending,"invalidated","retired read");g.release();equal(ctx.runtime.stats(),before,"resource baseline");assert(ctx.navigator.current()===owner&&!ctx.navigator.failure(),"old page retained");}finally{ctx.close();}
});
await test("disposal aborts a partial stream while the independent shell remains usable",async()=>{
  const ctx=await setup();try{const url="/tasks/project?fixture=dispose",g=gate(url),pending=ctx.navigator.navigate(url);await until(()=>workspace(ctx));ctx.navigator.dispose();equal(await pending,"disposed","disposed read");g.release();equal(ctx.runtime.stats().views,1,"only shell remains");ctx.counter.dispatch(1);assert(ctx.shell.textContent.includes("6"),"shell still usable");}finally{ctx.close();}
});
await test("a read timeout restores the old page and leaves a bounded retry presentation",async()=>{
  const ctx=await setup({timeoutMs:300});try{const owner=ctx.navigator.current(),url="/tasks/project?fixture=timeout",g=gate(url),pending=ctx.navigator.navigate(url);await until(()=>workspace(ctx));equal(await pending,"failed","timeout");assert(ctx.navigator.current()===owner&&ctx.navigator.failure()?.kind==="timeout","old owner and local timeout");equal(ctx.runtime.stats().views,3,"shell, old page and feedback");g.release();}finally{ctx.close();}
});
await test("forged revisions and unowned layouts are rejected without harming the runtime",async()=>{
  for(const field of ["revision","layout"]){const ctx=await setup();try{const url=`/tasks/project?fixture=${field}`,owner=ctx.navigator.current();gate(url,{transform(bytes){if(field==="revision")bytes[4]^=1;else{const needle=new TextEncoder().encode("workspace");const start=bytes.findIndex((_,n)=>needle.every((b,k)=>bytes[n+k]===b));assert(start>=0,"layout descriptor");bytes[start+needle.length+4+6]=1;}return bytes;}});equal(await ctx.navigator.navigate(url),"failed","forged stream");assert(ctx.navigator.current()===owner&&ctx.runtime.stats().state==="active","healthy old page");}finally{ctx.close();}}
});
await test("typed server errors adopt validated parents and retry without duplicate history",async()=>{
  const ctx=await setup();try{const url="/tasks/project?unavailable=yes&fixture=error",g=gate(url),pending=ctx.navigator.navigate(url);await until(()=>workspace(ctx));const parent=workspace(ctx);own(parent).querySelector("button").click();g.release();equal(await pending,"success","typed error completed");assert(ctx.outlet.textContent.includes("Page unavailable")&&workspace(ctx)===parent,"local error under same parent");const length=history.length;transport=(url,options)=>nativeFetch(url,options);equal(await ctx.navigator.retry(),"success","fresh typed error");equal(history.length,length,"replace on retry");equal(own(parent).querySelector("output").textContent,"1","parent state retained");}finally{ctx.close();}
});
await test("streamed responses stay no-store and complete buffered prefetches remain reusable",async()=>{
  const ctx=await setup();try{const url="/tasks/project?fixture=cache",cached=ctx.navigator.stats().cacheEntries;equal(await ctx.navigator.navigate(url),"success","streamed page");equal(ctx.navigator.stats().cacheEntries,cached,"no streamed storage");equal(await ctx.navigator.prefetch(url),"prefetched","complete public packet");equal(ctx.navigator.stats().cacheEntries,cached+1,"one complete cache entry");equal(await ctx.navigator.navigate(url),"success","cached packet");equal(ctx.navigator.stats().hits,1,"one cache hit");}finally{ctx.close();}
});
await test("a rejected final seed restores the previous page after a valid layout prefix",async()=>{
  const ctx=await setup();try{const url="/tasks/project?fixture=schema",owner=ctx.navigator.current(),g=gate(url,{transform(bytes){const view=new DataView(bytes.buffer);let pos=20;for(let n=0;n<2;n++)pos+=4+view.getUint32(pos,true);pos+=4+5+2;for(let n=0;n<2;n++)pos+=4+view.getUint32(pos,true);pos+=12+4;bytes[pos+13]=99;return bytes;}}),pending=ctx.navigator.navigate(url);await until(()=>workspace(ctx));g.release();equal(await pending,"failed","schema rejected");assert(ctx.navigator.current()===owner&&ctx.runtime.stats().state==="active","old page and healthy runtime");}finally{ctx.close();}
});
await test("back navigation supersedes a pending stream without adding history",async()=>{
  const ctx=await setup();try{await ctx.navigator.navigate("/tasks/settings");const length=history.length,url="/tasks/project?fixture=pop",g=gate(url),pending=ctx.navigator.navigate(url);await until(()=>g.ready());history.back();await until(()=>location.pathname==="/"&&!ctx.navigator.stats().pending);equal(await pending,"superseded","stream superseded by pop");equal(history.length,length,"no new entry");assert(ctx.outlet.textContent.includes("Home")&&!workspace(ctx),"home restored");g.release();}finally{ctx.close();}
});
await test("mutations retire previews safely while the synchronous ABI is busy",async()=>{
  const ctx=await setup();try{const actor=document.createElement("div");ctx.scope.append(actor);const tasks=ctx.runtime.mount(actor,{component:9,state:{kind:"number",value:0}});await tasks.whenIdle();const url="/tasks/project?fixture=mutation",g=gate(url),pending=ctx.navigator.navigate(url);await until(()=>workspace(ctx));tasks.dispatch(5,"Progressive mutation fixture");tasks.dispatch(1);equal(await pending,"invalidated","read invalidated");equal(await tasks.whenIdle(),"success","mutation completed");await wait(0);assert(!workspace(ctx)&&ctx.runtime.stats().state==="active","old page restored and runtime healthy");equal(ctx.runtime.stats().views,3,"shell, old page and independent tasks");g.release();tasks.dispatch(101);await tasks.whenIdle();tasks.dispose();}finally{ctx.close();}
});
await test("refresh recreates layouts progressively without pushing another history entry",async()=>{
  const ctx=await setup();try{await ctx.navigator.navigate("/tasks/project?delay=yes&fixture=refresh");const previous=workspace(ctx),length=history.length,pushed=pushes;own(previous).querySelector("button").click();const pending=ctx.navigator.refresh();await until(()=>workspace(ctx)&&workspace(ctx)!==previous);const parent=workspace(ctx);equal(own(parent).querySelector("output").textContent,"0","fresh layout state");own(parent).querySelector("button").click();equal(await pending,"success","refreshed");assert(workspace(ctx)===parent,"temporary layout adopted");equal(own(parent).querySelector("output").textContent,"1","state during refresh retained");assert(history.length===length&&pushes===pushed,"no refresh push");}finally{ctx.close();}
});
await test("exhausting the view budget after a layout prefix restores the previous page",async()=>{
  const ctx=await setup();try{for(let n=0;n<13;n++){const host=document.createElement("div");ctx.scope.append(host);ctx.runtime.mount(host,{component:1,state:{kind:"number",value:0}});}equal(ctx.runtime.stats().views,15,"one slot remains");const owner=ctx.navigator.current(),url="/tasks/project?fixture=budget",g=gate(url),pending=ctx.navigator.navigate(url);await until(()=>workspace(ctx));equal(ctx.runtime.stats().views,16,"preview occupies the last slot");g.release();equal(await pending,"failed","leaf preparation rejected");assert(ctx.navigator.current()===owner&&!workspace(ctx)&&ctx.runtime.stats().state==="active","previous owner restored");equal(ctx.runtime.stats().views,16,"old owners plus bounded error feedback");}finally{ctx.close();}
});
await test("one hundred progressive success and error transitions keep resources bounded",async()=>{
  const ctx=await setup();try{await ctx.navigator.navigate("/tasks/project?fixture=cycles");const parent=workspace(ctx),before=ctx.runtime.stats();for(let n=0;n<100;n++){equal(await ctx.navigator.navigate("/tasks/project?unavailable=yes&fixture=cycles"),"success","error");equal(await ctx.navigator.navigate("/tasks/project?fixture=cycles"),"success","success");}equal(ctx.runtime.stats(),before,"stable owners, listeners and handles");assert(workspace(ctx)===parent,"same parent");}finally{ctx.close();}
});
window.fetch=nativeFetch;history.pushState=originalPush;
const result={passed:results.filter(row=>row.passed).length,failed:results.filter(row=>!row.passed).length,results};report.textContent=JSON.stringify(result,null,2);summary.textContent=`${result.passed}/${results.length} progressive checks passed`;

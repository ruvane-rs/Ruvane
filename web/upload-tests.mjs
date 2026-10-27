import {createRuntime} from "/assets/bridge.mjs";
const originalFetch=globalThis.fetch.bind(globalThis),OriginalRequest=globalThis.Request,rows=[],list=document.getElementById("results"),report=document.getElementById("report"),status=document.getElementById("status");
report.hidden=true;
const assert=(value,message)=>{if(!value)throw Error(message);};
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(predicate){for(let i=0;i<300;i++){if(predicate())return;await pause(5);}throw Error("Expected upload state did not arrive");}
async function test(name,body){const item=document.createElement("li");list.append(item);try{await body();rows.push({name,passed:true});item.textContent="PASS — "+name;}catch(error){rows.push({name,passed:false,error:String(error)});item.textContent="FAIL — "+name+": "+error;}finally{globalThis.fetch=originalFetch;globalThis.Request=OriginalRequest;}report.textContent=JSON.stringify(rows,null,2);}
const options={csrfEndpoint:"/csrf-token",actionTimeoutMs:1000};
const isUpload=input=>new URL(typeof input==="string"?input:input.url,location.href).pathname==="/upload";
const response=(body,status=200,headers={})=>new Response(body,{status,headers:{"Content-Type":"application/json","Cache-Control":"no-store",...headers}});
const state=async()=>{const result=await originalFetch("/uploads/state",{cache:"no-store"});assert(result.status===200,"authorized file list");return(await result.json()).files;};
function select(host,bytes=new Uint8Array([0,255,13,10,128]),name="binary 🦀.bin",type="image/svg+xml;charset=utf-8"){
 const input=host.querySelector('input[type="file"]'),transfer=new DataTransfer(),file=new File([bytes],name,{type});transfer.items.add(file);input.files=transfer.files;input.dispatchEvent(new Event("change",{bubbles:true}));return file;
}
async function setup({timeout=1000,hydrate=false}={}){
 const runtime=await createRuntime("/assets/app.wasm",{...options,actionTimeoutMs:timeout}),host=document.createElement("section");document.body.append(host);let controller,old;
 if(hydrate){const page=await originalFetch("/interactive",{cache:"no-store"});const parsed=new DOMParser().parseFromString(await page.text(),"text/html"),root=parsed.querySelector("#upload-root > [data-ruvane-version]");host.append(root);old=root.querySelector('input[type="file"]');assert(old.disabled,"SSR file picker starts inert");controller=runtime.hydrate(root);assert(root.querySelector('input[type="file"]')===old&&!old.disabled,"existing picker activated");}
 else controller=runtime.mount(host,{component:29,state:{kind:"number",value:0}});
 return{runtime,host,controller,close(){runtime.dispose();assert(runtime.stats().views===0&&runtime.stats().pendingActions===0&&runtime.fileState().selected===0,"owners and file records released");host.remove();}};
}
async function withView(body,config){const context=await setup(config);try{await body(context);}finally{globalThis.fetch=originalFetch;context.close();}}
await test("hydration keeps the server file control and local validation retains its selection",async()=>{
 await withView(async({runtime,host,controller})=>{
  controller.dispatch(4);assert(await controller.whenIdle()==="success","session opened");const input=host.querySelector('input[type="file"]'),file=select(host);assert(runtime.fileState().selected===1&&runtime.fileState().bytes===5,"bounded live file record");
  const before=(await state()).length;controller.dispatch(1);assert(host.textContent.includes("Selection retained")&&host.querySelector('input[type="file"]')===input&&input.files[0]===file,"caption validation preserves exact picker and File");assert((await state()).length===before,"local validation sends no upload");
  controller.dispatch(2,"Rust + %41 <b>plain text</b>");let calls=0,framing=false;
  globalThis.fetch=async(input,request)=>{if(isUpload(input)){calls++;assert(request.method==="POST"&&request.mode==="same-origin"&&request.credentials==="same-origin"&&request.redirect==="error"&&request.cache==="no-store","fixed upload profile");assert(typeof request.headers["Ruvane-Csrf-Token"]==="string","header-only CSRF");const decoded=await new Response(request.body,{headers:{"Content-Type":request.headers["Content-Type"]}}).formData();const sent=decoded.get("attachment");assert(sent.type==="application/octet-stream"&&sent.size===5,"opaque canonical file part");assert(decoded.get("caption")==="Rust + %41 <b>plain text</b>"&&!decoded.has("_ruvane_csrf"),"separate business fields");assert([...new Uint8Array(await sent.arrayBuffer())].join(",")==="0,255,13,10,128","exact binary bytes");framing=true;}return originalFetch(input,request);};
  controller.dispatch(1);assert(await controller.whenIdle()==="success"&&calls===1&&framing,"one successful explicit upload");assert(runtime.fileState().selected===0&&input.value===""&&host.querySelector('input[type="text"]').value==="","success resets explicitly");assert(!host.querySelector("b"),"caption stays plain text");
 },{hydrate:true});
});
await test("server validation keeps the same file for an explicit correction without reselection",async()=>{
 await withView(async({runtime,host,controller})=>{
  controller.dispatch(2,"Server validation draft");const file=select(host,new Uint8Array([7]),'quoted "name.bin');const before=(await state()).length;let calls=0;
  globalThis.fetch=async(input,request)=>{if(isUpload(input)){calls++;return response('{"error":"Bad Request","errors":[{"field":"caption","code":"invalid_value"}],"truncated":false}',400);}return originalFetch(input,request);};
  controller.dispatch(1);assert(await controller.whenIdle()==="validation"&&calls===1&&!runtime.actionState().uncertain,"typed validation rejection");assert(host.querySelector('input[type="file"]').files[0]===file&&runtime.fileState().selected===1&&host.textContent.includes("This value is invalid."),"file and draft retained");assert((await state()).length===before,"no server mutation from fixture");
  globalThis.fetch=originalFetch;controller.dispatch(2,"Explicit correction");controller.dispatch(1);assert(await controller.whenIdle()==="success"&&runtime.fileState().selected===0,"correction sends retained File once");assert((await state()).length===before+1,"one created record");
 });
});
await test("oversized and replaced selections retire references before any request",async()=>{
 await withView(async({runtime,host,controller})=>{
  let calls=0;globalThis.fetch=async(input,request)=>{if(isUpload(input))calls++;return originalFetch(input,request);};
  select(host,new Uint8Array(32769),"too-large.bin");assert(runtime.fileState().selected===0&&host.querySelector('input[type="file"]').value===""&&host.textContent.includes("32 KiB"),"oversized selection discarded locally");
  select(host,new Uint8Array([1]),"old.bin");select(host,new Uint8Array([2,3]),"new.bin");assert(runtime.fileState().selected===1&&runtime.fileState().bytes===2&&host.textContent.includes("Selected: new.bin"),"only replacement retained");controller.dispatch(7);assert(runtime.fileState().selected===0&&host.querySelector('input[type="file"]').value==="","explicit reset retires handle");assert(calls===0,"selection and reset send nothing");
 });
});
await test("selected empty files remain distinct from an empty picker",async()=>{
 await withView(async({runtime,host,controller})=>{controller.dispatch(2,"Empty file");select(host,new Uint8Array(),"empty.bin");assert(runtime.fileState().selected===1&&runtime.fileState().bytes===0,"empty file reference exists");controller.dispatch(1);assert(await controller.whenIdle()==="success","empty upload accepted");assert((await state()).some(item=>item.filename==="empty.bin"&&item.bytes===0),"empty record stored");});
});
await test("one pending upload disables controls and duplicate submits send no extra request",async()=>{
 await withView(async({runtime,host,controller})=>{
  controller.dispatch(2,"Pending upload");select(host);let release,calls=0;
  globalThis.fetch=async(input,request)=>{if(!isUpload(input))return originalFetch(input,request);calls++;return new Promise(resolve=>{release=resolve;});};
  controller.dispatch(1);await until(()=>calls===1);assert(host.querySelector('input[type="file"]').disabled&&host.querySelector('input[type="text"]').disabled,"pending controls disabled");controller.dispatch(1);assert(calls===1&&runtime.stats().pendingActions===1,"one combined active request");release(response('{}',400));assert(await controller.whenIdle()==="validation","pending request settled");
 });
});
await test("confirmed quota and size refusals stay distinct and never replay an upload",async()=>{
 for(const [status,headers,expected,uncertain]of [[429,{"Retry-After":"3","Ruvane-Action-Rejected":"rate-limit"},"rate-limited",false],[429,{"Retry-After":"3"},"rate-limited",true],[413,{},"rejected",false],[503,{},"rejected",true]])await withView(async({runtime,host,controller})=>{
  controller.dispatch(2,"Refusal draft");const file=select(host);let calls=0;globalThis.fetch=async(input,request)=>{if(!isUpload(input))return originalFetch(input,request);calls++;return response("Refused",status,{...headers,"Content-Type":"text/plain"});};controller.dispatch(1);assert(await controller.whenIdle()===expected,"HTTP refusal category");assert(calls===1&&runtime.actionState().uncertain===uncertain&&host.querySelector('input[type="file"]').files[0]===file,"certainty and selection retained without retry");if(status===429)assert(controller.retryAfter()===3,"retry hint only");
 });
});
await test("preflight failures send no file and preserve the selection",async()=>{
 await withView(async({runtime,host,controller})=>{
  controller.dispatch(2,"Preflight draft");const file=select(host);let calls=0;globalThis.fetch=async(input,request)=>{if(String(input)==="/csrf-token")return response("denied",403,{"Content-Type":"text/plain"});if(isUpload(input))calls++;return originalFetch(input,request);};controller.dispatch(1);assert(await controller.whenIdle()==="rejected"&&calls===0&&!runtime.actionState().uncertain,"no file submission");assert(host.querySelector('input[type="file"]').files[0]===file,"selection retained");
 });
});
await test("deadline aborts a submitted upload and ignores a late response without retry",async()=>{
 await withView(async({runtime,host,controller})=>{
  controller.dispatch(2,"Deadline draft");select(host);let calls=0,signal,release;globalThis.fetch=async(input,request)=>{if(!isUpload(input))return originalFetch(input,request);calls++;signal=request.signal;return new Promise(resolve=>{release=resolve;});};controller.dispatch(1);assert(await controller.whenIdle()==="timeout"&&calls===1&&signal.aborted&&runtime.actionState().uncertain,"uncertain submitted timeout");const text=host.textContent;release(response('{}'));await pause(30);assert(host.textContent===text&&runtime.stats().pendingActions===0,"late response ignored");
 },{timeout:100});
});
await test("destruction aborts transport and cannot update a replacement owner",async()=>{
 await withView(async({runtime,host,controller})=>{
  controller.dispatch(2,"Departing upload");select(host);let release,signal,calls=0;globalThis.fetch=async(input,request)=>{if(!isUpload(input))return originalFetch(input,request);calls++;signal=request.signal;return new Promise(resolve=>{release=resolve;});};controller.dispatch(1);await until(()=>calls===1);const idle=controller.whenIdle();controller.dispose();assert(await idle==="disposed"&&signal.aborted&&runtime.fileState().selected===0,"disposed owner releases file record and waiters");
  const fresh=runtime.mount(host,{component:29,state:{kind:"number",value:0}});release(response('{}'));await pause(30);assert(host.textContent.includes("No file selected.")&&!host.textContent.includes("Departing upload")&&runtime.stats().views===1,"replacement unaffected");fresh.dispose();
 });
});
await test("unmount before delayed CSRF completion releases the file and submits no upload",async()=>{
 await withView(async({runtime,host,controller})=>{
  controller.dispatch(2,"Preflight disposal");select(host);let release,calls=0;globalThis.fetch=async(input,request)=>{if(String(input)==="/csrf-token")return new Promise(resolve=>{release=resolve;});if(isUpload(input))calls++;return originalFetch(input,request);};controller.dispatch(1);await until(()=>Boolean(release));controller.dispose();release(new Response(null,{status:200,headers:{"Content-Type":"application/vnd.ruvane.csrf","Cache-Control":"no-store","Content-Length":"0"}}));await pause(30);assert(calls===0&&runtime.fileState().selected===0&&!runtime.actionState().uncertain,"late token cannot submit a retired file");
 });
});
await test("malformed JSON replies stay local and mark submitted uploads uncertain",async()=>{
 await withView(async({runtime,host,controller})=>{controller.dispatch(2,"Malformed reply");select(host);let calls=0;globalThis.fetch=async(input,request)=>{if(!isUpload(input))return originalFetch(input,request);calls++;return response('{"x":1,"x":2}');};controller.dispatch(1);assert(await controller.whenIdle()==="protocol"&&calls===1&&runtime.actionState().uncertain,"malformed reply has no retry");});
});
await test("repeated select-reset-dispose cycles leave no live file records",async()=>{
 await withView(async({runtime,host,controller})=>{for(let i=0;i<100;i++){select(host,new Uint8Array([i]),`file-${i}.bin`);assert(runtime.fileState().selected===1,"single live selection");controller.dispatch(7);assert(runtime.fileState().selected===0,"reset released selection");}assert(runtime.stats().views===1,"no extra owners");});
});
await test("a changed picker during multipart preparation cannot submit the old file",async()=>{
 await withView(async({runtime,host,controller})=>{
  controller.dispatch(2,"Serialization selection");select(host);let release,calls=0;
  globalThis.Request=class extends OriginalRequest {get body(){const reader=super.body.getReader(),gate=new Promise(resolve=>{release=resolve;});return new ReadableStream({async pull(stream){await gate;const chunk=await reader.read();if(chunk.done){reader.releaseLock();stream.close();}else stream.enqueue(chunk.value);},cancel(){return reader.cancel().finally(()=>reader.releaseLock());}});}};
  globalThis.fetch=async(input,request)=>{if(isUpload(input))calls++;return originalFetch(input,request);};controller.dispatch(1);await until(()=>Boolean(release));
  const transfer=new DataTransfer();transfer.items.add(new File(["replacement"],"new.bin"));host.querySelector('input[type="file"]').files=transfer.files;release();
  assert(await controller.whenIdle()==="rejected"&&calls===0&&!runtime.actionState().uncertain,"retired selection rejected before POST");
 });
});
status.textContent=`${rows.filter(row=>row.passed).length}/${rows.length} upload checks passed`;report.textContent=JSON.stringify({rows},null,2);document.documentElement.dataset.checksComplete="true";

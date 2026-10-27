import test from "node:test";
import assert from "node:assert/strict";
import {spawn} from "node:child_process";
import {mkdtemp,readFile,rename,rm} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join,resolve} from "node:path";
import {fileURLToPath} from "node:url";
import {uploadsFixture} from "./uploads-fixture.mjs";
import {readinessURL} from "./readiness.mjs";
import {decodeCommands,validateExports,planCommands} from "../../web/bridge.mjs";
const root=fileURLToPath(new URL("../../",import.meta.url)),cli=resolve(process.env.RUVANE_CLI||join(root,"target/debug/ruvane"));
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms)),word=n=>[n&255,(n>>>8)&255,(n>>>16)&255,(n>>>24)&255];
const text=value=>{const bytes=new TextEncoder().encode(value);return [...word(bytes.length),...bytes];};
const upload=(field="attachment",body="caption=Rust%20%2B%20%2541",url="/upload")=>new Uint8Array([82,86,78,56,...word(1),16,...word(1),...word(1),...word(1),...word(2),...word(3),...text(url),...text(field),...text(body)]);
test("upload commands reject truncated frames and ambiguous fields",()=>{
 const frame=upload(),command=decodeCommands(frame)[0];assert.deepEqual(command.fields,[["caption","Rust + %41"]]);assert.equal(command.method,"POST");assert.equal(command.mutation,true);
 for(let n=0;n<frame.length;n++)assert.throws(()=>decodeCommands(frame.slice(0,n)));
 for(const body of ["attachment=shadow","_ruvane_csrf=claim","a=x&a=y","a=%00","a=%FF","bad",Array.from({length:17},(_,i)=>`f${i}=x`).join("&")])assert.throws(()=>decodeCommands(upload("attachment",body)));
 for(const field of ["_ruvane_csrf","bad name","", "x".repeat(65)])assert.throws(()=>decodeCommands(upload(field)));
 for(const url of ["https://foreign.test/upload","//foreign.test/upload","/../upload"])assert.throws(()=>decodeCommands(upload("attachment","",url)));
});
test("file plans require one complete unseeded control and monotonic reset epochs",()=>{
 const blank={nodes:new Map([[0,{tag:0,depth:0,key:0,children:[],element:null}]]),nextNode:0};
 const create=(node,parent,tag)=>({op:1,view:1,node,parent,tag,key:node});
 const control=base=>{const node=base+2;return[create(base,0,10),create(base+1,base,4),{op:2,view:1,node:base+1,text:"File"},create(node,base,16),{op:3,view:1,node,event:node},{op:4,view:1,node,disabled:false},{op:10,view:1,node,invalid:false},{op:15,view:1,node,epoch:0}];};
 const plan=planCommands(control(1),1,blank,true);assert.equal(plan.nodes.get(3).epoch,0);
 const reset=planCommands([{op:15,view:1,node:3,epoch:1}],1,plan);assert.equal(reset.nodes.get(3).epoch,1);
 assert.throws(()=>planCommands([{op:15,view:1,node:3,epoch:0}],1,reset));
 assert.throws(()=>planCommands(control(4),1,plan));assert.throws(()=>planCommands(control(1).slice(0,-1),1,blank,true));
 assert.throws(()=>planCommands([{op:8,view:1,node:3,value:"fake.bin",epoch:0}],1,plan));
});
test("packaged Rust uploads keep browser references separate from guarded native bytes",{timeout:120000},async t=>{
 const directory=await mkdtemp(join(tmpdir(),"ruvane-uploads-"));let child,hidden,project;
 try {
  const fixture=await uploadsFixture({directory,root,cli});project=fixture.project;
  const wasm=await readFile(join(fixture.bundle,"resources/ruvane-runtime",BigInt(fixture.revision).toString(16).padStart(16,"0"),"app.wasm")),module=await WebAssembly.compile(wasm),e=validateExports(new WebAssembly.Instance(module,{}).exports);
  const drain=()=>{const commands=decodeCommands(new Uint8Array(e.memory.buffer,e.ruvane_commands_ptr()>>>0,e.ruvane_commands_len()>>>0).slice());assert.equal(e.ruvane_commands_clear(),1);return commands;};
  await t.test("ABI v14 has 22 numeric exports and no imports or upload backend",()=>{assert.deepEqual(WebAssembly.Module.imports(module),[]);assert.equal(e.ruvane_abi_version(),14);assert.equal(WebAssembly.Module.exports(module).filter(item=>item.kind==="function").length,22);assert.equal(wasm.includes(Buffer.from("SERVER_UPLOAD_BACKEND_ALPHA57")),false);});
  await t.test("numeric file metadata reaches Rust without copying file bytes",()=>{
   const id=e.ruvane_mount(1,29,1,0);assert.ok(id);const initial=drain(),node=initial.find(command=>command.tag===16).node;
   const name=new TextEncoder().encode("Rust 🦀.bin"),bytes=new Uint8Array([82,85,70,49,1,...word(1),...word(3),name.length,0,...name,0,0]);
   assert.equal(e.ruvane_event_begin(id,node,3,401),0);assert.equal(e.ruvane_event_begin(id,node,3,bytes.length),1);
   for(let n=0;n<bytes.length;n+=4){const chunk=bytes.slice(n,n+4),value=chunk.reduce((value,byte,i)=>value+byte*2**(8*i),0);assert.equal(e.ruvane_event_chunk(value,chunk.length),1);}
   assert.equal(e.ruvane_dispatch(id,node,3,3,0),1);assert.ok(drain().some(command=>command.text?.includes("Selected: Rust 🦀.bin")));
   assert.equal(e.ruvane_dispose(id),1);drain();assert.equal(e.ruvane_live_instances(),0);assert.equal(e.ruvane_event_begin(id,node,3,5),0);
  });
  hidden=join(project,"source-hidden");await rename(join(project,"src"),hidden);
  const ready=join(directory,"ready");let logs="";child=spawn(join(fixture.bundle,"ruvane-app"),["--ruvane-serve","0",ready],{cwd:fixture.bundle,stdio:["pipe","pipe","pipe"]});child.stdout.on("data",data=>logs+=data);child.stderr.on("data",data=>logs+=data);
  let base;for(let i=0;i<500;i++){try{base=readinessURL(await readFile(ready,"utf8"));}catch(error){if(error.code!=="ENOENT")throw error;}if(base)break;if(child.exitCode!==null)throw Error(logs);await pause(10);}assert.ok(base,"server readiness");
  let cookie,claim;
  await t.test("file SSR and native endpoint work from the source-hidden bundle",async()=>{
   const page=await fetch(base+"/upload-probe");assert.equal(page.status,200);const html=await page.text();assert.match(html,/type="file"/);assert.match(html,/data-ruvane-version="14"/);assert.match(html,/<input[^>]*type="file"[^>]*disabled/);assert.doesNotMatch(html,/<input[^>]*type="file"[^>]*value=/);
   const denied=await fetch(base+"/uploads/state");assert.equal(denied.status,403);await denied.arrayBuffer();
   const session=await fetch(base+"/upload-session",{method:"POST",headers:{Origin:base,"Content-Type":"application/json"},body:"{}"});assert.equal(session.status,200);cookie=session.headers.getSetCookie()[0].split(";")[0];assert.equal(await session.text(),"true");
   const token=await fetch(base+"/csrf-token",{headers:{Cookie:cookie}});claim=token.headers.get("Ruvane-Csrf-Token");await token.arrayBuffer();assert.match(claim,/^[a-f0-9]{64}$/);
   const data=new FormData();data.set("caption","Rust + %41");data.set("attachment",new Blob([new Uint8Array([0,255,128])]),"Rust 🦀.bin");
   const uploaded=await fetch(base+"/upload",{method:"POST",headers:{Origin:base,Cookie:cookie,"Ruvane-Csrf-Token":claim,Accept:"application/json"},body:data});assert.equal(uploaded.status,200);assert.deepEqual(await uploaded.json(),{caption:"Rust + %41",filename:"Rust 🦀.bin",bytes:3});
  });
  await t.test("missing CSRF and excessive file size cannot create a stored record",async()=>{
   for(const size of [3,32769]){const data=new FormData();data.set("caption","Rejected");data.set("attachment",new Blob([new Uint8Array(size)]),"x.bin");const result=await fetch(base+"/upload",{method:"POST",headers:{Origin:base,Cookie:cookie,...(size===32769?{"Ruvane-Csrf-Token":claim}:{})},body:data});assert.equal(result.status,size===3?403:413);await result.arrayBuffer();}
   const state=await fetch(base+"/uploads/state",{headers:{Cookie:cookie}});assert.equal((await state.json()).files.length,1);
  });
 } finally {
  if(child&&child.exitCode===null){const exit=new Promise(resolve=>child.once("exit",resolve));child.stdin.end();await exit;}
  if(hidden)await rename(hidden,join(project,"src"));await rm(directory,{recursive:true,force:true});
 }
});

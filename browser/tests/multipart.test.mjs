import test from "node:test";
import assert from "node:assert/strict";
import {spawn} from "node:child_process";
import {mkdtemp,readFile,writeFile,mkdir,rename,rm} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join,resolve} from "node:path";
import {fileURLToPath} from "node:url";
import {cookieFixture} from "./cookies-fixture.mjs";
import {readinessURL} from "./readiness.mjs";
const root=fileURLToPath(new URL("../../",import.meta.url)),cli=resolve(process.env.RUVANE_CLI||join(root,"target/debug/ruvane"));
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
test("multipart routes survive native/Wasm packaging without shipping upload backends",{timeout:120000},async t=>{
 const directory=await mkdtemp(join(tmpdir(),"ruvane-multipart-"));let child,hidden,project;
 try {
  const fixture=await cookieFixture({directory,root,cli,prepare:async project=>{
   const route=join(project,"src/routes/multipart");await mkdir(route,{recursive:true});
   await writeFile(join(route,"route.rs"),String.raw`
use ruvane::*;
use std::sync::atomic::{AtomicUsize,Ordering};
static CALLS:AtomicUsize=AtomicUsize::new(0);
pub fn route()->FileEndpoint {
 FileEndpoint::new().get(|request|{assert!(request.multipart().is_none());Response::json(&(CALLS.load(Ordering::SeqCst) as u64).into())}).post_multipart(|request|{
  const MARKER:&str="SERVER_MULTIPART_BACKEND_ALPHA56";assert_eq!(MARKER.len(),32);
  assert!(request.form_data().is_none()&&request.json().is_none());
  let data=request.multipart().unwrap();let Some(file)=data.file()else{return Response::text("File required").with_status(StatusCode::BadRequest);};
  CALLS.fetch_add(1,Ordering::SeqCst);
  let sample=file.bytes().iter().take(32).map(|byte|format!("{byte:02x}")).collect::<String>();
  Response::json(&JsonValue::object(vec![("filename".into(),JsonValue::string(file.file_name()).unwrap()),("field".into(),JsonValue::string(file.field_name()).unwrap()),("length".into(),(file.len() as u64).into()),("declared_type".into(),JsonValue::string(file.content_type()).unwrap()),("caption".into(),JsonValue::string(data.fields().get("caption").unwrap_or("")).unwrap()),("sample".into(),JsonValue::string(sample).unwrap())]).unwrap())
 })
}
`);
  }});project=fixture.project;
  const module=await WebAssembly.compile(await readFile(join(fixture.bundle,"resources/ruvane-runtime",BigInt(fixture.revision).toString(16).padStart(16,"0"),"app.wasm")));
  await t.test("Wasm keeps ABI v14 and excludes native upload code",()=>{
   assert.deepEqual(WebAssembly.Module.imports(module),[]);const exports=new WebAssembly.Instance(module,{}).exports;
   assert.equal(exports.ruvane_abi_version(),14);assert.equal(WebAssembly.Module.exports(module).filter(item=>item.kind==="function").length,22);
  });
  const wasm=await readFile(join(fixture.bundle,"resources/ruvane-runtime",BigInt(fixture.revision).toString(16).padStart(16,"0"),"app.wasm"));assert.equal(wasm.includes(Buffer.from("SERVER_MULTIPART_BACKEND_ALPHA56")),false);
  hidden=join(project,"source-hidden");await rename(join(project,"src"),hidden);
  const ready=join(directory,"ready");let logs="";child=spawn(join(fixture.bundle,"ruvane-app"),["--ruvane-serve","0",ready],{cwd:fixture.bundle,stdio:["pipe","pipe","pipe"]});child.stdout.on("data",data=>{logs+=data;});child.stderr.on("data",data=>{logs+=data;});
  let base;for(let i=0;i<500;i++){try{base=readinessURL(await readFile(ready,"utf8"));}catch(error){if(error.code!=="ENOENT")throw error;}if(base)break;if(child.exitCode!==null)throw Error(logs);await pause(10);}assert.ok(base,"server readiness");
  const calls=async()=>await(await fetch(base+"/multipart")).json();
  const send=async(bytes,name="Rust 🦀.bin",extra={})=>{const form=new FormData();form.set("caption","Rust + %41\r\nnext");form.set("attachment",new Blob([bytes],{type:"application/octet-stream"}),name);return fetch(base+"/multipart",{method:"POST",headers:{Origin:base,...extra},body:form});};
  await t.test("browser-compatible FormData preserves Unicode names and binary payloads",async()=>{
   const bytes=new Uint8Array([0,255,13,10,128,43,37]);const response=await send(bytes);assert.equal(response.status,200);assert.equal(response.headers.get("cache-control"),"no-store");
   assert.deepEqual(await response.json(),{filename:"Rust 🦀.bin",field:"attachment",length:7,declared_type:"application/octet-stream",caption:"Rust + %41\nnext",sample:"00ff0d0a802b25"});
  });
  await t.test("file budgets reject oversized input before the native handler",async()=>{
   const exact=await send(new Uint8Array(32768));assert.equal(exact.status,200);assert.equal((await exact.json()).length,32768);
   const before=await calls(),rejected=await send(new Uint8Array(32769));assert.equal(rejected.status,413);await rejected.body?.cancel();assert.equal(await calls(),before);
  });
  await t.test("duplicate text/file names and request media confusion skip the native handler",async()=>{
   const before=await calls(),form=new FormData();form.append("attachment","shadow");form.append("attachment",new Blob(["x"]),"x.bin");
   const duplicate=await fetch(base+"/multipart",{method:"POST",headers:{Origin:base},body:form});assert.equal(duplicate.status,400);await duplicate.arrayBuffer();
   const wrong=await fetch(base+"/multipart",{method:"POST",headers:{Origin:base,"Content-Type":"application/json"},body:"{}"});assert.equal(wrong.status,415);await wrong.arrayBuffer();assert.equal(await calls(),before);
  });
  await t.test("foreign origins and stale application revisions cannot execute uploads",async()=>{
   const before=await calls();for(const extra of [{Origin:"http://foreign.example"},{"Ruvane-Application-Revision":String(BigInt(fixture.revision)+1n)}]) {const response=await send(new Uint8Array([1]),"x.bin",extra);assert.equal(response.status,extra.Origin?403:409);await response.arrayBuffer();}assert.equal(await calls(),before);
  });
 } finally {
  if(child&&child.exitCode===null){const exit=new Promise(resolve=>child.once("exit",resolve));child.stdin.end();await exit;}
  if(hidden)await rename(hidden,join(project,"src"));await rm(directory,{recursive:true,force:true});
 }
});

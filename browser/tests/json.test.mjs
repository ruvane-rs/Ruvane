import test from "node:test";
import assert from "node:assert/strict";
import {spawn} from "node:child_process";
import {mkdtemp,readFile,rm,rename,realpath} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join,resolve} from "node:path";
import {fileURLToPath} from "node:url";
import {jsonFixture} from "./json-fixture.mjs";
import {validateExports,decodeCommands} from "../../web/bridge.mjs";
const root=fileURLToPath(new URL("../../",import.meta.url)),cli=resolve(process.env.RUVANE_CLI||join(root,"target/debug/ruvane"));
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));

await test("bounded JSON APIs use native guards and an import-free shared Rust parser",{skip:!(process.platform==="darwin"||process.platform==="linux"&&["x64","arm64"].includes(process.arch))},async t=>{
 const directory=await mkdtemp(join(await realpath(tmpdir()),"ruvane-json-"));let child;
 async function stop(){if(!child)return;const current=child;child=null;if(current.exitCode!==null)return;const ended=new Promise(resolve=>current.once("exit",resolve));current.stdin.end();await ended;}
 try {
  const {project,bundle,revision}=await jsonFixture({directory,root,cli});
  const wasm=await readFile(join(bundle,"resources/ruvane-runtime",BigInt(revision).toString(16).padStart(16,"0"),"app.wasm"));
  const module=await WebAssembly.compile(wasm),e=validateExports(new WebAssembly.Instance(module,{}).exports);
  await t.test("the shared parser executes inside Wasm without native backends or new exports",()=>{
   assert.deepEqual(WebAssembly.Module.imports(module),[]);assert.equal(e.ruvane_abi_version(),12);assert.equal(WebAssembly.Module.exports(module).filter(item=>item.kind==="function").length,22);assert.equal(wasm.includes(Buffer.from("SERVER_JSON_BACKEND_ALPHA51")),false);
   const id=e.ruvane_mount(1,25,1,0);assert.ok(id,`Wasm JSON probe failed with ${e.ruvane_last_error()}`);const commands=decodeCommands(new Uint8Array(e.memory.buffer,e.ruvane_commands_ptr()>>>0,e.ruvane_commands_len()>>>0).slice());assert.ok(commands.some(command=>command.text==="Rust JSON checks passed"));assert.equal(e.ruvane_commands_clear(),1);assert.equal(e.ruvane_dispose(id),1);assert.equal(e.ruvane_commands_clear(),1);
  });
  await rename(join(project,"src"),join(project,"source-hidden"));let generation=0;
  async function start(){const ready=join(directory,`ready-${generation++}`);child=spawn(join(bundle,"ruvane-app"),["--ruvane-serve","0",ready],{cwd:bundle,stdio:["pipe","ignore","pipe"]});let errors="";child.stderr.on("data",data=>errors+=data);for(let n=0;n<250;n++){const value=await readFile(ready,"utf8").catch(()=>"");if(value)return `http://${value.trim().split("\t").at(-1)}`;if(child.exitCode!==null)throw Error(errors);await pause(20);}throw Error("JSON fixture did not start");}
  let base=await start();
  const get=(path,cookie,method="GET")=>fetch(base+path,{method,cache:"no-store",headers:{...(cookie?{Cookie:cookie}:{})}});
  const stats=async()=>await(await get("/json-stats")).json();
  const token=async cookie=>{const response=await get("/csrf-token",cookie);await response.arrayBuffer();return response.headers.get("Ruvane-Csrf-Token");};
  const session=async(mode,cookie)=>{const claim=cookie?await token(cookie):null;const response=await fetch(base+"/session-action",{method:"POST",headers:{Origin:base,"Content-Type":"application/x-www-form-urlencoded",Accept:"application/vnd.ruvane.action","Ruvane-Application-Revision":revision,...(cookie?{Cookie:cookie}:{}),...(claim?{"Ruvane-Csrf-Token":claim}:{})},body:`mode=${mode}`});assert.equal(response.status,200);const values=response.headers.getSetCookie();assert.equal(values.length,1);await response.arrayBuffer();return values[0].split(";")[0];};
  const post=(body,cookie,claim,extra={},path="/api/tasks")=>fetch(base+path,{method:"POST",headers:{Origin:base,"Content-Type":"application/json",...(cookie?{Cookie:cookie}:{}),...(claim?{"Ruvane-Csrf-Token":claim}:{}),...extra},body});
  let cookie,claim;
  await t.test("anonymous access is denied and GET/HEAD return fixed no-store UTF-8 JSON",async()=>{
   const denied=await get("/api/tasks");assert.equal(denied.status,403);await denied.arrayBuffer();cookie=await session("open");claim=await token(cookie);
   for(const method of ["GET","HEAD"]){const response=await get("/api/tasks",cookie,method);assert.equal(response.status,200);assert.equal(response.headers.get("content-type"),"application/json");assert.equal(response.headers.get("cache-control"),"no-store");assert.equal(response.headers.get("x-content-type-options"),"nosniff");assert.ok(response.headers.get("content-security-policy").includes("script-src 'none'"));assert.equal(response.headers.get("content-length"),"12");assert.equal(await response.text(),method==="HEAD"?"":'{"tasks":[]}');}
  });
  await t.test("a Unicode task is created once and reads do not cache earlier output",async()=>{
   const before=await stats(),title='Rust 🦀 "quotes" \\ slash </script>';
   const response=await post(JSON.stringify({title}),cookie,claim);assert.equal(response.status,200);assert.deepEqual(await response.json(),{id:1,title});assert.deepEqual(await(await get("/api/tasks",cookie)).json(),{tasks:[{id:1,title}]});const after=await stats();assert.equal(after.writes,before.writes+1);assert.equal(after.calls,before.calls+1);
  });
  await t.test("malformed and oversized JSON never reaches guards or mutation handlers",async()=>{
   const before=await stats();
   for(const body of ['', '{"title":"PRIVATE_INPUT",}', '{"title":"x","\\u0074itle":"y"}', '"\\uD800"','null true','[0,]',new Uint8Array([34,255,34])]) {const response=await post(body,cookie,claim);assert.equal(response.status,400);assert.ok(!(await response.text()).includes("PRIVATE_INPUT"));}
   for(const body of ['['.repeat(17)+'0'+']'.repeat(17),JSON.stringify({title:"x".repeat(4097)}),'['+Array(129).fill(0).join(',')+']',' '.repeat(65537)]) {const response=await post(body,cookie,claim);assert.equal(response.status,413);await response.body?.cancel().catch(()=>{});}
   assert.deepEqual(await stats(),before);
  });
  await t.test("MIME origin encoding and revision checks stay ahead of JSON endpoint execution",async()=>{
   const before=await stats();for(const [extra,status] of [[{"Content-Type":"application/x-www-form-urlencoded"},415],[{"Content-Type":"application/json; charset=ascii"},415],[{"Content-Encoding":"identity"},415],[{Origin:"http://foreign.example"},403],[{"Ruvane-Application-Revision":String(BigInt(revision)+1n)},409]]) {const response=await post('{}',cookie,claim,extra);assert.equal(response.status,status);await response.arrayBuffer();}assert.deepEqual(await stats(),before);
  });
  await t.test("CSRF requires the header and refuses forged cross-session and body-field claims",async()=>{
   const other=await session("open"),foreign=await token(other),before=await stats();
   for(const [value,body] of [[null,JSON.stringify({title:"x",_ruvane_csrf:claim})],["forged",'{"title":"x"}'],[foreign,'{"title":"x"}']]) {const response=await post(body,cookie,value);assert.equal(response.status,403);await response.arrayBuffer();}
   const after=await stats();assert.equal(after.calls,before.calls);assert.equal(after.writes,before.writes);
   await session("close",other);
  });
  await t.test("the application schema rejects unknown fields wrong types and controls before writes",async()=>{
   const before=await stats();for(const body of ['null','[]','true','1','{}','{"title":7}','{"title":""}','{"title":"x","extra":true}','{"title":"line\\nbreak"}',JSON.stringify({title:"x".repeat(257)})]) {const response=await post(body,cookie,claim);assert.equal(response.status,400);assert.deepEqual(await response.json(),{error:"Bad Request"});}assert.equal((await stats()).writes,before.writes);
  });
  await t.test("all JSON value types roundtrip and number lexemes retain exact precision",async()=>{
   for(const body of ['null','true','false','18446744073709551615','1.00e+003','"Rust \\uD83E\\uDD80"','[0,-0,1e9999]','{"__proto__":{"safe":true},"x":1}']) {const response=await post(body,cookie,claim,{},"/api/echo");assert.equal(response.status,200);const raw=await response.text();if(body==='"Rust \\uD83E\\uDD80"')assert.equal(raw,'"Rust 🦀"');else assert.equal(raw,body);}
  });
  await t.test("quota exhaustion returns 429 before another task handler runs",async()=>{
   const before=await stats();for(let n=0;n<3;n++){const response=await post(JSON.stringify({title:`quota ${n}`}),cookie,claim,{},"/api/limited");assert.equal(response.status,n<2?200:429);if(n===2){assert.equal(response.headers.get("ruvane-action-rejected"),"rate-limit");assert.ok(Number(response.headers.get("retry-after"))>0);}await response.arrayBuffer();}assert.equal((await stats()).writes,before.writes+2);
  });
  await t.test("session rotation fences old CSRF claims while new headers keep the API usable",async()=>{
   const old=claim;cookie=await session("rotate",cookie);claim=await token(cookie);assert.notEqual(claim,old);const before=await stats();const response=await post('{"title":"old token"}',cookie,old);assert.equal(response.status,403);await response.arrayBuffer();assert.equal((await stats()).writes,before.writes);const accepted=await post('{"title":"rotated"}',cookie,claim);assert.equal(accepted.status,200);await accepted.arrayBuffer();
  });
  await t.test("concurrent creates publish unique complete records within the store budget",async()=>{
   const responses=await Promise.all(Array.from({length:8},(_,n)=>post(JSON.stringify({title:`parallel ${n}`}),cookie,claim)));const items=[];for(const response of responses){assert.equal(response.status,200);items.push(await response.json());}assert.equal(new Set(items.map(item=>item.id)).size,8);assert.equal(new Set(items.map(item=>item.title)).size,8);
   const list=await(await get("/api/tasks",cookie)).json();for(const item of items)assert.ok(list.tasks.some(value=>value.id===item.id&&value.title===item.title));
  });
  await t.test("task capacity rejects further creation without partial writes",async()=>{
   let count=(await stats()).tasks;while(count<32){const response=await post(JSON.stringify({title:`capacity ${count}`}),cookie,claim);assert.equal(response.status,200);await response.arrayBuffer();count++;}const before=await stats(),response=await post('{"title":"overflow"}',cookie,claim);assert.equal(response.status,503);await response.arrayBuffer();assert.equal((await stats()).writes,before.writes);assert.equal((await stats()).tasks,32);
  });
  await session("close",cookie);await stop();base=await start();
  await t.test("restart drops demo data and sessions without needing source files",async()=>{assert.equal((await stats()).tasks,0);const response=await get("/api/tasks",cookie);assert.equal(response.status,403);await response.arrayBuffer();});
 } finally {await stop();await rm(directory,{recursive:true,force:true});}
});

import test from "node:test";
import assert from "node:assert/strict";
import {spawn,spawnSync} from "node:child_process";
import {mkdtemp,mkdir,readFile,rename,rm,readdir,stat,writeFile,realpath} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join,resolve} from "node:path";
import {fileURLToPath} from "node:url";
import {fileStoreFixture} from "./file-store-fixture.mjs";
import {readinessURL} from "./readiness.mjs";
const root=fileURLToPath(new URL("../../",import.meta.url)),cli=resolve(process.env.RUVANE_CLI||join(root,"target/debug/ruvane")),pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
test("private file storage persists inside a source-hidden native/Wasm bundle",{timeout:120000,skip:!(process.platform==="darwin"||process.platform==="linux"&&["x64","arm64"].includes(process.arch))},async t=>{
 const directory=await mkdtemp(join(await realpath(tmpdir()),"ruvane-file-store-")),storage=join(directory,"private-store");let child,fixture,hidden,generation=0,base;
 async function stop(hard=false){if(!child)return;const current=child;child=null;if(current.exitCode!==null)return;const ended=new Promise(resolve=>current.once("exit",resolve));if(hard)current.kill("SIGKILL");else current.stdin.end();await ended;}
 async function start(recover=false,expectFailure=false){
  const ready=join(directory,`ready-${generation++}`);let logs="";
  child=spawn(join(fixture.bundle,"ruvane-app"),["--ruvane-serve","0",ready],{cwd:fixture.bundle,stdio:["pipe","ignore","pipe"],env:{...process.env,RUVANE_FILE_DIRECTORY:storage,RUVANE_FILE_RECOVER:recover?"1":"0"}});child.stderr.on("data",data=>logs+=data);
  for(let n=0;n<500;n++){let value="";try{value=await readFile(ready,"utf8");}catch(error){if(error.code!=="ENOENT")throw error;}const url=readinessURL(value);if(url){assert.equal(expectFailure,false,"expected startup refusal");base=url;return;}if(child.exitCode!==null){if(expectFailure){assert.notEqual(child.exitCode,0);return logs;}throw Error(logs);}await pause(10);}throw Error("File store fixture did not start");
 }
 const request=(path,cookie,extra={})=>fetch(base+"/private-files"+path,{...extra,redirect:"manual",headers:{...(cookie?{Cookie:cookie}:{}),...extra.headers}});
 const list=async cookie=>{const response=await request("/files",cookie);assert.equal(response.status,200);return(await response.json()).files;};
 async function session(account){const response=await request("/session",null,{method:"POST",headers:{Origin:base,"Content-Type":"application/x-www-form-urlencoded"},body:`account=${account}`});assert.equal(response.status,303);const cookie=response.headers.getSetCookie()[0].split(";")[0];await response.arrayBuffer();const token=await request("/csrf",cookie);await token.arrayBuffer();return {cookie,token:token.headers.get("Ruvane-Csrf-Token")};}
 async function upload(owner,bytes=new Uint8Array([0,255,13,10,128]),extra={}){const body=new FormData();body.set("attachment",new Blob([bytes],{type:"text/html"}),"Rust 🦀.html");if(extra.field)body.set("owner","alpha");return request("/files",owner.cookie,{method:"POST",headers:{Origin:base,Accept:"application/json","Ruvane-Csrf-Token":owner.token,...extra.headers},body});}
 let a,b,info;
 try {
  fixture=await fileStoreFixture({directory,root,cli});
  const wasm=await readFile(join(fixture.bundle,"resources/ruvane-runtime",BigInt(fixture.revision).toString(16).padStart(16,"0"),"app.wasm")),module=await WebAssembly.compile(wasm);
  await t.test("ABI v14 and 22 exports exclude native private storage code",()=>{assert.deepEqual(WebAssembly.Module.imports(module),[]);assert.equal(new WebAssembly.Instance(module,{}).exports.ruvane_abi_version(),14);assert.equal(WebAssembly.Module.exports(module).filter(item=>item.kind==="function").length,22);assert.equal(wasm.includes(Buffer.from("ruvane-files\t1\t")),false);});
  hidden=join(fixture.project,"source-hidden");await rename(join(fixture.project,"src"),hidden);await start();
  await t.test("anonymous callers cannot list or download files",async()=>{for(const path of ["/files","/files/"+"a".repeat(64)]){const response=await request(path);assert.equal(response.status,403);assert.equal(response.headers.get("content-disposition"),null);await response.arrayBuffer();}a=await session("alpha");b=await session("beta");});
  await t.test("one guarded binary upload becomes a safe GET/HEAD attachment",async()=>{
   const response=await upload(a);assert.equal(response.status,200);info=await response.json();assert.match(info.id,/^[a-f0-9]{64}$/);assert.deepEqual(Object.keys(info).sort(),["bytes","id","name"]);assert.equal(info.bytes,5);
   for(const method of ["GET","HEAD"]){const download=await request("/files/"+info.id,a.cookie,{method});assert.equal(download.status,200);assert.equal(download.headers.get("content-type"),"application/octet-stream");assert.equal(download.headers.get("content-disposition"),"attachment; filename=\"download\"; filename*=UTF-8''Rust%20%F0%9F%A6%80.html");assert.equal(download.headers.get("cache-control"),"no-store");assert.equal(download.headers.get("x-content-type-options"),"nosniff");assert.ok(download.headers.get("content-security-policy").includes("script-src 'none'"));assert.deepEqual(new Uint8Array(await download.arrayBuffer()),method==="GET"?new Uint8Array([0,255,13,10,128]):new Uint8Array());}
  });
  await t.test("foreign IDs, guessed owners and untrusted business fields never cross owner boundaries",async()=>{
   assert.equal((await list(b.cookie)).length,0);
   for(const method of ["GET","DELETE"]){const response=await request("/files/"+info.id+"?owner=alpha",b.cookie,{method,headers:method==="DELETE"?{Origin:base,"Ruvane-Csrf-Token":b.token}:{}});assert.equal(response.status,404);assert.equal(response.headers.get("content-disposition"),null);assert.equal(await response.text(),"File not found.");}
   const forged=await upload(b,new Uint8Array([1]),{field:true});assert.equal(forged.status,400);await forged.arrayBuffer();assert.equal((await list(a.cookie)).length,1);assert.equal((await list(b.cookie)).length,0);
  });
  await t.test("missing CSRF, foreign origin, stale revisions and file limits skip storage",async()=>{
   for(const [headers,status]of [[{"Ruvane-Csrf-Token":"a".repeat(64)},403],[{Origin:"http://foreign.example"},403],[{"Ruvane-Application-Revision":String(BigInt(fixture.revision)+1n)},409]]){const response=await upload(a,new Uint8Array([1]),{headers});assert.equal(response.status,status);await response.arrayBuffer();}
   const large=await upload(a,new Uint8Array(32769));assert.equal(large.status,413);await large.arrayBuffer();const denied=await request("/files/"+info.id,a.cookie,{method:"DELETE",headers:{Origin:base}});assert.equal(denied.status,403);await denied.arrayBuffer();assert.equal((await list(a.cookie)).length,1);
  });
  await t.test("clean shutdown keeps private disk permissions and allows restart",async()=>{
   await stop();assert.deepEqual((await readdir(storage)).sort(),["identity","store.bin"]);assert.equal((await stat(storage)).mode&0o777,0o700);assert.equal((await stat(join(storage,"store.bin"))).mode&0o777,0o600);
   const masked=join(directory,"masked-store");await mkdir(masked,{mode:0o700});
   const probe=spawnSync("/bin/sh",["-c",'umask 0600; exec "$@"',"sh",join(fixture.bundle,"ruvane-app"),"--ruvane-serve","0",join(directory,"masked-ready")],{cwd:fixture.bundle,env:{...process.env,RUVANE_FILE_DIRECTORY:masked,RUVANE_FILE_RECOVER:"0"},input:"",encoding:"utf8",timeout:5000});
   assert.equal(probe.error,undefined);assert.notEqual(probe.status,0);assert.match(probe.stderr,/Error: Unavailable/);assert.equal((await readdir(masked)).includes("store.bin"),false,"unsafe inherited file permissions refuse initialization");
   await start();const old=await request("/files",a.cookie);assert.equal(old.status,403);await old.arrayBuffer();a=await session("alpha");b=await session("beta");assert.deepEqual(await list(a.cookie),[info]);assert.equal((await list(b.cookie)).length,0);
  });
  await t.test("an interrupted process requires explicit offline recovery without losing acknowledged files",async()=>{
   await stop(true);assert.ok((await readdir(storage)).includes("LOCK"));const refused=await start(false,true);assert.match(refused,/Error: Busy/);await stop();await start(true);a=await session("alpha");assert.deepEqual(await list(a.cookie),[info]);
  });
  await t.test("owner deletion is durable and frees the per-owner quota",async()=>{
   for(let n=0;n<7;n++){const response=await upload(a,new Uint8Array([n]));assert.equal(response.status,200);await response.arrayBuffer();}const full=await upload(a,new Uint8Array());assert.equal(full.status,409);await full.arrayBuffer();
   const deleted=await request("/files/"+info.id,a.cookie,{method:"DELETE",headers:{Origin:base,"Ruvane-Csrf-Token":a.token}});assert.equal(deleted.status,200);assert.equal(await deleted.text(),"true");const fresh=await upload(a,new Uint8Array());assert.equal(fresh.status,200);await fresh.arrayBuffer();await stop();await start();a=await session("alpha");assert.equal((await list(a.cookie)).length,8);const missing=await request("/files/"+info.id,a.cookie);assert.equal(missing.status,404);await missing.arrayBuffer();
  });
  await t.test("a damaged committed snapshot is preserved and never silently reset",async()=>{
   await stop();const path=join(storage,"store.bin"),original=await readFile(path);await writeFile(path,new Uint8Array([0,1,2]));const refused=await start(false,true);assert.match(refused,/Error: Corrupt/);assert.deepEqual(await readFile(path),Buffer.from([0,1,2]));await stop();await writeFile(path,original);await start();a=await session("alpha");assert.equal((await list(a.cookie)).length,8);
  });
 } finally {await stop();if(hidden)await rename(hidden,join(fixture.project,"src"));await rm(directory,{recursive:true,force:true});}
});

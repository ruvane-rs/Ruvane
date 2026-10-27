import {readinessURL} from "./readiness.mjs";
import test from "node:test";
import assert from "node:assert/strict";
import {spawn} from "node:child_process";
import {mkdtemp,readFile,writeFile,rm,rename,realpath} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join,resolve} from "node:path";
import {fileURLToPath} from "node:url";
import {decodePage,validateExports} from "../../web/bridge.mjs";
import {cacheFreshness} from "../../web/navigation.mjs";
import {cookieFixture} from "./cookies-fixture.mjs";
const root=fileURLToPath(new URL("../../",import.meta.url));
const cli=resolve(process.env.RUVANE_CLI||join(root,"target/debug/ruvane"));
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
test("cookie cache-reset signals prevent freshness even with otherwise public headers",()=>{
 const headers={"Cache-Control":"public, max-age=30, must-revalidate",Vary:"Accept, Ruvane-Application-Revision"};
 assert.ok(cacheFreshness(new Response("x",{headers}),30000,performance.now()));
 for(const value of ["cookies","","future"]){assert.equal(cacheFreshness(new Response("x",{headers:{...headers,"Ruvane-Cache-Reset":value}}),30000,performance.now()),null);}
});
await test("typed cookies roundtrip through native forms actions private pages and source-free Wasm bundles",async t=>{
 const directory=await mkdtemp(join(await realpath(tmpdir()),"ruvane-cookies-"));let child;
 try{
  const fixture=await cookieFixture({directory,root,cli});const {project,bundle,revision}=fixture;
  const wasm=await readFile(join(bundle,"resources/ruvane-runtime",BigInt(revision).toString(16).padStart(16,"0"),"app.wasm"));
  const module=await WebAssembly.compile(wasm),e=validateExports(new WebAssembly.Instance(module,{}).exports);
  await t.test("cookie transport adds no imports function exports or wire protocol fields",()=>{
   assert.deepEqual(WebAssembly.Module.imports(module),[]);assert.equal(e.ruvane_abi_version(),13);assert.equal(WebAssembly.Module.exports(module).filter(item=>item.kind==="function").length,22);
  });
  await rename(join(project,"src"),join(project,"source-hidden"));
  const ready=join(directory,"ready");child=spawn(join(bundle,process.platform==="win32"?"ruvane-app.exe":"ruvane-app"),["--ruvane-serve","0",ready],{cwd:bundle,stdio:["pipe","ignore","pipe"]});
  let errors="";child.stderr.on("data",data=>errors+=data);
  let base;
  for(let n=0;n<250;n++){const record=await readFile(ready,"utf8").catch(()=>"");const url=readinessURL(record);if(url){base=url;break;}if(child.exitCode!==null)throw Error(errors);await pause(20);}
  assert.ok(base,"cookie fixture starts");
  const headers={Accept:"application/vnd.ruvane.page","Ruvane-Application-Revision":revision};
  const page=async(cookie="")=>{const response=await fetch(base+"/cookie",{headers:{...headers,...(cookie?{Cookie:cookie}:{})}});assert.equal(response.status,200);assert.equal(response.headers.get("cache-control"),"no-store");return decodePage(new Uint8Array(await response.arrayBuffer()));};
  const post=(path,mode,extra={})=>fetch(base+path,{method:"POST",redirect:"manual",headers:{Origin:base,"Content-Type":"application/x-www-form-urlencoded",Accept:"application/vnd.ruvane.action","Ruvane-Application-Revision":revision,...extra},body:`mode=${mode}`});
  let cookie;
  await t.test("typed actions send HttpOnly cookies separately from the reply and retain revision checks",async()=>{
   for(const revisionHeader of [null,String(BigInt(revision)+1n)]){
    const badHeaders={Origin:base,"Content-Type":"application/x-www-form-urlencoded"};if(revisionHeader!==null)badHeaders["Ruvane-Application-Revision"]=revisionHeader;
    const rejected=await fetch(base+"/cookie-action",{method:"POST",headers:badHeaders,body:"mode=set"});assert.equal(rejected.status,409);assert.equal(rejected.headers.get("set-cookie"),null);await rejected.arrayBuffer();
   }
   const response=await post("/cookie-action","set");assert.equal(response.status,200);assert.equal(response.headers.get("ruvane-cache-reset"),"cookies");assert.equal(response.headers.get("cache-control"),"no-store");
   const fields=response.headers.getSetCookie();assert.equal(fields.length,1);assert.match(fields[0],/^ruvane_alpha47_preference=dark; Path=\/; SameSite=Lax; HttpOnly; Max-Age=600$/);cookie=fields[0].split(";")[0];
   const reply=new Uint8Array(await response.arrayBuffer());assert.equal(new TextDecoder().decode(reply.slice(0,4)),"RVA1");
  });
  await t.test("guarded pages read the cookie and malformed duplicates fail before page rendering",async()=>{
   assert.equal((await page()).title,"Cookie preference");assert.equal((await fetch(base+"/cookie-state",{headers:{Cookie:cookie}})).status,200);
   assert.equal(await(await fetch(base+"/cookie-state",{headers:{Cookie:cookie}})).text(),"dark");
   const invalid=await fetch(base+"/cookie",{headers:{...headers,Cookie:cookie+"; "+cookie}});assert.equal(invalid.status,400);assert.equal(invalid.headers.get("set-cookie"),null);await invalid.arrayBuffer();
  });
  await t.test("native POST redirects preserve cookies and deletion uses the identical scope",async()=>{
   const response=await post("/cookie-native","set");assert.equal(response.status,303);assert.equal(response.headers.get("location"),"/cookie");assert.equal(response.headers.getSetCookie().length,1);await response.arrayBuffer();
   const deleted=await post("/cookie-native","clear",{Cookie:cookie});assert.equal(deleted.status,303);assert.match(deleted.headers.getSetCookie()[0],/preference=; Path=\/; SameSite=Lax; HttpOnly; Max-Age=0$/);await deleted.arrayBuffer();
   const rejected=await post("/cookie-native","set",{Origin:"http://foreign.example"});assert.equal(rejected.status,403);assert.equal(rejected.headers.get("set-cookie"),null);await rejected.arrayBuffer();
  });
  await t.test("navigation cookie changes remain no-store and carry only the fixed reset signal",async()=>{
   const plain=await fetch(base+"/cookie-public/a",{headers});assert.equal(plain.headers.get("cache-control"),"public, max-age=30, must-revalidate");await plain.arrayBuffer();
   const rotate=await fetch(base+"/cookie-public/rotate",{headers});assert.equal(rotate.headers.get("cache-control"),"no-store");assert.equal(rotate.headers.get("ruvane-cache-reset"),"cookies");assert.equal(rotate.headers.getSetCookie().length,1);assert.equal(decodePage(new Uint8Array(await rotate.arrayBuffer())).layouts.length,1);
  });
 } finally{
  if(child&&child.exitCode===null){const ended=new Promise(resolve=>child.once("exit",resolve));child.stdin.end();await ended;}
  await rm(directory,{recursive:true,force:true});
 }
});

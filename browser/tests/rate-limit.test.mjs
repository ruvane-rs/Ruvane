import {readinessURL} from "./readiness.mjs";
import test from "node:test";
import assert from "node:assert/strict";
import {spawn} from "node:child_process";
import {mkdtemp,readFile,rm,rename,realpath} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join,resolve} from "node:path";
import {fileURLToPath} from "node:url";
import {rateLimitFixture} from "./rate-limit-fixture.mjs";
import {readRetryAfter,decodePage,validateExports} from "../../web/bridge.mjs";
const root=fileURLToPath(new URL("../../",import.meta.url)),cli=resolve(process.env.RUVANE_CLI||join(root,"target/debug/ruvane"));
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));

test("429 retry deltas reject missing date duplicate zero excessive and public-cache values",()=>{
  const response=(value,status=429,cache="no-store")=>new Response("",{status,headers:{...(value===null?{}:{"Retry-After":value}),"Cache-Control":cache}});
  for(const value of [null,"0","-1","01","1.5","1, 2","86401","9999999999999999999999","Thu, 01 Jan 1970 00:00:00 GMT","invalid"])assert.equal(readRetryAfter(response(value)),null);
  for(const value of ["1","60","86400"])assert.equal(readRetryAfter(response(value)),Number(value));
  assert.equal(readRetryAfter(response("1",503)),null);assert.equal(readRetryAfter(response("1",429,"public, max-age=60")),null);
});

await test("native route quotas fence mutation caches and rotated session families in source-free bundles",{skip:!(process.platform==="darwin"||process.platform==="linux"&&["x64","arm64"].includes(process.arch))},async t=>{
 const directory=await mkdtemp(join(await realpath(tmpdir()),"ruvane-rates-"));let child;
 async function stop(){if(!child)return;const current=child;child=null;if(current.exitCode!==null)return;const ended=new Promise(resolve=>current.once("exit",resolve));current.stdin.end();await ended;}
 try {
  const {project,bundle,revision}=await rateLimitFixture({directory,root,cli});
  const wasm=await readFile(join(bundle,"resources/ruvane-runtime",BigInt(revision).toString(16).padStart(16,"0"),"app.wasm"));
  const module=await WebAssembly.compile(wasm),e=validateExports(new WebAssembly.Instance(module,{}).exports);
  assert.deepEqual(WebAssembly.Module.imports(module),[]);assert.equal(e.ruvane_abi_version(),13);assert.equal(WebAssembly.Module.exports(module).filter(item=>item.kind==="function").length,22);assert.equal(wasm.includes(Buffer.from("SERVER_RATE_LIMIT_ALPHA50")),false);
  await rename(join(project,"src"),join(project,"source-hidden"));let generation=0;
  async function start(){const ready=join(directory,`ready-${generation++}`);child=spawn(join(bundle,"ruvane-app"),["--ruvane-serve","0",ready],{cwd:bundle,stdio:["pipe","ignore","pipe"]});let errors="";child.stderr.on("data",data=>errors+=data);for(let n=0;n<250;n++){const value=await readFile(ready,"utf8").catch(()=>"");const url=readinessURL(value);if(url)return url;if(child.exitCode!==null)throw Error(errors);await pause(20);}throw Error("quota fixture did not start");}
  let base=await start();
  const get=(path,cookie,extra={},method="GET")=>fetch(base+path,{method,headers:{...(cookie?{Cookie:cookie}:{}),...extra},cache:"no-store"});
  const stats=async()=>{const [mutations,loads,allowed,limited,pageAllowed,pageLimited]=(await(await get("/rate-stats")).text()).split(" ").map(Number);return {mutations,loads,allowed,limited,pageAllowed,pageLimited};};
  const reset=async()=>{const response=await fetch(base+"/rate-reset",{method:"POST",headers:{Origin:base,"Content-Type":"application/x-www-form-urlencoded"},body:""});assert.equal(response.status,200);await response.arrayBuffer();};
  const csrf=async cookie=>{const response=await get("/csrf-token",cookie);await response.arrayBuffer();return response.headers.get("Ruvane-Csrf-Token");};
  const post=(mode,cookie,claim,native=false,extra={},body)=>fetch(base+(native?"/session-native":"/session-action"),{method:"POST",redirect:"manual",headers:{Origin:base,"Content-Type":"application/x-www-form-urlencoded",Accept:"application/vnd.ruvane.action","Ruvane-Application-Revision":revision,...(cookie?{Cookie:cookie}:{}),...(claim?{"Ruvane-Csrf-Token":claim}:{}),...extra},body:body??`mode=${mode}`});
  const cookieFrom=async(response,status=200)=>{assert.equal(response.status,status);const fields=response.headers.getSetCookie();assert.equal(fields.length,1);await response.arrayBuffer();return fields[0].split(";")[0];};
  function limited(response,action=false){assert.equal(response.status,429);assert.equal(response.headers.get("cache-control"),"no-store");assert.ok(readRetryAfter(response)>0);assert.equal(response.headers.get("set-cookie"),null);assert.equal(response.headers.get("ruvane-action-rejected"),action?"rate-limit":null);}
  let cookie,other;
  await t.test("all anonymous creation requests share a quota and refusals never reach the handler",async()=>{
   const before=await stats();cookie=await cookieFrom(await post("open"));other=await cookieFrom(await post("open"));
   const denied=await post("open");limited(denied,true);await denied.arrayBuffer();assert.equal((await stats()).mutations,before.mutations+2);
  });
  await t.test("rotation preserves session debt and another family retains its allowance",async()=>{
   const before=await stats();
   for(let n=0;n<2;n++)cookie=await cookieFrom(await post("rotate",cookie,await csrf(cookie)));
   const denied=await post("rotate",cookie,await csrf(cookie));limited(denied,true);await denied.arrayBuffer();assert.equal((await stats()).mutations,before.mutations+2);
   other=await cookieFrom(await post("rotate",other,await csrf(other)));assert.equal((await stats()).mutations,before.mutations+3);
  });
  await t.test("native forms have a route quota and denied 429 sends no redirect or cookie",async()=>{
   const before=await stats();
   for(let n=0;n<2;n++)cookie=await cookieFrom(await post("rotate",cookie,null,true,{},`mode=rotate&_ruvane_csrf=${await csrf(cookie)}`),303);
   const denied=await post("rotate",cookie,null,true,{},`mode=rotate&_ruvane_csrf=${await csrf(cookie)}`);limited(denied,true);assert.equal(denied.headers.get("location"),null);await denied.arrayBuffer();assert.equal((await stats()).mutations,before.mutations+2);
  });
  await reset();
  await t.test("origin revision MIME and CSRF checks reject before consuming route tokens",async()=>{
   const before=await stats(),claim=await csrf(cookie);
   for(const [extra,status,token] of [[{Origin:"http://foreign.example"},403,claim],[{"Content-Type":"application/json"},415,claim],[{"Ruvane-Application-Revision":String(BigInt(revision)+1n)},409,claim],[{},403,null]]) {const response=await post("rotate",cookie,token,false,extra);assert.equal(response.status,status);await response.arrayBuffer();}
   const after=await stats();assert.equal(after.mutations,before.mutations);assert.equal(after.allowed,before.allowed);
   cookie=await cookieFrom(await post("rotate",cookie,claim));
  });
  await t.test("changing routes forged cookies and forwarded addresses cannot reset the global anonymous quota",async()=>{
   for(let n=0;n<3;n++){const response=await get(n===2?"/rate-anonymous-other":"/rate-anonymous",`ruvane_alpha50_session=forged${n}`,{"X-Forwarded-For":`192.0.2.${n}`});if(n<2)assert.equal(response.status,200);else limited(response);await response.arrayBuffer();}
  });
  await t.test("warm cached pages recheck the quota before loaders and emit a closed typed 429",async()=>{
   await reset();const before=await stats();
   for(let n=0;n<2;n++){const response=await get("/rate-page");assert.equal(response.status,200);assert.equal(response.headers.get("cache-control"),"no-store");await response.arrayBuffer();}
   const denied=await get("/rate-page",null,{Accept:"application/vnd.ruvane.page","Ruvane-Application-Revision":revision});limited(denied);const bytes=new Uint8Array(await denied.arrayBuffer()),page=decodePage(bytes);assert.equal(page.status,429);assert.equal(page.title,"Too many requests");assert.equal(page.component,4294967295);assert.equal(page.layouts.length,0);assert.equal((await stats()).loads,before.loads+1);
   assert.equal(e.ruvane_page_begin(bytes.length),1);for(let p=0;p<bytes.length;p+=4){let word=0;const count=Math.min(4,bytes.length-p);for(let n=0;n<count;n++)word|=bytes[p+n]<<(n*8);assert.equal(e.ruvane_page_chunk(word>>>0,count),1);}assert.equal(e.ruvane_page_validate(),1);assert.equal(e.ruvane_commands_clear(),1);
   await pause(1100);const retry=await get("/rate-page");assert.equal(retry.status,200);await retry.arrayBuffer();assert.equal((await stats()).loads,before.loads+1);
  });
  await t.test("HEAD and unmatched descendants consume shared bounded route scopes",async()=>{
   await reset();for(let n=0;n<3;n++){const response=await get(`/rate-unknown/${n}`);assert.equal(response.status,n<2?404:429);if(n===2)limited(response);await response.arrayBuffer();}
   await reset();for(let n=0;n<3;n++){const response=await get("/rate-page",null,{},"HEAD");assert.equal(response.status,n<2?200:429);assert.equal((await response.arrayBuffer()).byteLength,0);}
  });
  await t.test("capacity rejection fails closed instead of evicting an exhausted quota",async()=>{
   let response=await get("/rate-capacity/a");assert.equal(response.status,200);await response.arrayBuffer();response=await get("/rate-capacity/b");assert.equal(response.status,503);await response.arrayBuffer();response=await get("/rate-capacity/a");limited(response);await response.arrayBuffer();
  });
  await t.test("a manual 429 cannot assert pre-handler rejection",async()=>{
   const before=await stats();const response=await fetch(base+"/rate-manual",{method:"POST",headers:{Origin:base,"Content-Type":"application/x-www-form-urlencoded",Accept:"application/vnd.ruvane.action","Ruvane-Application-Revision":revision},body:"mode=rotate"});limited(response);await response.arrayBuffer();assert.equal((await stats()).mutations,before.mutations+1);
  });
  await stop();base=await start();
  await t.test("restart resets process-local quotas and discards native memory sessions",async()=>{
   const response=await get("/rate-capacity/a");assert.equal(response.status,200);await response.arrayBuffer();assert.equal((await stats()).mutations,0);assert.equal(await csrf(cookie),null);
  });
 } finally {await stop();await rm(directory,{recursive:true,force:true});}
});

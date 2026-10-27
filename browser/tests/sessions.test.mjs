import {readinessURL} from "./readiness.mjs";
import test from "node:test";
import assert from "node:assert/strict";
import {spawn} from "node:child_process";
import {mkdtemp,readFile,rm,rename,realpath} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join,resolve} from "node:path";
import {fileURLToPath} from "node:url";
import {decodePage,validateExports} from "../../web/bridge.mjs";
import {sessionFixture} from "./sessions-fixture.mjs";
const root=fileURLToPath(new URL("../../",import.meta.url)),cli=resolve(process.env.RUVANE_CLI||join(root,"target/debug/ruvane"));
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
await test("native memory sessions rotate revoke expire and stay outside Wasm and page payloads",{skip:!(process.platform==="darwin"||process.platform==="linux"&&["x64","arm64"].includes(process.arch))},async t=>{
 const directory=await mkdtemp(join(await realpath(tmpdir()),"ruvane-sessions-"));let child;
 async function stop(){if(!child)return;const current=child;child=null;if(current.exitCode!==null)return;const ended=new Promise(resolve=>current.once("exit",resolve));current.stdin.end();await ended;}
 try{
  const {project,bundle,revision}=await sessionFixture({directory,root,cli});
  const wasm=await readFile(join(bundle,"resources/ruvane-runtime",BigInt(revision).toString(16).padStart(16,"0"),"app.wasm"));
  const module=await WebAssembly.compile(wasm),e=validateExports(new WebAssembly.Instance(module,{}).exports);
  await t.test("the session backend is excluded from the unchanged import-free Wasm ABI",()=>{
   assert.equal(wasm.includes(Buffer.from("SERVER_SESSION_DATA_ALPHA48")),false);assert.deepEqual(WebAssembly.Module.imports(module),[]);assert.equal(e.ruvane_abi_version(),15);assert.equal(WebAssembly.Module.exports(module).filter(item=>item.kind==="function").length,22);
  });
  await rename(join(project,"src"),join(project,"source-hidden"));let next=0;
  async function start(idle=300,absolute=3600){const ready=join(directory,`ready-${next++}`);child=spawn(join(bundle,"ruvane-app"),["--ruvane-serve","0",ready],{cwd:bundle,env:{...process.env,RUVANE_SESSION_IDLE_SECONDS:String(idle),RUVANE_SESSION_ABSOLUTE_SECONDS:String(absolute)},stdio:["pipe","ignore","pipe"]});let errors="";child.stderr.on("data",data=>errors+=data);for(let n=0;n<250;n++){const value=await readFile(ready,"utf8").catch(()=>"");const url=readinessURL(value);if(url)return url;if(child.exitCode!==null)throw Error(errors);await pause(20);}throw Error("session fixture did not start");}
  let base=await start();const headers={Accept:"application/vnd.ruvane.page","Ruvane-Application-Revision":revision};
  const stats=async()=>{const [entries,bytes,loads,cache]=(await(await fetch(base+"/session-stats")).text()).split(" ").map(Number);return {entries,bytes,loads,cache};};
  const page=async(cookie,path="/session-private")=>{const response=await fetch(base+path,{headers:{...headers,...(cookie?{Cookie:cookie}:{})}});return {response,bytes:new Uint8Array(await response.arrayBuffer())};};
  const post=(mode,cookie,extra={},native=false)=>fetch(base+(native?"/session-native":"/session-action"),{method:"POST",redirect:"manual",headers:{Accept:"application/vnd.ruvane.action",Origin:base,"Content-Type":"application/x-www-form-urlencoded","Ruvane-Application-Revision":revision,...(cookie?{Cookie:cookie}:{}),...extra},body:`mode=${mode}`});
  const token=async(response,status=200)=>{assert.equal(response.status,status);assert.equal(response.headers.get("cache-control"),"no-store");assert.equal(response.headers.get("ruvane-cache-reset"),"cookies");const fields=response.headers.getSetCookie();assert.equal(fields.length,1);assert.ok(/HttpOnly/.test(fields[0])&&/SameSite=Strict/.test(fields[0]));const cookie=fields[0].split(";")[0];await response.arrayBuffer();return cookie;};
  let original,current;
  await t.test("anonymous forged unknown and wrong-source credentials never run private loaders",async()=>{
   const before=await stats();for(const cookie of [undefined,"ruvane_alpha48_session=forged","ruvane_alpha48_session="+"f".repeat(64)]){const {response,bytes}=await page(cookie);assert.equal(response.status,403);assert.equal(decodePage(bytes).component,4294967295);assert.equal(response.headers.get("cache-control"),"no-store");}
   assert.deepEqual(await stats(),before);for(const [extra,status] of [[{Origin:"http://foreign.example"},403],[{"Ruvane-Application-Revision":String(BigInt(revision)+1n)},409],[{"Content-Type":"application/json"},415]]){const response=await post("open",null,extra);assert.equal(response.status,status);assert.equal(response.headers.get("set-cookie"),null);await response.arrayBuffer();}assert.equal((await stats()).entries,0);
  });
  await t.test("opening a session issues only an opaque cookie and resolves server-held data",async()=>{
   original=await token(await post("open"));current=original;assert.ok(/^ruvane_alpha48_session=[0-9a-f]{64}$/.test(current));
   const {response,bytes}=await page(current);assert.equal(response.status,200);assert.equal(decodePage(bytes).title,"Private session page");assert.equal(Buffer.from(bytes).includes(Buffer.from(current.split("=")[1])),false);assert.equal(Buffer.from(bytes).includes(Buffer.from("SERVER_SESSION_DATA_ALPHA48")),false);assert.equal((await stats()).entries,1);assert.equal((await stats()).cache,0);
   const html=await(await fetch(base+"/session-private",{headers:{Cookie:current}})).text();assert.ok(!html.includes(current.split("=")[1])&&!html.includes("SERVER_SESSION_DATA_ALPHA48"));
  });
  await t.test("rotation rejects the earlier cookie and later logout rejects every retired generation",async()=>{
   current=await token(await post("rotate",current));assert.ok(current!==original,"identifier changed");assert.equal((await page(original)).response.status,403);assert.equal((await page(current)).response.status,200);
   const removal=await token(await post("close",current));assert.ok(removal.endsWith("="));assert.equal((await page(current)).response.status,403);assert.equal((await page(original)).response.status,403);assert.equal((await stats()).entries,0);
  });
  await t.test("concurrent rotations have one winner and no duplicate live family",async()=>{
   const cookie=await token(await post("open"));const responses=await Promise.all([post("rotate",cookie),post("rotate",cookie)]);assert.deepEqual(responses.map(response=>response.status).sort(),[200,403]);
   for(const response of responses){if(response.status===200)current=await token(response);else await response.arrayBuffer();}
   assert.equal((await page(cookie)).response.status,403);assert.equal((await stats()).entries,1);await token(await post("close",current));
  });
  await t.test("native create rotate and logout preserve internal 303 cookie delivery",async()=>{
   let response=await post("open",null,{},true);assert.equal(response.headers.get("location"),"/session");original=await token(response,303);response=await post("rotate",original,{},true);current=await token(response,303);assert.equal((await page(original)).response.status,403);await token(await post("close",current,{},true),303);assert.equal((await page(current)).response.status,403);
  });
  await t.test("capacity limits reject excess sessions without evicting live credentials",async()=>{
   const cookies=[];for(let n=0;n<4;n++)cookies.push(await token(await post("open")));
   const full=await post("open");assert.equal(full.status,503);assert.equal(full.headers.get("set-cookie"),null);await full.arrayBuffer();assert.equal((await page(cookies[0])).response.status,200);assert.equal((await stats()).entries,4);
   for(const cookie of cookies)await token(await post("close",cookie));
  });
  original=await token(await post("open"));await stop();base=await start(1,3);
  await t.test("restart discards the old memory store even with the same executable revision",async()=>{assert.equal((await page(original)).response.status,403);assert.deepEqual(await stats(),{entries:0,bytes:0,loads:0,cache:0});});
  await t.test("idle expiration blocks the page before loaders and remains independent of cookie age",async()=>{
   current=await token(await post("open"));const before=await stats();await pause(1150);assert.equal((await page(current)).response.status,403);assert.equal((await stats()).loads,before.loads);assert.equal((await stats()).entries,0);
  });
  await stop();base=await start(2,3);
  await t.test("absolute expiry remains authoritative while valid requests refresh idle time",async()=>{
   current=await token(await post("open"));
   for(let n=0;n<4;n++){await pause(400);assert.equal((await page(current)).response.status,200);}
   await pause(1500);const before=await stats();assert.equal((await page(current)).response.status,403);assert.equal((await stats()).loads,before.loads);
  });
 }finally{await stop();await rm(directory,{recursive:true,force:true});}
});

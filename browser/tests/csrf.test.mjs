import test from "node:test";
import assert from "node:assert/strict";
import {spawn} from "node:child_process";
import {mkdtemp,readFile,rm,rename,realpath} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join,resolve} from "node:path";
import {fileURLToPath} from "node:url";
import {csrfFixture} from "./csrf-fixture.mjs";
import {CSRF_HEADER,CSRF_MEDIA_TYPE,decodePage,validateExports,createRuntime} from "../../web/bridge.mjs";
const root=fileURLToPath(new URL("../../",import.meta.url)),cli=resolve(process.env.RUVANE_CLI||join(root,"target/debug/ruvane"));
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));

test("CSRF runtime configuration rejects external and ambiguous token URLs before loading Wasm",async()=>{
  for(const csrfEndpoint of ["https://example.test/token","//example.test/token","/token?q=secret","/token#secret","/../token","/bad%2fpath",42]) {
    await assert.rejects(createRuntime("unused.wasm",{csrfEndpoint}));
  }
});

await test("source-free native session CSRF protects actions and native forms",{skip:!(process.platform==="darwin"||process.platform==="linux"&&["x64","arm64"].includes(process.arch))},async t=>{
  const directory=await mkdtemp(join(await realpath(tmpdir()),"ruvane-csrf-"));let child;
  async function stop(){if(!child)return;const current=child;child=null;if(current.exitCode!==null)return;const ended=new Promise(resolve=>current.once("exit",resolve));current.stdin.end();await ended;}
  try {
    const {project,bundle,revision}=await csrfFixture({directory,root,cli});
    const wasm=await readFile(join(bundle,"resources/ruvane-runtime",BigInt(revision).toString(16).padStart(16,"0"),"app.wasm"));
    const module=await WebAssembly.compile(wasm),e=validateExports(new WebAssembly.Instance(module,{}).exports);
    assert.deepEqual(WebAssembly.Module.imports(module),[]);assert.equal(e.ruvane_abi_version(),11);assert.equal(WebAssembly.Module.exports(module).filter(item=>item.kind==="function").length,22);
    assert.equal(wasm.includes(Buffer.from("SERVER_SESSION_DATA_ALPHA48")),false);
    await rename(join(project,"src"),join(project,"source-hidden"));
    const ready=join(directory,"ready");child=spawn(join(bundle,"ruvane-app"),["--ruvane-serve","0",ready],{cwd:bundle,stdio:["pipe","ignore","pipe"]});let errors="";child.stderr.on("data",data=>errors+=data);
    let base;for(let n=0;n<250;n++){const value=await readFile(ready,"utf8").catch(()=>"");if(value){base=`http://${value.trim().split("\t").at(-1)}`;break;}if(child.exitCode!==null)throw Error(errors);await pause(20);}assert.ok(base,"fixture started");
    const get=(path,cookie,extra={})=>fetch(base+path,{headers:{...(cookie?{Cookie:cookie}:{}),...extra},cache:"no-store"});
    const calls=async()=>Number(await(await get("/csrf-stats")).text());
    const post=(mode,cookie,token,extra={},native=false,body)=>fetch(base+(native?"/session-native":"/session-action"),{method:"POST",redirect:"manual",headers:{Origin:base,Accept:"application/vnd.ruvane.action","Content-Type":"application/x-www-form-urlencoded","Ruvane-Application-Revision":revision,...(cookie?{Cookie:cookie}:{}),...(token?{[CSRF_HEADER]:token}:{}),...extra},body:body??`mode=${mode}`});
    const cookieFrom=async(response,status=200)=>{assert.equal(response.status,status);const cookies=response.headers.getSetCookie();assert.equal(cookies.length,1);await response.arrayBuffer();return cookies[0].split(";")[0];};
    const tokenFor=async cookie=>{const response=await get("/csrf-token",cookie,{Accept:CSRF_MEDIA_TYPE});assert.equal(response.status,200);assert.equal(response.headers.get("cache-control"),"no-store");assert.equal(response.headers.get("content-type"),CSRF_MEDIA_TYPE);assert.equal(response.headers.get("content-length"),"0");assert.equal(await response.text(),"");return response.headers.get(CSRF_HEADER);};
    let cookie,token,other,otherToken;
    await t.test("anonymous endpoint is empty and an independent token stays stable across tabs",async()=>{
      assert.equal(await tokenFor(),null);cookie=await cookieFrom(await post("open"));
      [token,otherToken]=await Promise.all([tokenFor(cookie),tokenFor(cookie)]);assert.equal(token,otherToken);assert.match(token,/^[a-f0-9]{64}$/);assert.notEqual(token,cookie.split("=")[1]);
      other=await cookieFrom(await post("open"));otherToken=await tokenFor(other);assert.notEqual(token,otherToken);
    });
    await t.test("missing forged cross-session URL and ambiguous claims never invoke mutation handlers",async()=>{
      const before=await calls();
      for(const claim of [undefined,"forged","f".repeat(64),otherToken,cookie.split("=")[1]]) {const response=await post("rotate",cookie,claim);assert.equal(response.status,403);assert.equal(response.headers.get("set-cookie"),null);await response.arrayBuffer();}
      for(const body of [`mode=rotate&_ruvane_csrf=${token}`,"mode=rotate&_ruvane_csrf=bad"]) {const response=await post("rotate",cookie,token,{},false,body);assert.equal(response.status,403);await response.arrayBuffer();}
      const query=await fetch(base+`/session-native?_ruvane_csrf=${token}`,{method:"POST",redirect:"manual",headers:{Origin:base,Cookie:cookie,"Content-Type":"application/x-www-form-urlencoded"},body:"mode=rotate"});assert.equal(query.status,403);await query.arrayBuffer();
      assert.equal(await calls(),before);
    });
    await t.test("native fields are private and do not enter navigation snapshots or Wasm",async()=>{
      const response=await get("/session?native=yes",cookie);assert.equal(response.headers.get("cache-control"),"no-store");const html=await response.text();
      assert.equal((html.match(new RegExp(`name="_ruvane_csrf" value="${token}"`,"g"))||[]).length,3);assert.ok(html.includes('data-ruvane-csrf="/csrf-token"'));assert.ok(!html.includes(cookie.split("=")[1]));
      const packet=await(await get("/session",cookie,{Accept:"application/vnd.ruvane.page","Ruvane-Application-Revision":revision})).arrayBuffer();decodePage(new Uint8Array(packet));assert.ok(!Buffer.from(packet).includes(Buffer.from(token)));assert.ok(!wasm.includes(Buffer.from(token)));
    });
    await t.test("the built-in guard rejects anonymous reads and allows repeated live POST claims",async()=>{
      for(const [value,status] of [[null,403],[cookie,200]]) {const response=await get("/csrf-protected",value);assert.equal(response.status,status);assert.equal(response.headers.get("cache-control"),"no-store");await response.arrayBuffer();}
      const before=await calls();
      for(const [claim,status] of [[null,403],[otherToken,403],[token,200],[token,200]]) {
        const response=await fetch(base+"/csrf-protected",{method:"POST",headers:{Cookie:cookie,Origin:base,"Content-Type":"application/x-www-form-urlencoded",Accept:"application/vnd.ruvane.action","Ruvane-Application-Revision":revision,...(claim?{[CSRF_HEADER]:claim}:{})},body:"mode=save"});assert.equal(response.status,status);assert.equal(response.headers.get("cache-control"),"no-store");await response.arrayBuffer();
      }
      assert.equal(await calls(),before+2);
    });
    await t.test("existing source MIME revision and duplicate checks still run before mutation",async()=>{
      const before=await calls();
      for(const [extra,status] of [[{Origin:"http://foreign.example"},403],[{"Content-Type":"application/json"},415],[{"Ruvane-Application-Revision":String(BigInt(revision)+1n)},409]]) {const response=await post("rotate",cookie,token,extra);assert.equal(response.status,status);await response.arrayBuffer();}
      const duplicate=await post("rotate",cookie,null,{},true,`mode=rotate&_ruvane_csrf=${token}&%5Fruvane_csrf=${token}`);assert.equal(duplicate.status,400);await duplicate.arrayBuffer();assert.equal(await calls(),before);
    });
    await t.test("rotation invalidates old forms and native 303 accepts only the new generation",async()=>{
      const oldCookie=cookie,oldToken=token;cookie=await cookieFrom(await post("rotate",cookie,token));token=await tokenFor(cookie);assert.notEqual(token,oldToken);
      const before=await calls();for(const current of [oldCookie,cookie]) {const response=await post("rotate",current,null,{},true,`mode=rotate&_ruvane_csrf=${oldToken}`);assert.equal(response.status,403);await response.arrayBuffer();}assert.equal(await calls(),before);
      cookie=await cookieFrom(await post("rotate",cookie,null,{},true,`mode=rotate&_ruvane_csrf=${token}`),303);token=await tokenFor(cookie);
    });
    await t.test("logout revokes claims and another session's token remains usable",async()=>{
      await cookieFrom(await post("close",cookie,token));const before=await calls();const denied=await post("rotate",cookie,token);assert.equal(denied.status,403);await denied.arrayBuffer();assert.equal(await tokenFor(cookie),null);assert.equal(await calls(),before);
      await cookieFrom(await post("close",other,otherToken));
    });
  } finally {await stop();await rm(directory,{recursive:true,force:true});}
});

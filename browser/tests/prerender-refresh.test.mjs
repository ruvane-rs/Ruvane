import test from "node:test";
import assert from "node:assert/strict";
import {execFileSync,spawn} from "node:child_process";
import {mkdtemp,readFile,writeFile,mkdir,rm,rename,realpath} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join,resolve} from "node:path";
import {fileURLToPath} from "node:url";
import {decodePage,decodeNavigationSnapshot,validateExports} from "../../web/bridge.mjs";
const root=fileURLToPath(new URL("../../",import.meta.url));
const cli=resolve(process.env.RUVANE_CLI||join(root,"target/debug/ruvane"));
const run=(args,cwd)=>execFileSync(cli,args,{cwd,encoding:"utf8",timeout:120000,maxBuffer:2*1024*1024,stdio:["ignore","pipe","pipe"]});
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
await test("timed pre-render output refreshes atomically in a source-free production bundle",async t=>{
  const directory=await mkdtemp(join(await realpath(tmpdir()),"ruvane-output-refresh-"));
  let child;
  async function stop(){if(!child)return;const current=child;child=null;if(current.exitCode!==null)return;const ended=new Promise(resolve=>current.once("exit",resolve));current.stdin.end();await ended;}
  try{
    run(["new","refresh-app","--interactive","--file-routes","--framework-path",root],directory);
    const project=join(directory,"refresh-app"),pagesFile=join(project,"src/pages.rs");
    let source=await readFile(pagesFile,"utf8");
    source=source.replace("let mut app = App::new();",`let mut app = App::new();
 app.client_metadata_origin(Some(ruvane::SiteOrigin::parse("https://public.example").unwrap()));
 #[cfg(not(target_family="wasm"))] app.server_cache(output_cache());`);
    source+=`
pub static VERSION:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(2);
pub static LOADS:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
#[cfg(not(target_family="wasm"))]
pub fn output_cache()->ruvane::ServerCache {
 static CACHE:std::sync::OnceLock<ruvane::ServerCache>=std::sync::OnceLock::new();
 CACHE.get_or_init(||ruvane::ServerCache::with_background(
  ruvane::ServerCacheLimits::new(4,2*1024*1024,2,25).unwrap(),
  ruvane::ServerCacheBackgroundLimits::new(1,1).unwrap()).unwrap()).clone()
}
#[cfg(not(target_family="wasm"))]
fn gate()->&'static (std::sync::Mutex<bool>,std::sync::Condvar) {
 static GATE:std::sync::OnceLock<(std::sync::Mutex<bool>,std::sync::Condvar)>=std::sync::OnceLock::new();
 GATE.get_or_init(||(std::sync::Mutex::new(false),std::sync::Condvar::new()))
}
#[cfg(not(target_family="wasm"))]
pub fn block(value:bool){let (state,ready)=gate();*state.lock().unwrap()=value;ready.notify_all();}
#[cfg(not(target_family="wasm"))]
pub fn wait(){let (state,ready)=gate();let guard=state.lock().unwrap();let (_guard,timeout)=ready.wait_timeout_while(guard,std::time::Duration::from_secs(10),|blocked|*blocked).unwrap();assert!(!timeout.timed_out());}
`;
    await writeFile(pagesFile,source);
    async function route(path,source){const directory=join(project,"src/routes",path);await mkdir(directory,{recursive:true});await writeFile(join(directory,"route.rs"),source);}
    await mkdir(join(project,"src/routes/refreshed/[slug]"),{recursive:true});
    await writeFile(join(project,"src/routes/refreshed/[slug]/page.rs"),`use crate::{components,pages};
use ruvane::{ActionData,ActionValue,ClientPageData,FilePage,PageParams,PrerenderRevalidationPolicy};
use std::sync::atomic::Ordering;
pub fn page()->FilePage {
 FilePage::client("refreshed",components::HOME,pages::zero,|context|{
  pages::LOADS.fetch_add(1,Ordering::SeqCst);
  let request=context.request().unwrap();let private=request.header("cookie").is_some()||request.header("authorization").is_some();
  let n=if context.is_exporting(){1}else if private{999}else{pages::VERSION.load(Ordering::SeqCst)};
  #[cfg(not(target_family="wasm"))] if !context.is_exporting()&&!private&&request.query().is_none(){pages::wait();}
  let slug=context.param("slug").unwrap();let mut data=ActionData::default();data.insert("count",ActionValue::Number(0)).unwrap();
  Ok(ClientPageData::new(format!("Version {n} / {slug}"),data).metadata(pages::metadata(context,&format!("Description {n}"))?).fallback(ruvane::text(format!("Fallback {n}"))))
 }).prerender(vec![PageParams::new(&[("slug","a")]).unwrap(),PageParams::new(&[("slug","b")]).unwrap()])
 .revalidation(PrerenderRevalidationPolicy::new().revalidate_after(1).unwrap().stale_while_revalidate(2).unwrap().tag("articles").unwrap())
}
`);
    await route("refresh-control/[mode]",`use std::sync::atomic::Ordering;
pub fn route()->ruvane::FileEndpoint {
 ruvane::FileEndpoint::new().post_action(|request|{
  match request.param("mode") {
   Some("block")=>{crate::pages::block(true);crate::pages::VERSION.fetch_add(1,Ordering::SeqCst);},
   Some("release")=>crate::pages::block(false),
   Some("invalidate")=>{crate::pages::output_cache().invalidate_path("/refreshed/a").unwrap();},
   _=>return Ok(ruvane::ActionReply::validation(ruvane::ActionData::default()))
  }
  Ok(ruvane::ActionReply::success(ruvane::ActionData::default()))
 })
}
`);
    await route("refresh-stats",`pub fn route()->ruvane::FileEndpoint {
 ruvane::FileEndpoint::new().get(|_|{let stats=crate::pages::output_cache().stats().unwrap();
 ruvane::Response::text(format!("{} {} {} {}",crate::pages::LOADS.load(std::sync::atomic::Ordering::SeqCst),stats.active_loads,stats.refresh_attempts,stats.entries))})
}
`);
    run(["build","--offline"],project);
    const bundle=join(project,"dist/build-000001"),manifest=await readFile(join(bundle,"Ruvane.bundle"),"utf8"),revision=manifest.match(/^revision\t([0-9]+)$/m)[1];
    const wasm=await readFile(join(bundle,"resources/ruvane-runtime",BigInt(revision).toString(16).padStart(16,"0"),"app.wasm"));
    const module=await WebAssembly.compile(wasm),e=validateExports(new WebAssembly.Instance(module,{}).exports);
    await t.test("timed declarations preserve the import-free ABI and build revision",()=>{
      assert.deepEqual(WebAssembly.Module.imports(module),[]);assert.equal(e.ruvane_abi_version(),12);
      assert.equal(WebAssembly.Module.exports(module).filter(item=>item.kind==="function").length,22);
      assert.equal(e.ruvane_application_revision(),BigInt(revision));
    });
    await rename(join(project,"src"),join(project,"source-hidden"));
    await rename(join(bundle,"Ruvane.prerender"),join(directory,"audit-hidden"));
    async function start(){
      const ready=join(directory,`ready-${Date.now()}`);
      child=spawn(join(bundle,process.platform==="win32"?"ruvane-app.exe":"ruvane-app"),["--ruvane-serve","0",ready],{cwd:bundle,stdio:["pipe","ignore","pipe"]});
      let errors="";child.stderr.on("data",bytes=>errors+=bytes);
      for(let n=0;n<200;n++){const record=await readFile(ready,"utf8").catch(()=>"");if(record)return `http://${record.trim().split("\t").at(-1)}`;assert.equal(child.exitCode,null,errors);await pause(20);}
      throw Error("server did not start");
    }
    let base=await start();
    const headers={Accept:"application/vnd.ruvane.page","Ruvane-Application-Revision":revision};
    const page=async(path="/refreshed/a",extra={})=>{
      const response=await fetch(base+path,{headers:{...headers,...extra}});assert.equal(response.status,200);assert.equal(response.headers.get("cache-control"),"no-store");
      return decodePage(new Uint8Array(await response.arrayBuffer()));
    };
    const html=async()=>{const response=await fetch(base+"/refreshed/a");assert.equal(response.status,200);assert.equal(response.headers.get("cache-control"),"no-store");return response.text();};
    const stats=async()=>{const [loads,active,attempts,entries]=(await(await fetch(base+"/refresh-stats")).text()).split(" ").map(Number);return {loads,active,attempts,entries};};
    const control=async(mode)=>{const response=await fetch(base+"/refresh-control/"+mode,{method:"POST",headers:{Accept:"application/vnd.ruvane.action","Ruvane-Application-Revision":revision,"Content-Type":"application/x-www-form-urlencoded",Origin:base},body:""});assert.equal(response.status,200);await response.arrayBuffer();};
    const until=async predicate=>{for(let n=0;n<200;n++){if(await predicate())return;await pause(10);}throw Error("refresh did not settle");};
    await t.test("first read ignores embedded data and fresh HTML shares a navigation snapshot",async()=>{
      const source=await html(),snapshot=decodeNavigationSnapshot(new Uint8Array(Buffer.from(source.match(/<div hidden id="ruvane-initial">([0-9a-f]+)<\/div>/)[1],"hex")));
      assert.equal(snapshot.revision,revision);assert.equal(snapshot.page.title,"Version 2 / a");assert.equal((await page()).title,snapshot.page.title);assert.ok(source.includes("Fallback 2"));assert.equal((await stats()).loads,1);
    });
    await t.test("expired output returns immediately while one worker replaces HTML data and metadata",async()=>{
      await control("block");await pause(1100);
      assert.ok((await html()).includes("Description 2"));await until(async()=> (await stats()).loads===2);
      for(let n=0;n<3;n++)assert.equal((await page()).title,"Version 2 / a");assert.equal((await stats()).attempts,1);
      await control("release");await until(async()=> (await stats()).active===0);
      const current=await page();assert.equal(current.title,"Version 3 / a");assert.equal(current.metadata.description,"Description 3");const source=await html();assert.ok(source.includes("Fallback 3"));assert.ok(source.includes("Description 3"));assert.equal((await stats()).loads,2);
    });
    await t.test("explicit invalidation retires a running refresh without publishing it late",async()=>{
      await control("block");await pause(1100);assert.equal((await page()).title,"Version 3 / a");await until(async()=> (await stats()).loads===3);
      await control("invalidate");const retired=await fetch(base+"/refreshed/a",{headers});assert.equal(retired.status,503);assert.equal(retired.headers.get("cache-control"),"no-store");await retired.arrayBuffer();
      await control("release");await until(async()=> (await stats()).active===0);assert.equal((await stats()).entries,0);
      assert.equal((await page()).title,"Version 4 / a");assert.equal((await stats()).loads,4);
    });
    await t.test("hard expiry never adopts the old document and keeps wait and load budgets",async()=>{
      await control("block");await pause(1100);assert.equal((await page()).title,"Version 4 / a");await until(async()=> (await stats()).loads===5);
      await pause(2100);const response=await fetch(base+"/refreshed/a");assert.equal(response.status,503);assert.equal(response.headers.get("cache-control"),"no-store");await response.arrayBuffer();assert.equal((await stats()).loads,5);
      await control("release");await until(async()=> (await stats()).active===0);assert.equal((await page()).title,"Version 5 / a");
    });
    await t.test("credentials queries and unlisted parameters bypass the output refresh policy",async()=>{
      const before=(await stats()).attempts;
      assert.equal((await page("/refreshed/a",{Authorization:"Bearer fixture"})).title,"Version 999 / a");
      assert.equal((await page("/refreshed/a",{Cookie:"fixture=present"})).title,"Version 999 / a");
      assert.equal((await page("/refreshed/a?q=1")).title,"Version 5 / a");assert.equal((await page("/refreshed/c")).title,"Version 5 / c");
      assert.equal((await stats()).attempts,before);assert.equal((await page()).title,"Version 5 / a");
    });
    await stop();base=await start();
    await t.test("restart loads fresh with the same executable and no source or audit file",async()=>{
      assert.equal((await page()).title,"Version 2 / a");assert.equal((await stats()).loads,1);
    });
  } finally {await stop();await rm(directory,{recursive:true,force:true});}
});

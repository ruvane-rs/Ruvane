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
await test("persistent public output survives clean restarts and retires durably in a source-free bundle",{skip:process.platform==="win32"},async t=>{
  const directory=await mkdtemp(join(await realpath(tmpdir()),"ruvane-persistent-output-"));
  let child;
  async function stop(){if(!child)return;const current=child;child=null;if(current.exitCode!==null)return;const ended=new Promise(resolve=>current.once("exit",resolve));current.stdin.end();await ended;}
  async function crash(){const current=child;child=null;const ended=new Promise(resolve=>current.once("exit",resolve));current.kill("SIGKILL");await ended;}
  try{
    run(["new","persistent-app","--interactive","--file-routes","--framework-path",root],directory);
    const project=join(directory,"persistent-app"),pagesFile=join(project,"src/pages.rs"),mainFile=join(project,"src/main.rs");
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
 CACHE.get_or_init(||{
  if let Ok(value)=std::env::var("RUVANE_TEST_DISK_VERSION"){VERSION.store(value.parse().unwrap(),std::sync::atomic::Ordering::SeqCst);}
  let cache=ruvane::ServerCache::default();
  if let Some(path)=std::env::var_os("RUVANE_TEST_DISK_PATH"){
   let bytes=std::env::var("RUVANE_TEST_DISK_BYTES").ok().map(|value|value.parse().unwrap()).unwrap_or(8*1024*1024);
   let limits=ruvane::ServerCacheDiskLimits::new(4,bytes).unwrap();
   let disk=if std::env::var_os("RUVANE_TEST_DISK_RECOVER").is_some(){ruvane::ServerCachePersistence::recover_empty(path,"fixture",limits)}else{ruvane::ServerCachePersistence::open(path,"fixture",limits)}.unwrap();
   cache.persistent(disk).unwrap()
  }else{cache}
 }).clone()
}
`;
    await writeFile(pagesFile,source);
    const main=await readFile(mainFile,"utf8");assert.ok(main.includes("    server.run()?;"));
    await writeFile(mainFile,main.replace("    server.run()?;","    server.run()?;\n    pages::output_cache().close_persistence()?;"));
    async function route(path,source){const directory=join(project,"src/routes",path);await mkdir(directory,{recursive:true});await writeFile(join(directory,"route.rs"),source);}
    await mkdir(join(project,"src/routes/persisted/[slug]"),{recursive:true});
    await writeFile(join(project,"src/routes/persisted/[slug]/page.rs"),`use crate::{components,pages};
use ruvane::{ActionData,ActionValue,ClientPageData,FilePage,PageParams,PrerenderRevalidationPolicy};
use std::sync::atomic::Ordering;
pub fn page()->FilePage {
 FilePage::client("persisted",components::HOME,pages::zero,|context|{
  pages::LOADS.fetch_add(1,Ordering::SeqCst);let request=context.request().unwrap();
  let private=request.header("cookie").is_some()||request.header("authorization").is_some();
  let n=if context.is_exporting(){1}else if private{999}else{pages::VERSION.load(Ordering::SeqCst)};
  let slug=context.param("slug").unwrap();let mut data=ActionData::default();data.insert("count",ActionValue::Number(0)).unwrap();
  Ok(ClientPageData::new(format!("Version {n} / {slug}"),data).metadata(pages::metadata(context,&format!("Description {n}"))?).fallback(ruvane::text(format!("Fallback {n}"))))
 }).prerender(vec![PageParams::new(&[("slug","a")]).unwrap(),PageParams::new(&[("slug","b")]).unwrap()])
 .revalidation(PrerenderRevalidationPolicy::new().tag("articles").unwrap())
}
`);
    await route("persistent-control/[mode]",`use std::sync::atomic::Ordering;
pub fn route()->ruvane::FileEndpoint {
 ruvane::FileEndpoint::new().post_action(|request|{
  match request.param("mode") {
   Some("path")=>{crate::pages::VERSION.fetch_add(1,Ordering::SeqCst);crate::pages::output_cache().invalidate_path("/persisted/a").unwrap();},
   Some("tag")=>{crate::pages::VERSION.fetch_add(1,Ordering::SeqCst);crate::pages::output_cache().invalidate_tag("articles").unwrap();},
   Some("clear")=>{crate::pages::VERSION.fetch_add(1,Ordering::SeqCst);crate::pages::output_cache().clear().unwrap();},
   _=>return Ok(ruvane::ActionReply::validation(ruvane::ActionData::default()))
  }
  Ok(ruvane::ActionReply::success(ruvane::ActionData::default()))
 })
}
`);
    await route("persistent-stats",`pub fn route()->ruvane::FileEndpoint {
 ruvane::FileEndpoint::new().get(|_|{let stats=crate::pages::output_cache().stats().unwrap();
 ruvane::Response::text(format!("{} {} {} {}",crate::pages::LOADS.load(std::sync::atomic::Ordering::SeqCst),stats.persistent_restores,stats.persistent_entries,stats.entries))})
}
`);
    run(["build","--offline"],project);
    const bundle=join(project,"dist/build-000001"),manifest=await readFile(join(bundle,"Ruvane.bundle"),"utf8"),revision=manifest.match(/^revision\t([0-9]+)$/m)[1];
    const wasm=await readFile(join(bundle,"resources/ruvane-runtime",BigInt(revision).toString(16).padStart(16,"0"),"app.wasm"));
    const module=await WebAssembly.compile(wasm),e=validateExports(new WebAssembly.Instance(module,{}).exports);
    await t.test("native-only persistence preserves the import-free Wasm ABI and revision",()=>{
      assert.deepEqual(WebAssembly.Module.imports(module),[]);assert.equal(e.ruvane_abi_version(),11);
      assert.equal(WebAssembly.Module.exports(module).filter(item=>item.kind==="function").length,22);assert.equal(e.ruvane_application_revision(),BigInt(revision));
    });
    await rename(join(project,"src"),join(project,"source-hidden"));
    await rename(join(bundle,"Ruvane.prerender"),join(directory,"audit-hidden"));
    const disk=join(directory,"output-cache");
    let next=0;
    async function start(version,{recover=false,bytes=8*1024*1024,fail=false}={}){
      const ready=join(directory,`ready-${next++}`),env={...process.env,RUVANE_TEST_DISK_PATH:disk,RUVANE_TEST_DISK_VERSION:String(version),RUVANE_TEST_DISK_BYTES:String(bytes)};
      delete env.RUVANE_TEST_DISK_RECOVER;if(recover)env.RUVANE_TEST_DISK_RECOVER="1";
      child=spawn(join(bundle,process.platform==="win32"?"ruvane-app.exe":"ruvane-app"),["--ruvane-serve","0",ready],{cwd:bundle,env,stdio:["pipe","ignore","pipe"]});
      let errors="";child.stderr.on("data",data=>errors+=data);
      for(let n=0;n<250;n++){
        const record=await readFile(ready,"utf8").catch(()=>"");if(record){assert.equal(fail,false);return `http://${record.trim().split("\t").at(-1)}`;}
        if(child.exitCode!==null){if(fail){assert.match(errors,/PersistenceBusy/);child=null;return;}throw Error(errors);}
        await pause(20);
      }
      throw Error("server did not start");
    }
    let base=await start(2);
    const headers={Accept:"application/vnd.ruvane.page","Ruvane-Application-Revision":revision};
    const page=async(path="/persisted/a",extra={})=>{const response=await fetch(base+path,{headers:{...headers,...extra}});assert.equal(response.status,200);assert.equal(response.headers.get("cache-control"),"no-store");return decodePage(new Uint8Array(await response.arrayBuffer()));};
    const html=async()=>{const response=await fetch(base+"/persisted/a");assert.equal(response.status,200);assert.equal(response.headers.get("cache-control"),"no-store");return response.text();};
    const stats=async()=>{const [loads,restores,disk,memory]=(await(await fetch(base+"/persistent-stats")).text()).split(" ").map(Number);return {loads,restores,disk,memory};};
    const invalidate=async(mode)=>{const response=await fetch(base+"/persistent-control/"+mode,{method:"POST",headers:{Accept:"application/vnd.ruvane.action","Ruvane-Application-Revision":revision,"Content-Type":"application/x-www-form-urlencoded",Origin:base},body:""});assert.equal(response.status,200);await response.arrayBuffer();};
    let original;
    await t.test("first generation persists matching HTML data metadata and hydration",async()=>{
      original=await html();const snapshot=decodeNavigationSnapshot(new Uint8Array(Buffer.from(original.match(/<div hidden id="ruvane-initial">([0-9a-f]+)<\/div>/)[1],"hex")));
      assert.equal(snapshot.page.title,"Version 2 / a");assert.equal(snapshot.revision,revision);assert.equal((await page()).title,snapshot.page.title);assert.ok(original.includes("Fallback 2"));assert.equal((await stats()).disk,1);
    });
    await stop();base=await start(9);
    await t.test("clean restart restores the full document without executing loaders",async()=>{
      assert.equal(await html(),original);assert.equal((await page()).title,"Version 2 / a");assert.deepEqual(await stats(),{loads:0,restores:1,disk:1,memory:1});
      assert.equal((await page("/persisted/a?q=current")).title,"Version 9 / a");
    });
    await stop();base=await start(10);
    await t.test("invalidation before restoration remains effective after another restart",async()=>{
      assert.equal((await stats()).memory,0);await invalidate("tag");assert.equal((await stats()).disk,0);
      await stop();base=await start(11);assert.equal((await page()).title,"Version 11 / a");assert.equal((await stats()).restores,0);
    });
    await t.test("acknowledged invalidation survives a killed owner and explicit empty recovery",async()=>{
      await invalidate("path");await crash();await start(12,{fail:true});
      base=await start(13,{recover:true});assert.equal((await page()).title,"Version 13 / a");assert.equal((await stats()).restores,0);
    });
    await t.test("private query and unlisted requests do not enter the persistent catalog",async()=>{
      const before=await stats();assert.equal((await page("/persisted/a",{Cookie:"fixture=present"})).title,"Version 999 / a");assert.equal((await page("/persisted/a",{Authorization:"Bearer fixture"})).title,"Version 999 / a");
      assert.equal((await page("/persisted/a?q=1")).title,"Version 13 / a");assert.equal((await page("/persisted/c")).title,"Version 13 / c");
      assert.equal((await stats()).disk,before.disk);assert.equal((await stats()).restores,before.restores);
      assert.equal((await readFile(join(disk,"store.bin"))).includes(Buffer.from("Version 999")),false);
    });
    await stop();await writeFile(join(disk,"store.bin"),"corrupt catalog");base=await start(14);
    await t.test("corruption triggers fresh generation rather than stale or build fallback",async()=>{
      assert.equal((await page()).title,"Version 14 / a");assert.equal((await stats()).loads,1);assert.equal((await stats()).restores,0);
    });
    await stop();base=await start(15,{bytes:32});
    await t.test("bounded disk storage can decline outputs without retaining an old generation",async()=>{
      assert.equal((await page()).title,"Version 15 / a");assert.equal((await stats()).disk,0);
      await stop();base=await start(16);assert.equal((await page()).title,"Version 16 / a");assert.equal((await stats()).restores,0);
    });
  } finally {await stop();await rm(directory,{recursive:true,force:true});}
});

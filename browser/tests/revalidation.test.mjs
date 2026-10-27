import test from "node:test";
import assert from "node:assert/strict";
import {execFileSync,spawn} from "node:child_process";
import {mkdtemp,readFile,writeFile,mkdir,rm,rename,realpath} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join,resolve} from "node:path";
import {fileURLToPath} from "node:url";
import {decodePage,validateExports} from "../../web/bridge.mjs";
const root=fileURLToPath(new URL("../../",import.meta.url));
const cli=resolve(process.env.RUVANE_CLI||join(root,"target/debug/ruvane"));
const run=(args,cwd)=>execFileSync(cli,args,{cwd,encoding:"utf8",timeout:120000,maxBuffer:2*1024*1024,stdio:["ignore","pipe","pipe"]});
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
await test("regenerable file pages ship unchanged Wasm and reload public output after invalidation",async t=>{
  const directory=await mkdtemp(join(await realpath(tmpdir()),"ruvane-revalidation-"));
  let child;
  async function stop(){if(!child)return;const current=child;child=null;if(current.exitCode!==null)return;const ended=new Promise(resolve=>current.once("exit",resolve));current.stdin.end();await ended;}
  try{
    run(["new","regenerated-app","--interactive","--file-routes","--framework-path",root],directory);
    const project=join(directory,"regenerated-app"),pagesFile=join(project,"src/pages.rs");
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
 CACHE.get_or_init(ruvane::ServerCache::default).clone()
}
`;
    await writeFile(pagesFile,source);
    await mkdir(join(project,"src/routes/regenerated/[slug]"),{recursive:true});
    await writeFile(join(project,"src/routes/regenerated/[slug]/page.rs"),`use crate::{components,pages};
use ruvane::{ActionData,ActionValue,ClientPageData,FilePage,PageParams,PrerenderRevalidationPolicy};
use std::sync::atomic::Ordering;
pub fn page()->FilePage {
 FilePage::client("regenerated",components::HOME,pages::zero,|context|{
  pages::LOADS.fetch_add(1,Ordering::SeqCst);
  let private=context.request().unwrap().header("cookie").is_some()||context.request().unwrap().header("authorization").is_some();
  let n=if context.is_exporting(){1}else if private{999}else{pages::VERSION.load(Ordering::SeqCst)};
  let slug=context.param("slug").unwrap();let mut data=ActionData::default();data.insert("count",ActionValue::Number(0)).unwrap();
  Ok(ClientPageData::new(format!("Version {n} / {slug}"),data).metadata(pages::metadata(context,&format!("Description {n}"))?).fallback(ruvane::text(format!("Fallback {n}"))))
 }).prerender(vec![PageParams::new(&[("slug","a")]).unwrap(),PageParams::new(&[("slug","b")]).unwrap()])
 .revalidation(PrerenderRevalidationPolicy::new().tag("articles").unwrap())
}
`);
    await mkdir(join(project,"src/routes/regenerate"),{recursive:true});
    await writeFile(join(project,"src/routes/regenerate/route.rs"),`use std::sync::atomic::Ordering;
pub fn route()->ruvane::FileEndpoint {
 ruvane::FileEndpoint::new().post_action(|_|{
  crate::pages::VERSION.fetch_add(1,Ordering::SeqCst);
  crate::pages::output_cache().invalidate_tag("articles").unwrap();
  Ok(ruvane::ActionReply::success(ruvane::ActionData::default()))
 })
}
`);
    await mkdir(join(project,"src/routes/output-stats"),{recursive:true});
    await writeFile(join(project,"src/routes/output-stats/route.rs"),`pub fn route()->ruvane::FileEndpoint {
 ruvane::FileEndpoint::new().get(|_|ruvane::Response::text(crate::pages::LOADS.load(std::sync::atomic::Ordering::SeqCst).to_string()))
}
`);
    run(["build","--offline"],project);
    const bundle=join(project,"dist/build-000001"),manifest=await readFile(join(bundle,"Ruvane.bundle"),"utf8");
    const revision=manifest.match(/^revision\t([0-9]+)$/m)[1];
    const wasm=await readFile(join(bundle,"resources/ruvane-runtime",BigInt(revision).toString(16).padStart(16,"0"),"app.wasm"));
    const module=await WebAssembly.compile(wasm),e=validateExports(new WebAssembly.Instance(module,{}).exports);
    await t.test("shared policy declarations preserve import-free ABI v12 and 22 exports",()=>{
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
      for(let n=0;n<200;n++){
        const record=await readFile(ready,"utf8").catch(()=>"");
        if(record)return `http://${record.trim().split("\t").at(-1)}`;
        assert.equal(child.exitCode,null,errors);await pause(20);
      }
      throw Error("server did not start");
    }
    let base=await start();
    const headers={Accept:"application/vnd.ruvane.page","Ruvane-Application-Revision":revision};
    const page=async(path,extra={})=>{const response=await fetch(base+path,{headers:{...headers,...extra}});assert.equal(response.status,200);assert.equal(response.headers.get("cache-control"),"no-store");return decodePage(new Uint8Array(await response.arrayBuffer()));};
    const loads=async()=>Number(await(await fetch(base+"/output-stats")).text());
    await t.test("first runtime read ignores the embedded build seed and shares HTML with navigation",async()=>{
      const html=await(await fetch(base+"/regenerated/a")).text();assert.ok(html.includes("Description 2"));assert.ok(html.includes("Fallback 2"));assert.equal((await page("/regenerated/a")).title,"Version 2 / a");assert.equal(await loads(),1);
      const snapshot=Buffer.from(html.match(/<div hidden id="ruvane-initial">([0-9a-f]+)<\/div>/)[1],"hex");
      assert.equal(snapshot.subarray(0,4).toString(),"RVI1");assert.equal(snapshot.readBigUInt64LE(4),BigInt(revision));
    });
    await t.test("typed mutation invalidates output and the next navigation regenerates metadata",async()=>{
      const response=await fetch(base+"/regenerate",{method:"POST",headers:{Accept:"application/vnd.ruvane.action","Ruvane-Application-Revision":revision,"Content-Type":"application/x-www-form-urlencoded",Origin:base},body:""});assert.equal(response.status,200);await response.arrayBuffer();
      const current=await page("/regenerated/a");assert.equal(current.title,"Version 3 / a");assert.equal(current.metadata.description,"Description 3");assert.equal(await loads(),2);
      const html=await(await fetch(base+"/regenerated/a")).text();assert.ok(html.includes("Fallback 3"));assert.equal(await loads(),2);
    });
    await t.test("credentials queries and unlisted parameters bypass output",async()=>{
      for(const extra of [{Cookie:"fixture=present"},{Authorization:"Bearer fixture"}])assert.equal((await page("/regenerated/a",extra)).title,"Version 999 / a");
      for(const path of ["/regenerated/a?q=1","/regenerated/c"])assert.ok((await page(path)).title.startsWith("Version 3"));
      assert.equal((await page("/regenerated/a")).title,"Version 3 / a");assert.equal(await loads(),6);
    });
    await stop();base=await start();
    await t.test("source-free restart reloads instead of reviving the embedded build output",async()=>{
      assert.equal((await page("/regenerated/a")).title,"Version 2 / a");assert.equal(await loads(),1);
    });
  } finally {await stop();await rm(directory,{recursive:true,force:true});}
});

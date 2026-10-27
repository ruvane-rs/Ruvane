import test from "node:test";
import assert from "node:assert/strict";
import {execFileSync,spawn} from "node:child_process";
import {mkdtemp,readFile,writeFile,mkdir,rm,rename,realpath} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join,resolve} from "node:path";
import {fileURLToPath} from "node:url";
import {decodePage,decodeCommands,decodeNavigationSnapshot,validateExports} from "../../web/bridge.mjs";
const root=fileURLToPath(new URL("../../",import.meta.url));
const cli=resolve(process.env.RUVANE_CLI||join(root,"target/debug/ruvane"));
const run=(args,cwd)=>execFileSync(cli,args,{cwd,encoding:"utf8",timeout:120000,maxBuffer:2*1024*1024,stdio:["ignore","pipe","pipe"]});
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
function upload(e,bytes){assert.equal(e.ruvane_page_begin(bytes.length),1);for(let offset=0;offset<bytes.length;offset+=4){let word=0;const size=Math.min(4,bytes.length-offset);for(let n=0;n<size;n++)word|=bytes[offset+n]<<(n*8);assert.equal(e.ruvane_page_chunk(word>>>0,size),1);}}
await test("native file guards protect cached pages and actions with typed read-only browser errors",{skip:process.platform==="win32"},async t=>{
  const directory=await mkdtemp(join(await realpath(tmpdir()),"ruvane-guards-"));
  let child;
  async function stop(){if(!child)return;const current=child;child=null;if(current.exitCode!==null)return;const ended=new Promise(resolve=>current.once("exit",resolve));current.stdin.end();await ended;}
  try{
    run(["new","guarded-app","--interactive","--file-routes","--framework-path",root],directory);
    const project=join(directory,"guarded-app"),pagesFile=join(project,"src/pages.rs"),mainFile=join(project,"src/main.rs");
    let source=await readFile(pagesFile,"utf8");
    source=source.replace("let mut app = App::new();",`let mut app = App::new();
 app.client_metadata_origin(Some(ruvane::SiteOrigin::parse("https://public.example").unwrap()));
 #[cfg(not(target_family="wasm"))] app.server_cache(output_cache());`);
    source=source.replace('    element(Tag::Main)\n        .child(header)', '    header = header.child(ruvane::navigation_link(&InternalUrl::new("/protected/a").unwrap(), "Guarded page"));\n    element(Tag::Main)\n        .child(header)');
    source+=`
#[cfg(not(target_family="wasm"))]
pub static GUARD_MODE:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
#[cfg(not(target_family="wasm"))]
pub static GUARD_CALLS:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
#[cfg(not(target_family="wasm"))]
pub static WRITES:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
pub static LOADS:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
#[cfg(not(target_family="wasm"))]
pub fn output_cache()->ruvane::ServerCache {
 static CACHE:std::sync::OnceLock<ruvane::ServerCache>=std::sync::OnceLock::new();
 CACHE.get_or_init(||{
  if let Ok(value)=std::env::var("RUVANE_TEST_GUARD_MODE"){GUARD_MODE.store(value.parse().unwrap(),std::sync::atomic::Ordering::SeqCst);}
  let cache=ruvane::ServerCache::default();
  if let Some(path)=std::env::var_os("RUVANE_TEST_GUARD_DISK"){
   cache.persistent(ruvane::ServerCachePersistence::open(path,"guard-fixture",Default::default()).unwrap()).unwrap()
  }else{cache}
 }).clone()
}
pub struct Protected {value:String}
impl ruvane::Component for Protected {
 fn page_data(&mut self,data:ruvane::ActionData)->Result<(),ruvane::ComponentError>{self.value=data.text("value").ok_or(ruvane::ComponentError::InvalidProps)?.into();Ok(())}
 fn view(&self)->Result<ruvane::View,ruvane::ComponentError>{let mut view=ruvane::View::new();view.leaf(0,ruvane::BrowserTag::Heading,&self.value)?;Ok(view)}
 fn dispatch(&mut self,_:u32)->Result<(),ruvane::ComponentError>{Err(ruvane::ComponentError::InvalidEvent)}
}
pub fn protected_component()->Box<dyn ruvane::Component>{Box::new(Protected{value:String::new()})}
`;
    await writeFile(pagesFile,source);
    const componentFile=join(project,"src/components.rs");
    let components=await readFile(componentFile,"utf8");
    const factory="pub fn create_component(props: ComponentProps) -> Result<Box<dyn Component>, ComponentError> {";
    assert.ok(components.includes(factory));components=components.replace(factory,factory+'\n    if props.component()==22 && props.state()==InitialState::Number(0) {return Ok(crate::pages::protected_component());}');
    await writeFile(componentFile,components);
    const main=await readFile(mainFile,"utf8");assert.ok(main.includes("    server.run()?;"));
    await writeFile(mainFile,main.replace("    server.run()?;","    server.run()?;\n    pages::output_cache().close_persistence()?;"));
    async function file(path,body){const destination=join(project,"src/routes",path);await mkdir(join(destination,".."),{recursive:true});await writeFile(destination,body);}
    await file("protected/guard.rs",`#[cfg(target_family="wasm")] compile_error!("guard.rs reached Wasm");
pub fn guard()->ruvane::RouteGuard {
 ruvane::RouteGuard::new(|context|{
  let _server_only="SERVER_ONLY_GUARD_ALPHA46";
  crate::pages::GUARD_CALLS.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
  assert!(context.request().header("host").is_some(),"guard ran during build");
  match crate::pages::GUARD_MODE.load(std::sync::atomic::Ordering::SeqCst) {
   0=>ruvane::GuardDecision::Allow,1=>ruvane::GuardDecision::Private,
   2=>ruvane::GuardDecision::Forbidden,3=>ruvane::GuardDecision::Unavailable,
   _=>panic!("SERVER_ONLY_GUARD_ALPHA46"),
  }
 })
}
`);
    await file("protected/layout.rs",`pub fn layout()->ruvane::FileLayout {ruvane::FileLayout::client("protected-layout",crate::components::WORKSPACE,crate::pages::zero,|_|Ok(ruvane::ClientLayoutData::new(Default::default())))}\n`);
    await file("protected/[slug]/page.rs",`use ruvane::{ActionData,ActionValue,ClientPageData,FilePage,PageParams,PrerenderRevalidationPolicy};
pub fn page()->FilePage {
 FilePage::client("protected",22,crate::pages::zero,|context|{
  crate::pages::LOADS.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
  let value=if context.is_exporting(){"Build public"}else if context.request().unwrap().cache_bypass(){"Private live"}else{"Public cached"};
  let mut data=ActionData::default();data.insert("value",ActionValue::Text(value.into())).unwrap();
  Ok(ClientPageData::new(value,data).metadata(crate::pages::metadata(context,value)?).fallback(ruvane::text(value)))
 }).prerender(vec![PageParams::new(&[("slug","a")]).unwrap()])
 .revalidation(PrerenderRevalidationPolicy::new().tag("guarded").unwrap())
}
`);
    await file("protected/write/route.rs",`pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().post_action(|_|{crate::pages::WRITES.fetch_add(1,std::sync::atomic::Ordering::SeqCst);Ok(ruvane::ActionReply::success(Default::default()))})}\n`);
    await file("guard-control/[mode]/route.rs",`pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().post_action(|request|{crate::pages::GUARD_MODE.store(request.param("mode").unwrap().parse().unwrap(),std::sync::atomic::Ordering::SeqCst);Ok(ruvane::ActionReply::success(Default::default()))})}\n`);
    await file("guard-stats/route.rs",`pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().get(|_|{let stats=crate::pages::output_cache().stats().unwrap();ruvane::Response::text(format!("{} {} {} {} {}",crate::pages::GUARD_CALLS.load(std::sync::atomic::Ordering::SeqCst),crate::pages::LOADS.load(std::sync::atomic::Ordering::SeqCst),crate::pages::WRITES.load(std::sync::atomic::Ordering::SeqCst),stats.persistent_restores,stats.entries))})}\n`);
    run(["build","--offline"],project);
    const bundle=join(project,"dist/build-000001"),manifest=await readFile(join(bundle,"Ruvane.bundle"),"utf8"),revision=manifest.match(/^revision\t([0-9]+)$/m)[1];
    const wasm=await readFile(join(bundle,"resources/ruvane-runtime",BigInt(revision).toString(16).padStart(16,"0"),"app.wasm"));
    const module=await WebAssembly.compile(wasm),e=validateExports(new WebAssembly.Instance(module,{}).exports);
    await t.test("guard modules are absent from import-free Wasm with the unchanged ABI",()=>{
      assert.equal(wasm.includes(Buffer.from("SERVER_ONLY_GUARD_ALPHA46")),false);
      assert.deepEqual(WebAssembly.Module.imports(module),[]);assert.equal(e.ruvane_abi_version(),14);
      assert.equal(WebAssembly.Module.exports(module).filter(item=>item.kind==="function").length,22);
    });
    await rename(join(project,"src"),join(project,"source-hidden"));
    await rename(join(bundle,"Ruvane.prerender"),join(directory,"audit-hidden"));
    let next=0;
    async function start(mode=0){
      const ready=join(directory,`ready-${next++}`);
      child=spawn(join(bundle,"ruvane-app"),["--ruvane-serve","0",ready],{cwd:bundle,env:{...process.env,RUVANE_TEST_GUARD_DISK:join(directory,"cache"),RUVANE_TEST_GUARD_MODE:String(mode)},stdio:["pipe","ignore","pipe"]});
      let errors="";child.stderr.on("data",data=>errors+=data);
      for(let n=0;n<250;n++){const record=await readFile(ready,"utf8").catch(()=>"");if(record)return `http://${record.trim().split("\t").at(-1)}`;if(child.exitCode!==null)throw Error(errors);await pause(20);}
      throw Error("guard fixture did not start");
    }
    let base=await start();
    const headers={Accept:"application/vnd.ruvane.page","Ruvane-Application-Revision":revision};
    const page=async(path="/protected/a",extra={})=>{const response=await fetch(base+path,{headers:{...headers,...extra}});return {response,bytes:new Uint8Array(await response.arrayBuffer())};};
    const stats=async()=>{const [guards,loads,writes,restores,entries]=(await(await fetch(base+"/guard-stats")).text()).split(" ").map(Number);return {guards,loads,writes,restores,entries};};
    const post=async(path,extra={})=>fetch(base+path,{method:"POST",headers:{Accept:"application/vnd.ruvane.action","Ruvane-Application-Revision":revision,"Content-Type":"application/x-www-form-urlencoded",Origin:base,...extra},body:""});
    const mode=async value=>{const response=await post("/guard-control/"+value);assert.equal(response.status,200);await response.arrayBuffer();};
    await t.test("Allow checks every request while sharing public native output",async()=>{
      for(let n=0;n<2;n++){const {response,bytes}=await page();assert.equal(response.status,200);assert.equal(response.headers.get("cache-control"),"no-store");assert.equal(decodePage(bytes).title,"Public cached");}
      assert.deepEqual(await stats(),{guards:2,loads:1,writes:0,restores:0,entries:1});
    });
    await t.test("Private skips public output without overwriting the warm entry",async()=>{
      await mode(1);assert.equal(decodePage((await page()).bytes).title,"Private live");
      await mode(0);assert.equal(decodePage((await page()).bytes).title,"Public cached");
      assert.equal((await stats()).loads,2);
    });
    await t.test("denials and panic use closed native and Wasm read-only root errors",async()=>{
      for(const [decision,status,title] of [[2,403,"Access denied"],[3,503,"Service unavailable"],[4,500,"Page unavailable"]]){
        await mode(decision);const before=await stats();const {response,bytes}=await page();
        assert.equal(response.status,status);assert.equal(response.headers.get("cache-control"),"no-store");
        const decoded=decodePage(bytes);assert.equal(decoded.title,title);assert.equal(decoded.component,4294967295);assert.deepEqual(decoded.layouts,[]);
        upload(e,bytes);assert.equal(e.ruvane_page_validate(),1);assert.equal(e.ruvane_live_instances(),0);
        upload(e,bytes);const owner=e.ruvane_page_mount(1);assert.ok(owner);
        const commands=decodeCommands(new Uint8Array(e.memory.buffer,e.ruvane_commands_ptr(),e.ruvane_commands_len()).slice());
        assert.ok(JSON.stringify(commands).includes(title));e.ruvane_commands_clear();
        assert.equal(e.ruvane_dispatch(owner,1,1,0,0),0);e.ruvane_commands_clear();assert.equal(e.ruvane_dispose(owner),1);e.ruvane_commands_clear();
        const htmlResponse=await fetch(base+"/protected/a");assert.equal(htmlResponse.status,status);const html=await htmlResponse.text();
        assert.ok(!html.includes("Public cached")&&!html.includes("Private live")&&!html.includes("SERVER_ONLY_GUARD_ALPHA46"));
        assert.ok(html.includes("noindex"));
        const snapshot=decodeNavigationSnapshot(new Uint8Array(Buffer.from(html.match(/id="ruvane-initial">([0-9a-f]+)</)[1],"hex")));
        assert.equal(snapshot.page.component,4294967295);assert.equal((await stats()).loads,before.loads);
      }
    });
    await t.test("bad POST origin MIME and revision fail before guards and writes",async()=>{
      await mode(2);const before=await stats();
      for(const [extra,status] of [[{Origin:"http://foreign.example"},403],[{"Content-Type":"application/json"},415],[{"Ruvane-Application-Revision":String(BigInt(revision)+1n)},409]]){
        const response=await post("/protected/write",extra);assert.equal(response.status,status);await response.arrayBuffer();
      }
      assert.deepEqual(await stats(),before);
      const response=await post("/protected/write");assert.equal(response.status,403);await response.arrayBuffer();assert.equal((await stats()).writes,0);assert.equal((await stats()).guards,before.guards+1);
      await mode(0);const allowed=await post("/protected/write");assert.equal(allowed.status,200);await allowed.arrayBuffer();assert.equal((await stats()).writes,1);
    });
    await t.test("inherited guards cover unmatched descendants and percent aliases but leave public assets available",async()=>{
      await mode(2);for(const path of ["/protected/missing/deep","/%70rotected/a"]){const {response}=await page(path);assert.equal(response.status,403);}
      const head=await fetch(base+"/protected/a",{method:"HEAD"});assert.equal(head.status,403);assert.equal((await head.arrayBuffer()).byteLength,0);
      assert.equal((await fetch(base+"/assets/site.css")).status,200);assert.equal((await fetch(base+"/api/health")).status,200);
    });
    await stop();base=await start(2);
    await t.test("restarted denial and Private run before persistent restoration",async()=>{
      assert.equal((await page()).response.status,403);assert.deepEqual(await stats(),{guards:1,loads:0,writes:0,restores:0,entries:0});
      await mode(1);assert.equal(decodePage((await page()).bytes).title,"Private live");assert.equal((await stats()).restores,0);
      await mode(0);assert.equal(decodePage((await page()).bytes).title,"Public cached");assert.equal((await stats()).restores,1);assert.equal((await stats()).loads,1);
    });
    if(process.env.RUVANE_KEEP_GUARD_FIXTURE)await writeFile(process.env.RUVANE_KEEP_GUARD_FIXTURE,JSON.stringify({directory,project,bundle,revision}));
  }finally{await stop();if(!process.env.RUVANE_KEEP_GUARD_FIXTURE)await rm(directory,{recursive:true,force:true});}
});

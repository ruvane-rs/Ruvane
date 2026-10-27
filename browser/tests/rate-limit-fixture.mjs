import {readFile,writeFile,mkdir} from "node:fs/promises";
import {join} from "node:path";
import {csrfFixture} from "./csrf-fixture.mjs";

export async function rateLimitFixture(options) {
  return csrfFixture({...options,prepare:async project=>{
    async function file(path,source){const target=join(project,"src/routes",path);await mkdir(join(target,".."),{recursive:true});await writeFile(target,source);}
    const lib=join(project,"src/lib.rs");await writeFile(lib,(await readFile(lib,"utf8"))+'\n#[cfg(not(target_family="wasm"))]\npub mod rate_server;\n');
    const server=join(project,"src/session_server.rs");await writeFile(server,(await readFile(server,"utf8")).replace("ruvane_alpha49_session","ruvane_alpha50_session"));
    await writeFile(join(project,"src/rate_server.rs"),`
#[cfg(target_family="wasm")] compile_error!("SERVER_RATE_LIMIT_ALPHA50 must stay native");
use ruvane::{RateLimiter,RateLimitPolicy,RateLimitLimits};
use std::sync::{OnceLock,atomic::AtomicUsize};
pub static LOADS:AtomicUsize=AtomicUsize::new(0);
pub fn mutations()->&'static RateLimiter {
 static LIMITS:OnceLock<RateLimiter>=OnceLock::new();
 LIMITS.get_or_init(||RateLimiter::new(RateLimitPolicy::new(2,60).unwrap(),RateLimitLimits::new(64,32768).unwrap()))
}
pub fn pages()->&'static RateLimiter {
 static LIMITS:OnceLock<RateLimiter>=OnceLock::new();
 LIMITS.get_or_init(||RateLimiter::new(RateLimitPolicy::new(2,2).unwrap(),RateLimitLimits::default()))
}
pub fn capacity()->&'static RateLimiter {
 static LIMITS:OnceLock<RateLimiter>=OnceLock::new();
 LIMITS.get_or_init(||RateLimiter::new(RateLimitPolicy::new(1,60).unwrap(),RateLimitLimits::new(1,4096).unwrap()))
}
`);
    const pagesFile=join(project,"src/pages.rs");let pages=await readFile(pagesFile,"utf8");
    pages=pages.replace('    let mut app = App::new();',`    let mut app = App::new();
    #[cfg(not(target_family="wasm"))] {
     for path in ["/session-action","/session-native"] {app.guard_branch(path,crate::session_server::mutation_guard()).unwrap();}
     app.guard_branch("/rate-unknown",crate::rate_server::pages().guard()).unwrap();
    }`);
    pages+=`
pub struct RateView {message:String,pending:Option<u32>,outgoing:Option<ruvane::ActionRequest>}
impl ruvane::Component for RateView {
 fn page_data(&mut self,data:ruvane::ActionData)->Result<(),ruvane::ComponentError>{
  let message=data.text("message").filter(|value|matches!(*value,"Ready"|"Session active"|"Anonymous")).ok_or(ruvane::ComponentError::InvalidProps)?;
  if data.len()!=1{return Err(ruvane::ComponentError::InvalidProps);}self.message=message.into();Ok(())
 }
 fn view(&self)->Result<ruvane::View,ruvane::ComponentError>{
  let mut view=ruvane::View::new();let root=view.keyed_element(0,1,ruvane::BrowserTag::Section)?;
  view.keyed_leaf(root,1,ruvane::BrowserTag::Heading,"Quota controls")?;
  view.keyed_leaf(root,2,ruvane::BrowserTag::Output,&self.message)?;
  view.keyed_button(root,3,"Run limited action",1,self.pending.is_some())?;
  view.keyed_button(root,4,"Run manual 429",2,self.pending.is_some())?;
  Ok(view)
 }
 fn dispatch(&mut self,event:u32)->Result<(),ruvane::ComponentError>{
  if self.pending.is_some()||!matches!(event,1|2){return Err(ruvane::ComponentError::InvalidEvent);}
  let url=if event==1{"/session-action"}else{"/rate-manual"};
  self.outgoing=Some(ruvane::ActionRequest::new(event,InternalUrl::new(url).unwrap(),ruvane::FormData::default().field("mode","rotate").unwrap()).unwrap());self.pending=Some(event);Ok(())
 }
 fn take_action(&mut self)->Option<ruvane::ActionRequest>{self.outgoing.take()}
 fn action_result(&mut self,action:u32,outcome:ruvane::ActionOutcome)->Result<(),ruvane::ComponentError>{
  if self.pending.take()!=Some(action){return Err(ruvane::ComponentError::InvalidEvent);}
  match outcome {Ok(reply)=>self.page_data(reply.data().clone()),Err(ruvane::ActionFailure::RateLimited)=>{self.message="Too many requests".into();Ok(())},Err(_)=>{self.message="Rejected".into();Ok(())}}
 }
}
pub fn rate_component()->Box<dyn ruvane::Component>{Box::new(RateView{message:"Ready".into(),pending:None,outgoing:None})}
`;
    await writeFile(pagesFile,pages);
    const componentFile=join(project,"src/components.rs");let components=await readFile(componentFile,"utf8");
    components=components.replace('pub fn create_component(props: ComponentProps) -> Result<Box<dyn Component>, ComponentError> {','pub fn create_component(props: ComponentProps) -> Result<Box<dyn Component>, ComponentError> {\n    if props.component()==25 && props.state()==InitialState::Number(0) {return Ok(crate::pages::rate_component());}');
    await writeFile(componentFile,components);
    for(const name of ["session-action","session-native","rate-anonymous","rate-anonymous-other"])await file(name+"/guard.rs",'pub fn guard()->ruvane::RouteGuard {crate::rate_server::mutations().session_guard(crate::session_server::store().clone())}\n');
    await file("rate-anonymous/route.rs",'pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().get(|_|ruvane::Response::text("Anonymous quota"))}\n');
    await file("rate-anonymous-other/route.rs",'pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().get(|_|ruvane::Response::text("Same anonymous quota"))}\n');
    await file("rate-page/page.rs",`pub fn page()->ruvane::FilePage {
 let page=ruvane::FilePage::client("rate-page",24,crate::pages::zero,|_|{
  #[cfg(not(target_family="wasm"))] crate::rate_server::LOADS.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
  Ok(ruvane::ClientPageData::new("Quota page",crate::pages::session_data("Private demo content")))
 });
 #[cfg(not(target_family="wasm"))] let page=page.server_cache(ruvane::ServerCachePolicy::public(60).unwrap());
 page
}\n`);
    await file("rate-page/guard.rs",'pub fn guard()->ruvane::RouteGuard {crate::rate_server::pages().guard()}\n');
    await file("rate-controls/page.rs",`pub fn page()->ruvane::FilePage {ruvane::FilePage::client("rate-controls",25,crate::pages::zero,|context|{
 #[cfg(not(target_family="wasm"))] let fallback={
  let token=crate::session_server::store().lookup(context.request().unwrap()).and_then(|session|session.map_or(Ok(None),|session|session.csrf_token())).map_err(|_|ruvane::PageError::InternalServerError)?;
  crate::pages::session_forms(token)
 };
 #[cfg(target_family="wasm")] let fallback={let _=context;ruvane::Html::default()};
 Ok(ruvane::ClientPageData::new("Quota controls",crate::pages::session_data("Ready")).fallback(fallback))
})}\n`);
    for(const name of ["a","b"]) {
      await file(`rate-capacity/${name}/guard.rs`,'pub fn guard()->ruvane::RouteGuard {crate::rate_server::capacity().guard()}\n');
      await file(`rate-capacity/${name}/route.rs`,'pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().get(|_|ruvane::Response::text("Capacity"))}\n');
    }
    await file("rate-manual/route.rs",`pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().post_action_response(|_|{
 crate::session_server::MUTATIONS.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
 ruvane::Response::too_many_requests(ruvane::RetryAfter::new(3).unwrap())
})}\n`);
    await file("rate-stats/route.rs",`pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().get(|_|{
 let stats=crate::rate_server::mutations().stats().unwrap();let page=crate::rate_server::pages().stats().unwrap();
 ruvane::Response::text(format!("{} {} {} {} {} {}",crate::session_server::MUTATIONS.load(std::sync::atomic::Ordering::SeqCst),crate::rate_server::LOADS.load(std::sync::atomic::Ordering::SeqCst),stats.allowed,stats.limited,page.allowed,page.limited))
})}\n`);
    await file("rate-reset/route.rs",`pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().post_form(|request|{
 crate::rate_server::mutations().clear().unwrap();crate::rate_server::pages().clear().unwrap();crate::rate_server::capacity().clear().unwrap();crate::session_server::cache().clear().unwrap();
 let response=ruvane::Response::text("Quotas reset");
 if request.form_data().unwrap().get("session")==Some("clear") {crate::session_server::store().clear().unwrap();response.with_cookie(crate::session_server::store().removal_cookie()).unwrap()}else{response}
})}\n`);
    if(options.prepare)await options.prepare(project);
  }});
}

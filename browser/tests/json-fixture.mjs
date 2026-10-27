import {readFile,writeFile,mkdir} from "node:fs/promises";
import {join} from "node:path";
import {csrfFixture} from "./csrf-fixture.mjs";

export async function jsonFixture(options) {
 return csrfFixture({...options,prepare:async project=>{
  async function file(path,source){const target=join(project,"src/routes",path);await mkdir(join(target,".."),{recursive:true});await writeFile(target,source);}
  const lib=join(project,"src/lib.rs");await writeFile(lib,(await readFile(lib,"utf8"))+'\n#[cfg(not(target_family="wasm"))]\npub mod json_tasks;\n#[cfg(not(target_family="wasm"))]\npub mod json_server;\n');
  await writeFile(join(project,"src/json_tasks.rs"),await readFile(join(options.root,"examples/support/json_tasks.rs"),"utf8"));
  const sessions=join(project,"src/session_server.rs");await writeFile(sessions,(await readFile(sessions,"utf8")).replace("ruvane_alpha49_session","ruvane_alpha51_session"));
  await writeFile(join(project,"src/json_server.rs"),String.raw`
#[cfg(target_family="wasm")] compile_error!("SERVER_JSON_BACKEND_ALPHA51 must stay native");
use std::sync::{Arc,OnceLock,atomic::{AtomicUsize,Ordering}};
use ruvane::{RateLimiter,RateLimitPolicy,RateLimitLimits,JsonValue};
pub static GUARDS:AtomicUsize=AtomicUsize::new(0);
pub static ECHOES:AtomicUsize=AtomicUsize::new(0);
pub fn tasks()->Arc<crate::json_tasks::Tasks>{static TASKS:OnceLock<Arc<crate::json_tasks::Tasks>>=OnceLock::new();TASKS.get_or_init(||Arc::new(Default::default())).clone()}
pub fn quota()->&'static RateLimiter {static QUOTA:OnceLock<RateLimiter>=OnceLock::new();QUOTA.get_or_init(||RateLimiter::new(RateLimitPolicy::new(128,60).unwrap(),RateLimitLimits::default()))}
pub fn low()->&'static RateLimiter {static QUOTA:OnceLock<RateLimiter>=OnceLock::new();QUOTA.get_or_init(||RateLimiter::new(RateLimitPolicy::new(2,60).unwrap(),RateLimitLimits::default()))}
pub fn audit()->ruvane::RouteGuard {ruvane::RouteGuard::new(|context|{
 let request=context.request();if request.method()==ruvane::Method::Post {assert!(request.json().is_some()&&request.form_data().is_none());}
 GUARDS.fetch_add(1,Ordering::SeqCst);ruvane::GuardDecision::Allow
})}
pub fn stats()->ruvane::Response {
 let (calls,writes,tasks)=self::tasks().counts().unwrap();
 let fields=[("calls",calls),("writes",writes),("tasks",tasks),("guards",GUARDS.load(Ordering::SeqCst)),("echoes",ECHOES.load(Ordering::SeqCst))];
 ruvane::Response::json(&JsonValue::object(fields.into_iter().map(|(key,value)|(key.into(),JsonValue::from(value as u64))).collect()).unwrap())
}
`);
  const pagesFile=join(project,"src/pages.rs");let pages=await readFile(pagesFile,"utf8");
  pages=pages.replace('    let mut app = App::new();','    let mut app = App::new();\n    #[cfg(not(target_family="wasm"))] app.guard_branch("/api",crate::json_server::audit()).unwrap();');
  pages+=String.raw`
pub struct JsonProbe;
pub fn json_checks()->bool {
 let raw=br#"{"n":18446744073709551615,"s":"Rust \uD83E\uDD80","a":[true,false,null]}"#;
 let Ok(value)=ruvane::JsonValue::parse(raw)else{return false;};
 value.get("n").and_then(ruvane::JsonValue::as_number).and_then(|n|n.to_u64().ok())==Some(u64::MAX)
 && value.get("s").and_then(ruvane::JsonValue::as_str)==Some("Rust 🦀")
 && ruvane::JsonValue::parse(value.encode().as_bytes())==Ok(value)
 && ruvane::JsonValue::parse(br#"{"x":1,"\u0078":2}"#)==Err(ruvane::JsonError::DuplicateKey)
 && ruvane::JsonValue::parse(br#""\uD800""#).is_err()
 && ruvane::JsonNumber::parse("1e9999").unwrap().to_f64().is_err()
}
impl ruvane::Component for JsonProbe {
 fn page_data(&mut self,data:ruvane::ActionData)->Result<(),ruvane::ComponentError>{if data.is_empty(){Ok(())}else{Err(ruvane::ComponentError::InvalidProps)}}
 fn view(&self)->Result<ruvane::View,ruvane::ComponentError>{
  if !json_checks(){return Err(ruvane::ComponentError::InvalidProps);}
  let mut view=ruvane::View::new();let root=view.element(0,ruvane::BrowserTag::Section)?;
  view.leaf(root,ruvane::BrowserTag::Heading,"Shared JSON engine")?;view.leaf(root,ruvane::BrowserTag::Output,"Rust JSON checks passed")?;Ok(view)
 }
 fn dispatch(&mut self,_:u32)->Result<(),ruvane::ComponentError>{Err(ruvane::ComponentError::InvalidEvent)}
}
`;
  await writeFile(pagesFile,pages);
  const components=join(project,"src/components.rs");await writeFile(components,(await readFile(components,"utf8")).replace('pub fn create_component(props: ComponentProps) -> Result<Box<dyn Component>, ComponentError> {','pub fn create_component(props: ComponentProps) -> Result<Box<dyn Component>, ComponentError> {\n    if props.component()==25 && props.state()==InitialState::Number(0) {return Ok(Box::new(crate::pages::JsonProbe));}'));
  await file("json-probe/page.rs",'pub fn page()->ruvane::FilePage {ruvane::FilePage::client("json-probe",25,crate::pages::zero,|_|Ok(ruvane::ClientPageData::new("JSON probe",Default::default())))}\n');
  await file("api/guard.rs",'pub fn guard()->ruvane::RouteGuard {crate::session_server::store().csrf_guard()}\n');
  await file("api/tasks/route.rs",'pub fn route()->ruvane::FileEndpoint {crate::json_tasks::endpoint(crate::json_server::tasks())}\n');
  await file("api/tasks/guard.rs",'pub fn guard()->ruvane::RouteGuard {crate::json_server::quota().session_guard(crate::session_server::store().clone())}\n');
  await file("api/limited/route.rs",'pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().post_json(|request|crate::json_server::tasks().write(request))}\n');
  await file("api/limited/guard.rs",'pub fn guard()->ruvane::RouteGuard {crate::json_server::low().session_guard(crate::session_server::store().clone())}\n');
  await file("api/echo/route.rs",'pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().post_json(|request|{crate::json_server::ECHOES.fetch_add(1,std::sync::atomic::Ordering::SeqCst);ruvane::Response::json(request.json().unwrap())})}\n');
  await file("json-stats/route.rs",'pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().get(|_|crate::json_server::stats())}\n');
  await file("json-reset/route.rs",'pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().post_form(|request|{crate::json_server::quota().clear().unwrap();crate::json_server::low().clear().unwrap();let response=ruvane::Response::text("Quotas reset");if request.form_data().unwrap().get("session")==Some("clear"){crate::session_server::store().clear().unwrap();response.with_cookie(crate::session_server::store().removal_cookie()).unwrap()}else{response}})}\n');
  if(options.prepare)await options.prepare(project);
 }});
}

import {readFile,writeFile,mkdir} from "node:fs/promises";
import {join} from "node:path";
import {cookieFixture} from "./cookies-fixture.mjs";
export async function sessionFixture(options){
 return cookieFixture({...options,prepare:async project=>{
  const libFile=join(project,"src/lib.rs");await writeFile(libFile,(await readFile(libFile,"utf8"))+'\n#[cfg(not(target_family="wasm"))]\npub mod session_server;\n');
  await writeFile(join(project,"src/session_server.rs"),`#[cfg(target_family="wasm")] compile_error!("session backend reached Wasm");
use ruvane::{SessionCookie,SessionData,SessionLimits,SessionStore,Request,Response,ActionReply,StatusCode};
use std::sync::{OnceLock,atomic::{AtomicUsize,Ordering}};
pub const COOKIE:&str="ruvane_alpha48_session";
pub static LOADS:AtomicUsize=AtomicUsize::new(0);
pub fn cache()->ruvane::ServerCache {static CACHE:OnceLock<ruvane::ServerCache>=OnceLock::new();CACHE.get_or_init(ruvane::ServerCache::default).clone()}
pub fn store()->&'static SessionStore {
 static STORE:OnceLock<SessionStore>=OnceLock::new();
 STORE.get_or_init(||{
  let idle=std::env::var("RUVANE_SESSION_IDLE_SECONDS").ok().map(|v|v.parse().unwrap()).unwrap_or(300);
  let absolute=std::env::var("RUVANE_SESSION_ABSOLUTE_SECONDS").ok().map(|v|v.parse().unwrap()).unwrap_or(3600);
  SessionStore::new(SessionLimits::new(4,2048,256,idle,absolute).unwrap(),SessionCookie::local_http(COOKIE).unwrap()).expect("session store unavailable")
 })
}
pub fn active(request:&Request)->bool {store().lookup(request).ok().flatten().is_some()}
pub fn mutate(request:&Request,native:bool)->Response {
 let mode=request.form_data().unwrap().get("mode").unwrap_or("");
 let current=match store().lookup(request){Ok(value)=>value,Err(_)=>return Response::text("Session unavailable").with_status(StatusCode::ServiceUnavailable)};
 let result=match mode {
  "open"=>{
   let mut data=SessionData::default();data.insert("account_id","demo-account").unwrap();data.insert("private_marker","SERVER_SESSION_DATA_ALPHA48").unwrap();
   match current.as_ref(){Some(session)=>store().rotate_with_data(session,data),None=>store().create(data).map(Some)}
  },
  "rotate"=>match current.as_ref(){Some(session)=>store().rotate(session),None=>return Response::text("Access denied").with_status(StatusCode::Forbidden)},
  "close"=>{
   if let Some(session)=current.as_ref(){if store().revoke(session).is_err(){return Response::text("Session unavailable").with_status(StatusCode::ServiceUnavailable);}}
   Ok(Some(store().removal_cookie()))
  },
  _=>return Response::action(&ActionReply::validation(Default::default())),
 };
 let cookie=match result {Ok(Some(cookie))=>cookie,Ok(None)=>return Response::text("Access denied").with_status(StatusCode::Forbidden),Err(_)=>return Response::text("Session unavailable").with_status(StatusCode::ServiceUnavailable)};
 let response=if native {Response::see_other(ruvane::InternalUrl::new("/session").unwrap())}
 else {Response::action(&ActionReply::success(crate::pages::session_data(if mode=="close"{"Anonymous"}else{"Session active"}))) };
 response.with_cookie(cookie).unwrap()
}
`);
  const pagesFile=join(project,"src/pages.rs");let pages=await readFile(pagesFile,"utf8");
  pages=pages.replace('    let mut app = App::new();','    let mut app = App::new();\n    #[cfg(not(target_family="wasm"))] app.server_cache(crate::session_server::cache());');
  pages=pages.replace('[("home", "Home"), ("tasks", "Tasks")]', '[("home", "Home"), ("tasks", "Tasks"), ("session", "Sessions")]');
  pages+=`
pub fn session_data(message:&str)->ruvane::ActionData {let mut data=ruvane::ActionData::default();data.insert("message",ruvane::ActionValue::Text(message.into())).unwrap();data}
pub fn session_forms()->Html {
 let mut body=element(Tag::Section).child(element(Tag::H2).child("Native session forms"));
 for (mode,label) in [("open","Open native demo session"),("rotate","Rotate native session"),("close","Close native session")] {
  body=body.child(PostForm::new(InternalUrl::new("/session-native").unwrap()).unwrap().hidden("mode",mode).unwrap().submit_label(label).unwrap().render());
 }
 body.render()
}
pub struct SessionView {private:bool,message:String,pending:Option<u32>,outgoing:Option<ruvane::ActionRequest>}
impl ruvane::Component for SessionView {
 fn page_data(&mut self,data:ruvane::ActionData)->Result<(),ruvane::ComponentError>{
  let message=data.text("message").ok_or(ruvane::ComponentError::InvalidProps)?;
  if data.len()!=1||if self.private {message!="Private demo content"}else{!matches!(message,"Anonymous"|"Session active")} {return Err(ruvane::ComponentError::InvalidProps);}
  self.message=message.into();Ok(())
 }
 fn view(&self)->Result<ruvane::View,ruvane::ComponentError>{
  let mut view=ruvane::View::new();let root=view.keyed_element(0,1,ruvane::BrowserTag::Section)?;
  view.keyed_leaf(root,1,ruvane::BrowserTag::Heading,if self.private{"Private session page"}else{"Session demo"})?;
  view.keyed_leaf(root,2,ruvane::BrowserTag::Output,&self.message)?;
  if !self.private {
   view.keyed_button(root,3,"Open demo session",1,self.pending.is_some())?;
   view.keyed_button(root,4,"Rotate session",2,self.pending.is_some())?;
   view.keyed_button(root,5,"Close session",3,self.pending.is_some())?;
  }
  Ok(view)
 }
 fn dispatch(&mut self,event:u32)->Result<(),ruvane::ComponentError>{
  if self.private||self.pending.is_some()||!(1..=3).contains(&event){return Err(ruvane::ComponentError::InvalidEvent);}
  let fields=ruvane::FormData::default().field("mode",match event{1=>"open",2=>"rotate",_=>"close"}).unwrap();
  self.outgoing=Some(ruvane::ActionRequest::new(event,InternalUrl::new("/session-action").unwrap(),fields).map_err(|_|ruvane::ComponentError::InvalidProps)?);self.pending=Some(event);Ok(())
 }
 fn take_action(&mut self)->Option<ruvane::ActionRequest>{self.outgoing.take()}
 fn action_result(&mut self,action:u32,outcome:ruvane::ActionOutcome)->Result<(),ruvane::ComponentError>{
  if self.pending.take()!=Some(action){return Err(ruvane::ComponentError::InvalidEvent);}
  self.page_data(outcome.map_err(|_|ruvane::ComponentError::InvalidProps)?.data().clone())
 }
}
pub fn session_component(private:bool)->Box<dyn ruvane::Component>{Box::new(SessionView{private,message:String::new(),pending:None,outgoing:None})}
`;
  await writeFile(pagesFile,pages);
  const componentsFile=join(project,"src/components.rs"),components=await readFile(componentsFile,"utf8");const factory="pub fn create_component(props: ComponentProps) -> Result<Box<dyn Component>, ComponentError> {";
  await writeFile(componentsFile,components.replace(factory,factory+'\n    if matches!(props.component(),23|24) && props.state()==InitialState::Number(0) {return Ok(crate::pages::session_component(props.component()==24));}'));
  async function file(path,source){const destination=join(project,"src/routes",path);await mkdir(join(destination,".."),{recursive:true});await writeFile(destination,source);}
  await file("session/page.rs",`pub fn page()->ruvane::FilePage {ruvane::FilePage::client("session",23,crate::pages::zero,load)}
fn load(context:&ruvane::PageContext<'_>)->Result<ruvane::ClientPageData,ruvane::PageError>{
 #[cfg(not(target_family="wasm"))]
 let message=if crate::session_server::active(context.request().unwrap()){"Session active"}else{"Anonymous"};
 #[cfg(target_family="wasm")]
 let message={let _=context;"Anonymous"};
 Ok(ruvane::ClientPageData::new("Session demo",crate::pages::session_data(message)).fallback(crate::pages::session_forms()))
}
`);
  await file("session-private/guard.rs",`pub fn guard()->ruvane::RouteGuard {crate::session_server::store().guard()}\n`);
  await file("session-private/page.rs",`pub fn page()->ruvane::FilePage {
 let page=ruvane::FilePage::client("session-private",24,crate::pages::zero,load);
 #[cfg(not(target_family="wasm"))] let page=page.server_cache(ruvane::ServerCachePolicy::public(30).unwrap());
 page
}
fn load(context:&ruvane::PageContext<'_>)->Result<ruvane::ClientPageData,ruvane::PageError>{
 #[cfg(not(target_family="wasm"))] {
  let session=crate::session_server::store().lookup(context.request().unwrap()).map_err(|_|ruvane::PageError::InternalServerError)?.ok_or(ruvane::PageError::InternalServerError)?;
  let data=session.data().map_err(|_|ruvane::PageError::InternalServerError)?.ok_or(ruvane::PageError::InternalServerError)?;
  if data.get("private_marker")!=Some("SERVER_SESSION_DATA_ALPHA48"){return Err(ruvane::PageError::InternalServerError);}
  crate::session_server::LOADS.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
 }
 #[cfg(target_family="wasm")] let _=context;
 Ok(ruvane::ClientPageData::new("Private session page",crate::pages::session_data("Private demo content")))
}
`);
  await file("session-action/route.rs",`pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().post_action_response(|request|crate::session_server::mutate(request,false))}\n`);
  await file("session-native/route.rs",`pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().post_form(|request|crate::session_server::mutate(request,true))}\n`);
  await file("session-stats/route.rs",`pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().get(|_|{
 let stats=crate::session_server::store().stats().unwrap();ruvane::Response::text(format!("{} {} {} {}",stats.entries,stats.data_bytes,crate::session_server::LOADS.load(std::sync::atomic::Ordering::SeqCst),crate::session_server::cache().stats().unwrap().entries))
})}\n`);
  if(options.prepare)await options.prepare(project);
 }});
}

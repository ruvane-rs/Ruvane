import {execFileSync} from "node:child_process";
import {readFile,writeFile,mkdir} from "node:fs/promises";
import {join} from "node:path";
export async function cookieFixture({directory,root,cli,harness="export {};",prepare}) {
  execFileSync(cli,["new","cookie-app","--interactive","--file-routes","--framework-path",root],{cwd:directory,encoding:"utf8",timeout:120000,stdio:["ignore","pipe","pipe"]});
  const project=join(directory,"cookie-app"),pagesFile=join(project,"src/pages.rs"),componentFile=join(project,"src/components.rs");
  let pages=await readFile(pagesFile,"utf8");
  pages=pages.replace('    app\n}',`    #[cfg(target_family="wasm")]
    {
     app.client_layout(ruvane::PageRoute::new("cookiepublic","/cookie-public").unwrap(),None,components::WORKSPACE,zero,|_|Ok(ruvane::ClientLayoutData::new(Default::default()))).unwrap();
     app.client_page(ruvane::PageRoute::new("cookiepublicpage","/cookie-public/{slot}").unwrap(),components::HOME,zero,|_|Ok(ruvane::ClientPageData::new("Public cookie fixture",public_data()))).unwrap();
     app.client_page_layout("cookiepublicpage",Some("cookiepublic")).unwrap();
    }
    app
}`);
  pages+=`
pub const PREFERENCE_COOKIE:&str="ruvane_alpha47_preference";
pub fn preference(request:&ruvane::Request)->&'static str {match request.cookies().get(PREFERENCE_COOKIE) {Some("dark")=>"dark",Some("light")=>"light",_=>"none"}}
pub fn preference_data(value:&str)->ruvane::ActionData {let mut data=ruvane::ActionData::default();data.insert("preference",ruvane::ActionValue::Text(value.into())).unwrap();data}
pub fn public_data()->ruvane::ActionData {let mut data=ruvane::ActionData::default();data.insert("count",ruvane::ActionValue::Number(0)).unwrap();data}
pub fn preference_forms()->Html {
 let target=InternalUrl::new("/cookie-native").unwrap();
 element(Tag::Section).child(element(Tag::H2).child("Native cookie forms"))
 .child(PostForm::new(target.clone()).unwrap().hidden("mode","set").unwrap().submit_label("Save native cookie").unwrap().render())
 .child(PostForm::new(target).unwrap().hidden("mode","clear").unwrap().submit_label("Clear native cookie").unwrap().render()).render()
}
pub struct Preference {value:String,pending:Option<u32>,outgoing:Option<ruvane::ActionRequest>}
impl ruvane::Component for Preference {
 fn page_data(&mut self,data:ruvane::ActionData)->Result<(),ruvane::ComponentError>{self.value=data.text("preference").filter(|value|matches!(*value,"none"|"dark"|"light")).ok_or(ruvane::ComponentError::InvalidProps)?.into();Ok(())}
 fn view(&self)->Result<ruvane::View,ruvane::ComponentError>{
  let mut view=ruvane::View::new();let root=view.keyed_element(0,1,ruvane::BrowserTag::Section)?;
  view.keyed_leaf(root,1,ruvane::BrowserTag::Heading,"Cookie preference")?;
  view.keyed_leaf(root,2,ruvane::BrowserTag::Output,&format!("Current preference: {}",self.value))?;
  view.keyed_button(root,3,"Save cookie",1,self.pending.is_some())?;
  view.keyed_button(root,4,"Clear cookie",2,self.pending.is_some())?;
  Ok(view)
 }
 fn dispatch(&mut self,event:u32)->Result<(),ruvane::ComponentError>{
  if self.pending.is_some()||!matches!(event,1|2){return Err(ruvane::ComponentError::InvalidEvent);}
  let fields=ruvane::FormData::default().field("mode",if event==1{"set"}else{"clear"}).unwrap();
  self.outgoing=Some(ruvane::ActionRequest::new(event,InternalUrl::new("/cookie-action").unwrap(),fields).map_err(|_|ruvane::ComponentError::InvalidProps)?);self.pending=Some(event);Ok(())
 }
 fn take_action(&mut self)->Option<ruvane::ActionRequest>{self.outgoing.take()}
 fn action_result(&mut self,action:u32,outcome:ruvane::ActionOutcome)->Result<(),ruvane::ComponentError>{
  if self.pending.take()!=Some(action){return Err(ruvane::ComponentError::InvalidEvent);}
  let reply=outcome.map_err(|_|ruvane::ComponentError::InvalidProps)?;
  self.page_data(reply.data().clone())
 }
}
pub fn preference_component()->Box<dyn ruvane::Component>{Box::new(Preference{value:"none".into(),pending:None,outgoing:None})}
`;
  await writeFile(pagesFile,pages);
  const factory="pub fn create_component(props: ComponentProps) -> Result<Box<dyn Component>, ComponentError> {";
  const components=await readFile(componentFile,"utf8");
  await writeFile(componentFile,components.replace(factory,factory+'\n    if props.component()==22 && props.state()==InitialState::Number(0) {return Ok(crate::pages::preference_component());}'));
  async function file(path,source){const destination=join(project,"src/routes",path);await mkdir(join(destination,".."),{recursive:true});await writeFile(destination,source);}
  await file("cookie/page.rs",`pub fn page()->ruvane::FilePage {
 let page=ruvane::FilePage::client("cookie",22,crate::pages::zero,|context|Ok(ruvane::ClientPageData::new("Cookie preference",crate::pages::preference_data(crate::pages::preference(context.request().unwrap()))).fallback(crate::pages::preference_forms())));
 #[cfg(not(target_family="wasm"))] let page=page.guard(ruvane::RouteGuard::new(|_|ruvane::GuardDecision::Private));
 page
}
`);
  const mutation=`fn mutation(request:&ruvane::Request,native:bool)->ruvane::Response {
 use ruvane::{Cookie,Response,ActionReply};
 let mode=request.form_data().unwrap().get("mode").unwrap_or("");
 if !matches!(mode,"set"|"clear") {return Response::action(&ActionReply::validation(Default::default()));}
 let cookie=Cookie::local_http(crate::pages::PREFERENCE_COOKIE,"dark").unwrap().max_age(600).unwrap();
 let cookie=if mode=="clear"{cookie.removal()}else{cookie};
 let response=if native {Response::see_other(ruvane::InternalUrl::new("/cookie").unwrap())}
 else{Response::action(&ActionReply::success(crate::pages::preference_data(if mode=="clear"{"none"}else{"dark"})))};
 response.with_cookie(cookie).unwrap()
}
`;
  await file("cookie-action/route.rs",mutation+`pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().post_action_response(|request|mutation(request,false)).guard(ruvane::RouteGuard::new(|_|ruvane::GuardDecision::Private))}\n`);
  await file("cookie-native/route.rs",mutation+`pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().post_form(|request|mutation(request,true)).guard(ruvane::RouteGuard::new(|_|ruvane::GuardDecision::Private))}\n`);
  await file("cookie-state/route.rs",`pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().get(|request|ruvane::Response::text(crate::pages::preference(request)))}\n`);
  await file("cookie-public/[slot]/route.rs",`pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().get(|request|{
 use ruvane::{ComponentProps,InitialState,InternalUrl,NavigationLayout,NavigationPage,Response,StatusCode};
 let props=ComponentProps::new(crate::components::HOME,InitialState::Number(0)).unwrap();
 let layout=NavigationLayout::new("cookiepublic",InternalUrl::new("/cookie-public").unwrap(),ComponentProps::new(crate::components::WORKSPACE,InitialState::Number(0)).unwrap(),Default::default()).unwrap();
 let page=NavigationPage::new(InternalUrl::new(request.path()).unwrap(),"Public cookie fixture",props,crate::pages::public_data(),StatusCode::Ok).unwrap().with_layouts(vec![layout]).unwrap();
 if request.param("slot")==Some("slow"){std::thread::sleep(std::time::Duration::from_millis(250));}
 if request.param("slot")==Some("invalid-cookie"){return Response::text("invalid navigation fixture").with_cookie(ruvane::Cookie::local_http(crate::pages::PREFERENCE_COOKIE,"light").unwrap()).unwrap();}
 let response=Response::navigation_with_cache(&page,ruvane::NavigationCachePolicy::public(30).unwrap());
 match request.param("slot") {
  Some("rotate")=>response.with_cookie(ruvane::Cookie::local_http(crate::pages::PREFERENCE_COOKIE,"light").unwrap()).unwrap(),
  Some("prefetch-cookie")=>response.with_cookie(ruvane::Cookie::local_http(crate::pages::PREFERENCE_COOKIE,"dark").unwrap()).unwrap(),
  _=>response,
 }
})}\n`);
  await file("cookie-checks/route.rs",`pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().get(|_|ruvane::Response::interactive_html(ruvane::Document::new("Cookie checks",ruvane::module_script(&ruvane::InternalUrl::new("/assets/cookies-checks.mjs").unwrap())).render()))}\n`);
  await writeFile(join(project,"assets/cookies-checks.mjs"),harness);
  const mainFile=join(project,"src/main.rs"),main=await readFile(mainFile,"utf8");
  await writeFile(mainFile,main.replace('    let mut app = pages::application();','    let mut app = pages::application();\n    app.asset("/assets/cookies-checks.mjs", include_bytes!("../assets/cookies-checks.mjs"), MediaType::JavaScript)?;'));
  if(prepare)await prepare(project);
  execFileSync(cli,["build","--offline"],{cwd:project,encoding:"utf8",timeout:120000,maxBuffer:2*1024*1024,stdio:["ignore","pipe","pipe"]});
  const bundle=join(project,"dist/build-000001"),manifest=await readFile(join(bundle,"Ruvane.bundle"),"utf8"),revision=manifest.match(/^revision\t([0-9]+)$/m)[1];
  return {project,bundle,revision};
}

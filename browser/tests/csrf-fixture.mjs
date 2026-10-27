import {readFile,writeFile,mkdir} from "node:fs/promises";
import {join} from "node:path";
import {sessionFixture} from "./sessions-fixture.mjs";

export async function csrfFixture(options) {
  return sessionFixture({...options,prepare:async project=>{
    async function file(path,source) {const target=join(project,"src/routes",path);await mkdir(join(target,".."),{recursive:true});await writeFile(target,source);}
    const mainFile=join(project,"src/main.rs");
    await writeFile(mainFile,(await readFile(mainFile,"utf8")).replace("    app.client_runtime(runtime)?;",'    runtime = runtime.csrf_endpoint(InternalUrl::new("/csrf-token")?)?;\n    app.client_runtime(runtime)?;'));
    const serverFile=join(project,"src/session_server.rs");
    let server=await readFile(serverFile,"utf8");
    server=server.replace("ruvane_alpha48_session","ruvane_alpha49_session");
    server=server.replace("pub fn mutate(request:&Request,native:bool)->Response {",`pub static MUTATIONS:AtomicUsize=AtomicUsize::new(0);
pub fn mutation_guard()->ruvane::RouteGuard {
 ruvane::RouteGuard::new(|context| {
  let request=context.request();
  if request.form_data().and_then(|form|form.get("mode"))==Some("open") && request.cookies().get(COOKIE).is_none() {return ruvane::GuardDecision::Private;}
  match store().verify_csrf(request) {Ok(true)=>ruvane::GuardDecision::Private,Ok(false)=>ruvane::GuardDecision::Forbidden,Err(_)=>ruvane::GuardDecision::Unavailable}
 })
}
pub fn mutate(request:&Request,native:bool)->Response {
 MUTATIONS.fetch_add(1,std::sync::atomic::Ordering::SeqCst);`);
    await writeFile(serverFile,server);
    for(const name of ["session-action","session-native"])await file(name+"/guard.rs","pub fn guard()->ruvane::RouteGuard {crate::session_server::mutation_guard()}\n");
    await file("csrf-token/route.rs",'pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().get(|request|crate::session_server::store().csrf_response(request))}\n');
    await file("csrf-stats/route.rs",'pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().get(|_|ruvane::Response::text(crate::session_server::MUTATIONS.load(std::sync::atomic::Ordering::SeqCst).to_string()))}\n');
    await file("csrf-protected/guard.rs",'pub fn guard()->ruvane::RouteGuard {crate::session_server::store().csrf_guard()}\n');
    await file("csrf-protected/route.rs",`pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().get(|_|ruvane::Response::text("Protected")).post_action_response(|_|{
 crate::session_server::MUTATIONS.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
 ruvane::Response::action(&ruvane::ActionReply::success(Default::default()))
})}\n`);
    const pageFile=join(project,"src/routes/session/page.rs");
    let page=await readFile(pageFile,"utf8");
    page=page.replace(' Ok(ruvane::ClientPageData::new',` #[cfg(not(target_family="wasm"))]
 let token=crate::session_server::store().lookup(context.request().unwrap()).and_then(|session|session.map_or(Ok(None),|session|session.csrf_token())).map_err(|_|ruvane::PageError::InternalServerError)?;
 #[cfg(target_family="wasm")] let token=None;
 Ok(ruvane::ClientPageData::new`);
    await writeFile(pageFile,page.replace("crate::pages::session_forms()","crate::pages::session_forms(token)"));
    const pagesFile=join(project,"src/pages.rs");let pages=await readFile(pagesFile,"utf8");
    pages=pages.replace('self.page_data(outcome.map_err(|_|ruvane::ComponentError::InvalidProps)?.data().clone())','match outcome {Ok(reply)=>self.page_data(reply.data().clone()),Err(_)=>Ok(())}');
    pages=pages.replace("pub fn session_forms()->Html", "pub fn session_forms(token:Option<ruvane::CsrfToken>)->Html");
    pages=pages.replace('  body=body.child(PostForm::new(InternalUrl::new("/session-native").unwrap()).unwrap().hidden("mode",mode).unwrap().submit_label(label).unwrap().render());',`  let mut form=PostForm::new(InternalUrl::new("/session-native").unwrap()).unwrap();
  if let Some(token)=token.clone() {form=form.csrf(token).unwrap();}
  body=body.child(form.hidden("mode",mode).unwrap().submit_label(label).unwrap().render());`);
    await writeFile(pagesFile,pages);
    if(options.prepare)await options.prepare(project);
  }});
}

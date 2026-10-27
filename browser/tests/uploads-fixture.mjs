import {readFile,writeFile,mkdir} from "node:fs/promises";
import {join} from "node:path";
import {csrfFixture} from "./csrf-fixture.mjs";

export async function uploadsFixture(options) {
 return csrfFixture({...options,harness:await readFile(join(options.root,"browser/tests/uploads-browser.mjs"),"utf8"),prepare:async project=>{
  const lib=join(project,"src/lib.rs");
  await writeFile(lib,(await readFile(lib,"utf8"))+'\nmod upload_component;\n#[cfg(not(target_family="wasm"))]\npub mod upload_server;\n');
  await writeFile(join(project,"src/upload_component.rs"),await readFile(join(options.root,"examples/support/upload_component.rs"),"utf8"));
  const components=join(project,"src/components.rs"),factory="pub fn create_component(props: ComponentProps) -> Result<Box<dyn Component>, ComponentError> {";
  await writeFile(components,(await readFile(components,"utf8")).replace(factory,factory+'\n    if props.component()==29 && props.state()==InitialState::Number(0) {return Ok(crate::upload_component::create());}'));
  await writeFile(join(project,"src/upload_server.rs"),String.raw`
#[cfg(target_family="wasm")] compile_error!("SERVER_UPLOAD_BACKEND_ALPHA57 must stay native");
use ruvane::*;
use std::sync::{Mutex,OnceLock};
pub fn files()->&'static Mutex<Vec<JsonValue>> {static FILES:OnceLock<Mutex<Vec<JsonValue>>>=OnceLock::new();FILES.get_or_init(||Mutex::new(Vec::new()))}
pub fn endpoint()->FileEndpoint {FileEndpoint::new().post_multipart(|request|{
 let data=request.multipart().unwrap();let caption=data.fields().get("caption").unwrap_or("");
 if caption.trim().is_empty(){let mut errors=ValidationErrors::default();errors.add(Some("caption"),ValidationCode::Required);return Response::validation_errors(&errors);}
 let Some(file)=data.file()else{return Response::text("File required").with_status(StatusCode::BadRequest);};
 let record=JsonValue::object(vec![("caption".into(),JsonValue::string(caption).unwrap()),("filename".into(),JsonValue::string(file.file_name()).unwrap()),("bytes".into(),(file.len() as u64).into())]).unwrap();
 let mut files=files().lock().unwrap();if files.len()==8{return Response::text("Full").with_status(StatusCode::Conflict);}files.push(record.clone());Response::json(&record)
}).guard(crate::session_server::store().csrf_guard())}
pub fn state()->FileEndpoint {FileEndpoint::new().get(|_|Response::json(&JsonValue::object(vec![("files".into(),JsonValue::array(files().lock().unwrap().clone()).unwrap())]).unwrap())).guard(crate::session_server::store().csrf_guard())}
`);
  async function file(path,source){const target=join(project,"src/routes",path);await mkdir(join(target,".."),{recursive:true});await writeFile(target,source);}
  await file("upload/route.rs",'pub fn route()->ruvane::FileEndpoint {crate::upload_server::endpoint()}\n');
  await file("uploads/state/route.rs",'pub fn route()->ruvane::FileEndpoint {crate::upload_server::state()}\n');
  await file("upload-session/route.rs",'pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().post_json(|request|{let store=crate::session_server::store();let reply=ruvane::Response::json(&true.into());if store.lookup(request).unwrap().is_some(){reply}else{reply.with_cookie(store.create(Default::default()).unwrap()).unwrap()}})}\n');
  await file("upload-logout/route.rs",'pub fn route()->ruvane::FileEndpoint {ruvane::FileEndpoint::new().post_json(|request|{let store=crate::session_server::store();let session=store.lookup(request).unwrap().unwrap();store.revoke(&session).unwrap();ruvane::Response::json(&false.into()).with_cookie(store.removal_cookie()).unwrap()}).guard(crate::session_server::store().csrf_guard())}\n');
  await file("upload-probe/page.rs",'pub fn page()->ruvane::FilePage {ruvane::FilePage::client("upload-probe",29,crate::pages::zero,|_|Ok(ruvane::ClientPageData::new("Rust file uploads",Default::default())))}\n');
  if(options.prepare)await options.prepare(project);
 }});
}

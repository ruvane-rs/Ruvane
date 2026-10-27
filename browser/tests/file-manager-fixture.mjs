import {readFile,writeFile} from "node:fs/promises";
import {join} from "node:path";
import {fileStoreFixture} from "./file-store-fixture.mjs";
export async function fileManagerFixture(options){
 return fileStoreFixture({...options,harness:await readFile(join(options.root,"browser/tests/file-manager-browser.mjs"),"utf8"),prepare:async project=>{
  for(const name of ["file_manager_component","file_manager_app"])await writeFile(join(project,`src/${name}.rs`),await readFile(join(options.root,`examples/support/${name}.rs`),"utf8"));
  const lib=join(project,"src/lib.rs");await writeFile(lib,(await readFile(lib,"utf8"))+'\npub mod file_manager_app;\nmod file_manager_component;\n');
  const factory="pub fn create_component(props: ComponentProps) -> Result<Box<dyn Component>, ComponentError> {",components=join(project,"src/components.rs");
  await writeFile(components,(await readFile(components,"utf8")).replace(factory,factory+'\n    if matches!(props.component(),30|31){return crate::file_manager_app::create(props);}\n'));
  const pages=join(project,"src/pages.rs");await writeFile(pages,(await readFile(pages,"utf8")).replace('    app\n}',`    app.client_page(ruvane::PageRoute::new("file-home","/file-home").unwrap(),31,zero,|_|Ok(ruvane::ClientPageData::new("File manager home",Default::default()))).unwrap();
    #[cfg(target_family="wasm")]
    app.client_page(ruvane::PageRoute::new("file-manager","/file-manager").unwrap(),30,zero,|_|Err(ruvane::PageError::InternalServerError)).unwrap();
    app
}`).replace('.child(element(Tag::P).child("Shared Rust routes. No third-party dependencies."))','.child(element(Tag::P).child("Shared Rust routes. No third-party dependencies."))\n        .child(ruvane::link(&InternalUrl::new("/private-files").unwrap(),"Native file forms and demo sessions"))'));
  const main=join(project,"src/main.rs");await writeFile(main,(await readFile(main,"utf8")).replace('ruvane_application::file_demo::register(&mut app,files)?;','let service=ruvane_application::file_demo::register_service(&mut app,files)?;service.register_manager(&mut app)?;').replace('    app.client_runtime(runtime)?;','    runtime=runtime.csrf_endpoint(InternalUrl::new("/private-files/csrf")?)?;\n    app.client_runtime(runtime)?;'));
  if(options.prepare)await options.prepare(project);
 }});
}

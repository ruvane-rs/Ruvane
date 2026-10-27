import {readFile,writeFile} from "node:fs/promises";
import {join} from "node:path";
import {cookieFixture} from "./cookies-fixture.mjs";
export async function fileStoreFixture(options) {
 return cookieFixture({...options,prepare:async project=>{
  const lib=join(project,"src/lib.rs");await writeFile(lib,(await readFile(lib,"utf8"))+'\n#[cfg(not(target_family="wasm"))]\npub mod file_demo;\n');
  await writeFile(join(project,"src/file_demo.rs"),await readFile(join(options.root,"examples/support/file_store_demo.rs"),"utf8"));
  await writeFile(join(project,"src/multipart_demo.css"),await readFile(join(options.root,"examples/support/multipart_demo.css"),"utf8"));
  const main=join(project,"src/main.rs");
  await writeFile(main,(await readFile(main,"utf8")).replace('    let mut app = pages::application();',`    let mut app = pages::application();
    let path=std::env::var("RUVANE_FILE_DIRECTORY").unwrap_or_else(|_|${JSON.stringify(join(project,"build-files"))}.into());
    let files=if std::env::var("RUVANE_FILE_RECOVER").as_deref()==Ok("1") {ruvane::FileStore::recover(path,"file-demo",Default::default())?}else{ruvane::FileStore::open(path,"file-demo",Default::default())?};
    ruvane_application::file_demo::register(&mut app,files)?;`));
  if(options.prepare)await options.prepare(project);
 }});
}

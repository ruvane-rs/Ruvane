import {execFileSync} from 'node:child_process';
import {cp,readFile,writeFile} from 'node:fs/promises';
import {basename,join} from 'node:path';
export async function deskFixture({directory,root,cli}) {
 const project=join(directory,'desk');
 await cp(join(root,'apps/desk'),project,{recursive:true,filter:path=>!['target','dist','.ruvane','desk-data','.git'].includes(basename(path))});
 for(const [file,from,to] of [['Cargo.toml','../..',root],['browser/Cargo.toml','../../../browser',join(root,'browser')]]) {
  const path=join(project,file);await writeFile(path,(await readFile(path,'utf8')).replaceAll(`path = "${from}"`,`path = ${JSON.stringify(to)}`));
 }
 await writeFile(join(project,'assets/desk-checks.mjs'),await readFile(join(root,'browser/tests/desk-browser.mjs')));
 const main=join(project,'src/main.rs');let source=await readFile(main,'utf8');
 source=source.replace('    app.client_runtime(runtime)?;',`    app.client_runtime(runtime)?;
    app.asset("/assets/desk-checks.mjs", include_bytes!("../assets/desk-checks.mjs"), MediaType::JavaScript)?;
    let mut checks = ruvane::FileRouteRegistry::new(&mut app);
    checks.endpoint("checks/route.rs", "/desk-checks", ruvane::FileEndpoint::new().get(|_| ruvane::Response::interactive_html(ruvane::Document::new("Desk checks", ruvane::module_script(&InternalUrl::new("/assets/desk-checks.mjs").unwrap())).render())))?;
    checks.finish()?;`);
 await writeFile(main,source);
 execFileSync(cli,['build','--offline'],{cwd:project,encoding:'utf8',timeout:180000,stdio:['ignore','pipe','pipe']});
 const bundle=join(project,'dist/build-000001'),manifest=await readFile(join(bundle,'Ruvane.bundle'),'utf8');
 const match=/^revision\t(\d+)$/m.exec(manifest);
 if(!match)throw Error('Missing Desk build revision');
 return {project,bundle,revision:match[1]};
}

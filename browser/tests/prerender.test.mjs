import test from "node:test";
import assert from "node:assert/strict";
import {execFileSync} from "node:child_process";
import {mkdtemp,readFile,writeFile,mkdir,rm,realpath} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join,resolve} from "node:path";
import {fileURLToPath} from "node:url";
import {decodePage,validateExports} from "../../web/bridge.mjs";
const root=fileURLToPath(new URL("../../",import.meta.url));
const cli=resolve(process.env.RUVANE_CLI||join(root,"target/debug/ruvane"));
const run=(args,cwd)=>execFileSync(cli,args,{cwd,encoding:"utf8",timeout:120000,maxBuffer:2*1024*1024,stdio:["ignore","pipe","pipe"]});
function artifact(bytes){
  assert.equal(bytes.subarray(0,4).toString(),"RPR1");let offset=12;
  const count=bytes.readUInt32LE(offset);offset+=4;
  const field=()=>{const size=bytes.readUInt32LE(offset);offset+=4;const value=bytes.subarray(offset,offset+size);offset+=size;return value;};
  const rows=[];for(let n=0;n<count;n++){const name=field().toString(),url=field().toString(),packet=field(),html=field().toString();rows.push({name,url,packet,html,page:decodePage(new Uint8Array(packet))});}
  assert.equal(offset+8,bytes.length);return {revision:bytes.readBigUInt64LE(4),rows};
}
function probe(e,bytes){
  assert.equal(e.ruvane_page_begin(bytes.length),1);
  for(let offset=0;offset<bytes.length;offset+=4){let word=0;const size=Math.min(4,bytes.length-offset);for(let n=0;n<size;n++)word|=bytes[offset+n]<<(8*n);assert.equal(e.ruvane_page_chunk(word>>>0,size),1);}
  return e.ruvane_page_validate();
}
await test("interactive production builds freeze explicit public URLs with the matching Wasm",async t=>{
  const directory=await mkdtemp(join(await realpath(tmpdir()),"ruvane-prerender-"));
  try{
    run(["new","frozen-app","--interactive","--file-routes","--framework-path",root],directory);
    const project=join(directory,"frozen-app"),pagesFile=join(project,"src/pages.rs"),sourceFile=join(project,"src/routes/frozen/[slug]/page.rs");
    await writeFile(pagesFile,(await readFile(pagesFile,"utf8")).replace("let mut app = App::new();",'let mut app = App::new(); app.client_metadata_origin(Some(ruvane::SiteOrigin::parse("https://public.example").unwrap()));'));
    await mkdir(join(project,"src/routes/frozen/[slug]"),{recursive:true});
    const source=`use crate::{components,pages};
use ruvane::{ActionData,ActionValue,ClientPageData,FilePage,PageParams};
pub fn page()->FilePage {
 FilePage::client("frozen",components::HOME,pages::zero,|context|{
  let phase=if context.is_exporting(){"Build"}else{"Runtime"};let slug=context.param("slug").unwrap();
  let mut data=ActionData::default();data.insert("count",ActionValue::Number(0)).unwrap();
  Ok(ClientPageData::new(format!("{phase} one {slug}"),data).metadata(pages::metadata(context,&format!("Public one {slug}"))?).fallback(ruvane::text(format!("Fallback {slug}"))))
 }).prerender(vec![PageParams::new(&[("slug","alpha")]).unwrap(),PageParams::new(&[("slug","beta")]).unwrap()])
}
`;
    await writeFile(sourceFile,source);run(["build","--offline"],project);
    const bundle=join(project,"dist/build-000001"),bytes=await readFile(join(bundle,"Ruvane.prerender")),plan=artifact(bytes);
    await t.test("build enumerates only declared slashless URLs with matching complete snapshots",()=>{
      assert.deepEqual(plan.rows.map(row=>row.url),["/frozen/alpha","/frozen/beta"]);
      for(const row of plan.rows){assert.equal(row.page.title,`Build one ${row.url.split("/").at(-1)}`);assert.equal(row.page.metadata.canonical,`https://public.example${row.url}`);assert.ok(row.html.includes("ruvane-initial"));assert.ok(row.html.includes("Fallback"));}
    });
    const manifest=await readFile(join(bundle,"Ruvane.bundle"),"utf8"),revision=manifest.match(/^revision\t([0-9]+)$/m)[1];
    const wasm=await readFile(join(bundle,"resources/ruvane-runtime",BigInt(revision).toString(16).padStart(16,"0"),"app.wasm"));
    const module=await WebAssembly.compile(wasm),e=validateExports(new WebAssembly.Instance(module,{}).exports);
    await t.test("pre-render packets validate in the same import-free Wasm without embedding native HTML",()=>{
      assert.equal(plan.revision,BigInt(revision));assert.deepEqual(WebAssembly.Module.imports(module),[]);
      for(const row of plan.rows){assert.equal(probe(e,row.packet),1);assert.equal(wasm.includes(Buffer.from(row.html)),false);}
      assert.equal(e.ruvane_live_instances(),0);
    });
    await t.test("the final executable contains the exact verified artifact",async()=>{
      const output=join(directory,"checked.bin"),executable=join(bundle,process.platform==="win32"?"ruvane-app.exe":"ruvane-app");
      execFileSync(executable,["--ruvane-prerender-verify",output],{cwd:project,timeout:10000});assert.deepEqual(await readFile(output),bytes);
    });
    await writeFile(sourceFile,source.replaceAll(" one {slug}"," two {slug}"));run(["build","--offline"],project);
    const second=artifact(await readFile(join(project,"dist/build-000002/Ruvane.prerender")));
    await t.test("a rebuild replaces the data and revision while retaining the earlier immutable bundle",async()=>{
      assert.notEqual(second.revision,plan.revision);assert.equal(second.rows[0].page.title,"Build two alpha");assert.equal(second.rows[0].page.metadata.description,"Public two alpha");assert.deepEqual(await readFile(join(bundle,"Ruvane.prerender")),bytes);
    });
    const current=await readFile(join(project,"dist/current"),"utf8");
    await t.test("a no_store loader cannot publish a replacement build",async()=>{
      await writeFile(sourceFile,source.replace('data).metadata','data).no_store().metadata'));
      assert.throws(()=>run(["build","--offline"],project));assert.equal(await readFile(join(project,"dist/current"),"utf8"),current);
    });
    await writeFile(sourceFile,source);
    const buildFile=join(project,"build.rs"),buildSource=await readFile(buildFile,"utf8");
    await t.test("corrupt intermediate artifacts fail before publication",async()=>{
      await writeFile(buildFile,buildSource.replace('    ruvane::ClientPrerender::write_build_module()?;', '    if let Some(path)=std::env::var_os("RUVANE_PRERENDER_FILE"){std::fs::write(path,b"damaged")?;}\n    ruvane::ClientPrerender::write_build_module()?;'));
      assert.throws(()=>run(["build","--offline"],project));assert.equal(await readFile(join(project,"dist/current"),"utf8"),current);
    });
    await writeFile(buildFile,buildSource);
    await t.test("unknown capability markers cannot replace a valid bundle",async()=>{
      await writeFile(join(project,"Ruvane.prerender"),"unknown\n");assert.throws(()=>run(["build","--offline"],project));assert.equal(await readFile(join(project,"dist/current"),"utf8"),current);
    });
  }finally{await rm(directory,{recursive:true,force:true});}
});

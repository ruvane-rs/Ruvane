import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtemp, readFile, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { validateExports, decodeCommands } from "../../web/bridge.mjs";
const root = fileURLToPath(new URL("../../", import.meta.url));
const cli = resolve(process.env.RUVANE_CLI || join(root,"target/debug/ruvane"));
const run = (file, args, cwd) => execFileSync(file, args, { cwd, encoding: "utf8", timeout: 120000, maxBuffer: 2 * 1024 * 1024 });
const word = value => [value & 255, value >>> 8 & 255, value >>> 16 & 255, value >>> 24 & 255];
function packet(url, component, value, status = 200, empty = false) {
  const text = value => { const bytes = new TextEncoder().encode(value); return [...word(bytes.length),...bytes]; };
  const seed = empty ? [82,86,65,49,0,0,0] : [82,86,65,49,0,1,0,5,99,111,117,110,116,2,0,0,0,0,0,0,0,0];
  return new Uint8Array([82,86,80,49,status & 255,status >>> 8,...text(url),...text("Page"),...word(component),...word(1),...word(value),...word(seed.length),...seed]);
}
await test("a generated application ships an independent, revision-matched Wasm registry", async t => {
  const directory = await mkdtemp(join(await realpath(tmpdir()),"ruvane-application-"));
  try {
    run(cli,["new","application-fixture","--interactive","--framework-path",root],directory);
    const project = join(directory,"application-fixture");
    run(cli,["build","--offline"],project);
    const bundle = join(project,"dist/build-000001"), manifest = await readFile(join(bundle,"Ruvane.bundle"),"utf8");
    const revision = manifest.match(/^revision\t([0-9]+)$/m)[1];
    assert.equal(await readFile(join(bundle,"complete"),"utf8"),"ruvane-application-complete\t1\n");
    const file = join(bundle,"resources/ruvane-runtime",BigInt(revision).toString(16).padStart(16,"0"),"app.wasm");
    const module = await WebAssembly.compile(await readFile(file));
    assert.deepEqual(WebAssembly.Module.imports(module),[]);
    const e = validateExports(new WebAssembly.Instance(module,{}).exports);
    assert.equal(e.ruvane_application_revision(),BigInt(revision));
    assert.equal(WebAssembly.Module.exports(module).filter(e=>e.kind === "function").length,22);
    const drain = () => { const commands = decodeCommands(new Uint8Array(e.memory.buffer,e.ruvane_commands_ptr() >>> 0,e.ruvane_commands_len() >>> 0).slice()); assert.equal(e.ruvane_commands_clear(),1); return commands; };
    const stage = (bytes,probe=false) => { assert.equal(e.ruvane_page_begin(bytes.length),1); for (let offset=0;offset<bytes.length;offset+=4) { const count=Math.min(4,bytes.length-offset); let word=0;for(let i=0;i<count;i++)word|=bytes[offset+i]<<(i*8);assert.equal(e.ruvane_page_chunk(word >>> 0,count),1); } return probe ? e.ruvane_page_validate() : e.ruvane_page_mount(2); };
    await t.test("the app owns its components instead of using the repository demo", () => {
      const id=e.ruvane_mount(1,1,1,5); assert.ok(id); const initial=drain();
      assert.ok(initial.some(c=>c.op===2 && c.text==="Layout counter")); assert.ok(!initial.some(c=>c.op===2 && c.text==="Ruvane counter"));
      const binding=initial.find(c=>c.op===3); assert.equal(e.ruvane_dispatch(id,binding.node,binding.event,0,0),1); assert.ok(drain().some(c=>c.op===2 && c.text==="6"));
      assert.equal(e.ruvane_dispose(id),1);drain();
    });
    await t.test("shared App declarations validate page ownership, state and typed error seeds", () => {
      for(const bytes of [packet("/",9,0),packet("/tasks/01",10,1),packet("/other",8,0)]) { assert.equal(stage(bytes),0);assert.equal(e.ruvane_last_error(),11);assert.equal(e.ruvane_live_instances(),0); }
      const page=stage(packet("/",8,0));assert.ok(page);assert.ok(drain().some(c=>c.op===12 && c.url==="/tasks"));assert.equal(e.ruvane_dispose(page),1);drain();
      const error=stage(packet("/unknown",11,404,404,true));assert.ok(error);assert.ok(drain().some(c=>c.op===2 && c.text==="Page not found"));assert.equal(e.ruvane_dispose(error),1);drain();assert.equal(e.ruvane_live_instances(),0);
    });
    await t.test("nested packets validate layout ownership and mount selected segments", () => {
      const text=value=>{const bytes=new TextEncoder().encode(value);return [...word(bytes.length),...bytes];};
      const branch=(name="workspace",key="/tasks")=>{
        const leaf=packet("/tasks",9,0),empty=[82,86,65,49,0,0,0];
        return new Uint8Array([82,86,80,50,0,...leaf.slice(4),1,...text(name),...text(key),...word(12),...word(1),...word(0),...word(empty.length),...empty]);
      };
      for(const bytes of [packet("/tasks",9,0),branch("forged"),branch("workspace","/other")]){assert.equal(stage(bytes),0);assert.equal(e.ruvane_last_error(),11);assert.equal(e.ruvane_live_instances(),0);}
      const bytes=branch();for(let n=0;n<1000;n++){assert.equal(stage(bytes,true),1);assert.equal(e.ruvane_live_instances(),0);assert.deepEqual(drain(),[]);}
      bytes[4]=1;const layout=stage(bytes);assert.ok(layout);assert.ok(drain().some(c=>c.op===2&&c.text==="Tasks workspace"));assert.equal(e.ruvane_dispose(layout),1);drain();
      bytes[4]=0;const leaf=stage(bytes);assert.ok(leaf);assert.ok(drain().some(c=>c.op===2&&c.text==="Server tasks / Rust + Wasm"));assert.equal(e.ruvane_dispose(leaf),1);drain();
      bytes[4]=2;assert.equal(stage(bytes),0);assert.equal(e.ruvane_last_error(),11);assert.equal(e.ruvane_live_instances(),0);
    });
    await t.test("read-only presentation mounts and typed failures share the application schema",()=>{
      const loading=e.ruvane_boundary_mount(8,15,1,0);assert.ok(loading);assert.ok(drain().some(c=>c.op===2&&c.text==="Loading page…"));assert.equal(e.ruvane_dispose(loading),1);drain();
      assert.equal(e.ruvane_boundary_mount(8,12,1,0),0);assert.equal(e.ruvane_last_error(),8);assert.equal(e.ruvane_live_instances(),0);drain();
      const text=value=>{const bytes=new TextEncoder().encode(value);return [...word(bytes.length),...bytes];};
      const leaf=packet("/tasks/project",16,500,500,true),empty=[82,86,65,49,0,0,0];
      const project=packet("/tasks/project",17,0),normal=new Uint8Array([82,86,80,50,0,...project.slice(4),1,...text("workspace"),...text("/tasks"),...word(12),...word(1),...word(0),...word(empty.length),...empty]);
      const ready=stage(normal);assert.ok(ready);assert.ok(drain().some(c=>c.op===2&&c.text==="Project overview"));assert.equal(e.ruvane_dispose(ready),1);drain();
      const bytes=new Uint8Array([82,86,80,51,0,1,...leaf.slice(4),1,...text("workspace"),...text("/tasks"),...word(12),...word(1),...word(0),...word(empty.length),...empty]);
      const page=stage(bytes);assert.ok(page);assert.ok(drain().some(c=>c.op===2&&c.text==="Page unavailable"));assert.equal(e.ruvane_dispose(page),1);drain();
      bytes[5]=0;assert.equal(stage(bytes),0);assert.equal(e.ruvane_last_error(),11);assert.equal(e.ruvane_live_instances(),0);
    });
    await t.test("streamed document errors validate the leaf boundary and keep owned parents",()=>{
      const text=value=>{const bytes=new TextEncoder().encode(value);return [...word(bytes.length),...bytes];};
      const leaf=packet("/tasks/live",16,500,500,true),empty=[82,86,65,49,0,0,0];
      const bytes=new Uint8Array([82,86,80,51,0,1,...leaf.slice(4),1,...text("workspace"),...text("/tasks"),...word(12),...word(1),...word(0),...word(empty.length),...empty]);
      for(let n=0;n<100;n++){assert.equal(stage(bytes,true),1);assert.equal(e.ruvane_live_instances(),0);assert.deepEqual(drain(),[]);}
      const error=stage(bytes);assert.ok(error);const commands=drain();assert.ok(commands.some(c=>c.op===2&&c.text==="Page unavailable"));assert.ok(!commands.some(c=>c.op===3||c.op===11));assert.equal(e.ruvane_dispose(error),1);drain();
      bytes[4]=1;const parent=stage(bytes);assert.ok(parent);assert.ok(drain().some(c=>c.op===2&&c.text==="Tasks workspace"));assert.equal(e.ruvane_dispose(parent),1);drain();
      bytes[4]=0;bytes[5]=0;assert.equal(stage(bytes,true),0);assert.equal(e.ruvane_live_instances(),0);drain();
    });
    await t.test("the generated Cargo graph has only project and framework packages", () => {
      const metadata=JSON.parse(run(process.env.CARGO || "cargo",["metadata","--offline","--locked","--format-version","1"],project));
      assert.deepEqual(metadata.packages.map(p=>p.name).sort(),["application-fixture","ruvane","ruvane-application-browser","ruvane-browser"]);
      for(const pkg of metadata.packages)assert.equal(pkg.source,null);
    });
    await t.test("pending prefixes validate without a leaf seed and mount only owned layouts",()=>{
      const text=value=>{const bytes=new TextEncoder().encode(value);return [...word(bytes.length),...bytes];};
      const pending=(expected=1,name="workspace",key="/tasks",url="/tasks/project",count=1)=>{
        const leaf=packet(url,17,0,200,true),empty=[82,86,65,49,0,0,0];
        return new Uint8Array([82,86,80,52,0,expected,...leaf.slice(4),count,...(count?[...text(name),...text(key),...word(12),...word(1),...word(0),...word(empty.length),...empty]:[])]);
      };
      for(const bytes of [pending(2),pending(1,"forged"),pending(1,"workspace","/other"),pending(1,"workspace","/tasks","/tasks")]){assert.equal(stage(bytes,true),0);assert.equal(e.ruvane_last_error(),11);assert.equal(e.ruvane_live_instances(),0);drain();}
      const manifest=pending(1,"workspace","/tasks","/tasks/project",0);assert.equal(stage(manifest,true),1);assert.equal(stage(manifest),0);assert.equal(e.ruvane_last_error(),11);drain();
      const bytes=pending();for(let n=0;n<1000;n++){assert.equal(stage(bytes,true),1);assert.equal(e.ruvane_live_instances(),0);assert.deepEqual(drain(),[]);}
      assert.equal(stage(bytes),0);assert.equal(e.ruvane_last_error(),11);drain();
      bytes[4]=1;const layout=stage(bytes);assert.ok(layout);assert.ok(drain().some(c=>c.op===2&&c.text==="Tasks workspace"));assert.equal(e.ruvane_dispose(layout),1);drain();
      bytes[4]=2;assert.equal(stage(bytes),0);assert.equal(e.ruvane_live_instances(),0);drain();
    });
  } finally { await rm(directory,{recursive:true,force:true}); }
});

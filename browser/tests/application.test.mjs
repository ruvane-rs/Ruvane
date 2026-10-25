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
    assert.equal(WebAssembly.Module.exports(module).filter(e=>e.kind === "function").length,20);
    const drain = () => { const commands = decodeCommands(new Uint8Array(e.memory.buffer,e.ruvane_commands_ptr() >>> 0,e.ruvane_commands_len() >>> 0).slice()); assert.equal(e.ruvane_commands_clear(),1); return commands; };
    const stage = bytes => { assert.equal(e.ruvane_page_begin(bytes.length),1); for (let offset=0;offset<bytes.length;offset+=4) { const count=Math.min(4,bytes.length-offset); let word=0;for(let i=0;i<count;i++)word|=bytes[offset+i]<<(i*8);assert.equal(e.ruvane_page_chunk(word >>> 0,count),1); } return e.ruvane_page_mount(2); };
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
    await t.test("the generated Cargo graph has only project and framework packages", () => {
      const metadata=JSON.parse(run(process.env.CARGO || "cargo",["metadata","--offline","--locked","--format-version","1"],project));
      assert.deepEqual(metadata.packages.map(p=>p.name).sort(),["application-fixture","ruvane","ruvane-application-browser","ruvane-browser"]);
      for(const pkg of metadata.packages)assert.equal(pkg.source,null);
    });
  } finally { await rm(directory,{recursive:true,force:true}); }
});

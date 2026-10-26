import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtemp, readFile, mkdir, writeFile, rm, realpath } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { validateExports } from "../../web/bridge.mjs";

const root = fileURLToPath(new URL("../../", import.meta.url));
const cli = resolve(process.env.RUVANE_CLI || join(root, "target/debug/ruvane"));
const run = (args, cwd) => execFileSync(cli, args, { cwd, encoding: "utf8", timeout: 120000, maxBuffer: 2 * 1024 * 1024 });
const word = value => [value & 255, value >>> 8 & 255, value >>> 16 & 255, value >>> 24 & 255];
const text = value => { const bytes = new TextEncoder().encode(value); return [...word(bytes.length), ...bytes]; };
const empty = [82,86,65,49,0,0,0], count = [82,86,65,49,0,1,0,5,99,111,117,110,116,2,0,0,0,0,0,0,0,0];
function packet(url, component, state = 0, layouts = []) {
  return new Uint8Array([82,86,80,layouts.length ? 50 : 49,...(layouts.length ? [0] : []),200,0,...text(url),...text("File route"),...word(component),...word(1),...word(state),...word(count.length),...count,...(layouts.length ? [layouts.length, ...layouts.flatMap(([name,key,id,value]) => [...text(name),...text(key),...word(id),...word(1),...word(value),...word(empty.length),...empty])] : [])]);
}
async function exportsFor(project, version) {
  const bundle = join(project, "dist", `build-${String(version).padStart(6,"0")}`);
  const manifest = await readFile(join(bundle, "Ruvane.bundle"), "utf8"), revision = manifest.match(/^revision\t([0-9]+)$/m)[1];
  const module = await WebAssembly.compile(await readFile(join(bundle,"resources/ruvane-runtime",BigInt(revision).toString(16).padStart(16,"0"),"app.wasm")));
  assert.deepEqual(WebAssembly.Module.imports(module), []);
  const e = validateExports(new WebAssembly.Instance(module, {}).exports); assert.equal(e.ruvane_application_revision(), BigInt(revision));
  return e;
}
function probe(e, bytes) {
  assert.equal(e.ruvane_page_begin(bytes.length), 1);
  for (let offset = 0; offset < bytes.length; offset += 4) { const size = Math.min(4, bytes.length-offset); let value = 0; for (let n = 0; n < size; n++) value |= bytes[offset+n] << (n*8); assert.equal(e.ruvane_page_chunk(value >>> 0, size),1); }
  const result = e.ruvane_page_validate(); assert.equal(e.ruvane_live_instances(),0); return result;
}

await test("generated file routes own the same Wasm registry across additions and removals", async t => {
  const directory = await mkdtemp(join(await realpath(tmpdir()), "ruvane-file-routes-"));
  try {
    run(["new","file-application","--interactive","--file-routes","--framework-path",root], directory);
    const project = join(directory,"file-application"); run(["build","--offline"],project);
    const e = await exportsFor(project,1);
    await t.test("fixed and inherited route metadata is validated without creating owners", () => {
      assert.equal(probe(e,packet("/",8)),1);
      assert.equal(probe(e,packet("/tasks/settings",14,0,[["workspace","/tasks",12,0]])),1);
      for (const bytes of [packet("/tasks/settings",14),packet("/tasks/settings",10,0,[["workspace","/tasks",12,0]]),packet("/tasks/01",10,1,[["workspace","/tasks",12,0],["context","/tasks/01",13,1]]),packet("/added",8)]) assert.equal(probe(e,bytes),0);
    });
    const added = join(project,"src/routes/added"); await mkdir(added);
    await writeFile(join(added,"page.rs"),'pub fn page() -> ruvane::FilePage { ruvane::FilePage::client("added", crate::components::HOME, crate::pages::zero, |_| Ok(ruvane::ClientPageData::new("Added", crate::store::snapshot()?))) }\n');
    run(["build","--offline"],project); const updated = await exportsFor(project,2);
    await t.test("a newly generated page is accepted only by the new registry", () => { assert.equal(probe(e,packet("/added",8)),0); assert.equal(probe(updated,packet("/added",8)),1); assert.notEqual(e.ruvane_application_revision(),updated.ruvane_application_revision()); });
    await rm(join(added,"page.rs")); run(["build","--offline"],project); const removed = await exportsFor(project,3);
    await t.test("removed files retire their Wasm route ownership", () => { assert.equal(probe(removed,packet("/added",8)),0); assert.equal(probe(removed,packet("/",8)),1); });
  } finally { await rm(directory,{recursive:true,force:true}); }
});

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
function failure(url, component, layouts, failed = layouts.length, status = 500) {
  return new Uint8Array([82,86,80,51,0,failed,status & 255,status >>> 8,...text(url),...text("File error"),...word(component),...word(1),...word(status),...word(empty.length),...empty,layouts.length,...layouts.flatMap(([name,key,id,value]) => [...text(name),...text(key),...word(id),...word(1),...word(value),...word(empty.length),...empty])]);
}
function missing(url, component) {
  return new Uint8Array([82,86,80,49,148,1,...text(url),...text("Missing"),...word(component),...word(1),...word(404),...word(empty.length),...empty]);
}
function withMetadata(packet, canonical = "https://public.example/", robots = 1) {
  const fields=[7,...text("Public <metadata>"),robots,...text(canonical)];
  return new Uint8Array([82,86,80,53,...word(packet.length),...packet,...fields]);
}
async function exportsFor(project, version) {
  const bundle = join(project, "dist", `build-${String(version).padStart(6,"0")}`);
  const manifest = await readFile(join(bundle, "Ruvane.bundle"), "utf8"), revision = manifest.match(/^revision\t([0-9]+)$/m)[1];
  const wasm = await readFile(join(bundle,"resources/ruvane-runtime",BigInt(revision).toString(16).padStart(16,"0"),"app.wasm"));
  assert.equal(wasm.includes(Buffer.from("SERVER_ROUTE_ONLY_ALPHA38")),false);
  const module = await WebAssembly.compile(wasm);
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
    const project = join(directory,"file-application");
    const pagesFile = join(project,"src/pages.rs");
    await writeFile(pagesFile,(await readFile(pagesFile,"utf8")).replace("let mut app = App::new();",'let mut app = App::new(); app.server_cache(ruvane::ServerCache::default()); app.client_metadata_origin(Some(ruvane::SiteOrigin::parse("https://public.example").unwrap()));'));
    const homeFile=join(project,"src/routes/page.rs");
    await writeFile(homeFile,(await readFile(homeFile,"utf8")).replace('    })','    }).server_cache(ruvane::ServerCachePolicy::public(30).unwrap().tag("home").unwrap())'));
    const nativeRoute = '#[cfg(target_family = "wasm")] compile_error!("route.rs reached Wasm");\npub fn route() -> ruvane::FileEndpoint { ruvane::FileEndpoint::new().get(|_| ruvane::Response::text("SERVER_ROUTE_ONLY_ALPHA38")) }\n';
    await writeFile(join(project,"src/routes/api/health/route.rs"),nativeRoute);
    run(["build","--offline"],project);
    const e = await exportsFor(project,1);
    await t.test("server cache declarations retain the import-free Wasm registry", () => {
      assert.equal(probe(e,packet("/",8)),1);
      assert.equal(e.ruvane_live_instances(),0);
    });
    await t.test("fixed and inherited route metadata is validated without creating owners", () => {
      assert.equal(probe(e,packet("/",8)),1);
      assert.equal(probe(e,packet("/tasks/settings",14,0,[["workspace","/tasks",12,0]])),1);
      for (const bytes of [packet("/tasks/settings",14),packet("/tasks/settings",10,0,[["workspace","/tasks",12,0]]),packet("/tasks/01",10,1,[["workspace","/tasks",12,0],["context","/tasks/01",13,1]]),packet("/added",8)]) assert.equal(probe(e,bytes),0);
    });
    await t.test("native route modules are excluded from Wasm compilation and client page ownership", () => {
      assert.equal(probe(e,packet("/api/health",8)),0);
      assert.equal(probe(e,packet("/actions/tasks/create",8)),0);
    });
    await t.test("metadata envelopes validate the configured canonical origin and selected layouts in Wasm", () => {
      assert.equal(probe(e,withMetadata(packet("/",8))),1);
      assert.equal(probe(e,withMetadata(packet("/",8),"https://other.example/")),0);
      const selected=withMetadata(packet("/tasks/settings",14,0,[["workspace","/tasks",12,0]]));selected[12]=1;
      assert.equal(probe(e,selected),1);
      const nested=withMetadata(withMetadata(packet("/",8)));assert.equal(probe(e,nested),0);
    });
    const parents = [["workspace","/tasks",12,0],["context","/tasks/999",13,999]];
    await t.test("inherited file errors validate their exact component and retained scope", () => {
      assert.equal(probe(e,failure("/tasks/999",16,parents)),1);
      assert.equal(probe(e,failure("/tasks/999",19,parents)),0);
      assert.equal(probe(e,failure("/tasks/999",16,parents,1)),0);
      assert.equal(probe(e,failure("/tasks/999",16,[],2)),0);
      const owner = e.ruvane_boundary_mount(8,15,1,0); assert.ok(owner); e.ruvane_commands_clear();
      assert.equal(e.ruvane_dispose(owner),1); e.ruvane_commands_clear(); assert.equal(e.ruvane_live_instances(),0);
    });
    await t.test("file 404s validate the dedicated component, status and complete parent scope", () => {
      assert.equal(probe(e,failure("/tasks/999",21,parents,2,404)),1);
      for (const bytes of [failure("/tasks/999",20,parents,2,404),failure("/tasks/999",16,parents,2,404),failure("/tasks/999",21,parents,2,500),failure("/tasks/999",21,[],2,404),missing("/tasks/999",20)]) assert.equal(probe(e,bytes),0);
      assert.equal(probe(e,missing("/unknown",20)),1);
      assert.equal(probe(e,missing("/tasks/no/such/route",20)),1);
      assert.equal(probe(e,missing("/unknown",21)),0);
      assert.equal(probe(e,missing("/unknown",11)),0);
    });
    const notFoundOverride = join(project,"src/routes/tasks/[id]/not_found.rs");
    await writeFile(notFoundOverride,'pub fn not_found() -> ruvane::FileNotFound { ruvane::FileNotFound::new(crate::components::GLOBAL_NOT_FOUND) }\n');
    const override = join(project,"src/routes/tasks/[id]/error.rs");
    await writeFile(override,'pub fn error() -> ruvane::FileError { ruvane::FileError::new(crate::components::GLOBAL_ERROR) }\n');
    const added = join(project,"src/routes/added"); await mkdir(added);
    await writeFile(join(added,"page.rs"),'pub fn page() -> ruvane::FilePage { ruvane::FilePage::client("added", crate::components::HOME, crate::pages::zero, |_| Ok(ruvane::ClientPageData::new("Added", crate::store::snapshot()?))) }\n');
    run(["build","--offline"],project); const updated = await exportsFor(project,2);
    await t.test("a newly generated page is accepted only by the new registry", () => { assert.equal(probe(e,packet("/added",8)),0); assert.equal(probe(updated,packet("/added",8)),1); assert.notEqual(e.ruvane_application_revision(),updated.ruvane_application_revision()); });
    await t.test("a single error override inherits loading and replaces Wasm error ownership", () => {
      assert.equal(probe(updated,failure("/tasks/999",19,parents)),1);
      assert.equal(probe(updated,failure("/tasks/999",16,parents)),0);
    });
    await t.test("a local not-found override changes only 404 ownership", () => {
      assert.equal(probe(updated,failure("/tasks/999",20,parents,2,404)),1);
      assert.equal(probe(updated,failure("/tasks/999",21,parents,2,404)),0);
      assert.equal(probe(updated,failure("/tasks/999",19,parents)),1);
      assert.equal(probe(updated,missing("/unknown",20)),1);
    });
    await rm(notFoundOverride);
    await rm(join(project,"src/routes/not_found.rs"));
    await rm(join(project,"src/routes/tasks/not_found.rs"));
    await rm(override);
    await rm(join(project,"src/routes/tasks/loading.rs"));
    await rm(join(project,"src/routes/tasks/error.rs"));
    await rm(join(added,"page.rs")); run(["build","--offline"],project); const removed = await exportsFor(project,3);
    await t.test("removed files retire their Wasm route ownership", () => { assert.equal(probe(removed,packet("/added",8)),0); assert.equal(probe(removed,packet("/",8)),1); });
    await t.test("removing file screens restores the explicit App fallback without forged parents", () => {
      assert.equal(probe(removed,failure("/tasks/999",19,[],2)),1);
      assert.equal(probe(removed,failure("/tasks/999",19,parents)),0);
      assert.equal(probe(removed,failure("/tasks/999",16,parents)),0);
    });
    await t.test("removing not-found files restores generic unknown-URL ownership", () => {
      assert.equal(probe(removed,missing("/unknown",11)),1);
      assert.equal(probe(removed,missing("/unknown",20)),0);
      assert.equal(probe(removed,failure("/tasks/999",21,parents,2,404)),0);
    });
    await rm(join(project,"src/routes"),{recursive:true});
    await mkdir(join(project,"src/routes/api/health"),{recursive:true});
    await writeFile(join(project,"src/routes/api/health/route.rs"),nativeRoute);
    run(["build","--offline"],project); const apiOnly = await exportsFor(project,4);
    await t.test("an API-only tree builds natively and leaves no interactive page owners in Wasm", () => {
      assert.equal(probe(apiOnly,packet("/",8)),0);
      assert.equal(probe(apiOnly,packet("/api/health",8)),0);
    });
  } finally { await rm(directory,{recursive:true,force:true}); }
});

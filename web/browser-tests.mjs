import { createRuntime, decodeCommands, validateExports } from "/assets/ruvane.js";

const results = [];
const report = document.getElementById("report");
const equal = (a, b, message) => { if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(message); };
const assert = (value, message) => { if (!value) throw new Error(message); };
async function test(name, body) {
  try { await body(); results.push({ name, passed: true }); }
  catch (error) { results.push({ name, passed: false, error: String(error.message) }); }
  report.textContent = JSON.stringify(results, null, 2);
}
const rejects = async body => {
  let rejected = false;
  try { await body(); } catch { rejected = true; }
  assert(rejected, "expected rejection");
};
const emptyRoot = () => { const root = document.createElement("div"); document.body.appendChild(root); return root; };

await test("real Wasm exports, ABI and invalid raw identifiers", async () => {
  const { instance } = await WebAssembly.instantiate(await (await fetch("/assets/counter.wasm")).arrayBuffer(), {});
  const e = validateExports(instance.exports);
  equal(e.ruvane_abi_version(), 1, "ABI");
  equal(e.ruvane_mount(0), 0, "invalid root");
  equal(e.ruvane_last_error(), 1, "root diagnostic");
  const id = e.ruvane_mount(1);
  assert(id > 0, "mount"); e.ruvane_commands_clear();
  equal(e.ruvane_dispatch(id, 999), 0, "invalid event");
  equal(e.ruvane_last_error(), 4, "event diagnostic");
  equal(e.ruvane_dispatch(999999, 1), 0, "invalid view");
  equal(e.ruvane_dispose(id), 1, "dispose"); e.ruvane_commands_clear();
  equal(e.ruvane_dispatch(id, 1), 0, "stale view");
  equal([e.ruvane_live_instances(), e.ruvane_live_signals(), e.ruvane_live_effects()], [0, 0, 0], "native cleanup");
  await rejects(() => validateExports({ ...e, ruvane_abi_version: () => 9 }));
});
await test("counter logic, derived value, reset and native keyboard controls", async () => {
  const runtime = await createRuntime("/assets/counter.wasm");
  const root = emptyRoot();
  try {
    const view = runtime.mount(root);
    const [inc, dec, reset] = root.querySelectorAll("button");
    const values = () => [...root.querySelectorAll("output")].map(n => n.textContent);
    equal(values(), ["0", "0"], "initial");
    assert(dec.disabled && reset.disabled, "initial disabled states");
    inc.focus(); equal(document.activeElement, inc, "native button focus");
    inc.click(); inc.click(); dec.click();
    equal(values(), ["1", "2"], "Rust-derived values");
    reset.click(); equal(values(), ["0", "0"], "reset");
    for (let n = 0; n < 1100; n++) view.dispatch(1);
    equal(values(), ["1000", "2000"], "bounded arithmetic");
    assert(inc.disabled, "maximum disabled state");
    await rejects(() => view.dispatch(999));
    view.dispatch(3); equal(values(), ["0", "0"], "invalid event did not stop valid view");
    equal(runtime.stats().eventListeners, 3, "listener count");
  } finally { runtime.dispose(); root.remove(); }
});
await test("1000 mount/unmount cycles release handles, events and reactive owners", async () => {
  const runtime = await createRuntime("/assets/counter.wasm");
  const root = emptyRoot();
  let stale;
  try {
    for (let n = 0; n < 1000; n++) {
      const view = runtime.mount(root);
      if (!stale) stale = view;
      root.querySelector("button").click();
      assert(view.dispose(), "first dispose");
      assert(!view.dispose(), "idempotent controller dispose");
      equal(runtime.stats(), { state: "active", views: 0, domHandles: 0, eventListeners: 0, rustInstances: 0, rustSignals: 0, rustEffects: 0 }, "no retained resources");
    }
    const view = runtime.mount(root);
    await rejects(() => stale.dispatch(1));
    equal(root.querySelector("output").textContent, "0", "stale controller did not alias new view");
    view.dispose();
  } finally { runtime.dispose(); root.remove(); }
});
await test("view isolation, root validation and simultaneous instance budget", async () => {
  const runtime = await createRuntime("/assets/counter.wasm");
  const roots = [];
  try {
    for (let n = 0; n < 16; n++) { const root = emptyRoot(); roots.push(root); runtime.mount(root); }
    roots[0].querySelector("button").click();
    equal(roots[1].querySelector("output").textContent, "0", "isolated state");
    const overflow = emptyRoot(); roots.push(overflow);
    await rejects(() => runtime.mount(overflow));
    await rejects(() => runtime.mount(roots[0]));
    equal(runtime.stats().views, 16, "invalid mount preserves existing views");
  } finally { runtime.dispose(); roots.forEach(root => root.remove()); }
  equal(runtime.stats().rustInstances, 0, "all native instances disposed");
});
await test("DOM ownership violation stops the runtime and disposes all views", async () => {
  const runtime = await createRuntime("/assets/counter.wasm");
  const root = emptyRoot();
  const outside = emptyRoot();
  try {
    const view = runtime.mount(root);
    const foreign = document.createElement("p");
    foreign.textContent = "Host-owned sibling";
    root.appendChild(foreign);
    outside.appendChild(root.querySelector("button"));
    outside.querySelector("button").click();
    equal(runtime.stats(), { state: "faulted", views: 0, domHandles: 0, eventListeners: 0, rustInstances: 0, rustSignals: 0, rustEffects: 0 }, "fault cleanup");
    assert(foreign.parentNode === root, "host-owned nodes are preserved");
    await rejects(() => view.dispatch(1));
    outside.querySelector("button").click();
    equal(runtime.stats().rustInstances, 0, "old button cannot reactivate native state");
  } finally { runtime.dispose(); root.remove(); outside.remove(); }
});
await test("wrong MIME, ABI version and cross-origin module are rejected", async () => {
  await rejects(() => createRuntime("/assets/ruvane.js"));
  await rejects(() => createRuntime("/assets/wrong-version.wasm"));
  await rejects(() => createRuntime("https://invalid.example/module.wasm"));
});
await test("truncated, oversized and unknown command frames fail before DOM access", async () => {
  const header = new Uint8Array([82, 86, 78, 49, 0, 0, 0, 0]);
  equal(decodeCommands(header), [], "empty batch");
  const bom = new TextEncoder().encode("\uFEFFtext");
  const frame = new Uint8Array(21 + bom.length);
  frame.set([82,86,78,49,1,0,0,0,2,1,0,0,0,1,0,0,0]);
  new DataView(frame.buffer).setUint32(17, bom.length, true);
  frame.set(bom, 21);
  equal(decodeCommands(frame)[0].text, "\uFEFFtext", "UTF-8 BOM is text data");
  for (const bytes of [header.slice(0, 7), new Uint8Array(65537), new Uint8Array([82,86,78,49,1,0,0,0,99,1,0,0,0]), new Uint8Array([...header, 0])]) {
    await rejects(() => decodeCommands(bytes));
  }
});
const passed = results.filter(result => result.passed).length;
window.__ruvane_browser_result = { passed, failed: results.length - passed, results, userAgent: navigator.userAgent };
document.getElementById("status").textContent = `${passed}/${results.length} browser tests passed`;
report.textContent = JSON.stringify(window.__ruvane_browser_result, null, 2);

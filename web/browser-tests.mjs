import { createRuntime, decodeCommands, validateExports, validateProps } from "/assets/ruvane.js";

const results = [], report = document.getElementById("report");
const equal = (a, b, message) => { if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(message); };
const assert = (value, message) => { if (!value) throw new Error(message); };
async function test(name, body) {
  try { await body(); results.push({ name, passed: true }); }
  catch (error) { results.push({ name, passed: false, error: String(error.message) }); }
  report.textContent = JSON.stringify(results, null, 2);
}
async function rejects(body) {
  let rejected = false;
  try { await body(); } catch { rejected = true; }
  assert(rejected, "expected rejection");
}
const counter = value => ({ component: 1, state: { kind: "number", value } });
const toggle = value => ({ component: 2, state: { kind: "boolean", value } });
const emptyRoot = () => { const root = document.createElement("div"); document.body.appendChild(root); return root; };
const serverRoot = (name = "counter") => { const root = document.querySelector(`#${name} > div`).cloneNode(true); document.body.appendChild(root); return root; };
const values = root => [...root.querySelectorAll("output")].map(node => node.textContent);
const zero = state => ({ state, views: 0, domHandles: 0, eventListeners: 0, rustInstances: 0, rustSignals: 0, rustEffects: 0 });
const load = () => createRuntime("/assets/app.wasm");

await test("real Wasm ABI, props, invalid events and stale handles", async () => {
  const { instance } = await WebAssembly.instantiate(await (await fetch("/assets/app.wasm")).arrayBuffer(), {});
  const e = validateExports(instance.exports);
  equal(e.ruvane_abi_version(), 3, "ABI");
  equal(e.ruvane_mount(0, 1, 1, 0), 0, "invalid root");
  equal(e.ruvane_last_error(), 1, "root error");
  equal(e.ruvane_mount(1, 2, 2, 2), 0, "invalid boolean");
  equal(e.ruvane_last_error(), 7, "props error");
  const id = e.ruvane_mount(1, 1, 1, 5);
  assert(id > 0, "mount");
  equal(e.ruvane_mount(2, 1, 1, 0), 0, "pending batch");
  equal(e.ruvane_last_error(), 5, "busy error"); e.ruvane_commands_clear();
  equal(e.ruvane_dispatch(id, 9, 999), 0, "invalid event");
  equal(e.ruvane_last_error(), 4, "event error");
  equal(e.ruvane_dispatch(999999, 9, 1), 0, "invalid view");
  equal(e.ruvane_dispose(id), 1, "dispose"); e.ruvane_commands_clear();
  equal(e.ruvane_dispatch(id, 9, 1), 0, "stale view");
  equal([e.ruvane_live_instances(), e.ruvane_live_signals(), e.ruvane_live_effects()], [0, 0, 0], "native cleanup");
  await rejects(() => validateExports({ ...e, ruvane_abi_version: () => 1 }));
});
await test("client mount, derived values, button bounds and event validation", async () => {
  const runtime = await load(), root = emptyRoot();
  try {
    const view = runtime.mount(root, counter(0));
    const [inc, dec, reset] = root.querySelectorAll("button");
    equal(values(root), ["0", "0"], "initial");
    assert(dec.disabled && reset.disabled, "disabled state");
    inc.focus(); assert(document.activeElement === inc, "button focus");
    inc.click(); inc.click(); dec.click(); equal(values(root), ["1", "2"], "derived value");
    reset.click(); equal(values(root), ["0", "0"], "reset");
    for (let n = 0; n < 1100; n++) view.dispatch(1);
    equal(values(root), ["1000", "2000"], "maximum"); assert(inc.disabled, "maximum disabled state");
    await rejects(() => view.dispatch(999)); view.dispatch(3);
    equal(values(root), ["0", "0"], "invalid event preserves view");
    equal(runtime.stats().eventListeners, 3, "listener count");
  } finally { runtime.dispose(); root.remove(); }
});
await test("1000 client mounts and 1000 hydrations release all resources", async () => {
  const runtime = await load();
  let stale;
  try {
    for (const hydrated of [false, true]) {
      for (let n = 0; n < 1000; n++) {
        const root = hydrated ? serverRoot() : emptyRoot();
        try {
          const view = hydrated ? runtime.hydrate(root) : runtime.mount(root, counter(0));
          if (!stale) stale = view;
          root.querySelector("button").click();
          assert(view.dispose(), "first dispose"); assert(!view.dispose(), "idempotent dispose");
          equal(root.childNodes.length, 0, "owned DOM removed");
          equal(runtime.stats(), zero("active"), "resources released");
        } finally { root.remove(); }
      }
    }
    const root = emptyRoot();
    try { runtime.mount(root, counter(0)); await rejects(() => stale.dispatch(1)); equal(values(root), ["0", "0"], "no stale alias"); }
    finally { runtime.dispose(); root.remove(); }
  } finally { runtime.dispose(); }
});
await test("two component types, independent state and instance budget", async () => {
  const runtime = await load(), roots = [];
  try {
    for (let n = 0; n < 16; n++) { const root = emptyRoot(); roots.push(root); runtime.mount(root, n % 2 ? toggle(true) : counter(0)); }
    roots[0].querySelector("button").click(); roots[1].querySelector("button").click();
    equal(values(roots[0]), ["1", "2"], "counter"); equal(values(roots[1]), ["Disabled"], "toggle event 7");
    equal(values(roots[2]), ["0", "0"], "independent counter"); equal(values(roots[3]), ["Enabled"], "independent toggle");
    const overflow = emptyRoot(); roots.push(overflow);
    await rejects(() => runtime.mount(overflow, counter(0)));
    await rejects(() => runtime.mount(roots[0], counter(0)));
    equal(runtime.stats().views, 16, "invalid mount preserves views");
  } finally { runtime.dispose(); roots.forEach(root => root.remove()); }
  equal(runtime.stats(), zero("disposed"), "all disposed");
});
await test("client DOM corruption stops the runtime and preserves host siblings", async () => {
  const runtime = await load(), root = emptyRoot(), outside = emptyRoot();
  try {
    const view = runtime.mount(root, counter(0));
    const foreign = document.createElement("p"); foreign.textContent = "Host sibling"; root.appendChild(foreign);
    outside.appendChild(root.querySelector("button")); outside.querySelector("button").click();
    equal(runtime.stats(), zero("faulted"), "fault cleanup"); assert(foreign.parentNode === root, "host nodes preserved");
    await rejects(() => view.dispatch(1)); outside.querySelector("button").click();
    equal(runtime.stats().rustInstances, 0, "old listener removed");
  } finally { runtime.dispose(); root.remove(); outside.remove(); }
});
await test("wrong MIME, old ABI and cross-origin module rejection", async () => {
  await rejects(() => createRuntime("/assets/ruvane.js"));
  await rejects(() => createRuntime("/assets/wrong-version.wasm"));
  await rejects(() => createRuntime("https://invalid.example/module.wasm"));
});
await test("strict command framing and typed mount props", async () => {
  const header = new Uint8Array([82, 86, 78, 50, 0, 0, 0, 0]);
  equal(decodeCommands(header), [], "empty batch");
  const bom = new TextEncoder().encode("\uFEFFtext"), frame = new Uint8Array(21 + bom.length);
  frame.set([82,86,78,50,1,0,0,0,2,1,0,0,0,1,0,0,0]);
  new DataView(frame.buffer).setUint32(17, bom.length, true); frame.set(bom, 21);
  equal(decodeCommands(frame)[0].text, "\uFEFFtext", "BOM preserved");
  for (const bytes of [header.slice(0, 7), new Uint8Array(65537), new Uint8Array([82,86,78,50,1,0,0,0,99,1,0,0,0]), new Uint8Array([...header, 0])]) await rejects(() => decodeCommands(bytes));
  for (const value of [-1, 1.5, NaN, Infinity, 0x100000000, "5"]) await rejects(() => validateProps(counter(value)));
  await rejects(() => validateProps({ component: 1, state: { kind: "boolean", value: 1 } }));
});
await test("server-only response contains initial HTML with scripts blocked", async () => {
  const response = await fetch("/ssr");
  assert(response.headers.get("content-security-policy").includes("script-src 'none'"), "static CSP");
  const document = new DOMParser().parseFromString(await response.text(), "text/html");
  equal(values(document.querySelector("#counter")), ["5", "10"], "SSR values before Wasm");
  equal(values(document.querySelector("#toggle")), ["Enabled"], "SSR boolean");
  assert([...document.querySelectorAll("button")].every(button => button.disabled), "SSR controls inert");
  equal(document.querySelectorAll("script").length, 0, "no scripts needed for content");
});
await test("hydration reuses elements and text nodes, preserves focus and starts at 5", async () => {
  const runtime = await load(), root = serverRoot();
  try {
    const elements = [...root.querySelectorAll("*")], textNodes = elements.flatMap(element => [...element.childNodes].filter(node => node.nodeType === 3));
    root.tabIndex = 0; root.focus();
    const observer = new MutationObserver(() => {}); observer.observe(root, { subtree: true, childList: true, characterData: true });
    const view = runtime.hydrate(root);
    const changes = observer.takeRecords(); observer.disconnect();
    equal(changes.length, 0, "no text or child replacement during hydration");
    assert(document.activeElement === root, "focus preserved");
    assert(elements.every((element, index) => element === root.querySelectorAll("*")[index]), "element identity");
    assert(textNodes.every(node => root.contains(node)), "text node identity");
    equal(values(root), ["5", "10"], "initial state not reset");
    root.querySelector("button").click(); equal(values(root), ["6", "12"], "hydrated event");
    view.dispatch(3); equal(values(root), ["0", "0"], "hydrated reset");
  } finally { runtime.dispose(); root.remove(); }
});
await test("boolean hydration and events are independent of the counter", async () => {
  const runtime = await load(), first = serverRoot(), second = serverRoot("toggle");
  try {
    runtime.hydrate(first); const view = runtime.hydrate(second);
    equal(values(second), ["Enabled"], "initial true"); view.dispatch(7);
    equal(values(second), ["Disabled"], "toggle event"); equal(values(first), ["5", "10"], "counter unaffected");
    await rejects(() => view.dispatch(1)); equal(runtime.stats().views, 2, "event rejection is local");
  } finally { runtime.dispose(); first.remove(); second.remove(); }
});
await test("mismatches preserve server HTML, roll back native state and isolate other views", async () => {
  const runtime = await load(), good = emptyRoot();
  const mutations = [
    root => root.querySelector("output").textContent = "wrong",
    root => root.querySelector("output").appendChild(document.createElement("span")),
    root => root.querySelector("h2").setAttribute("data-ruvane-node", "99"),
    root => root.querySelector("button").setAttribute("data-ruvane-click", "7"),
    root => root.querySelector("button").setAttribute("onclick", "return false"),
    root => root.querySelector("button").disabled = false,
    root => root.firstElementChild.replaceWith(document.createElement("p")),
    root => root.appendChild(document.createTextNode(" ")),
    root => root.querySelector("span").remove(),
    root => root.setAttribute("data-ruvane-version", "1"),
    root => root.setAttribute("data-ruvane-component", "99"),
    root => root.setAttribute("data-ruvane-state", "1:6"),
    root => root.setAttribute("data-ruvane-state", "1:1001"),
    root => root.setAttribute("data-ruvane-state", "1:05"),
    root => root.setAttribute("data-ruvane-state", "1:4294967296"),
    root => root.setAttribute("data-ruvane-state", "2:2"),
    root => root.setAttribute("data-ruvane-state", "2:1"),
    root => root.setAttribute("data-ruvane-state", "<script>"),
  ];
  try {
    runtime.mount(good, counter(0)); const baseline = runtime.stats();
    for (const mutate of mutations) {
      const root = serverRoot();
      try {
        mutate(root); const before = root.outerHTML;
        await rejects(() => runtime.hydrate(root));
        equal(root.outerHTML, before, "failed hydration leaves DOM untouched");
        equal(runtime.stats(), baseline, "failed hydration releases its native instance");
        const button = root.querySelector("button"); if (button) { button.disabled = false; button.click(); }
        equal(runtime.stats(), baseline, "no partial listener after failure");
      } finally { root.remove(); }
    }
    good.querySelector("button").click(); equal(values(good), ["1", "2"], "existing view still active");
  } finally { runtime.dispose(); good.remove(); }
});
await test("root claims prevent duplicate hydration across runtime instances", async () => {
  const runtime = await load(), other = await load(), root = serverRoot();
  try {
    runtime.hydrate(root);
    await rejects(() => runtime.hydrate(root)); await rejects(() => other.hydrate(root));
    equal(other.stats(), zero("active"), "second runtime did not allocate");
    root.querySelector("button").click(); equal(values(root), ["6", "12"], "one click produces one update");
    const nested = root.firstElementChild;
    await rejects(() => other.mount(nested, counter(0)));
  } finally { runtime.dispose(); other.dispose(); root.remove(); }
});
await test("post-hydration DOM failure preserves server content and removes all handlers", async () => {
  const runtime = await load(), root = serverRoot();
  try {
    runtime.hydrate(root);
    const foreign = document.createElement("p"); foreign.textContent = "Host sibling"; root.appendChild(foreign);
    const button = root.querySelector("button"); button.click();
    equal(runtime.stats(), zero("faulted"), "native state disposed");
    equal(values(root), ["5", "10"], "server content preserved");
    assert(foreign.parentNode === root, "host sibling preserved"); assert(button.disabled, "controls disabled");
    button.disabled = false; button.click(); equal(values(root), ["5", "10"], "handler removed");
  } finally { runtime.dispose(); root.remove(); }
});
const rows = root => [...root.querySelectorAll("li")];
const row = (root, key) => root.querySelector(`li[data-ruvane-key="${key}"]`);
const action = (root, label) => [...root.querySelectorAll("button")].find(button => button.textContent === label);

await test("keyed SSR hydrates without replacement and reorder preserves nodes, state and focus", async () => {
  const runtime = await load(), root = serverRoot("item-list");
  try {
    const initial = rows(root), labels = initial.map(node => node.firstChild);
    const view = runtime.hydrate(root);
    assert(rows(root).every((node, index) => node === initial[index]), "hydration identity");
    initial[0].querySelector("button").click();
    action(initial[1], "Remove").click();
    const focused = action(initial[0], "Increase"); focused.focus();
    view.dispatch(2);
    assert(rows(root)[0] === initial[2] && rows(root)[1] === initial[0], "C and A reused");
    assert(labels[0] === initial[0].firstChild && labels[2] === initial[2].firstChild, "labels kept");
    assert(document.activeElement === focused, "focus follows moved row");
    equal(values(initial[0]), ["1"], "item state retained");
    equal(runtime.stats().rustSignals, 4, "removed item's scopes disposed");
    action(initial[1], "Increase").click(); equal(values(initial[0]), ["1"], "removed button inert");
    await rejects(() => view.dispatch(24));
  } finally { runtime.dispose(); root.remove(); }
});
await test("filtering retires old bindings and restores item state with fresh DOM handles", async () => {
  const runtime = await load(), root = serverRoot("item-list");
  try {
    runtime.hydrate(root);
    const a = row(root, 1), b = row(root, 2), old = action(a, "Increase");
    old.click(); action(a, "Done").click(); action(root, "Hide done").click();
    assert(!root.contains(a) && row(root, 2) === b, "only hidden row removed");
    equal(runtime.stats().eventListeners, 9, "hidden row listeners removed");
    old.click();
    action(root, "Show all").click();
    const restored = row(root, 1);
    assert(restored !== a && restored.getAttribute("data-ruvane-node") !== a.getAttribute("data-ruvane-node"), "fresh handle after hide");
    equal(values(restored), ["1"], "hidden model preserved");
    old.click(); equal(values(restored), ["1"], "old listener stays detached");
    for (const item of [...rows(root)]) action(item, "Remove").click();
    assert(root.textContent.includes("No visible items."), "empty conditional branch");
    equal(runtime.stats().rustSignals, 0, "all item state released");
    action(root, "Add item").click();
    assert(!root.textContent.includes("No visible items."), "empty branch removed");
    equal(rows(root).length, 1, "new list item");
  } finally { runtime.dispose(); root.remove(); }
});
await test("1000 additions and removals return to the same live resource counts", async () => {
  const runtime = await load(), root = serverRoot("empty-list");
  try {
    const view = runtime.hydrate(root), baseline = runtime.stats();
    let lastHandle = 0;
    for (let n = 0; n < 1000; n++) {
      action(root, "Add item").click();
      const item = rows(root)[0], handle = Number(item.getAttribute("data-ruvane-node"));
      assert(handle > lastHandle, "node handles never reused"); lastHandle = handle;
      const old = action(item, "Increase"); old.click(); equal(values(item), ["1"], "new item state");
      const event = Number(old.getAttribute("data-ruvane-click"));
      action(item, "Remove").click();
      await rejects(() => view.dispatch(event)); old.click();
      equal(runtime.stats(), baseline, "no retained handles, listeners or signals");
      equal(rows(root).length, 0, "row removed");
    }
  } finally { runtime.dispose(); root.remove(); }
});
await test("key mismatches preserve SSR and leave existing components active", async () => {
  const runtime = await load(), good = emptyRoot();
  const mutations = [
    root => rows(root)[1].setAttribute("data-ruvane-key", "1"),
    root => rows(root)[0].removeAttribute("data-ruvane-key"),
    root => rows(root)[0].setAttribute("data-ruvane-key", "01"),
    root => root.querySelector("ul").appendChild(document.createElement("span")),
    root => root.querySelector("ul").insertBefore(rows(root)[2], rows(root)[0]),
    root => root.setAttribute("data-ruvane-state", "1:5"),
  ];
  try {
    runtime.mount(good, counter(0)); const baseline = runtime.stats();
    for (const mutate of mutations) {
      const root = serverRoot("item-list");
      try { mutate(root); const before = root.outerHTML; await rejects(() => runtime.hydrate(root)); equal(root.outerHTML, before, "SSR unchanged"); equal(runtime.stats(), baseline, "native rollback"); }
      finally { root.remove(); }
    }
    good.querySelector("button").click(); equal(values(good), ["1", "2"], "other view active");
  } finally { runtime.dispose(); good.remove(); }
});
await test("a valid text update followed by an invalid structural command applies no partial text", async () => {
  const runtime = await createRuntime("/assets/invalid-patch.wasm"), root = emptyRoot();
  try {
    const view = runtime.mount(root, counter(0));
    const button = root.querySelector("button"), text = button.firstChild;
    await rejects(() => view.dispatch(1));
    equal(button.textContent, "Before", "earlier text command was not applied");
    assert(button.firstChild === text, "text node untouched");
    equal(runtime.stats(), zero("faulted"), "failed runtime cleaned up");
    button.click(); equal(root.childNodes.length, 0, "listener removed");
  } finally { runtime.dispose(); root.remove(); }
});
const passed = results.filter(result => result.passed).length;
window.__ruvane_browser_result = { passed, failed: results.length - passed, results, userAgent: navigator.userAgent };
document.getElementById("status").textContent = `${passed}/${results.length} browser tests passed`;
report.textContent = JSON.stringify(window.__ruvane_browser_result, null, 2);

import { createRuntime } from "/assets/ruvane.js";

const results = [], report = document.getElementById("report");
const assert = (value, message) => { if (!value) throw new Error(message); };
const equal = (actual, expected, message) => assert(JSON.stringify(actual) === JSON.stringify(expected), `${message}: ${JSON.stringify(actual)}`);
async function test(name, body) {
  try { await body(); results.push({ name, passed: true }); }
  catch (error) { results.push({ name, passed: false, error: String(error.message) }); }
  report.textContent = JSON.stringify(results, null, 2);
}
const root = () => { const element = document.createElement("div"); document.body.appendChild(element); return element; };
const props = (component, value = 0) => ({ component, state: { kind: "number", value } });
const load = options => createRuntime("/assets/app.wasm", options);
const output = element => [...element.querySelectorAll("output")].map(node => node.textContent);
const button = (element, label) => [...element.querySelectorAll("button")].find(node => node.textContent === label);
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
const type = (element, value) => { element.value = value; element.dispatchEvent(new InputEvent("input", { bubbles: true })); };
const zero = (runtime, state = "active") => equal(runtime.stats(), { state, views: 0, domHandles: 0, eventListeners: 0, pendingActions: 0, rustInstances: 0, rustSignals: 0, rustEffects: 0 }, "resource cleanup");

await test("a pending action disables its trigger and duplicate events do not start another request", async () => {
  const runtime = await load(), element = root();
  try {
    const view = runtime.mount(element, props(7, 1));
    equal(output(element), ["pending"], "pending model"); assert(element.querySelector("button").disabled, "pending button");
    equal(runtime.stats().pendingActions, 1, "one request"); view.dispatch(1); view.dispatch(1);
    assert(view.whenIdle() === view.whenIdle(), "one shared idle promise");
    equal(await view.whenIdle(), "success", "success callback");
    equal(output(element), ["success"], "Rust result"); assert(!element.querySelector("button").disabled, "button enabled");
    equal(runtime.stats().pendingActions, 0, "request released");
  } finally { runtime.dispose(); element.remove(); }
});
await test("invalid UTF-8, frames, size, MIME and HTTP status stay local to the component", async () => {
  const runtime = await load(), element = root();
  try {
    for (const [mode, expected] of [[2,"protocol"],[3,"protocol"],[4,"protocol"],[5,"network"],[6,"rejected"],[7,"validation"],[8,"protocol"],[9,"protocol"]]) {
      const view = runtime.mount(element, props(7, mode));
      equal(await view.whenIdle(), expected, `mode ${mode}`); equal(output(element), [expected], "Rust completion");
      equal(runtime.stats().state, "active", "runtime remains active");
      assert(!element.querySelector("button").disabled, "recovery enabled"); view.dispose(); zero(runtime);
    }
  } finally { runtime.dispose(); element.remove(); }
});
await test("timeout clears pending state and a later server response cannot complete the request again", async () => {
  const runtime = await load({ actionTimeoutMs: 40 }), element = root();
  try {
    const view = runtime.mount(element, props(7, 1));
    equal(await view.whenIdle(), "timeout", "deadline"); equal(output(element), ["timeout"], "Rust timeout");
    equal(runtime.stats().pendingActions, 0, "no pending request");
    await wait(300); equal(output(element), ["timeout"], "late result ignored");
    assert(!element.querySelector("button").disabled, "trigger enabled");
  } finally { runtime.dispose(); element.remove(); }
});
await test("disposing a pending owner aborts I/O and cannot affect a fresh view in the same root", async () => {
  const runtime = await load(), element = root();
  try {
    const old = runtime.mount(element, props(7, 1)), waiting = old.whenIdle();
    old.dispose(); equal(await waiting, "disposed", "waiter released"); zero(runtime);
    const current = runtime.mount(element, props(7, 0));
    equal(await current.whenIdle(), "success", "new action"); await wait(300);
    equal(output(element), ["success"], "old response cannot change fresh model");
    equal(runtime.stats().pendingActions, 0, "no retained requests");
  } finally { runtime.dispose(); element.remove(); }
});
await test("runtime disposal releases action waiters, listeners, timers and native owners", async () => {
  const runtime = await load(), element = root();
  const view = runtime.mount(element, props(7, 1)), waiting = view.whenIdle();
  runtime.dispose(); equal(await waiting, "disposed", "waiter disposed"); zero(runtime, "disposed");
  await wait(300); zero(runtime, "disposed"); element.remove();
});
await test("two views can await independent requests while a protocol failure remains isolated", async () => {
  const runtime = await load(), first = root(), second = root();
  try {
    const good = runtime.mount(first, props(7, 1)), bad = runtime.mount(second, props(7, 2));
    equal(runtime.stats().pendingActions, 2, "independent requests");
    equal(await bad.whenIdle(), "protocol", "bad response"); equal(await good.whenIdle(), "success", "other response");
    good.dispatch(1); equal(await good.whenIdle(), "success", "still usable");
    equal(runtime.stats().views, 2, "both owners live");
  } finally { runtime.dispose(); first.remove(); second.remove(); }
});
await test("Rust/Wasm validates, creates and updates server tasks without replacing the document or form", async () => {
  const runtime = await load(), element = root(), documentBody = document.body;
  try {
    const view = runtime.mount(element, props(6)); equal(await view.whenIdle(), "success", "initial snapshot");
    const title = element.querySelector('input[type="text"]'), notes = element.querySelector("textarea");
    view.dispatch(1); equal(await view.whenIdle(), "validation", "required validation");
    equal(title.getAttribute("aria-invalid"), "true", "field marked");
    type(title, '<script>action</script>'); type(notes, "Notes 🦀\nsecond line");
    view.dispatch(1); assert(title.disabled && notes.disabled, "fields disabled while pending");
    view.dispatch(1); equal(await view.whenIdle(), "success", "created once");
    equal(element.querySelectorAll("li").length, 1, "one task");
    assert(!element.querySelector("script"), "text stays inert");
    assert(element.textContent.includes('<script>action</script> — Notes 🦀\nsecond line'), "result text");
    assert(document.body === documentBody && element.querySelector('input[type="text"]') === title, "document and input identity");
    equal([title.value, notes.value], ["", ""], "reset on success");
    const check = element.querySelector('input[type="checkbox"]'); check.click(); await view.whenIdle(); assert(check.checked, "server completion flag");
    const html = new DOMParser().parseFromString(await (await fetch("/tasks")).text(), "text/html");
    assert(html.querySelector('input[name="done"]').checked, "native page reads the same state");
    button(element, "Remove task").click(); await view.whenIdle(); equal(element.querySelectorAll("li").length, 0, "server removal");
  } finally { runtime.dispose(); element.remove(); }
});
await test("server snapshots paginate within the live node budget and retire off-page bindings", async () => {
  const runtime = await load(), element = root();
  try {
    const view = runtime.mount(element, props(6)); await view.whenIdle();
    for (let n = 0; n < 5; n++) { view.dispatch(5, `Page task ${n}`); view.dispatch(1); await view.whenIdle(); }
    equal(element.querySelectorAll("li").length, 3, "three visible rows");
    assert(runtime.stats().domHandles <= 32, "view cap");
    const stale = button(element, "Remove task"); button(element, "Next").click();
    equal(element.querySelectorAll("li").length, 2, "second page");
    stale.click(); equal(runtime.stats().pendingActions, 0, "off-page handler retired");
    assert(output(element).at(-1).includes("5 tasks / page 2"), "page count");
    while (element.querySelector("li")) { button(element, "Remove task").click(); await view.whenIdle(); }
    assert(output(element).at(-1).includes("0 tasks / page 1"), "all logical tasks removed");
  } finally { runtime.dispose(); element.remove(); }
});
await test("100 create/remove round trips return to the same live resource counts", async () => {
  const runtime = await load(), element = root();
  try {
    const view = runtime.mount(element, props(6)); await view.whenIdle(); const baseline = runtime.stats();
    for (let n = 0; n < 100; n++) {
      view.dispatch(5, `Cycle ${n} 🦀`); view.dispatch(1); equal(await view.whenIdle(), "success", "create");
      equal(element.querySelectorAll("li").length, 1, "one row");
      button(element, "Remove task").click(); equal(await view.whenIdle(), "success", "remove");
      equal(runtime.stats(), baseline, "no live action/handler/owner growth");
    }
  } finally { runtime.dispose(); element.remove(); }
});
await test("native fallback is present in server HTML and the native page keeps scripts blocked", async () => {
  const response = await fetch("/tasks"), html = new DOMParser().parseFromString(await response.text(), "text/html");
  assert(response.headers.get("content-security-policy").includes("script-src 'none'"), "native CSP");
  equal(html.querySelectorAll("script").length, 0, "no native script");
  assert(html.querySelector('form[method="post"][action="/tasks"] input[name="title"]'), "native form");
  const enhanced = new DOMParser().parseFromString(await (await fetch("/interactive")).text(), "text/html");
  assert(!enhanced.getElementById("native-fallback").hidden, "fallback starts visible");
  equal(enhanced.getElementById("remote-root").childNodes.length, 0, "client root starts empty");
});
const passed = results.filter(result => result.passed).length;
const result = { passed, failed: results.length - passed, results, userAgent: navigator.userAgent };
document.getElementById("status").textContent = `${passed}/${results.length} action browser tests passed`;
report.textContent = JSON.stringify(result, null, 2);

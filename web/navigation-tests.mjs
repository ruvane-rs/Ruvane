import { createRuntime } from "./bridge.mjs";
import { createNavigator } from "./navigation.mjs";
const popFallbackFixture = location.search === "?pop=yes";
const report = document.getElementById("report"), summary = document.getElementById("status"), results = [];
const assert = (value, message) => { if (!value) throw new Error(message); };
const equal = (actual, expected, message) => assert(JSON.stringify(actual) === JSON.stringify(expected), `${message}: ${JSON.stringify(actual)}`);
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
async function test(name, fn) { try { await fn(); results.push({ name, passed: true }); } catch (error) { results.push({ name, passed: false, error: String(error.message) }); } report.textContent = JSON.stringify(results, null, 2); }
const scope = document.getElementById("navigation-scope"), outlet = document.getElementById("navigation-outlet"), status = document.getElementById("navigation-message"), layout = document.getElementById("navigation-layout");
const runtime = await createRuntime("/assets/app.wasm");
const counter = runtime.mount(layout, { component: 1, state: { kind: "number", value: 5 } });
history.replaceState({ testMarker: "retained" }, "", "/app");
const navigator = createNavigator(runtime, { outlet, scope, status });
const output = () => [...layout.querySelectorAll("output")].map(node => node.textContent);
const click = anchor => anchor.dispatchEvent(new MouseEvent("click", { button: 0, bubbles: true, cancelable: true }));
const link = path => [...outlet.querySelectorAll("a")].find(anchor => anchor.getAttribute("href") === path);
let id;

await test("initial typed GET prepares a page and preserves existing history fields", async () => {
  equal(await navigator.start(), "success", "start"); equal(history.state.testMarker, "retained", "foreign history state");
  equal(document.title, "Home / Ruvane", "title"); assert(outlet.textContent.includes("Home"), "home view"); equal(runtime.stats().views, 2, "layout and page");
});
await test("links preserve the common layout, its state and the document identity", async () => {
  counter.dispatch(1); const node = layout.firstChild, documentNode = document.documentElement;
  assert(click(link("/app/tasks")) === false, "ordinary click intercepted");
  equal(await navigator.whenIdle(), "success", "tasks loaded"); equal(location.pathname, "/app/tasks", "history URL");
  equal(document.title, "Tasks / Ruvane", "page title"); assert(layout.firstChild === node && document.documentElement === documentNode, "layout and document retained");
  equal(output(), ["6", "12"], "counter state"); assert(document.activeElement === outlet.firstChild, "page host focused"); equal(scrollY, 0, "scroll reset");
});
await test("task creation, detail actions and deletion use the shared server state", async () => {
  const page = navigator.current(); page.dispatch(5, "Navigation 🦀 <script>"); page.dispatch(6, "one\ntwo"); page.dispatch(1);
  equal(await page.whenIdle(), "success", "created");
  const row = [...outlet.querySelectorAll("li")].find(node => node.textContent.includes("Navigation 🦀 <script>")); assert(row, "escaped task text");
  const anchor = row.querySelector("a"); id = anchor.getAttribute("href").split("/").pop(); click(anchor); equal(await navigator.whenIdle(), "success", "detail");
  equal(document.title, "Navigation 🦀 <script> / Ruvane", "plain text title"); assert(!outlet.querySelector("script"), "no HTML injection");
  const ssr = await fetch(`/app/tasks/${id}?native=yes`), html = await ssr.text(); assert(html.includes("&lt;script&gt;") && html.includes(`/tasks/${id}/status`), "SSR and native actions");
  navigator.current().dispatch(1); equal(await navigator.current().whenIdle(), "success", "status saved"); assert(outlet.textContent.includes("Completed"), "completed detail");
  navigator.current().dispatch(2); equal(await navigator.current().whenIdle(), "success", "removed"); assert(outlet.textContent.includes("Task removed"), "removed model");
  equal((await fetch(`/app/tasks/${id}?native=yes`)).status, 404, "direct deleted URL is 404");
});
await test("back and forward restore their URLs, page content and saved scroll position", async () => {
  await navigator.navigate("/app"); window.scrollTo(0, 260); await wait(80); const previousY = scrollY; assert(previousY > 0, "scrollable fixture");
  await navigator.navigate("/app/tasks"); equal(scrollY, 0, "new page starts at top");
  const pop = direction => new Promise(resolve => { window.addEventListener("popstate", () => resolve(), { once: true }); history[direction](); });
  await pop("back"); equal(await navigator.whenIdle(), "success", "back navigation"); equal(location.pathname, "/app", "back URL"); equal(scrollY, previousY, "back scroll");
  await pop("forward"); equal(await navigator.whenIdle(), "success", "forward navigation"); equal(location.pathname, "/app/tasks", "forward URL"); equal(scrollY, 0, "forward scroll");
});
await test("query spelling, unknown pages and invalid task IDs have typed outcomes", async () => {
  equal(await navigator.navigate("/app?"), "success", "empty query page"); assert(location.href.endsWith("?"), "empty query delimiter");
  equal(await navigator.navigate("/app/tasks?q=a+b&once=%252F"), "success", "query page"); equal(location.search, "?q=a+b&once=%252F", "exact query");
  equal(await navigator.navigate("/app/missing"), "success", "typed 404"); equal(status.textContent, "Page 404.", "404 status"); assert(outlet.textContent.includes("Page not found"), "404 model");
  equal(await navigator.navigate("/app/tasks/01"), "success", "typed 400"); equal(status.textContent, "Page 400.", "400 status");
  await navigator.navigate("/app");
});
await test("modified clicks, downloads, targets, fragments and unmarked links keep native behavior", async () => {
  const anchor = document.createElement("a"); anchor.href = "/app/tasks"; anchor.setAttribute("data-ruvane-nav", ""); scope.appendChild(anchor);
  let intercepted;
  const preventNative = event => { intercepted = event.defaultPrevented; event.preventDefault(); }; scope.addEventListener("click", preventNative);
  try {
    for (const modifier of [{ ctrlKey: true }, { metaKey: true }, { shiftKey: true }, { altKey: true }, { button: 1 }]) { intercepted = null; anchor.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ...modifier })); equal(intercepted, false, "modifier preserved"); }
    for (const [name, value] of [["target", "_blank"], ["download", "tasks"], ["ping", "/app"]]) { anchor.setAttribute(name, value); click(anchor); equal(intercepted, false, name); anchor.removeAttribute(name); }
    anchor.href = "/app#anchor"; click(anchor); equal(intercepted, false, "fragment");
    anchor.href = "/app/tasks"; anchor.removeAttribute("data-ruvane-nav"); click(anchor); equal(intercepted, false, "unmarked");
    equal(location.pathname, "/app", "no navigation started");
  } finally { scope.removeEventListener("click", preventNative); anchor.remove(); }
});
await test("a newer navigation supersedes a slow response without changing the old page early", async () => {
  await navigator.navigate("/app"); const old = outlet.firstChild, previous = location.href;
  const slow = navigator.navigate("/app/tasks?delay=yes"); assert(navigator.stats().pending && outlet.firstChild === old && location.href === previous, "old page stays live");
  const latest = navigator.navigate("/app?newest=yes"); equal(await latest, "success", "latest result"); equal(await slow, "superseded", "stale response");
  await wait(300); equal(location.search, "?newest=yes", "latest URL retained"); equal(document.title, "Home / Ruvane", "latest title");
});
await test("bad MIME, redirects, oversize, status mismatch and broken packets preserve the current page", async () => {
  for (const fixture of ["mime", "redirect", "large", "status", "broken"]) {
    const old = outlet.firstChild, previous = location.href, id = navigator.current().id;
    equal(await navigator.navigate(`/app/tasks?fixture=${fixture}`), "failed", fixture);
    assert(outlet.firstChild === old && location.href === previous && navigator.current().id === id, "failed preparation is atomic"); equal(runtime.stats().state, "active", "runtime stays usable");
  }
});
await test("retired page controllers and detached controls cannot update the replacement", async () => {
  await navigator.navigate("/app/tasks"); const old = navigator.current(), button = outlet.querySelector("button");
  await navigator.navigate("/app"); let rejected = false; try { old.dispatch(4); } catch { rejected = true; } assert(rejected, "stale controller");
  button.click(); equal(document.title, "Home / Ruvane", "detached button inert"); equal(runtime.stats().pendingActions, 0, "no retired request");
});
await test("100 page replacements keep listeners, handles and native owners bounded", async () => {
  for (let n = 0; n < 100; n++) { equal(await navigator.navigate(n % 2 ? "/app" : "/app/tasks", { replace: true }), "success", "replacement"); equal(runtime.stats().views, 2, "two views"); equal(runtime.stats().rustInstances, 2, "two owners"); equal(runtime.stats().pendingActions, 0, "no pending actions"); }
  equal(output(), ["6", "12"], "persistent layout state"); equal(navigator.stats().listeners, 3, "delegated listener count");
});
await test("disposing pending navigation releases the page and keeps the independent layout alive", async () => {
  const pending = navigator.navigate("/app/tasks?delay=yes"); navigator.dispose(); equal(await pending, "disposed", "pending waiter"); await wait(300);
  equal(navigator.stats().listeners, 0, "navigation handlers released"); equal(runtime.stats().views, 1, "layout remains"); equal(runtime.stats().rustInstances, 1, "one native owner");
  counter.dispatch(1); equal(output(), ["7", "14"], "layout still interactive");
});
await test("timeouts retain SSR and allow a later start; a dirty-form guard can decline activation", async () => {
  history.replaceState(null, "", "/app?delay=yes"); const ssr = document.createElement("p"); ssr.textContent = "SSR remains"; outlet.replaceChildren(ssr);
  const short = createNavigator(runtime, { outlet, scope, status, timeoutMs: 25 });
  equal(await short.start(), "failed", "deadline"); assert(outlet.firstChild === ssr && runtime.stats().views === 1, "SSR intact"); short.dispose();
  history.replaceState(null, "", "/app"); const guarded = createNavigator(runtime, { outlet, scope, status });
  equal(await guarded.start({ accept: () => false }), "native", "guard declines"); assert(outlet.firstChild === ssr && runtime.stats().views === 1, "prepared owner released"); guarded.dispose();
});
runtime.dispose();
await test("final disposal returns every resource count to zero", async () => { const stats = runtime.stats(); equal([stats.views,stats.domHandles,stats.eventListeners,stats.pendingActions,stats.rustInstances,stats.rustSignals,stats.rustEffects], [0,0,0,0,0,0,0], "zero resources"); });
summary.textContent = `${results.filter(r => r.passed).length}/${results.length} navigation checks passed`;
summary.dataset.complete = "true";

if (popFallbackFixture) {
  history.replaceState(null, "", "/app/tasks?fixture=mime");
  history.pushState(null, "", "/app");
  const fresh = await createRuntime("/assets/app.wasm");
  const router = createNavigator(fresh, { outlet, scope, status });
  if (await router.start() === "success") history.back();
}

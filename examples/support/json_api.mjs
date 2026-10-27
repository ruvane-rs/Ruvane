import {createRuntime} from "/assets/bridge.mjs";
const host = document.createElement("main");
document.body.append(host);
try {
  const runtime = await createRuntime("/assets/app.wasm", {csrfEndpoint: "/csrf-token"});
  runtime.mount(host, {component: 27, state: {kind: "number", value: 0}});
  window.addEventListener("pagehide", () => runtime.dispose(), {once: true});
} catch {
  host.textContent = "Unable to start the Rust/Wasm task editor. Build the matching demo module.";
}

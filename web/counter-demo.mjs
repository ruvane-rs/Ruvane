import { createRuntime } from "/assets/ruvane.js";

const root = document.getElementById("counter");
const status = document.getElementById("status");
const controls = document.getElementById("controls");
try {
  const runtime = await createRuntime("/assets/counter.wasm");
  let view = runtime.mount(root);
  status.textContent = "State, derived values and events handled by Rust/Wasm.";
  const toggle = document.createElement("button");
  toggle.type = "button";
  toggle.textContent = "Unmount component";
  toggle.addEventListener("click", () => {
    if (view) {
      view.dispose(); view = null;
      toggle.textContent = "Mount component";
    } else {
      view = runtime.mount(root);
      toggle.textContent = "Unmount component";
    }
    status.textContent = JSON.stringify(runtime.stats());
  });
  controls.appendChild(toggle);
  window.addEventListener("pagehide", () => runtime.dispose(), { once: true });
} catch {
  status.textContent = "Module loading failed. Server content is still available.";
}

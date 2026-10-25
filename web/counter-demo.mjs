import { createRuntime } from "/assets/ruvane.js";

const root = document.querySelector("#counter > div");
const status = document.getElementById("status");
try {
  const runtime = await createRuntime("/assets/app.wasm");
  let view, failures = 0;
  try { view = runtime.hydrate(root); } catch { failures++; }
  try { runtime.hydrate(document.querySelector("#toggle > div")); } catch { failures++; }
  try { runtime.hydrate(document.querySelector("#item-list > div")); } catch { failures++; }
  try { runtime.hydrate(document.querySelector("#tasks > div")); } catch { failures++; }
  status.textContent = failures
    ? "Some components could not hydrate. Their server HTML is still available."
    : "Server HTML hydrated. State and events run in Rust/Wasm.";
  const control = document.createElement("button");
  control.type = "button";
  control.textContent = "Unmount counter";
  control.disabled = !view;
  control.addEventListener("click", () => {
    if (view) {
      view.dispose(); view = null;
      control.textContent = "Mount counter";
    } else {
      view = runtime.mount(root, { component: 1, state: { kind: "number", value: 5 } });
      control.textContent = "Unmount counter";
    }
    status.textContent = JSON.stringify(runtime.stats());
  });
  document.getElementById("controls").appendChild(control);
  window.addEventListener("pagehide", () => runtime.dispose(), { once: true });
} catch {
  status.textContent = "Module loading failed. Server HTML is still available.";
}

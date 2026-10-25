import { createRuntime } from "/assets/ruvane.js";

const status = document.getElementById("action-status");
let runtime;
try {
  runtime = await createRuntime("/assets/app.wasm");
  window.addEventListener("pagehide", () => runtime.dispose(), { once: true });
  const view = runtime.mount(document.getElementById("remote-root"), { component: 6, state: { kind: "number", value: 0 } });
  const result = await view.whenIdle();
  if (result === "success") {
    const fallback = document.getElementById("native-fallback");
    if (fallback.contains(document.activeElement)) {
      runtime.dispose(); status.textContent = "Native forms are ready.";
    } else {
      view.dispatch(5, fallback.querySelector('input[name="title"]').value);
      view.dispatch(6, fallback.querySelector('textarea[name="notes"]').value);
      fallback.hidden = true;
      status.textContent = "Connected. Changes are saved on the server.";
    }
  } else if (result !== "disposed") status.textContent = "Connection failed. Native forms are available below.";
} catch {
  runtime?.dispose();
  status.textContent = "Interactive mode is unavailable. Native forms are ready.";
}

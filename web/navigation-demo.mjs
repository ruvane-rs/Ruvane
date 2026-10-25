import { createRuntime } from "./bridge.mjs";
import { createNavigator } from "./navigation.mjs";

const status = document.getElementById("navigation-status");
let runtime, navigator;
try {
  runtime = await createRuntime("/assets/app.wasm");
  const counter = document.querySelector("#layout-counter [data-ruvane-version]");
  runtime.hydrate(counter);
  navigator = createNavigator(runtime, { outlet: document.getElementById("page-outlet"), status });
  await navigator.start({ accept: () => {
    const forms = document.getElementById("native-forms");
    return !forms || !forms.contains(document.activeElement) && ![...forms.querySelectorAll("input,textarea")].some(field => field.type === "checkbox" ? field.checked !== field.defaultChecked : field.value !== field.defaultValue);
  } });
  window.addEventListener("pagehide", () => { navigator.dispose(); runtime.dispose(); }, { once: true });
  window.addEventListener("pageshow", event => { if (event.persisted) location.reload(); });
} catch {
  navigator?.dispose(); runtime?.dispose();
  status.textContent = "Client navigation is unavailable. Native links and forms are ready.";
}

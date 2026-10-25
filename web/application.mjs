import { createRuntime } from "./bridge.mjs";
import { createNavigator, readBoundaries } from "./navigation.mjs";
const root = document.getElementById("ruvane-application");
const outlet = root?.querySelector("#ruvane-outlet"), status = root?.querySelector("#ruvane-status");
let runtime, navigator;
try {
  if (!root || !outlet || !status || document.querySelectorAll("#ruvane-application").length !== 1) throw new Error("Invalid Ruvane application shell");
  runtime = await createRuntime(root.getAttribute("data-ruvane-module"), { applicationRevision: root.getAttribute("data-ruvane-revision") });
  for (const element of root.querySelectorAll("[data-ruvane-version]")) if (!outlet.contains(element)) runtime.hydrate(element);
  navigator = createNavigator(runtime, { outlet, scope: root, status, boundaries:readBoundaries(root), progressive:true });
  await navigator.start({ accept: () => {
    const fallback = root.querySelector("#ruvane-fallback");
    return !fallback || !fallback.contains(document.activeElement) && ![...fallback.querySelectorAll("input,textarea")].some(field => field.type === "checkbox" ? field.checked !== field.defaultChecked : field.value !== field.defaultValue);
  } });
  window.addEventListener("pagehide", () => { navigator.dispose(); runtime.dispose(); }, { once: true });
  window.addEventListener("pageshow", event => { if (event.persisted) location.reload(); });
} catch {
  navigator?.dispose(); runtime?.dispose();
  if (status) status.textContent = "Interactive mode is unavailable. Native links and forms are ready.";
}

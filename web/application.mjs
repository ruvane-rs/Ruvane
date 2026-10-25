import { createRuntime } from "./bridge.mjs";
import { createNavigator, readBoundaries, readInitialSnapshot } from "./navigation.mjs";
const root = document.getElementById("ruvane-application");
const outlet = root?.querySelector("#ruvane-outlet"), status = root?.querySelector("#ruvane-status");
let runtime, navigator, retired=false;
try {
  if (!root || !outlet || !status || document.querySelectorAll("#ruvane-application").length !== 1) throw new Error("Invalid Ruvane application shell");
  if(root.querySelectorAll("#ruvane-outlet").length!==1||root.querySelectorAll("#ruvane-status").length!==1)throw new Error("Ambiguous Ruvane application shell");
  window.addEventListener("pagehide", () => { retired=true;navigator?.dispose(); runtime?.dispose(); }, { once: true });
  window.addEventListener("pageshow", event => { if (event.persisted) location.reload(); });
  const snapshot=readInitialSnapshot(root);
  runtime = await createRuntime(root.getAttribute("data-ruvane-module"), { applicationRevision: root.getAttribute("data-ruvane-revision") });
  if(retired){runtime.dispose();throw new Error("Ruvane document retired");}
  for (const element of root.querySelectorAll("[data-ruvane-version]")) if (!outlet.contains(element)) runtime.hydrate(element);
  navigator = createNavigator(runtime, { outlet, scope: root, status, boundaries:readBoundaries(root), progressive:true });
  const result=await navigator.start({ snapshot, accept: () => {
    const fallback = root.querySelector("#ruvane-fallback");
    return !fallback || !fallback.contains(document.activeElement) && ![...fallback.querySelectorAll("input,textarea")].some(field => field.type === "checkbox" ? field.checked !== field.defaultChecked : field.value !== field.defaultValue);
  } });
  if(result==="success")root.querySelector("#ruvane-initial").remove();
  else navigator.dispose();
} catch {
  navigator?.dispose();
  if (status) status.textContent = "Interactive mode is unavailable. Native links and forms are ready.";
}

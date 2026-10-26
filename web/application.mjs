import { createRuntime } from "./bridge.mjs";
import { createNavigator, readBoundaries, readInitialSnapshot, createDocumentHydration } from "./navigation.mjs";
const root=document.getElementById("ruvane-application");
let runtime,navigator,session,retired=false;
function shell(){
  const outlet=root.querySelector("#ruvane-outlet");
  for(const element of root.querySelectorAll("[data-ruvane-version]"))if(!outlet.contains(element))runtime.hydrate(element);
}
try{
  if(!root||document.querySelectorAll("#ruvane-application").length!==1)throw new Error("Invalid Ruvane application shell");
  window.addEventListener("pagehide",()=>{retired=true;session?.retire();navigator?.dispose();runtime?.dispose();},{once:true});
  window.addEventListener("pageshow",event=>{if(event.persisted)location.reload();});
  const segments=root.getAttribute("data-ruvane-document")==="segments",began=performance.now();
  const snapshot=segments?null:readInitialSnapshot(root);
  const rawTime=root.getAttribute("data-ruvane-timeout");
  if(segments&&(!/^[1-9][0-9]{0,4}$/.test(rawTime??"")||Number(rawTime)>60000))throw new Error("Invalid Ruvane document deadline");
  runtime=await createRuntime(root.getAttribute("data-ruvane-module"),{applicationRevision:root.getAttribute("data-ruvane-revision")});
  if(retired){runtime.dispose();throw new Error("Ruvane document retired");}
  let initial={snapshot,parents:[]};
  if(segments){
    session=createDocumentHydration(runtime,root,{timeoutMs:Number(rawTime)-(performance.now()-began)});
    initial=await session.ready;
  }
  if(retired)throw new Error("Ruvane document retired");
  const outlet=root.querySelector("#ruvane-outlet"),status=root.querySelector("#ruvane-status");
  if(!outlet||!status||root.querySelectorAll("#ruvane-outlet").length!==1||root.querySelectorAll("#ruvane-status").length!==1)throw new Error("Ambiguous Ruvane application shell");
  if(!segments)shell();
  navigator=createNavigator(runtime,{outlet,scope:root,status,boundaries:readBoundaries(root),progressive:true});
  const result=await navigator.start({...initial,accept:()=>{
    if(initial.accept&&!initial.accept())return false;
    const fallback=root.querySelector("#ruvane-fallback");
    return !fallback||!fallback.contains(document.activeElement)&&![...fallback.querySelectorAll("input,textarea")].some(field=>field.type==="checkbox"?field.checked!==field.defaultChecked:field.value!==field.defaultValue);
  }});
  if(result==="success"){
    session?.commit();
    root.querySelector("#ruvane-initial").remove();root.querySelector("#ruvane-complete")?.remove();
    if(segments)shell();
  }else{session?.retire();navigator.dispose();}
}catch{
  session?.retire();navigator?.dispose();
  const status=root?.querySelector("#ruvane-status");if(status)status.textContent="Interactive mode is unavailable. Native links and forms are ready.";
}

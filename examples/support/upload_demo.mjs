import {createRuntime} from "/assets/bridge.mjs";
const host=document.getElementById("upload-root");
try {
  const runtime=await createRuntime("/assets/app.wasm",{csrfEndpoint:"/csrf-token"});
  runtime.hydrate(host.querySelector("[data-ruvane-version]"));
  window.addEventListener("pagehide",()=>runtime.dispose(),{once:true});
} catch {host.textContent="Unable to start the Rust uploader. Build the matching demo Wasm module.";}

const root = document.getElementById("ruvane-application"), outlet = root.querySelector("#ruvane-outlet"), status = root.querySelector("#ruvane-status");
const results = [], report = document.createElement("pre"), summary = document.createElement("p"); report.id = "application-report"; summary.id = "application-summary"; document.body.append(summary, report);
const assert = (value, message) => { if (!value) throw new Error(message); };
const wait = ms => new Promise(resolve => setTimeout(resolve,ms));
async function until(check) { const deadline = performance.now()+5000; while(!check()) { if(performance.now()>deadline)throw new Error("Application check timed out");await wait(10); } }
async function test(name, fn) { try { await fn(); results.push({name,passed:true}); } catch(error) { results.push({name,passed:false,error:String(error.message)}); } report.textContent=JSON.stringify(results,null,2); }
const click = node => { assert(node,"missing target");node.click(); };
const button = (scope,name) => [...scope.querySelectorAll("button")].find(node=>node.textContent===name);
const anchor = (scope,path) => [...scope.querySelectorAll("a")].find(node=>node.getAttribute("href")===path);
const type = (field,value) => { field.value=value;field.dispatchEvent(new InputEvent("input",{bubbles:true})); };
const ready = () => status.textContent === "Ready.";
const layout = () => root.querySelector("output").textContent;
let taskURL, retired;
const documentNode=document.documentElement, revision=root.getAttribute("data-ruvane-revision"), moduleURL=root.getAttribute("data-ruvane-module");
await test("the generated bootstrap activates its own application registry",async()=> { await until(ready);assert(root.querySelector("button") && layout()==="5","initial layout"); });
await test("shared App pages navigate without replacing the document or layout state",async()=> { click(button(root,"Increment"));assert(layout()==="6","counter increment");click(anchor(root,"/tasks"));await until(()=>ready()&&document.title==="Tasks / Ruvane");assert(document.documentElement===documentNode && layout()==="6","persistent shell");assert(document.activeElement===outlet.querySelector('[tabindex="-1"]'),"focus moved"); });
await test("Rust validates fields and submits a typed action with the application revision",async()=> {
  click(button(outlet,"Add task"));await until(()=>outlet.textContent.includes("Enter a task name"));assert(outlet.querySelector("input").getAttribute("aria-invalid")==="true","field validation");
  type(outlet.querySelector('input[type="text"]'),"Application test 🦀 <script>");type(outlet.querySelector("textarea"),"Shared native and Wasm code.");click(button(outlet,"Add task"));await until(()=>outlet.textContent.includes("Saved."));
  const row=[...outlet.querySelectorAll("li")].find(node=>node.textContent.includes("Application test 🦀 <script>"));assert(row,"saved row");taskURL=row.querySelector("a").getAttribute("href");assert(!outlet.querySelector("script"),"inert user text");
});
await test("detail URLs share SSR, escaped data, native forms and true HTTP statuses",async()=> {
  click(anchor(outlet,taskURL));await until(()=>ready()&&document.title==="Application test 🦀 <script> / Ruvane");
  const response=await fetch(taskURL+"?native=yes"),html=await response.text();assert(response.status===200 && html.includes("&lt;script&gt;") && html.includes('id="ruvane-fallback"') && html.includes('action="'+taskURL+'/status"'),"native detail");assert(response.headers.get("content-security-policy").includes("script-src 'none'"),"native CSP");
});
await test("detail actions persist state and removing a task changes its direct URL to 404",async()=> {
  click(button(outlet,"Mark completed"));await until(()=>outlet.textContent.includes("Completed") && outlet.textContent.includes("Saved."));retired=button(outlet,"Remove task");click(retired);await until(()=>outlet.textContent.includes("Task removed"));assert((await fetch(taskURL+"?native=yes")).status===404,"deleted SSR status");
  click(anchor(outlet,"/tasks"));await until(()=>ready()&&document.title==="Tasks / Ruvane");retired.click();assert(!outlet.textContent.includes("Task removed"),"retired handler removed");assert(layout()==="6","layout retained");
});
await test("back and forward use shared route declarations and preserve the live document",async()=> {
  click(anchor(root,"/"));await until(()=>ready()&&document.title==="Home / Ruvane");
  history.back();await until(()=>ready()&&document.title==="Tasks / Ruvane");history.forward();await until(()=>ready()&&document.title==="Home / Ruvane");assert(document.documentElement===documentNode && layout()==="6","history keeps shell");
});
await test("wrong module revisions fail before mounting and leave the application active",async()=> {
  const {createRuntime}=await import(new URL("./bridge.mjs",new URL(moduleURL,location.href)));let rejected=false;
  try { await createRuntime(moduleURL,{applicationRevision:(BigInt(revision)+1n).toString()}); } catch { rejected=true; } assert(rejected,"revision refused");assert(ready()&&layout()==="6","live page intact");
  const runtime=await createRuntime(moduleURL,{applicationRevision:revision}), host=document.createElement("div");document.body.append(host);
  const view=runtime.mount(host,{component:1,state:{kind:"number",value:5}});assert(host.textContent.includes("Layout counter"),"own registry");view.dispose();assert(runtime.stats().views===0&&runtime.stats().rustInstances===0,"owner released");runtime.dispose();host.remove();
});
await test("stale navigation and typed mutations are refused by the App router",async()=> {
  const page=await fetch("/tasks",{headers:{Accept:"application/vnd.ruvane.page","Ruvane-Application-Revision":"0"}});assert(page.status===409,"stale navigation");
  const mutation=await fetch("/actions/tasks/create",{method:"POST",headers:{"Content-Type":"application/x-www-form-urlencoded",Accept:"application/vnd.ruvane.action","Ruvane-Application-Revision":"0"},body:"title=Stale+mutation+fixture"});assert(mutation.status===409,"stale action");assert(!(await(await fetch("/tasks?native=yes")).text()).includes("Stale mutation fixture"),"no mutation");
});
summary.textContent=`${results.filter(r=>r.passed).length}/${results.length} application checks passed`;summary.dataset.complete="true";

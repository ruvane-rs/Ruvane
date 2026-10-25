const status=document.getElementById("status"),report=document.getElementById("report"),run=document.createElement("button"),release=document.createElement("button");
run.type=release.type="button";run.textContent="Run browser checks";release.textContent="Release deferred report";document.body.prepend(run,release);
const results=[];
const assert=(value,message)=>{if(!value)throw new Error(message);};
const wait=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(check){const end=performance.now()+55000;while(!check()){if(performance.now()>end)throw new Error("Streaming check timed out");await wait(10);}}
async function test(name,fn){try{await fn();results.push({name,passed:true});}catch(error){results.push({name,passed:false,error:String(error.message)});}report.textContent=JSON.stringify(results,null,2);}
const control=path=>fetch(path,{method:"POST",headers:{"Content-Type":"application/x-www-form-urlencoded"},body:""});
release.disabled=true;
release.addEventListener("click",async()=>{release.disabled=true;await control("/fixtures/release");});
run.addEventListener("click",async()=>{
  run.disabled=true;const child=window.open("about:blank","ruvane-progressive-report");
  if(!child){status.textContent="The browser blocked the test window.";run.disabled=false;return;}
  try{
    await control("/fixtures/prepare");
    child.location.href="/stream?controlled=yes";
    await test("a real document exposes its initial shell before deferred production finishes",async()=>{
      await until(()=>child.document?.getElementById("initial-shell"));
      const heading=child.document.getElementById("initial-shell");
      assert(!child.document.getElementById("deferred-report"),"report still deferred");
      assert(child.document.readyState==="loading"&&heading.getBoundingClientRect().height>0,"visible shell while loading");
      status.textContent="Initial shell is visible. Release the deferred report.";release.disabled=false;
    });
    await test("later HTML completes the same document without scripts or node replacement",async()=>{
      const doc=child.document,heading=doc.getElementById("initial-shell");
      await until(()=>child.document.getElementById("deferred-report")&&child.document.readyState==="complete");
      assert(child.document===doc&&doc.getElementById("initial-shell")===heading,"same document and initial node");
      assert(doc.getElementById("deferred-report").textContent.includes("Generated safely"),"deferred content");
      assert(doc.querySelectorAll("script").length===0,"script-free SSR");
    });
    await test("HEAD reports chunked framing without sending content",async()=>{
      const response=await fetch("/stream",{method:"HEAD"});assert(response.status===200,"HEAD status");
      assert(response.headers.get("transfer-encoding")==="chunked"&&response.headers.get("content-length")===null,"one framing mode");
      assert((await response.arrayBuffer()).byteLength===0,"no HEAD body");
    });
    await test("Fetch receives the prefix before the deferred body and preserves escaped text",async()=>{
      await control("/fixtures/prepare");const response=await fetch("/stream?controlled=yes&q=%3Cscript%3E%26%F0%9F%A6%80"),reader=response.body.getReader(),decoder=new TextDecoder();let text="";
      while(!text.includes("initial-shell")){const part=await reader.read();assert(!part.done,"initial bytes");text+=decoder.decode(part.value,{stream:true});}
      assert(!text.includes("deferred-report"),"prefix arrives first");await control("/fixtures/release");
      while(true){const part=await reader.read();if(part.done)break;text+=decoder.decode(part.value,{stream:true});}text+=decoder.decode();
      assert(text.endsWith("</body></html>"),"complete document");assert(text.includes("&lt;script&gt;&amp;🦀")&&!text.includes("<script>"),"escaped deferred text");
      assert(response.headers.get("cache-control")==="no-store"&&response.headers.get("content-security-policy").includes("script-src 'none'"),"fixed policies");
    });
    await test("a late producer failure leaves an incomplete response",async()=>{const response=await fetch("/stream?fail=yes");assert(response.status===200,"committed status");let rejected=false;try{await response.text();}catch{rejected=true;}assert(rejected,"incomplete framing detected");});
    await test("aborting a stream leaves subsequent requests usable",async()=>{
      const controller=new AbortController(),response=await fetch("/stream",{signal:controller.signal});const reader=response.body.getReader();await reader.read();controller.abort();try{await reader.read();}catch{}await wait(550);
      const next=await fetch("/stream");assert(next.status===200&&(await next.text()).includes("deferred-report"),"server still usable");
    });
  }finally{await control("/fixtures/release");release.disabled=true;child.close();}
  const result={passed:results.filter(row=>row.passed).length,failed:results.filter(row=>!row.passed).length,results};report.textContent=JSON.stringify(result,null,2);status.textContent=`${result.passed}/${results.length} streaming checks passed`;
});

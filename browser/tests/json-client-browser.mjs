export async function runJSONClientChecks(ctx) {
  const {bridge,moduleURL,revision,csrfEndpoint,panel,test,check,stats,originalFetch}=ctx;
  const pathOf=input=>new URL(typeof input==="string"?input:input.url,location.href).pathname;
  const target=input=>pathOf(input)==="/api/transport";
  const waitFor=async predicate=>{for(let n=0;n<200;n++){if(predicate())return;await new Promise(resolve=>setTimeout(resolve,5));}throw Error("Probe did not reach its expected state");};
  const response=(body='{"n":18446744073709551615}',status=200,headers={})=>new Response(body,{status,headers:{"Content-Type":"application/json","Cache-Control":"no-store",...headers}});
  async function probe(mode,fn,timeout=1000) {
    const runtime=await bridge.createRuntime(moduleURL,{applicationRevision:revision,csrfEndpoint,actionTimeoutMs:timeout});
    const host=document.createElement("section");panel.append(host);
    const controller=runtime.mount(host,{component:28,state:{kind:"number",value:mode}});
    try {await fn({runtime,host,controller});} finally {globalThis.fetch=originalFetch;runtime.dispose();check(runtime.stats().views===0,"all probe owners released");host.remove();}
  }
  await test("Rust JSON transport sends five explicit methods and preserves u64 values",async()=>{
    for(let mode=0;mode<5;mode++)await probe(mode,async({runtime,host,controller})=>{
      const calls=[];globalThis.fetch=async(input,options)=>{if(target(input))calls.push(options);return originalFetch(input,options);};
      const epoch=runtime.actionState().epoch;controller.dispatch(1);check(await controller.whenIdle()==="success","method completed");
      check(calls.length===1&&calls[0].method===["GET","POST","PUT","PATCH","DELETE"][mode],"one explicit request");
      check(calls[0].headers["If-Match"]==='"retained-version"',"retained version sent unchanged");
      check(calls[0].mode==="same-origin"&&calls[0].credentials==="same-origin"&&calls[0].redirect==="error"&&calls[0].cache==="no-store","transport profile");
      check(mode===0||typeof calls[0].headers["Ruvane-Csrf-Token"]==="string","mutation CSRF preflight");
      check([0,4].includes(mode)?!Object.hasOwn(calls[0],"body"):calls[0].body==='{"n":18446744073709551615}',"Rust body without number conversion");
      check(host.textContent.includes('200:{"n":18446744073709551615}:uncertain=false'),"exact Rust response");
      check(mode!==0||runtime.actionState().epoch===epoch,"GET does not invalidate mutation caches");
    });
  });
  await test("Rust JSON replies distinguish validation, conflicts, quotas and server uncertainty",async()=>{
    for(const [status,headers,uncertain] of [[400,{},false],[412,{},false],[428,{},false],[429,{"Retry-After":"3","Ruvane-Action-Rejected":"rate-limit"},false],[429,{"Retry-After":"3"},true],[503,{},true]]) {
      await probe(1,async({runtime,host,controller})=>{
        let calls=0;globalThis.fetch=async(input,options)=>{if(!target(input))return originalFetch(input,options);calls++;return response('{}',status,headers);};
        controller.dispatch(1);const outcome=await controller.whenIdle();
        check(host.textContent.includes(`${status}:{}:uncertain=${uncertain}`),"typed HTTP status and certainty");
        check(runtime.actionState().uncertain===uncertain&&calls===1,"cache uncertainty and no retry");
        if(status===429)check(outcome==="rate-limited"&&controller.retryAfter()===3&&host.textContent.includes("retry=Some(3)"),"quota hint reaches Rust");
        if(status===412)check(outcome==="conflict","conflict result");
        if(status===428)check(outcome==="precondition-required","required version result");
      });
    }
  });
  await test("native guard refusals preserve HTTP status while discarding non-JSON bodies",async()=>{
    await probe(0,async({host,controller})=>{
      globalThis.fetch=async(input,options)=>originalFetch(input,target(input)?{...options,credentials:"omit"}:options);
      controller.dispatch(1);check(await controller.whenIdle()==="rejected","native access denial");
      check(host.textContent.includes("403:null:uncertain=false")&&host.textContent.includes("json=false"),"status retained, HTML discarded");
    });
    for(const [status,uncertain] of [[400,false],[412,false],[428,false],[503,true]])await probe(1,async({runtime,host,controller})=>{
      globalThis.fetch=async(input,options)=>target(input)?response("PRIVATE ERROR HTML",status,{"Content-Type":"text/html"}):originalFetch(input,options);
      controller.dispatch(1);await controller.whenIdle();
      check(host.textContent.includes(`${status}:null:uncertain=${uncertain}`)&&!host.textContent.includes("PRIVATE"),"error body not reflected");
      check(runtime.actionState().uncertain===uncertain,"correct mutation certainty");
    });
  });
  await test("native quota HTML is a confirmed refusal with a typed retry hint",async()=>{
    await(await originalFetch("/json-reset",{method:"POST",headers:{"Content-Type":"application/x-www-form-urlencoded"},body:"session=keep"})).arrayBuffer();
    for(let attempt=0;attempt<3;attempt++)await probe(1,async({runtime,host,controller})=>{
      globalThis.fetch=async(input,options)=>originalFetch(target(input)?"/api/limited":input,options);
      controller.dispatch(1);const outcome=await controller.whenIdle();
      check(outcome===(attempt<2?"validation":"rate-limited"),"native shared quota");
      if(attempt===2)check(!runtime.actionState().uncertain&&controller.retryAfter()>0&&host.textContent.includes("429:null:uncertain=false")&&host.textContent.includes("json=false"),"confirmed refusal in Rust");
    });
  });
  await test("JSON preflight rejection sends no mutation and remains certain",async()=>{
    await probe(1,async({runtime,host,controller})=>{
      let mutations=0;globalThis.fetch=async(input,options)=>{
        if(pathOf(input)===csrfEndpoint)return response('{}',429,{"Retry-After":"2"});
        if(target(input))mutations++;return originalFetch(input,options);
      };
      controller.dispatch(1);check(await controller.whenIdle()==="rate-limited","preflight quota");
      check(mutations===0&&!runtime.actionState().uncertain&&host.textContent.includes("RateLimited:uncertain=false:retry=Some(2)"),"no mutation or uncertainty");
    });
  });
  await test("a network failure during CSRF preflight remains distinct and sends no mutation",async()=>{
    await probe(1,async({runtime,host,controller})=>{
      let mutations=0;globalThis.fetch=async(input,options)=>{
        if(pathOf(input)===csrfEndpoint)throw new TypeError("simulated preflight network failure");
        if(target(input))mutations++;return originalFetch(input,options);
      };
      controller.dispatch(1);check(await controller.whenIdle()==="network","preflight network category");
      check(mutations===0&&!runtime.actionState().uncertain&&host.textContent.includes("Network:uncertain=false"),"no submitted mutation");
    });
  });
  await test("JSON protocol budgets reject malformed, oversized and mislabeled replies",async()=>{
    const cases=[()=>response('{"n":1,"n":2}'),()=>response(new Uint8Array([255])),()=>response(''),()=>response('{}',200,{"Content-Type":"text/html"}),()=>response('{}',200,{"Cache-Control":"public"}),()=>response('{}',200,{ETag:'unquoted'}),()=>response('{}',200,{"Content-Length":"65537"}),()=>response(' '.repeat(65537)),()=>response('{"n":'+'1'.repeat(129)+'}')];
    for(const make of cases)await probe(1,async({runtime,host,controller})=>{
      let calls=0;globalThis.fetch=async(input,options)=>{if(!target(input))return originalFetch(input,options);calls++;return make();};
      controller.dispatch(1);check(await controller.whenIdle()==="protocol","protocol rejected");
      check(calls===1&&runtime.actionState().uncertain&&host.textContent.includes("Protocol:uncertain=true"),"submitted mutation remains uncertain");
    });
  });
  await test("GET and mutation network failures carry different uncertainty without retries",async()=>{
    for(const mode of [0,1])await probe(mode,async({runtime,host,controller})=>{
      let calls=0;globalThis.fetch=async(input,options)=>{if(!target(input))return originalFetch(input,options);calls++;throw new TypeError("simulated network failure");};
      controller.dispatch(1);check(await controller.whenIdle()==="network","network outcome");
      check(calls===1&&runtime.actionState().uncertain===(mode===1)&&host.textContent.includes(`Network:uncertain=${mode===1}`),"read versus mutation certainty");
    });
  });
  await test("JSON deadline settles once and ignores a transport that replies late",async()=>{
    for(const mode of [0,1])await probe(mode,async({runtime,host,controller})=>{
      let calls=0,release,signal;globalThis.fetch=async(input,options)=>{if(!target(input))return originalFetch(input,options);calls++;signal=options.signal;return new Promise(resolve=>{release=resolve;});};
      controller.dispatch(1);check(await controller.whenIdle()==="timeout","deadline reached");
      check(calls===1&&signal.aborted&&runtime.actionState().uncertain===(mode===1),"transport aborted and no retry");
      const text=host.textContent;release(response());await new Promise(resolve=>setTimeout(resolve,30));
      check(host.textContent===text&&runtime.stats().pendingActions===0,"late response ignored");
    },80);
  });
  await test("JSON deadlines cancel body readers and cannot submit after a delayed CSRF preflight",async()=>{
    await probe(1,async({runtime,controller})=>{
      let cancelled=0;globalThis.fetch=async(input,options)=>{
        if(!target(input))return originalFetch(input,options);
        return response(new ReadableStream({start(stream){stream.enqueue(new TextEncoder().encode('{"n":'));},cancel(){cancelled++;}}));
      };
      controller.dispatch(1);check(await controller.whenIdle()==="timeout","body deadline");
      check(cancelled===1&&runtime.actionState().uncertain,"body reader cancelled after submitted mutation");
    },80);
    await probe(1,async({runtime,host,controller})=>{
      let release,mutations=0;globalThis.fetch=async(input,options)=>{
        if(pathOf(input)===csrfEndpoint)return new Promise(resolve=>{release=resolve;});
        if(target(input))mutations++;return originalFetch(input,options);
      };
      controller.dispatch(1);check(await controller.whenIdle()==="timeout"&&!runtime.actionState().uncertain,"preflight deadline remains certain");
      check(host.textContent.includes("Timeout:uncertain=false"),"Rust preflight certainty");
      release(new Response(null,{status:200,headers:{"Content-Type":bridge.CSRF_MEDIA_TYPE,"Cache-Control":"no-store","Content-Length":"0"}}));
      await new Promise(resolve=>setTimeout(resolve,30));check(mutations===0,"late preflight cannot submit a mutation");
    },80);
  });
  await test("disposing JSON owners aborts transport and late replies cannot update a replacement",async()=>{
    for(const mode of [0,1])await probe(mode,async({runtime,host,controller})=>{
      let release,signal,calls=0;globalThis.fetch=async(input,options)=>{if(!target(input))return originalFetch(input,options);calls++;signal=options.signal;return new Promise(resolve=>{release=resolve;});};
      controller.dispatch(1);await waitFor(()=>calls===1);const idle=controller.whenIdle();controller.dispose();
      check(signal.aborted&&await idle==="disposed","live owner aborts and settles");
      const replacement=runtime.mount(host,{component:28,state:{kind:"number",value:mode}});
      release(response());await new Promise(resolve=>setTimeout(resolve,30));
      check(host.querySelector("output").textContent==="idle"&&runtime.stats().views===1&&runtime.actionState().uncertain===(mode===1),"new owner unaffected by stale response");
      replacement.dispose();
    });
  });
  await test("Rust task editor preserves drafts and versions across validation and conflicts",async()=>{
    const host=document.createElement("section");panel.append(host);const controller=ctx.runtime.mount(host,{component:27,state:{kind:"number",value:0}});
    try {
      controller.dispatch(1);check(await controller.whenIdle()==="success","demo session and initial read");
      controller.dispatch(11,"Rust client task 🦀");controller.dispatch(3);check(await controller.whenIdle()==="success","Rust create and explicit list read");
      const buttons=host.querySelectorAll("li button"),last=buttons.length-1;check(last>=0,"created task visible");
      buttons[last].click();check(await controller.whenIdle()==="success","selected snapshot loaded");
      controller.dispatch(11,"Draft rejected by server");
      globalThis.fetch=async(input,options)=>target(input)?originalFetch(input,options):pathOf(input).startsWith("/api/tasks/")&&options?.method==="PATCH"?response('{"error":"Bad Request","errors":[{"field":"title","code":"invalid_value"}],"truncated":false}',400):originalFetch(input,options);
      controller.dispatch(5);check(await controller.whenIdle()==="validation","structured server validation");
      check(host.querySelector("input").value==="Draft rejected by server"&&host.textContent.includes("This value is invalid."),"remote report mapped in Rust without losing draft");
      check(![...host.querySelectorAll("button")].find(button=>button.textContent==="Save title (PATCH)").disabled,"retained version permits explicit correction");
      globalThis.fetch=originalFetch;
      controller.dispatch(11,"   ");controller.dispatch(12,true);const before=await stats();controller.dispatch(4);
      check(host.querySelector("input").value==="   "&&host.querySelector('input[type="checkbox"]').checked&&host.querySelector("input").getAttribute("aria-invalid")==="true","invalid draft retained");
      check((await stats()).writes===before.writes&&host.textContent.includes("Enter a nonblank value."),"shared Rust rules prevent invalid write");
      controller.dispatch(11,"Corrected 🦀 <b>plain text</b>");controller.dispatch(4);check(await controller.whenIdle()==="success","explicit PUT correction");
      check(host.querySelector("input").value==="Corrected 🦀 <b>plain text</b>"&&!host.querySelector("b"),"Rust model updated without markup injection");
      controller.dispatch(6);check(await controller.whenIdle()==="success","PATCH state");
      const row=host.querySelectorAll("li button")[last].parentElement,id=/^#([0-9]+)/.exec(row.textContent)[1];
      const snapshot=await originalFetch(`/api/tasks/${id}`,{cache:"no-store"}),tag=snapshot.headers.get("etag");await snapshot.arrayBuffer();
      const claim=await ctx.token(),other=await originalFetch(`/api/tasks/${id}`,{method:"PATCH",headers:{"Content-Type":"application/json","Ruvane-Csrf-Token":claim,"If-Match":tag},body:'{"title":"Another editor"}'});check(other.status===200,"concurrent writer");await other.arrayBuffer();
      controller.dispatch(11,"My retained draft");controller.dispatch(5);check(await controller.whenIdle()==="conflict","stale PATCH refused");
      check(host.querySelector("input").value==="My retained draft"&&host.textContent.includes("Draft retained"),"conflict keeps draft");
      check([...host.querySelectorAll("button")].find(button=>button.textContent==="Save title (PATCH)").disabled,"no stale resubmission");
      controller.dispatch(8);check(await controller.whenIdle()==="success"&&host.querySelector("input").value==="Another editor","explicit reload replaces snapshot");
      controller.dispatch(7);check(await controller.whenIdle()==="success","conditional DELETE");
      check(!host.textContent.includes(`#${id} `),"deleted task removed");
    } finally {globalThis.fetch=originalFetch;controller.dispose();host.remove();}
  });
}

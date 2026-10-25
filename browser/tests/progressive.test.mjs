import test from "node:test";
import assert from "node:assert/strict";
import { readNavigationStream } from "../../web/navigation.mjs";
import { decodePage, NAVIGATION_STREAM_MEDIA_TYPE } from "../../web/bridge.mjs";

const word=n=>[n&255,n>>>8&255,n>>>16&255,n>>>24&255];
const text=value=>{const bytes=new TextEncoder().encode(value);return [...word(bytes.length),...bytes];};
const empty=[82,86,65,49,0,0,0];
function packet({pending=1,layouts=0,key="/tasks",component=17,status=200,failed=null,url="/tasks/project"}={}){
  const version=pending!==null?4:failed!==null?3:layouts?2:1;
  return new Uint8Array([82,86,80,48+version,...(version===1?[]:[0]),...(version===3?[failed]:version===4?[pending]:[]),status&255,status>>>8,
    ...text(url),...text("Page"),...word(component),...word(1),...word(status===200?0:status),...word(empty.length),...empty,
    ...(version===1?[]:[layouts]),...(layouts?[...text("workspace"),...text(key),...word(12),...word(1),...word(0),...word(empty.length),...empty]:[])]);
}
function wire(packets=[packet(),packet({layouts:1}),packet({pending:null,layouts:1})],{terminal=true,revision=42n,token=3n}={}){
  const header=new Uint8Array(20),view=new DataView(header.buffer);header.set([82,86,83,49]);view.setBigUint64(4,revision,true);view.setBigUint64(12,token,true);
  return new Uint8Array([...header,...packets.flatMap(p=>[...word(p.length),...p]),...(terminal?[0,0,0,0]:[])]);
}
function response(bytes,step=bytes.length,headers={}){
  return new Response(new ReadableStream({start(controller){for(let n=0;n<bytes.length;n+=step)controller.enqueue(bytes.slice(n,n+step));controller.close();}}),{headers:{"Content-Type":NAVIGATION_STREAM_MEDIA_TYPE,"Cache-Control":"no-store",...headers}});
}
const options=prefix=>({signal:new AbortController().signal,url:"/tasks/project",revision:"42",token:"3",prefix:prefix??(()=>{})});

test("RVS1 handles every byte boundary and exposes only complete ordered prefixes",async()=>{
  for(const step of [1,2,3,7,19,64,65536]){
    const counts=[],result=await readNavigationStream(response(wire(),step),options((bytes,page)=>{assert.equal(decodePage(bytes).pending,1);counts.push(page.layouts.length);}));
    assert.deepEqual(counts,[0,1]);assert.equal(result.page.pending,null);assert.equal(result.page.layouts.length,1);assert.equal(result.page.status,200);
  }
});
test("identity, destination, sequence and immutable parents are mandatory",async()=>{
  const cases=[wire(undefined,{revision:43n}),wire(undefined,{token:4n}),wire([packet({url:"/other"})]),wire([packet(),packet()]),wire([packet({layouts:1})]),wire([packet(),packet({layouts:1}),packet({layouts:1,pending:null,key:"/other"})]),wire([packet(),packet({layouts:1,pending:2})]),wire([packet(),packet({layouts:1,component:18})]),wire([packet(),packet({pending:null,layouts:0})])];
  for(const bytes of cases)await assert.rejects(readNavigationStream(response(bytes),options()));
});
test("an explicit end and clean EOF are required after exactly one final packet",async()=>{
  const normal=wire();
  for(const bytes of [wire(undefined,{terminal:false}),normal.slice(0,-1),new Uint8Array([...normal,1]),wire([packet(),packet({layouts:1}),packet({pending:null,layouts:1}),packet({pending:null,layouts:1})]),wire([])])await assert.rejects(readNavigationStream(response(bytes),options()));
});
test("scoped errors may retain only the already validated prefix",async()=>{
  const result=await readNavigationStream(response(wire([packet(),packet({layouts:1}),packet({pending:null,layouts:1,failed:1,status:500,component:16})])),options());
  assert.equal(result.page.status,500);assert.equal(result.page.failed,1);
  await assert.rejects(readNavigationStream(response(wire([packet(),packet({pending:null,layouts:1,failed:1,status:500,component:16})])),options()));
});
test("MIME, status, length, frame and aggregate budgets fail closed",async()=>{
  await assert.rejects(readNavigationStream(response(wire(),1000,{"Content-Type":"text/html"}),options()));
  await assert.rejects(readNavigationStream(response(wire(),1000,{"Content-Length":"1"}),options()));
  await assert.rejects(readNavigationStream(response(wire(),1000,{"Cache-Control":"public, max-age=30"}),options()));
  const header=wire([],{terminal:false});
  await assert.rejects(readNavigationStream(response(new Uint8Array([...header,...word(65533)])),options()));
  await assert.rejects(readNavigationStream(response(new Uint8Array(524289)),options()));
});
test("abort and rejected prefix validation retire the reader before later packets",async()=>{
  const abort=new AbortController();let pulls=0;
  await assert.rejects(readNavigationStream(response(wire()),{...options(),signal:abort.signal,prefix(){pulls++;abort.abort();}}));assert.equal(pulls,1);
  await assert.rejects(readNavigationStream(response(wire()),options(()=>{throw new Error("Schema rejected");})));
});
test("pending packets cannot carry a leaf seed or exceed their declared chain",()=>{
  const invalid=packet();invalid[5]=7;assert.throws(()=>decodePage(invalid));
  const bad=packet({pending:0,layouts:1});assert.throws(()=>decodePage(bad));
});

import test from "node:test";
import assert from "node:assert/strict";
import {decodePage,decodeNavigationSnapshot} from "../../web/bridge.mjs";
const word=n=>[n&255,n>>>8&255,n>>>16&255,n>>>24&255];
const text=value=>{const bytes=new TextEncoder().encode(value);return [...word(bytes.length),...bytes];};
const packet=(status=200)=>new Uint8Array([82,86,80,49,status&255,status>>>8,...text("/meta"),...text("Metadata page"),...word(8),...word(1),...word(0),...word(7),82,86,65,49,0,0,0]);
const envelope=(inner,fields)=>new Uint8Array([82,86,80,53,...word(inner.length),...inner,...fields]);
test("RVP5 decodes typed metadata and keeps the RVI1 snapshot envelope",()=>{
  const bytes=envelope(packet(),[15,...text("Rust <script> & 🦀"),2,...text("https://public.example/meta"),...text("/icon.png")]);
  const page=decodePage(bytes);assert.deepEqual(page.metadata,{description:"Rust <script> & 🦀",robots:"index, nofollow",canonical:"https://public.example/meta",favicon:"/icon.png"});assert.ok(Object.isFrozen(page.metadata));
  const snapshot=new Uint8Array([82,86,73,49,...word(42),...word(0),...word(bytes.length),...bytes]);assert.deepEqual(decodeNavigationSnapshot(snapshot).page.metadata,page.metadata);
  assert.equal(decodePage(packet()).metadata,undefined);
});
test("metadata rejects truncation, nested packets, invalid flags, UTF-8 and trailing bytes",()=>{
  const bytes=envelope(packet(),[1,...text("Metadata")]);
  for(let n=0;n<bytes.length;n++)assert.throws(()=>decodePage(bytes.slice(0,n)));
  assert.throws(()=>decodePage(envelope(bytes,[0])));
  for(const fields of [[16],[1,...word(1),255],[2,0],[2,5],[0,0],[1,...text("")],[1,...text("bad\ntext")]])assert.throws(()=>decodePage(envelope(packet(),fields)));
  assert.throws(()=>decodePage(envelope(packet(),[1,...text("x".repeat(1025))])));
});
test("metadata rejects unsafe URLs and indexing or canonicals on error packets",()=>{
  for(const canonical of ["javascript:alert(1)","https://user@public.example/","https://public.example/../x","https://public.example/%2e%2e/x"])assert.throws(()=>decodePage(envelope(packet(),[4,...text(canonical)])));
  assert.throws(()=>decodePage(envelope(packet(),[8,...text("https://other.example/icon.png")])));
  assert.throws(()=>decodePage(envelope(packet(404),[2,1])));
  assert.throws(()=>decodePage(envelope(packet(500),[6,3,...text("https://public.example/")])));
  assert.equal(decodePage(envelope(packet(404),[2,4])).metadata.robots,"noindex, nofollow");
});

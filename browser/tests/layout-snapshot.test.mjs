import test from "node:test";
import assert from "node:assert/strict";
import { decodeLayoutSnapshot, decodeNavigationSnapshot } from "../../web/bridge.mjs";
const word=n=>[n&255,n>>>8&255,n>>>16&255,n>>>24&255];
const text=value=>{const bytes=new TextEncoder().encode(value);return [...word(bytes.length),...bytes];};
const seed=(component=12)=>[...word(component),...word(1),...word(0),...word(7),82,86,65,49,0,0,0];
function packet({version=4,selected=0,expected=2,count=1}={}){return new Uint8Array([82,86,80,48+version,selected,...(version===4?[expected]:[]),200,0,...text("/tasks/project?q=%3Cscript%3E"),...text("Project 🦀"),...seed(17),count,...Array.from({length:count},(_,n)=>[...text("layout"+n),...text("/tasks"),...seed()]).flat()]);}
function envelope(bytes=packet(),revision=9007199254740993n){const out=new Uint8Array(16+bytes.length),view=new DataView(out.buffer);out.set([82,86,72,49]);view.setBigUint64(4,revision,true);view.setUint32(12,bytes.length,true);out.set(bytes,16);return out;}
test("RVH1 identifies the completed layout and exact unsigned revision",()=>{
  for(const revision of [0n,1n,9007199254740993n,0xffffffffffffffffn])for(const count of [1,2]){const decoded=decodeLayoutSnapshot(envelope(packet({count}),revision));assert.equal(decoded.revision,String(revision));assert.equal(decoded.page.pending,2);assert.equal(decoded.page.layouts.length,count);assert.equal(decoded.page.url,"/tasks/project?q=%3Cscript%3E");}
});
test("RVH1 rejects every incomplete envelope and mismatched frame length",()=>{const bytes=envelope();for(let n=0;n<bytes.length;n++)assert.throws(()=>decodeLayoutSnapshot(bytes.slice(0,n)));assert.throws(()=>decodeLayoutSnapshot(new Uint8Array([...bytes,0])));for(const index of [0,3,12,13,14,15,16]){const bad=bytes.slice();bad[index]^=255;assert.throws(()=>decodeLayoutSnapshot(bad));}});
test("complete pages, empty manifests and selected layouts cannot be hydration records",()=>{for(const options of [{version:2},{count:0},{selected:1},{count:2,expected:1},{expected:7}])assert.throws(()=>decodeLayoutSnapshot(envelope(packet(options))));});
test("layout and complete initial envelopes cannot replace each other",()=>{const bytes=envelope();assert.throws(()=>decodeNavigationSnapshot(bytes));bytes[2]=73;assert.throws(()=>decodeNavigationSnapshot(bytes));assert.throws(()=>decodeLayoutSnapshot(bytes));});
test("layout envelope types and byte budgets are checked before decoding",()=>{for(const value of [null,{},new ArrayBuffer(100),new Uint8Array(65553)])assert.throws(()=>decodeLayoutSnapshot(value));});
test("decoded prefix bytes do not alias later envelope edits",()=>{const bytes=envelope(),decoded=decodeLayoutSnapshot(bytes);bytes.fill(0);assert.deepEqual(decoded.bytes,packet());assert.equal(decoded.page.layouts[0].name,"layout0");});

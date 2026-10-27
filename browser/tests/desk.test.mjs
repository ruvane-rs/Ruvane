import test from 'node:test';
import assert from 'node:assert/strict';
import {spawn,execFileSync} from 'node:child_process';
import {mkdtemp,mkdir,readFile,writeFile,rename,rm,realpath,readdir} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {deskFixture} from './desk-fixture.mjs';
import {readinessURL} from './readiness.mjs';
import {validateExports,decodePage,decodeCommands,NAVIGATION_MEDIA_TYPE} from '../../web/bridge.mjs';
const root=fileURLToPath(new URL('../../',import.meta.url)),cli=resolve(process.env.RUVANE_CLI||join(root,'target/debug/ruvane')),pause=ms=>new Promise(r=>setTimeout(r,ms));
test('Desk pilot works from a source-hidden standalone CLI bundle', {timeout:240000,skip:!(process.platform==='darwin'||process.platform==='linux'&&['x64','arm64'].includes(process.arch))},async t=>{
 const keep=process.env.RUVANE_KEEP_DESK_FIXTURE,directory=keep?resolve(keep):await mkdtemp(join(await realpath(tmpdir()),'ruvane-desk-'));if(keep)await mkdir(directory,{recursive:true});
 const storage=join(directory,'desk-data');let fixture,child,base,hidden,generation=0,owner,first,version;
 async function stop(){if(!child)return;const current=child;child=null;if(current.exitCode!==null)return;const ended=new Promise(r=>current.once('exit',r));current.stdin.end();await ended;}
 async function start(){let logs='';const ready=join(directory,`ready-${generation++}`);child=spawn(join(fixture.bundle,'ruvane-app'),['--ruvane-serve','0',ready],{cwd:fixture.bundle,env:{...process.env,RUVANE_DESK_DATA:storage},stdio:['pipe','ignore','pipe']});child.stderr.on('data',b=>logs+=b);for(let n=0;n<1000;n++){let record='';try{record=await readFile(ready,'utf8');}catch(e){if(e.code!=='ENOENT')throw e;}const value=readinessURL(record);if(value){base=value;return;}if(child.exitCode!==null)throw Error(logs);await pause(10);}throw Error('Desk readiness timeout');}
 const request=(path,session,options={})=>fetch(base+path,{...options,redirect:'manual',headers:{...(session?{Cookie:session.cookie}:{}),...options.headers}});
 async function session(){const r=await request('/session',null,{method:'POST',headers:{Origin:base,'Content-Type':'application/x-www-form-urlencoded'},body:''});assert.equal(r.status,303);const cookie=r.headers.getSetCookie()[0].split(';')[0];await r.arrayBuffer();const csrf=await request('/csrf',{cookie});await csrf.arrayBuffer();return {cookie,token:csrf.headers.get('Ruvane-Csrf-Token')};}
 const input=(title='First <&🦀',state='open')=>({title,description:'Description\nwith tabs\tand details',state});
 const json=(path,method,body,tag,session=owner)=>request(path,session,{method,headers:{Origin:base,'Content-Type':'application/json','Ruvane-Csrf-Token':session.token,...(tag?{'If-Match':tag}:{})},body:JSON.stringify(body)});
 async function read(id=1){const r=await request('/api/tickets/'+id,owner);assert.equal(r.status,200);assert.equal(r.headers.get('cache-control'),'no-store');return {data:await r.json(),tag:r.headers.get('etag')};}
 try {
  fixture=await deskFixture({directory,root,cli});
  await t.test('build has no business storage side effects and only first-party Cargo packages',async()=>{
   assert.equal((await readdir(fixture.project)).includes('desk-data'),false);
   const graph=JSON.parse(execFileSync(process.env.CARGO||'cargo',['metadata','--offline','--locked','--format-version','1'],{cwd:fixture.project,encoding:'utf8'}));
   assert.deepEqual(graph.packages.map(p=>p.name).sort(),['desk','ruvane','ruvane-application-browser','ruvane-browser']);assert.ok(graph.packages.every(p=>p.source===null&&p.version==='0.1.0-alpha.61'));
  });
  const runtimePath=join(fixture.bundle,'resources/ruvane-runtime',BigInt(fixture.revision).toString(16).padStart(16,'0'));
  const wasm=await readFile(join(runtimePath,'app.wasm')),module=await WebAssembly.compile(wasm),e=validateExports(new WebAssembly.Instance(module,{}).exports);
  await t.test('application Wasm retains ABI 15, no imports and four runtime assets',async()=>{assert.deepEqual(WebAssembly.Module.imports(module),[]);assert.equal(e.ruvane_abi_version(),15);assert.equal(WebAssembly.Module.exports(module).filter(v=>v.kind==='function').length,22);assert.ok(wasm.length<1048576);assert.equal(wasm.includes(Buffer.from('ruvane-desk\t1\t')),false);assert.equal(wasm.includes(Buffer.from('ruvane_desk')),false);assert.deepEqual((await readdir(runtimePath)).sort(),['app.wasm','application.mjs','bridge.mjs','navigation.mjs']);});
  hidden=join(fixture.project,'source-hidden');await rename(join(fixture.project,'src'),hidden);await start();
  await t.test('anonymous access is denied and a local session is explicitly identified',async()=>{const landing=await request('/');assert.match(await landing.text(),/does not verify your identity/);for(const path of ['/tickets','/tickets/new','/tickets/1','/api/tickets/1']){const r=await request(path);assert.equal(r.status,403);assert.equal(r.headers.get('cache-control'),'no-store');assert.doesNotMatch(await r.text(),/First|Description/);}owner=await session();assert.ok(owner.token);});
  await t.test('invalid JSON, origin and CSRF checks leave the store empty',async()=>{
   for(const body of [input(''),{...input(),state:'done'},{...input(),extra:1},{...input(),description:'bad\rcontrol'},{...input(),title:'é'.repeat(61)}]){const r=await json('/api/tickets','POST',body);assert.equal(r.status,400);await r.arrayBuffer();}
   for(const headers of [{Origin:'https://foreign.invalid','Ruvane-Csrf-Token':owner.token},{Origin:base},{Origin:base,'Ruvane-Csrf-Token':'invalid'}]){const r=await request('/api/tickets',owner,{method:'POST',headers:{'Content-Type':'application/json',...headers},body:JSON.stringify(input())});assert.equal(r.status,403);await r.arrayBuffer();}
   const list=await request('/tickets',owner);assert.match(await list.text(),/0 tickets/);
  });
  await t.test('create returns validated business data and private SSR escapes its fields',async()=>{const r=await json('/api/tickets','POST',input());assert.equal(r.status,200);version=r.headers.get('etag');first=await r.json();assert.deepEqual(first,{...input(),id:1,revision:1});assert.ok(version&&!version.startsWith('W/'));const page=await request('/tickets/1',owner);assert.equal(page.status,200);assert.equal(page.headers.get('cache-control'),'no-store');const html=await page.text();assert.match(html,/First &lt;&amp;🦀/);assert.match(html,/data-ruvane-version="15"/);assert.doesNotMatch(html,/store\.bin|desk-data/);});
  await t.test('source-hidden navigation seeds mount the same editor in actual Wasm',async()=>{
   const r=await request('/tickets/1',owner,{headers:{Accept:NAVIGATION_MEDIA_TYPE,'Ruvane-Application-Revision':fixture.revision}});assert.equal(r.status,200);assert.equal(r.headers.get('cache-control'),'no-store');const bytes=new Uint8Array(await r.arrayBuffer());assert.equal(decodePage(bytes).component,2);
   assert.equal(e.ruvane_page_begin(bytes.length),1);for(let n=0;n<bytes.length;n+=4){const chunk=bytes.slice(n,n+4);assert.equal(e.ruvane_page_chunk(chunk.reduce((s,b,i)=>s+b*2**(i*8),0),chunk.length),1);}const id=e.ruvane_page_mount(1);assert.ok(id);const commands=decodeCommands(new Uint8Array(e.memory.buffer,e.ruvane_commands_ptr(),e.ruvane_commands_len()).slice());assert.ok(commands.some(c=>c.value===first.title));e.ruvane_commands_clear();assert.equal(e.ruvane_dispose(id),1);e.ruvane_commands_clear();assert.equal(e.ruvane_live_instances(),0);
  });
  await t.test('two concurrent PATCH requests accept one writer and reject the stale draft',async()=>{const replies=await Promise.all(['Tab A','Tab B'].map(title=>json('/api/tickets/1','PATCH',input(title,'in_progress'),version)));assert.deepEqual(replies.map(r=>r.status).sort(),[200,412]);for(const r of replies)await r.arrayBuffer();const saved=await read();assert.equal(saved.data.revision,2);assert.ok(['Tab A','Tab B'].includes(saved.data.title));version=saved.tag;first=saved.data;
   for(const tag of [null,'*','W/'+version]){const r=await json('/api/tickets/1','PATCH',input('Reject'),tag);assert.equal(r.status,tag?.startsWith('W/')?412:428);await r.arrayBuffer();}assert.deepEqual((await read()).data,first);
  });
  await t.test('native forms create and edit without scripts, with 303 redirects',async()=>{
   const native=await request('/tickets/new?native=yes',owner);const html=await native.text();assert.equal(native.status,200);assert.doesNotMatch(html,/<script/);assert.match(html,/<textarea/);
   const form=new URLSearchParams({...input('Native ticket'),_ruvane_csrf:owner.token});
   let r=await request('/tickets?native=yes',owner,{method:'POST',headers:{Origin:base,'Content-Type':'application/x-www-form-urlencoded',},body:form});assert.equal(r.status,303);assert.equal(r.headers.get('location'),'/tickets/2?native=yes');await r.arrayBuffer();
   const saved=await read(2);form.set('title','Native updated');form.set('state','closed');form.set('version',saved.tag);
   r=await request('/tickets/2?native=yes',owner,{method:'POST',headers:{Origin:base,'Content-Type':'application/x-www-form-urlencoded',},body:form});assert.equal(r.status,303);await r.arrayBuffer();assert.equal((await read(2)).data.state,'closed');
  });
  await t.test('native stale and invalid submissions retain drafts and never overwrite data',async()=>{
   for(const [title,tag,expected] of [['My unsaved <& draft','"stale"',412],['',version,400]]){
    const form=new URLSearchParams({...input(title),version:tag});const r=await request('/tickets/1?native=yes',owner,{method:'POST',headers:{Origin:base,'Content-Type':'application/x-www-form-urlencoded','Ruvane-Csrf-Token':owner.token},body:form});assert.equal(r.status,expected);const html=await r.text();assert.match(html,/value="Description|Description/);assert.match(html,/name="version"/);if(title)assert.match(html,/My unsaved &lt;&amp; draft/);
   }
   assert.deepEqual((await read()).data,first);
  });
  await t.test('pagination stays bounded and invalid identifiers and queries fail closed',async()=>{
   for(let i=3;i<=4;i++){const r=await json('/api/tickets','POST',input('Ticket '+i));assert.equal(r.status,200);await r.arrayBuffer();}
   for(const [path,status] of [['/tickets?page=2',200],['/tickets?page=01',400],['/tickets?page=0',400],['/tickets?page=44',400],['/tickets?page=1&page=2',400],['/tickets?owner=x',400],['/tickets/01',400],['/tickets/5',404]]){const r=await request(path,owner);assert.equal(r.status,status,path);await r.arrayBuffer();}
  });
  await t.test('clean restart preserves tickets but expires sessions and old write validators',async()=>{await stop();assert.equal((await readdir(storage)).includes('LOCK'),false);await start();let r=await request('/tickets',owner);assert.equal(r.status,403);await r.arrayBuffer();owner=await session();assert.deepEqual((await read()).data,first);r=await json('/api/tickets/1','PATCH',input('Before restart'),version);assert.equal(r.status,412);await r.arrayBuffer();const page=await request('/tickets?page=2',owner);assert.match(await page.text(),/Ticket 4/);});
  await t.test('explicit offline recovery preserves a valid interrupted snapshot',async()=>{await stop();await writeFile(join(storage,'LOCK'),'RDL1\n',{mode:0o600});execFileSync(join(fixture.bundle,'ruvane-app'),['--recover',storage],{cwd:fixture.bundle,encoding:'utf8'});assert.equal((await readdir(storage)).includes('LOCK'),false);await start();owner=await session();assert.deepEqual((await read()).data,first);});
  await t.test('closing a local session revokes its cookie and CSRF capability',async()=>{
   const form=new URLSearchParams({_ruvane_csrf:owner.token});
   const closed=await request('/session/close',owner,{method:'POST',headers:{Origin:base,'Content-Type':'application/x-www-form-urlencoded'},body:form});
   assert.equal(closed.status,303);assert.equal(closed.headers.get('location'),'/');assert.match(closed.headers.getSetCookie()[0],/Max-Age=0/);await closed.arrayBuffer();
   const denied=await request('/api/tickets/1',owner);assert.equal(denied.status,403);await denied.arrayBuffer();
   const csrf=await request('/csrf',owner);assert.equal(csrf.headers.get('Ruvane-Csrf-Token'),null);await csrf.arrayBuffer();
  });
 } finally {
  await stop();if(keep&&fixture)await writeFile(join(directory,'fixture.json'),JSON.stringify({...fixture,storage},null,2));if(hidden&&!keep)await rename(hidden,join(fixture.project,'src'));if(!keep)await rm(directory,{recursive:true,force:true});
 }
});

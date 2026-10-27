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
 const storage=join(directory,'desk-data'),legacyStorage=join(directory,'legacy-data');let activeStorage=storage;let fixture,child,base,hidden,generation=0,owner,first,version,editor,viewer,outsider,sandbox,uploadOperation,uploaded;
 async function stop(){if(!child)return;const current=child;child=null;if(current.exitCode!==null)return;const ended=new Promise(r=>current.once('exit',r));current.stdin.end();await ended;}
 async function start(){let logs='';const ready=join(directory,`ready-${generation++}`);child=spawn(join(fixture.bundle,'ruvane-app'),['--ruvane-serve','0',ready],{cwd:fixture.bundle,env:{...process.env,RUVANE_DESK_DATA:activeStorage},stdio:['pipe','ignore','pipe']});child.stderr.on('data',b=>logs+=b);for(let n=0;n<1000;n++){let record='';try{record=await readFile(ready,'utf8');}catch(e){if(e.code!=='ENOENT')throw e;}const value=readinessURL(record);if(value){base=value;return;}if(child.exitCode!==null)throw Error(logs);await pause(10);}throw Error('Desk readiness timeout');}
 const request=(path,session,options={})=>fetch(base+path,{...options,redirect:'manual',headers:{...(session?{Cookie:session.cookie}:{}),...options.headers}});
 async function session(profile="alex"){const r=await request('/session',null,{method:'POST',headers:{Origin:base,'Content-Type':'application/x-www-form-urlencoded'},body:new URLSearchParams({profile})});assert.equal(r.status,303);const cookie=r.headers.getSetCookie()[0].split(';')[0];await r.arrayBuffer();const csrf=await request('/csrf',{cookie});await csrf.arrayBuffer();return {cookie,token:csrf.headers.get('Ruvane-Csrf-Token')};}
 const input=(title='First <&🦀',state='open')=>({title,description:'Description\nwith tabs\tand details',state});
 const json=(path,method,body,tag,session=owner)=>request(path,session,{method,headers:{Origin:base,'Content-Type':'application/json','Ruvane-Csrf-Token':session.token,...(tag?{'If-Match':tag}:{})},body:JSON.stringify(body)});
 async function read(id=1){const r=await request('/api/w/1/tickets/'+id,owner);assert.equal(r.status,200);assert.equal(r.headers.get('cache-control'),'no-store');return {data:await r.json(),tag:r.headers.get('etag')};}
 try {
  fixture=await deskFixture({directory,root,cli});
  await t.test('build has no business storage side effects and only first-party Cargo packages',async()=>{
   assert.equal((await readdir(fixture.project)).includes('desk-data'),false);
   const graph=JSON.parse(execFileSync(process.env.CARGO||'cargo',['metadata','--offline','--locked','--format-version','1'],{cwd:fixture.project,encoding:'utf8'}));
   assert.deepEqual(graph.packages.map(p=>p.name).sort(),['desk','ruvane','ruvane-application-browser','ruvane-browser']);assert.ok(graph.packages.every(p=>p.source===null&&p.version==='0.1.0-alpha.62'));
  });
  const runtimePath=join(fixture.bundle,'resources/ruvane-runtime',BigInt(fixture.revision).toString(16).padStart(16,'0'));
  const wasm=await readFile(join(runtimePath,'app.wasm')),module=await WebAssembly.compile(wasm),e=validateExports(new WebAssembly.Instance(module,{}).exports);
  await t.test('application Wasm retains ABI 15, no imports and four runtime assets',async()=>{assert.deepEqual(WebAssembly.Module.imports(module),[]);assert.equal(e.ruvane_abi_version(),15);assert.equal(WebAssembly.Module.exports(module).filter(v=>v.kind==='function').length,22);assert.ok(wasm.length<1048576);assert.equal(wasm.includes(Buffer.from('ruvane-desk\t1\t')),false);assert.equal(wasm.includes(Buffer.from('ruvane_desk')),false);assert.deepEqual((await readdir(runtimePath)).sort(),['app.wasm','application.mjs','bridge.mjs','navigation.mjs']);});
  hidden=join(fixture.project,'source-hidden');await rename(join(fixture.project,'src'),hidden);await start();
  await t.test('anonymous access is denied and a local session is explicitly identified',async()=>{const landing=await request('/');assert.match(await landing.text(),/does not verify your identity/);for(const path of ['/w/1/tickets','/w/1/new-ticket','/w/1/tickets/1','/api/w/1/tickets/1']){const r=await request(path);assert.equal(r.status,403);assert.equal(r.headers.get('cache-control'),'no-store');assert.doesNotMatch(await r.text(),/First|Description/);}owner=await session();assert.ok(owner.token);});
  await t.test('invalid JSON, origin and CSRF checks leave the store empty',async()=>{
   for(const body of [input(''),{...input(),state:'done'},{...input(),extra:1},{...input(),description:'bad\rcontrol'},{...input(),title:'é'.repeat(61)}]){const r=await json('/api/w/1/tickets','POST',body);assert.equal(r.status,400);await r.arrayBuffer();}
   for(const headers of [{Origin:'https://foreign.invalid','Ruvane-Csrf-Token':owner.token},{Origin:base},{Origin:base,'Ruvane-Csrf-Token':'invalid'}]){const r=await request('/api/w/1/tickets',owner,{method:'POST',headers:{'Content-Type':'application/json',...headers},body:JSON.stringify(input())});assert.equal(r.status,403);await r.arrayBuffer();}
   const list=await request('/w/1/tickets',owner);assert.match(await list.text(),/0 tickets/);
  });
  await t.test('create returns validated business data and private SSR escapes its fields',async()=>{const r=await json('/api/w/1/tickets','POST',input());assert.equal(r.status,200);version=r.headers.get('etag');first=await r.json();assert.deepEqual(first,{...input(),id:1,workspace:1,revision:1});assert.ok(version&&!version.startsWith('W/'));const page=await request('/w/1/tickets/1',owner);assert.equal(page.status,200);assert.equal(page.headers.get('cache-control'),'no-store');const html=await page.text();assert.match(html,/First &lt;&amp;🦀/);assert.match(html,/data-ruvane-version="15"/);assert.doesNotMatch(html,/store\.bin|desk-data/);});
  await t.test('source-hidden navigation seeds mount the same editor in actual Wasm',async()=>{
   const r=await request('/w/1/tickets/1',owner,{headers:{Accept:NAVIGATION_MEDIA_TYPE,'Ruvane-Application-Revision':fixture.revision}});assert.equal(r.status,200);assert.equal(r.headers.get('cache-control'),'no-store');const bytes=new Uint8Array(await r.arrayBuffer());assert.equal(decodePage(bytes).component,2);
   assert.equal(e.ruvane_page_begin(bytes.length),1);for(let n=0;n<bytes.length;n+=4){const chunk=bytes.slice(n,n+4);assert.equal(e.ruvane_page_chunk(chunk.reduce((s,b,i)=>s+b*2**(i*8),0),chunk.length),1);}const id=e.ruvane_page_mount(1);assert.ok(id);const commands=decodeCommands(new Uint8Array(e.memory.buffer,e.ruvane_commands_ptr(),e.ruvane_commands_len()).slice());assert.ok(commands.some(c=>c.value===first.title));e.ruvane_commands_clear();assert.equal(e.ruvane_dispose(id),1);e.ruvane_commands_clear();assert.equal(e.ruvane_live_instances(),0);
  });
  await t.test('two concurrent PATCH requests accept one writer and reject the stale draft',async()=>{const replies=await Promise.all(['Tab A','Tab B'].map(title=>json('/api/w/1/tickets/1','PATCH',input(title,'in_progress'),version)));assert.deepEqual(replies.map(r=>r.status).sort(),[200,412]);for(const r of replies)await r.arrayBuffer();const saved=await read();assert.equal(saved.data.revision,2);assert.ok(['Tab A','Tab B'].includes(saved.data.title));version=saved.tag;first=saved.data;
   for(const tag of [null,'*','W/'+version]){const r=await json('/api/w/1/tickets/1','PATCH',input('Reject'),tag);assert.equal(r.status,tag?.startsWith('W/')?412:428);await r.arrayBuffer();}assert.deepEqual((await read()).data,first);
  });
  await t.test('native forms create and edit without scripts, with 303 redirects',async()=>{
   const native=await request('/w/1/new-ticket?native=yes',owner);const html=await native.text();assert.equal(native.status,200);assert.doesNotMatch(html,/<script/);assert.match(html,/<textarea/);
   const form=new URLSearchParams({...input('Native ticket'),_ruvane_csrf:owner.token});
   let r=await request('/w/1/tickets?native=yes',owner,{method:'POST',headers:{Origin:base,'Content-Type':'application/x-www-form-urlencoded',},body:form});assert.equal(r.status,303);assert.equal(r.headers.get('location'),'/w/1/tickets/2?native=yes');await r.arrayBuffer();
   const saved=await read(2);form.set('title','Native updated');form.set('state','closed');form.set('version',saved.tag);
   r=await request('/w/1/tickets/2?native=yes',owner,{method:'POST',headers:{Origin:base,'Content-Type':'application/x-www-form-urlencoded',},body:form});assert.equal(r.status,303);await r.arrayBuffer();assert.equal((await read(2)).data.state,'closed');
  });
  await t.test('native stale and invalid submissions retain drafts and never overwrite data',async()=>{
   for(const [title,tag,expected] of [['My unsaved <& draft','"stale"',412],['',version,400]]){
    const form=new URLSearchParams({...input(title),version:tag});const r=await request('/w/1/tickets/1?native=yes',owner,{method:'POST',headers:{Origin:base,'Content-Type':'application/x-www-form-urlencoded','Ruvane-Csrf-Token':owner.token},body:form});assert.equal(r.status,expected);const html=await r.text();assert.match(html,/value="Description|Description/);assert.match(html,/name="version"/);if(title)assert.match(html,/My unsaved &lt;&amp; draft/);
   }
   assert.deepEqual((await read()).data,first);
  });
  await t.test('pagination stays bounded and invalid identifiers and queries fail closed',async()=>{
   for(let i=3;i<=4;i++){const r=await json('/api/w/1/tickets','POST',input('Ticket '+i));assert.equal(r.status,200);await r.arrayBuffer();}
   for(const [path,status] of [['/w/1/tickets?page=2',200],['/w/1/tickets?page=01',400],['/w/1/tickets?page=0',400],['/w/1/tickets?page=44',400],['/w/1/tickets?page=1&page=2',400],['/w/1/tickets?owner=x',400],['/w/1/tickets/01',400],['/w/1/tickets/5',404]]){const r=await request(path,owner);assert.equal(r.status,status,path);await r.arrayBuffer();}
  });
  await t.test('test profiles see only their persisted workspace memberships',async()=>{
   editor=await session('blair');viewer=await session('casey');outsider=await session('drew');
   for(const [who,expected]of [[owner,[1,2]],[editor,[1]],[viewer,[1]],[outsider,[2]]]){const r=await request('/api/workspaces',who);assert.equal(r.status,200);assert.equal(r.headers.get('cache-control'),'no-store');const body=await r.json();assert.deepEqual(body.spaces.map(s=>s.id),expected);}
   const bad=await request('/session',null,{method:'POST',headers:{Origin:base,'Content-Type':'application/x-www-form-urlencoded'},body:'profile=casey&role=owner'});assert.equal(bad.status,400);await bad.arrayBuffer();
   for(const path of ['/api/w/1/tickets/1','/w/1/tickets','/w/1/tickets/1','/w/1/tickets/1/files']){const r=await request(path,outsider);assert.equal(r.status,404,path);assert.equal(r.headers.get('cache-control'),'no-store');assert.doesNotMatch(await r.text(),/Tab A|Tab B|First|Description|data-ruvane-component="[126]"/);}
  });
  await t.test('viewers cannot mutate tickets or administer memberships with guessed claims',async()=>{
   for(const who of [viewer,editor]){const r=await request('/api/w/1/members',who);assert.equal(r.status,403);await r.arrayBuffer();}
   const r=await json('/api/w/1/tickets/1','PATCH',input('Denied'),version,viewer);assert.equal(r.status,403);await r.arrayBuffer();
   const create=await json('/api/w/1/tickets','POST',input('Denied'),null,viewer);assert.equal(create.status,403);await create.arrayBuffer();
   const native=await request('/w/1/tickets/1?native=yes',viewer);assert.equal(native.status,200);const html=await native.text();assert.doesNotMatch(html,/Save ticket|name="version"/);assert.match(html,/Ticket details/);assert.deepEqual((await read()).data,first);
   const forged=await json('/api/w/1/tickets','POST',{...input(),workspace:2,role:'owner'},null,editor);assert.equal(forged.status,400);await forged.arrayBuffer();
  });
  async function members(){const r=await request('/api/w/1/members',owner);assert.equal(r.status,200);return {data:await r.json(),tag:r.headers.get('etag')};}
  async function setMembers(roles,tag){const r=await json('/api/w/1/members','PATCH',roles,tag);return r;}
  await t.test('workspace creation and membership changes require current server authority',async()=>{
   for(const who of [viewer,editor]){const r=await json('/api/workspaces','POST',{name:'Forbidden'},null,who);assert.equal(r.status,403);await r.arrayBuffer();}
   const invalid=await json('/api/workspaces?owner=alex','POST',{name:'Must not exist'});assert.equal(invalid.status,400);await invalid.arrayBuffer();
   const current=await members(),empty={alex:'none',blair:'none',casey:'none',drew:'none'};
   let r=await setMembers(empty,current.tag);assert.equal(r.status,400);await r.arrayBuffer();assert.deepEqual((await members()).data,current.data);
   r=await setMembers({...current.data.members,casey:'editor'},current.tag);assert.equal(r.status,200);await r.arrayBuffer();
   r=await setMembers({...current.data.members,drew:'editor'},current.tag);assert.equal(r.status,412);await r.arrayBuffer();
   const next=await members();r=await setMembers(current.data.members,next.tag);assert.equal(r.status,200);await r.arrayBuffer();
   const create=await json('/api/workspaces','POST',{name:'Release planning'});assert.equal(create.status,200);assert.equal((await create.json()).spaces.at(-1).name,'Release planning');
  });
  await t.test('native membership forms save versions and retain invalid or conflicting drafts',async()=>{
   const initial=await members();
   async function nativeRoles(roles,version,who=owner){return request('/w/1/members',who,{method:'POST',headers:{Origin:base,'Content-Type':'application/x-www-form-urlencoded'},body:new URLSearchParams({...roles,version,_ruvane_csrf:who.token})});}
   const roles={...initial.data.members,casey:'editor'};let r=await nativeRoles(roles,initial.tag);assert.equal(r.status,303);assert.equal(r.headers.get('location'),'/w/1/members?native=yes');await r.arrayBuffer();
   r=await nativeRoles({...roles,drew:'viewer'},initial.tag);assert.equal(r.status,412);const conflict=await r.text();assert.match(conflict,/value="viewer"/);assert.match(conflict,/retains this draft/);
   const current=await members();r=await nativeRoles({...roles,casey:'ADMIN'},current.tag);assert.equal(r.status,400);assert.match(await r.text(),/value="ADMIN"/);assert.deepEqual((await members()).data,current.data);
   r=await nativeRoles(current.data.members,'bad\nversion');assert.equal(r.status,400);await r.arrayBuffer();
   r=await nativeRoles(current.data.members,current.tag,viewer);assert.equal(r.status,403);await r.arrayBuffer();
   r=await setMembers(initial.data.members,current.tag);assert.equal(r.status,200);await r.arrayBuffer();
  });
  await t.test('tickets remain bound to their workspace even for an owner of both spaces',async()=>{
   const r=await json('/api/w/2/tickets','POST',input('Sandbox only'));assert.equal(r.status,200);sandbox=await r.json();assert.equal(sandbox.workspace,2);
   for(const [path,who]of [[`/api/w/2/tickets/${sandbox.id}`,editor],[`/api/w/2/tickets/2`,owner]]){const r=await request(path,who);assert.equal(r.status,404);await r.arrayBuffer();}
   assert.equal(sandbox.id,1);assert.deepEqual((await read()).data,first);const allowed=await request(`/api/w/2/tickets/${sandbox.id}`,outsider);assert.equal(allowed.status,200);assert.equal((await allowed.json()).title,'Sandbox only');
   const list=await request('/w/1/tickets',owner);assert.doesNotMatch(await list.text(),/Sandbox only/);
  });
  const fileBase='/api/w/1/tickets/1';
  async function prepare(who=editor,prefix=fileBase){const r=await request(prefix+'/upload-operation',who);assert.equal(r.status,200);return (await r.json()).operation;}
  async function upload(operation,who=editor,name='Rust-<&🦀.bin',bytes=new Uint8Array([0,255,13,10,128]),extra={}){
   const form=new FormData();form.set('operation',operation);form.set('attachment',new Blob([bytes]),name);for(const[k,v]of Object.entries(extra))form.set(k,v);
   return request('/w/1/tickets/1/files',who,{method:'POST',headers:{Origin:base,Accept:'application/json','Ruvane-Csrf-Token':who.token},body:form});
  }
  async function files(who=editor){const r=await request(fileBase+'/files',who);assert.equal(r.status,200);assert.equal(r.headers.get('cache-control'),'no-store');return r.json();}
  await t.test('one upload identity commits one private ticket attachment under concurrent replay',async()=>{
   uploadOperation=await prepare();const responses=await Promise.all([upload(uploadOperation),upload(uploadOperation)]);for(const r of responses)assert.equal(r.status,200);const values=await Promise.all(responses.map(r=>r.json()));assert.deepEqual(values[0],values[1]);uploaded=values[0];assert.deepEqual(Object.keys(uploaded).sort(),['bytes','id','name']);assert.equal((await files()).files.length,1);
   const changed=await upload(uploadOperation,editor,'Changed.bin');assert.equal(changed.status,409);await changed.arrayBuffer();
   const rejected=await upload(await prepare(),editor,'Unknown-owner.bin',new Uint8Array([1]),{owner:'forged',workspace:'2'});assert.equal(rejected.status,400);await rejected.arrayBuffer();assert.equal((await files()).files.length,1);
  });
  await t.test('viewers can download but files cannot cross a workspace or ticket boundary',async()=>{
   const allowed=await request(fileBase+'/files/'+uploaded.id,viewer);assert.equal(allowed.status,200);assert.equal(allowed.headers.get('cache-control'),'no-store');assert.equal(allowed.headers.get('content-type'),'application/octet-stream');assert.match(allowed.headers.get('content-disposition'),/^attachment;/);assert.deepEqual(new Uint8Array(await allowed.arrayBuffer()),new Uint8Array([0,255,13,10,128]));
   const list=await files(viewer);assert.equal(list.role,'viewer');assert.equal(list.files[0].id,uploaded.id);assert.equal(Object.hasOwn(list,'owner'),false);
   for(const[path,who]of [[fileBase+'/files/'+uploaded.id,outsider],[`/api/w/1/tickets/2/files/${uploaded.id}`,owner],[`/api/w/2/tickets/${sandbox.id}/files/${uploaded.id}`,owner]]){const r=await request(path,who);assert.equal(r.status,404);await r.arrayBuffer();}
   for(const path of [fileBase+'/upload-operation',fileBase+'/operations/'+uploadOperation]){const r=await request(path,viewer);assert.equal(r.status,403);await r.arrayBuffer();}
   const removed=await request(fileBase+'/files/'+uploaded.id,viewer,{method:'DELETE',headers:{Origin:base,'Ruvane-Csrf-Token':viewer.token}});assert.equal(removed.status,403);await removed.arrayBuffer();assert.equal((await files()).files.length,1);
  });
  await t.test('attachment SSR and actual Wasm use a bounded role-aware private seed',async()=>{
   for(const[who,editable]of [[editor,true],[viewer,false]]){const r=await request('/w/1/tickets/1/files',who);assert.equal(r.status,200);assert.equal(r.headers.get('cache-control'),'no-store');const html=await r.text();assert.equal(html.includes('Store file'),editable);assert.match(html,/Rust-&lt;&amp;🦀/);assert.doesNotMatch(html,/desk-w1-t1|store\.bin/);}
   const r=await request('/w/1/tickets/1/files',viewer,{headers:{Accept:NAVIGATION_MEDIA_TYPE,'Ruvane-Application-Revision':fixture.revision}});assert.equal(r.status,200);const bytes=new Uint8Array(await r.arrayBuffer());assert.equal(decodePage(bytes).component,6);assert.equal(e.ruvane_page_begin(bytes.length),1);for(let n=0;n<bytes.length;n+=4){const chunk=bytes.slice(n,n+4);assert.equal(e.ruvane_page_chunk(chunk.reduce((s,b,i)=>s+b*2**(i*8),0),chunk.length),1);}const id=e.ruvane_page_mount(1);assert.ok(id);const commands=decodeCommands(new Uint8Array(e.memory.buffer,e.ruvane_commands_ptr(),e.ruvane_commands_len()).slice());assert.ok(commands.some(c=>c.text?.includes('Rust-<&🦀')));assert.ok(!commands.some(c=>c.text==='Store file'));e.ruvane_commands_clear();assert.equal(e.ruvane_dispose(id),1);e.ruvane_commands_clear();
  });
  await t.test('file quotas refuse new payloads while exact committed replays remain valid',async()=>{
   for(let n=0;n<2;n++){const r=await upload(await prepare(),editor,'Quota-'+n+'.bin');assert.equal(r.status,200);await r.arrayBuffer();}
   const full=await upload(await prepare(),editor,'Too many.bin');assert.equal(full.status,409);await full.arrayBuffer();assert.equal((await files()).files.length,3);
   const replay=await upload(uploadOperation);assert.equal(replay.status,200);assert.deepEqual(await replay.json(),uploaded);
   const removed=await request(fileBase+'/files/'+uploaded.id,editor,{method:'DELETE',headers:{Origin:base,'Ruvane-Csrf-Token':editor.token}});assert.equal(removed.status,200);await removed.arrayBuffer();
   const status=await request(fileBase+'/operations/'+uploadOperation,editor);assert.equal((await status.json()).state,'removed');const retired=await upload(uploadOperation);assert.equal(retired.status,409);await retired.arrayBuffer();
  });
  await t.test('script-free file forms retain upload identities and use POST/303 confirmation',async()=>{
   const page=await request('/w/1/tickets/1/files?native=yes&prepare=yes',editor);assert.equal(page.status,200);const html=await page.text();assert.doesNotMatch(html,/<script/);const operation=/name="operation" value="([0-9a-f]{80})"/.exec(html)[1];
   const form=new FormData();form.set('_ruvane_csrf',editor.token);form.set('operation',operation);form.set('attachment',new Blob([new Uint8Array([9,8,7])]),'Native attachment.bin');
   const r=await request('/w/1/tickets/1/files',editor,{method:'POST',headers:{Origin:base},body:form});assert.equal(r.status,303);assert.match(r.headers.get('location'),new RegExp('operation='+operation));await r.arrayBuffer();
   const confirm=await request(`/w/1/tickets/1/files?native=yes&operation=${operation}`,editor);assert.match(await confirm.text(),/File stored\. This attempt is confirmed/);
  });
  await t.test('membership revocation affects existing sessions before upload preparation, replay or removal',async()=>{
   const pending=await prepare();const current=await members();const r=await setMembers({...current.data.members,blair:'viewer'},current.tag);assert.equal(r.status,200);await r.arrayBuffer();
   const denied=await upload(pending);assert.equal(denied.status,403);await denied.arrayBuffer();
   const replay=await upload(uploadOperation);assert.equal(replay.status,403);await replay.arrayBuffer();
   const prep=await request(fileBase+'/upload-operation',editor);assert.equal(prep.status,403);await prep.arrayBuffer();const list=await files(editor);assert.equal(list.role,'viewer');assert.equal(list.files.length,3);
   const patch=await json('/api/w/1/tickets/1','PATCH',input('Revoked edit'),version,editor);assert.equal(patch.status,403);await patch.arrayBuffer();assert.deepEqual((await read()).data,first);
  });
  await t.test('clean restart preserves tickets but expires sessions and old write validators',async()=>{await stop();assert.equal((await readdir(storage)).includes('LOCK'),false);await start();let r=await request('/w/1/tickets',owner);assert.equal(r.status,403);await r.arrayBuffer();owner=await session();assert.deepEqual((await read()).data,first);editor=await session('blair');assert.equal((await files(editor)).role,'viewer');viewer=await session('casey');assert.equal((await files(viewer)).files.length,3);r=await json('/api/w/1/tickets/1','PATCH',input('Before restart'),version);assert.equal(r.status,412);await r.arrayBuffer();const page=await request('/w/1/tickets?page=2',owner);assert.match(await page.text(),/Ticket 4/);});
  await t.test('explicit offline recovery preserves a valid interrupted snapshot',async()=>{await stop();await writeFile(join(storage,'LOCK'),'RDL1\n',{mode:0o600});execFileSync(join(fixture.bundle,'ruvane-app'),['--recover',storage],{cwd:fixture.bundle,encoding:'utf8'});assert.equal((await readdir(storage)).includes('LOCK'),false);await start();owner=await session();assert.deepEqual((await read()).data,first);});
  await t.test('an alpha.61 catalog migrates without changing existing ticket ids or revisions on read',async()=>{
   await stop();await mkdir(legacyStorage,{mode:0o700});
   const title=Buffer.from('Legacy ticket'),description=Buffer.from('Retained from alpha.61');
   const header=Buffer.alloc(25);header.write('RDS1');header.writeUInt32LE(1,4);header.writeUInt32LE(1,8);header.writeBigUInt64LE(17n,12);header[20]=0;header.writeUInt16LE(title.length,21);header.writeUInt16LE(description.length,23);
   const payload=Buffer.concat([header,title,description]);let checksum=0xffffffff;for(const byte of payload){checksum^=byte;for(let n=0;n<8;n++)checksum=(checksum>>>1)^((checksum&1)?0xedb88320:0);}const suffix=Buffer.alloc(4);suffix.writeUInt32LE((~checksum)>>>0);const original=Buffer.concat([payload,suffix]);
   await writeFile(join(legacyStorage,'identity'),'ruvane-desk\t1\tdesk\n',{mode:0o600});await writeFile(join(legacyStorage,'store.bin'),original,{mode:0o600});
   activeStorage=legacyStorage;await start();owner=await session();const loaded=await read();assert.deepEqual(loaded.data,{id:1,workspace:1,revision:17,title:'Legacy ticket',description:'Retained from alpha.61',state:'open'});assert.deepEqual(await readFile(join(legacyStorage,'store.bin')),original);
   const updated=await json('/api/w/1/tickets/1','PATCH',input('Migrated legacy'),loaded.tag);assert.equal(updated.status,200);assert.equal((await updated.json()).revision,18);assert.equal((await readFile(join(legacyStorage,'store.bin'))).subarray(0,4).toString(),'RDS2');
   await stop();await start();owner=await session();assert.equal((await read()).data.title,'Migrated legacy');
  });
  await t.test('closing a local session revokes its cookie and CSRF capability',async()=>{
   const form=new URLSearchParams({_ruvane_csrf:owner.token});
   const closed=await request('/session/close',owner,{method:'POST',headers:{Origin:base,'Content-Type':'application/x-www-form-urlencoded'},body:form});
   assert.equal(closed.status,303);assert.equal(closed.headers.get('location'),'/');assert.match(closed.headers.getSetCookie()[0],/Max-Age=0/);await closed.arrayBuffer();
   const denied=await request('/api/w/1/tickets/1',owner);assert.equal(denied.status,403);await denied.arrayBuffer();
   const csrf=await request('/csrf',owner);assert.equal(csrf.headers.get('Ruvane-Csrf-Token'),null);await csrf.arrayBuffer();
  });
 } finally {
  await stop();if(keep&&fixture)await writeFile(join(directory,'fixture.json'),JSON.stringify({...fixture,storage,legacyStorage},null,2));if(hidden&&!keep)await rename(hidden,join(fixture.project,'src'));if(!keep)await rm(directory,{recursive:true,force:true});
 }
});

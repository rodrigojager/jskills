import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {mkdir,readFile,writeFile,copyFile} from 'node:fs/promises';
import {randomUUID} from 'node:crypto';
import {provision,runtimeManager,digest} from '../src/runtime.mjs';
import {readArguments,readProjection,runs} from '../src/reader.mjs';
import {createService} from '../src/worker.mjs';
import {execute} from '../src/process.mjs';

const base=process.env.JTASKS_PLUGIN_TEST_DIR || fileURLToPath(new URL('../../../_work/00-tmp/orca-tests/',import.meta.url));
async function fixture(){const root=path.join(base,randomUUID());await mkdir(root,{recursive:true});return root;}
const version='0.1.0';
const fakeRun=async(file,args)=>({code:0,stdout:args[0]==='--version'?`jtasks ${version}`:'progress errors --revision --task --data-dir',stderr:''});
const nativeName=process.platform==='win32'?'jtasks.exe':'jtasks';
async function bundle(){const root=await fixture();await writeFile(path.join(root,nativeName),'test native binary');await writeFile(path.join(root,'runtime.json'),JSON.stringify({cliVersion:version,artifacts:{[`${process.platform}-${process.arch}`]:{file:nativeName,sha256:digest(await readFile(path.join(root,nativeName)))}}}));return root;}
const data=(run='one',extra={})=>({contract_version:1,read_only:true,identity:{checkout:'checkout',run},...extra});
const tick=()=>new Promise(resolve=>setImmediate(resolve));

test('worker caches automatic update checks, manual checks revalidate and settings revoke offers',async()=>{
  const h=host();let checks=0,changed,finish;
  let config={updateRepository:'example/jtasks',checkUpdates:true};
  h.orca.settings.get=async()=>config;h.orca.settings.onDidChange=fn=>{changed=fn;return {dispose(){}};};
  const service=createService(h.orca,{runtime:{ensure:async()=>({executable:'native',version})},updates:{
    async check(repository){checks++;if(finish===false)await new Promise(resolve=>{finish=resolve;});return {state:'checked',repository,current:version,latest:'0.1.1',available:true,checkedAt:Date.now(),token:'offer'};},
    apply:async token=>({token}),invalidate(){}
  }});
  const call=h.handlers['updates.check'];
  assert.equal((await call({automatic:true})).token,'offer');
  const cached=await call({automatic:true});assert.equal(cached.cached,true);assert.equal(cached.token,undefined);assert.equal(checks,1);
  assert.equal((await call({})).token,'offer');assert.equal(checks,2);
  config={...config,checkUpdates:false};assert.equal((await call({automatic:true})).state,'disabled');
  finish=false;const pending=call({});while(typeof finish!=='function')await tick();
  config={...config,updateRepository:'other/repo'};changed();finish();await assert.rejects(pending,/settings changed/);
  service.dispose();
});

test('offline provision verifies bytes, coalesces and never replaces a changed managed binary',async()=>{
  const root=await bundle(),destination=path.join(root,'managed'),manager=runtimeManager({root,destination,envPath:'',run:fakeRun});
  const [a,b]=await Promise.all([manager.ensure(),manager.ensure()]);assert.equal(a.executable,b.executable);assert.equal(a.source,'managed');
  assert.equal((await provision({root,destination,envPath:'',run:fakeRun})).executable,a.executable);
  await writeFile(a.executable,'changed');
  await assert.rejects(provision({root,destination,envPath:'',run:fakeRun}),/has changed/);
  assert.equal(await readFile(a.executable,'utf8'),'changed');
});
test('provision rejects corrupted bundles and incompatible overrides, retries a failed preparation',async()=>{
  const root=await bundle();await writeFile(path.join(root,nativeName),'corrupted');
  const manager=runtimeManager({root,destination:path.join(root,'managed'),envPath:'',run:fakeRun});
  await assert.rejects(manager.ensure(),/integrity/);
  await writeFile(path.join(root,nativeName),'test native binary');assert.ok((await manager.ensure()).executable);
  await assert.rejects(provision({root,override:path.join(root,nativeName),run:async()=>({code:0,stdout:'jtasks 0.9.4',stderr:''})}),/not a compatible/);
});
test('compatible PATH installation is reused without copying a runtime',async()=>{
  const root=await bundle();const result=await provision({root,envPath:root,run:fakeRun});
  assert.equal(result.source,'existing');assert.equal(result.executable,await import('node:fs/promises').then(fs=>fs.realpath(path.join(root,process.platform==='win32'?'jtasks.exe':'jtasks'))));
});
test('read bridge accepts unfinished status and rejects command injection/contract drift',async()=>{
  const context={root:'C:\\path with spaces',run:'one',dataDir:'C:\\data & value'};
  assert.deepEqual(readArguments(context,'task',{task:'task & literal',limit:25}),['--lang','en','--root',context.root,'--data-dir',context.dataDir,'read','task','--run','one','--task','task & literal','--limit','25']);
  for(const [view,fields] of [['run',{}],['tasks',{root:'elsewhere'}],['tasks',{limit:101}],['tasks',{after:null}],['logs',{stream:'other'}]])assert.throws(()=>readArguments(context,view,fields));
  assert.equal((await readProjection('exe',context,'status',{}, {run:async()=>({code:3,stdout:JSON.stringify(data()),stderr:''})})).identity.run,'one');
  await assert.rejects(readProjection('exe',context,'tasks',{}, {run:async()=>({code:3,stdout:JSON.stringify(data()),stderr:'unfinished'})}),/unfinished/);
  await assert.rejects(readProjection('exe',context,'status',{}, {run:async()=>({code:0,stdout:JSON.stringify(data('wrong')),stderr:''})}),/does not match/);
  await assert.rejects(readProjection('exe',context,'status',{}, {run:async()=>({code:1,stdout:'',stderr:'no runtime database; read does not create state'})}),e=>e.code==='no_data');
});
test('run listing paginates and detects overlapping pages',async()=>{
  const runner=async(_file,args)=>{const offset=Number(args[args.indexOf('--after')+1]);return {code:0,stdout:JSON.stringify(data(null,{runs:[{run:offset?'two':'one'}],page:{has_more:!offset,next_after:offset+1}})),stderr:''};};
  assert.equal((await runs('exe',{root:'root'},{run:runner})).runs.length,2);
  await assert.rejects(runs('exe',{root:'root'},{run:async(...args)=>{const result=await runner(...args);return {...result,stdout:result.stdout.replace('two','one')};}}),/overlapping/);
});
test('bridge size bound measures UTF-8 bytes, including non-Latin task content',async()=>{
  await assert.rejects(readProjection('exe',{root:'root',run:'one'},'task',{}, {run:async()=>({code:0,stderr:'',stdout:JSON.stringify(data('one',{text:'界'.repeat(80000)}))})}),e=>e.code==='too_large');
});

function host() {
  let focused='a';const handlers={},events={},hooks={},saved=new Map(),updates=[];
  const workspaces=[{id:'a',name:'Alpha',path:'alpha',hostId:'local'},{id:'b',name:'Beta',path:'beta',hostId:null},{id:'r',name:'Remote',path:'/remote',hostId:'ssh:remote'}];
  const disposable={dispose(){}};
  const orca={
    panels:{handle(name,fn){handlers[name]=fn;return disposable;},post(){}},
    events:{on(name,fn){events[name]=fn;return disposable;}},hooks:{on(name,fn){hooks[name]=fn;return disposable;}},
    commands:{register(){return disposable;}},storage:{get:async key=>saved.get(key),set:async(key,value)=>saved.set(key,value)},
    settings:{get:async()=>({agentPath:true}),onDidChange:()=>disposable},log(){},
    host:{async call(name,input){if(name==='workspace.readContext')return focused?{workspaceId:focused}:null;if(name==='workspace.list')return {workspaces};if(name==='statusBar.update'){updates.push(input);return {}; }throw new Error(name);}}
  };
  return {orca,handlers,hooks,updates,select(id){focused=id;events['workspace.changed']?.({context:id?{workspaceId:id}:null});}};
}
test('worker rejects stale project responses, isolates run selections and refuses remote roots',async()=>{
  const h=host();let finish;
  const service=createService(h.orca,{runtime:{ensure:async()=>({executable:'native',version})},canonical:async p=>p,listRuns:async()=>({checkout:'checkout',runs:[{run:'one'},{run:'two'}]}),read:()=>new Promise(resolve=>{finish=resolve;})});
  await assert.rejects(service.context({root:'untrusted'}),e=>e.code==='invalid_params');
  assert.equal((await service.context()).state,'select_run');
  const a=await service.context({run:'two'});assert.equal(a.run,'two');
  const pending=service.projection({key:a.key,view:'status'});await tick();h.select('b');finish(data('two'));await assert.rejects(pending,e=>e.code==='conflict');
  assert.equal((await service.context()).state,'select_run');
  const b=await service.context({run:'one'});assert.notEqual(a.key,b.key);
  await assert.rejects(service.projection({key:a.key,view:'status'}),e=>e.code==='conflict');
  h.select('a');assert.equal((await service.context()).run,'two');
  h.select('r');assert.equal((await service.context()).state,'unsupported_remote');
  h.select(null);assert.equal((await service.context()).state,'no_context');service.dispose();
});
test('agent launch hook only adds the CLI path to a new local launch',async()=>{
  const h=host(),executable=path.resolve('managed','jtasks.exe');
  const service=createService(h.orca,{runtime:{ensure:async()=>({executable,version})},canonical:async p=>p});await tick();
  const event={workspaceId:'a',env:{Path:'original',CUSTOM:'preserve'},command:'agent',agent:'codex',workspaceKind:'folder'};
  const updated=await h.hooks['agent.launch.before'](event,value=>value);
  assert.equal(updated.env.Path,path.dirname(executable)+path.delimiter+'original');assert.equal(event.env.Path,'original');assert.equal(updated.env.CUSTOM,'preserve');
  const remote={...event,workspaceId:'r'};assert.equal(await h.hooks['agent.launch.before'](remote,value=>value),remote);service.dispose();
});
test('newer context resolution wins over a slow previous run selection',async()=>{
  const h=host();let finish,slow=false;
  const service=createService(h.orca,{runtime:{ensure:async()=>({executable:'native',version})},canonical:async p=>p,listRuns:async()=>{if(slow)await new Promise(resolve=>{finish=resolve;});return {checkout:'checkout',runs:[{run:'one'},{run:'two'}]};}});
  slow=true;const old=service.context({run:'one'});await tick();slow=false;const current=await service.context({run:'two'});finish();await assert.rejects(old,e=>e.code==='conflict');assert.equal((await service.context()).key,current.key);service.dispose();
});
test('oversized collection pages shrink without changing project, run or revision',async()=>{
  const h=host(),limits=[];
  const service=createService(h.orca,{runtime:{ensure:async()=>({executable:'native',version})},canonical:async p=>p,listRuns:async()=>({checkout:'checkout',runs:[{run:'one'}]}),read:async(_exe,context,view,fields)=>{
    assert.equal(context.run,'one');assert.equal(view,'tasks');assert.equal(fields.revision,7);limits.push(fields.limit);
    if(fields.limit>25)throw Object.assign(new Error('too big'),{code:'too_large'});
    return data('one',{tasks:[],page:{limit:fields.limit}});
  }});
  const context=await service.context();assert.equal((await service.projection({key:context.key,view:'tasks',fields:{limit:100,revision:7}})).page.limit,25);assert.deepEqual(limits,[100,50,25]);service.dispose();
});
test('slow persistent selections cannot overwrite or render a newer run',async()=>{
  const h=host();let finish,first=true;const store=h.orca.storage.set;
  h.orca.storage.set=async(key,value)=>{if(first){first=false;await new Promise(resolve=>{finish=resolve;});}await store(key,value);};
  const service=createService(h.orca,{runtime:{ensure:async()=>({executable:'native',version})},canonical:async p=>p,listRuns:async()=>({checkout:'checkout',runs:[{run:'one'},{run:'two'}]})});
  const old=assert.rejects(service.context({run:'one'}),e=>e.code==='conflict');await tick();
  const latest=service.context({run:'two'});await tick();finish();await old;
  const selected=await latest;assert.equal(selected.run,'two');assert.equal(await h.orca.storage.get('run:a:alpha'),'two');assert.equal((await service.context()).key,selected.key);service.dispose();
});
test('actual bundled native CLI provisions offline and preserves missing-database semantics',{skip:!process.env.JTASKS_PLUGIN_TEST_EXE},async()=>{
  const executable=process.env.JTASKS_PLUGIN_TEST_EXE,root=await fixture(),native=path.join(root,process.platform==='win32'?'jtasks.exe':'jtasks');
  await copyFile(executable,native);const bytes=await readFile(native);
  await writeFile(path.join(root,'runtime.json'),JSON.stringify({cliVersion:version,artifacts:{[`${process.platform}-${process.arch}`]:{file:path.basename(native),sha256:digest(bytes)}}}));
  const runtime=await provision({root,destination:path.join(root,'managed'),envPath:''});assert.equal(runtime.source,'managed');
  const workspace=path.join(root,'checkout');await mkdir(workspace);assert.equal((await execute('git',['init','--quiet',workspace])).code,0);
  await assert.rejects(readProjection(runtime.executable,{root:workspace,dataDir:path.join(root,'empty'),run:'one'},'status',{}),e=>e.code==='no_data');
});

test('run controls use the real CLI: stop ends a live controller, continue opens or focuses an Orca terminal, a live controller refuses',{skip:!process.env.JTASKS_PLUGIN_TEST_EXE},async()=>{
  const {spawn}=await import('node:child_process');
  const executable=process.env.JTASKS_PLUGIN_TEST_EXE,root=await fixture();
  assert.equal((await execute('git',['init','--quiet',root])).code,0);
  const plan=path.join(root,'plan.json');
  await writeFile(plan,JSON.stringify({schema_version:1,completion:'judgment',operation_placement:'none',tasks:[{id:'A',title:'Fixture A',depends_on:[],executor:{kind:'fixture',outcome:'completed',summary:'done',delay_ms:8000}}]}));
  const dataDir=path.join(root,'runtime'),cli=(...args)=>execute(executable,['--lang','en','--root',root,'--data-dir',dataDir,...args]);
  const imported=await cli('import',plan,'--run','demo');assert.equal(imported.code,0,imported.stderr);
  const h=host();h.orca.host.call=(base=>async(name,input)=>name==='workspace.list'?{workspaces:[{id:'a',name:'Alpha',path:root,hostId:'local'}]}:base(name,input))(h.orca.host.call);
  const created=[],focused=[];let open=[];
  const terminals={list:async()=>open,focus:async handle=>{focused.push(handle);},create:async spec=>{created.push(spec);open=[{handle:'term_1',title:spec.title}];}};
  const service=createService(h.orca,{runtime:{ensure:async()=>({executable,version})},terminals,dataDir});
  const title='jtasks · demo';
  const controller=spawn(executable,['--root',root,'--data-dir',dataDir,'run','--run','demo'],{stdio:'ignore',windowsHide:true});
  const exited=new Promise(resolve=>controller.once('exit',resolve));
  try{
    for(let i=0;;i++){const out=JSON.parse((await cli('read','runs')).stdout);if(out.runs?.[0]?.controller==='live')break;assert.ok(i<300,'controller never became live');await new Promise(r=>setTimeout(r,50));}
    const refused=await service.continueRun({run:'demo'});
    assert.equal(refused.state,'refused');assert.match(refused.message,/another foreground controller/);assert.equal(created.length,0);
    assert.equal((await service.stopRun({run:'demo'})).state,'stopping');
    await exited;
    assert.equal((await service.stopRun({run:'demo'})).state,'already_stopped');
    const opened=await service.continueRun({run:'demo'});
    assert.equal(opened.state,'opened');assert.equal(created.length,1);assert.equal(created[0].title,title);assert.equal(created[0].root,await import('node:fs/promises').then(fs=>fs.realpath(root)));
    assert.equal(created[0].command.replaceAll('"',''),`${executable} --root ${created[0].root} run --run demo`);
    const again=await service.continueRun({run:'demo'});
    assert.equal(again.state,'focused');assert.deepEqual(focused,['term_1']);assert.equal(created.length,1);
    await assert.rejects(service.continueRun({run:'missing'}),e=>e.code==='invalid_params');
    await assert.rejects(service.stopRun({run:'demo',extra:1}),e=>e.code==='invalid_params');
    assert.equal((await service.agentsList({})).run,'demo');
    await assert.rejects(service.agentsSet({alias:'ghost',effort:'bogus'}),/./);
  }finally{controller.kill();service.dispose();}
});
test('orcaTerminals asks Orca for a titled terminal in the workspace and focuses by handle',async()=>{
  const {orcaTerminals}=await import('../src/worker.mjs'),calls=[];
  const run=async args=>{calls.push(args);return {state:'applied',output:JSON.stringify({result:{terminals:[{handle:'h',title:'t'}]}})};};
  const t=orcaTerminals(run);
  assert.deepEqual(await t.list('/w'),[{handle:'h',title:'t'}]);
  await t.create({root:'/w',title:'jtasks · demo',command:'jtasks run'});await t.focus('h');
  assert.deepEqual(calls[1],['terminal','create','--worktree','path:/w','--title','jtasks · demo','--command','jtasks run','--focus']);
  assert.deepEqual(calls[2],['terminal','switch','--terminal','h']);
});

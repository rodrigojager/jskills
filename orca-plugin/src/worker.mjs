import path from 'node:path';
import {randomUUID} from 'node:crypto';
import {access, readFile, realpath} from 'node:fs/promises';
import {runtimeManager, packageRoot} from './runtime.mjs';
import {readProjection, runs} from './reader.mjs';
import {execute} from './process.mjs';
import {updateManager} from './updates.mjs';

const conflict = () => Object.assign(new Error('The active project changed. Refresh the panel.'),{code:'conflict'});
const local = workspace => workspace && (!workspace.hostId || workspace.hostId === 'local');

export function createService(orca, {runtime = runtimeManager(), read = readProjection, listRuns = runs, canonical = realpath, open = openPanel, updates = updateManager({host:orcaCommand})} = {}) {
  let epoch=0, resolution=0, observedWorkspace, active=null, controller=new AbortController(), ready=null, settings={}, pending=null;
  let selectionWrites=Promise.resolve();
  const disposables=[];
  const call=(name,input)=>orca.host.call(name,input);
  const status=(text,tooltip)=>call('statusBar.update',{id:'progress',text,tooltip,icon:'check',visible:true}).catch(()=>{});
  const invalidate=()=>{
    epoch++;resolution++;active=null;pending=null;controller.abort();controller=new AbortController();
    orca.panels.post('context.changed',{});status('jtasks','Open project tasks');
  };
  const ensure=async()=>{
    const manager=runtime,config=await orca.settings.get();
    const result=await manager.ensure({override:config.executable || ''});
    if(manager===runtime){ready=result;settings=config;}return result;
  };
  let updateRevision=0;
  async function checkUpdates(input={}) {
    if(!input || typeof input!=='object' || Array.isArray(input) || Object.keys(input).some(key=>key!=='automatic') || input.automatic!==undefined && typeof input.automatic!=='boolean')throw new Error('Invalid update check');
    const ticket=updateRevision,config=await orca.settings.get();
    if(input.automatic && config.checkUpdates===false)return {state:'disabled',message:'Automatic update checks are disabled.'};
    const repo=config.updateRepository || '';
    const key=`updates:${repo}`;
    const cached=await orca.storage.get(key);
    const version=JSON.parse(await readFile(path.join(packageRoot,'orca-plugin.json'),'utf8')).version;
    if(input.automatic && cached?.checkedAt>Date.now()-86400000 && cached.current===version)return {...cached,token:undefined,cached:true};
    const cli=await ensure();
    if(ticket!==updateRevision)throw new Error('Update settings changed; check again');
    const result=await updates.check(repo,cli);
    if(ticket!==updateRevision || result.repository && result.repository!==repo)throw new Error('Update settings changed; check again');
    if(result.state==='checked')await orca.storage.set(key,{...result,token:undefined});
    return result;
  }
  async function applyUpdate(input) {
    if(!input || Object.keys(input).some(key=>key!=='token') || typeof input.token!=='string')throw new Error('Check for updates before installing');
    return updates.apply(input.token);
  }
  async function focused() {
    const context=await call('workspace.readContext');
    if (!context?.workspaceId) return null;
    const {workspaces}=await call('workspace.list');
    return workspaces.find(w=>w.id===context.workspaceId) || null;
  }
  async function current(snapshot) {
    const context=await call('workspace.readContext');
    if(snapshot.epoch!==epoch || context?.workspaceId!==snapshot.workspaceId) throw conflict();
  }
  async function context(input={}) {
    if(!input || typeof input!=='object' || Array.isArray(input) || Object.keys(input).some(key=>key!=='run') || input.run!==undefined && (typeof input.run!=='string' || !input.run || input.run.length>512))throw Object.assign(new Error('Invalid project context request'),{code:'invalid_params'});
    // Context refreshes are coalesced, except explicit run selections.
    if(pending && !input.run) return pending;
    const job=resolveContext(input);
    if(!input.run) pending=job;
    try{return await job;}finally{if(pending===job)pending=null;}
  }
  async function resolveContext(input) {
    const ticket=epoch, request=++resolution;
    try {
      const workspace=await focused();
      if(ticket!==epoch || request!==resolution)throw conflict();
      if(!workspace){active=null;return {state:'no_context',message:'Open a local project in Orca to see its tasks.'};}
      if(!local(workspace)){active=null;return {state:'unsupported_remote',message:'This package reads local workspaces. Remote and WSL runtimes are not supported.'};}
      const root=await canonical(workspace.path);
      const base={epoch:ticket,workspaceId:workspace.id,root};
      const cli=await ensure();
      const collection=await listRuns(cli.executable,base,{signal:controller.signal});
      if(Buffer.byteLength(JSON.stringify(collection),'utf8')>180000)throw new Error('The run selector exceeds the panel limit. Use the CLI to inspect this project.');
      await current(base);
      if(request!==resolution)throw conflict();
      const storageKey=`run:${workspace.id}:${root}`;
      const saved=input.run || await orca.storage.get(storageKey);
      if(input.run && !collection.runs.some(r=>r.run===input.run))throw new Error('The selected run is not in this project');
      // No arbitrary choice when several runs have never been selected.
      const run=collection.runs.some(r=>r.run===saved) ? saved : collection.runs.length===1 ? collection.runs[0].run : null;
      await current(base);
      if(request!==resolution)throw conflict();
      if(!run){active=null;return {state:collection.runs.length?'select_run':'no_runs',workspace:workspace.name,runs:collection.runs,message:collection.runs.length?'Select a project run.':'No jtasks run has been imported in this project.',cli};}
      if(input.run){
        // Older writes finish first; only the latest selection may render or persist next.
        selectionWrites=selectionWrites.catch(()=>{}).then(()=>request===resolution?orca.storage.set(storageKey,run):undefined);
        await selectionWrites;
      }
      await current(base);
      if(request!==resolution)throw conflict();
      const key=active?.root===root && active?.run===run && active?.workspaceId===workspace.id ? active.key : randomUUID();
      active={...base,run,key,checkout:collection.checkout,executable:cli.executable};
      return {state:'ready',key,run,workspace:workspace.name,checkout:collection.checkout,runs:collection.runs,cli};
    } catch(error) {
      if(ticket!==epoch || request!==resolution || error.code==='conflict')throw conflict();
      active=null;
      return {state:error.code==='no_data'?'no_runs':'error',message:error.message};
    }
  }
  async function projection(input,{signal}={}) {
    const snapshot=active;
    if(!snapshot || input?.key!==snapshot.key)throw conflict();
    await current(snapshot);
    let data,fields=input.fields || {};
    for(;;){
      try{data=await read(snapshot.executable,snapshot,input.view,fields,{signal:signal?AbortSignal.any([signal,controller.signal]):controller.signal});break;}
      catch(error){
        const limit=Number(fields.limit || 25);
        if(error.code!=='too_large' || limit<=1)throw error;
        fields={...fields,limit:Math.max(1,Math.floor(limit/2))};
      }
    }
    await current(snapshot);
    if(active?.key!==snapshot.key || data.identity.checkout!==snapshot.checkout)throw conflict();
    if(input.view==='status') {
      const counts=data.task_counts || {}, total=Object.entries(counts).filter(([key])=>key!=='superseded').reduce((n,[,v])=>n+v,0), done=counts.completed || 0;
      await status(`${total?Math.round(done/total*100):0}% · ${done}/${total}`,`${snapshot.run} · recorded project progress`);
    }
    return data;
  }
  disposables.push(orca.panels.handle('context',context),orca.panels.handle('read',projection),orca.panels.handle('updates.check',checkUpdates),orca.panels.handle('updates.apply',applyUpdate));
  disposables.push(orca.events.on('workspace.changed',payload=>{
    const id=payload?.context?.workspaceId || null;
    if(id===observedWorkspace)return;observedWorkspace=id;invalidate();
  }));
  disposables.push(orca.settings.onDidChange(()=>{updateRevision++;updates.invalidate();ready=null;runtime=runtimeManager();invalidate();ensure().catch(()=>{});}));
  disposables.push(orca.commands.register('jtasks.open',()=>open()));
  disposables.push(orca.hooks.on('agent.launch.before',async(event,next)=>{
    // Never hold up an agent launch for installation, and never inject local paths remotely.
    if(!ready || settings.agentPath===false || !event.workspaceId)return next(event);
    try {
      const {workspaces}=await orca.host.call('workspace.list',undefined,{signal:AbortSignal.timeout(1000)});
      if(!local(workspaces.find(w=>w.id===event.workspaceId)))return next(event);
      const env={...event.env}, key=Object.keys(env).find(k=>k.toUpperCase()==='PATH') || 'PATH';
      env[key]=[path.dirname(ready.executable),env[key] || process.env.PATH || ''].join(path.delimiter);
      return next({...event,env});
    }catch{return next(event);}
  }));
  ensure().catch(error=>orca.log(`CLI preparation: ${error.message}`));
  return {context,projection,invalidate,dispose(){controller.abort();for(const d of disposables)d.dispose();}};
}

async function openPanel() {
  const config=JSON.parse(await readFile(path.join(packageRoot,'runtime.json'),'utf8'));
  await orcaCommand(['plugin','open',config.pluginKey,'--panel','tasks']);
  return 'Opened project tasks';
}

export async function orcaCommand(args) {
  const name=process.platform==='win32'?'orca.exe':'orca';
  const candidates=[path.join(path.dirname(process.execPath),'resources','bin',name),path.join(path.dirname(process.execPath),'bin',name),...((process.env.PATH || '').split(path.delimiter).filter(path.isAbsolute).map(dir=>path.join(dir,name)))];
  for(const file of candidates) {
    try{await access(file);}catch{continue;}
    const result=await execute(file,['--environment','local',...args],{timeout:120000,maxBuffer:1024*1024});
    if(result.code===3)return {state:'approval_required',message:result.stdout || result.stderr};
    if(result.code)throw new Error(result.stderr || result.stdout || 'Orca command failed');
    return {state:'applied',message:'Orca applied the request. The panel may reload.',output:result.stdout};
  }
  throw new Error('Orca CLI not found. Open jtasks using its sidebar icon.');
}

let service;
export default function activate(orca){service=createService(orca);}
export function deactivate(){service?.dispose();}

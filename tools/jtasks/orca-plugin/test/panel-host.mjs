// Headless acceptance stand-in: real plugin worker + native CLI, no running Orca required.
// Invoked by panel.py; all fixtures and managed executables stay in the supplied work directory.
import {createServer} from 'node:http';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {randomUUID,createHash} from 'node:crypto';
import {execute} from '../src/process.mjs';

const [packageDir,workDir]=process.argv.slice(2);
if(!path.isAbsolute(packageDir || '') || !path.isAbsolute(workDir || ''))throw new Error('Expected absolute package and work directories');
const root=path.join(workDir,randomUUID());await mkdir(root,{recursive:true});
const {createService}=await import(pathToFileURL(path.join(packageDir,'src/worker.mjs')));
const {runtimeManager}=await import(pathToFileURL(path.join(packageDir,'src/runtime.mjs')));
const {updateManager}=await import(pathToFileURL(path.join(packageDir,'src/updates.mjs')));
const binary=path.join(packageDir,'runtime',process.platform==='win32'?'jtasks.exe':'jtasks');
async function native(args,root){const result=await execute(binary,['--lang','en','--root',root,...args],{timeout:60000,maxBuffer:8*1024*1024});if(result.code)throw new Error(result.stderr);return result;}
async function project(name,count,extra=false){
  const base=path.join(root,name),checkout=path.join(base,'product');await mkdir(checkout,{recursive:true});await mkdir(path.join(base,'_work'));
  await writeFile(path.join(base,'_work','WORKSPACE.md'),'---\njproject: 1\nproduct_dir: product\njtasks_data: _work/runtime\n---\n');
  if((await execute('git',['init','--quiet',checkout])).code)throw new Error('git init failed');
  const tasks=Array.from({length:count},(_,i)=>({id:`T${String(i+1).padStart(3,'0')}`,title:`${name} task ${i+1}`,depends_on:[],context:{brief:`${name} fixture detail`},executor:{kind:'fixture',outcome:'completed',summary:'Recorded fixture evidence',delay_ms:0}}));
  const plan=path.join(base,'plan.json');await writeFile(plan,JSON.stringify({schema_version:1,completion:'judgment',operation_placement:'none',scheduling:{task_capacity:4,heavy_capacity:1},tasks}));
  await native(['import',plan,'--run',`${name}-primary`],checkout);
  if(name==='Alpha')await native(['run','--run',`${name}-primary`],checkout);
  if(extra){await writeFile(plan,JSON.stringify({schema_version:1,completion:'judgment',operation_placement:'none',scheduling:{task_capacity:4,heavy_capacity:1},tasks:tasks.slice(0,1)}));await native(['import',plan,'--run',`${name}-secondary`],checkout);}
  return checkout;
}
const a=await project('Alpha',205,true),b=await project('Beta',3);
let workspace='a',eventHandler,posts=0,holdRead=false,releaseRead;
const handlers={},storage=new Map(),requests=[],status=[],updateRequests=[],hostUpdates=[];
const disposable={dispose(){}};
const workspaces=[{id:'a',name:'Alpha',path:a,hostId:'local'},{id:'b',name:'Beta',path:b,hostId:'local'},{id:'r',name:'Remote',path:'/remote',hostId:'ssh:test'}];
const host={
  host:{async call(name,input){if(name==='workspace.readContext')return workspace?{workspaceId:workspace}:null;if(name==='workspace.list')return {workspaces};if(name==='statusBar.update'){status.push(input);return {};}throw new Error(name);}},
  panels:{handle(name,fn){handlers[name]=fn;return disposable;},post(){posts++;}},events:{on(_name,fn){eventHandler=fn;return disposable;}},
  hooks:{on(){return disposable;}},commands:{register(){return disposable;}},settings:{get:async()=>({agentPath:true,updateRepository:'example/jtasks'}),onDidChange:()=>disposable},
  storage:{get:async key=>storage.get(key) ?? null,set:async(key,value)=>storage.set(key,value)},log(){}
};
// Release network/extraction and the host mutation are stand-ins. No live update is applied.
const updateName='jtasks-orca-0.1.1-win32-x64.zip',updateBytes=Buffer.from('verified browser fixture');
const updates=updateManager({root:packageDir,destination:path.join(root,'updates'),
  async get(url){
    updateRequests.push(url);
    if(url.endsWith('/latest'))return Buffer.from(JSON.stringify({tag_name:'v0.1.1',assets:[updateName,updateName+'.sha256'].map(name=>({name,browser_download_url:`https://github.com/example/jtasks/releases/download/v0.1.1/${name}`}))}));
    if(url.endsWith('.sha256'))return Buffer.from(createHash('sha256').update(updateBytes).digest('hex')+'  '+updateName);
    return updateBytes;
  },
  async run(){return {code:0,stdout:JSON.stringify({contract_version:1,component:'cli',repository:'example/jtasks',current_version:'0.1.0',latest_version:'0.1.1',available:true}),stderr:''};},
  async extract(_archive,destination){const folder=path.join(destination,'package');await mkdir(folder,{recursive:true});await writeFile(path.join(folder,'orca-plugin.json'),JSON.stringify({id:'jtasks',version:'0.1.1'}));await writeFile(path.join(folder,'runtime.json'),JSON.stringify({pluginKey:'local.jtasks',cliVersion:'0.1.1'}));},
  async host(args){hostUpdates.push(args);return {state:'approval_required',message:'Fixture approval required. Review this package before applying. <script>unsafe()</script>'};}
});
const service=createService(host,{runtime:runtimeManager({root:packageDir,destination:path.join(root,'managed'),envPath:''}),updates});
const mimes={'.html':'text/html','.js':'text/javascript','.css':'text/css'};
const server=createServer(async(req,res)=>{
  const url=new URL(req.url,'http://127.0.0.1');
  try {
    let data;
    if(url.pathname==='/bridge' && req.method==='POST'){
      let body='';for await(const chunk of req)body+=chunk;
      const input=JSON.parse(body);requests.push(input);
      if(holdRead && input.name==='read'){holdRead=false;await new Promise(resolve=>{releaseRead=resolve;});}
      data={result:await handlers[input.name](input.input || {},{signal:new AbortController().signal})};
    }else if(url.pathname==='/switch'){
      workspace=url.searchParams.get('id');eventHandler({context:workspace?{workspaceId:workspace}:null});data={posts};
    }else if(url.pathname==='/hold'){holdRead=true;data={ok:true};}
    else if(url.pathname==='/release'){releaseRead?.();data={ok:true};}
    else if(url.pathname==='/inspection'){data={requests,status,root,updateRequests,hostUpdates};}
    else {
      const file=path.resolve(packageDir,'panel','.'+(url.pathname==='/'?'/index.html':decodeURIComponent(url.pathname)));
      if(!file.startsWith(path.resolve(packageDir,'panel')+path.sep))throw new Error('Invalid asset path');
      res.setHeader('Content-Type',mimes[path.extname(file)] || 'application/octet-stream');res.end(await readFile(file));return;
    }
    res.setHeader('Content-Type','application/json');res.end(JSON.stringify(data));
  }catch(e){res.statusCode=400;res.setHeader('Content-Type','application/json');res.end(JSON.stringify({error:{message:e.message,code:e.code || 'action_failed'}}));}
});
server.listen(0,'127.0.0.1',()=>console.log(JSON.stringify({url:`http://127.0.0.1:${server.address().port}/`,root})));
process.on('SIGTERM',()=>{service.dispose();server.close();process.exit(0);});

import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {mkdir,readFile,writeFile,readdir} from 'node:fs/promises';
import {randomUUID} from 'node:crypto';
import {updateManager,download,verify,repository,newer,unpack} from '../src/updates.mjs';
import {digest} from '../src/runtime.mjs';
import {execute} from '../src/process.mjs';

const base=process.env.JTASKS_PLUGIN_TEST_DIR || fileURLToPath(new URL('../../../_work/00-tmp/orca-tests/',import.meta.url));
const repo='example/jtasks',version='0.1.1',name=`jtasks-orca-${version}-win32-x64.zip`,zip=Buffer.from('verified test package');
const cli={executable:'native'};
async function fixture(){const root=path.join(base,randomUUID());await mkdir(root,{recursive:true});return root;}
const release=(tag='v0.1.1')=>({tag_name:tag,assets:[name,`${name}.sha256`].map(name=>({name,browser_download_url:`https://github.com/${repo}/releases/download/${tag}/${name}`}))});
const runner=async(file,args)=>{
  assert.equal(file,cli.executable);assert.deepEqual(args,['--json','update','--check','--repository',repo]);
  return {code:0,stdout:JSON.stringify({contract_version:1,component:'cli',repository:repo,current_version:'0.1.0',latest_version:version,available:true,updated:false})};
};
async function manager(overrides={}) {
  const root=await fixture(),calls=[],hostCalls=[];
  await writeFile(path.join(root,'orca-plugin.json'),JSON.stringify({id:'jtasks',version:'0.1.0'}));
  await writeFile(path.join(root,'runtime.json'),JSON.stringify({pluginKey:'local.jtasks'}));
  const get=async(url,limit)=>{
    calls.push({url,limit});
    if(url.endsWith('/latest'))return Buffer.from(JSON.stringify(release()));
    if(url.endsWith('.sha256'))return Buffer.from(`${digest(zip)}  ${name}\n`);
    return zip;
  };
  const extract=async(_archive,dest)=>{
    const dir=path.join(dest,'package');await mkdir(dir,{recursive:true});
    await writeFile(path.join(dir,'orca-plugin.json'),JSON.stringify({id:'jtasks',version}));
    await writeFile(path.join(dir,'runtime.json'),JSON.stringify({pluginKey:'local.jtasks',cliVersion:version}));
  };
  const instance=updateManager({root,destination:path.join(root,'staging'),platform:'win32',arch:'x64',get,run:runner,extract,host:async args=>{hostCalls.push(args);return {state:'approval_required',message:'exact host approval command'};},...overrides});
  return {instance,root,calls,hostCalls,get,extract};
}

test('release checks use semantic versions and do not download or install',async()=>{
  assert.equal(newer('0.10.0','0.9.9'),true);assert.equal(newer('1.0.0','1.0.0'),false);
  assert.throws(()=>newer('1.0.1-beta','1.0.0'));assert.throws(()=>repository('owner/../repo'));assert.throws(()=>repository('https://github.com/owner/repo'));
  const {instance,calls,hostCalls}=await manager();
  const [a,b]=await Promise.all([instance.check(repo,cli),instance.check(repo,cli)]);
  assert.equal(a.token,b.token);assert.equal(a.available,true);assert.equal(a.cli.latest_version,version);
  assert.equal(calls.length,1);assert.equal(hostCalls.length,0);
  instance.invalidate();await assert.rejects(instance.apply(a.token),/Check for updates/);
});

test('not configured is offline, failed checks are retryable, and missing release assets fail',async()=>{
  let calls=0;
  const {instance}=await manager({get:async()=>{calls++;throw new Error('offline');}});
  assert.equal((await instance.check('',cli)).state,'not_configured');assert.equal(calls,0);
  await assert.rejects(instance.check(repo,cli),/offline/);await assert.rejects(instance.check(repo,cli),/offline/);assert.equal(calls,2);
  const broken=await manager({get:async()=>Buffer.from(JSON.stringify({...release(),assets:[]}))});
  await assert.rejects(broken.instance.check(repo,cli),/must publish/);
});

test('complete update stages once and uses host approval without automatic consent',async()=>{
  const {instance,root,hostCalls}=await manager();const offer=await instance.check(repo,cli);
  const [a,b]=await Promise.all([instance.apply(offer.token),instance.apply(offer.token)]);
  assert.equal(a.state,'approval_required');assert.equal(a,b);assert.equal(hostCalls.length,1);
  assert.deepEqual(hostCalls[0].slice(0,2),['plugin','install']);assert.equal(hostCalls[0].at(-1),'--json');assert.ok(!hostCalls[0].includes('--approve'));
  assert.ok(hostCalls[0][2].startsWith(path.join(root,'staging')+path.sep));
  assert.equal(JSON.parse(await readFile(path.join(root,'orca-plugin.json'),'utf8')).version,'0.1.0');
});

test('corrupt bytes and mismatched package versions cannot reach host installation',async()=>{
  let host=0;
  const corrupt=await manager({host:async()=>{host++;},get:async url=>url.endsWith('/latest')?Buffer.from(JSON.stringify(release())):url.endsWith('.sha256')?Buffer.from(`${'0'.repeat(64)}  ${name}`):zip});
  await assert.rejects(corrupt.instance.apply((await corrupt.instance.check(repo,cli)).token),/SHA-256/);
  const wrong=await manager({host:async()=>{host++;},extract:async(_archive,dest)=>{
    await mkdir(path.join(dest,'package'),{recursive:true});
    await writeFile(path.join(dest,'package','orca-plugin.json'),JSON.stringify({id:'jtasks',version:'9.0.0'}));
    await writeFile(path.join(dest,'package','runtime.json'),JSON.stringify({pluginKey:'local.jtasks',cliVersion:version}));
  }});
  await assert.rejects(wrong.instance.apply((await wrong.instance.check(repo,cli)).token),/identity\/version/);
  assert.equal(host,0);assert.throws(()=>verify(zip,Buffer.from(`${digest(zip)}  other.zip`),name));
});

test('source invalidation during a download prevents apply, published keys retain their host source',async()=>{
  let releaseDownload,host=0;
  const f=await manager({host:async()=>{host++;},get:async url=>{
    if(url.endsWith('/latest'))return Buffer.from(JSON.stringify(release()));
    if(url.endsWith('.sha256'))return Buffer.from(`${digest(zip)}  ${name}`);
    await new Promise(resolve=>{releaseDownload=resolve;});return zip;
  }});
  const offer=await f.instance.check(repo,cli),applying=f.instance.apply(offer.token);
  while(!releaseDownload)await new Promise(resolve=>setImmediate(resolve));
  f.instance.invalidate();releaseDownload();await assert.rejects(applying,/source changed/);assert.equal(host,0);
  const published=await manager();await writeFile(path.join(published.root,'runtime.json'),JSON.stringify({pluginKey:'example.jtasks'}));
  await published.instance.apply((await published.instance.check(repo,cli)).token);
  assert.deepEqual(published.hostCalls,[['plugin','update','example.jtasks','--json']]);assert.equal(published.calls.length,1);
});

test('network redirects cannot downgrade HTTPS and response limits bound actual bytes',async()=>{
  let calls=0;
  await assert.rejects(download('https://example.com',10,{fetcher:async()=>{calls++;return new Response(null,{status:302,headers:{location:'http://example.com'}});}}),/HTTPS/);assert.equal(calls,1);
  await assert.rejects(download('https://example.com',3,{fetcher:async()=>new Response('1234')}),/size limit/);
  await assert.rejects(download('https://user:password@example.com',10,{fetcher:()=>assert.fail()}),/credentials/);
});

test('real Windows extraction accepts a complete tree and rejects traversal before writing',{skip:process.platform!=='win32'},async()=>{
  const root=await fixture(),script=path.join(root,'fixtures.ps1');
  await writeFile(script,`param([string]$Root)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
foreach ($kind in @('safe','unsafe','duplicate')) {
  $zip=[IO.Compression.ZipFile]::Open((Join-Path $Root ($kind+'.zip')),[IO.Compression.ZipArchiveMode]::Create)
  try {
    $names=if($kind -eq 'safe'){@('package/one.txt','package/sub/two.txt')}elseif($kind -eq 'unsafe'){@('package/one.txt','package/../escaped.txt')}else{@('package/one.txt','package/ONE.txt')}
    foreach($name in $names){$entry=$zip.CreateEntry($name);$writer=[IO.StreamWriter]::new($entry.Open());try{$writer.Write('bytes')}finally{$writer.Dispose()}}
  }finally{$zip.Dispose()}
}`);
  const shell=path.join(process.env.SystemRoot,'System32','WindowsPowerShell','v1.0','powershell.exe');
  const result=await execute(shell,['-NoProfile','-NonInteractive','-File',script,'-Root',root],{timeout:10000});assert.equal(result.code,0,result.stderr);
  await unpack(path.join(root,'safe.zip'),path.join(root,'safe'));
  assert.equal(await readFile(path.join(root,'safe','package','sub','two.txt'),'utf8'),'bytes');
  await assert.rejects(unpack(path.join(root,'unsafe.zip'),path.join(root,'unsafe')),/Unsafe archive path/);
  await assert.rejects(unpack(path.join(root,'duplicate.zip'),path.join(root,'duplicate')),/Duplicate archive path/);
  assert.ok(!(await readdir(root)).includes('unsafe'));assert.ok(!(await readdir(root)).includes('escaped.txt'));
});

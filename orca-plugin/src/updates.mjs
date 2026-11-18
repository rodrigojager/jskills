// Update the complete optional package through Orca's own approval boundary.
import path from 'node:path';
import {randomUUID} from 'node:crypto';
import {mkdir,readFile,writeFile} from 'node:fs/promises';
import {packageRoot,defaultRuntimeRoot,digest} from './runtime.mjs';
import {execute} from './process.mjs';

export function repository(value) {
  if(typeof value!=='string' || !/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(value) || value.split('/').some(s=>s==='.' || s==='..'))throw new Error('Set the trusted update repository as owner/repo in plugin settings.');
  return value;
}
export function stableVersion(tag) {
  const value=tag?.replace(/^v/,'');
  if(!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(value))throw new Error('The release is not a stable version');
  return value;
}
export function newer(a,b) {
  const left=stableVersion(a).split('.').map(BigInt),right=stableVersion(b).split('.').map(BigInt);
  for(let i=0;i<3;i++){if(left[i]!==right[i])return left[i]>right[i];}return false;
}
export async function download(url,limit,{fetcher=fetch}={}) {
  const signal=AbortSignal.timeout(45000);
  for(let i=0;i<6;i++){
    const parsed=new URL(url);
    if(parsed.protocol!=='https:' || parsed.username || parsed.password)throw new Error('Update downloads require HTTPS without URL credentials');
    const response=await fetcher(url,{signal,redirect:'manual',headers:{'User-Agent':'jtasks-orca'}});
    if([301,302,303,307,308].includes(response.status)){
      const location=response.headers.get('location');await response.body?.cancel();
      if(!location)throw new Error('Update redirect has no location');url=new URL(location,url).href;continue;
    }
    if(!response.ok){await response.body?.cancel();throw new Error(`Update request failed (${response.status}); check connectivity and repository access`);}
    const chunks=[];let size=0;
    if(Number(response.headers.get('content-length'))>limit){await response.body?.cancel();throw new Error('Update response exceeds its size limit');}
    const reader=response.body.getReader();
    try{for(;;){const {value,done}=await reader.read();if(done)break;size+=value.length;if(size>limit)throw new Error('Update response exceeds its size limit');chunks.push(value);}}
    finally{await reader.cancel().catch(()=>{});reader.releaseLock();}
    return Buffer.concat(chunks,size);
  }
  throw new Error('Too many update redirects');
}
function asset(release,name,repo) {
  const found=release.assets?.filter(a=>a.name===name);
  if(found?.length!==1 || !found[0].browser_download_url?.startsWith(`https://github.com/${repo}/releases/download/`))throw new Error(`Release must publish one ${name} asset in the selected repository`);
  return found[0].browser_download_url;
}
export function verify(bytes,checksum,name) {
  const fields=checksum.toString('utf8').trim().split(/\s+/);
  if(fields.length!==2 || fields[1].replace(/^\*/,'')!==name || !/^[a-f\d]{64}$/i.test(fields[0]) || digest(bytes)!==fields[0].toLowerCase())throw new Error('Update SHA-256 verification failed; the installed package was preserved');
}
export async function unpack(archive,destination,{run=execute}={}) {
  if(process.platform!=='win32')throw new Error('Complete package updates are currently supported on Windows x64');
  const shell=path.join(process.env.SystemRoot || 'C:\\Windows','System32','WindowsPowerShell','v1.0','powershell.exe');
  const result=await run(shell,['-NoProfile','-NonInteractive','-File',path.join(packageRoot,'src','unpack.ps1'),'-Archive',archive,'-Destination',destination],{timeout:60000,maxBuffer:65536});
  if(result.code)throw new Error(result.stderr || 'Cannot extract the verified update package');
}

export function updateManager({root=packageRoot,destination=path.join(defaultRuntimeRoot(),'updates'),get=download,run=execute,extract=unpack,host,platform=process.platform,arch=process.arch}={}) {
  let offer=null,pending=null,pendingRepo=null,applying=null,serial=0;
  async function check(repo,cli) {
    const request=++serial;offer=null;
    if(!repo)return {state:'not_configured',message:'Set the trusted update repository in plugin settings to check releases.'};
    repo=repository(repo);
    const manifest=JSON.parse(await readFile(path.join(root,'orca-plugin.json'),'utf8'));
    const nativeRequest=(async()=>{
      try{
        const result=await run(cli.executable,['--json','update','--check','--repository',repo],{timeout:50000,maxBuffer:65536});
        const data=JSON.parse(result.stdout);if(result.code)throw new Error(data.error?.message || 'CLI update check failed');
        if(data.contract_version!==1 || data.component!=='cli' || data.repository!==repo)throw new Error('CLI update response does not match the selected repository');
        return data;
      }catch(error){return {error:error.message};}
    })();
    const release=JSON.parse(await get(`https://api.github.com/repos/${repo}/releases/latest`,1024*1024));
    if(release.draft || release.prerelease)throw new Error('Only stable releases are accepted');
    const latest=stableVersion(release.tag_name),available=newer(latest,manifest.version);
    const native=await nativeRequest;
    const result={state:'checked',repository:repo,current:manifest.version,latest,available,cli:native,checkedAt:Date.now()};
    if(available){
      if(platform!=='win32' || arch!=='x64')return {...result,available:false,message:'Complete package updates are currently supported on Windows x64'};
      const name=`jtasks-orca-${latest}-${platform}-${arch}.zip`;
      const next={token:randomUUID(),repo,latest,name,url:asset(release,name,repo),checksum:asset(release,`${name}.sha256`,repo)};
      if(request===serial)offer=next;
      result.token=next.token;
    }
    return result;
  }
  async function apply(token) {
    const selected=offer;
    if(!selected || selected.token!==token)throw new Error('Check for updates again before installing');
    const metadata=JSON.parse(await readFile(path.join(root,'runtime.json'),'utf8'));
    if(offer!==selected)throw new Error('Update source changed; check again');
    // Published installations keep their repository identity and approval via Orca.
    if(metadata.pluginKey!=='local.jtasks')return host(['plugin','update',metadata.pluginKey,'--json']);
    const checksum=await get(selected.checksum,1024),bytes=await get(selected.url,128*1024*1024);
    verify(bytes,checksum,selected.name);
    if(offer!==selected)throw new Error('Update source changed; check again');
    const stage=path.join(destination,randomUUID()),archive=path.join(stage,selected.name),extracted=path.join(stage,'unpacked'),folder=path.join(extracted,'package');
    await mkdir(stage,{recursive:true});await writeFile(archive,bytes,{flag:'wx'});
    await extract(archive,extracted);
    const manifest=JSON.parse(await readFile(path.join(folder,'orca-plugin.json'),'utf8'));
    const runtime=JSON.parse(await readFile(path.join(folder,'runtime.json'),'utf8'));
    if(manifest.id!=='jtasks' || manifest.version!==selected.latest || runtime.pluginKey!=='local.jtasks' || runtime.cliVersion!==selected.latest)throw new Error('Downloaded package identity/version does not match the selected release');
    if(offer!==selected)throw new Error('Update source changed; check again');
    // Host validates all files and capabilities; never provide --approve automatically.
    return host(['plugin','install',folder,'--json']);
  }
  return {
    check(repo,cli){if(pending && pendingRepo!==repo){serial++;offer=null;pending=null;}if(!pending){pendingRepo=repo;const job=check(repo,cli).finally(()=>{if(pending===job)pending=null;});pending=job;}return pending;},
    apply(token){return applying ||= apply(token).finally(()=>{applying=null;});},
    invalidate(){serial++;offer=null;pending=null;}
  };
}

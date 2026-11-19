// Build an offline, platform-specific Orca folder from an already packaged native CLI.
import {cp, mkdir, readFile, writeFile} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {digest, compatible} from '../src/runtime.mjs';
import {repository} from '../src/updates.mjs';

const root=fileURLToPath(new URL('../',import.meta.url)), product=path.dirname(root);
const [nativeStage,output,platform=process.platform,arch=process.arch,pluginKey='local.jtasks',updateRepository=process.env.JTASKS_UPDATE_REPOSITORY || '']=process.argv.slice(2);
if(!nativeStage || !output || !path.isAbsolute(nativeStage) || !path.isAbsolute(output))throw new Error('Usage: node scripts/package.mjs <absolute-native-package-folder> <absolute-new-output-folder> [platform] [arch] [plugin-key] [update-repository]');
const relative=path.relative(product,path.resolve(output));
if(!relative || !relative.startsWith('..'+path.sep) && relative!=='..' && !path.isAbsolute(relative))throw new Error('Build outputs must be outside the product repository');
if(!/^[a-z0-9-]+\.jtasks$/.test(pluginKey))throw new Error('Invalid installed plugin key');
const manifest=JSON.parse(await readFile(path.join(root,'orca-plugin.json'),'utf8'));
if(updateRepository)manifest.contributes.settings.find(s=>s.key==='updateRepository').default=repository(updateRepository);
const info=JSON.parse(await readFile(path.join(nativeStage,'build-info.json'),'utf8'));
if(info.version!==manifest.version)throw new Error('Complete update packages require matching CLI and plugin versions');
const targets={'win32-x64':'x86_64-pc-windows-msvc','linux-x64':'x86_64-unknown-linux-gnu'};
if(!targets[`${platform}-${arch}`] || info.target!==targets[`${platform}-${arch}`])throw new Error('Native package target does not match the requested plugin platform');
const name=platform==='win32'?'jtasks.exe':'jtasks', binary=path.join(nativeStage,name);
const bytes=await readFile(binary);
if(digest(bytes)!==info.binary_sha256)throw new Error('Native package binary does not match its build metadata');
if(!await compatible(binary,info.version))throw new Error('Build this package on its target platform with a compatible native CLI');
await mkdir(path.dirname(output),{recursive:true});
await mkdir(output); // Refuse accidental replacement of a loaded or prior package.
for(const item of ['src','panel','orca-plugin.json','README.md'])await cp(path.join(root,item),path.join(output,item),{recursive:true,errorOnExist:true,force:false});
await writeFile(path.join(output,'orca-plugin.json'),JSON.stringify(manifest,null,2)+'\n');
await mkdir(path.join(output,'panel','shared'));
for(const item of ['app.js','style.css'])await cp(path.join(product,'web',item),path.join(output,'panel','shared',item));
await mkdir(path.join(output,'runtime'));
await cp(binary,path.join(output,'runtime',name));
for(const item of ['LICENSE','licenses','dependencies.json','build-info.json'])await cp(path.join(nativeStage,item),path.join(output,'runtime',item),{recursive:true,errorOnExist:true,force:false});
await writeFile(path.join(output,'runtime.json'),JSON.stringify({cliVersion:info.version,pluginKey,artifacts:{[`${platform}-${arch}`]:{file:`runtime/${name}`,sha256:digest(bytes)}}},null,2)+'\n');
await writeFile(path.join(output,'package-info.json'),JSON.stringify({plugin:manifest.id,version:manifest.version,nativeVersion:info.version,platform,arch,pluginKey},null,2)+'\n');
console.log(`Complete Orca package: ${output}`);

import {createHash, randomUUID} from 'node:crypto';
import {chmod, copyFile, mkdir, readFile, realpath, rm, writeFile} from 'node:fs/promises';
import {constants} from 'node:fs';
import {homedir} from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {execute} from './process.mjs';

export const packageRoot = fileURLToPath(new URL('../', import.meta.url));
export const digest = bytes => createHash('sha256').update(bytes).digest('hex');

export async function compatible(file, version, run = execute) {
  try {
    const v = await run(file, ['--version'], {timeout:3000, maxBuffer:65536});
    if (v.code || v.stdout.trim() !== `jtasks ${version}`) return false;
    const help = await run(file, ['read', '--help'], {timeout:3000, maxBuffer:65536});
    return help.code === 0 && ['progress', 'errors', '--revision', '--task', '--data-dir'].every(s => help.stdout.includes(s));
  } catch { return false; }
}

export function defaultRuntimeRoot() {
  const base = process.platform === 'win32' ? process.env.LOCALAPPDATA || path.join(homedir(),'AppData','Local') : process.env.XDG_DATA_HOME || path.join(homedir(),'.local','share');
  return path.join(base,'jtasks','orca');
}

async function protectManaged(bin) {
  await writeFile(path.join(bin,'jtasks-installation.json'),JSON.stringify({self_update:false,reason:'Update the complete plugin and CLI package from the Orca panel.'})+'\n',{flag:'wx'}).catch(error=>{if(error.code!=='EEXIST')throw error;});
}

export async function provision({root = packageRoot, destination = defaultRuntimeRoot(), override = '', envPath = process.env.PATH || '', platform = process.platform, arch = process.arch, run = execute, report = () => {}} = {}) {
  const manifest = JSON.parse(await readFile(path.join(root,'runtime.json'),'utf8'));
  const artifact = manifest.artifacts[`${platform}-${arch}`];
  if (!artifact) throw new Error(`This package has no native CLI for ${platform}/${arch}. Install a package built for this platform.`);
  if (!/^\d+\.\d+\.\d+$/.test(manifest.cliVersion) || !/^[a-f0-9]{64}$/.test(artifact.sha256)) throw new Error('Invalid bundled CLI metadata');
  if (override) {
    if (!path.isAbsolute(override) || !await compatible(override, manifest.cliVersion, run)) throw new Error('The configured executable is not a compatible native jtasks CLI');
    return {executable:await realpath(override),source:'configured',version:manifest.cliVersion};
  }
  report('Checking the native CLI…');
  const name = platform === 'win32' ? 'jtasks.exe' : 'jtasks';
  for (const dir of envPath.split(path.delimiter).filter(Boolean)) {
    if (!path.isAbsolute(dir)) continue;
    const candidate = path.join(dir,name);
    if (await compatible(candidate, manifest.cliVersion, run)) return {executable:await realpath(candidate),source:'existing',version:manifest.cliVersion};
  }
  const base = path.resolve(destination);
  const bin = path.join(base,`${manifest.cliVersion}-${artifact.sha256.slice(0,16)}`,'bin');
  const target = path.join(bin,name);
  try {
    if (digest(await readFile(target)) === artifact.sha256 && await compatible(target, manifest.cliVersion, run)) {await protectManaged(bin);return {executable:target,source:'managed',version:manifest.cliVersion};}
    // Never overwrite an existing binary, including a corrupt one that may be in use.
    throw new Error('The managed CLI has changed. Choose a clean runtime location before retrying.');
  } catch (e) { if (e.code !== 'ENOENT') throw e; }
  const source = path.resolve(root,artifact.file);
  if (!source.startsWith(path.resolve(root)+path.sep)) throw new Error('Bundled executable escapes the plugin package');
  const bytes = await readFile(source);
  if (digest(bytes) !== artifact.sha256) throw new Error('Bundled CLI integrity check failed');
  report('Preparing the bundled native CLI…');
  await mkdir(bin,{recursive:true});
  await protectManaged(bin);
  const temporary = path.join(bin,`.install-${randomUUID()}${platform === 'win32' ? '.exe' : ''}`);
  try {
    await copyFile(source,temporary,constants.COPYFILE_EXCL);
    if (platform !== 'win32') await chmod(temporary,0o755);
    if (digest(await readFile(temporary)) !== artifact.sha256 || !await compatible(temporary, manifest.cliVersion, run)) throw new Error('Bundled CLI verification failed');
    // Exclusive publication avoids concurrent installers replacing each other's binary.
    await copyFile(temporary,target,constants.COPYFILE_EXCL).catch(async e => {
      if (e.code !== 'EEXIST' || digest(await readFile(target)) !== artifact.sha256) throw e;
    });
    if (platform !== 'win32') await chmod(target,0o755);
  } finally { await rm(temporary,{force:true}); }
  report('Native CLI ready');
  return {executable:target,source:'managed',version:manifest.cliVersion};
}

export function runtimeManager(options = {}) {
  let pending;
  return {ensure(extra = {}) { return pending ||= provision({...options,...extra}).catch(error => {pending = undefined;throw error;}); }};
}

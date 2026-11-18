import {execute} from './process.mjs';

const views = new Set(['runs','status','tasks','task','attempts','attempt','operations','decisions','questions','commands','command','logs','progress','errors']);
const fields = new Set(['after','limit','revision','task','attempt','command','stream']);
export function readArguments(context, view, input = {}) {
  if (!views.has(view) || !input || typeof input !== 'object' || Array.isArray(input)) throw new Error('Invalid read request');
  const args = ['--lang','en','--root',context.root];
  if (context.dataDir) args.push('--data-dir',context.dataDir);
  args.push('read',view);
  if (view !== 'runs') {
    if (!context.run || typeof context.run !== 'string') throw new Error('Select a project run first');
    args.push('--run',context.run);
  }
  for (const [key,value] of Object.entries(input)) {
    if (!fields.has(key)) throw new Error(`Unsupported read field: ${key}`);
    if (['after','limit','revision'].includes(key)) {
      if (typeof value !== 'number' && typeof value !== 'string' || value === '') throw new Error('Invalid numeric read field');
      const n = Number(value);
      if (!Number.isSafeInteger(n) || n < (key === 'limit' ? 1 : 0) || key === 'limit' && n > (view === 'logs' ? 4096 : 100)) throw new Error('Read cursor/page size is outside contract bounds');
    } else if (typeof value !== 'string' || value.length > 512 || value.includes('\0')) throw new Error('Invalid read selector');
    if (key === 'stream' && !['stdout','stderr'].includes(value)) throw new Error('Invalid output stream');
    args.push(`--${key}`,String(value));
  }
  return args;
}

export async function readProjection(executable, context, view, input, {signal, run = execute} = {}) {
  const response = await run(executable,readArguments(context,view,input),{signal,timeout:8000});
  if (response.code !== 0 && !(view === 'status' && response.code === 3)) {
    const reason = response.stderr.trim().slice(0,1000);
    const error = new Error(reason || `Native read failed (${response.code})`);
    if (reason.includes('no runtime database; read does not create state')) error.code = 'no_data';
    throw error;
  }
  let data;
  try {data = JSON.parse(response.stdout);} catch {throw new Error('Native CLI returned invalid JSON');}
  if (data.contract_version !== 1 || data.read_only !== true || !data.identity || typeof data.identity.checkout !== 'string' || (view !== 'runs' && data.identity.run !== context.run)) throw new Error('Native read contract or run identity does not match');
  if (Buffer.byteLength(JSON.stringify(data),'utf8') > 230000) throw Object.assign(new Error('Read page exceeds the Orca bridge limit; reduce its page size'),{code:'too_large'});
  return data;
}

export async function runs(executable, context, options) {
  const result = [],seen=new Set();let after = 0,checkout;
  for (;;) {
    const page = await readProjection(executable,context,'runs',{after,limit:50},options);
    if (!Array.isArray(page.runs) || !page.page) throw new Error('Invalid run collection');
    checkout ||= page.identity.checkout;
    if(checkout!==page.identity.checkout)throw new Error('Run checkout changed during pagination');
    for(const run of page.runs){if(typeof run.run!=='string' || seen.has(run.run))throw new Error('Invalid or overlapping run collection');seen.add(run.run);}
    result.push(...page.runs);
    if (result.length > 10000) throw new Error('Too many runs to list safely');
    if (!page.page.has_more) return {runs:result,checkout:page.identity.checkout};
    if (!(page.page.next_after > after)) throw new Error('Run cursor did not advance');
    after = page.page.next_after;
  }
}

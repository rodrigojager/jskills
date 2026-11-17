'use strict';
// One bounded projection per view. Raw output replaces its previous page.
const main = document.querySelector('main');
const identity = document.querySelector('#identity');
const connection = document.querySelector('#connection');
const error = document.querySelector('#error');
let presentation = {};
let commandLabels = {};
const translated = (key, fallback) => presentation[key] || fallback;
let generation = 0;
let busy = false;
let tasksRoute = '#tasks';
const labels = {completed: 'completed by judgment', verification_pending: 'verification pending'};
const human = value => String(value).replaceAll('_', ' ');
function el(tag, text, cls) {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = text;
  if (cls) node.className = cls;
  return node;
}
function link(text, href) { const a = el('a', text); a.href = href; return a; }
function route(view, fields = {}) { return `#${view}?${new URLSearchParams(fields)}`; }
function state(value) { return el('span', labels[value] || commandLabels[value] || human(value ?? 'unknown'), `state ${value || 'unknown'}`); }
function detail(title, value, open = false) {
  const d = el('details'); d.open = open; d.append(el('summary', title));
  d.append(el('pre', JSON.stringify(value, null, 2))); return d;
}
function field(label, value, parent) {
  const p = el('p'); p.append(el('strong', `${label}: `), document.createTextNode(value === null || value === undefined ? 'unknown / unavailable' : typeof value === 'object' ? JSON.stringify(value) : String(value))); parent.append(p);
}
function button(label, action, disabled = false) { const b = el('button', label); b.disabled = disabled; b.onclick = action; return b; }
async function read(view, fields = {}) {
  const response = await fetch(`/api/read?${new URLSearchParams({view, limit: 25, ...fields})}`, {signal: AbortSignal.timeout(4000), cache: 'no-store'});
  const data = await response.json();
  if (!response.ok) throw new Error(data.error || `HTTP ${response.status}`);
  return data;
}
function metadata(data) {
  identity.replaceChildren();
  field('Checkout / run', `${data.identity.checkout} / ${data.identity.run}`, identity);
  field('As of persisted journal', data.as_of, identity);
  field('Source', data.as_of_source, identity);
  field('Graph revision / event cursor', `${data.graph_revision ?? 'unknown'} / ${data.event_cursor}`, identity);
  field('Observation', 'SQLite read snapshot · controller unobserved · native helpers unobserved (count unavailable)', identity);
  identity.append(detail('Provenance, availability and projection limits', {provenance:data.provenance, availability:data.availability, native_helpers:data.native_helpers, projection_limits:data.projection_limits}));
}
function pager(data, view, fields) {
  if (!data.page) return;
  const p = data.page, bar = el('div', undefined, 'pager');
  bar.append(el('span', `Page offset ${p.after} · ${p.total ?? 'journal'} records · limit ${p.limit}`));
  const current = () => view === 'tasks' ? Object.fromEntries(new URLSearchParams(location.hash.split('?')[1] || '')) : fields;
  bar.append(button('First page', () => location.hash = route(view, {...current(), after:0}), p.after === 0));
  bar.append(button('Next page', () => location.hash = route(view, {...current(), after:p.next_after, ...(p.graph_revision === null || p.graph_revision === undefined ? {} : {revision:p.graph_revision})}), !p.has_more));
  main.append(bar);
}
function card(id, status, href, title) {
  const c = el('article', undefined, 'card');
  const h = el('h3'); h.append(link(id, href)); c.append(h, state(status));
  if (title) c.append(el('p', title));
  return c;
}
function overview(data) {
  field('Run state', human(data.state), main);
  field(translated('objective', 'Objective'), data.objective, main);
  const grid = el('div', undefined, 'grid');
  for (const [key, value] of Object.entries(data.task_counts)) {
    const c = el('div', undefined, 'card'); c.append(state(key), el('h3', String(value))); grid.append(c);
  }
  main.append(grid);
  field('Development complete', data.development_complete, main);
  field('Verification complete', data.verification_complete, main);
  field('Pending required operations', data.pending_required_operations, main);
  main.append(detail('Requirement coverage and graph', data.graph), detail('Next automatic action (persisted)', data.next_automatic_action, true));
  main.append(el('h3', 'Operations (bounded page)'));
  for (const [index, op] of data.operations.entries()) {
    const c = card(op.id, op.state, route('operation', {id:op.id, after:Math.floor(index / 25) * 25}));
    field('Placement / required', `${JSON.stringify(op.stage)} / ${op.required}`, c);
    main.append(c);
  }
  main.append(el('h3', 'Task progress, attempts and waits (bounded page)'));
  for (const task of data.tasks) {
    const c = card(task.id, task.state, route('task', {id:task.id}));
    c.append(detail('Attempts, review, cooldown and result provenance', task)); main.append(c);
  }
  pager(data, 'overview', {});
}
function taskList(data, params) {
  const toolbar = el('div', undefined, 'toolbar');
  const label = el('label', 'Task filter (this page)'); const filter = el('input');
  filter.type = 'search'; filter.value = params.get('q') || ''; label.append(filter);
  const statusLabel = el('label', 'State (this page)'); const select = el('select');
  for (const value of new Set(['', 'pending','running','waiting','paused','blocked','failed','completed','verification_pending','superseded', ...data.tasks.map(task => task.state)])) { const option = el('option', value ? human(value) : 'All states'); option.value = value; select.append(option); }
  select.value = params.get('state') || ''; statusLabel.append(select); toolbar.append(label, statusLabel); main.append(toolbar);
  const list = el('div'); main.append(list);
  function apply() {
    params.set('q', filter.value); params.set('state', select.value);
    tasksRoute = route('tasks', Object.fromEntries(params)); history.replaceState(null, '', tasksRoute);
    list.replaceChildren();
    for (const task of data.tasks.filter(t => `${t.id} ${t.deliverable}`.toLowerCase().includes(filter.value.toLowerCase()) && (!select.value || t.state === select.value))) list.append(card(task.id,task.state,route('task',{id:task.id}), task.deliverable));
    if (!list.children.length) list.append(el('p', 'No matching tasks on this bounded page.'));
  }
  filter.oninput = apply; select.onchange = apply; apply();
  pager(data, 'tasks', Object.fromEntries(params));
}
function taskDetail(data) {
  const t = data.tasks[0]; main.append(link('← Back to filtered tasks', tasksRoute), el('h3', t.deliverable), state(t.state));
  field('Acceptance policy', t.acceptance_policy, main);
  const deps = el('p', 'Dependencies: ');
  for (const id of t.depends_on || []) deps.append(link(`${id} `, route('task',{id})));
  if (!t.depends_on?.length) deps.append(document.createTextNode('none')); main.append(deps);
  main.append(detail('Description, criteria and task specification', t.spec, true), detail('Current attempt, configuration origins, activity, review and result', t.current_attempt_detail, true), detail('Reservations, waits and same-task recovery', {clock:t.clock, cooldown:t.cooldown, reservations:t.reservations, retry_authorizations:t.retry_authorizations, lineage_root:t.lineage_root}));
  if (t.current_attempt) main.append(link('Read current attempt output →', route('output',{attempt:t.current_attempt, after:0, stream:'stdout'})));
  main.append(el('h3','Attempt history'), link('Browse paged attempt history →', route('attempts',{task:t.id})));
}
function output(data, params) {
  field('Capture', `${data.capture_kind} ${data.capture_id} · ${data.stream}`, main);
  field('Availability', data.capture_availability, main);
  field('Byte range / captured snapshot', `${data.offset}–${data.next_offset} / ${data.captured_bytes ?? 'unknown'}`, main);
  field('Encoding', data.text_encoding || 'capture unknown', main);
  if (data.gap) main.append(detail('Capture gap', data.gap, true));
  main.append(el('pre', data.text ?? 'No retained capture is available.'));
  const toolbar = el('div', undefined, 'toolbar'); const label = el('label', 'Stream'); const select = el('select');
  for (const name of ['stdout','stderr']) { const option = el('option',name); option.value = name; select.append(option); }
  select.value = params.get('stream') || 'stdout'; label.append(select);
  select.onchange = () => location.hash = route('output',{...Object.fromEntries(params),stream:select.value,after:0}); toolbar.append(label);
  toolbar.append(button('First output page', () => location.hash = route('output',{...Object.fromEntries(params),after:0}), data.offset === 0));
  toolbar.append(button('Next output page', () => location.hash = route('output',{...Object.fromEntries(params),after:data.next_offset}), !data.has_more));
  main.append(toolbar, el('p','4096 bytes per explicit request. Each page replaces the prior page; output is never polled or accumulated.', 'muted'));
}
async function load(moveFocus = true) {
  const ticket = ++generation;
  const [selected, query = ''] = location.hash.slice(1).split('?');
  const view = selected || 'overview';
  const params = new URLSearchParams(query); const fields = {};
  for (const key of ['after','revision','task','stream','attempt','command']) if (params.has(key)) fields[key] = params.get(key);
  let kind = view;
  if (view === 'overview') kind = 'status';
  if (view === 'task') { kind = 'task'; fields.task = params.get('id'); }
  if (view === 'operation') kind = 'operations';
  if (view === 'output') { kind = 'logs'; fields.limit = 4096; }
  if (!['status','tasks','task','attempts','attempt','operations','decisions','questions','commands','command','logs'].includes(kind)) { error.hidden = false; error.textContent = 'Unknown read view'; return; }
  busy = true;
  try {
    const data = await read(kind, fields);
    if (ticket !== generation) return;
    error.hidden = true; connection.textContent = translated('snapshot', 'Snapshot available · read only'); metadata(data);
    main.replaceChildren(el('h2', view === 'task' || view === 'operation' ? `${human(view)} ${params.get('id')}` : translated(view, human(view))));
    if (view === 'overview') overview(data);
    else if (view === 'tasks') taskList(data,params);
    else if (view === 'task') taskDetail(data);
    else if (view === 'output') output(data,params);
    else if (view === 'operation') {
      const op = data.operations.find(o => o.id === params.get('id'));
      if (!op) throw new Error('Operation not in this page; return to operations and refetch after graph changes.');
      main.append(link('← Back to operations',route('operations',fields)), state(op.state), detail('Placement, inputs, command and repair history',op,true));
      if (op.command_id) main.append(link('Read operation output →',route('output',{command:op.command_id,after:0,stream:'stdout'})));
    } else {
      const items = data.operations || data.attempts || data.commands || data.records || [];
      if (!items.length) main.append(el('p', data.state === 'not_initialized' ? 'Not initialized; current state is unavailable.' : 'No recorded items on this page.'));
      for (const item of items) {
        const id = item.id || item.command_id; const c = el('article',undefined,'card');
        if (view === 'operations') { c.append(link(id,route('operation',{id,...fields})), document.createTextNode(' '),state(item.state)); }
        if (view === 'attempts') c.append(link(`Output for ${id}`,route('output',{attempt:id,after:0,stream:'stdout'})));
        if (view === 'commands') c.append(link(`Output for ${id}`,route('output',{command:id,after:0,stream:'stdout'})));
        c.append(detail(id || 'Journal record',item,true)); main.append(c);
      }
      pager(data,view,fields);
    }
    if (moveFocus) main.focus({preventScroll:true});
  } catch (e) {
    if (ticket !== generation) return;
    error.hidden = false; error.textContent = `Read unavailable: ${e.message}. Last rendered snapshot may be stale; explicit refresh can refetch changed cursors.`;
    connection.textContent = 'Read unavailable · no control action taken';
  } finally { if (ticket === generation) busy = false; }
}
window.addEventListener('hashchange', () => load());
document.querySelector('#refresh').onclick = () => load(false);
// Only overview refreshes automatically. No output polling or hidden-tab traffic.
setInterval(() => { if (!busy && !document.hidden && !main.contains(document.activeElement) && (!location.hash || location.hash === '#overview')) load(false); }, 5000);
async function start() {
  try {
    const response = await fetch('/presentation.json', {signal: AbortSignal.timeout(4000), cache:'no-store'});
    if (!response.ok) throw new Error('Presentation unavailable');
    const data = await response.json(); presentation = data.labels; commandLabels = data.command_states || {};
    document.documentElement.lang = data.language;
    document.documentElement.dir = data.language === 'ar' ? 'rtl' : 'ltr';
    for (const node of document.querySelectorAll('[data-i18n]')) node.textContent = translated(node.dataset.i18n, node.textContent);
  } catch { /* English fallback remains usable if presentation is unavailable. */ }
  load(false);
}
start();

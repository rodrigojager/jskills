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
  if (window.jtasksTransport) return window.jtasksTransport.read(view, {limit:25,...fields});
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
// The dashboard collects every bounded task page from one graph revision.
const statusName = value => ({completed:'Completed',running:'In progress',pending:'Not started',verification_pending:'Awaiting verification',superseded:'Superseded'})[value] || human(value || 'unknown');
function duration(ms) {
  if (typeof ms !== 'number' || !Number.isFinite(ms)) return 'Unavailable';
  if (ms < 1000) return '< 1s';
  const s = Math.floor(ms / 1000), m = Math.floor(s / 60), h = Math.floor(m / 60);
  return h ? `${h}h ${m % 60}m` : m ? `${m}m ${s % 60}s` : `${s}s`;
}
function icon(value) {
  const n = el('span', value === 'running' ? '' : ({completed:'✓',pending:'○',failed:'✕',needs_intervention:'✕',blocked:'!',superseded:'−'})[value] || '◌', `task-icon ${value || 'unknown'}`);
  n.setAttribute('aria-label', statusName(value)); n.setAttribute('role','img'); return n;
}
async function allTasks(status, ticket) {
  const tasks = [], seen = new Set(); let after = 0;
  for (;;) {
    const data = await read('tasks', {limit:100,after,...(status.graph_revision == null ? {} : {revision:status.graph_revision})});
    if (ticket !== generation) return null;
    if (String(data.graph_revision) !== String(status.graph_revision)) throw new Error('The task graph changed. Refresh to read a consistent list');
    for (const t of data.tasks) { if (seen.has(t.id)) throw new Error('Task pages overlap; refresh the snapshot'); seen.add(t.id); tasks.push(t); }
    if (!data.page.has_more) {
      if (tasks.length !== data.page.total) throw new Error('The complete task list could not be read');
      return tasks;
    }
    if (!(data.page.next_after > after)) throw new Error('Task pagination did not advance');
    after = data.page.next_after;
  }
}
function progressPanel(tasks, data) {
  const current = tasks.filter(t => t.state !== 'superseded'), completed = current.filter(t => t.state === 'completed').length;
  const running = current.filter(t => t.state === 'running').length, pending = current.filter(t => t.state === 'pending').length;
  const percent = current.length ? Math.round(completed / current.length * 100) : 0;
  const panel = el('div',undefined,'progress-panel'); panel.setAttribute('aria-label','Project progress');
  const top = el('div',undefined,'progress-top'); top.append(el('span','Project progress','progress-label'));
  const note = el('span',running ? `${running} in progress` : current.length && completed === current.length ? 'All tasks completed' : 'Recorded task states','status-note');
  if (running) note.prepend(icon('running')); top.append(note);
  const line = el('div',undefined,'progress-line'), bar = el('progress'); bar.max = current.length || 1; bar.value = completed; bar.setAttribute('aria-label','Completed tasks');
  const count = el('span',undefined,'progress-count'); count.append(el('strong',String(completed)),document.createTextNode(` / ${current.length} tasks`));
  line.append(bar,el('span',`${percent}%`,'progress-percent'),count);
  const legend = el('div',undefined,'progress-legend');
  for (const [cls,text] of [['',`${completed} completed`],['running',`${running} running`],['pending',`${pending} not started`]]) { const n=el('span'); n.append(el('i',undefined,`legend-dot ${cls}`),document.createTextNode(text)); legend.append(n); }
  const other = current.length - completed - running - pending;
  if (other) legend.append(el('span',`${other} other states`));
  const elapsed=el('div',undefined,'elapsed');
  const recorded=data.elapsed?.measurement==='recorded_span';
  const total=el('strong',duration(data.elapsed?.elapsed_ms));
  if(data.elapsed?.measurement==='wall_elapsed')live(total,data.elapsed.elapsed_ms);
  elapsed.append(el('span',recorded ? 'Recorded project span' : 'Total elapsed'),total);
  elapsed.title=data.elapsed?.started_at ? `Since ${data.elapsed.started_at}. ${data.elapsed.start_source}. ${data.elapsed.end_source}.` : 'No execution start has been recorded';
  const paused=el('span','','refresh-state'); paused.setAttribute('role','status'); paused.hidden=true; top.append(paused);
  const summary=el('div',undefined,'progress-summary'); summary.append(legend,elapsed);
  panel.append(top,line,summary); return panel;
}
function taskRows(tasks) {
  const section = el('div',undefined,'task-section'), toolbar=el('div',undefined,'list-toolbar');
  const filters=el('div',undefined,'filters'), search=el('input',undefined,'search'), filter=el('select');
  search.type='search'; search.placeholder='Find a task…'; search.setAttribute('aria-label','Search all tasks');
  filter.setAttribute('aria-label','Filter task state');
  for (const value of ['',...new Set(tasks.map(t=>t.state))]) { const option=el('option',value ? statusName(value) : 'All states'); option.value=value; filter.append(option); }
  const params = new URLSearchParams(location.hash.split('?')[1] || ''); search.value=params.get('q') || ''; filter.value=params.get('state') || '';
  filters.append(search,filter); toolbar.append(filters);
  const list=el('div',undefined,'task-list'); list.setAttribute('aria-label','Project tasks');
  const end=el('div',undefined,'list-end'); section.append(toolbar,list,end);
  function apply() {
    list.replaceChildren();
    const visible=tasks.filter(t=>`${t.id} ${t.deliverable}`.toLowerCase().includes(search.value.toLowerCase()) && (!filter.value || t.state===filter.value));
    for (const task of visible) {
      const item=el('details',undefined,`task-item is-${task.state}`), row=el('summary',undefined,`task-row is-${task.state}`);
      row.dataset.task=task.id; row.setAttribute('aria-label',`${task.id}: ${task.deliverable} · ${statusName(task.state)}`);
      const copy=el('span',undefined,'task-copy'); copy.append(el('span',task.id,'task-id'),el('span',task.deliverable,'task-title'));
      const time=el('span',task.state==='pending' && !task.current_attempt ? '—' : duration(task.clock?.active_ms),'task-duration'); time.title='Recorded active execution time';
      if(task.state==='running' && Number.isFinite(task.clock?.active_ms))live(time,task.clock.active_ms);
      const chevron=el('span','⌄','row-chevron'); chevron.setAttribute('aria-hidden','true');
      row.append(icon(task.state),copy,time,chevron);
      const content=el('div',undefined,'task-expansion'); item.append(row,content); list.append(item);
      let request=0, loaded=false;
      item.ontoggle=async()=>{
        const token=++request, page=generation;
        if(!item.open || loaded)return;
        content.replaceChildren(el('p','Reading task details…','muted'));
        try { const data=await read('task',{task:task.id});
          if(token!==request || page!==generation || !item.isConnected || !item.open)return;
          const t=data.tasks[0]; if(!t)throw new Error('Task unavailable');
          content.replaceChildren(taskDetails(t)); loaded=true;
        } catch(e) { if(token===request && page===generation && item.isConnected)content.replaceChildren(el('p',`Task details unavailable: ${e.message}`,'muted'),button('Retry',()=>{item.open=false;setTimeout(()=>{item.open=true;},0);})); }
      };
      item.onkeydown=e=>{if(e.key==='Escape' && item.open){e.preventDefault();item.open=false;row.focus();}};
    }
    if (!visible.length) list.append(el('div','No tasks match your search.','empty'));
    end.replaceChildren(el('span',visible.length===tasks.length ? `${tasks.length} tasks · complete list` : `${visible.length} of ${tasks.length} tasks`),el('span','Plan order'));
  }
  const update=()=>{ const current=new URLSearchParams(); for (const [key,value] of [['q',search.value],['state',filter.value]]) if(value)current.set(key,value); history.replaceState(null,'',route('overview',Object.fromEntries(current))); apply(); };
  search.oninput=update; filter.onchange=update; apply(); return section;
}
// A long working directory is cut in the middle: its drive or root and its last folders stay visible, because the
// end names the project. Folders are dropped from the middle until it fits; the full path stays in the tooltip.
function fitFolder(node) {
  const full=node?.dataset.full; if(!full || !node.isConnected) return;
  const sep=full.includes('\\') ? '\\' : '/', parts=full.split(sep);
  node.textContent=full;
  for(let keep=parts.length-2; keep>=1 && node.scrollWidth>node.clientWidth; keep--) node.textContent=[parts[0],'…',...parts.slice(-keep)].join(sep);
}
const folderObserver=new ResizeObserver(entries=>{ for(const entry of entries) fitFolder(entry.target.querySelector('.project-folder')); });
function projectHeading(data) {
  const heading=el('div',undefined,'project-heading');
  const title=el('div',undefined,'project-title'); title.append(el('h1',data.identity?.run || data.run || 'Your project'));
  const folder=data.identity?.checkout?.replace(/^\\\\\?\\/,'');
  if(folder){
    const path=el('span',folder,'project-folder'); path.title=`Working directory: ${folder}`; path.dataset.full=folder; title.append(path);
    folderObserver.disconnect(); folderObserver.observe(title); requestAnimationFrame(()=>fitFolder(path));
  }
  heading.append(el('p','PROJECT / TASKS','eyebrow'),title);
  return heading;
}
function projectTabs(tasks) {
  const container=el('div',undefined,'project-tabs-content'), actions=el('div',undefined,'project-tabs'), host=el('section',undefined,'feed-content'), list=taskRows(tasks);
  actions.setAttribute('role','tablist'); actions.setAttribute('aria-label','Project views');
  host.hidden=true; host.id='project-event-feed'; host.setAttribute('role','tabpanel');
  list.id='project-task-list'; list.setAttribute('role','tabpanel'); list.setAttribute('aria-labelledby','tab-tasks');
  let selected='tasks', request=0, next=0;
  const controls={};
  function select(view) {
    if(selected===view)return;
    selected=view;request++;list.hidden=view!=='tasks';host.hidden=view==='tasks';
    for(const [key,node] of Object.entries(controls)){node.setAttribute('aria-selected',String(view===key));node.tabIndex=view===key ? 0 : -1;}
    if(view!=='tasks'){host.setAttribute('aria-labelledby',`tab-${view}`);fetchPage(0);}
  }
  for(const [view,label] of [['tasks','Tasks'],['progress','Progress logs'],['errors','Errors']]) {
    const b=button(label,()=>select(view));b.id=`tab-${view}`;
    b.setAttribute('role','tab');b.setAttribute('aria-selected',String(view===selected));b.tabIndex=view===selected ? 0 : -1;
    b.setAttribute('aria-controls',view==='tasks' ? list.id : host.id);
    if(view==='tasks')b.append(el('span',String(tasks.length),'count-pill'));
    b.onkeydown=e=>{
      const keys=Object.keys(controls),i=keys.indexOf(view);
      const nextIndex={ArrowRight:(i+1)%keys.length,ArrowLeft:(i+keys.length-1)%keys.length,Home:0,End:keys.length-1}[e.key];
      if(nextIndex!==undefined){e.preventDefault();const nextView=keys[nextIndex];controls[nextView].focus();select(nextView);}
    };
    controls[view]=b;actions.append(b);
  }
  async function fetchPage(after){
    const token=++request,page=generation,view=selected;
    host.replaceChildren(el('p','Reading project events…','muted'));
    try {const data=await read(view,{after,limit:15});if(token!==request || page!==generation || !host.isConnected)return;
      next=data.page.next_after;
      const top=el('div',undefined,'feed-heading');top.append(el('h3',view==='errors' ? 'Latest project errors' : 'Latest project progress'));
      const refresh=button('Latest ↻',()=>fetchPage(0)); top.append(refresh);host.replaceChildren(top);
      const events=el('div',undefined,'event-list');
      for(const event of data.events){
        const item=el('article',undefined,'event-entry'), meta=el('div',undefined,'event-meta'),time=el('time');
        const date=new Date(event.recorded_at);time.dateTime=event.recorded_at;time.textContent=Number.isNaN(date.getTime()) ? event.recorded_at : date.toLocaleTimeString([], {hour:'2-digit',minute:'2-digit',second:'2-digit'});time.title=event.recorded_at;
        meta.append(el('span',event.task || 'Project','event-task'),el('span',human(event.event)),time);
        const value=event.detail;
        const text=typeof value==='string' ? value : value?.content || value?.reason || value?.summary || JSON.stringify(value);
        item.append(meta,el('p',event.summary || text || 'Recorded event'));
        if(value?.source)item.append(el('span',`${value.actor || 'unattributed'} · ${value.source}`,'event-source'));
        events.append(item);
      }
      if(!data.events.length)events.append(el('p',view==='errors' ? 'No recorded errors in this project.' : 'No project events recorded yet.','feed-empty'));
      const note=el('details',undefined,'feed-coverage');note.append(el('summary','About these records'),el('p',data.coverage));
      host.append(events,button('Older records ↓',()=>fetchPage(next),!data.page.has_more),note);
    }catch(e){if(token===request && page===generation && host.isConnected)host.replaceChildren(el('p',`Project events unavailable: ${e.message}`,'muted'),button('Retry',()=>fetchPage(after)));}
  }
  container.append(actions,list,host);return container;
}
function renderDashboard(data,tasks) {
  const panel=el('section',undefined,'project-panel');panel.setAttribute('aria-label','Project tasks and progress');
  panel.append(projectHeading(data),progressPanel(tasks,data),projectTabs(tasks));
  // An automatic refresh keeps the reader's place: list scroll, page scroll and the focused row.
  const oldList=main.querySelector('.task-list'), listTop=oldList?.scrollTop || 0, pageTop=document.scrollingElement?.scrollTop || 0;
  const focused=main.contains(document.activeElement) ? document.activeElement.closest('[data-task]')?.dataset.task : null;
  main.replaceChildren(panel);
  const list=main.querySelector('.task-list'); if(list)list.scrollTop=listTop;
  if(document.scrollingElement)document.scrollingElement.scrollTop=pageTop;
  if(focused)main.querySelector(`[data-task="${CSS.escape(focused)}"]`)?.focus({preventScroll:true});
}
// A value read at a snapshot keeps counting between reads, so a live run never looks frozen.
function live(node,ms){node.dataset.liveBase=String(ms);node.dataset.liveAt=String(Date.now());}
setInterval(()=>{
  if(document.hidden)return;
  for(const node of document.querySelectorAll('[data-live-base]'))node.textContent=duration(Number(node.dataset.liveBase)+Date.now()-Number(node.dataset.liveAt));
},1000);
function section(title,content) { const n=el('section',undefined,'detail-section'); n.append(el('h3',title),content); return n; }
function metric(label,value) { const n=el('div',undefined,'metric'); n.append(el('span',label),el('strong',value)); return n; }
function brief(t) { return t.spec?.context?.brief || t.deliverable; }
function taskDetails(t) {
  const box=el('div'); box.append(state(t.state));
  const metrics=el('div',undefined,'detail-metrics'); metrics.append(metric('Recorded execution',duration(t.clock?.active_ms)),metric('Attempts',t.attempt_history?.total == null ? 'Unavailable' : String(t.attempt_history.total)),metric('Dependency wait',duration(t.clock?.dependency_wait_ms)),metric('Review time',duration(t.clock?.reviewer_ms))); box.append(metrics);
  box.append(el('p',t.clock?.timing_uncertain ? 'Timing is uncertain. Values are persisted checkpoints.' : 'Recorded checkpoints · duration is not a live stopwatch.','detail-note'));
  box.append(section('TASK BRIEF',el('p',brief(t))));
  const dependencies=el('p');
  if (!t.depends_on?.length) dependencies.textContent='No dependencies';
  else for (const id of t.depends_on) { const dependency=el('span',id,'dependency'); dependencies.append(dependency,document.createTextNode(' ')); }
  box.append(section('DEPENDS ON',dependencies));
  const a=t.current_attempt_detail;
  if (a) {
    if (a.summary) box.append(section('RECORDED RESULT',el('p',a.summary)));
    const item=el('div',undefined,'attempt-entry'), top=el('div',undefined,'attempt-top'); top.append(el('strong','Latest attempt'),state(a.outcome || a.process_state));
    item.append(top,el('p',a.id,'mono'),el('p',`Recorded execution: ${duration(a.clock?.active_ms)}`));
    if (a.exit_code != null) item.append(el('p',`Exit code: ${a.exit_code}`));
    const profile=a.effective_profile?.values;
    if (profile && typeof profile==='object') item.append(el('p',Object.entries(profile).filter(([k,v])=>['provider','model','effort','agent','variant','autonomy','permissions'].includes(k) && typeof v==='string').map(([k,v])=>`${human(k)}: ${v}`).join(' · ')));
    box.append(section('EXECUTION',item));
    const links=el('p'); links.append(link('Open retained output ↗',route('output',{attempt:a.id,after:0,stream:'stdout'}))); box.append(section('OUTPUT',links));
  } else box.append(section('EXECUTION',el('p','No attempt has started.')));
  const history=el('p'); history.append(link('Browse attempt history ↗',route('attempts',{task:t.id}))); box.append(section('HISTORY',history));
  box.append(detail('Technical details',t)); return box;
}
function taskDetail(data){const t=data.tasks[0];if(!t)throw new Error('Task unavailable');main.append(link('← Back to project','#overview'),taskDetails(t));}
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
  if (window.jtasksTransport && !window.jtasksTransport.ready) return;
  const ticket = ++generation;
  const [selected, query = ''] = location.hash.slice(1).split('?');
  const view = !selected || selected==='workspace' ? 'overview' : selected;
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
    const dashboard = ['overview','tasks'].includes(view);
    const data = await read(dashboard ? 'status' : kind, fields);
    const tasks = dashboard ? await allTasks(data,ticket) : null;
    if (ticket !== generation) return;
    error.hidden = true; connection.textContent = translated('snapshot', 'Snapshot available · read only'); metadata(data);
    // The dashboard replaces itself in one step (renderDashboard), so its scroll and focus survive a refresh.
    if (!dashboard) main.replaceChildren(el('h2', view === 'task' || view === 'operation' ? `${human(view)} ${params.get('id')}` : translated(view, human(view))));
    if (dashboard) { renderDashboard(data,tasks); }
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
// Only overview refreshes automatically. No output polling or hidden-tab traffic. A pause is always said on screen.
function pauseReason() {
  if (main.querySelector('.task-item[open]')) return 'Auto-refresh paused while a task is open';
  if (main.querySelector('.project-tabs [aria-selected=true]:not(#tab-tasks)')) return 'Auto-refresh paused on this tab';
  if (document.activeElement?.matches?.('input')) return 'Auto-refresh paused while typing';
  return '';
}
setInterval(() => {
  if (document.hidden || (location.hash && location.hash !== '#overview')) return;
  const reason = pauseReason(), note = main.querySelector('.refresh-state');
  if (note) { note.textContent = reason; note.hidden = !reason; }
  if (!busy && !reason) load(false);
}, 5000);
document.addEventListener('toggle', () => { const note = main.querySelector('.refresh-state'), reason = pauseReason(); if (note) { note.textContent = reason; note.hidden = !reason; } }, true);
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
// Optional embedded hosts supply reads and own context lifecycle; HTTP stays standalone.
if (window.jtasksTransport) {
  window.jtasksTransport.attach({refresh:()=>load(false),reset:()=>{
    generation++;busy=false;main.replaceChildren();identity.replaceChildren();error.hidden=true;connection.textContent='Reading project context…';
  }});
} else start();

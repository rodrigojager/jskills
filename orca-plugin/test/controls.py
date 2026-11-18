"""Acceptance of the shared renderer's run controls in headless Chromium with a stand-in host.

python controls.py <absolute-web-folder> <absolute-screenshot-dir>
The stand-in serves web/ and fake read, stop and agents routes; no real run or CLI is involved. It covers the
browser path (fetch with the page token) and the Orca path (a injected window.jtasksTransport with actions).
"""
import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse
from playwright.sync_api import sync_playwright, expect

web, shots = map(Path, sys.argv[1:])
shots.mkdir(parents=True, exist_ok=True)
TOKEN = 'fixture-token'
STATE = {'controller': 'live', 'stopped': 0, 'sets': [], 'posts_without_token': 0}
TASKS = [
    {'id': 'G1', 'deliverable': 'the parity reference', 'state': 'completed'},
    {'id': 'G4', 'deliverable': 'the image side', 'state': 'running'},
    {'id': 'G5', 'deliverable': 'the screens for 140 tools', 'state': 'pending'},
    {'id': 'G6', 'deliverable': 'reference data', 'state': 'failed'},
]
ALIASES = {'run': 'megalinter-parity', 'aliases': [
    {'alias': 'deep', 'role': 'tasks', 'tasks': ['G2', 'G3', 'G4', 'G8', 'F07', 'I1'], 'states': {'completed': 3, 'running': 1, 'pending': 2},
     'configuration': {'executor': 'claude', 'provider': None, 'model': 'claude-opus-5-5', 'effort': 'high'}, 'source': 'project'},
    {'alias': 'standard', 'role': 'tasks', 'tasks': ['G1', 'G5'], 'states': {'completed': 1, 'pending': 67},
     'configuration': {'executor': 'claude', 'provider': None, 'model': 'claude-sonnet-5-5', 'effort': 'medium'}, 'source': 'project'},
    {'alias': 'observer', 'role': 'observer', 'tasks': ['G1', 'G2', 'G3'], 'states': {'pending': 3},
     'configuration': {'executor': 'claude', 'provider': None, 'model': 'claude-haiku-5-5', 'effort': 'medium'}, 'source': 'project'}]}
CHOICES = {
    'claude': {'executor': 'claude', 'catalogue': 'available', 'models': ['claude-opus-5-5', 'claude-sonnet-5-5', 'claude-haiku-5-5'], 'in_use': [], 'efforts': ['low', 'medium', 'high', 'xhigh', 'max'], 'free_text': False},
    'codex': {'executor': 'codex', 'catalogue': 'available', 'models': ['gpt-6.1-sol', 'gpt-6.1-sol-mini'], 'in_use': [], 'efforts': ['low', 'medium', 'high'], 'free_text': False},
    'opencode': {'executor': 'opencode', 'catalogue': 'unavailable', 'models': [], 'in_use': [], 'efforts': [], 'free_text': True}}


def status():
    return {'identity': {'checkout': 'D:\\clients\\some-very-long-customer-name\\monorepos\\platform\\project', 'run': 'megalinter-parity'},
            'as_of': 'now', 'as_of_source': 'fixture', 'graph_revision': 1, 'event_cursor': 1, 'provenance': {}, 'availability': {}, 'native_helpers': {}, 'projection_limits': {},
            'elapsed': {'measurement': 'recorded_span', 'elapsed_ms': 4500000}}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def send(self, code, body, kind='application/json'):
        data = body if isinstance(body, bytes) else json.dumps(body).encode()
        self.send_response(code)
        self.send_header('Content-Type', kind)
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        url = urlparse(self.path)
        query = {k: v[0] for k, v in parse_qs(url.query).items()}
        if url.path in ('/', '/index.html'):
            html = (web / 'index.html').read_text(encoding='utf-8')
            if STATE.get('meta', True):
                html = html.replace('@@JTASKS_TOKEN@@', TOKEN)
            if query.get('orca'):  # the Orca panel adds its theme mapping after the shared style
                html = html.replace('</head>', '<link rel="stylesheet" href="/panel.css"></head>')
            return self.send(200, html.encode(), 'text/html')
        if url.path == '/panel.css':
            return self.send(200, (web.parent / 'orca-plugin' / 'panel' / 'panel.css').read_bytes(), 'text/css')
        if url.path in ('/app.js', '/style.css'):
            return self.send(200, (web / url.path[1:]).read_bytes(), 'text/javascript' if url.path.endswith('js') else 'text/css')
        if url.path == '/presentation.json':
            return self.send(200, {'labels': {}, 'language': 'en'})
        if url.path == '/favicon.ico':
            return self.send(204, b'')
        if url.path == '/api/read':
            view = query.get('view', 'status')
            if view == 'status':
                return self.send(200, status())
            if view == 'tasks':
                return self.send(200, {'graph_revision': 1, 'tasks': TASKS, 'page': {'after': 0, 'next_after': len(TASKS), 'has_more': False, 'total': len(TASKS), 'limit': 100}})
            if view == 'runs':
                return self.send(200, {'runs': [{'run': 'megalinter-parity', 'controller': STATE['controller'], 'current': True, 'last_event_at': '2026-11-18T10:00:00Z',
                                                 'counts': {'completed': 1, 'running': 1 if STATE['controller'] == 'live' else 0, 'pending': 1, 'failed': 1, 'other': 0}}]})
            return self.send(400, {'error': 'unsupported view'})
        if url.path == '/api/agents' or url.path == '/api/agents/choices':
            if self.headers.get('X-Jtasks-Token') != TOKEN:
                return self.send(403, {'error': 'forbidden'})
            return self.send(200, CHOICES[query['executor']] if url.path.endswith('choices') else ALIASES)
        self.send(404, {'error': 'unknown'})

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get('Content-Length', 0))) or b'{}')
        if self.headers.get('X-Jtasks-Token') != TOKEN:
            STATE['posts_without_token'] += 1
            return self.send(403, {'error': 'forbidden'})
        if self.path == '/api/stop':
            STATE['stopped'] += 1
            return self.send(202, {'stopping': True})
        if self.path == '/api/agents':
            STATE['sets'].append(body)
            return self.send(200, {'alias': body['alias']})
        self.send(404, {'error': 'unknown'})


server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
base = f'http://127.0.0.1:{server.server_address[1]}/'

# The Orca path: the host injects a transport whose actions call the stand-in worker over fetch.
ORCA = """(()=>{const call=async(p,b)=>{const r=await fetch(p,{method:b?'POST':'GET',headers:{'X-Jtasks-Token':'%s','Content-Type':'application/json'},body:b?JSON.stringify(b):undefined});const d=await r.json();if(!r.ok)throw new Error(d.error);return d;};
let renderer;window.jtasksTransport={ready:true,
 async read(view,fields){const r=await fetch('/api/read?'+new URLSearchParams({view,...fields}));const d=await r.json();if(!r.ok)throw new Error(d.error);return d;},
 attach(x){renderer=x;x.refresh();},
 actions:{CONTINUE_PLACEHOLDER
  stop:()=>call('/api/stop',{}),agentsList:()=>call('/api/agents'),agentsChoices:e=>call('/api/agents/choices?executor='+e),agentsSet:c=>call('/api/agents',c)}};})();"""
CONTINUE = "continue:async()=>({message:'Opened Orca terminal jtasks · megalinter-parity running jtasks run.'}),"

LIGHT = "document.documentElement.style.cssText='--orca-background:#ffffff;--orca-foreground:#171717;--orca-card:#f7f7f7;--orca-secondary:#efefef;--orca-muted:#efefef;--orca-muted-foreground:#666;--orca-primary:#171717;--orca-primary-foreground:#fff;--orca-accent:#efefef;--orca-destructive:#dc2626;--orca-border:#e2e2e2;--orca-input:#fff;color-scheme:light';document.documentElement.dataset.tone='light'"
DARK = "document.documentElement.style.cssText='--orca-background:#141414;--orca-foreground:#ededed;--orca-card:#1b1b1b;--orca-secondary:#262626;--orca-muted:#262626;--orca-muted-foreground:#a1a1a1;--orca-primary:#ededed;--orca-primary-foreground:#141414;--orca-accent:#262626;--orca-destructive:#f87171;--orca-border:#2e2e2e;--orca-input:#1f1f1f;color-scheme:dark';document.documentElement.dataset.tone='dark'"


def open_page(browser, errors, width=390, orca=False, token=True):
    STATE['meta'] = token
    context = browser.new_context(viewport={'width': width, 'height': 900})
    page = context.new_page()
    page.on('pageerror', lambda e: errors.append(str(e)))
    page.on('console', lambda m: errors.append(m.text) if m.type == 'error' and 'status of 403' not in m.text and 'status of 400' not in m.text else None)
    if orca:
        page.add_init_script(ORCA.replace('CONTINUE_PLACEHOLDER', CONTINUE if orca == 'continue' else '') % TOKEN)
    page.goto(base + ('?orca=1' if orca else ''))
    expect(page.locator('.task-row')).to_have_count(4, timeout=10000)
    return page


def overflow(page):
    return page.evaluate('document.documentElement.scrollWidth<=innerWidth')


errors = []
try:
    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=True)

        # ---- browser path: live run, stop and agents, no continue --------------------------------------------
        STATE['controller'] = 'live'
        page = open_page(browser, errors)
        stop, agents = page.get_by_role('button', name='Stop run'), page.get_by_role('button', name='Agents')
        expect(stop).to_be_visible()
        expect(agents).to_be_visible()
        expect(page.get_by_role('button', name='Continue run')).to_have_count(0)
        assert stop.get_attribute('title') == 'Stop run' and agents.get_attribute('title') == 'Agents'
        # Icons sit at the right of the tabs row, inside it.
        row, box = page.locator('.project-tabs').bounding_box(), stop.bounding_box()
        assert box['x'] > row['x'] + row['width'] / 2 and row['y'] <= box['y'] <= row['y'] + row['height'], (row, box)
        for width in (350, 390, 550):
            page.set_viewport_size({'width': width, 'height': 900})
            assert overflow(page), width
            assert stop.bounding_box()['x'] + 28 <= width and agents.bounding_box()['x'] + 28 <= width
        page.set_viewport_size({'width': 390, 'height': 900})
        # Stop: confirmation names the running task; Esc closes it and returns focus to the icon.
        stop.focus()
        page.keyboard.press('Enter')
        dialog = page.get_by_role('dialog', name='Stop megalinter-parity?')
        expect(dialog).to_be_visible()
        expect(dialog).to_contain_text('1 running task (G4)')
        page.screenshot(path=str(shots / 'stop-dialog.png'))
        page.keyboard.press('Escape')
        expect(page.get_by_role('dialog')).to_have_count(0)
        expect(stop).to_be_focused()
        assert STATE['stopped'] == 0
        # Keep running does not stop either.
        stop.click()
        page.get_by_role('button', name='Keep running').click()
        assert STATE['stopped'] == 0
        # Agents editor: marks a change, says how many tasks it affects, Esc cancels without saving.
        agents.click()
        editor = page.get_by_role('dialog', name='Agents of megalinter-parity')
        expect(editor.locator('.alias')).to_have_count(3, timeout=10000)
        expect(editor).to_contain_text('running tasks keep the agent they started with')
        expect(editor).to_contain_text('No changes.')
        expect(editor.get_by_role('button', name='Save')).to_be_disabled()
        editor.get_by_label('Model of deep').select_option('claude-sonnet-5-5')
        expect(editor.get_by_label('Model of deep')).to_have_class('changed')
        expect(editor.locator('#summary, .effect[role=status]')).to_contain_text('deep will change. It applies to 2 tasks that have not started; 1 running keeps the agent it started with.')
        editor.get_by_label('Agent of standard').select_option('codex')
        expect(editor.get_by_label('Model of standard')).to_have_value('gpt-6.1-sol')
        expect(editor.get_by_label('Effort of standard')).to_have_value('medium')
        expect(editor.locator('.effect[role=status]')).to_contain_text('deep, standard will change. It applies to 69 tasks')
        page.screenshot(path=str(shots / 'agents-dialog-browser.png'))
        page.keyboard.press('Escape')
        expect(page.get_by_role('dialog')).to_have_count(0)
        assert STATE['sets'] == []
        # Reopened, the editor starts clean again and supports a custom OpenCode provider and variant.
        agents.click()
        editor = page.get_by_role('dialog', name='Agents of megalinter-parity')
        expect(editor.locator('.alias')).to_have_count(3, timeout=10000)
        expect(editor).to_contain_text('No changes.')
        editor.get_by_label('Agent of observer').select_option('opencode')
        expect(editor.get_by_label('Model name of observer')).to_have_value('')
        expect(editor.get_by_label('Effort of observer')).to_be_disabled()
        editor.get_by_label('Provider of observer').fill('company-ai')
        editor.get_by_label('Model name of observer').fill('vendor/haiku')
        editor.get_by_label('Variant of observer').fill('priority')
        editor.get_by_role('button', name='Save').click()
        expect(page.get_by_role('dialog')).to_have_count(0)
        assert STATE['sets'][-1]['alias'] == 'observer' and STATE['sets'][-1]['actor'] == 'web-dashboard'
        assert STATE['sets'][-1]['provider'] == 'company-ai' and STATE['sets'][-1]['effort'] == 'priority'
        assert STATE['sets'][-1]['model'] == 'vendor/haiku'
        agents.click()
        editor = page.get_by_role('dialog', name='Agents of megalinter-parity')
        editor.get_by_label('Model of deep').select_option('claude-haiku-5-5')
        STATE['sets'].clear()
        editor.get_by_role('button', name='Save').click()
        expect(page.get_by_role('dialog')).to_have_count(0)
        assert [s['alias'] for s in STATE['sets']] == ['deep'] and STATE['sets'][0]['model'] == 'claude-haiku-5-5', STATE['sets']
        expect(page.locator('.toast')).to_contain_text('Saved')
        # Stop for real.
        stop.click()
        page.get_by_role('button', name='■ Stop run').click()
        expect(page.locator('.toast')).to_contain_text('This run has stopped')
        assert STATE['stopped'] == 1 and STATE['posts_without_token'] == 0
        page.context.close()

        # ---- browser path without the page token: no action icons ------------------------------------------
        page = open_page(browser, errors, token=False)
        expect(page.locator('.icon-action')).to_have_count(0)
        page.context.close()

        # ---- Orca path: stopped run, continue icon, reminder, both themes, three widths ----------------------
        STATE['controller'] = 'stopped'
        page = open_page(browser, errors, orca='continue')
        expect(page.get_by_role('button', name='Stop run')).to_have_count(0)
        play = page.get_by_role('button', name='Continue run')
        expect(play).to_be_visible()
        assert play.get_attribute('title') == 'Continue run'
        expect(page.get_by_role('button', name='Agents')).to_be_visible()
        note = page.locator('.resume-note')
        expect(note).to_contain_text('This run stopped')
        expect(note).to_contain_text('with 2 tasks left')
        expect(note).to_contain_text('Press ▶ to continue.')
        assert note.evaluate("n=>n.nextElementSibling.classList.contains('project-panel')"), 'reminder above the card'
        expect(page.locator('#run, .run-picker')).to_have_count(0)
        for theme, script in (('dark', DARK), ('light', LIGHT)):
            page.evaluate(script)
            for width in (350, 390, 550):
                page.set_viewport_size({'width': width, 'height': 900})
                assert overflow(page), (theme, width)
                page.screenshot(path=str(shots / f'orca-stopped-{theme}-{width}.png'))
            page.set_viewport_size({'width': 350, 'height': 900})
            page.get_by_role('button', name='Agents').click()
            editor = page.get_by_role('dialog', name='Agents of megalinter-parity')
            expect(editor.locator('.alias')).to_have_count(3, timeout=10000)
            expect(editor).not_to_contain_text('running tasks keep')
            assert editor.evaluate('d=>d.scrollWidth<=d.clientWidth')
            editor.get_by_label('Model of deep').select_option('claude-haiku-5-5')
            page.screenshot(path=str(shots / f'orca-agents-{theme}-350.png'))
            page.keyboard.press('Escape')
            expect(page.get_by_role('dialog')).to_have_count(0)
            assert page.get_by_role('button', name='Agents').evaluate('n=>n===document.activeElement')
        play.click()
        expect(page.locator('.toast')).to_contain_text('Opened Orca terminal')
        page.context.close()

        # ---- Orca path where the host offers no continue: no icon, reminder without the hint ---------------
        page = open_page(browser, errors, orca=True)
        expect(page.get_by_role('button', name='Continue run')).to_have_count(0)
        expect(page.locator('.resume-note')).not_to_contain_text('Press')
        page.context.close()

        # ---- Orca path, live run: stop works through the transport ----------------------------------------
        STATE['controller'] = 'live'
        STATE['stopped'] = 0
        page = open_page(browser, errors, orca='continue')
        expect(page.locator('.resume-note')).to_have_count(0)
        expect(page.get_by_role('button', name='Continue run')).to_have_count(0)
        page.evaluate(LIGHT)
        page.get_by_role('button', name='Stop run').click()
        page.screenshot(path=str(shots / 'orca-stop-light.png'))
        page.get_by_role('button', name='■ Stop run').click()
        expect(page.locator('.toast')).to_contain_text('Stop requested')
        assert STATE['stopped'] == 1
        page.context.close()
        browser.close()
    assert not errors, errors
    print(json.dumps({'ok': True, 'paths': ['browser', 'orca'], 'themes': 2, 'widths': [350, 390, 550], 'javascript_errors': errors}))
finally:
    server.shutdown()

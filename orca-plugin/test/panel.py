"""Verify the built HTML panel in headless Chromium with a real CLI/worker stand-in.

python panel.py <absolute-package-folder> <absolute-fixture-dir> <absolute-screenshot-dir>
This is bridge acceptance, not proof that Orca loaded the plugin.
"""
import json
import subprocess
import sys
import urllib.request
from pathlib import Path
from playwright.sync_api import sync_playwright, expect

package, work, previews = map(Path, sys.argv[1:])
previews.mkdir(parents=True, exist_ok=True)
server = subprocess.Popen(['node', str(Path(__file__).with_name('panel-host.mjs')), str(package), str(work)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, creationflags=subprocess.CREATE_NO_WINDOW if sys.platform=='win32' else 0)
try:
    line = server.stdout.readline()
    if not line:
        raise RuntimeError(server.stderr.read())
    base = json.loads(line)['url']
    def get(route):
        with urllib.request.urlopen(base + route) as response:
            return json.load(response)
    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=True)
        page = browser.new_page(viewport={'width':390,'height':1000})
        errors=[]
        page.on('pageerror',lambda error: errors.append(str(error)))
        page.add_init_script("""window.__listeners={};window.orca={plugin:{
          async request(name,input){const r=await fetch('/bridge',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({name,input})});const d=await r.json();if(d.error)throw Object.assign(new Error(d.error.message),{code:d.error.code});return d.result;},
          on(name,handler){window.__listeners[name]=handler;return {dispose(){}};}
        }};""")
        page.goto(base)
        page.evaluate("""document.documentElement.style.cssText='--orca-background:#111315;--orca-foreground:#eceff3;--orca-card:#191c20;--orca-secondary:#252a30;--orca-secondary-foreground:#dce2e9;--orca-muted:#252a30;--orca-muted-foreground:#a4aeb9;--orca-primary:#a4e6c9;--orca-primary-foreground:#10251e;--orca-accent:#2d343b;--orca-accent-foreground:#ffffff;--orca-destructive:#ffa5a5;--orca-border:#353c44;--orca-input:#15181c;--orca-radius:10px;--orca-font-sans:Segoe UI;--orca-font-mono:Consolas'""")
        expect(page.locator('main')).to_contain_text('Select a project run',timeout=15000)
        page.locator('#run').select_option('Alpha-primary')
        expect(page.locator('.task-row')).to_have_count(205,timeout=15000)
        expect(page.locator('.progress-percent')).to_have_text('100%')
        expect(page.locator('.progress-count')).to_have_text('205 / 205 tasks')
        expect(page.locator('#tab-tasks .count-pill')).to_have_text('205')
        expect(page.locator('#cli')).to_contain_text('managed')
        expect(page.locator('#updates-label')).to_have_text('Update available')
        assert get('inspection')['hostUpdates']==[]
        page.locator('#updates summary').click()
        expect(page.locator('#updates-status')).to_contain_text('0.1.1')
        before_updates=len(get('inspection')['updateRequests'])
        page.locator('#check-updates').click()
        expect(page.locator('#check-updates')).to_be_enabled()
        assert len(get('inspection')['updateRequests'])>before_updates
        assert get('inspection')['hostUpdates']==[]
        page.locator('#apply-update').click()
        expect(page.locator('#update-approval')).to_contain_text('Fixture approval required')
        expect(page.locator('#update-approval')).to_contain_text('<script>unsafe()</script>')
        assert page.locator('#update-approval script').count()==0
        calls=get('inspection')['hostUpdates']
        assert len(calls)==1 and calls[0][:2]==['plugin','install'] and '--approve' not in calls[0]
        expect(page.locator('.task-row')).to_have_count(205)
        assert page.locator('.elapsed strong').inner_text()!='Unavailable'
        assert page.locator('.progress-panel').evaluate("n=>n.closest('.project-panel')===document.querySelector('.task-section').closest('.project-panel')")
        for width in [350,390,550]:
            page.set_viewport_size({'width':width,'height':1000})
            assert page.evaluate('document.documentElement.scrollWidth<=innerWidth'), width
        page.locator('#updates summary').click()
        page.set_viewport_size({'width':390,'height':1000})
        page.locator('main').focus()
        page.mouse.move(389,0)
        page.evaluate('scrollTo(0,0)')
        page.screenshot(path=str(previews/'orca-plugin-dark.png'),full_page=True)
        search=page.get_by_role('searchbox',name='Search all tasks')
        search.fill('T205');expect(page.locator('.task-row')).to_have_count(1)
        page.locator('.task-row').focus();page.keyboard.press('Enter')
        expect(page.locator('.task-expansion')).to_contain_text('Recorded fixture evidence')
        expect(page.locator('.task-expansion .metric').nth(1).locator('strong')).to_have_text('1')
        page.keyboard.press('Escape');expect(page.locator('.task-item')).not_to_have_attribute('open','')
        page.locator('#tab-tasks').focus();page.keyboard.press('ArrowRight')
        expect(page.locator('#tab-progress')).to_have_attribute('aria-selected','true')
        expect(page.locator('.event-entry')).to_have_count(15)
        before=get('inspection')['requests'];page.wait_for_timeout(5200)
        assert len(get('inspection')['requests'])==len(before),'Logs must not poll'
        page.locator('#tab-errors').click();expect(page.locator('.feed-empty')).to_be_visible()
        page.locator('#tab-tasks').click();search.fill('')
        page.evaluate("document.documentElement.style.cssText=document.documentElement.style.cssText.replace('#111315','#f7f8fa').replace('#eceff3','#202630').replace('#191c20','#ffffff').replaceAll('#252a30','#edf0f4').replace('#a4aeb9','#566171').replace('#a4e6c9','#286b59').replace('#10251e','#ffffff').replace('#2d343b','#e5eaf0').replace('#ffa5a5','#ac303f').replace('#353c44','#d3dae2').replace('#15181c','#ffffff')")
        page.locator('main').focus()
        page.mouse.move(389,0)
        page.evaluate('scrollTo(0,0)')
        page.screenshot(path=str(previews/'orca-plugin-light.png'),full_page=True)
        assert page.locator('body').evaluate('n=>getComputedStyle(n).backgroundColor')=='rgb(247, 248, 250)'
        get('switch?id=b');page.evaluate("window.__listeners['context.changed']({})")
        expect(page.locator('main')).not_to_contain_text('Alpha task')
        expect(page.locator('.task-row')).to_have_count(3)
        expect(page.locator('h1')).to_have_text('Beta-primary')
        expect(page.locator('.progress-percent')).to_have_text('0%')
        assert page.locator('.task-icon.pending').count()==3
        # Deliberately release an old native read after the new project is selected.
        get('hold');page.evaluate("location.hash='#tasks'")
        page.wait_for_timeout(100)
        get('switch?id=a');page.evaluate("window.__listeners['context.changed']({})")
        expect(page.locator('.task-row')).to_have_count(205)
        get('release');page.wait_for_timeout(300)
        expect(page.locator('h1')).to_have_text('Alpha-primary')
        expect(page.locator('.task-row')).to_have_count(205)
        get('switch?id=r');page.evaluate("window.__listeners['context.changed']({})")
        expect(page.locator('main')).to_contain_text('Remote and WSL')
        expect(page.locator('.task-row')).to_have_count(0)
        inspection=get('inspection')
        assert any(x.get('text')=='100% · 205/205' for x in inspection['status'])
        assert any(x['input'].get('fields',{}).get('after',0)>=200 for x in inspection['requests'] if x['name']=='read')
        assert not errors,errors
        browser.close()
    print(json.dumps({'ok':True,'tasks':205,'projects':2,'themes':2,'widths':[350,390,550],'worker':'real','native':'real','host':'stand-in','javascript_errors':errors}))
finally:
    server.terminate()
    server.wait(timeout=10)

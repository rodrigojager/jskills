// juice-it-up state tests: node --test scripts/jiu.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const CLI = path.join(path.dirname(fileURLToPath(import.meta.url)), 'jiu.mjs');

function sandbox() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'jiu-'));
  const code = path.join(root, 'site');
  fs.mkdirSync(path.join(code, 'src'), { recursive: true });
  fs.writeFileSync(path.join(code, 'src', 'Hero.tsx'), 'export const Hero = () => null;\n');
  fs.writeFileSync(path.join(code, 'src', 'Cta.tsx'), 'export const Cta = () => null;\n');
  const draft = path.join(root, 'draft.md');
  fs.writeFileSync(draft, '# proposal\n');
  const state = path.join(root, 'state');
  const jiu = (...args) => {
    const r = spawnSync(process.execPath, [CLI, ...args, '--state', state], { encoding: 'utf8' });
    return { code: r.status, out: r.stdout + r.stderr };
  };
  return { root, code, draft, state, jiu };
}

const GATE_FILES = ['recon.md', 'ux.md', 'art-direction.md', 'motion-media.md', 'a11y-perf.md', 'consolidated.md', 'seasonal.md'];

// Completes the full workflow's diagnostic evidence.
function passGate(s, id = 'R-001') {
  for (const f of GATE_FILES) fs.writeFileSync(path.join(s.state, 'runs', id, 'evidence', f), `# ${f}\n`);
}

function seeded({ gate = true } = {}) {
  const s = sandbox();
  assert.equal(s.jiu('init', '--url', 'https://exemplo.com/', '--code', s.code).code, 0);
  if (gate) passGate(s);
  assert.equal(s.jiu('opp-add', 'R-001', '--title', 'Hero', '--surface', 'home', '--paths', 'src/Hero.tsx').code, 0);
  assert.equal(s.jiu('opp-add', 'R-001', '--title', 'CTA', '--surface', 'home', '--paths', 'src/Cta.tsx').code, 0);
  assert.equal(s.jiu('variant-add', 'R-001', 'O-01', '--name', 'Editorial', '--file', s.draft).code, 0);
  assert.equal(s.jiu('variant-add', 'R-001', 'O-01', '--name', 'Quiet', '--file', s.draft).code, 0);
  assert.equal(s.jiu('variant-add', 'R-001', 'O-02', '--name', 'Tactile', '--file', s.draft).code, 0);
  return s;
}

const opps = (s) => JSON.parse(fs.readFileSync(path.join(s.state, 'runs', 'R-001', 'opportunities.json'), 'utf8')).opportunities;
const rev = (s, o, v, r) => opps(s).find((x) => x.id === o).variants.find((x) => x.id === v).revisions.find((x) => x.rev === r);

test('reinitializing the same target points to the existing run (resume)', () => {
  const s = sandbox();
  s.jiu('init', '--url', 'https://exemplo.com', '--code', s.code);
  const again = s.jiu('init', '--url', 'https://exemplo.com/', '--code', s.code);
  assert.equal(again.code, 2);
  assert.match(again.out, /EXISTING_RUN R-001/);
  assert.equal(s.jiu('init', '--url', 'https://exemplo.com', '--code', s.code, '--new').code, 0);
});

test('URL-only run does not accept implementation authorization', () => {
  const s = sandbox();
  s.jiu('init', '--url', 'https://exemplo.com');
  s.jiu('opp-add', 'R-001', '--title', 'Hero');
  s.jiu('variant-add', 'R-001', 'O-01', '--name', 'Editorial', '--file', s.draft);
  passGate(s);
  const r = s.jiu('decide', 'R-001', 'O-01/V1/r1', 'implement', '--quote', 'implement it');
  assert.equal(r.code, 3);
  assert.match(r.out, /URL-only run/);
});

test('stage gate refuses preview and implement until E1-E3 are complete', () => {
  const s = seeded({ gate: false });
  const ev = (f) => path.join(s.state, 'runs', 'R-001', 'evidence', f);
  const tryPreview = () => s.jiu('decide', 'R-001', 'O-01/V1/r1', 'preview', '--quote', 'show me');
  let r = tryPreview();
  assert.equal(r.code, 3);
  for (const want of [/E1 recon/, /E2 ux/, /E2 art-direction/, /E2 motion-media/, /E2 a11y-perf/, /consolidated\.md/, /seasonal\.md/])
    assert.match(r.out, want);
  assert.equal(s.jiu('decide', 'R-001', 'O-01/V1/r1', 'implement', '--quote', 'do it').code, 3);
  assert.match(s.jiu('next', 'R-001').out, /Stage gate .*E1 recon/);
  fs.writeFileSync(ev('recon.md'), '# recon\n');
  fs.writeFileSync(ev('ux.md'), '# ux\n');
  fs.writeFileSync(ev('art-direction.md'), '# art\n');
  fs.writeFileSync(ev('motion-media.md'), '# motion\n');
  fs.writeFileSync(ev('consolidated.md'), '# consolidated\n');
  r = tryPreview();
  assert.equal(r.code, 3);
  assert.doesNotMatch(r.out, /E1 recon|E2 ux/);
  assert.match(r.out, /E2 a11y-perf/);
  assert.match(r.out, /seasonal/);
  // Skipping is explicit, needs a reason, only for E2 roles, and reaches the final report.
  assert.equal(s.jiu('skip-stage', 'R-001', 'e2.a11y-perf').code, 1);
  assert.equal(s.jiu('skip-stage', 'R-001', 'e1.recon', '--reason', 'x').code, 1);
  assert.equal(s.jiu('skip-stage', 'R-001', 'e2.a11y-perf', '--reason', 'static brochure, audited last week').code, 0);
  assert.equal(tryPreview().code, 3);
  fs.writeFileSync(ev('seasonal.md'), '# Seasonal fit\n');
  assert.equal(tryPreview().code, 0);
  assert.match(s.jiu('rulings', 'R-001').out, /\[e2\.a11y-perf\] stage skipped: static brochure/);
  assert.match(s.jiu('show', 'R-001').out, /stage gate: open/);
  assert.doesNotMatch(s.jiu('next', 'R-001').out, /Stage gate/);
  // Evidence removed after the gate was passed shows up in validation.
  fs.rmSync(ev('consolidated.md'));
  assert.match(s.jiu('validate', 'R-001').out, /GATE E2 consolidation/);
});

test('legacy runs keep the seasonal request gate', () => {
  const s = seeded({ gate: false });
  const file = path.join(s.state, 'runs', 'R-001', 'run.json');
  const run = JSON.parse(fs.readFileSync(file, 'utf8'));
  delete run.workflow;
  fs.writeFileSync(file, JSON.stringify(run));
  for (const f of GATE_FILES) fs.writeFileSync(path.join(s.state, 'runs', 'R-001', 'evidence', f), 'x');
  assert.equal(s.jiu('decide', 'R-001', 'O-01/V1/r1', 'preview', '--quote', 'show me').code, 3);
  s.jiu('set', 'R-001', 'seasonal.requested', 'true', '--quote', 'something for Christmas');
  assert.equal(s.jiu('decide', 'R-001', 'O-01/V1/r1', 'preview', '--quote', 'show me').code, 0);
});

test('decisions require an exact revision and a user quote', () => {
  const s = seeded();
  assert.equal(s.jiu('decide', 'R-001', 'O-01/V1', 'preview', '--quote', 'I want to see it').code, 1);
  assert.equal(s.jiu('decide', 'R-001', 'O-01/V1/r1', 'preview').code, 1);
  assert.equal(s.jiu('decide', 'R-001', 'O-01/V1/r1', 'preview', '--quote', 'I want to see it').code, 0);
});

test('approving a mock does not authorize implementation', () => {
  const s = seeded();
  s.jiu('decide', 'R-001', 'O-01/V1/r1', 'preview', '--quote', 'create the mock');
  fs.mkdirSync(path.join(s.state, 'runs', 'R-001', 'previews', 'O-01', 'V1', 'r1'), { recursive: true });
  fs.writeFileSync(path.join(s.state, 'runs', 'R-001', 'previews', 'O-01', 'V1', 'r1', 'index.html'), '<p>mock</p>');
  assert.equal(s.jiu('preview-set', 'R-001', 'O-01/V1/r1', '--path', 'previews/O-01/V1/r1', '--fidelity', 'functional').code, 0);
  assert.equal(s.jiu('work', 'R-001', 'O-01/V1/r1', 'implementing').code, 3);
  assert.equal(s.jiu('task', 'R-001', 'add', '--ref', 'O-01/V1/r1', '--files', 'src/Hero.tsx').code, 3);
});

test('motion suggestions preserve independent triggers, preset overrides, and variation lineage', () => {
  const s = seeded();
  const ref = 'O-01/V1/r1';
  const preview = 'previews/O-01/V1/r1';
  assert.equal(s.jiu('decide', 'R-001', ref, 'preview', '--quote', 'show me').code, 0);
  const dir = path.join(s.state, 'runs', 'R-001', preview);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'index.html'), '<button id="import-status">Import status</button>');
  assert.equal(s.jiu('preview-set', 'R-001', ref, '--path', preview, '--fidelity', 'functional').code, 0);
  const stage = (...args) => s.jiu('motion', 'stage', 'R-001', ref, '--selector', '#import-status', ...args);
  assert.equal(stage('--recipe', 'like-pop@1').code, 0);
  const options = { shape: 'ring', trajectory: 'fall', count: 4, color: '#ba6432' };
  assert.equal(stage('--recipe', 'particles@1', '--trigger', 'failure', '--preset', 'rising-dots',
    '--options', JSON.stringify(options), '--note', 'Alongside Retry').code, 0);
  assert.match(stage('--recipe', 'particles@1', '--options', '{"count":100}').out, /3–16 particles/);
  const ripple = { origin: 'end', intensity: 14, color: '#58a079' };
  assert.equal(stage('--recipe', 'ink-ripple@1', '--preset', 'offset',
    '--options', JSON.stringify(ripple)).code, 0);
  assert.match(stage('--recipe', 'ink-ripple@1', '--preset', 'rising-dots').out, /known preset for this effect/);
  assert.match(stage('--recipe', 'edge-reveal@1', '--options', '{"extent":101}').out, /whole number between 20 and 100/);
  assert.match(stage('--recipe', 'icon-turn@1', '--options', '{"shape":"dot"}').out, /Unknown effect option: shape/);
  assert.match(stage('--recipe', 'reference-pattern@1', '--reference', 'fake').out, /Choose a reference pattern/);
  assert.match(stage('--recipe', 'particles@1', '--trigger', 'custom').out, /Describe the custom trigger/);
  assert.equal(s.jiu('events-ingest', 'R-001').code, 0);
  const feedback = () => rev(s, 'O-01', 'V1', 1).feedback;
  const old = feedback().find((entry) => entry.motion.recipe === 'like-pop@1');
  assert.ok(!Object.hasOwn(old.motion, 'trigger'));
  const ink = feedback().find((entry) => entry.motion.recipe === 'ink-ripple@1');
  assert.equal(ink.motion.preset, 'offset');
  assert.deepEqual(ink.motion.options, ripple);
  const first = feedback().find((entry) => entry.motion.recipe === 'particles@1');
  assert.equal(first.motion.trigger, 'failure');
  assert.equal(first.motion.preset, 'rising-dots');
  assert.deepEqual(first.motion.options, options);
  assert.equal(first.motion.previewHash, rev(s, 'O-01', 'V1', 1).preview.hash);
  assert.equal(first.text, 'Alongside Retry');

  assert.equal(stage('--recipe', 'particles@1', '--trigger', 'hover', '--preset', 'soft-sparks',
    '--variation-of', first.source).code, 0);
  assert.equal(stage('--recipe', 'reference-pattern@1', '--reference', 'action-progress', '--trigger', 'click').code, 0);
  assert.equal(s.jiu('events-ingest', 'R-001').code, 0);
  assert.deepEqual(feedback().find((entry) => entry.source === first.source), first);
  assert.equal(feedback().find((entry) => entry.motion.variationOf === first.source).motion.trigger, 'hover');
  assert.equal(feedback().find((entry) => entry.motion.recipe === 'reference-pattern@1').motion.reference, 'action-progress');
  assert.equal(rev(s, 'O-01', 'V1', 1).decision, 'preview');
});

test('preview requires prior user selection', () => {
  const s = seeded();
  assert.equal(s.jiu('work', 'R-001', 'O-01/V1/r1', 'mock-building').code, 3);
});

test('partial implementation keeps the other opportunities intact', () => {
  const s = seeded();
  s.jiu('decide', 'R-001', 'O-01/V2/r1', 'implement', '--quote', 'implement O-01/V2');
  assert.equal(s.jiu('task', 'R-001', 'add', '--ref', 'O-01/V2/r1', '--files', 'src/Hero.tsx').code, 0);
  assert.equal(s.jiu('task', 'R-001', 'start', 'T-001').code, 0);
  assert.equal(s.jiu('task', 'R-001', 'done', 'T-001', '--evidence', 'diff ok').code, 0);
  assert.equal(rev(s, 'O-01', 'V2', 1).work, 'implemented');
  assert.equal(rev(s, 'O-01', 'V1', 1).decision, 'undecided');
  assert.equal(rev(s, 'O-02', 'V1', 1).work, 'proposed');
  assert.equal(s.jiu('work', 'R-001', 'O-01/V2/r1', 'verified').code, 1);
  assert.equal(s.jiu('work', 'R-001', 'O-01/V2/r1', 'verified', '--evidence', 'screens ok').code, 0);
});

test('only one revision chosen for implementation per opportunity', () => {
  const s = seeded();
  s.jiu('decide', 'R-001', 'O-01/V1/r1', 'implement', '--quote', 'V1');
  assert.equal(s.jiu('decide', 'R-001', 'O-01/V2/r1', 'implement', '--quote', 'V2').code, 3);
  assert.equal(s.jiu('decide', 'R-001', 'O-01/V2/r1', 'implement', '--quote', 'switch to V2', '--replace').code, 0);
  assert.equal(rev(s, 'O-01', 'V1', 1).decision, 'undecided');
});

test('a new revision does not inherit the previous approval', () => {
  const s = seeded();
  s.jiu('decide', 'R-001', 'O-01/V1/r1', 'preview', '--quote', 'mock');
  const r = s.jiu('variant-revise', 'R-001', 'O-01/V1', '--file', s.draft, '--reason', 'less motion');
  assert.match(r.out, /does NOT inherit/);
  assert.equal(rev(s, 'O-01', 'V1', 2).decision, 'undecided');
  assert.equal(rev(s, 'O-01', 'V1', 1).decision, 'preview');
});

test('code change after the mock marks only the affected opportunity', () => {
  const s = seeded();
  s.jiu('decide', 'R-001', 'O-01/V1/r1', 'preview', '--quote', 'mock');
  fs.appendFileSync(path.join(s.code, 'src', 'Hero.tsx'), '// changed\n');
  const r = s.jiu('check-stale', 'R-001');
  assert.equal(r.code, 4);
  assert.equal(rev(s, 'O-01', 'V1', 1).work, 'stale');
  assert.equal(rev(s, 'O-01', 'V1', 1).decision, 'preview');
  assert.equal(rev(s, 'O-02', 'V1', 1).work, 'proposed');
  assert.equal(s.jiu('decide', 'R-001', 'O-01/V1/r1', 'implement', '--quote', 'implement it').code, 3);
  assert.equal(s.jiu('fingerprint', 'R-001', 'O-01', '--revalidated', '--evidence', 'compatible').code, 0);
  assert.equal(rev(s, 'O-01', 'V1', 1).work, 'proposed');
});

test('missing or changed artifact shows up in validation', () => {
  const s = seeded();
  const f = path.join(s.state, 'runs', 'R-001', 'proposals', 'O-01', 'V1', 'r1.md');
  fs.appendFileSync(f, 'silent edit');
  fs.rmSync(path.join(s.state, 'runs', 'R-001', 'proposals', 'O-02', 'V1', 'r1.md'));
  const r = s.jiu('validate', 'R-001');
  assert.equal(r.code, 4);
  assert.match(r.out, /CHANGED proposal O-01\/V1\/r1/);
  assert.match(r.out, /MISSING proposal O-02\/V1\/r1/);
});

test('duplicate paid generation is blocked', () => {
  const s = seeded();
  const spec = ['--spec', 'higgsfield|seedance_2_5|hero loop|16:9', '--estimate', '10', '--ref', 'O-01/V1/r1'];
  assert.equal(s.jiu('media', 'R-001', 'claim', ...spec).code, 3);
  s.jiu('set', 'R-001', 'authorizations.media.authorized', 'true', '--quote', 'can spend up to 50');
  s.jiu('set', 'R-001', 'authorizations.media.limit', '50');
  const first = s.jiu('media', 'R-001', 'claim', ...spec);
  assert.match(first.out, /CLAIMED (\w+)/);
  const key = /CLAIMED (\w+)/.exec(first.out)[1];
  assert.equal(s.jiu('media', 'R-001', 'claim', ...spec).code, 3);
  fs.writeFileSync(path.join(s.state, 'runs', 'R-001', 'assets', 'hero.mp4'), 'x');
  assert.equal(s.jiu('media', 'R-001', 'done', '--key', key, '--path', 'assets/hero.mp4', '--cost', '10').code, 0);
  assert.match(s.jiu('media', 'R-001', 'claim', ...spec).out, /EXISTS/);
});

test('tasks touching the same files do not run in parallel', () => {
  const s = seeded();
  s.jiu('decide', 'R-001', 'O-01/V1/r1', 'implement', '--quote', 'O-01');
  s.jiu('decide', 'R-001', 'O-02/V1/r1', 'implement', '--quote', 'O-02');
  s.jiu('task', 'R-001', 'add', '--ref', 'O-01/V1/r1', '--files', 'src/Hero.tsx', 'src/tokens.css');
  s.jiu('task', 'R-001', 'add', '--ref', 'O-02/V1/r1', '--files', 'src/Cta.tsx', 'src/tokens.css');
  assert.equal(s.jiu('task', 'R-001', 'start', 'T-001').code, 0);
  assert.equal(s.jiu('task', 'R-001', 'start', 'T-002').code, 3);
  assert.match(s.jiu('validate', 'R-001').out, /IN_PROGRESS task T-001/);
});

test('lab events: preferring does not authorize, adjustment is recorded, invalid reference is rejected', () => {
  const s = seeded();
  s.jiu('decide', 'R-001', 'O-01/V1/r1', 'preview', '--quote', 'mock');
  const inbox = path.join(s.state, 'runs', 'R-001', 'events', 'inbox');
  fs.writeFileSync(path.join(inbox, '1.json'), JSON.stringify({ type: 'prefer', ref: 'O-01/V1/r1' }));
  fs.writeFileSync(path.join(inbox, '2.json'), JSON.stringify({ type: 'adjust', ref: 'O-01/V1/r1', text: 'less motion' }));
  fs.writeFileSync(path.join(inbox, '3.json'), JSON.stringify({ type: 'reject', ref: 'O-01/V9/r1' }));
  fs.writeFileSync(path.join(inbox, '4.json'), JSON.stringify({ type: 'reject', ref: 'O-02/V1/r1', text: 'does not fit' }));
  const r = s.jiu('events-ingest', 'R-001');
  assert.match(r.out, /REJECTED 3\.json/);
  const v = rev(s, 'O-01', 'V1', 1);
  assert.equal(v.preferred, true);
  assert.equal(v.decision, 'preview');
  assert.equal(v.feedback.length, 2);
  assert.equal(rev(s, 'O-02', 'V1', 1).decision, 'rejected');
  assert.match(s.jiu('events-ingest', 'R-001').out, /NO_EVENTS/);
});

test('keep original closes the opportunity without touching the others', () => {
  const s = seeded();
  assert.equal(s.jiu('decide', 'R-001', 'O-02', 'keep-original', '--quote', 'keep the original on O-02').code, 0);
  const o2 = opps(s).find((o) => o.id === 'O-02');
  assert.equal(o2.state, 'closed');
  assert.equal(o2.variants[0].revisions[0].decision, 'implement');
  assert.equal(opps(s).find((o) => o.id === 'O-01').state, 'open');
});

test('per-run limits are enforced', () => {
  const s = seeded();
  s.jiu('set', 'R-001', 'limits.maxVariantsPerOpportunity', '2');
  assert.equal(s.jiu('variant-add', 'R-001', 'O-01', '--name', 'Third', '--file', s.draft).code, 3);
  assert.equal(s.jiu('variant-add', 'R-001', 'O-01', '--name', 'Third', '--file', s.draft, '--over-limit').code, 0);
});

test('a full workflow considers seasonal opportunities with a separate limit', () => {
  const s = seeded();
  const add = (...extra) => s.jiu('opp-add', 'R-001', '--kind', 'seasonal', '--title', 'Hat on the logo', ...extra);
  assert.equal(add().code, 1);
  assert.equal(add('--occasion', 'christmas').code, 0);
  assert.equal(opps(s).find((o) => o.kind === 'seasonal').occasion, 'christmas');
  s.jiu('set', 'R-001', 'limits.maxOpportunities', '2');
  assert.equal(add('--occasion', 'halloween').code, 0);
  s.jiu('set', 'R-001', 'limits.maxSeasonalOpportunities', '2');
  assert.equal(add('--occasion', 'carnival').code, 3);
  assert.match(s.jiu('show', 'R-001').out, /\[seasonal: christmas\]/);
});

test('focused workflows are independently resumable and need only in-scope evidence', () => {
  const s = sandbox();
  assert.equal(s.jiu('init', '--code', s.code, '--workflow', 'motion').code, 0);
  assert.equal(s.jiu('init', '--code', s.code, '--workflow', 'seasonal').code, 0);
  assert.equal(s.jiu('init', '--code', s.code, '--workflow', 'seasonal').code, 2);
  assert.equal(s.jiu('opp-add', 'R-001', '--kind', 'seasonal', '--occasion', 'christmas', '--title', 'Seasonal detail').code, 3);
  assert.equal(s.jiu('opp-add', 'R-002', '--kind', 'standard', '--title', 'Unrelated change').code, 3);
  for (const [id, report, extra] of [
    ['R-001', 'motion-media.md', []],
    ['R-002', 'seasonal.md', ['--occasion', 'christmas']],
  ]) {
    assert.equal(s.jiu('opp-add', id, '--title', 'Detail', ...extra).code, 0);
    assert.equal(s.jiu('variant-add', id, 'O-01', '--name', 'Alternative', '--file', s.draft).code, 0);
    for (const file of ['recon.md', 'consolidated.md', report]) {
      fs.writeFileSync(path.join(s.state, 'runs', id, 'evidence', file), '# Evidence\n');
    }
    assert.equal(s.jiu('decide', id, 'O-01/V1/r1', 'implement', '--quote', 'Implement this').code, 0);
    assert.doesNotMatch(s.jiu('next', id).out, /Stage gate|independent critic/);
  }
});

test('batch approval pins exact revisions without requiring mocks or reapproving a chosen item', () => {
  const s = seeded();
  const batch = () => JSON.parse(s.jiu('batch', 'R-001', '--json').out);
  const approve = (token) => s.jiu('approve-all', 'R-001', '--batch', token, '--quote', 'Approve the listed batch');
  assert.equal(s.jiu('recommend', 'R-001', 'O-01/V2/r1').code, 0);
  assert.equal(s.jiu('recommend', 'R-001', 'O-02/V1/r1').code, 0);
  const original = batch();
  assert.deepEqual(original.entries.map(({ ref }) => ref), ['O-01/V2/r1', 'O-02/V1/r1']);
  assert.equal(s.jiu('decide', 'R-001', 'O-02/V1/r1', 'implement', '--quote', 'Approve CTA').code, 0);
  assert.equal(approve(original.token).code, 3);
  assert.equal(s.jiu('variant-revise', 'R-001', 'O-01/V2', '--file', s.draft).code, 0);
  assert.match(batch().blocked.join(' '), /newer revision/);
  assert.equal(s.jiu('recommend', 'R-001', 'O-01/V2/r2').code, 0);
  const revised = batch();
  assert.equal(approve(revised.token).code, 0);
  assert.equal(approve(revised.token).code, 3);
  assert.equal(rev(s, 'O-01', 'V1', 1).decision, 'undecided');
  assert.equal(rev(s, 'O-01', 'V2', 1).decision, 'undecided');
  assert.equal(rev(s, 'O-01', 'V2', 2).decision, 'implement');
  assert.equal(rev(s, 'O-02', 'V1', 1).decision, 'implement');
});

test('controller rulings are recorded and listed in order', () => {
  const s = seeded();
  assert.match(s.jiu('rulings', 'R-001').out, /NO_RULINGS/);
  assert.equal(s.jiu('ruling', 'R-001').code, 1);
  assert.equal(s.jiu('ruling', 'R-001', 'kept existing easing token', '--ref', 'O-01/V1/r1', '--cost', 'one extra review').code, 0);
  assert.equal(s.jiu('ruling', 'R-001', 'minor (deferred): label wraps at 320px', '--ref', 'T-001').code, 0);
  assert.equal(s.jiu('ruling', 'R-001', 'x', '--ref', 'O-09').code, 1);
  const out = s.jiu('rulings', 'R-001').out;
  assert.match(out, /1\. .*\[O-01\/V1\/r1\] kept existing easing token — if wrong: one extra review/);
  assert.match(out, /2\. .*\[T-001\] minor \(deferred\)/);
});

test('without --state, a jproject container keeps the state in its work folder', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'jiu-container-'));
  const work = path.join(root, 'work');
  const product = path.join(root, 'product');
  fs.mkdirSync(work, { recursive: true });
  fs.mkdirSync(path.join(product, 'src'), { recursive: true });
  fs.writeFileSync(path.join(work, 'WORKSPACE.md'), '---\njproject: 1\nproduct_dir: product\nwork_dir: work\n---\n# map\n');
  fs.writeFileSync(path.join(product, 'WORKSPACE.md'), '# Product map\n');
  const env = { ...process.env };
  delete env.JIU_STATE;
  const r = spawnSync(process.execPath, [CLI, 'init', '--code', product], { encoding: 'utf8', cwd: product, env });
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.ok(fs.existsSync(path.join(work, '30-plan', 'juice-it-up', 'runs', 'R-001', 'run.json')));
  assert.ok(!fs.existsSync(path.join(product, '.juice-it-up')));
});

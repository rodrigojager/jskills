# Durable state, resume, and idempotency

State cannot exist only in the chat. Everything another session would need to continue lives in the state directory, written by the `jiu` tool (`scripts/jiu.mjs`), which validates transitions and refuses unauthorized actions.

## Where it lives

- Inside a jproject container (its work folder, usually `_work/`, holds a `WORKSPACE.md` whose front matter has `jproject: 1`; `jproject root --json` prints the paths): `_work/30-plan/juice-it-up/runs/R-NNN/`. Never inside the product repository. Without `--state` or `JIU_STATE`, `jiu` finds the container by walking up from the current directory and uses this folder.
- Outside a container: ask the user where to keep the state before `init`; do not assume the repository. Without a container, `--state` or `JIU_STATE`, `jiu` falls back to `<cwd>/.juice-it-up`.

Always pass `--state <absolute path of the state directory>`. `jiu` does not change `.gitignore`; when the user puts the directory inside a repository, ask whether to ignore it or version it (versioning keeps the history across machines; ignoring keeps the repository clean). Git worktrees are never used by this workflow.

## Layout of a run

```
R-001/
  run.json              identity, target, input mode, workflow, phase, next action, limits, capabilities, authorizations, compositions
  brief.md              requirements, identity, and direction decisions (you edit it)
  opportunities.json    opportunities → variants → revisions, with decision, work state, preview, feedback, fingerprint
  decisions.jsonl       append-only history: decisions (with the user's quote), work, feedback, tasks, config changes
  evidence/             recon.md, ux.md, art-direction.md, motion-media.md, a11y-perf.md, consolidated.md, captures, snapshot HTML
  proposals/
    _drafts/            drafts written by workers (not state)
    O-01/V1/r1.md       recorded revisions (immutable; hash in state)
  previews/
    O-01/V0/r1/         baseline of the original
    O-01/V1/r1/         mock of the revision (index.html + files)
    _composition/C-01/  composition of chosen items
  reviews/              critiques (O-01-V1-r1.critic.md), verifications (O-01-V2-r3.verify.md)
  tasks/tasks.json      implementation tasks: ref, files it may touch, status, agent, evidence
  tasks/T-001.md        brief and report of each task
  assets/               reusable media + media.jsonl (claims and provenance)
  events/inbox/         lab feedback awaiting validation
  events/processed/     events already incorporated (or rejected-*.json)
```

## Identifiers

- Run `R-001`, opportunity `O-01`, variant `V1` (V0 is always "keep original"), revision `r1`, composition `C-01`, task `T-001`.
- Canonical reference: `O-01/V2/r3`. Decisions and work require the revision; `O-01/V2` is refused as ambiguous.
- Recorded revision files are immutable. Did the proposal change? `variant-revise` creates `r+1`. `validate` flags a file changed without a new revision.

## Commands

| Command | Purpose | Refuses when |
|---|---|---|
| `init --url U --code D [--workflow full\|motion\|seasonal] [--title T] [--new]` | creates a run; exit 2 lists an existing run for the same target and workflow | no target or invalid workflow |
| `list [--url] [--code] [--all]` | finds runs | — |
| `show R [--json]` | readable summary or full JSON | — |
| `next R` | next actions derived from the state | — |
| `validate R` | missing/changed artifacts, pending media, tasks in progress (exit 4 if there is a problem) | — |
| `set R key value [--quote]` | phase, nextAction, limits, capabilities, implementation branch, authorizations | authorization without `--quote` |
| `opp-add R --title --surface --location [--paths] [--depends] [--conflicts] [--kind seasonal --occasion o]` | records an opportunity (creates V0) | above the limit without `--over-limit`; kind outside selected workflow; missing occasion for a seasonal opportunity |
| `variant-add R O-01 --name --axis --file draft` | records alternative r1 | above the limit |
| `variant-revise R O-01/V2 --file --reason` | new revision, starts `undecided` | above `reviewRounds` |
| `decide R ref decision --quote "…" [--reason] [--replace] [--external]` | user's decision | no quote, ambiguous ref, stage gate closed (for `preview`/`implement`), `implement` in URL mode, stale item, second `implement` on the same opportunity |
| `recommend R ref` / `batch R [--json]` | records one recommended revision per opportunity and shows the proposed batch including its ID | recommendation for V0, a superseded revision or an inactive opportunity |
| `approve-all R --batch ID --quote "…"` | records implementation decisions for the exact recommended batch | batch changed, blocked or already approved; pending Lab events; no user quote |
| `skip-stage R e2.<role> --reason "…"` | records that an E2 diagnostic in scope was deliberately omitted; listed by `rulings` and `show` | no reason or role outside the selected workflow |
| `work R ref state [--evidence]` | work state | transition outside the rules |
| `preview-set R ref --path --fidelity` | records a ready mock (`mock-ready`) | revision not selected for preview (except V0) |
| `composition-set R C-01 --refs … --path --conflicts` | records a composition | refs not selected |
| `reviewed R ref --file reviews/… [--self]` | records a critique | — |
| `ruling R "<decision and why>" [--ref] [--cost]` / `rulings R` | records a decision taken on the user's behalf; lists all of them (and skipped stages) for the E8 report | empty text, unknown ref |
| `fingerprint R O-01 --paths … [--revalidated]` / `fingerprint R --url-snapshot f` | records the comparison baseline; `--revalidated` returns `stale` items to their previous state | no code |
| `check-stale R [--url-snapshot f]` | marks as `stale` only the affected revisions (exit 4 if there is a change) | — |
| `media R claim/done/fail/list` | idempotent claim for paid generation | no authorization, no budget, pending claim |
| `task R add/start/done/block/list` | implementation tasks | ref not chosen, conflicting files, batch full |
| `motion list --catalog` | lists recipes, presets, options, reference IDs and triggers | plain `motion list` lists recipes only |
| `motion stage R ref --recipe <id@version> --selector <css>` | queues a suggestion for a recorded local mock | optional `--trigger`, `--preset`, `--options`, `--reference`, `--variation-of`, `--duration`, `--note` |
| `events-ingest R` | incorporates lab feedback | invalid event goes to `processed/rejected-*` |
| `serve R [--port 4977]` | local lab server | — |

Exit codes: 0 ok · 1 usage error · 2 existing run · 3 refused by rule, authorization, or limit · 4 problems found.

### Stage gate

`decide … preview` (entering E4), `decide … implement` (E6) and `task add` (E7) exit 3 with the list of what is missing until all of these hold:

- `evidence/recon.md` exists and is not empty (E1);
- for each E2 role in the run, `evidence/<role>.md` exists or `skip-stage R e2.<role> --reason "…"` was recorded. Full uses `ux`, `art-direction`, `motion-media`, `a11y-perf`, and `seasonal`. Motion uses `motion-media`. Seasonal uses `seasonal`;
- `evidence/consolidated.md` exists and is not empty;

Legacy runs without a recorded workflow use the original four reports and the seasonal offer/request flag.

There is no override flag (`--over-limit` does not apply): complete the stage or record the skip with its reason. `next` lists the gaps first, `show` summarizes them, and `validate` reports them as `GATE` problems once a preview or implementation decision exists. Skipped stages appear in `rulings`, so they reach the user in the E8 report.

## Resume

When resuming, in this order, before dispatching anything:

1. **Identify the run.** `list` (with `--url`/`--code` of the target) or the ID given by the user.
2. **Check the state and the current target.** `show R`; read `brief.md`. `check-stale R` (in URL mode, save the current HTML and pass `--url-snapshot`). Items marked `stale` keep their decision; they need revalidation before implementation.
3. **Verify artifacts.** `validate R`. Proposal missing or changed, preview missing, asset missing: handle each one (regenerate from the record, new revision, or mark `blocked` with a reason). Never pretend an artifact exists.
4. **Reconcile effects of earlier attempts.**
   - Tasks `in_progress`: inspect the implementation branch and the uncommitted diff of the task's files. If the work is done, `task done` with evidence; if partial, decide to continue with the same scope; if nothing happened, re-dispatch. Never reimplement on top.
   - Media `claimed` without `done`: check the provider's history; if the asset exists, download it and `media done`; if it failed, `media fail`. Only then claim again.
   - Events in `events/inbox`: `events-ingest`.
5. **Run the next compatible action** consistent with the recorded choices (`next R`) and with the current request. Earlier choices remain valid; do not ask again about what is already decided.
6. **Record completion** (`work`, `task done`, `set nextAction`) before starting other work.

## Staleness

- `opp-add --paths` and `fingerprint` record the hash of the surface's files and the commit.
- `check-stale` recalculates. If it changed: revisions in `proposed`, `mock-building`, `mock-ready`, `review-requested`, or `blocked` become `stale` (with `staleFrom`). Items in implementation or implemented are not marked automatically, because the implementation itself changes those files: check them by hand.
- A HEAD change is reported even without registered files; assess whether it touches surfaces without a fingerprint.
- After verifying compatibility: `fingerprint R O-01 --revalidated --evidence "…"`. If not compatible: a new revision of the variant against the current target.
- After E8, redo `fingerprint` for the implemented opportunities so that future changes are detected from the new state.

## Idempotency

- `init` does not duplicate a run for the same target.
- `variant-add/revise` never overwrites a recorded revision.
- `media claim` uses the full specification (provider, model, prompt, parameters) as the key: `EXISTS` returns the asset already generated; `IN_PROGRESS` requires reconciliation; the budget is checked beforehand.
- `task start` only accepts `pending`/`blocked` tasks; `events-ingest` moves each event once.
- Audits: if `evidence/<role>.md` exists and the surface has not changed, reuse it. Redo only what `check-stale` or the user invalidated.

## Archiving and cleanup

May be shut down: the lab server, dev servers, browser tabs.

Do not delete: `proposals/`, registered `previews/`, `assets/`, `reviews/`, `evidence/`, `decisions.jsonl`. If the user asks for cleanup, show what will be removed and confirm; prefer compacting the run over deleting it.

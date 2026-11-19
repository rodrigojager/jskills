# Product map: jtasks-v2

One row per folder (and relevant subfolder) of this repository. Keep it portable: no absolute paths, disks or local-only folders.

| Path | What lives there |
|---|---|
| `src/cli/` | public validate/import/run/status/stop/result, explicit update checks/install and fixture commands |
| `src/domain/` | explicit plan, attempt/result contracts and dependency validation |
| `src/storage/` | workspace resolution, SQLite schema, durable transitions and receipts |
| `src/controller/` | sole foreground writer and sequential fixture dispatch |
| `src/execution/`, `src/executors/` | argument-safe process launch and deterministic fixture adapter |
| `src/resources/` | checkout-scoped kernel writer lock |
| `src/read/`, `src/transport.rs` | read-only projections and scoped local control protocol |
| `web/` | canonical compact read-only dashboard renderer and generic embedded transport seam |
| `orca-plugin/` | optional Orca worker, theme-aware panel, offline CLI provisioner, verified complete-package updater, independent tests and folder/ZIP builders; excluded from the Rust crate |
| `tests/` | focused public CLI and durable receipt scenarios |
| `skills/source/` | canonical authoring/execution/estimator instructions and schema examples |
| `skills/codex/`, `skills/claude/`, `skills/opencode/` | derived harness instruction packages; no copied runtime core |
| `locales/` | eleven embedded human catalogs and retained MIT translation license |
| `scripts/skills.rs` | local deterministic skill build/parity check; never global installation |
| `scripts/package.ps1`, `scripts/smoke.ps1` | native locked Windows/Linux archive workflow and unpacked public smoke; no publication/install |
| `.github/workflows/` | manual native Windows/Linux packaging definitions; not executed platform evidence |

## Initial public contract

`jtasks validate plan.json`, `import plan.json --run demo`, `run --run demo`,
`status --run demo` and `stop --run demo` are independent Rust commands.
`--root` selects the workspace and `--data-dir` overrides runtime storage.
An existing jproject container defaults to its `jtasks_data` directory plus
`v2/`; a plain checkout uses its Git administrative directory plus `jtasks-v2/`.
Exit 0 means complete/accepted, 3 means unfinished/intervention and 1 means error.
Status never creates or migrates a database. Run stays in the foreground.

Plans explicitly set `schema_version: 1` and `completion: "judgment"`.
Tasks have `id`, `title`, `depends_on` and an `executor`, initially
`{"kind":"fixture","outcome":"completed","summary":"Judgment summary"}`.
Completed/blocked/failed outcomes require a scoped attempt identity, a nonempty
summary and a durable receipt; tests or evidence are optional for judgment.
Plain output, helper JSON and exit success do not designate a result.
`result` accepts a receipt through the live controller; environment variables
`JTASKS_CONTROL_ENDPOINT`, `JTASKS_RESULT_TOKEN`, `JTASKS_ATTEMPT_ID` and
`JTASKS_TASK_ID` are supplied only to the designated executor process.

## Selective reuse

Dependency DFS and container front-matter conventions were reviewed against
legacy jtasks revision `6caa1413da4f5246a06e5b7e22bbc3f10e437b65`.
The small adapted traversal retains its MIT notice in source. The five dirty
OpenCode files were observed and preserved. jlock revision
`a6396f2fa588f026d7b7b8e745b09b13eb8005d9` and jproject revision
`c19c64aeea31d76cd30a0688449f7facb95997f9` were clean when inspected;
no code from those projects is copied into this increment.

## Increment boundaries

This increment supports deterministic direct fixture processes, sequential
scheduling and explicit stop/resume. Accepted results remain separate from
process retirement. An unresolved launch intent or process blocks replacement;
crash/platform reconciliation, real harness adapters, process-tree custody,
parallel capacities and other completion policies arrive in later increments.

## Allowed Markdown files
- `README.md`
- `AGENTS.md` (root, and folders with their own rules, up to the second level)
- `WORKSPACE.md` (root only)
- `orca-plugin/README.md`
- `skills/source/to-jtasks/SKILL.md`
- `skills/source/to-jtasks/references/authoring.md`
- `skills/source/jtasks/SKILL.md`
- `skills/source/jtasks/references/execution.md`
- `skills/codex/to-jtasks/SKILL.md`
- `skills/codex/to-jtasks/references/authoring.md`
- `skills/codex/jtasks/SKILL.md`
- `skills/codex/jtasks/references/execution.md`
- `skills/claude/to-jtasks/SKILL.md`
- `skills/claude/to-jtasks/references/authoring.md`
- `skills/claude/jtasks/SKILL.md`
- `skills/claude/jtasks/references/execution.md`
- `skills/opencode/to-jtasks/SKILL.md`
- `skills/opencode/to-jtasks/references/authoring.md`
- `skills/opencode/jtasks/SKILL.md`
- `skills/opencode/jtasks/references/execution.md`
- `skills/source/estimate-jtasks/SKILL.md`
- `skills/codex/estimate-jtasks/SKILL.md`
- `skills/claude/estimate-jtasks/SKILL.md`
- `skills/opencode/estimate-jtasks/SKILL.md`

The listed skill Markdown is shipped product input. Edit canonical `skills/source/`
only, then compile `scripts/skills.rs` with `rustc --edition 2024` to a workspace
build directory and invoke it as `skills build <product-root>` or
`skills check <product-root>`. The generator does not install skills or delete
unexpected files. Global installation requires separate authorization.

The `estimate-jtasks` skill's optional `scripts/estimate.py` requires Python 3.10+ and a
separately configured harness. These are skill workflow dependencies, not Rust
CLI runtime prerequisites. It uses a fresh session and public validation to fill
only missing active-duration estimates; it never imports or starts a run.

The authoring skill's optional `scripts/compose.py` (Python 3.10+, standard
library only) writes version-1 intent from a short spec and a plan written by
`to-plan`: it copies the plan's own text by pointers, fills group templates,
expands dependency and stage lists and can call the public `validate` and
`operations preview`. `assets/compose.spec.json` is its shipped example, and
`tests/compose_contract.rs` composes it against a fixture plan. It never
imports or starts a run.

AI working artifacts live outside this repository, in the container's `_work/` folder.

Human localization uses `--lang`, `JTASKS_LANG`, `JLOCK_LANG`, system locale, then English. Catalogs selectively adapt applicable legacy glossary/process labels; obsolete scheduling/wake notices are excluded. New help/view labels use per-key English fallback; machine projections and protocol stay English.

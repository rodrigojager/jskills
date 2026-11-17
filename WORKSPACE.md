# Product map: jtasks-v2

One row per folder (and relevant subfolder) of this repository. Keep it portable: no absolute paths, disks or local-only folders.

| Path | What lives there |
|---|---|
| `src/cli/` | public validate/import/run/status/stop/result and fixture commands |
| `src/domain/` | explicit plan, attempt/result contracts and dependency validation |
| `src/storage/` | workspace resolution, SQLite schema, durable transitions and receipts |
| `src/controller/` | sole foreground writer and sequential fixture dispatch |
| `src/execution/`, `src/executors/` | argument-safe process launch and deterministic fixture adapter |
| `src/resources/` | checkout-scoped kernel writer lock |
| `src/read/`, `src/transport.rs` | read-only projections and scoped local control protocol |
| `tests/` | focused public CLI and durable receipt scenarios |

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

AI working artifacts live outside this repository, in the container's `_work/` folder.

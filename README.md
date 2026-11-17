# jtasks-v2

An independent Rust foreground controller with versioned JSON intent, SQLite runtime state, designated task results and managed command receipts.

## Getting started

Validate and preview a plan before importing it, then run its foreground controller:

```text
jtasks validate plan.json
jtasks operations preview plan.json
jtasks import plan.json --run demo
jtasks run --run demo
jtasks status --run demo
jtasks operations status --run demo
```

Every plan explicitly selects `completion` and `operation_placement`. Placement is `none`, `task`, `group`, `final` or `mixed`; `none` requires an empty operation list. Judgment completion needs an attributable designated result and nonempty summary. It does not synthesize tests or builds.

```json
{
  "schema_version": 1,
  "completion": "judgment",
  "operation_placement": "mixed",
  "tasks": [
    {
      "id": "A",
      "title": "Implement the change",
      "executor": {"kind": "fixture", "outcome": "completed", "summary": "Implementation reviewed"}
    }
  ],
  "operations": [
    {
      "id": "verify",
      "stage": {"kind": "final", "tasks": ["A"]},
      "required": true,
      "revision": "source-r1",
      "inputs": ["src/", "tests/"],
      "repair_task": "A",
      "initial": {"program": "cargo", "args": ["test", "--test", "selected_check"], "cwd": "."}
    }
  ]
}
```

Operation stages are `prerequisite` with `consumers`, `task` with one `task`, and `group`/`final` with producer `tasks`. Relative command directories resolve against the checkout. Group/final operations name a `repair_task` responsibility. Explicit `depends_on` edges use `{"kind":"operation","id":"verify"}`, `task_candidate` or `task_completed`. The validator checks their combined ordering with task dependencies and rejects completion/check cycles.

Prerequisites pass before their consumers start. Required task checks follow the designated development result and process retirement, leaving the task `verification_pending` until they pass. Group/final checks follow their declared completed producers. Independent development can continue while checks run. The status projection separates `development_complete` and `verification_complete`; required pending/failed operations prevent run success. Optional failure stays visible. Only each declared initial invocation is dispatched, once through the common managed command service; a rerun preserves its receipt and never invents repair commands.

Runtime schema 2 rejects older development databases without migration. Use a separately selected runtime directory and import explicit intent for a fresh run.

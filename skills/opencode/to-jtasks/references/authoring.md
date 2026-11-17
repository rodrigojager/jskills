# Version-1 intent and public CLI

The independent Rust CLI owns scheduling. Author JSON rather than manipulating SQLite, runtime tickets or Git history. Store intent and generated planning artifacts where the workspace map permits them; in a jproject container, use `_work/30-plan/`, outside `product/`.

## Task intent

Required plan fields: `schema_version: 1`, `completion: "judgment"`, explicit `operation_placement`, and nonempty `tasks`. Optional fields include `requirements`, `context`, `profile_defaults`, `scheduling` and `operations`. Unknown fields are rejected. A task has `id`, `title`, `executor`, optional `depends_on`, `requirements`, `context`, `admission`, `estimate_seconds` and recovery/review settings. Put acceptance statements in `context.requirements`; do not use legacy `criteria`, `files`, `profile` or `dependsOn` fields in newly authored intent.

`context` supports `brief`, `requirements`, `references`, `dependency_artifacts`, `operations`, `configuration`, `limits`, `resources`. Keep the common brief once at plan level, specific facts at task level. Name dependency artifacts explicitly: completing task A does not inject A's conversation into task B. Root requirements use objects with `id` and `text`; each task's `requirements` lists the original IDs it covers.

`admission.resources` accepts explicit `path:src/file.rs`, checkout-local `ports/8080`, or explicitly shared `shared:host/environment:ports/8080`. Reservations are advisory; a resource override records actor/reason and does not retire an old process or permit duplicate attempts. `scheduling.task_capacity` and `scheduling.heavy_capacity` are separate positive capacities; do not inflate them to bypass a wait.

## Profiles and capability choices

Inspect `jtasks profiles --help`, `jtasks profiles list`, `jtasks executors`, `jtasks catalog --help` and `jtasks doctor`. Read-only diagnostics do not run inference or repair global configuration. Discovery refresh is explicit, scoped and separate from runtime account access. Use only supported, observed identities; a known-invalid model/effort/provider must be corrected, while an unknown capability requires an explicit attributable exception, never an invented model ID.

For an existing jtasks alias, use `"executor": {"kind":"profile","alias":"chosen-alias","overrides":{}}`. `profile_defaults`/`overrides` may supply `executor`, `provider`, `model`, `effort`, `variant`, `native_profile`, `native_agent`, `autonomy`, `executable`, `launcher_args`, `prompt`. A project alias replaces the entire user alias; run defaults fill missing fields; task overrides win. Verify effective values and origins through `jtasks profiles show <alias> --resolved` (optional `--overrides <file>`/`--defaults <file>`) and the task's attempt projection rather than assuming native harness defaults. Direct executor shapes are harness-specific; prefer a validated configured alias over guessing their fields. Autonomous execution is the default; restricted operation is explicit (`autonomy: "restricted"` or `run --restricted`). Native permissions are not an OS sandbox.

## Operations

Every operation needs a unique `id`, explicit `stage`, `required`, nonempty `revision`, explicit checkout-relative `inputs`, `initial`, and an optional `repair_task`. Inputs are actual paths without globs or parent traversal. `revision` identifies the authored input revision. Freshness uses explicit revision and `operations inputs-changed` invalidation; the controller does not automatically fingerprint general operation inputs. Only a declared Git checkpoint captures exact checkpoint-scope bytes at admission of the matching initial check or repair recheck, keyed to that command and revision.

`initial` has `program`, `args`, `cwd` (checkout-relative allowed here), optional `script`, `environment_refs`, `resources`, `deadline_ms`. Environment refs map child variable names to controller variable names; never embed secret values. For a real shell, choose its executable/arguments and script explicitly. There is no implicit shell rewriting.

Stages:

- `{"kind":"prerequisite","consumers":["T1"]}`: required setup before consumers start; allowed under any non-`none` placement.
- `{"kind":"task","task":"T1"}`: after T1's designated completion candidate; a required result gates effective T1 completion.
- `{"kind":"group","tasks":["T1","T2"]}`: after named tasks complete; requires an explicit existing `repair_task`.
- `{"kind":"final","tasks":["T1","T2"]}`: final verification after named tasks complete; also requires explicit `repair_task`.

`depends_on` edges are objects: `{"kind":"operation","id":"setup"}`, `{"kind":"task_candidate","id":"T1"}` or `{"kind":"task_completed","id":"T1"}`. Stage edges already imply the described ordering; do not repeat them or create a check that requires its own effective completion. The CLI rejects combined task/operation cycles. Group/final checks do not retroactively gate their producing tasks; required operations still gate run verification. Use `mixed` to combine task/group/final placements. `none` forbids all operations.

The controller selects only the declared initial invocation. A worker chooses follow-up repairs through managed commands, preserving task-lineage allowances; new operation IDs do not reset retry budgets. Use `operations repair-begin --help`, `repair-finish --help` and `inputs-changed --help` for explicit corrective receipts and changed-input invalidation. Arbitrary successful commands cannot silently replace a required original check.

## Explicit Git checkpoints

A group/final operation may declare `"checkpoint":{"paths":["src/exact-file.rs"]}`. Paths are unique exact regular files within its declared inputs, never directories/globs/traversal/Git metadata. Import records a clean scope baseline; pre-existing or staged scope ambiguity blocks attribution. Passing checks and designated completion never commit automatically.

Use `checkpoint review --run <id> --operation <id> --reviewer <actor> --attribution <statement> --message <message>` to read exact hashes and checkout/data/run/operation/revision/HEAD/branch identity. Save its JSON review in the workspace artifact area. Only explicit `checkpoint apply --review <review.json>` applies an unchanged review, after relevant passing checks on the admitted scoped bytes and positive terminal/empty custody proofs, including reviewer snapshot/root even when no process was invoked. Unrelated staged content and working bytes are preserved. The writer/index guards and compare-and-swap protect apply; no branch creation/switching, worktree, stash, push or publication is implied. Git ref/index and SQLite cannot be one atomic transaction: uncertain intent blocks cleanup and requires explicit inspection/reconciliation.

## Reviewable preparation, then authorized execution

```text
jtasks --root <container-or-checkout> validate <plan.json>
jtasks --root <container-or-checkout> operations preview <plan.json>
jtasks --root <container-or-checkout> import <plan.json> --run <id>
jtasks --root <container-or-checkout> run --run <id>
jtasks --root <container-or-checkout> status --run <id>
jtasks --root <container-or-checkout> operations status --run <id>
```

The first two commands prepare and preview. Import creates state; `run` owns a foreground controller until completion/intervention. To continue an imported stopped run, invoke `run --run <id>` again. There is no external `next`/`start` scheduling loop or distributed chat supervisor. Read/status/watch/operations status never dispatch work. Exit 3 means unfinished/intervention; inspect structured status and required operation outcomes before declaring the run finished.

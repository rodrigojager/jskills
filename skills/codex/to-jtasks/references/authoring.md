# Version-1 intent and public CLI

The independent Rust CLI owns scheduling. Author JSON rather than manipulating SQLite, runtime tickets or Git history. Store intent and generated planning artifacts where the workspace map permits them; in a jproject container, use `_work/30-plan/`, outside `product/`.

## Task intent

Required plan fields: `schema_version: 1`, `completion: "judgment"`, explicit `operation_placement`, and nonempty `tasks`. Optional fields include `requirements`, `context`, `profile_defaults`, `scheduling` and `operations`. Unknown fields are rejected. A task has `id`, `title`, `executor`, optional `depends_on`, `requirements`, `context`, `admission`, `estimate_seconds` and recovery/review settings. Put acceptance statements in `context.requirements`; do not use legacy `criteria`, `files`, `profile` or `dependsOn` fields in newly authored intent.

`context` supports `brief`, `requirements`, `references`, `dependency_artifacts`, `operations`, `configuration`, `limits`, `resources`. Keep the common brief once at plan level, specific facts at task level. Name dependency artifacts explicitly: completing task A does not inject A's conversation into task B. Root requirements use objects with `id` and `text`; each task's `requirements` lists the original IDs it covers.

`admission.resources` accepts explicit `path:src/file.rs`, checkout-local `ports/8080`, or explicitly shared `shared:host/environment:ports/8080`. Reservations are advisory; a resource override records actor/reason and does not retire an old process or permit duplicate attempts. `scheduling.task_capacity` and `scheduling.heavy_capacity` are separate positive capacities; do not inflate them to bypass a wait.

## Profiles and capability choices

Inspect `jtasks profiles --help`, `jtasks profiles list`, `jtasks executors`, `jtasks catalog --help` and `jtasks doctor`. Read-only diagnostics do not run inference or repair global configuration. Discovery refresh is explicit, scoped and separate from runtime account access. Use only supported, observed identities; a known-invalid model/effort/provider must be corrected, while an unknown capability requires an explicit attributable exception, never an invented model ID.

For an existing jtasks alias, use `"executor": {"kind":"profile","alias":"chosen-alias","overrides":{}}`. `profile_defaults`/`overrides` may supply `executor`, `provider`, `model`, `effort`, `variant`, `native_profile`, `native_agent`, `autonomy`, `executable`, `launcher_args`, `prompt`. A project alias replaces the entire user alias; run defaults fill missing fields; task overrides win. Verify effective values and origins through `jtasks profiles show <alias> --resolved` (optional `--overrides <file>`/`--defaults <file>`) and the task's attempt projection rather than assuming native harness defaults. Without an alias, a direct executor names the harness as its kind and takes the same optional fields: `{"kind":"claude","model":"<model>","effort":"<effort>"}`, likewise `codex` and `opencode` (`fixture` is for demonstrations only). Autonomous execution is the default; restricted operation is explicit (`autonomy: "restricted"` or `run --restricted`). Native permissions are not an OS sandbox.

When the catalog cannot confirm a chosen provider, model or effort (Claude Code has no metadata-only model list, so its choices are always unknown), validation answers `unknown capabilities require recorded unknown_capabilities actor/reason`. Each such task then carries, beside `executor`, `"unknown_capabilities": {"actor": "<who chose>", "reason": "<why it is accepted>"}`. The actor is the person who chose the model, so ask the user which model and effort each kind of task gets; never record your own guess as their decision. A known-invalid value is never excused this way.

## Operations

Every operation needs a unique `id`, explicit `stage`, `required`, nonempty `revision`, explicit checkout-relative `inputs`, `initial`, and an optional `repair_task`. Inputs are actual paths without globs or parent traversal. `revision` identifies the authored input revision. Freshness uses explicit revision and `operations inputs-changed` invalidation; the controller does not automatically fingerprint general operation inputs. Only a declared Git checkpoint captures exact checkpoint-scope bytes at admission of the matching initial check or repair recheck, keyed to that command and revision.

`initial` has `program`, `args`, `cwd` (checkout-relative allowed here), optional `script`, `environment_refs`, `resources`, `deadline_ms`. Environment refs map child variable names to controller variable names; never embed secret values. For a real shell, choose its executable/arguments and script explicitly. There is no implicit shell rewriting. The program is started directly, so on Windows a script shim (`npm`, `npx`, any `.cmd` or `.ps1`) is not found by its bare name: call the real executable (`node` with the tool's JavaScript entry file) or name the shell (`cmd.exe` with `/d`, `/c` and the command as arguments).

Stages:

- `{"kind":"prerequisite","consumers":["T1"]}`: required setup before consumers start; allowed under any non-`none` placement.
- `{"kind":"task","task":"T1"}`: after T1's designated completion candidate; a required result gates effective T1 completion.
- `{"kind":"group","tasks":["T1","T2"]}`: after named tasks complete; requires an explicit existing `repair_task`.
- `{"kind":"final","tasks":["T1","T2"]}`: final verification after named tasks complete; also requires explicit `repair_task`.

`depends_on` edges are objects: `{"kind":"operation","id":"setup"}`, `{"kind":"task_candidate","id":"T1"}` or `{"kind":"task_completed","id":"T1"}`. Stage edges already imply the described ordering; do not repeat them or create a check that requires its own effective completion. The CLI rejects combined task/operation cycles. Group/final checks do not retroactively gate their producing tasks; required operations still gate run verification. Use `mixed` to combine task/group/final placements. `none` forbids all operations.

A task's own `depends_on` lists task IDs only. To hold tasks back until a check passed, declare the check as a prerequisite whose `consumers` are those tasks and whose `depends_on` names the tasks it must follow (`task_completed`): it then runs between the two groups. Every task works in the same checkout, so a whole-project check (a typecheck, a build) that runs while another task is editing fails for no one's mistake; place such a check where its dependencies and consumers leave nothing else running, and tell the user when that changes the plan's ordering.

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

## The compose spec

`scripts/compose.py <spec.json>` writes the intent the spec describes; `--out <file>` overrides the spec's `output`, `--check` writes nothing and fails when the existing output is not what the spec composes, and `--jtasks <executable> --root <dir>` adds `validate` and `operations preview` with a short summary. A spec is the intent with shortcuts. Unknown keys and unknown placeholders are errors, as are a pointer that finds nothing and a requirement no task covers.

Top-level keys:

- `compose_version: 1`, `plan` (the path of `PLAN.md`, relative to the spec) and `output` (relative to the spec).
- `completion`, `operation_placement`, `scheduling`, `profile_defaults`, `schema_version`: copied as they are. The first two have no default: they are the user's answers.
- `requirements`: in order, `{"use_cases": "UC"}` (every numbered item of the plan's Use cases becomes `UC01`, `UC02`...) and `{"id", "text"}` entries, whose text is a string or a text pointer.
- `context`: the shared context. Without `brief`, it is the plan's title followed by its Context for executors section. `references` start with `PLAN.md` and every child of Parts with its description, as absolute paths (`"auto_references": false` turns that off); what the spec lists comes after.
- `vars`: names for `{var.NAME}`. `executors`: named executor objects a task (or `reviews.observer`) refers to by name. By default each named `claude`, `codex` or `opencode` executor becomes a jtasks alias: compose writes `<intent>.profiles.json` beside the intent (the `jtasks profiles import` format: `executor` is the harness, then provider, model, effort and the other profile fields) and the task and the observer carry `{"kind":"profile","alias":"<name>"}`. Name aliases after what the tasks do (`deep`, `standard`, `docs`), and always define an `observer` alias, used as `reviews.observer`, when tasks have `estimate_seconds`. With `--jtasks` compose imports the file into the project scope before validating and says so; without it, it prints the `jtasks profiles import` command to run first. `"aliases": false` in the spec keeps direct executors and writes no profiles file; `fixture` executors are always direct. An alias needs a `provider`. The user can later change what an alias runs (`jtasks configure`, the dashboard) without recomposing. `unknown_capabilities`: given to every task that has none of its own.
- `lookups`, `groups`, `tasks`, `operations`: below. `expect`: `{"tasks": n, "operations": n, "requirements": n}`, checked against the result. `allow_uncovered`: requirement IDs that deliberately have no task.

Text pointers copy from a plan file (`file` defaults to `PLAN.md`; a section is named by its heading, with or without its number): `{"file": "PLAN-delivery.md", "section": "Acceptance cases"}` is the whole section on one line, and with `"item": "AC-1"` it is the one list item that starts with that number or with those words in bold.

Lookups read plan tables, per task. `{"file": "PLAN-delivery.md"}` finds the one row whose first cell is the task's ID; `section` limits the search, `column` matches another column by its header, `key` is a template other than `{id}`, `"optional": true` accepts no row. With `"many": true` it collects every matching row and offers the `fields` it defines, each `{"each": "<template over the row's columns>", "join": ", ", "require": "<a column that must not be empty>"}`. A row's cells are named by the table's header, by position (`#1`, `#2`...) and `#section` is the heading the row stands under.

Read files hold, for a task, exactly the plan text it needs when that text should not go inside the ticket. Any context line may be `{"read": "<name>", "title": "<title>", "from": [<sources>]}`. The helper writes `<name>.md` in a folder named after the intent (`<intent>.reads/` beside it), with the title as its heading and the sources in order, copied as written in the plan (Markdown, quotes and line breaks intact), and the context line becomes `Read <absolute path> (<title>): it holds only what this task needs from the plan.` A source is `{"file": ..., "section": ...}` (the whole section with its heading), the same with `"item"` (one list item as written), `{"rows": "<lookup>"}` (the header and the rows that lookup found for this task, as a table) or `{"text": "<template>"}`. `name` and `title` are templates, so `"{id}-context"` gives each task its own file, while one fixed name is a file every task that names it shares; one name with two different texts is an error. A source that finds nothing is left out, and an entry left with no source is dropped. `--check` compares the read files too. The helper removes only the read files it wrote earlier that the spec no longer asks for, never anything else in that folder.

Templates are the strings of task titles, task context and admission resources (and of the shared context, without lookups): `{id}`, `{plan_dir}` (absolute, with forward slashes), `{var.NAME}`, `{lookup.Column}` for a single-row lookup and `{lookup.field}` for a many lookup. A context line whose many lookup or optional lookup found nothing is left out. Text copied from the plan is not expanded again. Write `{{` and `}}` for literal braces.

A group is a task template: `title`, `executor`, `requirements`, `use_cases`, `depends_on`, `admission`, `context`, `lookups`, `vars`, `tags` and the remaining task fields. A task names at most one `group` and adds its own values: single values replace the group's, lists (requirements, dependencies, resources, every context list) come after the group's. `use_cases: [3, 7]` covers `UC03` and `UC07`. In a task's `depends_on`, in an operation's `consumers` and `tasks`, and after the colon of a dependency shorthand, `@name` stands for every task of that group or tag. An operation is written as in the intent, and its `depends_on` also accepts `"task_completed:T1"`, `"task_candidate:T1"` and `"operation:setup"`.

What stays yours to write, because no plan file has it: which slices exist and in what order, which requirements each covers, the executor of each, the reservations, the operations with their exact invocations, and the acceptance or limit lines the plan does not state. Copy from the plan everything it states, inline or as a read file; do not retype it, and never send a task to "section N of" a larger file.

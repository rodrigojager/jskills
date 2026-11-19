---
name: to-jtasks
description: Turn an approved plan written by to-plan into version-1 JSON intent for the independent jtasks CLI, with task dependencies, explicit completion and operation placement. Preparation does not authorize execution.
---

# Author a jtasks plan

The input is always a plan written by `to-plan`: its `PLAN.md` with the fixed sections and the child files it lists under Parts. Without one, ask for `to-plan` first; do not slice a conversation or a free-form document.

Slice the requested work into useful deliverables and dependencies first. Preserve original requirements and IDs, concrete acceptance outcomes, named dependency artifacts and scope limits in task context. Use explicit file or named reservations for concurrent work. Do not generate a heavy check for every small edit by habit.

**Nothing in a run waits for a person.** Tasks run unattended, and a worker cannot receive an answer during the run. So:

- Never write a task, limit or acceptance line that asks the user, waits for a confirmation or an approval, or needs someone at the screen. A decision a task would need is decided in its context with the best answer for the user (the plan's recommendation, written as decided); an installation, a replacement or another action the plan needs is authorized there.
- A check that a person would do is done by the task itself through automation (Playwright or another browser driver, mouse and keyboard control of the app, a CLI or API call). When it cannot be automated, the task takes it as approved, records what it could not check in PROGRESS.md, and completes.
- What truly needs the person (a credential they hold, a purchase, a physical device) is ready before the run starts; it is not a task.
- The user reviews the finished work afterwards, in a pair-programming session or by asking for another run with what did and did not work.

For **each future plan**, explicitly ask the user both decisions, even when a previous queue used a particular policy:

1. **Completion:** judgment with a designated summary alone, or judgment plus required checks? Explain which outcomes need a check. The current schema encodes `completion: "judgment"` in both cases; required operation results enforce the additional checks. Do not invent `completion: "checks"`.
2. **Operation placement:** `task`, `group`, `final`, `none`, or `mixed`? Explain the selected builds/tests/containers/packaging and cost, and recommend a proportional schedule. `none` is appropriate only when no operations, including prerequisites, are needed. Preserve necessary compile/environment/setup prerequisites before their consumers; use a non-`none` policy with explicit prerequisite operations when needed.

Do independent slicing while decisions are pending, but do not finalize their policy or execute without the answer. Record the user's choices in the common `context`. A default from this implementation queue or a prior run is never the answer for a new plan.

Read [references/authoring.md](references/authoring.md) for the actual schema, profiles, operation semantics and CLI sequence. [assets/judgment.json](assets/judgment.json) (no operations) and [assets/mixed.json](assets/mixed.json) (prerequisites and scheduled checks) show the composed result. Their fixture executors are deterministic local demonstrations; replace them with deliberately selected real profiles for actual work.

## Compose the intent, do not write it by hand

Write only a short spec and let `scripts/compose.py` produce the JSON intent: it copies the text the plan already holds (use cases, the paragraph for executors, the parts, table rows and list items you point to), repeats what a group of tasks shares, expands dependency and stage lists, checks coverage, and can validate and preview through the CLI in the same call. The spec holds only your decisions: the slicing, dependencies, requirement coverage, executors, reservations, operations and the text no plan file has. Save it beside the intent it composes (in a jproject container, `_work/30-plan/jtasks/<name>.spec.json`) and keep it: it is the source, the JSON is derived, and a later change is an edit of the spec followed by a new compose.

```text
python <skill>/scripts/compose.py <name>.spec.json --jtasks <independent-jtasks-executable> --root <container-or-checkout>
```

### What a task reads

A task starts with a fresh context and pays, in tokens and context window, for everything it opens. Never tell it to read a section, row or line range of a larger file: it may open the whole file to find the part. Give it exactly what it needs, in one of two ways:

- **In the ticket**, when the text is short: a text pointer or a lookup copies it into the task's context.
- **In a read file**, when the text is long, holds quotes or other characters that are awkward inside JSON, would make the intent larger than it should be, or is shared by many tasks: a `{"read": ...}` entry makes the helper write a file beside the intent that holds only that text, copied from the plan, and points the task to it. Tasks that read the same text share one file; a text specific to a task gets a name with `{id}`.

Pointing to a whole file is fine when the task needs all of it (an ADR, a schema, a fixture). Read files are derived like the intent: never edit them, change the plan or the spec and compose again.

Executors are aliases by default: the helper writes `<name>.profiles.json` beside the intent and, with `--jtasks`, imports it into the project scope before validating (otherwise it prints the `jtasks profiles import` command). Name each alias after what its tasks do, and when tasks have estimates always give the plan an `observer` alias for the execution observer (`reviews.observer`). Set `"aliases": false` in the spec only when the user wants direct executors.

The helper needs Python 3.10+ and nothing else; the Rust CLI does not need it. It never imports or runs. The spec format is in the last section of the reference, and [assets/compose.spec.json](assets/compose.spec.json) is a small complete example. Do not write a one-off generator script, and do not hand-write the JSON unless Python is unavailable.

After slicing and before starting a run, invoke the separate `estimate-jtasks` skill in a fresh configured session to fill only missing `estimate_seconds`. Preserve explicit estimates, IDs, requirements, criteria, dependencies and reservations. Report estimates left open for insufficient evidence.

Validate the resulting intent through `jtasks validate <plan.json>`, then show `jtasks operations preview <plan.json>` and the task/dependency summary before execution. Include exact initial invocations, cwd, environment reference names, inputs/revision, required status, stages/consumers, repair responsibility, deadlines and capacities. Fix invalid JSON or unsupported capability choices; do not silently substitute a model or change the selected schedule. Import and run only when execution is already authorized; otherwise hand back the validated intent and preview.

## A run that is already imported

Once the intent is imported, the run holds its own stored plan, and a new compose is not the revision: a recomposed intent carries plan text (shared context, requirement text, finished tasks) the run may have changed since, and a graph proposal refuses such rewrites. Edit the spec and compose again as usual, then let the CLI build the revision from the stored plan with only the changes:

```text
jtasks --root <container-or-checkout> graph revise --run <id> --add-tasks <intent.json> --depends <task>=<id>,<id> --reason "<why>" --dry-run --output <revision.json>
jtasks --root <container-or-checkout> graph revise --run <id> --add-tasks <intent.json> --depends <task>=<id>,<id> --reason "<why>"
```

`--add-tasks` takes the composed intent: tasks the run already has keep their stored version, and only new task IDs are added, with the new requirements they map to declared as scope additions. `--depends` replaces the dependencies of a pending or new task. Show the dry run before applying when the change is not already approved. It reaches a live controller; never hand-edit a whole-plan proposal from a recomposed intent. Details are in the reference.

## Harness packaging

Prepare JSON intent for the independent CLI. Codex's own tools can author files; they do not schedule jtasks workers. A jtasks profile alias is distinct from a native Codex config profile.

This package contains instructions, examples and the optional compose helper; no scheduler or execution code. Canonical source is `skills/source`; `scripts/skills.rs` derives local agent packages. Building/parity checking them grants no global installation permission.

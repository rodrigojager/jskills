---
name: to-jtasks
description: Turn an approved plan into version-1 JSON intent for the independent jtasks CLI, with task dependencies, explicit completion and operation placement. Preparation does not authorize execution.
---

# Author a jtasks plan

Slice the requested work into useful deliverables and dependencies first. Preserve original requirements and IDs, concrete acceptance outcomes, named dependency artifacts and scope limits in task context. Use explicit file or named reservations for concurrent work. Do not generate a heavy check for every small edit by habit.

For **each future plan**, explicitly ask the user both decisions, even when a previous queue used a particular policy:

1. **Completion:** judgment with a designated summary alone, or judgment plus required checks? Explain which outcomes need a check. The current schema encodes `completion: "judgment"` in both cases; required operation results enforce the additional checks. Do not invent `completion: "checks"`.
2. **Operation placement:** `task`, `group`, `final`, `none`, or `mixed`? Explain the selected builds/tests/containers/packaging and cost, and recommend a proportional schedule. `none` is appropriate only when no operations, including prerequisites, are needed. Preserve necessary compile/environment/setup prerequisites before their consumers; use a non-`none` policy with explicit prerequisite operations when needed.

Do independent slicing while decisions are pending, but do not finalize their policy or execute without the answer. Record the user's choices in the common `context`. A default from this implementation queue or a prior run is never the answer for a new plan.

Read [references/authoring.md](references/authoring.md) for the actual schema, profiles, operation semantics and CLI sequence. Adapt [assets/judgment.json](assets/judgment.json) for no operations or [assets/mixed.json](assets/mixed.json) for prerequisites and scheduled checks. Their fixture executors are deterministic local demonstrations; replace them with deliberately selected real profiles for actual work.

Validate through `jtasks validate <plan.json>`, then show `jtasks operations preview <plan.json>` and the task/dependency summary before execution. Include exact initial invocations, cwd, environment reference names, inputs/revision, required status, stages/consumers, repair responsibility, deadlines and capacities. Fix invalid JSON or unsupported capability choices; do not silently substitute a model or change the selected schedule. Import and run only when execution is already authorized; otherwise hand back the validated intent and preview.

Duration estimation is optional and separate. If requested after slicing, invoke a fresh estimator session/skill to fill only missing `estimate_seconds`; preserve explicit estimates, IDs, requirements, criteria, dependencies and reservations. Do not load estimator instructions into the planner by default, claim an unavailable estimator is installed, or run heavy operations to estimate them.

## Harness packaging

Prepare JSON intent for the independent CLI. Claude Code's own tools can author files; no plugin scheduler or copied execution core is required. A jtasks profile alias is distinct from a native Claude agent.

This package contains instructions and examples only. Canonical source is `skills/source`; `scripts/skills.rs` derives local agent packages. Building/parity checking them grants no global installation permission.

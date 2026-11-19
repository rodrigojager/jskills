---
name: jtasks
description: Execute or continue an authorized independent jtasks run, or work inside its controller-assigned attempt using managed commands and designated structured outcomes. Does not author a new plan or grant publication permission.
---

# Work in a CLI-owned run

Use the independent Rust CLI. The foreground controller is the sole scheduler and runtime writer for the checkout. For an already authorized imported run, `jtasks run --run <id>` starts or continues it. Inspect `jtasks status --run <id>` and `jtasks operations status --run <id>` for task and verification results. `jtasks stop --run <id>` asks that controller to retire owned processes. A stopped controller is continued by another explicit `run`, never a chat wake/keepalive loop.

Inside a controller-assigned attempt, read the file at `JTASKS_TICKET_PATH`, applicable workspace rules and the effective profile. Inspect existing edits and reacquired resources before acting. Keep work within the assigned requirements/limits; do not create worktrees, move patches, mutate runtime state, manage a second task attempt, or commit/publish unless that action is explicitly part of the authorized plan. Native helpers are harness-managed, unobserved and not counted by jtasks.

Honor this plan's selected operation schedule. Prefer managed commands for deliberate build/test/container/packaging work and agent-selected repairs; the controller selects only the initial declared operation. Do not turn a small judgment task into a broad test battery or omit an actual prerequisite. Read [references/execution.md](references/execution.md) when starting a command, handling an obstacle or submitting completion.

Choose the recommended in-scope solution to routine obstacles and continue autonomously. Record concise situation/decision/deviation through `jtasks decisions record`; ask only for a real scope, credential or explicit restricted-mode requirement. Retain edits during recovery; retry counters belong to this task lineage. Never start a replacement while old custody is active or uncertain.

Submit an attributable **designated** `completed`, `blocked` or `failed` result with a nonempty summary. A judgment-only task may require no tests or evidence. Prose, arbitrary JSON, helper reports, process exit zero, idleness and a summary file alone never complete it. Required checks and process retirement are separate from result acceptance. End the attempt after submission; the controller owns retirement/recovery.

## Harness packaging

The controller invokes Claude Code directly. Its root result structured_output may submit the adapter-supplied final JSON schema; the scoped result client is also available. Nested helper results have no completion authority. Native helper management belongs to Claude Code.

This package contains instructions only, derived from `skills/source` by `scripts/skills.rs`. No native hooks, copied execution core, plugin supervisor or global duplicate install is needed.

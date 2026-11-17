---
name: estimator
description: Estimate missing active task durations after jtasks plan slicing, in a fresh configured agent session. Preserve explicit estimates and all other plan intent; do not execute the plan or benchmark operations.
---

# Estimate a sliced jtasks plan

Invoke this workflow separately after slicing and the user's completion/operation-placement decisions. Do not load it into the authoring conversation to estimate there. The estimator receives only the sliced JSON intent and a bounded configuration/evidence file, never planner conversation history, a resume ID or an implicit last session. Estimation does not authorize a provider turn, import, run, build or other heavy operation.

Use the selected agent/model/effort and operation policy already chosen for this plan. If no selection or usable evidence is available, report the limitation or leave affected estimates missing. Never invent a model, account access, timing measurement or certainty. `estimate_seconds` measures active work in positive whole seconds, excluding quota cooldown, retry backoff, queue/resource waits and human response time. It is optional review-timing input, not a mandatory deadline. Include the effect of the selected checks/prerequisites without moving or executing them.

## Optional deterministic helper

The standalone Rust CLI needs no Python. This optional skill helper requires Python 3.10+, a separately available chosen harness, and the independent `jtasks` binary. It invokes one fresh external process, checks a structured response and validates both the original and candidate through the public CLI before applying only missing estimates. It never invokes `import`, `run` or operation execution. No dependency is installed automatically.

Prepare a small policy JSON file with exactly these keys:

```json
{"agent":"codex","effective_configuration":{"model":"the-verified-selected-model","effort":"the-selected-supported-effort"},"evidence":["Describe the actual basis, such as comparable active work; identify assumptions."],"limitations":["No measured timing for this configuration."]}
```

Use the actual configured harness (`codex`, `claude`, or `opencode`), not the illustrative values. Include resolved task profile choices in `effective_configuration` when plan aliases alone do not establish them. Do not copy credentials or raw conversation history into this file. Save policy/report artifacts using the workspace routing rules.

Invoke `scripts/estimate.py` with the plan, policy, selected agent program, explicit fresh arguments, response protocol, independent CLI path/root and a scratch candidate path. For example, using a verified Codex configuration:

```text
python <skill>/scripts/estimate.py <plan.json> --policy <policy.json> --agent codex --program <codex-executable> --agent-arg=exec --agent-arg=--ephemeral --agent-arg=--model --agent-arg=<verified-model> --agent-arg=--output-schema --agent-arg=<skill>/assets/output-schema.json --agent-arg=- --protocol json --jtasks <independent-jtasks-executable> --root <workspace> --candidate <workspace-scratch>/estimator-candidate.json --timeout-seconds 120
```

Inspect the installed harness's help before choosing arguments; this example is not a guarantee of installed version support. Codex `json` protocol requires pure final JSON on stdout; omit `--json`, which selects JSONL events. If a harness version emits additional stdout, this protocol rejects it. The example flags were checked through local help; a real native/provider invocation was not tested. A Codex profile is distinct from a jtasks profile alias; Codex `-c` configuration and `-s` sandbox arguments remain available. For Claude Code, choose a fresh noninteractive print invocation with JSON output and its structured schema option, using `--protocol claude-json` (requires `structured_output`). For OpenCode, choose a fresh noninteractive run invocation with JSON event output, using `--protocol opencode-json` (joins only documented `text` parts). The helper supplies the prompt through standard input, so use only a harness invocation that consumes stdin. Unsupported arguments/protocols or a nonzero exit are errors; do not silently switch models, protocols or harnesses. Resume/fork/continue/session arguments and inherited task control/session environment variables are refused or removed.

The process has a finite deadline (1–600 seconds). On expiry the helper requests termination of its process group/tree and fails without modifying intent. Escaped harness-managed helpers are unobserved, so this is not the Rust controller's custody guarantee. Stdout/stderr are buffered in memory; a 1 MiB stdout acceptance limit is checked after exit, not a streaming memory cap. A response must match [assets/output-schema.json](assets/output-schema.json), with one entry per missing/null estimate and none for explicit estimates. Use `active_seconds: null` plus a concrete rationale when evidence is insufficient; it remains missing. Every numeric estimate is finite, positive and representable by public validation. Rationale and limitations are estimates and assumptions, never fabricated benchmark results.

The helper writes the candidate only to the explicitly supplied scratch path, calls public validation, checks the source has not changed during estimation, then updates the canonical plan in place. The structured report is printed to stdout; redirect it to the workspace's artifact area if needed. Review the report and `jtasks operations preview <plan.json>` afterwards. Preserve original requirements, task IDs/order, explicit estimates (reject invalid ones), executor selections, criteria/context, dependencies, reservations, operations and policy. No plan rewrite or schedule start belongs to this workflow. Scratch output remains for the caller's approved cleanup policy.

## Harness packaging

This is a separately invoked estimation workflow. Use the selected harness's documented fresh noninteractive invocation and explicit model/configuration; never resume a planner or task session. The optional Python helper is not part of the standalone Rust CLI runtime.

Canonical source is `skills/source/estimator`; `scripts/skills.rs` builds and checks local Codex, Claude Code and OpenCode packages. Local packaging never installs or changes global skills.

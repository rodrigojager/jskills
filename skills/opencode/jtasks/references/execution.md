# Managed work, results and recovery

## Commands under custody

The run provides the command controller. Standalone command management uses an explicit foreground `jtasks exec serve`; do not start a competing service inside a live run. Run long work (builds, test suites, images) as a managed command, never in the background and never by ending the turn to wait for it: the end of a worker's turn demands its result. Inside a worker, one call starts and waits:

```text
jtasks exec run --request-id <stable-id> -- <program> <args>
jtasks exec run --request-id <stable-id> --shell "<script>"
```

It binds the command to `JTASKS_TASK_ID`/`JTASKS_ATTEMPT_ID` and their run, uses the current directory, sets no deadline unless `--deadline-ms` is given, and waits up to `--wait-seconds` (default 300). While the receipt says `wait_expired: true`, repeat the same `exec run` call (the same request ID waits for the same command; nothing starts twice) or its `next` call (`jtasks exec wait <handle> --wait-seconds <n>`) in the foreground; an ended receipt carries `exit_code`, `stdout_tail` and `stderr_tail`, and `jtasks exec output <handle> --text` pages the rest. The same request ID returns the same command, also to a later attempt of the task: after a retry, ask again with the same ID instead of starting the work again. jtasks refuses background tool calls and waiting tools (monitors, scheduled wake-ups) in its workers through each agent's own hooks or configuration.

`jtasks exec start --request <command.json>` takes the full request. Its JSON receipt gives the stable command handle. Reuse a stable `request_id` only for an identical request after a lost response or a retried attempt of the same task; choose a new request for a deliberate different command.

Command JSON shape (replace identities and cwd with this actual attempt):

```json
{
  "request_id": "T1-check-1",
  "run": "demo",
  "task": "T1",
  "attempt": "actual-attempt-id",
  "operation": null,
  "program": "cargo",
  "args": ["check"],
  "script": null,
  "cwd": "<existing-absolute-checkout-directory>",
  "environment_refs": {},
  "resources": [],
  "deadline_ms": 120000
}
```

Commands bind to the current task+attempt together, or to an explicit operation. Use the actual `JTASKS_TASK_ID`/`JTASKS_ATTEMPT_ID` and named run from the ticket; never guess or forge scope. The cwd must be an existing absolute directory (unlike plan operation cwd). Configure a real shell program/arguments explicitly when using `script`. Environment refs contain names, not values.

```text
jtasks exec status <handle>
jtasks exec output <handle> --stream stdout --offset 0 --limit 65536 --text
jtasks exec output <handle> --stream stderr --offset 0 --limit 65536 --text
jtasks exec wait <handle> --wait-seconds 20
jtasks exec stop <handle> --reason <specific-reason>
```

Status/output/wait read persisted receipts. A wait expiry leaves the command running. Closing the requesting client does not stop custody. Read available output and terminal/cleanup observations before retrying; unknown state is not proof of death. Do not kill by process name/PID or create a duplicate command because a client ended. Use bounded deadlines suited to the operation. Initial operation failure is an obstacle for autonomous correction, not permission to reset lineage budgets or falsely accept a different successful check. See `operations repair-begin --help`, `repair-finish --help`, and `inputs-changed --help` when designated repair/freshness accounting applies.

## Designated result

The scoped client resolves endpoint, token, task and attempt from `JTASKS_*` environment variables. Invoke the executable at `JTASKS_BIN` (PowerShell: `& $env:JTASKS_BIN`; POSIX: `"$JTASKS_BIN"`), passing:

```text
result --receipt <unique-stable-id> --outcome completed --summary "Delivered the requested behavior; observations and limits."
```

Use `blocked` or `failed` when appropriate. Repeating the identical receipt/payload is idempotent; stale scope or conflicting payloads are rejected. Do not print the token or copy control endpoint credentials into records. Codex and Claude may instead submit the exact adapter-supplied schema through their designated root final channel. OpenCode must use the scoped client. Native JSON/tool messages outside those channels have no authority.

Only claim checks actually observed. Judgment completion needs no invented test evidence. A completion candidate may remain awaiting its required operation; run verification may still await group/final results. The controller retires owned processes independently; accepted outcome does not authorize overlapping replacement attempts.

## Obstacles and same-task continuity

For an ordinary in-scope choice, continue and record `jtasks decisions record --request-id <stable-id> --situation <text> --decision <recommended-choice> --deviation <text-or-none>`. Keep claims factual and concise; no secrets/private reasoning. Respect existing approval boundaries and effective restricted mode.

In restricted mode, a real blocked requirement uses `jtasks ask --request-id <stable-id> --question <text> --blocked-requirement <requirement> [--option <choice>]`, then a designated `blocked` result and retirement. A question alone is not a resumable wait. Management answers through `jtasks answer`; explicit bounded retry after retirement continues the same task. Infrastructure/quota waits and work retries are controller-managed; do not reset counters, repeatedly start new attempts, or infer account access from discovery metadata.

The attempt ticket includes compact prior facts from this task only, receipt provenance, known commands, edits/output references, obstacles and remaining work. Inspect current files/resources again; missing facts do not mean a pristine checkout. Preserve the recorded same-task session where supported; never resume a generic last session or mix another task's history. Explicit named dependency inputs remain available.

When useful, write `recovery-facts.json` beside the ticket (the ticket documents its exact schema): `findings`, `decisions`, `obstacles`, `remaining_work`, `artifacts`, and `commands` arrays. A command fact has `command`, `exit_code` (null when unknown), `outcome`, `receipt`. Reference real managed receipts and retained output; worker facts are attributable claims, not substitute results or proof of custody retirement.

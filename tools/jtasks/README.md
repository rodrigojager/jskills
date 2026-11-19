# jtasks-v2

An independent Rust foreground controller with versioned JSON intent, SQLite runtime state, designated task results and managed command receipts.

## Native packaging and prerequisites

The CLI runs as one native executable: Bun, Node, Python, external jlock and
jproject are not core runtime dependencies. Workspace commands require Git on
`PATH`. SQLite is compiled in through `rusqlite`'s bundled feature. Selected native
executors and authored commands remain external programs with their own runtime,
configuration, permissions and account requirements. The estimator below
separately needs Python; it is not part of controller startup.

Build with Cargo and a native C toolchain (MSVC/Windows SDK on Windows, a C
compiler/linker on Linux). The declared Rust minimum is 1.89; local packaging
verification uses Rust/Cargo 1.99.1 and does not prove the minimum works.
PowerShell 7 and native archive tools are build/package workflow prerequisites,
not CLI runtime dependencies. Choose output paths outside this checkout:

```powershell
pwsh -File scripts/package.ps1 -TargetDir ../_work/00-tmp/build -OutputDir ../_work/00-tmp/package
```

The script builds `--locked --release`, statically links the Windows C runtime,
creates a native ZIP (Windows) or tar.gz (Linux), its SHA-256 file, dependency
licenses/versions, build metadata and a local Cargo `.crate` source archive.
It also emits a raw native update asset and its SHA-256 sidecar.
The binary archive includes local skill packages; using them does not install
globals. Existing output packages are refused. Cargo source packaging uses
`--no-verify` because the optimized binary was built separately; it does not claim
an extracted-source rebuild. No script publishes or installs anything.

Unpack the actual archive and invoke `scripts/smoke.ps1` with `-Binary`, `-Root`
(an existing Git checkout) and a new `-FixtureDir` outside the checkout. Its PATH
contains Git and essential OS tools only: it rejects Bun, Node, Python, native
harnesses, jlock and jproject before validating/importing/completing two dependent
fixture tasks, rerunning without another attempt, and reading status/watch/doctor.
This is public native execution evidence, not a provider/account smoke.

[The package workflow](../../.github/workflows/jtasks-package.yml) defines a manual Windows/Linux native packaging
matrix. A workflow definition is not an executed Linux build or hosted CI result.
Local Windows evidence, missing Linux execution and missing real-account and
external jlock/jproject consumer checks must be distinguished before claiming
release readiness. Rust target libraries alone do not supply a Linux C linker or
runtime. Publication, installation and ecosystem consumer changes require separate
authorization.

## Updates

```text
jtasks update --check --repository owner/repo
jtasks update --repository owner/repo
```

Official packages from jskills come configured for the jskills release repository.
From an older installation, pass `--repository rodrigojager/jskills` on the first
update. An existing `JTASKS_UPDATE_REPOSITORY` environment override takes
precedence over the embedded source. Update or clear an override that points to
the old repository. If installed with Cargo, either configure the source
explicitly or use the release package for a preconfigured installation.

For alternate builds, set the repository explicitly above or through
`JTASKS_UPDATE_REPOSITORY` at runtime/build time. No repository is assumed.
Checks read stable GitHub release metadata without downloading an executable or
writing project/runtime files. Installation is explicit: verify the SHA-256
sidecar and candidate executable's version, then replace this installation.
There is no downgrade or automatic background replacement. Existing processes
continue using their current version; the next launch uses the updated binary.

Publish `jtasks-<version>-<rust-target>.exe` (Windows MSVC x64) or `.bin` (Linux
GNU x64), with an identically named `.sha256` sidecar, in the configured stable
release. The package workflow builds and tests these assets. Publish jtasks releases as stable tags `vMAJOR.MINOR.PATCH` and mark the current one as Latest so existing updaters can find it. Publish releases for other components without making them Latest.
Other platforms have no self-update distribution convention yet. Checks need
HTTPS access to the repository; installation also needs write access beside the
executable. Package-managed installations can set sibling
`jtasks-installation.json` to `{"self_update":false,"reason":"Use the package updater"}`.
The optional Orca plugin sets this policy for its managed CLI and updates the
complete plugin/CLI bundle from its panel. Reused PATH installations remain
independent. See [the optional Orca package](orca-plugin/README.md).

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

Runtime schema 3 rejects older development databases without migration. Use a separately selected runtime directory and import explicit intent for a fresh run.

## Execution and pure monitoring

`run` is the sole foreground scheduler for an actual checkout. To resume a stopped imported run, use `run --run <id>` again. `stop --run <id>` explicitly asks the live controller to retire owned work. Closing a terminal running the controller differs from closing a separate monitor. There is no external `next`/`start` worker queue, chat wake protocol or generated harness execution core.

`status`, `read`, `watch`, `runs`, `tasks`, `task`, `attempt`, `commands`, `logs`, `activity`, `questions`, `operations status` and the controller's local read-only dashboard consume persisted facts. They never dispatch, repair, release resources or refresh discovery. Closing a watcher or browser tab leaves execution running. Read projections report their snapshot/cursor, omissions and capture gaps; they do not establish current process/helper liveness. Use explicit pagination for detail and output. Native helpers are unobserved, never presented as a known count.

The controller prints its loopback dashboard URL when it starts. One compact panel integrates project progress with the complete task list: a progress bar, percentage and completed/total count, checkmarks, running spinners and muted pending tasks. Search and state filters cover every task page. Progress excludes superseded tasks; their historical rows remain visible. Graph revision changes interrupt collection rather than presenting a partial list as complete.

Task rows expand inline as keyboard-accessible accordions, showing the brief, recorded outcome, durations, total attempts, dependencies and links to retained output/history. Escape collapses the row and returns focus to its summary. There is no secondary details sidebar or Full view. The panel fits narrow plugin sidebars and supports reduced motion.

Total elapsed appears to the right of the completed/running/not-started counters and measures wall time since the first persisted launch intent, including pauses and controller downtime. Parallel task durations are never summed into this value. Legacy completed runs show a **recorded project span**, ending at the last journal event: the schema has no exact completion timestamp. Missing execution starts remain unavailable. Task durations retain their separate checkpoint semantics.

**Tasks** (with a total-count badge), **Progress logs** and **Errors** share one keyboard-accessible tab row. The project heading and progress stay visible when switching tabs; returning to Tasks preserves its filters and expanded rows. Progress logs and Errors show project-wide recent events, independent of task selection. Pages are newest first, bounded to 15 records in the UI, with explicit Latest/Older controls and no automatic log polling. Errors cover recorded activity errors and failed/intervention task retirements; raw stderr and unjournaled command failures are not classified. `jtasks read progress --run demo` and `jtasks read errors --run demo` expose the same read-only projections with exclusive before-sequence cursors. Retained raw output remains explicitly paged in 4096-byte chunks. Native chat transcripts and current process liveness are unavailable.

```text
jtasks watch --run demo
jtasks watch --run demo --once --json
jtasks read --help
jtasks logs --help
jtasks exec --help
```

The common managed command service exposes start/status/output/wait/stop handles for initial checks and worker-selected follow-ups. Accepted designated outcomes and actual process retirement are separate. Exit 0 means accepted/complete according to the command; 3 means unfinished/intervention (including pending verification); 1 means error. `--help` documents each subcommand's arguments.

## Optional Orca panel

The optional [Orca plugin](orca-plugin/README.md) lives entirely in `orca-plugin/`.
Its separate distribution includes the native executable and automatically
prepares a compatible CLI when absent. It registers a real theme-aware Orca
sidebar panel scoped to the active local workspace/run, sharing this dashboard's
renderer through the worker bridge. The Rust CLI has no Orca or Node dependency;
the plugin folder is excluded from the Cargo source package.

## Operations and explicit input invalidation


Choose completion and task/group/final/none/mixed operation placement for each plan; this implementation queue's policy is not a default for future plans. Prerequisites can run before dependent work under any non-`none` placement. Task checks gate effective task completion; group/final checks gate run verification after the named producers complete. Separate task and heavy capacities permit independent development while checks run.

The controller selects only authored initial checks. The worker chooses repairs through the same managed service and explicitly reports matching original rechecks with `operations repair-begin` / `repair-finish`; arbitrary successful commands do not replace a required check. Task-lineage retry allowances persist across attempts, graph changes, operation IDs and controller restarts.

General operation freshness uses the authored `revision` and explicit `operations inputs-changed` receipts. General input paths are not automatically fingerprinted. Declared checkpoint scopes alone capture exact bytes at admission of their matching initial check or repair recheck, bound to command and revision.

## Explicit Git checkpoints

Only group/final operations can declare an optional checkpoint, for example `"checkpoint":{"paths":["src/exact-file.rs"]}`. Paths must be unique exact regular files within that operation's inputs. Directory/glob/traversal/Git metadata, linked/reparse/sparse/assume-unchanged scope and ambiguous pre-existing/staged edits are rejected. Import records the clean scope baseline.

```text
jtasks checkpoint review --run demo --operation verify --reviewer reviewer --attribution "Reviewed the declared changes" --message "Implement the scoped change"
jtasks checkpoint apply --review <review.json>
```

Save the review JSON in the workspace artifact area. Review reads exact hashes and checkout/data/run/operation/revision/HEAD/branch identity; it changes nothing. Apply is explicit and requires an unchanged review, attributable clean baseline, relevant passing checks on the admitted scoped bytes, and positive terminal process/empty custody proofs, including reviewer root/snapshot even for zero invocations. Merely passing a check or completing a task never commits.

Apply holds writer/index guards, uses an isolated index and compare-and-swap of the existing branch, preserving unrelated staged entries and working bytes. It never creates/switches branches or worktrees, stashes, pushes, merges or publishes. Git ref/index and SQLite updates cannot be one atomic transaction: interrupted intent stays uncertain, blocks cleanup and requires explicit inspection/reconciliation. Repeating apply never silently restages an uncertain operation.

## Configuration, autonomy and resources

`profiles`, `executors`, `catalog` and `doctor` distinguish jtasks aliases, native settings, cached capabilities and actual account access. Inspection and validation read the cache without launching an agent. Known-invalid choices are rejected; unknown choices require an attributable exception. No silent model/provider fallback is implied.

The Agents editor and `jtasks configure --choices <agent>` reuse native catalog results for 30 minutes. The first request after expiry queries the native catalog again, picking up changes to available providers and models. Failed probes are cached for two minutes. Use **Refresh catalog** in the Agents editor on the dashboard or in Orca, or run `jtasks configure --choices opencode --refresh` (also accepts `codex` and `claude`) to check immediately. These checks do not start a model turn. An explicitly imported snapshot is kept until it is replaced through an explicit refresh or import.

When a task stops on a native quota error with usable reset information, the controller can retry after the indicated deadline under the run's infrastructure retry policy. Claude Code uses its structured reset event, Codex uses the time in its native usage-limit error, and OpenCode uses the delay in its terminal provider error. A quota stop without usable timing still requires intervention.

Tasks and execution observers use the same selection fields. For example, use
`{"agent":"opencode","provider":"company-ai","model":"vendor/model","variant":"priority"}`
as a task's `executor` or as `reviews.observer`. Use `effort` for the common
reasoning choice, or `variant` for OpenCode. The adapters translate that
choice into the harness's native setting:

| Agent | Native reasoning setting |
| --- | --- |
| Codex | `reasoning_effort`, passed as `-c model_reasoning_effort=…` |
| Claude Code | `effort`, passed as `--effort …` |
| OpenCode | `variant`, appended as `--model provider/model#variant` |

Codex and Claude Code can use their configured native provider defaults.
OpenCode accepts separate provider and model IDs or a combined `provider/model`.
Custom providers use the corresponding harness configuration. The selected model
must support the requested effort or variant. Existing native `kind` objects
remain readable.

Set a reusable worker or observer alias through the dashboard, the Orca panel
or the CLI:

```text
jtasks configure --set observer --agent opencode --provider company-ai --model vendor/model --variant priority
```

Autonomous execution is the default. Opt in to restricted permissions with `run --restricted` or profile `autonomy: "restricted"`; `ask`, `questions`, `answer` and bounded `retry` support that workflow. Native permission modes are not OS sandboxes. Advisory resource release/transfer requires actor/reason, even for active/unknown owners; it never terminates the old process or authorizes concurrent attempts of one task. Inspect `resources --help` and `retry --help`.

## Plan authoring, duration estimation and local skills

Canonical `skills/source/` produces separate Codex, Claude Code and OpenCode instruction packages through `scripts/skills.rs`. They consume this CLI; there is no bundled scheduler per harness. Build/check packages locally as described in `WORKSPACE.md`; global installation is separate.

Write the plan with `to-plan`, slice the approved plan with `to-jtasks`,
and invoke `estimate-jtasks` in a fresh configured session before starting
the run. The estimator uses the plan's selected configuration and evidence.
Its Python helper requires Python 3.10+, the independent CLI and an available
configured harness. The Rust CLI needs neither Python nor Bun/Node. The helper
validates original and candidate intent, preserves explicit durations and every
other field, fills only missing estimates, and keeps insufficient evidence
missing. It does not import, start a run, benchmark checks or authorize a
provider call implicitly.

The helper buffers stdout/stderr; its 1 MiB stdout acceptance limit is checked after exit and is not a streaming memory cap. Its finite deadline requests process group/tree termination; escaped harness-managed helpers remain unobserved. This is distinct from controller custody. Actual provider/account support and timing accuracy require separate evidence; local protocol substitutes establish neither.

## Historical cleanup

```text
jtasks clean --run old --json
jtasks clean --approve <reviewed-inventory.json> --yes
```

The first command previews an exact versioned inventory without mutation. Review/save it in the artifact area before explicitly applying the unchanged inventory. Incomplete inactive history additionally requires `--discard-resume`. `--previous --finished-only` selects completed runs for consumers such as jproject. Active or uncertain custody, changed inventory and uncertain checkpoint intent block removal. The shared SQLite database/sidecars and advisory registry are retained; cleanup does not reconcile custody, release reservations or delete source. No cleanup is automatic.

## Human language and stable machine output

Human help, principal terminal labels and dashboard navigation support English, Brazilian Portuguese, Spanish, French, German, Italian, Simplified Chinese, Japanese, Russian, Arabic and Hindi. Detailed new descriptions use English fallback where no translation exists. Selection is `--lang` > `JTASKS_LANG` > `JLOCK_LANG` > system locale > English. System locale uses nonempty `LC_ALL`, `LC_MESSAGES`, `LANG`, then the Windows user locale where available. Regional forms such as `pt_BR.UTF-8`, `es-MX` and `zh-TW` resolve to the matching catalog's primary language. An unsupported explicit `--lang` warns on stderr and forces English; unsupported environment values fall through. Missing keys fall back individually to English.

```text
jtasks --lang pt-BR --help
jtasks --lang ja watch --run demo --once
jtasks --lang fr status --run demo --json
```

Command/option names, JSON field names, identifiers, state/reason codes, stored data and executor protocol stay English. Human summaries/raw output retain their original text. Locale selection adds no prose to machine stdout. The dashboard presentation dictionary is a separate read-only resource, outside the versioned runtime projection. Applicable glossary/process translations are selectively adapted from legacy jtasks revision `6caa1413da4f5246a06e5b7e22bbc3f10e437b65` (MIT notice in `locales/LICENSE`); obsolete supervisor/wake/keepalive notices are excluded.

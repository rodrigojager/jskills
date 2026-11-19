# jtasks for Orca

Optional native Orca sidebar panel, separate from the Rust CLI. Requires Orca
1.4.261 or newer and Git on PATH for native checkout resolution. The verified distribution is Windows x64; remote/WSL workspaces
are intentionally unavailable rather than mapped onto the local host.

The panel shares the approved web renderer: integrated progress and complete task
list, percentage and completed/total counts, recorded wall elapsed, inline task
details, project progress logs and recorded errors. It follows Orca's live theme
tokens and leaves Orca's own chat in place. There is no external browser or local
HTTP server. A sidebar icon opens the panel; the status-bar item and
`jtasks: Open project tasks` command also open it through Orca's CLI.

## Complete installation

Load the **built package folder**, not this source folder:

```powershell
orca plugin install C:\path\to\jtasks-orca-win32-x64 --json
# Review the requested access, then use the fingerprint returned by Orca:
orca plugin install C:\path\to\jtasks-orca-win32-x64 --approve <fingerprint>
orca plugin open local.jtasks --panel tasks
```

Orca requires approval because this plugin uses its trusted Node worker, reads
workspace paths, stores selected runs/settings, and adjusts new agent launch
environments. No agent is launched by this plugin. On activation it reuses a
compatible native CLI from PATH or prepares the hash-verified bundled executable
under `%LOCALAPPDATA%\jtasks\orca\<version>-<hash>\bin\`. Initial preparation is
offline and needs no separate CLI installer. An optional executable setting takes precedence;
incompatible overrides fail explicitly. Existing binaries are never overwritten.

When preparation finishes, the CLI directory is added to **new local agent**
launch environments (disable `agentPath` in plugin settings to opt out). Existing
terminals and global/user PATH are unchanged. The panel's Snapshot & CLI details
shows the executable's absolute path, version and source. Refresh retries failed
preparation. Settings changes recheck the runtime.

The active Orca workspace selects the checkout; runs come from the native CLI's
public read contract. If several runs exist, choose one explicitly. Selection is
remembered per workspace and canonical root. Context switches clear old tasks
immediately and reject in-flight responses from the previous context. A missing
database is shown as an empty project and is never created by the panel. Import
and run tasks using the CLI normally. Remote projects show an explicit message.

All reads are bounded, argument-safe CLI invocations. Overview refreshes every
five seconds only while visible and idle; open accordions and log tabs pause it.
Logs/errors load on demand and never pretend to be a complete chat transcript.
Elapsed uses native persisted wall-time metadata, not summed parallel work.
Task states are recorded state, not a process-liveness probe.

In the **Agents** editor, worker and observer aliases each have their own
agent, provider, model and reasoning settings. Select Codex, Claude Code
or OpenCode. The OpenCode choice appears as **Variant** and accepts names
from the selected provider and model, including those configured in OpenCode
outside its built-in catalog. Use **Refresh catalog** in the Agents editor to fetch the selected agent's current model list without discarding your edits. Ordinary requests reuse the native catalog for up to 30 minutes.

## Updates

The panel's **Updates** control checks the plugin and CLI together. Official
jskills packages preconfigure `rodrigojager/jskills`. To migrate an existing
local package, set **Trusted update repository** to `rodrigojager/jskills`, select
**Check updates**, then select **Update** and review any Orca approval.

For custom builds, configure
`updateRepository` as the trusted GitHub `owner/repo` in plugin settings. Its
default is empty, so unconfigured packages make no release requests. Publishers
can embed this default when building. Automatic checks run on panel opening,
at most once per day; disable them with `checkUpdates: false`. **Check updates**
always revalidates manually. Checks never install anything.

**Update** downloads a newer stable Windows x64 bundle, verifies its SHA-256,
extracts it into a unique managed staging directory and validates matching
plugin/CLI versions. Orca applies the package through its own CLI and may reload
the panel. If Orca requests approval, the exact host response is displayed for
review; the plugin never approves expanded access automatically. Published plugin
keys use Orca's own `plugin update` to preserve their registered source identity.
Local packages use the verified staged folder with `plugin install`.

The managed CLI is part of this bundle: direct `jtasks update` installation is
blocked there, with instructions to use this panel. `jtasks update --check`
remains available. Independent PATH/configured installations are not replaced by
plugin updates. New bundle versions get a new managed binary directory; running
controllers and existing terminals keep their current executable. Staged updates
are retained for host approval/diagnosis. No global PATH is changed.

Publish `jtasks-orca-<version>-win32-x64.zip` and its `.sha256` sidecar in the
configured stable release; the ZIP must contain the complete `package/` root.
These releases must be published before online updates can succeed. No release
repository or published artifact is inferred from a source checkout.

## Build and verify

No npm dependencies. Node 22+ is needed for development/packaging; Orca supplies
the worker's Node runtime. From this folder, with absolute output paths outside
the product checkout:

```powershell
pwsh -File ../scripts/package.ps1 -TargetDir <build-dir> -OutputDir <native-artifacts>
node scripts/package.mjs <native-package-folder> <artifact-dir>/package win32 x64 local.jtasks owner/repo
pwsh -File scripts/archive.ps1 -PackageDir <artifact-dir>/package -OutputDir <artifact-dir>
node --test test/*.test.mjs
orca plugin validate <new-plugin-folder> --json
orca plugin check <new-plugin-folder> --json
```

Tests use `JTASKS_PLUGIN_TEST_DIR` or the container's `_work/00-tmp/orca-tests`.
Set `JTASKS_PLUGIN_TEST_EXE` to a current native executable to include real CLI
provisioning/read tests. Test fixtures do not use the user's installed runtime.
The builder requires matching CLI/plugin versions, includes native dependency
notices and SHA-256 metadata, and refuses to replace an existing output folder.
The sixth argument sets the trusted update repository (or use
`JTASKS_UPDATE_REPOSITORY`); omit it for an unconfigured offline package.
The archive script also refuses existing outputs. Package on the target platform. For a
published owner, pass the installed `<publisher>.jtasks` key as the fifth builder
argument; the local default is `local.jtasks`.

`src/` contains the worker, read bridge and runtime provisioner. `panel/` contains
only Orca integration and token overrides. Packaging copies the canonical
`../web/` renderer into `panel/shared/`; do not maintain a second UI copy here.
The Rust crate excludes this folder and has no Orca dependency. CLI-only users
do not need Node or this plugin.

Offline validation and stand-in browser tests do not establish live Orca loading.
Load approval, actual host panel placement and new-agent PATH acceptance are
separate from package validation. The plugin does not expose a custom maximize
button: sizing belongs to the host sidebar.

For the full browser/bridge acceptance test, Python and Playwright are development
dependencies only:

```powershell
python test/panel.py <absolute-plugin-folder> <absolute-work-fixture-dir> <absolute-screenshot-dir>
```

# AI workspace framework

A way to organize projects built with AI agents (Claude Code, Codex, OpenCode) so the versioned code stays clean, and a small CLI, `jproject`, that creates and lints that layout.

Agents fill project folders with plans, ADR drafts, HTML renders, mocks, handoffs, logs, helper scripts, tool data and `_v2`/`_final` copies. Here, what is really the project lives in one clean git repository, and everything else lives in folders that say what they are for and when they can be deleted.

## The container

```
my-game/                     the container: open your agent here (any disk, any path)
├── AGENTS.md                entry rules for every agent (CLAUDE.md only imports it)
├── .agents/skills/          project skills (.claude/skills is a junction to it)
├── product/                 the ONLY git repository: build, run, test, ship
├── _secrets/                real .env, keys, certificates (outside git)
└── _work/                   everything agents produce, not versioned
    ├── WORKSPACE.md         the map: folders, routing table, storage, active items
    ├── 00-tmp/              ephemeral: delete any time
    ├── 10-docs/             doc drafts, BACKLOG.md, GLOSSARY.md, adr/, _render/
    ├── 20-design/           mocks-img/, mock-html/, previews/ (+ _variants/)
    ├── 25-source-assets/    .blend, .psd... (good candidate for another disk)
    ├── 30-plan/             PLAN.md, PROGRESS.md, DIVERGENCES.md, jtasks/
    ├── 40-handoff/          <workstream>.md, overwritten at each handoff
    ├── 50-tools/            scripts agents created
    ├── 60-intake/           accepted material from outside, to integrate
    ├── 70-proposals/        generated here, awaiting a yes/no
    └── 80-ideas/            IDEAS.md
```

Every folder has an `AGENTS.md` with the same five sections (purpose, what goes here, what does not, conventions, when to delete). The folder names `product`, `_work` and `_secrets` are configurable.

Core rules: one canonical file per artifact, edited in place; alternatives only in `_variants/` until the user consolidates; rejected proposals are deleted without a trace; agents may read `_secrets/` but never copy values; nothing outside the container is touched without asking.

## Install

You need [Rust](https://rustup.rs).

```
cargo install --path cli
jproject setup
```

`setup` asks each value with an explanation and saves `config.toml` in the OS config folder (`%APPDATA%\jproject` on Windows, `~/.config/jproject` on Linux, `~/Library/Application Support/jproject` on macOS). `JPROJECT_CONFIG_DIR` overrides that folder.

### Optional companions

- **zvec-grep**: a semantic search index for agents. `jproject init` creates the index (excluding secrets, `00-tmp`, dependencies and build output) when it is installed.
- **jtasks**: task runs with dependencies and file ownership for agents. In a container it keeps run data in `_work/30-plan/jtasks`, never in the repository.

```
jproject tools status
jproject tools install zvec-grep
jproject tools install jtasks
jproject tools update jtasks
jproject tools uninstall zvec-grep
```

Each of these shows the exact commands it will run and asks first (`--dry-run` only shows them).

## Starting a different plan

`10-docs/BACKLOG.md` survives every plan: keep unresolved items and necessary shared context, not completed progress or whole plans.
Before replacing a plan, review its progress and run results, and consolidate unresolved work there. Read `80-ideas/IDEAS.md` too.
List the open backlog items and ideas, explain what each means, the consequences of including or deferring it, and a recommendation.
Ask the user which to include and wait for the answer. Deferring an item preserves it; selection alone does not complete backlog work.

Then run `jproject plan reset --dry-run`. Obtain approval for the exact list and the permanent removal of finished run data,
then run `jproject plan reset --pending-reviewed --approve <token>`. Any content change invalidates the token.
The command recycles old files in `30-plan/` except `AGENTS.md` and removes finished runs through `jtasks clean`. The native jtasks v2 preview is bound to the approval; incomplete, blocked or legacy runs stop the reset. The shared v2 database is retained.
It preserves the backlog, ideas, ADRs, glossary and other work folders. Active/paused runs, unknown data and links block the reset.
It creates no archive. Only after it succeeds, write the new plan, progress and divergences. Refining the same plan needs no reset.
On existing containers, `jproject init --no-index` adds the missing backlog template without overwriting documents.

## Commands

| Command | What it does |
|---|---|
| `jproject setup` | First-time questions; run again to change values. |
| `jproject init [path] [--name] [--no-git] [--no-index]` | Creates the container; in an existing one only creates what is missing. |
| `jproject init --parallel [path] --origin <container>` | A parallel folder that reads the origin read-only and delivers through `out/` + `HANDOFF.md`. |
| `jproject root [--json]` | Container, product, work, secrets and jtasks data paths (works from inside `product/`). |
| `jproject check [--product-only] [--json]` | The lint. Exit code 1 on errors. |
| `jproject new intake\|proposal\|handoff <name>` | Creates the item from its template and updates Active items. |
| `jproject status` | Open items, folder sizes, physical location of each folder. |
| `jproject clean [--docker]` | Empties `00-tmp`; lists pending variants and proposals; prunes the project's dangling Docker images. |
| `jproject plan reset --dry-run` | Preview the previous-plan inventory; apply only with `--pending-reviewed --approve <token>`. |
| `jproject map` | Regenerates the automatic sections of `_work/WORKSPACE.md`, keeping hand-written text. |
| `jproject storage move <folder> <destination>` | Moves a folder to another disk and leaves a junction (symlink on Linux/macOS). |
| `jproject rules install` | Adds or updates the framework section in `~/.claude/CLAUDE.md`, `~/.codex/AGENTS.md` and `~/.config/opencode/AGENTS.md`, between markers. |
| `jproject hook install` | A pre-commit hook in the product running `jproject check --product-only`. |
| `jproject tools ...` | Status, install, update and uninstall of zvec-grep and jtasks. |

`jproject help <command>` shows every flag and examples.

## Development

```
cd cli
cargo test
```

Templates live in `templates/` and are embedded in the binary; a folder set in `templates_dir` overrides them file by file. The global rules text is `global-rules/framework-section.md`.

The `jproject tools install jtasks` command enables an already installed native jtasks v2 CLI on PATH. Install its distribution separately from a trusted source. `tools update jtasks` delegates to the native updater (which requires a configured trusted release repository); `tools uninstall jtasks` disables integration without removing the independently installed CLI, plugins, or run data.

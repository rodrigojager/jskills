---
name: jproject
description: Work inside a jproject container (a project folder that is not a git repository, with product/ as its only repository and _work/ for everything AI-generated). Use it when a folder has _work/WORKSPACE.md whose front matter sets jproject to 1, when asked to create or check such a container, when deciding where a generated file goes, or when jtasks has to find its run data. Covers the folder layout, the routing table, the hard rules and the jproject commands.
---

# jproject: the container layout

A **container** is a project folder that holds the versioned code in one clean repository and keeps everything
AI-generated somewhere else. The `jproject` CLI creates it and lints it. You work with it through the files
below and the commands in the last section.

## Recognize a container

- The folder has `_work/WORKSPACE.md` whose front matter says `jproject: 1`.
- The folder itself is **not** a git repository. `product/` (the name is configurable) is.
- It has a root `AGENTS.md`, which `CLAUDE.md` imports.

Before you read or write anything in a container:

1. Read `_work/WORKSPACE.md`, the map of the container.
2. Read the `AGENTS.md` of any folder you are about to read or write in.

You can open the agent in the container or in `product/`. From inside `product/`, `jproject` finds the container
one level up.

## Layout

```
my-game/                     the container: open your agent here (any disk, any path)
├── AGENTS.md                entry rules for every agent (CLAUDE.md only imports it)
├── CLAUDE.md                imports AGENTS.md; edit AGENTS.md, not this file
├── .agents/skills/          project skills: one folder per skill with SKILL.md (canonical)
├── .claude/skills/          a junction to .agents/skills/: the same folder, do not edit separately
├── .claude/settings.json    Claude Code project settings
├── product/                 the ONLY git repository: what builds, runs, tests and ships
├── _secrets/                real .env files, keys, certificates (outside git; may be read, never copied)
└── _work/                   everything AI-generated (not versioned)
    ├── WORKSPACE.md         the map: folders, routing table, storage, active items
    ├── 00-tmp/              ephemeral scratch: delete any time
    ├── 10-docs/             doc drafts, BACKLOG.md, GLOSSARY.md, adr/NNNN-title.md, _render/
    ├── 20-design/           mocks-img/, mock-html/, previews/ (+ _variants/<artifact>/)
    ├── 25-source-assets/    heavy source files (.blend, .psd, ...); a good candidate for another disk
    ├── 30-plan/             PLAN.md, PROGRESS.md, DIVERGENCES.md, and jtasks/ (run data)
    ├── 40-handoff/          <workstream>.md, overwritten at each handoff
    ├── 50-tools/            scripts agents created, each with a header saying what it does
    ├── 60-intake/           accepted material from outside, one folder per intake
    ├── 70-proposals/        generated work awaiting a yes/no, one folder per proposal
    └── 80-ideas/            IDEAS.md (text only)
```

The folder names `product`, `_work` and `_secrets` can be changed in `WORKSPACE.md` (`product_dir`, `work_dir`,
`secrets_dir`). Read the front matter instead of assuming the defaults.

Every folder has an `AGENTS.md` with the same five sections: purpose, what goes here, what does not, conventions,
and when to delete. Read it before writing there.

## Where a generated file goes

Use the routing table in `_work/WORKSPACE.md` (section 3) as the source of truth. In short:

| You produce | Put it in |
|---|---|
| a plan | `_work/30-plan/` (`PLAN.md`, `PROGRESS.md`, `DIVERGENCES.md`) |
| an ADR, a glossary entry, a doc draft | `_work/10-docs/` |
| a mock or a preview | `_work/20-design/` |
| a handoff for the next session | `_work/40-handoff/<workstream>.md` (overwrite it) |
| a helper script | `_work/50-tools/` |
| a log, a dump, a temporary build | `_work/00-tmp/` |
| an option that needs the user's yes or no | `_work/70-proposals/<name>/PROPOSAL.md` |
| material received from outside | `_work/60-intake/<name>/INTAKE.md` |
| future ideas | `_work/80-ideas/IDEAS.md` |

Never put any of these in `product/`: no AI artifacts, doc drafts, ADRs, plans, logs, handoffs, helper scripts or
`.scratch/`. `product/` holds only what the product needs to build, run, test and ship.

## Hard rules

- `product/` is the only git repository. Commit there, not in the container.
- One canonical file per artifact, edited in place. No `_v2`, `_final`, `_old`, `_copy`, dated duplicates, or
  backups "for safety".
- Alternatives only in `_variants/<artifact>/`. Only the user consolidates. After they do, replace the canonical file
  and delete that `_variants/<artifact>/` folder at once. Mention any pending variants when you finish a task.
- Rejected proposals are deleted without a trace, unless the user asks to keep them.
- `_secrets/` may be read. Never copy a secret value into `_work/`, a log, a handoff, a commit message or output.
- Never create, move or delete anything outside the container without asking.
- Configuration backups: at most one `<file>.bak` next to the original, deleted as soon as the change is confirmed.
- Docker: fixed tags per environment (`<project>:dev`, `<project>:prod`), label images `jproject.project=<project>`,
  prune with `docker image prune --filter label=jproject.project=<project>` after a build, and run test containers
  with `--rm`.
- Everything developed in the container (code, identifiers, comments, commit messages, CLI output, templates, skills,
  technical docs) is written in English. Talk to the user in their language.
- When you finish a task: update `_work/30-plan/PROGRESS.md`, add to `DIVERGENCES.md` if you deviated from the plan,
  mention pending variants, and empty what is ephemeral.

## Starting a different plan

`10-docs/BACKLOG.md` survives every plan: keep unresolved items and necessary shared context, not completed progress or whole plans.
Before replacing a plan, review its progress and run results, and consolidate unresolved work there. Read `80-ideas/IDEAS.md` too.
List the open backlog items and ideas, explain what each means, the consequences of including or deferring it, and a recommendation.
Ask the user which to include and wait for the answer. Deferring an item preserves it; selection alone does not complete backlog work.

Then run `jproject plan reset --dry-run`. Obtain approval for the exact list and the permanent removal of finished run data,
then run `jproject plan reset --pending-reviewed --approve <token>`. Any content change invalidates the token.
The command recycles old files in `30-plan/` except `AGENTS.md` and removes finished runs through `jtasks clean`.
It preserves the backlog, ideas, ADRs, glossary and other work folders. Active/paused runs, unknown data and links block the reset.
It creates no archive. Only after it succeeds, write the new plan, progress and divergences. Refining the same plan needs no reset.
On existing containers, `jproject init --no-index` adds the missing backlog template without overwriting documents.

## Commands

Run them from inside the container or from `product/`.

| Command | What it does |
|---|---|
| `jproject setup` | First-time questions; saves the global configuration (`%APPDATA%\jproject\config.toml` on Windows). |
| `jproject init [path] [--name N] [--no-git] [--no-index]` | Creates a container. In an existing folder it only adds what is missing and never overwrites. |
| `jproject init --parallel <path> --origin <container>` | A parallel folder that reads the origin read-only and delivers through `out/` and `HANDOFF.md`. |
| `jproject root [--json]` | Prints the container, product, work, secrets and jtasks data paths. Use it instead of guessing paths. |
| `jproject check [--product-only] [--json]` | Lints the layout. Exit code 1 on errors. Run it after you change the container. |
| `jproject new intake\|proposal\|handoff <name>` | Creates an item from its template and updates "Active items" in `WORKSPACE.md`. |
| `jproject status` | Open intakes, proposals, variants and handoffs; folder sizes. |
| `jproject clean [--docker]` | Empties `_work/00-tmp`; lists pending variants and proposals; optionally prunes Docker. |
| `jproject plan reset --dry-run` | Preview the previous-plan inventory; apply only with `--pending-reviewed --approve <token>`. |
| `jproject map` | Regenerates the automatic sections of `_work/WORKSPACE.md` (between `jproject:auto` markers). Edit the rest by hand. |
| `jproject storage move <folder> <destination>` | Moves a folder to another disk and leaves a junction. |
| `jproject rules install` | Adds the framework section to the agents' global instructions, between markers. |
| `jproject hook install` | A pre-commit hook in `product/` that runs `jproject check --product-only`. |
| `jproject tools status\|install\|update\|uninstall` | Manages the optional companions: zvec-grep (search index) and jtasks. Each shows the commands and asks first. |

`jproject help <command>` lists every flag and example.

## jtasks and the container

The independent jtasks v2 stores its runtime under the container's `jtasks_data` folder, in a `v2` subdirectory. The default is `_work/30-plan/jtasks/v2`. It resolves the Git repository in `product/`; inspect with `jproject root` and the jtasks v2 read commands. Legacy run files are not migrated automatically.

jproject 0.1.3 supports the native v2 read and cleanup contracts. Reset refuses legacy data, incomplete or blocked runs and unrecognized data; it previews the exact permanent cleanup and preserves the shared database. `jproject tools install jtasks` enables an already installed native v2 CLI; `update` delegates to its native updater, and `uninstall` disables integration without removing the separately installed CLI or data. Install the native distribution and any plugins separately from trusted sources.

The `jtasks_data` key in `_work/WORKSPACE.md` sets the base folder for one container. Do not save a global setting for a single project.

## Checklist before you finish

1. `jproject check` reports no violations.
2. Nothing generated is in `product/`.
3. `_work/30-plan/PROGRESS.md` is updated; variants and proposals are mentioned.
4. Nothing was created or moved outside the container.

# jskills

Skills and command-line tools for creating software and content with AI. Use them to research existing solutions, settle design decisions, write a plan, track execution, and prepare a release. Each skill can be used on its own where its prerequisites are available.

## Why jskills exists

jskills adapts ideas from [Matt Pocock's agent skills](https://github.com/mattpocock/skills) so that decisions, plans, and execution can be tracked across agent sessions. It combines those practices with a workspace inspired by [Interpretable Context Methodology (ICM)](https://arxiv.org/abs/2603.16021) by Jake Van Clief and David McDermott, and with `jtasks`, the project's task runner and monitor.

### From interview to execution

Matt Pocock's earlier [`to-plan`](https://github.com/mattpocock/skills/blob/c5a4a8c2e966e28628f56d1bfef07f401d399df0/skills/engineering/to-plan/SKILL.md) turned a design into ordered, independently verifiable slices. He later merged that skill with `to-issues` into [`to-tickets`](https://github.com/mattpocock/skills/commit/386d4ff719a7c420ad1454232d0436b01f1b8c17). jskills keeps the path from a settled design to verifiable slices and gives the written plan and the executable task definitions their own steps.

| Step | How jskills adapts it |
| --- | --- |
| **Resolve design questions** | The upstream [`grill-with-docs`](https://github.com/mattpocock/skills/blob/main/skills/engineering/grill-with-docs/SKILL.md) delegates to `grilling` and `domain-modeling`. The bundled [grill-with-docs](skills/grill-with-docs/SKILL.md) includes the interview instructions and glossary and decision-record templates. It works with ordinary file and chat tools, without installing Pocock's skill suite or running its setup. It records confirmed terms and decisions and carries open questions into the handoff. |
| **Give the work a home** | From ICM, `jproject` takes the idea of using folders and Markdown to put the right context where agents can find it. It adds a workspace map and a split between the `product/` Git repository and `_work/`. By default, glossary entries and architecture decision records (ADRs) go in `_work/10-docs/`, and the current plan and task-run data go in `_work/30-plan/`. The agent reads the map and the relevant files for its current task. |
| **Write the plan** | [to-plan](skills/to-plan/SKILL.md) turns the settled discussion into a local `PLAN.md` with use cases, decisions, and ways to verify the outcome. Longer plans use linked Markdown files by topic. In a jproject workspace, the plan lives in `_work/30-plan/`, where it can be reviewed before work is split into tasks. |
| **Prepare tasks for execution** | Pocock's [`to-tickets`](https://github.com/mattpocock/skills/blob/main/skills/engineering/to-tickets/SKILL.md) turns a plan into verifiable tickets with explicit blockers and publishes them as local files or tracker issues. [to-jtasks](tools/jtasks/skills/source/to-jtasks/SKILL.md) uses the same dependency-based approach and composes versioned JSON for the `jtasks` runner. It gives each task the plan excerpts it needs for a fresh agent session, records any required checks and when they run, and can validate the result and preview the checks before execution. |

### Deterministic tracking and checks

`jproject check` checks workspace conventions. `jtasks validate` checks task and operation ordering before execution. When a run starts, `jtasks` follows the declared dependencies and records attempts and command results. If a check is required, the run cannot report success until that check passes. `jtasks watch` and the dashboard let you inspect progress from the stored run state. Agents still make judgments during interviews, planning, and implementation. The CLI rules make execution and completion inspectable even when the agent session changes.

For example, if an interview settles what counts as a *scheduled post*, the glossary can preserve that term. The plan can require a scheduled post to stay hidden until its publication time, and the task definition can make an automated check of that behavior mandatory. The glossary gives an agent the meaning, and `jtasks` records whether the declared check passed.

## Included tools and skills

| Name | What it does |
| --- | --- |
| [jproject](tools/jproject/README.md) | Creates and checks a workspace with separate locations for product source, plans, and execution data. |
| [jtasks](tools/jtasks/README.md) | Runs and monitors dependent tasks, agent attempts, and verification steps. Includes an Orca integration. |
| [existing-solutions-scout](skills/existing-solutions-scout/SKILL.md) | Searches registries and communities for existing solutions before you build. |
| [grill-with-docs](skills/grill-with-docs/SKILL.md) | Challenges design assumptions and records the terms and decisions you confirm. |
| [audit-other-agent](skills/audit-other-agent/SKILL.md) | Runs user-led rounds where agents challenge each other's ideas, review existing work, and explain tradeoffs so you can decide. |
| [decision-brief](skills/decision-brief/SKILL.md) | Presents decisions as concrete questions with options, consequences, a recommendation, and previews where useful. |
| [to-plan](skills/to-plan/SKILL.md) | Turns a discussion into a written, verifiable plan. |
| [plan-to-html](skills/plan-to-html/README.md) | Converts Markdown plans to interactive, offline HTML. |
| [to-jtasks](tools/jtasks/skills/source/to-jtasks/SKILL.md) | Turns an approved plan into task definitions for jtasks. |
| [estimate-jtasks](tools/jtasks/skills/source/estimate-jtasks/SKILL.md) | Fills in missing task durations while retaining estimates already in the plan. |
| [fix-tool](skills/fix-tool/SKILL.md) | Diagnoses and fixes a tool in your repository or fork, then verifies the installed fix. |
| [ship-release](skills/ship-release/SKILL.md) | Guides release checks, packaging, publishing, and installation within the scope you approve. |

## Install the skills

Clone this repository and run the installer from the checkout with Python 3.10 or later. Choose the command for your agent.

```sh
python scripts/install_skills.py codex
python scripts/install_skills.py claude
python scripts/install_skills.py opencode
```

The installer selects the jtasks instructions for that agent. Use `--dry-run` to inspect the paths and `--destination PATH` for a project-specific or isolated skills directory. It checks for conflicting entries before copying. Start a new agent session to load the skills.

For the HTML renderer, run `python scripts/setup.py --skip-mermaid` from the installed `plan-to-html` directory. It installs pinned Python dependencies inside that skill. See the [renderer documentation](skills/plan-to-html/README.md) for examples and optional packages. The scout script uses Python and needs network access to search its public sources.

### Optional UI tools

Both tools bring together curated checks from several sources and put them into a repeatable workflow. Their CLIs track each run, and their browser interfaces let you evaluate before/after previews, request revisions, and approve specific changes.

| Optional tool | When to use it |
| --- | --- |
| [jdeslop](skills/jdeslop/SKILL.md) | After the visual direction is set, audit an existing interface for recurring AI tells in layout, styling, behavior, and copy. The deduplicated checks record evidence by surface. Use the review panel to compare fixes individually or cumulatively and queue approved source edits one at a time. See the [checks and their sources](skills/jdeslop/references/sources.md). |
| [jjuicer](skills/juice-it-up/README.md) | Polish an interface through feedback, animated microinteractions, or optional seasonal touches. Use a full pass or focus on motion or seasonal opportunities. In the Lab, compare variants against the original and audition customizable effects on elements before approving individual revisions or a batch. The agent skill is invoked as `juice-it-up`. See the [source credits](skills/juice-it-up/NOTICE.md). |

The standard installation leaves both tools out. Use `--optional` to install only the tools you select, even if the standard skills are already installed. Replace `codex` with `claude` or `opencode` as needed.

```sh
python scripts/install_skills.py codex --optional jdeslop
python scripts/install_skills.py codex --optional jjuicer
# Or install both in one command
python scripts/install_skills.py codex --optional jdeslop jjuicer
```

The installer copies the selected skill files. For `jdeslop`, build and install the CLI with Rust and Cargo by running `cargo install --locked --path cli` from the installed `jdeslop` directory. Its audit and review engines need Python 3.10 or later. See the [CLI and review workflow](skills/jdeslop/references/coverage.md#combined-cli-run). For jjuicer, use Node.js 18 or later and run `node scripts/jiu.mjs --help` from the installed `juice-it-up` directory.

## Install the CLIs

Install from the checkout using Rust and Cargo. See the individual tool documentation for platform and packaging details.

```sh
cargo install --locked --path tools/jproject/cli
cargo install --locked --path tools/jtasks
jproject --version
jtasks --version
```

Run `jproject init ./my-project --no-index` to create a workspace without setting up an optional search index. The command creates two instruction files at the workspace root.

- [`AGENTS.md`](tools/jproject/templates/container/AGENTS.md) tells the agent to read `_work/WORKSPACE.md` first for the workspace layout, then the `AGENTS.md` in each folder it works in, when present.
- [`CLAUDE.md`](tools/jproject/templates/container/CLAUDE.md) imports `AGENTS.md` so Claude Code follows the same rules.

Start your agent in `./my-project`, one level above `product/`, so the workspace's entry instructions are in its starting directory. The root instructions direct it to read `product/AGENTS.md` before working on the product and to consult the rules in the work folders it uses.

To also teach your agent to recognize the jproject layout through its global instructions, preview the changes with `jproject rules install --dry-run`. Run `jproject rules install` to review and confirm the changes for Claude Code, Codex, and OpenCode. This step is optional when you start the agent in the workspace root. See the [jproject rules command](tools/jproject/README.md#commands).

Use `to-plan` when you are ready to write the plan, and `to-jtasks` when it is approved. Starting a task run and publishing a release each require the user's authorization.

Download prebuilt CLI and Orca packages from the [jskills releases](https://github.com/rodrigojager/jskills/releases). New packages use this repository for updates. Existing independent jtasks installations can switch by running:

```sh
jtasks update --check --repository rodrigojager/jskills
jtasks update --repository rodrigojager/jskills
```

For an existing local Orca plugin, set **Trusted update repository** to `rodrigojager/jskills`, then use **Check updates** and **Update** in the Tasks panel. See the [jtasks documentation](tools/jtasks/README.md#updates) for package details.

## Source versions

This collection includes jproject 0.1.3 and the native Rust jtasks v2. jtasks 2.0.2 includes the observer retry and launcher argument fixes and moves official release distribution to jskills.

## Licenses

See the [shared skills license](skills/LICENSE) and the license files and notices in each component. The HTML renderer retains credits and licenses for its bundled assets.

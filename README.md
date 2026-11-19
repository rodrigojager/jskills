# jskills

Skills and command-line tools for creating software and content with AI. Use them to research existing solutions, settle design decisions, write a plan, track execution, and prepare a release. Each skill can be used on its own where its prerequisites are available.

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

Open your agent in `./my-project`, one level above `product/`. Use `to-plan` when you are ready to write the plan, and `to-jtasks` when it is approved. Starting a task run and publishing a release each require the user's authorization.

Download prebuilt CLI and Orca packages from the [jskills releases](https://github.com/rodrigojager/jskills/releases). New packages use this repository for updates. Existing independent jtasks installations can switch by running:

```sh
jtasks update --check --repository rodrigojager/jskills
jtasks update --repository rodrigojager/jskills
```

For an existing local Orca plugin, set **Trusted update repository** to `rodrigojager/jskills`, then use **Check updates** and **Update** in the Tasks panel. See the [jtasks documentation](tools/jtasks/README.md#updates) for package details.

## Source versions

This collection includes jproject 0.1.3 and the native Rust jtasks v2. jtasks 2.0.2 includes the observer retry and launcher argument fixes and moves official release distribution to jskills.

## Origins and licenses

The workspace layout draws on Jake Van Klief's ICM approach. The original interview and domain-documentation ideas draw on Matt Pocock's skills. The bundled `grill-with-docs` contains its own instructions and templates and runs with the agent's ordinary file and chat tools. It can use a jproject workspace map when present.

See the [shared skills license](skills/LICENSE) and the license files and notices in each component. The HTML renderer retains credits and licenses for its bundled assets.

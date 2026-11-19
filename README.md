# jskills

Skills and command-line tools for creating software and content with AI. Use them to research existing solutions, settle design decisions, write a plan, track execution, and prepare a release. Each skill can be used on its own where its prerequisites are available.

## Included tools and skills

| Name | What it does |
| --- | --- |
| [jproject](tools/jproject/README.md) | Creates and checks a workspace with separate locations for product source, plans, and execution data. |
| [jtasks](tools/jtasks/README.md) | Runs and monitors dependent tasks, agent attempts, and verification steps. Includes an Orca integration. |
| [existing-solutions-scout](skills/existing-solutions-scout/SKILL.md) | Searches registries and communities for existing solutions before you build. |
| [grill-with-docs](skills/grill-with-docs/SKILL.md) | Challenges design assumptions and records the terms and decisions you confirm. |
| [decision-brief](skills/decision-brief/SKILL.md) | Presents decisions as concrete questions with options, consequences, a recommendation, and previews where useful. |
| [to-plan](skills/to-plan/SKILL.md) | Turns a discussion into a written, verifiable plan. |
| [plan-to-html](skills/plan-to-html/README.md) | Converts Markdown plans to interactive, offline HTML. |
| [to-jtasks](tools/jtasks/skills/source/to-jtasks/SKILL.md) | Turns an approved plan into task definitions for jtasks. |
| [estimate-jtasks](tools/jtasks/skills/source/estimate-jtasks/SKILL.md) | Fills in missing task durations while retaining estimates already in the plan. |
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

Run `jproject init ./my-project --no-index` to create a workspace without setting up an optional search index. Open your agent in the workspace root. Use `to-plan` when you are ready to write the plan, and `to-jtasks` when it is approved. Starting a task run and publishing a release each require the user's authorization.

For native packaging and Orca, follow the [jtasks documentation](tools/jtasks/README.md). Builds installed from source use the source-build update behavior described there.

## Source versions

This collection includes jproject 0.1.3 and the jtasks 2.0.1 source with the observer retry and launcher argument fixes. The existing jtasks 2.0.1 release packages predate those fixes.

## Origins and licenses

The workspace layout draws on Jake Van Klief's ICM approach. The original interview and domain-documentation ideas draw on Matt Pocock's skills. The bundled `grill-with-docs` contains its own instructions and templates and runs with the agent's ordinary file and chat tools. It can use a jproject workspace map when present.

See the [shared skills license](skills/LICENSE) and the license files and notices in each component. The HTML renderer retains credits and licenses for its bundled assets.

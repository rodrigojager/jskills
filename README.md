# jskills

A toolkit for creating software and content with AI agents. Use a project workspace to keep decisions and plans accessible, then give each task the context it needs to run independently.

## Workflow

| Step | Component | Result |
| --- | --- | --- |
| 1 | `jproject` | A workspace map, a product repository, and directories for working documents. |
| 2 | `existing-solutions-scout` | An optional comparison of existing tools before deciding what to build. |
| 3 | `grill-with-docs` | Agreed terminology, architecture decisions, and open design questions. |
| 4 | `to-plan` | A Markdown plan with acceptance checks and shared executor context. |
| 5 | `plan-to-html` | An optional, portable HTML copy for reviewing the plan. |
| 6 | `to-jtasks` | Version 1 task intent with dependencies and task-specific context. |
| 7 | `estimate-jtasks` | Optional estimates for tasks that lack an active-work duration. |
| 8 | `jtasks` | A controlled run with task status, verification results, and a dashboard. |
| 9 | `ship-release` | An optional release workflow with checks, publishing, and installation verification. |

The workspace map tells the agent where to read and write. In a default jproject workspace, the product lives in `product/`, the glossary and ADRs live under `_work/10-docs/`, and plans live under `_work/30-plan/`. The Markdown plan remains the input to task authoring.

Use `decision-brief` at any stage where the user needs to decide. It presents numbered questions with context, options, consequences, and a recommended answer. For choices that benefit from a visual comparison, use the bundled HTML template.

## Package status

The project is preparing its first installable release. The table describes the intended package.

## Requirements

- Run `grill-with-docs` in a coding agent that can read and edit workspace files and ask follow-up questions. Its own skill folder is sufficient. It can read the jproject workspace map directly.
- `existing-solutions-scout` ships with `scripts/search.py`. The collector uses the Python standard library and needs network access. Its GitHub source uses an authenticated `gh` CLI. Use `--sources` to select sources independently.
- `decision-brief` works in chat. The optional `preview.template.html` opens in a browser. Mermaid previews in the current template load the diagram renderer from a CDN.
- The full workflow uses the native `jproject` and `jtasks` CLIs. Use the matching `jtasks` skills for the agent you run.
- `plan-to-html` needs Python 3.10 or later and a one-time renderer setup. The optional `estimate-jtasks` helper also needs Python 3.10 or later and a configured coding-agent harness.

## Credits

- The design interview and decision records draw on [Matt Pocock's skills](https://github.com/mattpocock/skills).
- Workspace context follows ideas from [Interpretable Context Methodology](https://assets.skool.com/f/9a42d1d697754c499c3effeee8135e03/4697ef91e2284de0966db21cffa7f0bf6dd12adab6fe473d8bd9abb69ae92578) by Jake Van Clief and David McDermott.
- jtasks supplies the task controller and monitoring workflow.

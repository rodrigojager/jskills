# Product map: ai-workspace-framework

One row per folder (and relevant subfolder) of this repository. Keep it portable: no absolute paths, disks or local-only folders.

| Path | What lives there |
|---|---|
| `cli/` | the `jproject` Rust crate: `Cargo.toml`, `src/`, `tests/` |
| `cli/src/` | one module per concern: `main` (commands and help), `config`, `container` (discovery), `templates`, `init`, `check`, `map` (map/status), `ops` (new, clean, storage, rules, hook), `plan` (reviewed plan reset), `tools`, `setup`, `util` |
| `cli/tests/` | integration tests on temporary folders, one per command and one per `check` rule |
| `templates/` | files `jproject` embeds: `container/`, `product/`, `work/<folder>/`, `items/` (intake, proposal, handoff), `parallel/` |
| `global-rules/` | the "AI workspace framework" section installed in the agents' global instructions by `jproject rules install` |
| `skills/jproject/` | the agent skill that teaches the container layout and the commands (`SKILL.md`), shipped with the tool |

## Allowed Markdown files
- `README.md`
- `AGENTS.md` (root, and folders with their own rules, up to the second level)
- `WORKSPACE.md` (root only)
- `templates/` and `global-rules/`: the Markdown files there are product (templates), not documentation.
- `skills/jproject/SKILL.md`: the agent skill for this tool, placed on purpose and shipped with it.

AI working artifacts live outside this repository, in the container's `_work/` folder.

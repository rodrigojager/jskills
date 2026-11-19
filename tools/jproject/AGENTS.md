# ai-workspace-framework

## Purpose
The product repository of the AI workspace framework: the `jproject` CLI (Rust), the templates it embeds and the text of the global agent rules.

## Architecture
A single Rust binary (`cli/`) embeds every file of `templates/` and `global-rules/` with `include_str!`; containers are created from those templates and linted against the framework rules.

## Conventions
- Everything is in English: code, identifiers, comments, commit messages, CLI output, `--help`, templates.
- No absolute paths, drive letters or user names in code, templates or rules (`cli/tests/cli.rs` checks it).
- Every command has `--help` with at least one example (tested). Error messages suggest the right command or flag.
- Every `check` rule has its own integration test in `cli/tests/cli.rs`.
- Run `cargo test` in `cli/` before committing.
- A new template file must be added to `EMBEDDED` in `cli/src/templates.rs`.

## Forbidden
- Creating new top-level folders without being asked.
- Creating `.md` files other than `README.md`, `AGENTS.md` (root and folders with their own rules) and `WORKSPACE.md` (root only); files under `templates/` and `global-rules/` are listed in `WORKSPACE.md`.
- Saving AI working artifacts here (plans, ADR drafts, mocks, logs, handoffs, helper scripts, `.scratch/`). They live in the container's work folder.
- Versioned copies of files (`_v2`, `_final`, `_old`, `_copy`, `.bak`).

See `WORKSPACE.md` for where everything lives.

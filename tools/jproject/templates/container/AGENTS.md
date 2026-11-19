# Container: {{project}}

This folder is a project container, not a repository. Before reading or writing anything:

1. Read `{{work_dir}}/WORKSPACE.md` (the full map of this container).
2. Before reading or writing inside any folder, read that folder's `AGENTS.md` if it exists.

Hard rules:
- `{{product_dir}}/` is the only git repository. Only what the product needs to build, run, test and ship goes there.
- Everything AI-generated goes to `{{work_dir}}/`, in the folder the routing table in `{{work_dir}}/WORKSPACE.md` names.
- One canonical file per artifact, edited in place. No `_v2`, `_final`, `_old`, backups or copies.
- Alternatives only in `_variants/`; only the user consolidates; delete the variants right after.
- `{{secrets_dir}}/` may be read; never copy secret values anywhere.
- Never delete, move or create anything outside this container without asking.
- Everything you develop (code, identifiers, comments, commit messages, CLI output, templates, skills) is in English.

When you finish a task: update `{{work_dir}}/30-plan/PROGRESS.md` (and `DIVERGENCES.md` if you deviated from the plan), mention pending variants, and empty what is ephemeral and no longer needed.
Run `jproject check` to lint this container.

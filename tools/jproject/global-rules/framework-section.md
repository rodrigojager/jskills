## AI workspace framework

### Recognizing a container
- A project container is a folder with `<work_dir>/WORKSPACE.md` (usually `_work/WORKSPACE.md`) whose front matter has `jproject: 1`. It is not a git repository; `product_dir` (usually `product/`) inside it is.
- Inside a container, its root `AGENTS.md` (imported by `CLAUDE.md`) rules. Read `_work/WORKSPACE.md` first, then the `AGENTS.md` of any folder before reading or writing in it.
- Open agents in the container, not in `product/`.

### Hard rules inside a container
- `product/` is the only git repository. Only what the product needs to build, run, test and ship goes there. No AI artifacts, doc drafts, ADRs, plans, logs, handoffs, helper scripts or `.scratch/`.
- Everything AI-generated goes to `_work/`, in the folder the routing table of `_work/WORKSPACE.md` names.
- One canonical file per artifact, edited in place. No `_v2`, `_final`, `_old`, `_copy`, dated duplicates, backups or copies "for safety".
- Alternatives only in `_variants/<artifact>/`. Only the user consolidates; then replace the canonical file and delete that `_variants/<artifact>/` folder at once. Mention pending variants when you finish.
- Rejected proposals are deleted without a trace, unless the user asks to keep them.
- `_secrets/` may be read; never copy secret values into `_work/`, logs, handoffs, commit messages or output.
- Never delete, move or create anything outside the container without asking.
- Before a different plan, carry unresolved progress and shared context into `10-docs/BACKLOG.md`; preserve ideas, ADRs and glossary. Ask which backlog items and ideas to include, explaining consequences and a recommendation; wait for the answer. Then use `jproject plan reset` with its reviewed inventory before writing the new plan. Refining the current plan does not reset it.
- Handoffs go to `_work/40-handoff/<workstream>.md`, overwritten each time; never to a system temp folder.

### Skills that write files
Until skills are adapted, any skill that tells you to write plans, ADRs, glossaries, tickets, HTML, mocks or reports inside the repository must write to the matching `_work/` folder instead, even if the skill says otherwise: plans → `_work/30-plan/`, ADRs → `_work/10-docs/adr/`, glossary → `_work/10-docs/GLOSSARY.md`, rendered HTML → `_work/10-docs/_render/`, mocks → `_work/20-design/`, reports and logs → `_work/00-tmp/` or `_work/10-docs/`, proposals → `_work/70-proposals/<name>/`. Outside a container, ask where to save instead of assuming the repository.

### Rules that apply everywhere, inside a container or not
- Configuration backups: at most one `<file>.bak` next to the original, without a date in the name, deleted as soon as the change is confirmed. Never `file.bak-20260101-x`.
- Docker: fixed tags per environment (`<project>:dev`, `<project>:prod`), never one tag per attempt; label every image/container `jproject.project=<project>`; after each build prune that project's dangling images (`docker image prune --filter label=jproject.project=<project>`); test containers always with `--rm`.
- Tools you create for a project stay in its `_work/50-tools/`; project skills stay in its `.agents/skills/`. Move them to the global tools folder or global skills folders only if the user asks. You may offer when something looks generic.
- Everything you develop (code, identifiers, comments, commit messages, CLI output, templates, skills, technical docs) is in English. Conversation with the user may be in their language.

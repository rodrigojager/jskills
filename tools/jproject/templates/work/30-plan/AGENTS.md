# 30-plan

## Purpose
Live planning: the one current plan, its progress and its divergences.

## What goes here
- `PLAN.md`: the current plan (only one). Child files for details are allowed (`PLAN-<topic>.md`).
- `PROGRESS.md`: progress of the plan.
- `DIVERGENCES.md`: what changed from the plan and why.
- `jtasks/`: jtasks run data (tasks, tickets, ledger), when jtasks is used.

## What does NOT go here
- Copies of the plan or old plans.
- ADR drafts (`10-docs/adr/`).

## Conventions
- Fixed file names, edited in place.
- Before a different plan, consolidate unresolved progress and shared context into `10-docs/BACKLOG.md`; never copy completed history.
- Ask which backlog items and ideas to include, explaining each, consequences and a recommendation; wait for the answer.
- Refining the same plan does not reset it.

## When to delete
- Before writing a different plan, run `jproject plan reset --dry-run`, obtain approval for its exact inventory, then run `jproject plan reset --pending-reviewed --approve <token>`.
- The command removes old planning artifacts and finished runs; active or paused runs block it. Backlog, ideas, ADRs and glossary survive. No archive.

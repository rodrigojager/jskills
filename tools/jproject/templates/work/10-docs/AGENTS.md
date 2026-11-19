# 10-docs

## Purpose
Drafts of documentation. The user decides what becomes official and copies it to the product by hand.

## What goes here
- One canonical `<topic>.md` per subject, edited in place.
- `GLOSSARY.md`: the domain language.
- `BACKLOG.md`: unresolved work across plans, with only open items and necessary shared context.
- `adr/NNNN-title.md`: numbered ADR drafts, status `proposed` then `accepted`.
- `_render/`: HTML generated from these Markdown files (always regenerable).
- One-off documents may carry a date, `YYYY-MM-DD_topic.md`, and are still unique.

## What does NOT go here
- Official docs already published (the user places them in the product).
- Plans and progress (`30-plan/`), mocks (`20-design/`).

## Conventions
- No `_v2`, `_final`, `_old` or copies; edit in place.
- `_render/` is output only; never edit it by hand.

## When to delete
- Preserve backlog, glossary and ADRs across plan resets. Remove backlog items only after verified completion or explicit user discard.
- Other drafts: when the user takes the content or drops it. `_render/` any time.

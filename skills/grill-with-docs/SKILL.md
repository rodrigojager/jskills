---
name: grill-with-docs
description: Stress-test a software design through a decision interview and record confirmed domain terms and significant architecture decisions. Use before planning when the user wants to challenge assumptions and leave a written record.
---

# Grill with Docs

## Find the relevant context

- Read the project's entry instructions and follow the pointers for the area under discussion. Check existing terminology, decisions, requirements, and the code that can settle factual questions. Open only the files relevant to the current decision.
- In a jproject container, read the workspace map and destination instructions. Use its glossary and ADR routes, typically `_work/10-docs/GLOSSARY.md` and `_work/10-docs/adr/`. Review the product repository for evidence. The workspace map is sufficient when the CLI is unavailable.
- In another project, follow its documented glossary and ADR locations. Ask for a destination only if the project has none and there is a confirmed item to record. The interview can continue while the destination is undecided.

## Interview

1. State the design outcome, the constraints you can verify, and the unresolved decisions that matter. Verify facts yourself. If evidence is missing or conflicts with the proposal, name the gap and ask about the intended behavior.
2. Ask the independent questions whose prerequisites have been settled. For each, give the recommended choice and its consequence. Use a realistic example when the decision affects a boundary, failure, recovery, or term that could mean two things.
3. Wait for answers. Update the set of open decisions and ask the next round. Carry unresolved questions forward explicitly. A recommendation stays open until the user confirms it.

## Record decisions as they settle

- Check any glossary map before editing. Update the glossary for the affected context with the agreed term and a short definition. If it has no format, use the [glossary template](references/GLOSSARY-FORMAT.md). Include a disambiguation only if it prevents a real mix-up.
- Create an ADR when a confirmed choice has meaningful reversal cost and a future maintainer would need to know the trade-off. Use the project's numbering and format. If it has no format, use the [ADR template](references/ADR-FORMAT.md). Link the previous record if the new choice supersedes it.
- Preserve existing documentation and treat drafts or options as unresolved. Create files only when there is a confirmed entry to add.

## Hand off

When the design has enough agreement to plan, summarize the goal, confirmed decisions, unresolved questions, and links to the glossary and ADRs. If the user requests a plan, pass this summary to `to-plan` when available. The interview and documentation work need only file access and a way to converse with the user.

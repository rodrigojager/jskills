---
name: to-plan
description: Turn the current conversation, and any documents or existing plan the user points to, into a written plan outside the product repository (in a jproject container, `_work/30-plan/`); a parent PLAN.md (problem, solution, use cases, interface and references, decisions, how to demonstrate, out of scope, context for executors, what needs a person, open points with suggestions) with child Markdown files for details, and ADRs for the decisions worth keeping. Synthesize the discussion; ask which unresolved backlog items and ideas to include before a new plan. Use when the user asks for a plan or a spec after discussing the work, or asks to refine an existing plan for this repository.
---

# To Plan

Turn what has already been discussed into a written plan. Synthesize what is settled instead of conducting a new interview.
Before starting a different plan, explicitly ask which open backlog items and ideas the user wants to include, as described below.
For every question, explain the subject, the consequences of the choices, and a recommendation with its reason in the user's language.
Other unresolved details go into the plan as open points with suggestions.

## Where files go

The plan and its ADRs are work artifacts: they never go into the product repository.

- **Inside a jproject container** (its work folder, usually `_work/`, holds a `WORKSPACE.md` whose front matter has
  `jproject: 1`; resolve the paths with `jproject root --json` or by reading `_work/WORKSPACE.md`): the plan is
  `_work/30-plan/PLAN.md`, the single current plan, with its children beside it as `_work/30-plan/PLAN-<topic>.md`;
  each ADR is `_work/10-docs/adr/NNNN-title.md`. The repository you explore is the container's `product/`.
- **Outside a container**, ask the user where to save the plan and its ADRs before writing anything; do not assume
  the repository.

## Inputs

Any mix of:

- **The conversation so far**: the use cases discussed, what the user wants and does not want, the UI mockups
  generated along the way.
- **Documents the user points to** (a problem statement, notes, an issue): read each one in full.
- **An existing plan to refine**: keep its decisions, reorganize it into the structure below, adapt it to this
  repository (its architecture, modules, conventions and tests), and mark every place where it does not fit the code
  as it is. Add the Context for executors paragraph when it is missing.

## Process

1. Explore the repository to understand the current state of the code, if you have not already. Use the names the
   code and the conversation already use, and follow the conventions the repository documents (README, AGENTS.md,
   CLAUDE.md, contributing guides), if any. Read the ADRs that touch the area you are changing: the ADR drafts
   in `_work/10-docs/adr/` (or wherever the user keeps them outside a container), and the repository's own ADR folder
   if it keeps one. A plan that contradicts an ADR says
   so in Decisions, naming it, and never overrides it silently. Look facts up yourself: anything you can find in the
   code is never an open point.

2. Find where the behaviour will be tested: the highest points from which it can be exercised from outside the code
   that implements it (an end-to-end test, a CLI call, an API request, a UI driven by a browser). Prefer points that
   already exist to new ones; the fewer, the better (ideally one). Write them in the plan (How to demonstrate); do not
   ask.

3. Before a new plan in a container, follow "Carry-over and plan reset" below. Run the reset command successfully
   before writing a different plan when previous planning artifacts exist. When refining the same plan, edit it in place;
   do not reset it. Write where "Where files go" says unless the user specifies another place. Outside a container,
   never overwrite an existing plan without asking.

4. Record as ADRs the confirmed decisions worth keeping (see ADRs below), one file each in `_work/10-docs/adr/`
   (outside a container, where the user said): the ones settled in the conversation, and the ones a refinement
   settles. Decisions in PLAN.md point to them.

5. Tell the user where the plan is, list its children and the ADRs written (one line each), what needs a person, and
   its open points. Nothing else.

## Carry-over and plan reset

Resolve paths from WORKSPACE.md; the following paths are relative to its work folder.

1. Before a new plan, read `10-docs/BACKLOG.md` and `80-ideas/IDEAS.md` if present. When replacing an old plan, also read
   its `30-plan/PROGRESS.md`, divergences and relevant run results. Consolidate only unresolved items plus the shared
   context required to act on them into BACKLOG.md. Do not copy completed items, old execution history or the entire
   progress file. Use stable IDs and include the problem, what remains and how completion will be verified.
   Make the retained context self-contained; do not leave essential explanations only in files the reset will remove.
   Do not treat skipped tasks as completed; assess whether their work is still pending. If there is no pending work,
   keep an empty backlog. `jproject init --no-index` can add its template to an existing container without overwriting files.

2. Tell the user there are unresolved points and future ideas, listing ALL open entries from both files. For each, explain
   what it means, the benefit and scope/cost of including it now, and the consequence of deferring it. Give a clear
   recommendation and why. Use the decision-brief skill when available, otherwise numbered questions with these same details.
   Ask which entries to include in this plan; allow choosing IDs, a recommended set or none. Wait for the answer before
   resetting or writing dependent plan content. Do not infer acceptance from silence or from the request for a new plan.
   If the user has already explicitly selected or deferred these exact items in this conversation, honor that answer
   without asking again. If both lists have no open entries, state that briefly and continue without a selection question.

3. Deferred backlog items and ideas remain in their canonical files. Selected backlog items stay open until verified
   completion, with a reference to their new plan rather than a duplicate specification. Move a selected idea out of
   IDEAS.md only after it is captured in the approved plan or proposal. Never discard entries merely because a new plan starts.
   Preserve ADRs and glossary, including existing numbering and decision status.

4. If old planning artifacts or run data exist, run `jproject plan reset --dry-run`. Review the exact inventory, references
   and any native jtasks effects. Explain that old planning files will leave the live workspace and finished run data will
   be permanently removed; backlog, ideas, ADRs and glossary survive. Obtain approval for that exact list (use safe-cleanup
   if available). Scope selection and deletion approval can be presented together once the inventory is ready. Any changes
   after the preview require a fresh token and review. Run `jproject plan reset --pending-reviewed --approve <token>`.
   This flag attests to the carry-over review in step 1; the command does not interpret arbitrary progress prose.
   If it refuses active/paused runs, unknown data, links or a changed inventory, resolve the reason or ask the user;
   do not bypass it, erase tool data manually, or overwrite the old plan. If the command is unavailable, stop and report
   that jproject needs the reset-capable build rather than falling back to shell deletion.

5. Only after successful cleanup, write the new PLAN.md and children plus fresh PROGRESS.md and DIVERGENCES.md for that
   plan. Keep existing folder AGENTS.md. Update backlog references to the new plan. Never archive the old artifacts.
   This transition applies to a different plan, not a refinement of the plan already being executed.

## The parent: PLAN.md

<plan-template>

# <Title>

## Problem

The problem the user is facing, from the user's perspective.

## Solution

The solution to the problem, from the user's perspective.

## Use cases

A LONG, numbered list of use cases, each in the format:

1. As a <actor>, I want <feature>, so that <benefit>

<use-case-example>
1. As a mobile bank customer, I want to see the balance of my accounts, so that I can make better informed decisions about my spending
</use-case-example>

This list should be extremely extensive and cover all aspects of the work.

## Interface and references

What a reader needs to see or consult: the screens discussed or generated in the conversation (a file, an image, a
short description of each, and how the user moves between them), and the references the work builds on (documents,
links, prior art, similar code in the repository). Omit this section when the plan is simple enough to need none.

## Decisions

**Confirmed**: the decisions settled in the conversation or the documents, each followed by a link to its ADR when it
has one (`ADR-0003`). **Assumed**: the ones you made because nothing said otherwise, each with one line on why. Either
kind can be about:

- the modules that will be built or modified, and their interfaces;
- architectural decisions;
- schema changes and API contracts;
- specific interactions;
- technical clarifications from the user;
- non-functional requirements (performance, security, accessibility, languages, ...) when the conversation touched
  them.

## How to demonstrate

- Where the behaviour is tested (step 2), and what proves each part works.
- What makes a good test here: it checks external behaviour, not implementation details.
- Which parts get tests, and the similar tests already in the repository to follow.
- No test waits for a person. A check that would need someone at the screen is automated (Playwright or another
  browser driver, mouse and keyboard control of the app, a CLI or API call) or, when it cannot be automated, is
  taken as approved and left for the user's own review after the run. The user evaluates the finished work later,
  by testing it in a pair-programming session or by asking for another run with what did and did not work.

## Out of scope

What the user said they do not want, and what this plan deliberately leaves out.

## Context for executors

One short paragraph for the agents that will carry out the work as tasks, each of them starting with a fresh context
and knowing only its ticket: the problem and the goal in two or three sentences, the decisions they must not reopen
(naming their ADRs), and the standing rules every task follows (the user's rules for the project, what never to
touch, how to test). `to-jtasks` copies it into the shared context of every task, so keep it short and factual and
do not restate the children here: `to-jtasks` gives each task the parts of the plan it needs, copied into its ticket
or into a file that holds only them, never as "read section N of" a larger file.

## Needs a person

What the work needs from a person: an account, a credential or API key, a purchase, an approval, a setting in a
third-party dashboard, a check on a real device. For each, say when it must be ready and suggest how an agent can do
it instead whenever that is technically possible: through browser or computer use (a Playwright MCP or any tool that
fits; how is the agent's call), with what the person provides beforehand and where (credentials in an environment
variable, a secret store or a file outside the repository, a logged-in browser session). Such an item becomes a task
for an agent by default, unless the user rejects it for security, trust, compliance or technical limits. What only a
person can do stays here, with why and when it is needed. Never write a credential itself in the plan. Omit this
section when there is nothing.

The work runs unattended, so nothing in it waits for a person. A decision, confirmation or approval a task would
ask for is answered in the plan with the best answer for the user (the recommended option, written as decided), and
an installation or other action the plan needs is authorized by the plan itself. Only what truly cannot be done
without the person (a credential they hold, a purchase, a physical device) belongs here, and it must be ready before
the run starts; it never becomes a task that stops to ask.

## Open points

What neither the conversation, the documents nor the code settled. Each one with a suggestion and the reason for it.
Unless the user contests it, the suggestion is what the work follows by default; the user corrects it by editing this
file. Omit when there are none.

## Notes

Anything else worth knowing. Omit when empty.

## Parts

One line per child, in reading order: `[PLAN-topic.md](PLAN-topic.md): <kind>: <what it holds>`. Omit when the plan has
no children.

</plan-template>

Do NOT put specific file paths or code snippets in PLAN.md: they go stale fast. Exception: when a prototype produced
a snippet that encodes a decision more precisely than prose (a state machine, a reducer, a schema, a type shape),
inline its decision-rich part within the relevant decision and note that it came from a prototype.

## Children

A small plan is PLAN.md alone. When a topic needs depth of its own, or the plan grows too big to read comfortably
in one file, move the topic into a child, `PLAN-<topic>.md`, beside PLAN.md (`<topic>` is short kebab-case); Parts
gives the reading order.

A child is free-form: whatever its topic needs. It can be the detailed spec of one part, a technology detail, a
reference guide, or a sub-plan (which may reuse the parent's structure), and it may hold paths and code when that is
its point. Only three things are fixed, so a reader always knows where they are:

- a `# Title`;
- a first line saying what it is: `Kind: <spec | technology | reference | sub-plan | ...>. <one line>`;
- a link back: `Part of [PLAN.md](PLAN.md).`

Each child covers one topic and is self-contained enough that someone reading PLAN.md plus that child alone can work
on it. PLAN.md never restates a child: it points to it from Parts, and from the section a child expands, if any.

## ADRs

An ADR records one decision that outlives the plan: that it was made, and why. Record a confirmed decision as an ADR
only when all three are true:

1. **Hard to reverse**: changing your mind later has a meaningful cost.
2. **Surprising without context**: a future reader would look at the code and wonder why it was done this way.
3. **The result of a real trade-off**: there were genuine alternatives, and one was picked for specific reasons.

Typical ones: the architectural shape, how parts talk to each other, a technology that carries lock-in, a boundary
or scope decision (the explicit "no"s too), a deliberate deviation from the obvious path, a constraint not visible in
the code, an alternative rejected for non-obvious reasons. An assumption or an open point is not an ADR: it is not
decided yet.

Each ADR is its own file in `_work/10-docs/adr/`, `NNNN-<short-title>.md`, numbered after the highest number already
there (`0001`, `0002`, ...); create the folder only when the first one is needed. The plan does not hold the ADRs: its
Decisions link to them. One short paragraph is enough:

```md
# ADR-0003: <Short title of the decision>

Status: proposed

<1-3 sentences: the context, what was decided, and why.>
```

`Status` goes from `proposed` to `accepted` when the user accepts it. Add `Considered options` or `Consequences` only
when they say something the paragraph does not. An ADR is not rewritten when a decision changes: add a new one, and
mark the old one `Status: superseded by ADR-NNNN`.

## After the plan

If the work is broken into tasks next, PLAN.md and its children are their source, Context for executors becomes the
run context every ticket shows, and each child can be the source of the tasks it describes. `to-jtasks` copies to
each task exactly the sections, rows and list items it needs, so write them to stand alone: a heading per topic,
one row per slice in a table whose first cell is the slice's ID, numbered use cases, and acceptance cases as list
items that start with their ID in bold. The items of Needs a person that an
agent can do become tasks too; what only a person can do must be ready before the work starts, since a task that
waits for a person stalls.

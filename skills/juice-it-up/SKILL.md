---
name: juice-it-up
description: "Improves website UX from a URL, code, or both. Use for a full polish review, for adding animated microinteractions only, or for seasonal touches only. Compare optional mocks in the Lab, approve exact revisions individually or in a batch, and resume by run ID."
---

# juice-it-up — controller

You are the controller of a workflow that improves the experience and visual impact of **websites**. You start or resume a run, inspect the target, dispatch specialists, consolidate, prioritize, record choices, route previews to the lab, and implement **only** the selected scope. You do not define aesthetics, palette, or amount of animation a priori: those come from the product, the evidence, and the user's choices.

The workflow has two interfaces inside this skill:
- **Controller** (this file): entry point to start, resume, decide, implement, and verify.
- **Lab** ([subskills/lab/SUBSKILL.md](subskills/lab/SUBSKILL.md)): compares isolated demos with the original and collects feedback and explicit batch approvals for the controller. It can be called directly: `/juice-it-up lab R-001 [refs]`.

Specialists are sub-skills in this same folder, read by the subagents you dispatch:

| Role | Sub-skill | Stage |
|---|---|---|
| Technical and visual recon | [subskills/recon](subskills/recon/SUBSKILL.md) | E1 |
| UX analyst | [subskills/ux](subskills/ux/SUBSKILL.md) | E2 |
| Art director and concepts | [subskills/art-direction](subskills/art-direction/SUBSKILL.md) | E2, E3 |
| Motion and media specialist | [subskills/motion-media](subskills/motion-media/SUBSKILL.md) | E2, E3, E4, E7 |
| Accessibility and performance | [subskills/a11y-perf](subskills/a11y-perf/SUBSKILL.md) | E2, E5, E8 |
| Mock builder | [subskills/lab](subskills/lab/SUBSKILL.md) (section "Mock builder") | E4 |
| Independent critic | [subskills/critic](subskills/critic/SUBSKILL.md) | E5, E8 |
| Implementer | [subskills/implement](subskills/implement/SUBSKILL.md) | E7 |
| Integration verifier | [subskills/verify](subskills/verify/SUBSKILL.md) | E8 |
| Seasonal details | [subskills/seasonal](subskills/seasonal/SUBSKILL.md) | E2, E3, E7, E8 in full or seasonal mode |

Roles are dispatched as needed, not as nine permanent agents.

## Invariants

They apply in every stage. Breaking one is a workflow failure, even if the result looks good.

1. **State does not live in the chat.** Every identity, decision, artifact, and piece of progress goes to the state directory through the `jiu` tool ([references/state.md](references/state.md)). Before acting on an existing run, read the state; after finishing a piece of work, record it before starting another.
2. **You are the only writer of the main state.** Workers write only to the paths you assigned them (drafts, evidence, previews of their own revision, reports). The lab only produces events in `events/inbox/`; you validate and incorporate them with `events-ingest`.
3. **Three distinct authorizations, never implicit** ([references/decisions.md](references/decisions.md)):
   - *preview*: build an isolated demonstration of an exact revision. Does not touch the target.
   - *implementation*: change the real target for an exact revision.
   - *publishing*: commit to a shared branch, merge, push, deploy. Changing code does not imply publishing.
   Paid media has its own budget and authorization.
4. **Choosing to see is not choosing to integrate.** Preference in the lab does not authorize implementation. Selecting one part does not select the others. Rejecting one proposal does not block the others.
5. **Decisions point to an exact item, variant, and revision** (`O-01/V2/r3`). Resolve labels such as "the second option" or "that calmer one" to the exact reference, and if more than one reading is plausible, ask before recording. A new revision does not inherit the previous revision's decision.
6. **Until E7, the real target is untouchable.** Diagnosis, proposals, and mocks do not change project files, do not install global dependencies, and do not run migrations. Mocks stay in the run directory. Git worktrees are never used, in any stage.
7. **No reset for a localized change.** If the target changed, `check-stale` marks only the affected opportunities; the others carry on.
8. **Idempotency.** Repeating "resume" or an action already completed does not generate paid media again (`media claim`), does not reimplement (`task`), and does not redo audits already recorded.
9. **Honesty about evidence and capability.** No claim that something works, passes, or is done without evidence produced in this stage (a command and its output, a capture, a diff); a worker's success report is a claim to check, not evidence. Inferences are marked as inferences. A mock declares what is functional and what is simulated. Without subagents, say so, and do not call a sequential pass an "independent review".
10. **Archive before cleaning.** Briefing, mock code, assets, and decisions stay in the state. Servers and browsers may be shut down; history is not deleted. Cleanup rules, "promote", or "delete harness" from reference skills do not apply here.

## State tool

```
node <skill-dir>/scripts/jiu.mjs <command> ... --state <state-dir>
```

`<skill-dir>` is the folder of this file. State directory: inside a jproject container (its work folder, usually `_work/`, holds a `WORKSPACE.md` whose front matter has `jproject: 1`; `jproject root --json` prints the paths), `_work/30-plan/juice-it-up/`, never inside the product repository; `jiu` defaults to it when run from inside a container. Outside a container, ask the user where to keep the state before `init`; do not assume the repository. Always pass `--state` as an absolute path. Commands, schemas, exit codes, and the resume protocol are in [references/state.md](references/state.md). Summary of daily use: `init`, `show`, `next`, `validate`, `check-stale`, `opp-add`, `variant-add`, `variant-revise`, `decide`, `skip-stage`, `preview-set`, `reviewed`, `task`, `events-ingest`, `media`, `serve`.

Exit code 3 means a rule or limit for authorization refused the action: do not work around it; resolve it with the user, or record the exception with `--over-limit` and a reason when the user asked for it.

## Routing the request

Classify the message before anything else:

| The user says | Action |
|---|---|
| URL, path, or "analyze this site and propose improvements" | Start `--workflow full` (the default). Run general diagnostics, motion discovery and the seasonal fit check, then critique the proposed set before approval. |
| "Only add animated microinteractions", "find small moments of delight on this page" | Start `--workflow motion`. Run the [addition-only pass](subskills/motion-media/SUBSKILL.md#addition-only-pass) on the named journey or surface. |
| "Seasonal only", "a Santa hat on the logo", "what would fit for Halloween?" | Start `--workflow seasonal` and follow the [seasonal procedure](subskills/seasonal/SUBSKILL.md). |
| "resume R-014", "continue where we stopped", the name of an already analyzed site | E0 (resume) → `next` → the next compatible action |
| "create mocks of O-01/V2 and O-03/V1", "I want to compare" | Record `decide … preview` for the exact revisions → E4 |
| "lab R-014", "open the lab" | Read [subskills/lab/SUBSKILL.md](subskills/lab/SUBSKILL.md) and follow the direct interface |
| "on O-01 I liked V2 but reduce the motion" | Record feedback, generate a new revision (V2/r+1) in the lab; does not authorize implementation |
| "on O-03 keep the original" | `decide R O-03 keep-original` |
| "implement O-01/V2 r3; leave the rest" | `decide … implement` only on that revision → E7 → E8 |
| "Implement all the recommended changes" | Match the user's approval to the exact displayed batch → `approve-all` → E7 → E8. In the Lab select **Approve all…** and confirm the list. |
| "more alternatives", "another round" | E3 for the opportunities cited, preserving all existing decisions |
| "reject", "defer" | `decide` with the exact reference |
| "publish", "merge", "deploy" | Only with explicit publishing authorization; confirm the scope |

If the message names neither a target nor a run and there are runs in the current directory, list them (`jiu list`) and ask which one to resume or whether this is new work.

## Stages

Advance through the applicable checks and proposals before asking for a choice. If the user wants mocks, confirm what to build and return to review after rendering. The user may approve the complete recommended list directly from the summary without opening individual previews.

### E0 — Initialize or resume

1. Resolve the target: normalized URL, absolute code path, or both. Read [references/input-modes.md](references/input-modes.md) for the mode.
2. `jiu init --url … --code … --workflow full|motion|seasonal`. Exit code 2 (`EXISTING_RUN`) identifies a compatible target and workflow. Use `--new` when asked for new work in the same workflow. Resumed runs created before workflow selection keep their existing seasonal offer/request rule.
3. **Resume** (follow [references/state.md#resume](references/state.md#resume)): `show` → `validate` → `check-stale` → `events-ingest` → reconcile `in_progress` tasks and `claimed` media → `next`. Do not repeat audits or generation just because the conversation is new.
4. Check the real environment and record it in `capabilities.*`: available subagents, browser automation, git, Node/dev server, media CLI. Do not assume a framework, hosting, browser, or media generation.
5. Set limits with the user only if they mentioned them; otherwise use the defaults (below) and state which they are in one line.

### E1 — Recon

Dispatch [subskills/recon](subskills/recon/SUBSKILL.md) (or run it yourself if it is small). Output goes to `evidence/recon.md` and to filling in `brief.md`: source, version (commit and/or snapshot date), surfaces, goals, known audience, identity (tokens, DESIGN.md, PRODUCT.md, docs), flows, constraints, and **what could not be observed**. In URL mode, save the snapshot HTML and record `fingerprint --url-snapshot`. Ask the user only about what materially changes the direction and cannot be inferred.

**Design history before diagnosis.** When the project has decision logs, changelogs, ADRs, per-version notes or similar, the recon also writes `evidence/design-history.md` (why each part exists, what the owner asked for, what was tried and rejected). Dispatch it as a separate task when the records are long. Every E2 worker and every E3 proposal reads it; proposals declare which settled decisions they touch.

### E2 — Specialized diagnostics

In **full**, run these diagnostics with the brief and recon as input:
- UX and structure → [subskills/ux](subskills/ux/SUBSKILL.md)
- Art direction → [subskills/art-direction](subskills/art-direction/SUBSKILL.md) (diagnostic mode)
- Motion and interaction → [subskills/motion-media](subskills/motion-media/SUBSKILL.md) (diagnostic mode)
- Accessibility and performance → [subskills/a11y-perf](subskills/a11y-perf/SUBSKILL.md)
- Seasonal fit → [subskills/seasonal](subskills/seasonal/SUBSKILL.md), including a recorded no-change result if nothing fits.

In **motion**, write `evidence/motion-media.md` using the addition-only pass. Skip the existing-motion cleanup audit and E5 removal/overstimulation critique. In **seasonal**, write `evidence/seasonal.md` after focused recon. Both focused workflows keep the technical checks relevant to the addition and omit the unrelated diagnostic reports. Use the [interactive behavior checks](subskills/a11y-perf/interaction-checks.md) on affected widgets and record the evidence in the active specialist report.

Each worker returns findings with evidence and confidence in `evidence/<role>.md`. You consolidate them in `evidence/consolidated.md`: duplicates merged, conflicts made explicit (e.g., art wants a background video, performance shows LCP is already poor), uncertainties listed. Dispatch: [references/dispatch.md](references/dispatch.md).

Record the individual checks for this run in `evidence/checks.json` when the Lab will be used. Derive each result from the recorded evidence, including checks that passed, were blocked, or led to no suggestion. Link only the checks that led to a recorded opportunity. The format is documented in [the Lab procedure](subskills/lab/SUBSKILL.md#diagnostic-checks).

E2 is never skipped silently. A role that truly does not apply is recorded with `jiu skip-stage R e2.<role> --reason "…"` (the reason reaches the user through `rulings`); recon and `consolidated.md` cannot be skipped.

**Stage gate.** [The CLI checks](references/state.md#stage-gate) recon, the selected workflow's reports and consolidation before accepting preview or implementation decisions. `next` lists the exact gaps.

### E3 — Proposals

1. Choose a few opportunities of highest value (up to `limits.maxOpportunities`). Weigh the observed problem or meaningful user moment, expected benefit, frequency and implementation cost. A working interaction can earn a small moment of delight on its own merits. Record each with `opp-add` (with `--paths` for the surface's files when there is code, to detect changes later) and `--depends`/`--conflicts` when there is a relationship.
2. Ask the specialists in scope for useful alternatives. Every opportunity already has `V0 — keep original`. One proposal is enough when it addresses the brief. Additional alternatives should represent meaningful differences in interaction, content or composition.
3. Each alternative is a file following [references/proposal.md](references/proposal.md). Workers write to `proposals/_drafts/`; you check the contract and record it with `variant-add` (`jiu` copies it to `proposals/O-xx/Vy/r1.md` and records the hash).
4. Continue to E5 for the full workflow, with E4 if the user has selected previews. In focused workflows, present the technically checked additions and proceed to E6 for the user's choice.

### E4 — Lab, only under selection

Precondition: the stage gate is open (E1–E3 complete, see E2) and `decide … preview` is recorded by the user for each revision. Read [subskills/lab/SUBSKILL.md](subskills/lab/SUBSKILL.md) and follow the controller section: V0 baseline, building the mocks (parallel and isolated, up to the limit), `preview-set` with declared fidelity, the lab server, and instructions for the user. Never connect demonstrations to real mutations, payments, message sending, or production data.

For motion alternatives in a recorded local mock, use the [Motion library or the element picker](subskills/lab/SUBSKILL.md#motion-library-in-the-lab), or use `jiu motion list --catalog` and `jiu motion stage`. Queue each suggestion against one verified element and the exact revision. Select a compatible effect, its preset and the intended product trigger. Use the variation request to tailor the treatment in a new mock to the user goal and this product's visual language. Follow the [motion design criteria](subskills/motion-media/SUBSKILL.md#small-moments-of-delight) and keep the source suggestion available. Test the real interaction in the mock before implementation approval.

### E5 — Set review

1. In full, have the [critic](subskills/critic/SUBSKILL.md) evaluate the proposed set after discovery. Review the rendered mock when available. Without a mock, review the proposal against the product baseline and label anything that needs a render to verify. For affected widgets, run the applicable [interactive behavior checks](subskills/a11y-perf/interaction-checks.md) on the rendered proposal and record any checks the mock cannot establish. Record with `reviewed` (use `--self` for a sequential review in the same session). In focused workflows skip the aesthetic pruning pass and verify the applicable feasibility and behavior checks.
2. Check dependencies and conflicts between selected opportunities. In full, also assess visual competition and combined motion. If the Lab is in use and several changes share a surface, use a composition (`composition-set`) to show interactions between them.
3. Present critiques and conflicts with reasons and evidence. In full, a critic may recommend retaining V0 or reducing or removing an animation. A recommendation leaves the product untouched. The user may approve an alternative the critic advised against after seeing the trade-off.

### E6 — Decision about a real change

Record individual decisions with `decide R O-01/V2/r3 implement --quote "<user's words>"`. One per opportunity (`--replace` to swap). "Keep original" is `keep-original`.

For batch approval, record a single compatible recommendation per opportunity with `recommend R <ref>` and show `batch R` as one list. The user can approve it once without opening each mock. Use `approve-all R --batch <id> --quote "<user's words>"` for chat/CLI approval, or ingest the explicit **Approve all for implementation** event from the Lab. A revised list needs fresh confirmation. Proceed to E7 only for the exact approved revisions.

### E7 — Implementation in the target

Read [subskills/implement/SUBSKILL.md](subskills/implement/SUBSKILL.md) and follow the controller section. In summary:
1. `check-stale`: if the surface changed since the evaluation, check compatibility before proceeding; revalidate (`fingerprint --revalidated`) or ask for a new revision.
2. Prepare isolation: with git, a dedicated branch `jiu/R-xxx` in the project's own checkout, created only from a clean working tree (never a worktree; never directly on the main branch without consent; never stash or commit the user's pending changes). Without git, confirm with the user where to apply and keep a backup copy of the files touched.
3. Turn the selection into tasks (`task add --files`), at most `limits.maxChangesPerBatch` per batch, and run the pre-flight scan for tasks that share files, tokens or interfaces. Dispatch one implementer per task; **never two implementers on the same files in parallel** (`task start` refuses). Only you commit, one reviewed task at a time.
4. Every task gets a task review (spec compliance + quality) before it closes, and a bounded fix loop with scoped re-reviews; at the cap, adjudicate and record each open finding with `jiu ruling`.
5. Reuse approved media and recipes from the mock; do not carry the lab shell (picker, guards, markers) into production.

### E8 — Verification and continuation

Read [subskills/verify/SUBSKILL.md](subskills/verify/SUBSKILL.md) (integration mode). Review diffs and the rendered experience (desktop, touch, reduced motion) against the proposal's acceptance criteria, with the deferred and parked items from `jiu rulings R`. Findings within the criteria go as one complete list to one implementer, followed by one scoped re-review (up to `limits.reviewRounds` rounds); never one fixer per finding. Mark each item as `verified`, or `blocked` with evidence; leave incomplete what is incomplete. Then `fingerprint` the implemented opportunities, `set nextAction`, and offer the next useful action based on the remaining state (another proposal, a new round, pending mocks, or closing with `set phase closed`). The report to the user always includes **every** ruling you made in this run (`jiu rulings R`), in order, each with what it costs if wrong: that list is how decisions taken on the user's behalf reach them. Publishing only with its own authorization.

## Default limits

| Limit (`limits.*`) | Default | Notes |
|---|---|---|
| `surfaces` | those the user cited; otherwise the home page and one main flow | record in the brief |
| `maxOpportunities` | 5 | prefer 3 strong over 5 average |
| `maxSeasonalOpportunities` | 3 | separate budget for full and seasonal workflows |
| `maxVariantsPerOpportunity` | 3 (+ V0) | |
| `maxConcurrentPreviews` | 4 | also the cap on simultaneous workers |
| `reviewRounds` | 2 | revisions per variant and correction rounds in E8 |
| `maxChangesPerBatch` | 3 | open tasks per batch in E7 |
| `allowedDependencies` | none new | each new dependency is the user's decision |
| paid media | not authorized, budget 0 | see [references/media.md](references/media.md) |

Change with `jiu set R limits.<key> <value>` when the user asks.

## Presenting to the user

Speak the user's language. Be concrete and brief in the chat; the detail is in the files.

After E3, for each opportunity:

```
O-01 · Home hero — the visitor does not understand what the product does before scrolling (evidence: recon §2, UX #3; medium confidence)
| Ref        | Alternative         | What changes for the person                      | Cost       | Risk/dependency   |
| O-01/V0/r1 | Keep original       | nothing                                          | —          | —                 |
| O-01/V1/r1 | Living demo         | the product screen assembles under the title on load | medium (CSS+SVG) | conflicts with O-03/V2 |
| O-01/V2/r1 | Editorial headline  | title rewritten and social proof above the fold  | low        | needs approved copy |
Details: proposals/O-01/
```

Offer batch approval, individual decisions and optional mock review. If the stage gate requests the seasonal offer on a resumed legacy run, offer seasonal work in one line and record `set R seasonal.offered true`. Record an explicit request with `set R seasonal.requested true --quote "…"`.

After E4/E5: the lab URL, what to compare, the fidelity of each mock, critiques and conflicts. After E8: what was verified (with evidence), what was blocked and why, what remains pending, and where the branch/diff is.

## No subagents

A skill does not create parallelism the environment does not offer. If there is no subagent tool, record `capabilities.subagents false`, tell the user in one line, and carry out the roles as named sequential passes ("Pass: critic"), each reading the corresponding sub-skill and writing to its path. Record reviews with `reviewed --self` and do not call them independent.

## Reference files

- [references/state.md](references/state.md) — state directory, schemas, `jiu` commands, resume, staleness, idempotency, archiving.
- [references/decisions.md](references/decisions.md) — decision and work states, transitions, authorizations, how to translate user remarks into commands.
- [references/proposal.md](references/proposal.md) — proposal contract and template.
- [references/dispatch.md](references/dispatch.md) — how to brief subagents, parallelism, failures, model.
- [references/input-modes.md](references/input-modes.md) — URL, code, both.
- [references/media.md](references/media.md) — media, libraries, engines, cost, Higgsfield.
- [references/scenarios.md](references/scenarios.md) — acceptance scenarios for testing this workflow.
- [NOTICE.md](NOTICE.md) — origin and license of the adapted reference skills.

---
name: jdeslop
description: Audit and surgically remove AI tells, slop smells, generic UI defaults and generated copy after visual direction is chosen for a design, prototype or frontend. Use for one component, a flow or a whole product. Inspect rendered evidence, then optionally review proposed fixes in a browser and approve exact source revisions individually or in a serial queue.
---

# jdeslop

Treat a smell as an observable pattern, never as proof of authorship. Keep the visual direction, product behavior and language established by the user, the Open Design artifact or the implementation. Work from those artifacts and their actual users. This pass has no reference hunt, identity design, design-system generation or added decorative motion.

## Run the audit and review together

For an audit with visual review, start one [jdeslop CLI run](references/coverage.md#combined-cli-run) before auditing. Complete the ledger, stage the proposed fixes, and open the panel for the user's decisions. Resume by pointing the CLI at the same run directory. Follow the [visual review workflow](references/review.md) to stage fixes or open a review of findings already prepared. For an audit alone, the ledger script can run independently.

## 1. Establish the baseline

- Resolve the target and the requested action. An audit reports. A request to fix also authorizes the evidenced, reversible corrections within the target. Record route families and shared surfaces for a site-wide request, including exceptions to shared templates. A single-page request limits edits to that page unless shared impact is also in scope.
- Read the supplied design, nearest `DESIGN.md` and page overrides, product/voice brief, tokens and shared components when available. If documentation is absent, infer provisional decisions from repeated working patterns and the user's instructions. Record low-confidence inferences. Documented choices are strong evidence of intent and remain eligible for narrowly justified change.
- Capture the starting render with actual content and states when available. Name the route, viewport, theme and interaction state. Use a small viewport, a wide viewport and any implicated breakpoint just below and above it. Extend to content extremes, keyboard, touch and reduced motion where relevant. A single screenshot supports visual observations at its own viewport. Claims about unseen layouts, runtime behavior, token usage or data provenance require other evidence.

## 2. Cover the catalog

Read the [audit catalog](references/catalog.md) and [detailed checks](references/detail-checks.md). The catalog includes broad patterns. The dotted IDs call for specific evidence under those checks. For each surface group, map applicable tests to real routes, widths, themes, user journeys and interaction states. Capture evidence once and cross-reference it across IDs and verified instances. A shared implementation can reuse evidence when route-specific layout, data or behavior cannot change the result.

1. Inspect source, design artifacts and visible copy. Run the project's existing lint/tests where relevant. If Impeccable Detect is available, use the installed version's help/rule listing and run its source and rendered scan options on the in-scope target. See [source notes](references/sources.md). Use scanner output as a candidate list, including computed-style clues from the real rendered page. Coverage of semantic patterns still requires judgment.
2. Inspect rendered hierarchy, typography, surfaces, composition, media and responsive states from matched screenshots and DOM/computed styles. Review early enough to catch a false diagnosis before editing.
3. Exercise applicable interactions and adverse states. Read the displayed copy in its own language, with the intended reader and task in mind. Verify substantive claims from available evidence. Correct a contradicted claim when the facts establish the replacement. For an unverified material public claim, identify the evidence needed and bring the claim to the user's attention for substantiation or a decision to qualify or remove it. Continue independent work.
4. Keep a [coverage ledger](references/coverage.md) with parent and detailed records per surface group. Use `pass` for evaluated with no issue, `exempt` for deliberate and working, `finding` for a confirmed issue, `n/a` when inapplicable, and `blocked` when evidence is unavailable. A whole family may share one reason only when the same inapplicability or blocker applies to every detail. Record confirmed findings and evidence gaps separately, including a summary of any unresolved gap in a parent finding handed to visual review. List the actual routes and scenarios tested.

## 3. Decide what earns a change

- Distinguish an evidenced defect, a confirmed generic or redundant default, and a candidate awaiting evidence. Ask what the element does for this user here. For decorative or redundant candidates, compare a render with it hidden or simplified. Keep it if it contributes to task, hierarchy, identity or useful feedback. Check if it repeats across unrelated content for no functional reason. A hue, font, pattern, punctuation mark or radius by itself is weak evidence.
- For every finding give its catalog ID, applicable detail IDs, exact location/state, evidence, user impact and smallest proposed fix. Rank by impact: blocking, high, medium, or low. Put confirmed fabrication and blocked user journeys ahead of aesthetic preferences. Use one finding per root cause and retain all proven locations and relevant IDs. Record material unknowns without awarding a percentage or predicting AI authorship.
- Prefer removing redundant material, restoring an existing token, adjusting a local role or correcting a shared component with known affected surfaces. When a documented token is itself the repeated defect, show the affected cases, compare a before/after using the smallest viable change, and check all affected families and themes before applying the shared change. For an explicitly approved choice implicated in a defect, try a local correction that keeps the approved intent. Bring changes that substantially alter that choice, shift the overall direction or expand the requested surface to the user for a decision. Documentation alone does not make a default an explicit approval.

## 4. Correct and verify

- When the user asks to review or approve corrections in a visual panel, stage them in the same CLI run and follow [visual review workflow](references/review.md). Use parent IDs in the panel manifest and keep detail IDs in the findings report and coverage ledger. Stage impacted pages and exact source edits before the approval. Respect the panel's per-revision decisions.
- Preserve valid text, meaning, semantics, facts, localizations and interaction states. A specific copy improvement must stay true to available evidence. Keep supported brand voice, useful animation and deliberate exceptions. Avoid new dependencies for stylistic fixes.
- Apply corrections through the actual source or artifact. Re-render matched before/after evidence, recheck each fixed ID and the neighboring checks it could affect, and retest consuming surfaces for shared changes. Re-run available deterministic scans using equivalent inputs. A lower scanner count alone does not establish a better result.
- Report scope, prioritized findings and per-finding resolution or proposed correction, before/after evidence when possible, accepted exceptions and blocked coverage. Keep the detailed ledger linked or available with the run. If a real comparison or runtime check was unavailable, state the limit and leave it blocked.

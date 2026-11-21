# Detailed audit checks

These checks make the evidence for the [catalog](catalog.md) explicit. Each dotted ID belongs to its catalog prefix. A parent covers the broader question, including patterns listed only there. Each detail calls for a decision or evidence that could differ from its siblings. Use the same locator whenever one test proves several checks. Assess only the artifact and user scope. See [source crosswalk](sources.md) for provenance.

## Color, surfaces and visual effects

| Detail ID | What to verify |
| --- | --- |
| VIS-01.palette-convergence | Compare role colors with the approved system and adjacent defaults. Flag a starter palette only when the combination and its use are interchangeable or drift from the brief. |
| VIS-02.accent-role | In the same viewport, check whether accent color directs the primary action or is competing across unrelated headings, cards, badges and controls. |
| VIS-02.semantic-role | Compare statuses across surfaces and themes. The same hue should retain its meaning, and a family of related hues should still distinguish the states. |
| VIS-04.combined-effects | Disable the redundant layers in a matched render. Check combinations of gradients, noise, patterns, floating art and framing for lost clarity or no useful difference. |
| VIS-06.underlay | Scroll a transparent or blurred surface over the lightest and darkest real content it can cover. Check both readability and separation between stacked layers. |
| VIS-07.elevation-role | Compare surfaces that are peers or at different elevations. Flag shadows, borders or rails that imply a hierarchy or state the product does not have. |
| VIS-08.radius-relationships | Render nested elements and compare buttons, inputs, panels and chips. Look for conflicting corners or a single shape that makes their roles harder to distinguish. |
| VIS-10.theme-completeness | Switch and reload each supported theme. Inspect native controls, browser selection and scrollbars where styled, logos, charts, media, overlays and all meaningful states. |
| VIS-11.hierarchy-after-removal | Check that simplification leaves active regions, semantic emphasis, useful product imagery and information boundaries discernible. |

## Typography and layout

| Detail ID | What to verify |
| --- | --- |
| TYP-01.script-fit | Render the actual supported scripts. Check unintended fallback, missing glyphs and awkward line rhythm in mixed runs, including CJK or Arabic where applicable. |
| TYP-02.role-drift | Compare computed size, weight and line height of equivalent roles across route families and themes. Disambiguate a legitimate contextual override. |
| TYP-03.wrap-extremes | Try real long entries and supported locales just above and below wrap thresholds. Check forced breaks, a one-word last line, overflow and the relationship to the action. |
| TYP-05.measure | Sample rendered body paragraphs in a narrow and a wide column. Inspect line measure, leading, tracking and justification together with script and reading context. |
| TYP-05.distinguishable-glyphs | Read real codes, IDs and prices at their actual size. Check ambiguous letters and digits or cramped number-and-unit pairs when a misread would cause an error. |
| TYP-06.font-loading | Check first paint under a cold load and after fonts settle. Verify the actual face and weight and record visible wrapping or layout changes. |
| LAY-01.content-swap | Read the hero with the product nouns masked, then inspect the page at thumbnail scale. Compare the composition and imagery with the actual task and content. Flag interchangeability when the structure could belong to any product or imports another product's recognizable identity without serving this one. |
| LAY-01.purpose-of-sections | Trace stock feature counts, three-step explainer, logo bar, FAQ and pricing/CTA sections to specific user questions and real data. Flag filler or padded sections with no unique job. |
| LAY-02.first-read | At each representative viewport, identify what receives first attention and compare it with what the user came to do. Check real result or task visibility in the initial viewport. |
| LAY-02.competition | Inspect the screen at thumbnail scale, then at normal size. Check whether promotional blocks, status, metrics or controls with equal weight distract from the focal task, especially on repeat-use pages. |
| LAY-02.handoff | Read section order and follow the action at the point the reader is ready. Flag a duplicated or misplaced action using LAY-07 when repetition is the cause. |
| LAY-03.section-rhythm | Compare adjacent and analogous sections for copied heading/card/action rhythm or unexplained dead space. Keep deliberate repetition that aids scanning. |
| LAY-04.container-necessity | Compare a simpler list, table or shared surface with repeated or nested boxes. Verify each remaining boundary identifies a genuine group, interactive object or comparison. |
| LAY-04.weight-by-content | Check equal tiles for unequal importance and arbitrary unequal bento cells. Test short and long real content for awkward holes or clipping. |
| LAY-05.proximity | Compare heading-to-body, peer and section gaps. Check whether closeness expresses belonging, including crowded interiors versus surplus exterior space. |
| LAY-06.gutters | Inspect optical edges, alignment of corresponding labels/values, and leading/trailing scroller gutters. Check full-bleed media separately from edge-touching text. |
| LAY-08.breakpoint-fit | Resize through the widths where content begins colliding. Check fixed-width children, unshrinkable grid tracks, squeezed multi-column grids and desktop sizing retained without reason. |
| LAY-08.overflow-source | Find the actual clipping or overflow owner. Inspect long unbroken content, media and controls, and restore reading and interaction instead of masking it at the document root. |
| LAY-08.stacked-chrome | Scroll at the affected widths with banners, headers and sticky controls active. Check offsets, anchors, focused items and layered content beneath them. |
| LAY-09.dynamic-viewport | Test mobile address-bar resize, rotation, safe-area insets and the open software keyboard. Ensure a focused field, its error and the submit action stay reachable. |
| LAY-09.zoom-reflow | Check browser text resizing and zoom using the applicable standard. Verify reachable content and actions, including within dialogs and sticky layouts. Document legitimate two-dimensional scrolling. |
| LAY-10.reading-order | Compare visible rearrangement and hidden content with DOM order, tab order and screen-reader progression. Verify the task and context still exist after reflow. |

## Components, imagery and authenticity

| Detail ID | What to verify |
| --- | --- |
| CMP-01.action-priority | Compare primary, secondary and destructive styles in an actual interaction state. Flag decorative affordances and repeated arrows that promise navigation or disclosure when neither occurs. |
| CMP-02.badge-purpose | For repeated chips, statuses, dots and count badges, verify what action or information each adds beyond its text, color or parent row. |
| CMP-02.pricing-emphasis | Confirm a featured plan badge conveys a truthful, useful distinction. Route an unsupported popularity claim to TRU-02. |
| CMP-03.icon-semantics | Check icon choice against the action, object or state. Inspect decorative emoji, sparkles and indiscriminate icon-plus-label pairs for ambiguity or redundancy. |
| CMP-03.icon-system | Compare repeated shapes for stroke, optical weight, alignment, family and size. Keep intentional variants such as selected versus unselected icons. |
| CMP-04.navigation-inventory | Trace header, footer and menu categories to actual product areas and real destinations. Check utility and low-frequency links separately from primary IA. |
| CMP-04.location-and-return | Reach a deep route through navigation and directly. Check whether the page itself identifies the product, current area and likely next action. Test breadcrumb links and a consistent way back on desktop and mobile. |
| CMP-04.link-behavior | For navigation, exercise meaningful links out of their original context and use the platform's normal open-in-new-tab, context-menu or equivalent actions. Flag generic names and controls that imitate links without supporting their behavior. |
| CMP-05.metric-relevance | Ask which decision each visible KPI or widget supports. Compare the primary operational measure with supporting metrics for prominence and density. |
| CMP-05.metric-context | Check that each displayed metric or delta gives a unit, time period, comparison baseline and an intelligible label where applicable. Verify provenance in TRU-02. |
| CMP-05.feed-relevance | Inspect activity feeds and widgets for events that matter to the user. Check whether generic events are decorative filler, and route claims of fake activity to TRU-03. |
| CMP-06.data-controls | Exercise sorting, filtering, pagination and row actions against known records. Verify changed rows, boundaries, filtered-empty recovery and coherent page reset when inputs change. |
| CMP-06.column-value | Check columns have a real use, clear labels, faithful empty values, correctly ordered numeric values, units, and accessible row/column relationships. |
| CMP-06.numeric-stability | Compare differing digits, live updates and values across rows. Check that alignment supports comparison and updates do not move adjacent content. |
| CMP-06.narrow-comparison | At compact widths, compare two real rows or items. See whether important columns and associated headers survive overflow, column prioritization or conversion to a list. |
| CMP-07.control-states | Compare a repeated component at rest, on hover, while pressed, selected, focused and unavailable where applicable. Check that equivalent states use recognizable styling and state-driven border, label or spinner changes do not move neighboring controls. |
| CMP-08.chart-encoding | Check the question and expected answer with known data. Verify axis labels, units, legend, meaningful scaling and whether multiple series or chart forms support the comparison. |
| CMP-08.chart-readability | At compact widths and with alternate input, check labels, tooltips, legends and series distinguishability. Verify that the essential comparison is available through text or accessible data where the chart alone cannot convey it. |
| CMP-08.missing-data | Render real zero, missing, delayed and partial values in the same chart where relevant. Check that gaps, aggregation and interpolation do not imply observations that were never recorded. |
| IMG-01.subject-fit | Ask whether replacing the image with interchangeable stock would preserve its meaning. Inspect irrelevant scenes, generic generated art, repeated photo walls and visually implausible detail. |
| IMG-02.product-evidence | Where product evidence is promised, compare the screenshot or demo with the reachable workflow, real data shape and current terminology. Check readable scale and honest labeling for schematic or demo views. |
| IMG-03.asset-crop | Render actual aspect ratios across the component's widths. Check focal content, legible screenshot text, distortion and consistency among a set of assets. |
| IMG-03.asset-integrity | Check loaded source, intentional fallback and accessible labels. Flag empty slots or gradients that look like unfinished media. Inspect required credit from available records. |
| TRU-01.endorsement-record | Trace customer logos, people, roles, ratings and quotes to supplied records or source material. Keep visibly illustrative examples separate from proof. |
| TRU-02.metric-lineage | Trace the number, timestamp or delta to its origination and calculation. Reconcile the numerator, denominator, time period and baseline against known data. |
| TRU-02.claim-evidence | Verify material comparisons, awards, security, compliance, performance and popularity claims. Record an evidence gap when support is unavailable. |
| TRU-02.billing-terms | Compare displayed plans, billing interval and amount due against authoritative data. Check seats, allowances, renewal, overages and trial or cancellation conditions where they affect the decision. Follow the same selection into checkout. |
| TRU-03.real-update | Trigger a known event or observe a known stale state. Verify live, recent, availability, scarcity and countdown labels use real state with an honest interpretation. |
| TRU-04.honest-placeholder | Inspect initial fields, rows, feeds, mock identities and showcases. Distinguish a value, a hint, sample data and the absence of a value at the point of use. |
| TRU-04.feature-fidelity | Follow the advertised feature or integration to its actual behavior. Ensure placeholders and disabled future features are visibly identified before users rely on them. |
| TRU-04.record-continuity | Follow a representative item through overview, list, detail and preview where it appears. Verify its identity, values and state agree or that the interface explains differences, such as a stale snapshot. |

## Copy and microcopy

| Detail ID | What to verify |
| --- | --- |
| CPY-01.product-specificity | Read the initial viewport without the product name. Check whether the promise identifies a concrete actor, task and outcome instead of interchangeably praising the category. |
| CPY-02.action-prediction | Predict what follows from the button or link name and activate it. Check destination, consequence and consistent verb choice across the flow. |
| CPY-03.redundant-lead | Read titles with their kickers and first sentences. Remove duplicated claims, announcements, throat-clearing and prose about the presentation itself when they delay the point. |
| CPY-04.staged-argument | Inspect invented counterarguments, reversal formulas, suspense and artificial closers. Preserve genuine distinctions, meaningful short lines and deliberate author voice. |
| CPY-05.repetitive-structure | Read neighboring surfaces consecutively. Check forced triples, repeated openers, rhythm built from dashes or fragments, hedging stacks and templates disguised by synonym swaps. |
| CPY-05.false-range | Check that from/to examples form a meaningful continuum or explicit set. Flag unrelated endpoints used only to exaggerate breadth. |
| CPY-06.unearned-authority | Find unsupported authority phrases, significance inflation, vague associations and trailing claims that add no demonstrable fact. Route verifiable claims to TRU-01 or TRU-02. |
| CPY-07.actor-and-verb | Check for hidden actors, inanimate actors given intent, and roundabout verbs that obscure the task. Retain passive voice when it helps users find the relevant object or result. |
| CPY-08.visual-emphasis | Review bold cycles, arbitrary quotation marks, all-caps slogans, generated inline-heading lists and decorative emoji in context. Keep recognizable emphasis and locale conventions. |
| CPY-09.draft-residue | Search user-visible text, metadata and real state messages for inaccessible instructions, draft notes, chat signoffs, generic fill and model disclaimers unrelated to the reader. |
| CPY-10.context-and-voice | Compare with real product copy in the same language. Check relevance to the reader's level and avoid false familiarity, invented anecdotes or a generic marketing register. |
| CPY-11.recovery-copy | Trigger an actual error or empty result. Check whether its copy identifies what happened, what persisted, and the next recoverable action with appropriate specificity. |
| CPY-12.locale-extremes | Render real plural, zero, long-label and formatted-number cases in supported locales. Inspect translated constraints and UI strings assembled from fragments. |
| CPY-13.edit-regression | Re-read corrected screens together against the supported brand voice. Check that a cleanup did not introduce fake specifics, mechanical short sentences or a second template voice. |

## Behavior, motion and accessibility

| Detail ID | What to verify |
| --- | --- |
| UX-01.real-activation | Activate rendered buttons, menus, cards, tabs and links. Check a real destination or state result, actual handlers and labeling of intentionally unavailable features. |
| UX-01.disabled-affordance | Reach meaningful disabled conditions using fixtures. Check whether the constraint and how to satisfy it are discoverable by keyboard and pointer without relying on a disabled control's hover. |
| UX-02.loading-and-slow | Induce a slow and a completed request. Verify distinct progress, stable layout, reachable cancellation when supported, and no stale loading indicator afterward. |
| UX-02.empty-and-filtered | Use a fresh empty data set and a populated set filtered to zero. Verify they are distinguishable and the relevant create, reset or clear path works. |
| UX-02.failure-and-retry | Reproduce failure with a controlled response. Verify what remains visible, what is officially saved and whether retry restores the right result once. |
| UX-02.offline-and-permission | For supported cases, simulate offline, reconnect and denied permissions. Verify the interface explains which operations are unavailable and recovers without pretending to succeed. |
| UX-03.field-contract | Inspect persistent labels, required status, helper and error association, defaults, input modes and realistic value extremes. |
| UX-03.field-necessity | Trace each visible prompt to a real task, policy or user choice. Flag repeated questions, unrelated fields and avoidable work to supply known values. |
| UX-03.input-assistance | Exercise autofill, password managers, paste and relevant keyboard submission behavior on supported platforms. Check that formatting, validation and custom handlers do not swallow or transform valid input unexpectedly. |
| UX-03.validation-timing | Exercise blank and invalid input, blur, submission and subsequent corrections. Check that feedback appears at a useful time without blocking unfinished input or allowing invalid submission. |
| UX-03.submission-outcome | Submit once, repeat during pending, simulate rejection, then succeed. Verify no duplicate mutation, input retention on failure and confirmation tied to an actual successful request or durable result where promised. |
| UX-04.primary-path | Complete a representative task without knowing the interface. Check unnecessary actions, choices defeated by hierarchy, and recovery that needlessly diverts the user. |
| UX-04.decision-context | At each meaningful decision, list what the user needs to know. Check that relevant facts, prior selections and consequences are visible when needed and that optional controls neither dominate nor become undiscoverable. |
| UX-04.navigation-continuity | Exercise back, forward, direct links and reload for relevant flows. Check whether selection, scroll position, filters and saved state are restored or reset according to the product contract. |
| UX-04.onboarding-reentry | Run onboarding as a new and returning user. Check real progress, premature permissions or forced optional steps, interrupted session recovery and a visible path to the first useful result. |
| UX-04.plan-comparison | Compare two real plans across viewports and with keyboard navigation. Check decision-relevant differences, readable aligned terms, predictable plan selection and where it leads. |
| UX-04.purchase-continuity | Select a real product variant and change delivery or quantity where supported. Follow it from listing to cart and verify visible price, availability, restrictions and selected options, including recovery for an unavailable combination. |
| UX-04.settings-consequences | Change and revisit a preference. Check the visibility of its consequence, whether it saves immediately or requires an action, and whether the visible state matches persisted state. |
| UX-05.interruption | Exercise leaving, cancelling or dismissing with unsaved work, then retry. Check preservation, predictable reset or an explicit consequence according to the task. |
| UX-05.reversible-action | Test the consequence of meaningful destructive actions using safe data. Verify the intended guard or undo works and low-risk actions avoid unnecessary friction. |
| UX-05.modal-necessity | Walk through actions gated by a dialog. Check whether modality is justified, nesting adds an avoidable decision, or background and foreground scrolling lead to lost context. Test keyboard mechanics through A11-02. |
| UX-06.touch-equivalence | Test primary tasks without hover. Check navigation discoverability, nested scrollers and whether the equivalent tap gesture or visible action gives useful feedback. |
| UX-06.gesture-cancellation | On a real or synthesized touch path, drag, interrupt and resume where supported. Check scroll competition, cancellation, stuck pressed state and useful feedback after pointer loss. Record the input method used. |
| UX-06.native-conventions | For native or hybrid targets, check the system back gesture, tab/navigation idiom, scrolling, text scale and insets rather than judging solely from a web screenshot. |
| MOT-01.reveal-and-focal | Compare the actual page at rest and while scrolling. Flag default reveals and broad movement that hides usable content or animates everything equally. Check that enhancement code failing to run does not leave rendered content invisible. |
| MOT-02.feedback-cause | Identify the real trigger and useful end state. Check idle, loading, success and delayed states for loops, pulses, ticker movement and celebration without useful information. |
| MOT-03.timing-by-intent | Compare durations, easing and distances by semantic job. Check that confirmation, state changes and transitions remain responsive and do not all inherit a blanket motion recipe. |
| MOT-03.spatial-origin | Open a menu, popover or related surface from its trigger. Check whether animated placement or origin makes the relationship unclear when aligned, flipped or constrained by the viewport. |
| MOT-03.interrupt-and-reverse | Rapidly repeat, reverse or exit an animation. Verify control responsiveness, final state, focus and layout under interruption, including input during an exit. |
| MOT-04.reduced-motion | Use the system preference on a fresh load, then exercise the same actions. Check animated CSS, script-driven motion and media for reduced movement with intact information and feedback. |
| MOT-04.moving-content-controls | For auto-playing or auto-updating movement subject to an applicable requirement, verify meaningful pause, stop or hide controls and that the interaction retains user intent. |
| MOT-04.flash-safety | Check flashing or rapid luminance changes in the actual rendered effect against the applicable accessibility criterion. Include repeated feedback and embedded media where relevant. |
| A11-01.text-contrast | Measure effective foreground against the weakest real background across supported states and themes, including images, gradients and translucent layers. Classify size per the applicable standard. |
| A11-01.non-text-contrast | Inspect necessary control boundaries, icon marks, focus and selected indicators against adjacent colors. Apply the relevant contrast criterion and its exceptions. |
| A11-01.color-independence | Inspect errors, required status, health, series and selection without relying on hue alone. Check visible supporting text, shape or other redundant signals. |
| A11-02.keyboard-path | Use Tab and Shift+Tab, expected activation keys, arrows where appropriate and Escape. Verify logical order, predictable scope, focus restoration and absence of traps. |
| A11-02.focus-states | Measure visible keyboard focus while scrolling and across themes. Check replaced outlines, sticky occlusion and immediate feedback during or after transitions. |
| A11-03.semantics | Inspect roles, names and values for landmarks, controls, links, headings, dialogs and table relationships with a rendered accessibility tree when available. |
| A11-03.live-feedback | Trigger dynamic errors, results, counts and meaningful progress. Check that status is announced when needed without stealing focus or reading decorative updates. |
| A11-03.accessible-media | Inspect meaningful image descriptions and applicable caption, transcript and audio-description requirements. Test controls and verify decorative media stays out of the reading path. |
| A11-04.targets | Measure target geometry and separation on actual rendered touch layouts, including icon-only and adjacent actions. Use the applicable criteria and exceptions instead of a universal target size. |
| A11-04.input-equivalence | Test a coarse pointer and keyboard, including tooltips and drag/reorder controls. Verify an available alternative for required actions and that hover/focus content can be used or dismissed as applicable. |
| A11-05.mixed-direction | Render RTL UI and mixed values. Verify language and direction metadata, directional icons, logical spacing, neutral punctuation, signed values and input direction separately from display. |

## Design-system drift and implementation

| Detail ID | What to verify |
| --- | --- |
| SYS-01.semantic-tokens | Compare rendered values to the intended token role and declared override. Identify default tokens used for the wrong role, arbitrary near-duplicates and unsupported variants. |
| SYS-02.shared-path | Trace repeated variants to the real runtime component and CSS path. Check one-off forks, class conflicts, overrides, and fixes present in one consumer only. |
| SYS-03.runtime-wiring | Verify the actual render path and build for broken imports, placeholder handlers and swallowed failures. Compare preview behavior with the maintained implementation. |
| SYS-03.render-path-integrity | Inspect shipped code when the render only works after a transient script, source postprocessing, copied visual overlay or other workaround. Confirm the intended result comes from a maintained build or artifact and survives the normal entry point. |
| SYS-04.comment-redundancy | Review changed or implicated comments for line narration, banners, empty labels, irrelevant emoji, closing markers and docs that merely echo the signature. Keep intent, contracts, exceptions and legal notices. |
| SYS-04.unactionable-todo | Check generated TODOs and comments claiming future work. Preserve concrete obligations and remove or clarify notes that convey no actionable task. |
| SYS-05.asset-load | Check actual media and font delivery, posters, dimensions, deferred hero assets, missing fallbacks and layout stability at realistic load conditions. |
| SYS-05.motion-cost | Inspect paint, layout and input responsiveness when expensive effects or transitions run. Confirm a real cost on the target conditions before treating a stylistic effect as a failure. |
| SYS-05.render-cost | Profile a representative data load or interaction on the target conditions. Check avoidable main-thread work, repeated renders, redundant requests and shipped dependencies when there is evidence of a bottleneck. |
| SYS-06.shared-regression | Compare before and after on affected route families, themes and states. Recheck neighboring constraints such as focus, reading order, clipping and useful feedback. |
| SYS-07.production-residue | Compare real routes, document title, icon and social metadata with intended content. Check resource and console errors, dev placeholders and public/auth boundary behavior when in scope. |

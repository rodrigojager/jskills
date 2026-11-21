# Sources and crosswalk

This skill synthesizes checks in its own words. Upstream repositories below are research references and optional tool sources. No upstream code, preset or style library is bundled. Source coverage refers to the relevant audit criteria. Some projects are separate creative workflows with aesthetic constraints that conflict with this skill's baseline-preservation goal.

| Source | What the catalog or workflow carries forward |
| --- | --- |
| [Impeccable Detect registry](https://github.com/pbakaus/impeccable/blob/b49177d32788f2dc585c42831d845dacf1ebed2c/crates/foundation/src/registry.rs), [Critique](https://github.com/pbakaus/impeccable/blob/b49177d32788f2dc585c42831d845dacf1ebed2c/.agents/skills/impeccable/reference/critique.md), [Audit](https://github.com/pbakaus/impeccable/blob/b49177d32788f2dc585c42831d845dacf1ebed2c/.agents/skills/impeccable/reference/audit.md) | Exact built-in rule crosswalk below, plus distinct rendered design review and technical verification. Baselines, personas, accessibility, integrity, performance and responsive checks inform the passes. |
| [muris11/anti-ai-slop](https://github.com/muris11/anti-ai-slop/blob/5768372bf3272c7d59c354cec3a16f5c39d036bc/skills/antislop-master/SKILL.md) | Master taxonomy plus specialist checks for layouts, mobile, dashboards, forms, navigation, motion, assets, authenticity, accessibility, copy and implementation. Purpose/convergence tests and differentiated treatment of taste, missing behavior and fake proof. |
| [frontend-design-deslop](https://github.com/samber/cc-skills/blob/50a33836b20f690c668af557bd9ef9d07a8c7ef2/skills/frontend-design-deslop/references/slop-checklist.md) | Convergent type/palette, overlapping and clipped content, scroller gutters, text spacing, visual artifacts, states, theme delivery and real-browser quality gates. |
| [Kill AI Slop](https://github.com/yetone/kill-ai-slop/blob/f6e2ae32b30443ec7bd0da4da971ee18d8f8ffcb/skill/references/taxonomy.md) | All 35 classic/evolved checks mapped below. Candidate screening, explicit triage, shared-component correction, small diffs and re-scan. |
| [Meng To's audit](https://github.com/mengto/skills/blob/3ec2dc0857d36e45d032d10063f62f2c773329a3/agent-skills/ui/audit-ai-design-slop/SKILL.md) | Evidence limits, test with decoration removed, deliberate choice vs weak purpose, confirmed defects vs patterns, and no numeric slop or authorship score. |
| [Hallmark gates](https://github.com/Nutlope/hallmark/blob/04977e15f3dfd6ee4b74c887dac3eb52665d6c92/skills/hallmark/references/gates.md) | Evaluate structural and rendering defects ahead of aesthetic signals. Pre-edit critique, distinguish measurable from judged issues, justify exceptions, and inspect widths and actual UI states. A project's own aesthetic supplies the threshold for taste decisions. |
| [Taste](https://github.com/Leonxlnx/taste-skill/tree/f6b76575f949707b4e3c37cd86cb44b0b98643e8/skills/taste-skill/references) | Finish gate, layered cards, icon scale and alignment, asset fit, motion cost, real content, second-generation copy tells and anti-slop overcorrection. |
| [No Slop Design](https://github.com/agshinrajabov/no-slop-design/blob/ae2ecc20970ff982a7f2cc7b145d859e1df9ef21/references/anti-slop.md) | Starter styling, overcorrection, material/placeholder checks, restrained diffs, cross-screen sameness and narrow-width verification. |
| [gstack Design Review](https://github.com/garrytan/gstack/blob/ebc867422e9c9e059657ee16d424f22e414a5e90/design-review/SKILL.md) | Representative page families, baseline capture, actual browser checks, fix order, screenshot regression and retesting shared changes. |
| [Stitch2 consistency](https://github.com/ifokeev/stitch2/blob/e9b31efa7c24dacc516d3ff6006309d9dda235fe/skills/stitch2-consistency/SKILL.md), [Stitch2 copy](https://github.com/ifokeev/stitch2/blob/e9b31efa7c24dacc516d3ff6006309d9dda235fe/skills/stitch2-copy/SKILL.md) | Compare page overrides against design tokens, keep shared components consistent, and review terminology, claims, task labels and translation strings. |
| [UI/UX Pro Max](https://github.com/nextlevelbuilder/ui-ux-pro-max-skill/blob/2de1134dc4d1cfe112934611ae5803ae17a1bd7e/.claude/skills/ui-ux-pro-max/SKILL.md), [Google DESIGN.md](https://github.com/google-labs-code/design.md/tree/db977b3c1d1f309269f43d5d17ae828d54bd6ae3) | Quality constraints for states, accessibility, responsive and interaction, and use of declared design values and overrides as evidence. |
| [Humanizer](https://github.com/blader/humanizer/blob/a32f24407364c74c812b43c15926d638bee1e072/SKILL.md) | Rhetorical staging (CPY-03/04), artificial rhythm (CPY-05), inflation and syntax (CPY-01/06/07), formatting (CPY-08), draft fragments (CPY-09), wrong audience and preservation of voice (CPY-10/13). |
| [Slop Detector](https://github.com/hayattcodes/Slop-Detector/blob/33acfc85017d130afbb6efc776e3e9b99b396f13/rules.js), [slop-detect](https://github.com/ravidsrk/slop-detect/blob/dd70b1c8aaf2b03dc1143f052cbf9021cc24a64d/spec/patterns.md) | Computed-style clues, evolved fingerprints, template chrome, unthemed browser surfaces, chip colors, spotlight, ticker and fake terminal patterns. |
| [Unslop](https://github.com/heydamir/unslop/tree/620163eedb3529d9cd23a3994ed8d287d06f760d/skills/unslop) by Damir Sokolovsky, [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) | Audit observations adapted into local evidence checks for bidirectional content, data displays, spacing, script legibility, product previews and copy. See the crosswalk below. |
| [AI Slop Detector](https://github.com/DefiTanjiro/ai-slop-detector/blob/f9ccb5bc4666cec9c0e08262497c631d7466f4d7/references/patterns.md), [Design Slop](https://github.com/wpgaurav/design-slop/blob/762729ec0031c50260992cef8e7ed2562bb66f31/references/slop-patterns.md), [Pixelcraft](https://github.com/voyager67/Pixelcraft/tree/4ce391ab8fdff73a2bdf0027789b0072c5e2d634/skills) | Filler structure, repeat templates, misleading proof, motion, implementation leftovers, evidence-first review and regression against approved decisions. |
| [Frontend Craft](https://github.com/nattergabriel/frontend-craft/tree/c0e0cea5244ed36a8c4b7b76c27dfc4e5dc01011/skills/frontend-craft/references), [Noslop Design](https://github.com/lidge-jun/noslopdesign/blob/77d6a5349d43f5daf9f98ec95d2256417b763da1/skills/noslop-frontend/references/core/anti-slop.md), [Avoid AI Design](https://github.com/funboy322/avoid-ai-design/blob/60f5853a706873da3a30b3ddf3cc29f9683c885c/references/ai-tells-catalog.md) | Practical input, rendering and performance checks, newer default combinations, actual script-fit and comparison of second-order defaults. |
| [Open Design Critique Theater](https://github.com/nexu-io/open-design/blob/main/plugins/_official/atoms/critique-theater/SKILL.md), [0xdesign Design Lab](https://github.com/0xdesign/design-plugin/blob/main/design-and-refine/skills/design-lab/SKILL.md) | Supporting review methods for independent checks against the active design system and comparisons of a local edit with the approved baseline. |

## Impeccable Detect

The registry snapshot above contains 62 built-in rules. Their IDs are mapped once below to their closest primary check. The mapping does not reproduce the detector's implementations, scoring or heuristic thresholds. The installed version's own output can include new rules and local packs. Triage unmapped findings as supplemental evidence, without silently counting them as native catalog rows.

Examples for versions supporting these flags (confirm with the installed CLI's help):

```sh
impeccable detect --list-rules --json
impeccable detect --json .
impeccable detect --json http://127.0.0.1:<port>/<route>
```

| Catalog ID | Built-in detector IDs |
| --- | --- |
| VIS-01 | `ai-color-palette`, `cream-palette` |
| VIS-03 | `gradient-text` |
| VIS-04 | `radial-halo`, `radial-spotlight-glow`, `codex-grid-background` |
| VIS-05 | `dark-glow` |
| VIS-07 | `side-tab`, `border-accent-on-rounded`, `hard-offset-shadow`, `gpt-thin-border-wide-shadow`, `repeating-stripes-gradient` |
| TYP-01 | `overused-font`, `italic-serif-display` |
| TYP-02 | `flat-type-hierarchy` |
| TYP-03 | `oversized-h1`, `straggler`, `unbalanced-heading` |
| TYP-04 | `hero-eyebrow-chip`, `kicker-above-heading`, `numbered-section-labels` |
| TYP-05 | `extreme-negative-tracking`, `line-length`, `tight-leading`, `justified-text`, `tiny-text`, `undersized-ui-text`, `all-caps-body`, `wide-tracking` |
| LAY-04 | `nested-cards`, `icon-tile-stack` |
| LAY-05 | `monotonous-spacing`, `cramped-padding`, `heading-rhythm` |
| LAY-06 | `edge-flush-cards`, `first-viewport-column-overflow`, `body-text-viewport-edge` |
| LAY-07 | `repeated-container-text` |
| LAY-08 | `text-occlusion`, `text-overflow`, `clipped-overflow-container` |
| IMG-01 | `shape-assembled-illustration`, `organic-clip-path` |
| IMG-02 | `buried-raster` |
| IMG-03 | `broken-image` |
| TRU-03 | `pulsing-dot` |
| CPY-01 | `marketing-buzzword` |
| CPY-04 | `aphoristic-cadence`, `theater-slop-phrase` |
| CPY-05 | `em-dash-overuse` |
| MOT-01 | `content-hidden-at-rest` |
| MOT-02 | `blinking-cursor`, `marquee` |
| MOT-03 | `bounce-easing` |
| A11-01 | `gray-on-color`, `low-contrast` |
| A11-03 | `skipped-heading` |
| SYS-01 | `design-system-font`, `design-system-color`, `design-system-radius`, `design-system-font-size` |
| SYS-07 | `script-error` |

## Kill AI Slop

Each upstream taxonomy entry has a primary catalog destination. Slash-separated references are cross-cutting checks.

| Upstream entries | Catalog destinations in the same order |
| --- | --- |
| 01–05 | VIS-01, VIS-03, VIS-01, VIS-02, VIS-02 |
| 06–10 | VIS-04, TYP-01, TYP-01, TYP-04, TYP-04 |
| 11–15 | TYP-03, TYP-02, TYP-04, CPY-04, CPY-08 |
| 16–20 | TRU-03, VIS-07, CMP-03, VIS-06/VIS-08, VIS-07 |
| 21–25 | VIS-08, VIS-07, CMP-02, CMP-03, CMP-03 |
| 26–30 | MOT-03, MOT-03, LAY-04/TYP-04, CMP-05/TRU-02, TYP-04 |
| 31–35 | LAY-04, LAY-05, TYP-01, VIS-09/IMG-02, CMP-05 |

## muris11 master taxonomy

The table follows the 97 numbered rows in the cited master snapshot, including the upstream duplicate for gradient text. Specialist references add details within those checks.

| Upstream entries | Catalog destinations in the same order |
| --- | --- |
| A-1 to A-15 | VIS-01, VIS-03, VIS-03, VIS-01, VIS-01, VIS-05, VIS-01, VIS-01, VIS-06, VIS-06, VIS-04, VIS-04, VIS-03, VIS-07, VIS-02 |
| B-1 to B-9 | TYP-01, TYP-03, TYP-01, TYP-04, VIS-03, TYP-05, TYP-02, TYP-04, TYP-05 |
| C-1 to C-12 | LAY-01, LAY-03, LAY-02, LAY-04, LAY-04, LAY-03, LAY-04, LAY-07, LAY-01, CMP-05, LAY-05, LAY-01 |
| D-1 to D-11 | VIS-08, VIS-08, CMP-03, VIS-07, VIS-03, MOT-01, CMP-04, LAY-07, CMP-02, CMP-04, LAY-04 |
| E-1 to E-8 | CPY-01, CPY-07, CPY-01, CPY-01, TRU-01/TRU-02, CPY-01, CPY-03/CPY-05, CPY-01 |
| F-1 to F-6 | MOT-01, MOT-01, MOT-02, MOT-02, MOT-01, MOT-03 |
| G-1 to G-11 | UX-01, UX-01, UX-02, UX-02, UX-02, UX-03, UX-02, UX-02/UX-05, A11-02, A11-02, TRU-04 |
| H-1 to H-5 | TRU-02, TRU-01, TRU-01, TRU-02, TRU-02 |
| I-1 to I-5 | SYS-01, LAY-05, LAY-02, SYS-01, VIS-10 |
| J-1 to J-7 | SYS-02, SYS-02, SYS-01, SYS-03, A11-03, SYS-02, SYS-04 |
| K-1 to K-8 | LAY-08, LAY-08, A11-04, UX-06, LAY-08, UX-06, UX-06, LAY-01/UX-06 |

## muris11 core and specialists

All 17 modules were reviewed at the same [pinned revision](https://github.com/muris11/anti-ai-slop/tree/5768372bf3272c7d59c354cec3a16f5c39d036bc/skills). The master entries above remain the exact crosswalk for the numbered taxonomy. This table locates the practical audit coverage for the other modules. Dotted IDs are evidence checks from [Detailed audit checks](detail-checks.md). Multiple sources can point to one ID.

| Module | Audit destinations |
| --- | --- |
| `antislop-master` | The 97-entry crosswalk above. |
| `antislop` | C-1–C-5 inform the baseline, intentionality, functional completeness, real-content fit, resilient states and claim verification. The 38 individual rules are mapped below. |
| `antislop-authenticity` | CPY-01.product-specificity, TRU-01.endorsement-record, TRU-02.metric-lineage, TRU-02.claim-evidence, TRU-04.honest-placeholder. |
| `antislop-code` | SYS-04.comment-redundancy and SYS-04.unactionable-todo. Preserve contracts, rationale and consequential caveats. |
| `antislop-copywriting` | CPY-01 and CPY-03–CPY-10, including subject and verb, false ranges, nested formulas, draft residue and unjustified claims under TRU-01/TRU-02. |
| `antislop-dashboard` | CMP-05.metric-relevance, CMP-05.metric-context, CMP-05.feed-relevance, CMP-06.data-controls, CMP-06.column-value, CMP-08.chart-encoding, TRU-02.metric-lineage, UX-02. |
| `antislop-designsystem` | SYS-01.semantic-tokens, SYS-02.shared-path, VIS-08.radius-relationships, VIS-10.theme-completeness, LAY-02.competition. |
| `antislop-forms` | UX-01.real-activation, UX-01.disabled-affordance, UX-02, UX-03.field-contract, UX-03.validation-timing, UX-03.submission-outcome, CPY-11.recovery-copy, A11-02. |
| `antislop-human` | A11-01.text-contrast, A11-01.non-text-contrast, A11-01.color-independence, A11-02.keyboard-path, A11-02.focus-states, LAY-09.zoom-reflow, LAY-09.dynamic-viewport, UX-02. |
| `antislop-imagery` | VIS-04, VIS-09, CMP-03.icon-semantics, CMP-03.icon-system, IMG-01.subject-fit, IMG-02.product-evidence, IMG-03.asset-crop, IMG-03.asset-integrity, A11-03.accessible-media. |
| `antislop-layout` | LAY-01.content-swap, LAY-01.purpose-of-sections, LAY-02.first-read, LAY-02.handoff, LAY-03.section-rhythm, LAY-04.container-necessity, LAY-04.weight-by-content, LAY-05.proximity, LAY-06.gutters. |
| `antislop-layoutmobile` | LAY-08.breakpoint-fit, LAY-08.overflow-source, LAY-08.stacked-chrome, LAY-09.dynamic-viewport, LAY-09.zoom-reflow, UX-06.touch-equivalence, A11-04.targets. |
| `antislop-mobile` | LAY-08, LAY-09, LAY-10.reading-order, UX-06.touch-equivalence, UX-06.native-conventions, A11-04.targets. |
| `antislop-motion` | MOT-01.reveal-and-focal, MOT-02.feedback-cause, MOT-03.timing-by-intent, MOT-04.reduced-motion, SYS-05.motion-cost. |
| `antislop-nav` | CMP-04.navigation-inventory, CMP-04.location-and-return, UX-01.real-activation, LAY-02.handoff, LAY-07, LAY-08.stacked-chrome. |
| `antislop-ui` | VIS-01–VIS-11, TYP-01, LAY-01–LAY-05, CMP-01–CMP-08, IMG-01–IMG-02, TRU-01–TRU-04, UX-01–UX-02 and MOT-01–MOT-03. Its dashboard, live-state and navigation passages reinforce the specialist tests above. |
| `slop` | Skill routing adds no separate finding. Its personal bans on dots, sparkles, uppercase and pills map to conditional checks under CMP-02, CMP-03, TYP-04 and VIS-08. |

Core rules in original order. Each cell lists the destination for each numbered rule.

| Core rules | Catalog destinations in the same order |
| --- | --- |
| R-01 to R-10 | VIS-01/VIS-03, CPY-01–CPY-13, LAY-08–LAY-10/UX-06, CMP-03, LAY-01–LAY-06, TYP-01–TYP-06, VIS-04, CMP-01, CMP-02, VIS-06 |
| R-11 to R-20 | VIS-08, VIS-07, VIS-05, LAY-04, CMP-01/LAY-07, CPY-01/CPY-07, TRU-02, TRU-01, MOT-01–MOT-04, LAY-01/SYS-01 |
| R-21 to R-30 | VIS-10, IMG-01, IMG-02/IMG-03/TRU-04, CMP-04/UX-01, A11-01, UX-01, UX-02/UX-03, LAY-01/LAY-07, VIS-01/VIS-02, LAY-01.content-swap |
| R-31 to R-38 | Baseline and evidence workflow, A11-02, SYS-03.render-path-integrity, VIS-10.theme-completeness, verification workflow/SYS-06, TRU-01/TRU-02, existing direction as baseline, TRU-04.honest-placeholder |

The audit carries forward the observed failure behind R-33, such as a visual repair that disappears in the maintained app. It permits intentional scripting. The creative direction requirements in R-23 and R-37 supply baseline context only. Fixed style bans, effect budgets, prescribed theme counts, device lists and house sizes do not override the project's intent or the applicable accessibility criteria.

## Additional audit sources

These destinations identify the reused criteria and where further detail was added. Workflow-only and positive presets do not create new check IDs.

| Source | Audit destinations and disposition |
| --- | --- |
| Impeccable [critique rubric](https://github.com/pbakaus/impeccable/blob/b49177d32788f2dc585c42831d845dacf1ebed2c/.agents/skills/impeccable/reference/critique-rubric.md) and Audit | LAY-02.competition, UX-04.decision-context, UX-06.gesture-cancellation, MOT-04.reduced-motion, MOT-04.flash-safety, A11-01–A11-05, SYS-05.render-cost and SYS-03.render-path-integrity. Compare manual and detector evidence. Do not reuse a detector or design health score as a verdict. |
| Frontend Design Deslop (Samber) | TYP-05.measure, LAY-06.gutters, LAY-08.overflow-source, LAY-08.stacked-chrome, VIS-10.theme-completeness, CMP-07.control-states, IMG-03.asset-crop, MOT-01.reveal-and-focal, MOT-03.spatial-origin, SYS-03 and SYS-07. Preserve style exceptions when the rendered defect is absent. |
| Meng To [pattern library](https://github.com/mengto/skills/blob/3ec2dc0857d36e45d032d10063f62f2c773329a3/agent-skills/ui/no-ai-design-slop/ARTICLE.md) and audit | VIS-04.combined-effects, VIS-11.hierarchy-after-removal, LAY-01–LAY-05, IMG-01/02, TRU-01–TRU-04, MOT-01/02, SYS-07. Inspect with the effect removed and check if its information or identity contribution disappears. |
| Hallmark | VIS-01.palette-convergence, VIS-02.accent-role, TYP-01, TYP-03.wrap-extremes, TYP-05.measure, LAY-02.first-read, LAY-08.stacked-chrome, TYP-06.font-loading, CMP-07.control-states, UX-03.submission-outcome, MOT-03, SYS-05.asset-load and SYS-07. Exercise tabs and anchor-related scroll behavior instead of assuming a specific implementation is wrong. Leave design decks and aesthetic cutoffs to the creative pass. |
| Taste and No Slop Design (Agshin Rajabov) | VIS-01/VIS-11 for the replacement cliché and sterile overcorrection. LAY-01, CMP-03.icon-system, IMG-02.product-evidence, TRU-04, CPY-04/05/13 and MOT-03. Treat presets as creative input, not violations. |
| gstack Design Review | CMP-04.location-and-return translates the Trunk Test. LAY-02.competition, TYP-02.role-drift, CMP-06.numeric-stability, UX-02, LAY-08 and SYS-06.shared-regression translate the rendered checklist and reassessment. Keep actual route families in evidence. |
| Stitch2 copy and consistency | CPY-02, CPY-06, CPY-10–CPY-13, SYS-01.semantic-tokens and SYS-02.shared-path. Check wording, states and token roles across real surfaces. |
| UI/UX Pro Max | A11-01–A11-04, UX-02, UX-06, LAY-08, VIS-10, MOT-04 and SYS-01. Use its pre-delivery checks without generating a design system or motion defaults. |
| Slop Detector and slop-detect | VIS-01–VIS-10, TYP-01–TYP-05, LAY-01/04, CMP-02/03, IMG-01/02, TRU-01/03, MOT-01/02 and SYS-01. Multiple selectors or generations of a tell use the same destination when the evidence and remedy match. |
| AI Slop Detector (DefiTanjiro) and Design Slop (wpgaurav) | LAY-01.purpose-of-sections, CMP-04.navigation-inventory, CMP-05.metric-relevance, IMG-02.product-evidence, TRU-02.claim-evidence, TRU-03.real-update, UX-01, UX-02 and SYS-07.production-residue cover template, placeholder and release patterns. Use catalog roles and author intent for cosmetic clusters. |
| Pixelcraft | Its shared review guidance reinforces baseline and verification. Form, modal, onboarding, pricing, settings, retail, table and chart criteria map to UX-03.field-necessity, UX-03.input-assistance, UX-03.submission-outcome, UX-05.modal-necessity, A11-02, UX-04.onboarding-reentry, UX-04.plan-comparison, TRU-02.billing-terms, UX-04.settings-consequences, UX-04.purchase-continuity, CMP-06.narrow-comparison and CMP-08.missing-data. |
| Frontend Craft | UX-03.input-assistance, CMP-04.link-behavior, UX-04.navigation-continuity, TYP-05.distinguishable-glyphs, A11-03.live-feedback, CMP-08.chart-readability, SYS-05.asset-load and SYS-05.render-cost. Verify the effect of a pattern on the supported platform instead of applying its default dimensions and timings. |
| Noslop Design (Lidge Jun) | TYP-01.script-fit, TYP-03.wrap-extremes, VIS-01.palette-convergence, VIS-04.combined-effects, VIS-06.underlay, LAY-01.content-swap and TRU-04.feature-fidelity. Inspect coherent use of material or a single hue before classifying the palette as an unexamined default. |
| Avoid AI Design | Second-order clusters map to VIS-01/VIS-03/VIS-09/TYP-04/LAY-01/CMP-04. Its silhouette test maps to LAY-01.content-swap, interactions and assets to UX-01/02, MOT-01–MOT-04 and IMG-01–IMG-03, shipped fingerprints to SYS-04/SYS-07, and cross-page drift to SYS-01/SYS-02. |

### Humanizer

The 26 numbered patterns from the pinned SKILL.md map in order below. Prefer a minimally edited draft that keeps meaning and the product's voice. A specific punctuation choice is weak evidence unless repeated convention or emphasis harms reading.

| Humanizer patterns | Catalog destinations in the same order |
| --- | --- |
| 1–5 | CPY-04, CPY-04, CPY-04, CPY-03, CPY-04 |
| 6–11 | CPY-05, CPY-05, CPY-05, CPY-05, CPY-05, CPY-07 |
| 12–18 | CPY-07, CPY-06, CPY-06, CPY-06, CPY-01, CPY-06, CPY-07 |
| 19–21 | CPY-08, CPY-08, CPY-08 |
| 22–25 | CPY-09, CPY-09, CPY-03, CPY-03 |
| 26 | CPY-10 |

### Avoid AI Design

The [pinned catalog](https://github.com/funboy322/avoid-ai-design/blob/60f5853a706873da3a30b3ddf3cc29f9683c885c/references/ai-tells-catalog.md) has 81 tell headings, including I1, which the index links to K6. Its variants map to the existing audit tests below. Match current evidence to the pattern before proposing an edit. Detector severity and template replacement recipes do not carry over.

| Avoid AI Design IDs | Catalog destinations in the same order |
| --- | --- |
| SD1–SD9 | VIS-01, VIS-01, TYP-01/LAY-01, CMP-04, VIS-03/TYP-04, TYP-04, VIS-09/IMG-02, VIS-01, VIS-01 |
| T1–T7 | TYP-01, TYP-01, TYP-01, TYP-01, TYP-04, TYP-02, TYP-06 |
| C1–C11 | VIS-01, VIS-01, VIS-01/VIS-05, VIS-02, VIS-05, VIS-03, VIS-10, VIS-02, A11-01, VIS-01, VIS-04 |
| L1–L11 | LAY-01, LAY-04, LAY-04, CMP-05/TRU-02, LAY-01/TYP-04, LAY-01/CMP-04, CMP-02/TRU-02, CMP-04, LAY-01.content-swap, VIS-04, LAY-02 |
| K1–K14 | VIS-11/SYS-01, VIS-07/VIS-08, VIS-06, VIS-07, TYP-04, CMP-03/LAY-04, CMP-07.control-states/UX-02, VIS-08/SYS-01, VIS-07/VIS-10, VIS-04/MOT-01, LAY-04, CMP-02, TRU-03, CMP-03 |
| S1 | LAY-05 |
| M1–M9 | MOT-01, MOT-01, VIS-05, MOT-03, MOT-02, MOT-04, MOT-02, MOT-01, MOT-02 |
| I1–I3 | CMP-03/LAY-04, CMP-03, CMP-03 |
| CP1–CP8 | CPY-01, CPY-06/CPY-05, CMP-01, CPY-05, CPY-08, CPY-02, CPY-04, TRU-01/TRU-02 |
| IM1–IM5 | IMG-01, IMG-01, IMG-01, IMG-03/TRU-04, TRU-01 |
| F1–F2 | SYS-07, SYS-04 |
| X1 | SYS-01/SYS-02 |

## Unslop

| Upstream evidence | Catalog destinations |
| --- | --- |
| [Signs](https://github.com/heydamir/unslop/blob/620163eedb3529d9cd23a3994ed8d287d06f760d/skills/unslop/references/signs.md) | Surface and hero effects map to VIS-01–VIS-10, decorative icons and badges to CMP-02/03, and ornamental motion to MOT-01–MOT-03. Peer spacing and greetings that obscure context make LAY-05 and CPY-01 more explicit. |
| [Data display pass](https://github.com/heydamir/unslop/blob/620163eedb3529d9cd23a3994ed8d287d06f760d/skills/unslop/SKILL.md) | Dashboard clutter maps to CMP-05.metric-relevance. Numeric jitter maps to CMP-06.numeric-stability. Answering the intended analytical question is tested under CMP-08.chart-encoding. |
| [Right to left](https://github.com/heydamir/unslop/blob/620163eedb3529d9cd23a3994ed8d287d06f760d/skills/unslop/references/rtl.md) | A11-05 covers direction and mixed runs. CPY-12 covers local formats and digits, and TYP-05 covers legibility and script rendering. |
| [Visuals](https://github.com/heydamir/unslop/blob/620163eedb3529d9cd23a3994ed8d287d06f760d/skills/unslop/references/visuals.md) | Decorative imagery and asset fit map to IMG-01–IMG-03. TRU-04.record-continuity and IMG-02.product-evidence cover coherence of preview records and simulated workflows. |

Evaluate these observations against the project's approved direction. Treat upstream palette, type, spacing and composition presets as creative direction outside this audit.

## When upstream changes

Review the linked source revisions when refreshing this skill. Place each new observation under an existing check when evidence and correction are equivalent. Add a stable ID only if the new observation requires distinct evidence or changes the decision. Keep subjective thresholds as prompts to inspect and reserve hard failures for evidence of a defect. Record the new revision and any crosswalk changes together.

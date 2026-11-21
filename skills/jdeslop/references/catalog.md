# Deduplicated audit catalog

Use the minimum evidence named in each section and apply the linked [detailed checks](detail-checks.md) where defined. Assign a status to each parent and applicable detail for each surface group. Record instances under the relevant IDs and cross-reference one primary finding for each shared correction. IDs remain stable as the catalog grows.

## Color, surfaces and visual effects

Inspect computed styles and pixels in supported themes. Compare the role of the color or effect with the established design.

| ID | Check | Evidence and correction boundary |
| --- | --- | --- |
| VIS-01 | Default palette | Look for reflexive blue–violet–pink, black plus neon purple, slate/indigo starter colors, washed pastels, cream/ink/brick editorial palettes, default dark plus lime, or a single colored wash borrowed from terminal aesthetics. Check the combination, its role and nearby defaults. Preserve intentional palettes. |
| VIS-02 | Accent overuse and semantic drift | Inspect competing accents, rainbow chips, arbitrary hues, default alert palettes and single-hue status boxes that lose readable distinctions. Check whether the same color communicates conflicting states across screens. A categorical data palette has a legitimate job. |
| VIS-03 | Gradient on content or control | Inspect gradient text, the selectively highlighted hero word, gradient CTAs, gradient borders and stripe fills. Check legibility and whether emphasis maps to meaning. |
| VIS-04 | Ambient decoration | Inspect blobs, auroras, ornamental grids, noise, dot fields, spotlights and cursor lights. Compare the same view with the effect reduced. Preserve product-related diagrams and deliberate material textures. |
| VIS-05 | Unmotivated glow | Look for colored shadows on controls or text, every object glowing, or light that contradicts the visual material. Distinguish an actual focus indicator or simulated light source. |
| VIS-06 | Transparency and glass stacking | Look for blur on several overlapping levels, glass on every card, and navigation whose readability varies underneath. Test during scroll and against the weakest background. |
| VIS-07 | Border and elevation overload | Inspect blanket hairlines, cloudy shadows, hard offset shadows without a material role, simultaneous border and wide shadow, border gradients that disappear at corners, and repeated accent rails or stripes. A status rail or an established brutalist shadow can be justified. |
| VIS-08 | Shape monoculture | Check oversoft small cards, uniform radius regardless of scale, corners that clash when nested, pill treatment everywhere, and unrelated forms sharing the same shadow profile. Look for lost hierarchy and the existing shape scale. |
| VIS-09 | Decorative chrome | Check faux browser or desktop frames, traffic-light dots, code windows, registration marks, decorative numbering rules and template framing. Keep them where they clarify actual context or an approved visual motif. |
| VIS-10 | Theme by reflex | Distinguish an approved dark-only product from a default dark skin. For supported themes inspect native inputs, menus, selection, icons, media, loading and browser surfaces as well as the main background. |
| VIS-11 | Unresolved or sterile visual treatment | Check for blank image shapes, default scaffold styling, blanket neutrals or the same tone on all surfaces when active areas, grouping or emphasis disappear. Restore hierarchy in the existing direction. |

## Typography and layout

Inspect rendered reading order, real content, CSS geometry and typography. An intentional symmetrical composition or compact operator screen can be appropriate.

| ID | Check | Evidence and correction boundary |
| --- | --- | --- |
| TYP-01 | Template type recipe | Inspect starter grotesks, default editorial italic-serif pairings, a single italic word pasted into a roman heading, default modern geometric fonts and novelty display mono. Check their fit for the job. A single family, including a common one, can provide a complete system. |
| TYP-02 | Incoherent type roles | Compare size, weight and line height for equivalent roles, excess families or weights, and a flat hierarchy without a clear first read. Prefer the established scale over creating a replacement. |
| TYP-03 | Hero scale and wrapping | Look for identical display scale across every page, long sentences filling the viewport, forced line breaks, single-word stragglers, visibly unbalanced heading lines and an orphaned CTA below the fold. Check localized and longer content. |
| TYP-04 | Repeated kickers and decorative emphasis | Inspect uppercased eyebrows, pill kickers, serial numbers, arbitrary highlighted, struck-through or underlined words, extra bold, and decorative mono metadata repeated above headings. Preserve real sequence, categories and useful status metadata. |
| TYP-05 | Legibility, line measure and spacing | Check small dense text, cramped leading, crushed display tracking, wide body tracking, long center-aligned or justified text, and all-caps passages. Inspect actual zoom and viewport. For supported scripts, check rendered glyph coverage, joining and the legibility of fallback or synthesized styles. |
| TYP-06 | Font delivery | Confirm loaded faces, available weights and glyphs, localized fallback and text reflow or layout shift while fonts load. Distinguish browser fallback from the intended typeface. |
| LAY-01 | Interchangeable page formula | Flag default centered hero, generic subtitle, two equal CTAs, three features, testimonial, pricing, FAQ and CTA sequence where sections lack a product reason. Test what each section answers. |
| LAY-02 | No first or next read | Compare visual weight with the real task. Inspect equal emphasis across actions, over-scaled ornament, forced symmetry, wrong card ordering and hero copy that buries the actual product. |
| LAY-03 | Repeated composition across sections | Compare identical centered stacks, title/copy/button rhythm, repeated section treatments and large empty slots. Keep repetition where comparison, navigation or accessibility benefits. |
| LAY-04 | Card and bento reflex | Inspect grids over lists, tables or plain copy, generic icon/title/description tiles, equal sizes for unequal information, nested cards and irregular bento blocks. Check each container boundary and the expected scan path. |
| LAY-05 | Weak grouping and spacing rhythm | Check too-uniform spacing, magic gaps, headings closer to prior content, crowded interiors versus wasted exterior space, and inconsistent section transitions. Compare sibling rows and cards with the same role for unexplained gaps, heights or padding. |
| LAY-06 | Grid alignment and optical edges | Check unrelated baselines, uneven scroller gutters, one column leaving a large accidental hole, edge-touching text and inconsistent padding among equivalent components. |
| LAY-07 | Information duplicate | Inspect labels, overview tiles, nav tabs, summaries and end CTAs that repeat nearby information. Check whether summaries still serve a distinct decision. |
| LAY-08 | Reflow, clipping and overlay | Test narrow, wide and actual transitions for unintended horizontal scroll, shrunk desktop grids, overlapped hit areas, clipped popovers and stacked sticky chrome. Check that hiding overflow did not merely hide the clipped content. Verify deliberate horizontal scrolling still works. |
| LAY-09 | Viewport, safe-area and zoom behavior | Inspect fixed heights, `100vh` mobile crops, landscape, on-screen keyboard occlusion, safe areas, text zoom and long translations. Check that main actions and results stay reachable. |
| LAY-10 | Reordered or missing content | Compare visual and DOM order, reading/keyboard progression, and content hidden when resizing. Ensure responsive density changes keep essential information and actions available. |

## Components, imagery and authenticity

Inspect the visible artifact, asset provenance where relevant and the actual control semantics. Check claim sources independently of polish.

| ID | Check | Evidence and correction boundary |
| --- | --- | --- |
| CMP-01 | CTA and button hierarchy | Check two primary styles with competing weight, ornament that looks clickable, ambiguous destructive actions, and the same button presentation for divergent roles. Compare label, state and real destination. |
| CMP-02 | Badge and chip filler | Check fake status pills, repeated NEW/PRO tags, sparkle/AI badging, icon+text duplicates, and pill treatments on noninteractive data that suggest actions. |
| CMP-03 | Icon-system drift | Inspect mixed sets, fill/stroke and weight drift, offsets, inappropriate size, arbitrary colored icon tiles, one glyph used for unrelated meanings and inconsistent icons for the same action. Preserve approved exceptions with a clear role. |
| CMP-04 | Scaffolded navigation and footer | Check floating chrome with no scroll behavior, redundant header actions, unwired menus, vague categories, fake legal/footer links, a stock newsletter closer with no task fit, inconsistent placement, missing current location or unhelpful breadcrumbs. |
| CMP-05 | Generic dashboard treatment | Inspect metric triptychs, decorative graphs or sparklines, equal weight for unequal metrics, absent period/unit/baseline, and editorial typography or spacing that obstructs operational density. Use CMP-08 for chart rendering or encoding, and TRU-03 for false signs of activity. |
| CMP-06 | Tables and structured data | Check alignment of comparable numeric values, actual sorting/filtering, units, meaningful zeroes, accessible labels, sticky columns and row actions. Inspect dense, empty and long-value states. Compare varied digits in the same column or updating counter. Check whether proportional figures make comparisons harder or cause visible jumps, and use tabular figures where they solve the issue. |
| CMP-07 | Duplicated component dialects | Compare same-role controls across pages for unexplained drift, variant creep, unnecessary wrappers, stacked borders, spacing or opacity mismatches and native elements with unchanged starter styling. |
| CMP-08 | Chart encoding obscures the task | State the question the chart is meant to answer (trend, comparison, share of a whole or progress toward a target). Read the rendered chart against known data and check whether its marks, order, scale and series distinctions allow a correct answer. Flag decoration that looks like an extra series or makes an ordered or categorical relationship ambiguous. Preserve any encoding that works for the task. |
| IMG-01 | Generic visuals | Look for interchangeable stock, generic AI illustration, corporate clip art, arbitrary gradient letter avatars, decorative photo walls, literal piles of marks in place of readable quantities, render defects in faces/text, primitive-shape pictorial SVG and organic clip-path imitations that misrepresent the material or subject. |
| IMG-02 | Fake or muddy product imagery | Inspect placeholder dashboard mockups, impossible UI, illegible screenshot text, mixed scales, fake terminal windows and heavy treatments obscuring the product or making a shipped raster effectively invisible. Clearly labeled schematic and demo art can be appropriate. |
| IMG-03 | Asset delivery and fit | Check broken embeds, placeholder URLs, wrong ratio or focal crop, distorted assets, excessive blur, inaccessible media labels and layout shifts. Check any required attribution from available records. |
| TRU-01 | Fabricated endorsement | Verify logos, testimonials, avatars, affiliations, awards and star ratings. Preserve approved demo proof when clearly labeled inside a prototype. Remove or qualify confirmed unsupported public endorsements. |
| TRU-02 | Fabricated numbers or claims | Check metrics, ROI, usage, security, compliance, speed and superiority against available facts. Distinguish demonstrably false, labeled illustrative and unverified. |
| TRU-03 | Fake urgency or activity | Check artificial counters, false scarcity, fabricated notification feeds, decorative live pulses and ambiguous “updated” labels. Confirm the real event or data path. |
| TRU-04 | Prototype presented as finished | Check mock data, fake search, pretend integrations and unwired features represented as real. Compare a product preview that illustrates a specific workflow with the actual workflow and check that repeated records agree across views. Distinguish intentional prototype fidelity from a deceptive production affordance. |

## Copy and microcopy

Read the page aloud or cold in its own language. Check the local register and neighboring text before editing. Preserve facts, rhythm that reads naturally, names, translations, UI placeholders and the user's chosen voice.

| ID | Check | Evidence and correction boundary |
| --- | --- | --- |
| CPY-01 | Generic promise | Check whether the headline and first viewport name the thing, user and actual task. Flag category filler, interchangeable features, inflated verbs, empty adjectives and vision language unsupported by what the product does. On working screens, flag greetings that displace the workspace, reporting period or task when that context matters. |
| CPY-02 | CTA and label ambiguity | Check generic calls to action, inconsistent terms for one action, labels that mispredict destination or effect, and icon buttons without a meaningful name. |
| CPY-03 | Repeated or staged copy | Check kickers, heading intros, headings echoed by the first sentence, sentence fragments that restate a conclusion, throat-clearing and authorial narration instead of information. |
| CPY-04 | Formulaic rhetoric | Check contrived contrasts, arguing against an unnamed position, staged build-up, claims of hidden depth, faux aphorisms and endings written as morals. Keep real distinctions. |
| CPY-05 | Mechanical cadence | Check forced triads, repeated sentence openings, interchangeable paired fragments, repetitive dashes or parentheses, qualifier stacks and unnecessary hyphenated compounds. Judge density and naturalness, not punctuation alone. |
| CPY-06 | Inflation and borrowed authority | Flag vague attributions, status inflation, ungrounded significance, abstract connections, unsupported expertise and trailing participial phrases that add no fact. Route claims that need verification to TRU-01 or TRU-02. |
| CPY-07 | Awkward syntax and marketing register | Inspect actorless passives, inanimate actors as a reflex, substitutions for simple verbs, jargon, AI catchphrases and language that calls every ordinary action a transformation. Preserve vocabulary the actual audience uses. |
| CPY-08 | Decorative formatting | Examine repetitive bold, title case, emoji, gratuitous heading blocks and inconsistent typographic quote conventions. Quotes, bold and headings matter when they support meaning or the locale. |
| CPY-09 | Draft and chatbot residue | Check lorem ipsum, TODOs, generic examples, placeholders in production, canned greetings or closers, knowledge caveats unrelated to the interface and copy describing its own drafting instead of the subject. |
| CPY-10 | Wrong reader or fake voice | Flag re-explaining prerequisites, burying decisions, injected slang or fake candid anecdotes, unnecessary second-person narration, and a voice that clashes with the product and task. |
| CPY-11 | Unhelpful state and error copy | Check messages that omit what happened, what persisted or how to recover. Leave actionable detail near its control. Preserve precise validation and user input. |
| CPY-12 | Localization and formatting | Check user-facing string concatenation, inconsistent terms, pluralization, mixed language, and date, number, currency and unit formats. Check the supported locale's digit convention and justified exceptions. Use a real locale and content expansion where supported. See A11-05 for directional behavior. |
| CPY-13 | Overcorrection | Read the edited copy across surfaces for a new template cadence, staccato fragments, substitutions for banned words, sterile one-note voice, or invented details added to sound more specific. |

## Behavior, motion and accessibility

Exercise the actual journey with representative input, failure cases, keyboard, touch, and supported preference modes. Record blocked checks when the runtime or relevant states are unavailable.

| ID | Check | Evidence and correction boundary |
| --- | --- | --- |
| UX-01 | Dead or misleading controls | Check target, result, disabled conditions and feedback. Flag `#` links, links pretending to be buttons and active-looking elements with no action. Test safely with fixtures for consequential actions. |
| UX-02 | Missing or misleading state | Check loading and slow requests, fresh and filtered empty, no search results, error/retry, success, disabled, offline and permission states where supported. Validate real feedback and recovery. |
| UX-03 | Form integrity | Check persistent labels, correct input modes, optional/required distinctions, meaningful inline validation, whitespace and extreme inputs, control geometry across states, duplicate submission, pending state and preservation of input on failure. |
| UX-04 | Unclear next step | Exercise the primary task as a first-time and a returning user. Flag accidental steps, competing actions, memory burden, surprising back behavior and recovery hidden behind a new flow. Stay within the existing product task. |
| UX-05 | Data loss and interruption | Check cancel, back, dismiss, undo or confirmation where relevant, unsaved input, repeated activation and resetting after errors. Match safeguards to consequence. |
| UX-06 | Desktop patterns imposed on mobile | Check hover-only UI, nested scroll traps, crowded touch controls, responsive navigation, and native interaction conventions where the app targets native platforms. |
| MOT-01 | Animation monoculture | Find uniform entrance fades, every card lifting, parallax stacks, decorative page-wide transitions and excessive hover effects. Check whether static content is artificially withheld during reveal. |
| MOT-02 | Decorative movement posing as feedback | Check idle floating objects, pulsing CTAs, decorative blinking cursors, perpetual shimmer and flybys, count-up animations, marquees or celebration detached from a real event or repeated after obvious success. Verify pause controls where auto-rotation requires them. Preserve approved or functional feedback when proportionate. |
| MOT-03 | Motion quality and responsiveness | Check unnecessary bounce/elastic easing, off-center wobbling spinners, broad transitions, layout-property animation, interrupted/repeated inputs, abrupt scroll or layout movement, and animations that delay action. Assess the actual rendered timing and context. |
| MOT-04 | Reduced motion and fallback | Exercise the system preference, animated assets and no-script behavior where applicable. Confirm content and feedback remain available with movement reduced. |
| A11-01 | Contrast and color independence | Measure effective foreground against actual backgrounds and supported states, including imagery, disabled vs enabled, icons and controls. Use applicable WCAG criteria and record ratios. Verify state information without color alone. |
| A11-02 | Keyboard and focus | Check interactive reachability, immediate focus visibility, logical order, overlays, focus return, Escape, keyboard tooltip availability and sticky overlays that obscure focus. Focus traps must match active modal behavior. |
| A11-03 | Roles, names and structure | Inspect accessible names, labels, landmarks, heading progression, semantic HTML, dialog titles, image text alternatives and announcements for changed states. Verify through the accessibility tree where available. |
| A11-04 | Pointer and alternate input | Measure applicable target size and separation, touch vs hover, a non-drag path where required, and interaction with text scaling or coarse pointer. Use the platform standard and its exceptions. |
| A11-05 | Bidirectional layout and reading order | In supported right-to-left (RTL) locales or surfaces that display mixed-direction content, test a real localized render. Check document and nested language/direction metadata, directional navigation icons and sequences, and whether fixed left/right assumptions break layout. Check mixed-direction names, codes, emails and signed values for reordered punctuation or signs. Verify input and display direction separately. Preserve icons and sequences whose meaning stays fixed across locales. |

## Design-system drift and implementation

Use actual files, computed values and end-to-end evidence. Report inaccessible source as blocked. Limit refactors to the verified cause.

| ID | Check | Evidence and correction boundary |
| --- | --- | --- |
| SYS-01 | Token drift | Compare actual color, radius, spacing, typography, elevation and transitions against declared tokens, aliases and page overrides. Distinguish intentional overrides from accidental values. |
| SYS-02 | Component forks and hidden overrides | Check duplicated shared markup, conflicting utility classes, cascading `!important` or preview-only runtime fixes, fake component reuse, and changes applied in only one consumer. |
| SYS-03 | Nominal implementation | Look for orphan styles, unfinished starter artifacts, broken imports or handlers, visual UI built entirely through brittle absolute positioning, and generated stubs that mask failure. Ground findings in observable impact. |
| SYS-04 | Generated maintenance noise | Flag narration comments, redundant generated wrappers and copied boilerplate where they obscure a real shared fix or misstate behavior. Preserve explanations of non-obvious decisions. |
| SYS-05 | Asset and layout performance | Observe slow or shifting content, oversized media, lazy or delayed delivery of the lead image, absent video posters where they affect loading, layout-animation churn, excessive effects/compositor cost, fonts and initial payload where they visibly delay or destabilize the experience. Record measurement context. |
| SYS-06 | Shared changes and regressions | After a shared patch, compare each consuming route family, theme and state against baseline. Look for resurrected defaults, new clipping, changed focus/contrast, removed functional cues and untracked token changes. In audit-only mode, check recent diffs if available. |
| SYS-07 | Structural and release leftovers | Check leaked starter titles, icons, tool attribution or template remnants that were meant to be replaced, and social metadata, irrelevant stock content, console/resource failures and dead routes exposed by the surface. Preserve required attribution. Verify public and authenticated contexts separately when both are in scope. |

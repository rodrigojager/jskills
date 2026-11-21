---
name: jiu-motion-media
description: Sub-skill of juice-it-up (E2 diagnostic, E3 calibration, E4/E7 recipes). Motion, interaction, and media of a site — where something should move, where it should not, with exact values, chosen engine, and cost. Invoked by the controller; do not use outside the workflow.
---

# Motion and media specialist

Find moments where movement makes the interaction clearer, more satisfying or more expressive. Expect to reject most candidates. Adapted from Emil Kowalski (`find-animation-opportunities`, `review-animations` and `STANDARDS.md`, MIT), from `design-engineering` (motion cluster), and from `higgsfield-generate` (media routes). Use the reference values in [motion-standards.md](motion-standards.md).

## The gate (every candidate passes the four questions, in order)

1. **Frequency.** 100+ times/day (shortcuts, central navigation): never animate. Dozens/day (hover, frequent toggles): reject or make nearly imperceptible. Occasional: standard animation or a brief accent contained to the element. Reserve more expressive flourishes for a rare or first-time moment.
2. **Purpose.** Name what the person gets from this motion: feedback, spatial consistency, state indication, continuity, explanation or delight tied to a specific moment. For delight, name why that moment deserves emphasis and what would make the effect tiresome.
3. **Speed.** UI stays under 300 ms (table in motion-standards.md). If it only "works" by being slow and flashy, it fails. Marketing and explanation can be longer, with a reason.
4. **Function.** Data the person reads or operates does not move for style's sake. Decoration in dense interfaces gets in the way.

Skip ornamental motion on shortcuts and rapid keyboard navigation. Give an action the same state and semantic feedback whether it starts from a pointer, touch or the keyboard.

## Diagnostic mode (E2)

1. **Motion recon:** libraries present, existing duration/curve tokens (your suggestions extend these, they do not create parallels), product personality, approximate frequency map per surface.
2. **Seam sweep:** lack of feedback when pressing; abrupt state changes; popovers unrelated to their triggers; group entrances without a useful sequence; gestures without physics. Also look for [small moments of delight](#small-moments-of-delight) in flows that already work.
3. **Review of existing motion:** `transition: all`, `scale(0)`, `ease-in` in UI, more than 300 ms without reason, wrong origin, keyframes on things triggered rapidly, animated layout properties, no `prefers-reduced-motion`, hover without `@media (hover: hover) and (pointer: fine)`, autoplay > 5 s without pause, decorative loops that do not stop under reduced motion.
4. **Existing media:** videos, GIFs, heavy images, Lottie/Rive/3D — weight, codecs, poster, fallback, decoded memory.

Output `evidence/motion-media.md`:
- **Opportunities** (at most 5–7 across the whole site, fewer per page), ordered by leverage: location, today, purpose, frequency, suggested motion with exact values (curve, duration, properties), reduced-motion treatment.
- **Rejected candidates (mandatory):** 2–5 places considered and discarded, with the gate question that eliminated them.
- **Problems in existing motion:** table Before | After | Why.
- **Verdict:** how much motion this site actually needs; which suggestion has the most leverage. If nothing passes, say so: that is a good result.

When the feeling cannot be judged from the code alone, say so instead of guessing.

## Addition-only pass

For `--workflow motion`, walk the target journey and select useful interaction moments using the next section. Inspect existing animation only to reuse the project's tokens and engine or to avoid interrupting a real interaction. Write `evidence/motion-media.md` with target, trigger, intended feedback, end state, frequency and reduced-motion behavior.

Keep existing effects out of the change list. Skip the separate audit and aesthetic removal verdict. Frequency, intensity and repetition are calibration guidance for the requested effect. If the user wants a more expressive flourish, carry that preference into the proposal. Verify keyboard and touch behavior, reduced motion, responsiveness, rapid retriggering and any asynchronous success/failure state.

## Small moments of delight

1. **Walk the journey.** Look for actions with a meaningful outcome or emotional beat, such as expressing a reaction, discovering something, making progress or completing a task. Record the exact element, the event, why it matters to this person and how often they encounter it. Look for a specific cue in the brief, design system or working UI, such as a recurring silhouette, spatial relationship, interaction rhythm or product metaphor. Consult the [GitHub example catalog](microinteraction-examples.md) by event during both the full diagnostic and the addition-only pass.
2. **Design the moment.** Specify the actual trigger, what happens briefly, the persistent end state and when the effect stays quiet. Borrow the feedback principle from a reference and design its expression for the product. Use the strongest relevant cue to decide the shape, origin, path, rhythm, scale or handoff to the final state. If a generic preset is the starting point, look beyond recoloring for a change that expresses how this product works or feels. Keep familiar feedback straightforward when extra styling would make it harder to read. Keep decorative feedback local. For asynchronous actions, distinguish immediate press feedback from celebration on confirmed success.
3. **Make it reviewable.** Record promising candidates as ordinary opportunities with the original (V0) and one or more defensible alternatives. Explain the user benefit and which product cue informed each treatment. Compare the full event sequence in the Lab. A one-shot preview from the [motion library](../lab/SUBSKILL.md#motion-library-in-the-lab) can help tune it. Treat recipe names, targets and triggers as examples and choose each independently. Read queued settings and the user's variation request after ingest, and preserve the source suggestion when proposing a variation.
4. **Review the rendered motion.** At normal speed, compare the original, the reference and the proposed mock. Check whether the cue reads on first activation, whether its origin and settling follow the event, and whether the end state stays legible. Exercise repetition, rapid reversal, keyboard and touch, and reduced motion with the same persistent outcome. For async work, exercise failure and interruption. Check at least one narrow and one wide viewport for clipping, layout shifts and competition with other feedback. An occasional accent earns its place when it fits the product's visual language and still feels good on repetition. Record the changes or omissions that make the comparison stronger.

## Calibration mode (E3)

For alternatives where motion is central: write the trigger, the sequence (before, during, completion, interruption, exit, repetition), values, interruptibility, entrance/exit asymmetry, touch behavior and reduced motion, and the **engine** (one per component, respecting the stack): CSS/`@starting-style`/WAAPI for predetermined motion; springs/JS for interruptible gestures; video, frames on canvas, sprite, SVG, Rive, or Three.js according to [`<skill-dir>/references/media.md`](../../references/media.md). Always the simplest alternative. Update the proposal draft at its draft path, not the recorded revision.

## Recipes for mock (E4) and implementation (E7)

- Animate only `transform` and `opacity` (justified exceptions: `clip-path`, light `filter`).
- Strong custom curves; never `ease-in` in UI; never `scale(0)` (start at 0.9–0.97 + opacity).
- Transitions (interruptible) for things triggered rapidly; keyframes only for fixed sequences.
- Reduced motion: gentler, not zero. In the mock, apply the rules under `@media (prefers-reduced-motion: reduce)` **and** `html[data-jiu-reduced-motion]`.
- Hover behind `@media (hover: hover) and (pointer: fine)`.
- Decorative loops stop when off-screen and under reduced motion.
- Expose in the mock, as CSS variables, every number not yet decided (duration, curve, distance), to calibrate in the lab.

## Generated media

Only describe in E3; draft or existing asset in E4; final in E7 with authorization and budget. Every generation goes through `jiu media claim` first (idempotency and budget) — see `references/media.md`. Do not install CLIs or log in on your own.

## Prohibited

Implementing in the target outside E7, generating media without a claim, suggesting animation to meet a quota, starting other subagents.

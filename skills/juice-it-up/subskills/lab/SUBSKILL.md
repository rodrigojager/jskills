---
name: jiu-lab
description: Lab of juice-it-up (E4, E5). Turns selected proposals into isolated demonstrations, compares them with the original at real scale (desktop and mobile, speed, pause, reduced motion), and collects comments and preferences linked to item/variant/revision. Never integrates into the real target. Called by the controller or directly with `/juice-it-up lab R-001 [O-01/V2/r1 …]`.
---

# Lab

The lab answers design questions with demonstrations, not with production code. Adapted from `prototype` (Emil Kowalski, MIT: divergence, picker, verification), from `prototype-picker` (AgentsORG/design-engineering, MIT), and from `design-lab` (0xdesign: baseline, structured feedback), with these deliberate differences:

- **Choosing does not promote.** Preferring a variant integrates nothing; integration is E7, authorized in the controller.
- **Nothing is deleted.** Mocks, decisions, and assets stay archived in the state; only servers and temporary contexts are shut down.
- **Any stack, including URL only.** Mocks are always standalone HTML inside the run directory, served by `jiu serve`. No temporary routes in the project, no git worktrees, no copies of the project.

## Direct entry (`/juice-it-up lab R-001 [refs]`)

1. `jiu show R`, `validate R`, `events-ingest R`. No existing run: say that the lab works on a run and offer to start the controller.
2. Refs cited without a `preview` decision: the user's request to create/view the mock **is** the preview authorization for those exact revisions — record `decide … preview --quote "<request>"`. Ambiguous refs (no revision, "the second one") → resolve to the revision presented, or ask.
3. No refs: open the lab with what already has a preview, and list what is selected without a mock.
4. Follow the "Controller procedure" below for whatever is missing.
5. When finished, return to the user what they need: URL, what to compare, and that implementing requires going back to the controller with the exact revision.

## Controller procedure (E4)

1. **Preconditions.** Each revision has `preview` recorded. Respect `limits.maxConcurrentPreviews`; the excess goes into a queue and is told to the user.
2. **V0 baseline** for each opportunity involved, if not already there (`previews/<O>/V0/r1/`), in order of preference:
   - code that runs: original route on the local dev server (`preview-set … --path http://localhost:…/route`) **and** static captures archived in the V0 folder;
   - URL only: page with desktop/mobile captures of the surface and a link to the published site (sites often block `iframe`); fidelity "static";
   - record with `preview-set R O-xx/V0/r1 --path … --fidelity …`.
3. **Construction.** Dispatch one builder per revision (section below), in parallel up to the limit, each with its path `previews/<O>/<V>/r<n>/`. Motion mocks receive the motion specialist's recipes.
4. **Check.** Open each mock (automated browser when available): it renders, interactions respond, console is clean, desktop and mobile, reduced motion works, mutation guard present, honest fidelity. A dead button presented as functional is a defect: send it back to the builder.
5. **Record.** `preview-set R ref --path previews/<O>/<V>/r<n> --fidelity "<functional: …; simulated: …; differences: …; media: …>"`.
6. **Independent critique** (E5): dispatch `subskills/critic` for each ready mock; record with `reviewed`.
7. **Composition** when two or more choices share a surface: a builder assembles `previews/_composition/C-xx/` with the revisions together, and you record `composition-set R C-xx --refs … --conflicts "<what competes or is incompatible>"`.
8. **Serve.** Run in the background `node <skill-dir>/scripts/jiu.mjs serve R --port 4977 --state <dir>` and deliver `http://127.0.0.1:4977/`. Queue and Shift+←/→ navigate suggestions, including those awaiting a preview. The versions bar, 1–9 and ←/→ switch exact revisions. Open Review for proposal, fidelity and feedback. R replays and space pauses. The canvas toolbar controls viewport, scale, speed and reduced motion. Refresh reads newly recorded previews and processed feedback. Comments and preferences go to `events/inbox/`.
9. **Feedback.** When the user returns (or says so in the chat), `events-ingest R`. For each:
   - `prefer`: recorded as a preference; does not change authorization.
   - `adjust`: the builder writes the next revision's draft (adjusted proposal + mock) → `variant-revise` → `decide <new ref> preview --quote "<adjustment text>"` (the adjustment request is the authorization to see the new revision) → build → `preview-set`. The previous revision stays in the lab for comparison.
   - `reject`/`defer`: already applied by the ingest; confirm to the user.
   - `comment`: consider in the next adjustment or in the critique.
   - `motion-suggestion`: read the selected effect or reference, exact selector, trigger, preset settings and variation request. When asked to preview it, use the usual mock workflow. Implement the approved revision with the real event and its fallback behavior.
   Respect `limits.reviewRounds`; on reaching it, say what is missing and the cost of continuing.
10. **Close.** Stop the server and dev servers when the user finishes or the session ends. Do not delete `previews/`.

## Motion library in the Lab

Open **Effect library** to show a compact panel to the right of the existing preview. On narrow screens the panel docks below it. Hover or keyboard-focus a card to play its sample, or tap on touch. Select a card to tint and outline eligible elements in the recorded mock while keeping the mock interactive. An unrestricted effect outlines the whole canvas. Filter by element or suggested trigger. Select the same card again or use **Clear selection** to remove the active effect, target and test. Changing the element filter back to **Any element** also clears the selection.

Use **Pick a target** to select a compatible element while browsing the library. Use **Change target** or the element picker to replace it directly. The solid outline marks the current target and the dashed outline follows the candidate. Click the new target to complete the change, or cancel to keep the current one. Once the selected element supports the current effect, **Preview on page** plays the motion there and **Configure** opens its settings. Reference patterns use an agent-built preview.

To start with the element, use the separate **Pick an element** icon. Hover over the preview to outline the target, then click it. The element breadcrumbs select ancestors. Double-click the selector field outside the breadcrumbs, or use **Edit**, to type a CSS selector. The outline follows a selector that resolves to one visible element. Choose the intended **Trigger in the product** independently, then select from the compatible effects. **Change effect** opens the same two-column selection while preserving the target.

Duration and **Preview on page** appear after effect selection. The test plays a one-shot cue in the current mock state using the Lab's playback and reduced-motion settings. The agent wires the intended trigger and accompanying UI behavior into a revised mock.

**Particles** and **Reaction burst** offer Soft sparks, Rising dots and Falling confetti presets. Change the shape, trajectory, count, spread, color and timing independently. Reaction burst adds a quick anticipation and rebound to the target. For a favorite or reaction in a revised mock, put the accent on the activation that turns it on and preserve the actual selected state. Use **Variation request for the agent** to specify what the user should feel or understand, which existing motif or design token to draw from, and what to adapt in the motion. **Reference patterns** offers the [15 sourced examples](../motion-media/microinteraction-examples.md) as starting points for an agent-built preview.

**Directional nudge**, **Turn to disclose**, **Reveal from an edge**, **Focus halo**, and **Ink ripple** each have two starting presets with editable parameters. Use the relevant controls for direction and travel, turn angle, reveal edge and extent, halo spread and thickness, or ripple origin and opacity. Halo and ripple inherit the target color unless a hex color is specified. Treat the temporary halo as an accent around the existing focus indicator. For disclosure, preview the icon in its intended resting orientation and have the revised mock connect both orientations to the actual expanded state.

**Queue suggestion** records the exact revision, preview hash, target, recipe version, settings, trigger and optional request. It appears under **Motion suggestions** in the Library and Review. **Create variation** loads a suggestion's settings and request for editing and queues a separate suggestion linked to its source. The original remains available. Ingest with `events-ingest`. Compare V0 and a revised mock using the actual product event. A motion suggestion records an experiment and keeps normal implementation approval in place.

The agent can stage the same kind of suggestion from the CLI. The full catalog includes preset IDs, option values, and reference IDs.

```sh
node <skill>/scripts/jiu.mjs motion list --catalog
node <skill>/scripts/jiu.mjs motion stage R-001 O-01/V2/r1 --recipe ink-ripple@1 --selector "#apply-filters" --trigger click --preset offset --options '{"origin":"end","intensity":14}' --note "Use the existing action tint; preserve pressed and keyboard focus states." --state <dir>
node <skill>/scripts/jiu.mjs motion stage R-001 O-01/V2/r1 --recipe reference-pattern@1 --selector "#export" --trigger click --reference action-progress --note "Use measured progress and support cancellation." --state <dir>
```

Use `--variation-of <source>` to link a CLI suggestion to the earlier Lab event. Refresh the Lab to see CLI suggestions. Check that the selector resolves to one rendered element. Exercise real hover, focus, async success/failure, cancellation and touch behavior in the new mock where relevant. A reference may require multiple elements, geometry, gestures or real state.

## Batch approval

The Lab shows **Approve all…** after the controller records recommendations. It opens a local confirmation page listing one exact revision per opportunity and any blockers. Select **Approve all for implementation** to queue the authorization. The controller incorporates it with `events-ingest` and follows implementation and verification. The same batch can be approved from chat or the CLI without opening the Lab.

## Diagnostic checks

The controller writes the optional `evidence/checks.json` when consolidating diagnostics and adds opportunity links as suggestions are recorded. Each check needs a stable, unique `id`, a `title` and one of `pass`, `issue`, `skipped`, `blocked` or `pending`. Record what was observed or what evidence is missing in `detail`. An optional `opportunity` links the check to the relevant Queue item. Older runs display their recorded diagnostic reports.

For [interactive behavior](../a11y-perf/interaction-checks.md), record a separate result for each applicable check on each widget. Use a title that identifies the widget and a detail that describes how to reproduce the observed behavior.

```json
{
  "checks": [
    {
      "id": "motion.submit-feedback",
      "title": "Feedback while the connection test runs",
      "status": "issue",
      "detail": "The button stays visually unchanged during the pending request.",
      "opportunity": "O-01"
    }
  ]
}
```

## Mock builder (worker brief)

You build **one** revision. Read the recorded proposal (`proposals/<O>/<V>/r<n>.md`), the brief, the recon, and the V0 baseline.

**Hard rules**
1. **Never touch production code.** Write only to `previews/<O>/<V>/r<n>/`. Never create a git worktree, a branch, or a copy of the project.
2. **Real materials, worst-case content.** Fonts, tokens, icons, and copy from the real product (with code: the real files; URL only: declared approximation). Fill in with the content that breaks things: the longest name, the two-line title, the full list, the empty state — the same content used in V0, so the comparison is fair.
3. **Same task, representative context.** The mock shows the intervention within the page's surroundings (what comes before and after), not an isolated component on a white background.
4. **Works for real, or declares it is simulated.** Interactions, states, and motion are real. What cannot be real gets `data-jiu-simulated` and goes into the fidelity declaration. No "imagine this part".
5. **No real mutations.** Use `<skill-dir>/assets/mock-template.html` as the base and keep the `jiu:reduced-motion`, `jiu:guard`, and `jiu:fidelity` blocks. No payments, message sending, login, API writes, or production data.
6. **Open numbers become controls.** Duration, curve, distance, blur, delay that the proposal marked "to calibrate" live in CSS variables on `:root` with the proposed value, commented with the reasonable range.
7. **Reduced motion** under `@media (prefers-reduced-motion: reduce)` **and** `html[data-jiu-reduced-motion]`. Animations not visible via `document.getAnimations()` (canvas, video, Three.js, Rive) implement `window.__jiuPlayback({ speed, paused })`.
8. **Draft media** (existing, declared placeholder, or generated only with an authorized `jiu media claim` reservation).
9. **Same craft across all variants.** A sloppy variant does not widen the exploration; the user rejects a good direction because the finish was poor.
10. **Do not reproduce the picker or comparison controls** inside the mock: the lab already has them.

**Formats**
- Self-contained `index.html` (CSS/JS inline or in sibling files), served at `/run/previews/…`. CDN libraries only if the project already uses them; record the exact version.
- With code, real materials are read from the project and **copied into the revision folder**: token CSS, fonts, icons, images, the markup of the surrounding sections. You read the project; you never write to it, never run its build to produce the mock, and never add a route to it. When the real component cannot be faithfully reproduced as static HTML, rebuild it and declare the difference in the fidelity.

**Self-inspection before returning:** opens without console errors, each interaction responds, desktop 1440 and mobile 390 checked, reduced motion checked, guard present, fidelity written.

**Return:** status, path, fidelity declaration (functional / simulated / differences from the original / media), open values exposed, and concerns. Do not record anything in the state.

## Prohibited

Integrating into the target, deleting mocks or decisions, presenting a mock as the original application, treating a preference as authorization, starting other subagents (the builder), generating paid media without a claim.

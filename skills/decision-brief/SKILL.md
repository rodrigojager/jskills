---
name: decision-brief
description: Ask the user for decisions in a form they can answer quickly and with enough context - numbered questions, options with consequences, a recommendation, and a clickable visual preview (images, HTML prototypes, diagrams) whenever the options produce something that can be looked at. Use whenever work needs the user's decision - end of a phase, open points in a plan, a /goal that must stop and ask, choices between designs or structures.
---

# Decision brief

The user manages several projects at once and may not remember the details of this one. A question with little
context, or a choice between designs described only in words, costs them more than it saves.

## When

Only for what truly depends on the user: taste, product direction, cost, risk, credentials, anything outside the
agreed plan. What is done, obvious, or already decided is not asked. Few questions; each open point you resolve
yourself is recorded with your choice and its reason, not asked.

## Shape of the brief (in the chat)

1. **Context, 2–3 lines**: which project, where the work stands, why these decisions are needed now.
2. **Numbered questions** `Q1…Qn`, each with:
   - why it depends on the user (one line);
   - options `A`, `B`, `C`, each with what it is, what happens if chosen, what is lost if not;
   - the recommendation, why, what happens if the user follows it and what happens if not.
3. **Preview link** when there is one (below).
4. **How to answer**: "Q1 A, Q2 recommendation" or "agree with all except Q3".

More than about 5 questions: write the full brief to a file (in a jproject container, `_work/10-docs/` or next to
the plan), keep only the summary and the link in the chat.

## Visual previews

Whenever the options produce something that can be seen, show it, so the user compares instead of imagining:

| The options are… | Preview |
|---|---|
| UI, layout, styling, images, video frames | One image per option (screenshot of a prototype, render, frame), or a small HTML prototype per option |
| Structure: architecture, flow, data model, folder layout, plan | A diagram per option: Mermaid (in the preview page, or with the Mermaid preview tool when available) or a `.drawio` file the user can open in draw.io / diagrams.net |
| Text, naming, copy | Side-by-side samples in the preview page |

Put all options of one decision set in **one page**: copy `preview.template.html` from this skill, fill the
`QUESTIONS` array (kind `image`, `html`, `mermaid` or `text`), and give the user a clickable link to it
(`file:///C:/…/decisions.html`). Images open enlarged on click; HTML prototypes are embedded with a full-screen link.

Where previews live: in a jproject container, `_work/20-design/<topic>/` (mocks) or `_work/10-docs/_render/`
(rendered pages); otherwise the session scratchpad, or ask. One canonical page per decision set, updated in place;
delete previews the user rejected once the decision is made.

## After the answer

Apply exactly what was chosen. Record the decisions where the project keeps them (plan, ADR, `_work/` notes) with
the reason, so nobody asks again.

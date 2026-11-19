---
name: audit-other-agent
description: Run a user-led discussion across agents in rounds. Use when the user wants another agent to assess a topic, problem, idea, future feature, tradeoff, or existing work, critique a previous agent's position, or consolidate the discussion. A topic can be unrelated to a codebase and can start without any artifact or prior answer.
---

# Discuss and review with another agent

The user passes each turn and decides when the discussion is resolved. Each agent takes one turn, reads what came before, and leaves a response for the user to pass on. Agents wait for the user's next instruction each time.

## Reason independently and help the user decide

Apply these principles in every round, including reviews of existing work.

- Seek the best outcome for the user's stated objective within the agreed constraints. Never agree just to please the user, defer to another agent, or end a debate. Agree when the reasoning holds, disagree when there is a substantive reason, and change your position when evidence warrants it. Explain the consequences of following your recommendation. If reconsidering an agreed premise could materially improve the result, explain the opportunity and ask the user before treating the premise as changed.
- Extend the prior analysis. Test the most convincing option against an overlooked scenario, assumption, downstream effect, or plausible alternative when it could change the decision. Explain when that scenario would arise, who or what it affects, and how it changes the recommendation. Carry forward useful insights from earlier rounds and distinguish established facts from assumptions and predictions.
- Teach the user enough to decide. Use plain language and concrete examples, and explain the sequence of events when a consequence depends on how something works. For each meaningful option, explain what the user would choose, what happens next, what they gain or give up, and what it costs to reverse if that matters. State the recommendation and why it best meets the user's priorities. If agents disagree, show the strongest case on each side, what the disagreement hinges on, and which evidence or user preference would settle it. When asking the user to choose, use the `decision-brief` format if available, or present a numbered decision with options, consequences, and a recommendation.

## The shared review file

Keep one append-only file per topic:

- In a jproject container, use `_work/10-docs/reviews/<topic>.md`.
- Elsewhere, use the location the user gives, place it next to an existing artifact, or use `reviews/<topic>.md` in the workspace for a standalone discussion. Follow workspace rules for file placement.

On the first turn, record the user's question in their own words, their constraints and decision criteria, and the material they supplied, if any. Link existing artifacts or include the previous agent's answer with attribution when the user provides it outside the file. A topic can start with only a question, for example whether a proposed capability should exist at all.

On later turns, read the whole file and continue the same thread. Append a section headed `## Round <n> - <agent> (<role>)`. Preserve earlier rounds and use the user's latest clarifications to guide the discussion. If a round produces a prototype, save it beside the file as `<topic>.r<n>.<agent>.<ext>` and link it from that round.

## Discuss a topic or proposal

1. If there is no earlier position, lay out the plausible options, including keeping the current behavior or dropping the idea where relevant. State your recommendation, what it depends on, and what would change your mind.
2. If there is an earlier position, respond to its specific claims and reasoning. Explain what you agree with, what you would revise, and what you still disagree with. Give evidence, a counterexample, or a concrete tradeoff for each substantive objection. Label assumptions about future behavior.
3. If your own position is challenged, answer the objections and say what you changed and why. Carry forward promising parts of earlier positions, and propose a revised option when it resolves a real disagreement.
4. Leave the user and the next agent with the current recommendation and the few unresolved questions or checks that could change it. Keep substantive dissent visible until it is answered or the user decides.

## Review an existing artifact

- Read the original request and inspect the specified artifacts without changing them during review. Judge correctness, scope, quality, risks, and the user's rules. Record concrete findings with `F<round>.<n>`, severity (`critical`, `major`, `minor`, or `nit`), location, consequence, suggested fix, and evidence. Identify parts worth keeping. Link a prototype when permitted and useful.
- When answering a review, mark each finding `accepted`, `rejected`, or `partly` and give the reason. Challenge unsupported findings. Apply accepted fixes when the user has authorized revisions and record what changed.

## Hand back to the user

End each round with two lines:

- `summary: <agreement, dissent, and decision needed, or counts of findings by severity and disposition>`
- `next: give the other agent "/audit-other-agent continue <absolute path of the review file>"`

Then stop and let the user choose who goes next or whether to close. When the user closes the discussion, append an `## Outcome` with the decision they approved, the reasons that mattered, the rounds that informed it, and any remaining dissent or open points. An unresolved question can be the recorded outcome. For a prototype review, follow the user's choice of canonical result and the safe-cleanup rules for rejected prototypes.

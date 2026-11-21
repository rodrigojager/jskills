# juice-it-up

Agent skill that orchestrates the improvement of experience, UX, and visual impact on websites: specialized diagnostics, proposals per opportunity, isolated mocks in a comparison lab, decisions per item/variant/revision, and implementation of only what was chosen, with resume across sessions.

## Installation

Link (or copy) this folder as `juice-it-up` into the agent's skills directory:

- Claude Code: `~/.claude/skills/juice-it-up/` (or `<project>/.claude/skills/juice-it-up/`)
- Codex: `~/.codex/skills/juice-it-up/`
- OpenCode: `~/.config/opencode/skills/juice-it-up/`

On Windows: `New-Item -ItemType Junction -Path <dir>\juice-it-up -Target <this folder>`.

Only the top-level `SKILL.md` is a skill. Sub-skills are named `SUBSKILL.md` on purpose, so agents that scan skill folders recursively do not register them as separate skills.

Requirement: Node.js 18+ (the state tool and the lab server have no dependencies).

## Usage

```
/juice-it-up https://mysite.com ./my-site
/juice-it-up add only animated microinteractions to ./my-site
/juice-it-up review only seasonal opportunities for ./my-site
/juice-it-up resume R-001
/juice-it-up lab R-001 O-01/V2/r1
```

Or in natural language: "analyze this site and propose improvements", "create mocks of O-01/V2 and O-03/V1, do not change anything", "on O-01 I liked V2 but reduce the motion", "implement O-01/V2 r3 and leave the rest".

The state CLI selects the workflow at `init --workflow full|motion|seasonal`. Full review is the default and includes the seasonal fit check. Approve each result individually or confirm the recommended batch once in the Lab or with `batch` and `approve-all` in the CLI. [Command reference](references/state.md).

### Language and appearance

```sh
node scripts/jiu.mjs --lang pt-BR --help
node scripts/jiu.mjs --lang pt-BR --theme dark serve R-001
```

Set `JIU_LANG` or `JIU_THEME` to supply defaults. The Lab has a sun/moon switch and a language picker in its toolbar. Choices persist in the browser, and the default theme follows the OS. The CLI uses the terminal's own appearance.

To add a language, add the same locale code to [CLI messages](scripts/cli-locales.json) and [Lab messages](assets/lab-locale.mjs), with its display name in the Lab catalog. Keep the English keys and numbered placeholders such as `{0}`; translate only the values. The menus discover locale codes from the catalogs.

## Structure

```
SKILL.md                 controller (E0–E8)
references/              state, decisions, proposal, dispatch, input modes, media, acceptance scenarios
subskills/               recon, ux, art-direction, motion-media, a11y-perf, lab, critic, implement, verify
scripts/jiu.mjs          durable state (CLI) + lab server
scripts/jiu.test.mjs     tests for the state rules
assets/lab-harness.html  comparison lab
assets/mock-template.html base for the mocks
NOTICE.md                origin and licenses of the reference skills
```

## Tests

```
node --test scripts/jiu.test.mjs
```

The behavioral scenarios (which depend on the agent) are in `references/scenarios.md`.

## License

MIT (see `LICENSE`). Adapted third-party material keeps its own license and attribution in `NOTICE.md`.

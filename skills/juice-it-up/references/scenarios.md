# Acceptance scenarios

The central acceptance test: the user tries options, chooses per item, and continues in the same workflow without losing decisions or inadvertently changing the target.

The state rules of each scenario marked with ✱ have automated tests in `scripts/jiu.test.mjs` (`node --test scripts/jiu.test.mjs`). The others are controller behavior: run them in a new session with a test site and check the expected result.

| # | Scenario | How to trigger | Expected |
|---|---|---|---|
| 1 | URL only ✱ | `/juice-it-up https://public-site.com` | `url` mode; inferences marked; snapshot recorded; independent mocks with declared differences; `implement` refused; delivery offered as a package |
| 2 | Code only | `/juice-it-up ./my-site` | stack and tokens detected; attempt to run locally; if it does not render, blocker recorded and visual conclusions separated |
| 3 | URL and code | both | published × local divergences listed before proposing; proposals linked to a commit |
| 4 | Resume in another session ✱ | new session: "resume R-001" | reads the state before acting; does not redo recon/diagnostics; `next` guides; earlier decisions preserved |
| 5 | Mock approval without implementation ✱ | "create a mock of O-01/V2, do not change anything" | only `preview`; target untouched (`git status` clean); attempt to implement is refused |
| 6 | Partial implementation ✱ | "implement O-01/V2 r3; leave the rest" | only tasks for that ref; other opportunities with no decision change |
| 7 | Code change after the mock ✱ | edit a file on the surface and resume | `check-stale` marks only the affected opportunity; `implement` refused until revalidated |
| 8 | Subagent failure | worker with no output file | controller does not invent a result; re-dispatches, runs sequentially, or marks `blocked` |
| 9 | Missing artifact ✱ | delete `proposals/O-02/V1/r1.md` | `validate` flags it; controller handles it before proceeding |
| 10 | Duplicate paid generation ✱ | repeat "generate the hero video" | `media claim` returns `EXISTS` or `IN_PROGRESS`; no second charge |
| 11 | Conflict between proposals | choose O-01/V1 and O-03/V2 in the same fold | composition in the lab with the conflict highlighted; user decides |
| 12 | New round preserves choices ✱ | "more options for O-01" after decisions | new variants V3…; earlier decisions and mocks intact; new revision does not inherit approval |
| 13 | No subagent tool | environment without Agent | warning to the user; named sequential passes; `--self` reviews, never called independent |
| 14 | Tasks on the same file ✱ | two choices touching the same CSS | `task start` refuses the second until the first finishes |
| 15 | Feedback in the lab ✱ | "I prefer this" + "request adjustment" in the harness | events validated; preference does not authorize; adjustment becomes a new revision |
| 16 | Publishing | "deploy it" without prior authorization | controller confirms scope and records authorization before publishing |
| 17 | Default seasonal fit ✱ | a full review with no occasion specified | record `evidence/seasonal.md` with the fit assessment. Leave the product unchanged if no relevant occasion fits or until the user approves a proposal |
| 18 | Seasonal only ✱ | "I'd like a Santa hat on the logo and something for Halloween" | seasonal workflow uses focused recon and skips general audits; proposals include V0 and their own limit (`maxSeasonalOpportunities`) |
| 19 | Seasonal outside the window | implemented Halloween decoration, checked in March | zero DOM and zero network for the decoration; off-date preview shows it; kill switch works |
| 20 | In-scope reports ✱ | go from recon straight to proposals and ask for a mock | `decide … preview`/`implement` exit 3 listing missing in-scope evidence; deliberate skips appear in `rulings` |
| 21 | Motion addition only ✱ | "Just add playful microinteractions to this flow; leave its animations alone" | proposals add interactions and preserve existing effects, omit the aesthetic removal pass, and specify technical verification |
| 22 | Batch without Lab previews ✱ | "Implement everything on your recommended list" | show one exact revision per opportunity and accept one confirmation. A change to the list requires a new confirmation |

---
name: fix-tool
description: When a tool, plugin, CLI or MCP server the user relies on misbehaves, find the root cause and fix it in the user's own repository or fork (never upstream), usually in a parallel subagent so the main work goes on, then release, reinstall and verify the installed version. Use when the user asks to fix the tool itself ("fix jtasks", "this is a bug in the tool") rather than work around it.
---

# Fix a tool

The goal is a fix in the tool, so the problem cannot come back, not a workaround in the current project.

## Where the fix goes

- **The user's own tool** (its repository is in the user's account): fix it there.
- **A third-party tool**: never push, open an issue or open a pull request in the vendor's repository. Use the
  user's fork:
  1. `gh repo list <user's account> --fork` or `gh api repos/<vendor>/<repo>/forks` to see if a fork exists.
  2. If not: `gh repo fork <vendor>/<repo> --clone=false` (creates it in the user's account).
  3. Clone the fork into the user's designated tools directory. If no directory is configured, ask where to put it.
  4. Install the tool from the fork so the fix is what runs.
  Only if the user explicitly asks to contribute upstream may a pull request to the vendor be prepared, and then
  show it to the user before opening it.
- Never write the user's account names or hosts into the fix, the commit or the docs.

## Steps

1. **Reproduce and find the root cause** with evidence (logs, a minimal repro, the code path). State it in one or
   two sentences before changing anything.
2. **Decide with the user when it matters**: if the fix changes behaviour others rely on, or is larger than a
   contained change, present it (decision-brief) and wait.
3. **Fix in parallel** when the main work can continue: spawn one subagent with the repository path, the root
   cause, the acceptance check and the rules above. Prefer a deterministic guard (code, test, hook) over a note
   that an agent must remember. Add a regression test that fails without the fix.
4. **Avoid new bugs**: run the tool's full test suite once at the end and compare with the previous release; a
   failure that also exists on the previous release is reported as pre-existing, not hidden.
5. **Ship it** with the `ship-release` skill: version bump (plugins need it to update), tag, push to the user's
   repository or fork, release.
6. **Reinstall and verify**: update the installed copy (plugin update, package install from the fork, binary on
   PATH), then check the installed version and that the original problem is gone, in a new session or terminal.
7. **Report**: `cause: ...`, `fix: <repo>@<version>`, `installed: yes`, `original problem: gone`.

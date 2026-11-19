---
name: ship-release
description: Ship finished work end to end — commit, merge to main, push every repository involved, tag and publish the release, install the new build locally, update READMEs and help in every language, scan for secrets and leftovers, deploy when asked — and report each step. Use when the user says to push, publish, release, tag, "sobe", install the new version or deploy.
---

# Ship a release

The user ships often and has been bitten by half-done releases: a branch never merged, a tag never pushed, a
release never published, an old binary still on PATH, READMEs left in one language. Do every step that applies,
in order, and report each one.

## Whose repositories

- Only the user's own repositories, in the account configured as theirs. Never push, tag, release or open a pull
  request in a third party's repository, unless the user explicitly said they collaborate on that repository.
- A change to a third-party tool goes to the user's fork, never upstream.
- If a remote's owner is not the user's account, stop and ask. Check with `git remote -v` and `gh repo view
  --json owner`.
- Never write the user's account names, server addresses, hosts or credentials into a skill, a README, a commit
  message or any file that is shared or committed.

## Before shipping

1. **Tests and checks** the repository defines (its README, CONTRIBUTING, CI config). Run the full suite once, at
   the end, not repeatedly. A failure: find out whether it is new (compare with the last release) before deciding;
   report it plainly and do not ship a new failure without the user's go-ahead.
2. **Secrets and leftovers**: no credentials, `.env` files, keys, tokens, personal paths, scratch scripts, logs,
   build output or AI artifacts in the diff (`git diff --stat`, `git status --short`, a grep for key patterns).
   In a jproject container, `jproject check --product-only`.
3. **Version**: bump it everywhere the project keeps it, with the project's own script if it has one. A plugin
   whose marketplace compares versions needs the bump or users never see the update.
4. **Docs**: README and help text updated for the change, in every language the project has (same sections, same
   facts). Install instructions still correct.

## Shipping

5. Commit (project's message style, English, with the attribution lines the session requires). Merge to `main`
   directly unless the project says otherwise; delete merged branches the user created for this work.
6. Push **every repository the change touched** (a tool and its plugin, a library and its consumer). List them
   first, then confirm each one is in sync (`git status -sb` shows no ahead/behind).
7. Tag with the project's tag format and push the tag. Publish the GitHub release when the project uses releases
   (`gh release create`), with short notes of what changed.
8. Install locally so the new build is what runs: the project's install command, then check in a **new terminal**
   that the command on PATH reports the new version. Do not install over a version a running process still uses:
   say so and ask.
9. Deploy to the user's server only when the user asked for it, with the deployment notes the user keeps for you.
   Verify the deployed version or health endpoint afterwards.

## Report

One line per step: `tests: 263/265 (2 pre-existing)`, `pushed: repo-a, repo-b`, `tag: v1.4.2`, `release: published`,
`installed: v1.4.2 on PATH`, `deploy: skipped (not asked)`. Anything skipped or failed, with the reason.

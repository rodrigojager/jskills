---
name: existing-solutions-scout
description: Before building a skill, tool, plugin, MCP server, CLI, video or animation effect, or game feature, find out cheaply whether someone already made it - deterministic searches of GitHub, Hacker News, npm, crates.io and the MCP registry, optional cheap subagents to read the shortlist, then a didactic comparison and recommendation. Use when the user asks to check whether something exists, "avoid reinventing the wheel", or before starting to build anything reusable.
---

# Existing solutions scout

Collecting candidates needs no intelligence; judging them does. So: code collects, cheap agents read, the main
model compares and recommends.

## 1. Collect (no agent)

Write 2–4 queries in English (the thing, its synonyms, the host: "claude code", "opencode", "codex", "mcp",
"blender", "after effects", "godot"…), then:

```
python -I <skill>/scripts/search.py "query one" "query two" --limit 15 --out <tmp>/scout.json
```

Sources by default: GitHub (via `gh`), Hacker News, Reddit, npm, crates.io, the MCP registry. The first
`--details` results of each source (default 5) come with what is needed to judge them without opening the pages:
GitHub stars, forks, open issues, license, last push, latest release, the most-commented issues and the start of
the README; the top Hacker News and Reddit comments (criticism, comparisons, benchmarks people mention); npm weekly
downloads; crates recent downloads.

Reddit closed its own keyless access (anonymous JSON answers 403, RSS ends on 2026-11-13), so the script reads it
through Arctic Shift, a community archive: title search, once per subreddit, within `--since` days (default 365).
Pick subreddits for the domain with `--subreddits` (default: AI-coding ones; e.g. `blender,blenderhelp` or
`gamedev,godot`) and use short title words as queries. The archive is slow and sometimes busy; the script retries.
When it fails, use the web search tool with `site:reddit.com <query>`.

For domains the script does not cover (Blender extensions, asset stores, game engines' marketplaces), use the web
search tool with `site:` filters. The output is raw and unranked on purpose.

## 2. Shortlist (main model, quick)

From the raw list, keep only plausible matches (usually 3–8). Discard by title and description; do not open pages
yet.

## 3. Read the shortlist (cheap subagents, optional)

The script's details usually suffice. When a candidate still needs its docs, issues or code read in depth (how it
authenticates, how it fails, whether it fits the user's setup), spawn **one subagent per candidate**, on the cheapest model
(use a lightweight model when the host supports model selection), in parallel, each with a narrow task that ends in a few minutes:

> Read <url> (README, docs, the last release). Report in under 200 words: what it does, how it is installed and
> used, supported hosts/platforms, license, last activity, maintenance signals, obvious limits. Facts only, no
> opinion. If the page fails, say so.

One candidate per subagent keeps each one short (well under the 5-minute subagent cache lifetime) and cheap. Skip
this step when titles and descriptions already settle it.

## 4. Compare and recommend (main model, didactic)

Present to the user:
- a table: candidate, what it does, fit with what the user wants, maintenance, license, cost of adopting;
- for the best 1–3: what using it would be like, what is missing, what would have to be adapted;
- the comparison with building it ourselves (or with the user's own existing version);
- the recommendation, why, what happens if the user follows it and what happens if not.

Ranking happens here, by judgement, not in the script. Delete `<tmp>/scout.json` when done.

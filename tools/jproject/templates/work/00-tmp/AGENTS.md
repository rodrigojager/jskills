# 00-tmp

## Purpose
Ephemeral output: anything nobody needs to read later.

## What goes here
- Logs, temporary files, dumps, analysis output, test builds.

## What does NOT go here
- Anything someone will want to read later (docs: `10-docs/`, plans: `30-plan/`, tools: `50-tools/`).

## Conventions
- Free naming. Subfolders allowed.
- Never reference files here from other documents.

## When to delete
- Any time, without looking. `jproject clean` empties this folder (except this file).

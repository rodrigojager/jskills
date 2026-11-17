# jtasks-v2

## Purpose
This is the product repository of `jtasks-v2`: only what the project needs to build, run, test and ship.

## Architecture
One foreground Rust controller owns scheduling and SQLite writes per canonical checkout; scoped TCP clients submit outcomes and read projections never mutate runtime state.

## Conventions
- Rust 2024 with an explicit Rust 1.89 minimum. Run focused CLI integration checks for changed behavior; integration/release gates are separate tasks.
- Intent is versioned JSON. Runtime state is SQLite. Store launch intent before spawn; keep result acceptance separate from owned-process retirement.
- Modules are split into domain, storage, controller, execution, executors, resources, transport, read and CLI. Only the controller (or locked import) writes runtime state.
- Build output and fixture workspaces belong under the container's `_work/00-tmp/`, using Cargo's `--target-dir` argument.
- Configuration comes from environment variables; `.env.example` lists the names, real values live outside this repository.

## Forbidden
- Creating new top-level folders without being asked.
- Creating `.md` files other than `README.md`, `AGENTS.md` (root and folders with their own rules) and `WORKSPACE.md` (root only).
- Saving AI working artifacts here (plans, ADR drafts, mocks, logs, handoffs, helper scripts, `.scratch/`). They live in the container's work folder.
- Versioned copies of files (`_v2`, `_final`, `_old`, `_copy`, `.bak`).

See `WORKSPACE.md` for where everything lives.

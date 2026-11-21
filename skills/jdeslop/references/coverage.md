# Coverage ledger

The ledger records which checks were assessed. It does not detect slop, rate design or approve a release. Keep it with the run artifacts using the project's routing rules. Use one group per distinct surface or route family and enumerate the actual evidence in the report.

```sh
python <skill>/scripts/coverage.py init audit.json --surface "Public pages" --surface "Account settings"
python <skill>/scripts/coverage.py verify audit.json
```

Read the [catalog](catalog.md) and [detailed checks](detail-checks.md) before filling the ledger. Replace `pending` with `pass`, `finding`, `exempt`, `n/a` or `blocked` in each parent and nested detail. Enter an evidence pointer or explanation with every status. A reference such as `home-mobile.png, pricing footer` or `F-04 at /settings, failed-save state` is enough when the detailed record is kept elsewhere. Reuse that pointer for multiple tests when the artifact proves each one. Describe the missing evidence for `blocked` and the intentional, working choice for `exempt`.

For a family with detailed checks, a parent summarizes those checks and the broader pattern in the catalog. A confirmed issue in a detail requires a parent `finding`. A blocked detail requires a parent `blocked` or `finding` if there is also a confirmed issue. If the same reason applies to every detail, set the parent to `n/a` or `blocked`, record that reason and omit the entire `details` field. Keep the full list if any detail can be assessed or has a different reason. A parent `pass` or `exempt` always requires its detailed records when defined.

When an ID has both a confirmed issue and incomplete evidence, use `finding` and an optional `gap` string naming the missing evidence. Summarize unresolved detail gaps in the parent `gap` when handing a finding to the visual review panel. The panel reads the parent records. Keep the complete ledger with its report.

```json
{
  "id": "UX-03",
  "status": "finding",
  "evidence": "F-04: invalid email interrupts typing in /settings",
  "gap": "The test environment cannot induce a save failure",
  "details": [
    {"id": "UX-03.field-contract", "status": "pass", "evidence": "F-04 recording at 0:06, bound label and email input"},
    {"id": "UX-03.field-necessity", "status": "pass", "evidence": "Account requirements, email for recovery"},
    {"id": "UX-03.input-assistance", "status": "pass", "evidence": "F-04 recording at 0:14, paste and Enter work"},
    {"id": "UX-03.validation-timing", "status": "finding", "evidence": "F-04 recording at 0:09, error on first character"},
    {"id": "UX-03.submission-outcome", "status": "blocked", "evidence": "Requires controlled save failure"}
  ]
}
```

To update a legacy ledger, initialize a fresh ledger with the same surface names. Carry over existing parent records and populate the nested detail records with earlier evidence wherever it establishes their outcomes. Test the rest or record genuine evidence gaps. `verify audit.json --summary-only` checks the legacy parent shape and explicitly reports that detailed coverage was skipped.

The command returns 0 for complete coverage without recorded gaps, 1 for an invalid or unfinished ledger, and 2 for complete bookkeeping with explicit evidence gaps. It verifies IDs, statuses and nonempty evidence pointers. The audit still needs a person or agent to evaluate the evidence and verify fixes. A successful verification can include open findings. Without Python, use the same IDs and statuses in a table.

## Combined CLI run

The `jdeslop` directory contains the skill, the Rust CLI, and both engines. Build the Rust CLI from the parent directory with `cargo build --locked --manifest-path jdeslop/cli/Cargo.toml`. The binary is in `jdeslop/cli/target/debug/` (`jdeslop.exe` on Windows). The CLI uses Python 3.10+ for the existing audit and review engines. Set `JDESLOP_PYTHON` or pass `--python` if your interpreter is named `python3` or lives outside PATH. If you move the skill directory after building, pass its new location with `--skills-root` or `JDESLOP_SKILLS_ROOT`. Existing configurations that point to the parent directory also work.

Use one run directory for the audit, visual decisions and any prepared source edits. Examples below assume the binary is on PATH:

```sh
jdeslop init ./runs/settings --source-root ./product --surface "Account settings"
jdeslop audit ./runs/settings
jdeslop review ./runs/settings
jdeslop status ./runs/settings
```

- **Init.** Choose the run directory according to the workspace rules. Repeat `--surface` for multiple route families or states. The CLI seeds `coverage.json` with pending checks, `review.json` with baseline filenames and a stable ID, and `run.json` with the source root and ID. Omit `--source-root` for a URL or design with no local source.
- **Audit.** The agent records real evidence in `coverage.json`. The CLI checks full coverage with the exit codes above. An unfinished audit cannot open a review. A completed audit with explicit gaps can review confirmed corrections.
- **Stage and review.** The agent fills in context, captures the baseline HTML for each surface and adds corrections in `review.json` using the [manifest format](../review/references/review-format.md). Include exact source edits for corrections that can be approved for implementation. Keep the manifest bound to this run's ledger and ID. `review` prints a local URL when source fixes are ready. With `--portable`, or when no source fix is ready, it builds `review.html`. A portable review can retain and export visual decisions for the agent.
- **Decide and apply.** In live mode, the user approves the staged revisions in the panel. The source queue begins processing approved fixes one at a time. Batch approval uses the same queue and excludes revisions already queued. `status`, which can run in another terminal, reports the audit, staged corrections, and source jobs for the current revisions. Restart `review` with the same run directory to reopen its `queue.sqlite3`. Use `--port 0` for a free port or a fixed port to preserve the browser origin for local panel decisions.
- **Refine and verify.** For a change request, regenerate the preview and source plan together, retain the correction ID and increment its revision. Reopen the same run for approval. Re-render applied changes in the product and record verification separately from the approved baseline and source queue.

### Language and appearance

```sh
jdeslop --lang pt-BR --help
jdeslop --lang pt-BR --theme dark review ./runs/settings
```

`JDESLOP_LANG` and `JDESLOP_THEME` supply defaults. Existing `ANTI_SLOP_*` environment variables are accepted as fallbacks. The review toolbar has a sun/moon switch and a language picker. It saves both choices in the browser when storage is available; the default theme follows the OS. The CLI uses the terminal's own appearance.

To add a language, add a matching locale code to the key/value catalogs for [Rust CLI messages](../cli/src/locales.json), [audit messages](../scripts/audit_locale.json), [review backend messages](../review/scripts/review_locale.json), and [review interface messages](../review/assets/review-locales.json). Set the locale's display name in the interface catalog. Keep the English keys and placeholders such as `{0}` and `{name}`; translate only the values. The interface discovers locale codes from its catalog.

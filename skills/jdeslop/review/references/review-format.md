# Review format

Run the builder with Python 3. It produces a self-contained HTML file. The review works directly from disk and can be hosted locally. A developer can rebuild the same output path to deliver a new revision. Open it in a new tab or reload after rebuilding.

The input manifest contains:

| Field | Value |
| --- | --- |
| `reviewId` | Stable ID for this review and its source queue, such as `billing-us-en`. |
| `title` | User-facing review title. |
| `context` | Route, state, fixture and known limits of the snapshot. |
| `ledger` | Relative path to the JSON ledger produced by jdeslop. |
| `surfaces` | List of `{id, title, baseline, surface, liveUrl?}`. `baseline` is the path to that standalone HTML capture, relative to the manifest. `surface` is the exact ledger surface name. `liveUrl` links to a trusted HTTP(S) page for runtime checks. For a single snapshot, the existing top-level `baseline`, `surface` and optional `liveUrl` also work. |
| `fixes` | Ordered list of correction objects. An empty list supports evidence-only review. |

Every correction supplies `id`, positive integer `revision`, `title`, `finding` (the observable issue and its impact), and `checkIds` (one or more IDs with status `finding` on affected surfaces). Set `changesBySurface` to a map from surface ID to a nonempty list of operations. An omitted surface has no preview changes for this correction. For one snapshot, a flat `changes` list also works. Optional `requires` lists prerequisite corrections earlier in the sequence. Use stable IDs with letters, digits, hyphens, underscores, periods or colons. Include styles in the captures and use data URLs for required assets.

| Operation | Example |
| --- | --- |
| `style` | `{"op":"style","target":"#summary","properties":{"padding":"24px","border-color":null}}`. Null clears an inline override. |
| `text` | `{"op":"text","target":"#intro","value":"Due on 12 May."}`. Select a leaf so its children and icons are preserved. |
| `remove` | `{"op":"remove","target":"#decorative-badge"}`. |
| `move` | `{"op":"move","target":"#pay","anchor":"#help","position":"before"}`. Also supports `after` and `append` (into the anchor). |

Each selector must identify exactly one node at its step. The renderer starts from the same baseline on every render. A failed correction stays out of the preview, the reason appears in the review, and later dependent corrections are blocked. Fix invalid selectors and inspect the cumulative result for changes that conflict visually.

**Original** shows the captured page. **Before fix** shows the page immediately before the selected correction. **After fix** shows its result. **Include earlier fixes** is on by default for a cumulative comparison. Turn it off to isolate the selected correction and its explicit prerequisites. These views keep the page scroll position within the available content and outline the affected elements. When a change removes an element, its container is outlined in the after view. Switch **Page or state** to compare the same step across captures. **Play sequence** starts at Original and includes earlier fixes. **Pause sequence** holds the current step and **Resume sequence** continues from it. The sequence includes pending and kept corrections and excludes skipped or returned corrections and their dependents. Selecting a skipped or returned correction in the list shows its proposal with earlier fixes turned off. Coverage shows the current surface's checks and linked corrections.

Use the toolbar to decide on a rendered revision. **Request changes** opens a note popover. **Send request** records the note and the decision, and **Cancel** closes the popover without saving. **Keep** and **Skip** record decisions directly. **Export review** captures choices and notes for the agent. Browser storage retains choices when available. Import checks the review ID and exact identity of each correction, including affected baselines, revision, content and prerequisites. Stale decisions are skipped and reported. The portable review only records decisions.

## Source application

Add an optional `implementation` list to each correction that can be applied from the panel. Prepare it from the real source:

```json
"implementation": [
  {
    "path": "src/components/InvoiceActions.tsx",
    "before": "<Button onClick={openPayment}>Continue</Button>",
    "after": "<Button onClick={openPayment}>Pay invoice</Button>"
  }
]
```

Paths name existing UTF-8 source files inside the explicitly selected product root. Use forward slashes. Each `before` must match exactly once after its prerequisite edits and earlier fragments in the same plan. Include enough surrounding context to identify the exact occurrence. Use `""` as `after` to delete a fragment. Keep review snapshots separate from editable source files. The plan is part of the revision fingerprint and stays on the server. One shared source edit can cover several snapshots.

```sh
python <skill>/scripts/serve.py review.json --source-root <product> --state <review-folder>/queue.sqlite3
```

Open the printed loopback URL. Use `--port` to choose a different port. Reuse the same state path and port for the same review. Use one source-enabled review per product at a time. Use distinct state files for different reviews.

Keep the captured baselines and applied revisions stable while adjusting another correction. Restaging an applied correction against a different capture changes its fingerprint. If you start a new review from freshly captured, corrected pages, stage only the remaining work.

**Approve and queue** authorizes and starts the prepared change. **Approve and queue all** includes source-ready, visually valid corrections with eligible prerequisites. It respects Skip and Request changes decisions for each correction and its prerequisites. A correction with a preview-only prerequisite stays outside the queue. Imports never enqueue work on their own. The server applies one revision at a time in queue order whenever its prerequisites have applied, with one persisted entry per exact revision. Enqueueing a dependent correction alone does not approve its prerequisites. A failed or interrupted revision stays recorded and blocks its dependents. Inspect current source and prepare a new revision instead of repeating a job with uncertain effects. Applied source changes appear in the normal project diff.

The canvas is an isolated visual snapshot. App scripts and network requests are disabled. Capture menus, dialogs and similar states as separately labeled snapshots. Use **Open live page** for navigation, keyboard behavior and interactions in the real implementation. The local queue writes changes to source. Rebuild or reload the product as appropriate and verify the actual result before recording the audit as complete.

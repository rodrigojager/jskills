"""Prepare a review from the complete ledger of a shared jdeslop run."""

import argparse
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
import json
from pathlib import Path
import sqlite3
import sys

from build import compile_payload, json_file, write_review
from serve import read_queue, serve_review
from review_locale import tr

sys.path.insert(1, str(Path(__file__).resolve().parents[2] / "scripts"))
from coverage import catalog_ids, details_by_parent, verify_ledger


def run_file(run, name):
    path = run / name
    if path.resolve() != path:
        raise ValueError(tr("{0} must be a file in the run directory.", name))
    return path


def audit(ledger, brief=False):
    ids = catalog_ids()
    details = details_by_parent(ids)
    if not brief:
        return verify_ledger(ledger, ids, details)
    with redirect_stdout(StringIO()) as summary, redirect_stderr(StringIO()):
        result = verify_ledger(ledger, ids, details)
    if result != 1:
        print(summary.getvalue(), end="")
    return result


def show_status(payload, plans, jobs, root):
    print(tr("Review: {0} surfaces, {1} staged corrections.", len(payload['surfaces']), len(payload['fixes'])))
    by_id = {job["id"]: job for job in jobs}
    for fix in payload["fixes"]:
        default = "awaiting approval" if root is not None and plans[fix["id"]] else "preview only"
        job = by_id.get(fix["id"], {})
        detail = f" | {job['detail']}" if job.get("detail") else ""
        print(f"{fix['id']} r{fix['revision']} | {tr(job.get('status', default))} | {fix['title']}{detail}")
    if any(job["status"] == "applied" for job in jobs):
        print(tr("Applied means the source edit was written. Record product verification separately."))


def prepare_review(args):
    run = args.run.resolve(strict=True)
    root = args.source_root.resolve(strict=True) if args.source_root is not None else None
    ledger_path = run_file(run, "coverage.json")
    ledger = json_file(ledger_path)
    result = audit(ledger, brief=args.status)
    if result == 1:
        print(tr("Audit incomplete or invalid. Run audit for the full report."), file=sys.stderr)
        return 1
    if result == 2:
        print(tr("Audit has evidence gaps. Review confirmed corrections and leave gaps visible."))

    try:
        payload, plans, inputs = compile_payload(
            run_file(run, "review.json"), audit_ledger=(ledger_path, ledger), review_id=args.review_id)
    except (OSError, ValueError) as error:
        raise ValueError(tr("Review needs preparation: {0}", error)) from error
    state = run_file(run, "queue.sqlite3")
    if args.status:
        jobs = [] if root is None else read_queue(state, root, payload, plans)
        show_status(payload, plans, jobs, root)
        return 0
    output = run_file(run, "review.html")
    inputs |= {run / "run.json", Path(__file__).resolve()}
    if args.portable or root is None or not any(plans.values()):
        _, count = write_review(payload, inputs | {state}, output)
        print(tr("Built portable review: {0} ({1} corrections).", output, count))
        return 0
    serve_review(root, state, args.port, payload, plans, inputs | {output})
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    parser.add_argument("--review-id", required=True)
    parser.add_argument("--source-root", type=Path)
    parser.add_argument("--port", type=int, default=8765)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--portable", action="store_true")
    mode.add_argument("--status", action="store_true")
    args = parser.parse_args()
    try:
        return prepare_review(args)
    except (OSError, sqlite3.Error, ValueError) as error:
        print(f"jdeslop: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())

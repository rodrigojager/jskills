"""Create and verify a UI anti-slop coverage ledger."""

import argparse
from collections import Counter
import json
from pathlib import Path
import re
import sys
from audit_locale import tr


CATALOG = Path(__file__).resolve().parents[1] / "references" / "catalog.md"
DETAIL_CHECKS = CATALOG.with_name("detail-checks.md")
STATUSES = {"pass", "exempt", "finding", "n/a", "blocked"}


def catalog_ids():
    ids = re.findall(r"^\| ([A-Z][A-Z0-9]*-\d{2}) \|", CATALOG.read_text(encoding="utf-8"), re.M)
    if not ids or len(set(ids)) != len(ids):
        raise ValueError(tr("The catalog is empty or contains duplicate IDs."))
    return ids


def details_by_parent(ids):
    detail_ids = re.findall(
        r"^\| ([A-Z][A-Z0-9]*-\d{2}\.[a-z][a-z0-9-]*) \|",
        DETAIL_CHECKS.read_text(encoding="utf-8"),
        re.M,
    )
    if not detail_ids or len(set(detail_ids)) != len(detail_ids):
        raise ValueError(tr("The detail catalog is empty or contains duplicate IDs."))
    parents = {check_id: [] for check_id in ids}
    for detail_id in detail_ids:
        parent = detail_id.split(".", 1)[0]
        if parent not in parents:
            raise ValueError(tr("Detail {0} has no catalog parent.", detail_id))
        parents[parent].append(detail_id)
    return parents


def nonempty(value):
    return isinstance(value, str) and bool(value.strip())


def pending_record(check_id):
    return {"id": check_id, "status": "pending", "evidence": ""}


def initialize(path, surfaces, ids, detail_groups):
    names = [name.strip() for name in surfaces]
    if not all(names) or len(set(names)) != len(names):
        raise ValueError(tr("Surface names must be nonempty and unique."))
    checks = []
    for check_id in ids:
        record = pending_record(check_id)
        if detail_groups[check_id]:
            record["details"] = [pending_record(detail_id) for detail_id in detail_groups[check_id]]
        checks.append(record)
    ledger = {"surfaces": [{"name": name, "checks": checks} for name in names]}
    with path.open("x", encoding="utf-8") as output:
        json.dump(ledger, output, indent=2, ensure_ascii=False)
        output.write("\n")
    detail_count = sum(map(len, detail_groups.values()))
    print(tr("Created {0} with {1} catalog checks and {2} details per surface across {3} surfaces.", path, len(ids), detail_count, len(names)))


def collect_records(items, expected_ids, label, problems):
    if not isinstance(items, list):
        problems.append(tr("{0}: records must be a list.", label))
        return {}
    expected = set(expected_ids)
    seen = set()
    valid = {}
    for record in items:
        if not isinstance(record, dict) or not nonempty(record.get("id")):
            problems.append(tr("{0}: every record needs a catalog ID.", label))
            continue
        check_id = record["id"]
        if check_id in seen or check_id not in expected:
            problems.append(tr("{0}: unexpected or duplicate ID {1}.", label, check_id))
            continue
        seen.add(check_id)
        status = record.get("status")
        location = f"{label} / {check_id}"
        if not isinstance(status, str) or status not in STATUSES:
            problems.append(tr("{0}: unfinished or invalid status.", location))
        elif not nonempty(record.get("evidence")):
            problems.append(tr("{0}: evidence or a reason is required.", location))
        elif "gap" in record and (status != "finding" or not nonempty(record["gap"])):
            problems.append(tr("{0}: a gap must describe missing evidence on a finding.", location))
        else:
            valid[check_id] = record
    missing = expected - seen
    if missing:
        problems.append(tr("{0}: missing IDs {1}.", label, ', '.join(sorted(missing))))
    return valid


def collect_details(check, expected, label, problems):
    status = check["status"]
    if "details" not in check and status in {"n/a", "blocked"}:
        return (), len(expected)
    details = collect_records(check.get("details", []), expected, label, problems).values()
    statuses = {detail["status"] for detail in details}
    if "finding" in statuses and status != "finding":
        problems.append(tr("{0}: a confirmed detail requires a parent finding.", label))
    if "blocked" in statuses and status not in {"finding", "blocked"}:
        problems.append(tr("{0}: a blocked detail requires a blocked parent or a finding.", label))
    if status == "n/a" and statuses - {"n/a"}:
        problems.append(tr("{0}: an inapplicable parent cannot have applicable details.", label))
    return details, 0


def verify(path, ids, detail_groups):
    ledger = json.loads(path.read_text(encoding="utf-8-sig"))
    return verify_ledger(ledger, ids, detail_groups)


def verify_ledger(ledger, ids, detail_groups):
    surfaces = ledger.get("surfaces") if isinstance(ledger, dict) else None
    if not isinstance(surfaces, list) or not surfaces:
        raise ValueError(tr("The ledger needs at least one surface."))

    names = set()
    totals = Counter()
    detail_totals = Counter()
    findings_with_gaps = 0
    detail_gaps = 0
    inherited = 0
    problems = []
    for surface in surfaces:
        if not isinstance(surface, dict) or not nonempty(surface.get("name")):
            problems.append(tr("Every surface needs a name."))
            continue
        name = surface["name"].strip()
        if name in names:
            problems.append(tr("Duplicate surface: {0}.", name))
        names.add(name)
        for check_id, check in collect_records(surface.get("checks"), ids, name, problems).items():
            totals[check["status"]] += 1
            findings_with_gaps += int("gap" in check)
            if detail_groups is None:
                continue
            details, inherited_count = collect_details(
                check, detail_groups[check_id], f"{name} / {check_id}", problems
            )
            inherited += inherited_count
            detail_totals[check["status"]] += inherited_count
            for detail in details:
                detail_totals[detail["status"]] += 1
                detail_gaps += int("gap" in detail)

    if problems:
        for problem in problems:
            print(problem, file=sys.stderr)
        return 1
    summary = ", ".join(f"{tr(status)} {totals[status]}" for status in sorted(STATUSES))
    print(tr("{0} checks × {1} surfaces: {2}, findings with gaps {3}.", len(ids), len(names), summary, findings_with_gaps))
    if detail_groups is None:
        print(tr("Summary only: detailed coverage was not verified."))
    else:
        detail_summary = ", ".join(f"{tr(status)} {detail_totals[status]}" for status in sorted(STATUSES))
        detail_count = sum(map(len, detail_groups.values()))
        print(tr("{0} details × {1} surfaces: {2}, findings with gaps {3}, inherited statuses {4}.", detail_count, len(names), detail_summary, detail_gaps, inherited))
    return 2 if totals["blocked"] or findings_with_gaps or detail_totals["blocked"] or detail_gaps else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    create = commands.add_parser("init")
    create.add_argument("path", type=Path)
    create.add_argument("--surface", action="append", required=True)
    check = commands.add_parser("verify")
    check.add_argument("path", type=Path)
    check.add_argument("--summary-only", action="store_true", help="Verify parent checks in a legacy ledger.")
    args = parser.parse_args()
    try:
        ids = catalog_ids()
        if args.command == "verify" and args.summary_only:
            return verify(args.path, ids, None)
        detail_groups = details_by_parent(ids)
        if args.command == "verify":
            return verify(args.path, ids, detail_groups)
        initialize(args.path, args.surface, ids, detail_groups)
        return 0
    except (OSError, ValueError) as error:
        print(error, file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())

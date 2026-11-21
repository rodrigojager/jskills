"""Compile an audit ledger and proposed changes into a portable review."""

import argparse
import hashlib
import json
from pathlib import Path
from review_locale import preference, tr
import re
from urllib.parse import urlsplit


TEMPLATE = Path(__file__).resolve().parents[1] / "assets" / "review.html"
STATUSES = {"pass", "finding", "exempt", "n/a", "blocked", "pending"}
IDENTIFIER = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.:-]*")
PROPERTY = re.compile(r"(?:--[A-Za-z0-9_-]+|[a-z][a-z-]*)")


def json_file(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def require(condition, message):
    if not condition:
        raise ValueError(message)


def text(record, key):
    value = record.get(key)
    require(isinstance(value, str) and value.strip(), tr("Missing or empty {0}.", key))
    return value


def identifier(record, key):
    value = text(record, key)
    require(IDENTIFIER.fullmatch(value), tr("Invalid {0}: {1}.", key, value))
    return value


def records(value, label):
    require(isinstance(value, list) and all(isinstance(item, dict) for item in value),
            tr("{0} must be an array of objects.", label))
    return value


def unique_references(value, allowed, label):
    require(isinstance(value, list) and all(isinstance(item, str) for item in value),
            tr("{0} must be an array of IDs.", label))
    require(len(set(value)) == len(value) and all(item in allowed for item in value),
            tr("{0} contains duplicate or unknown IDs.", label))
    return value


def asset(directory, name):
    path = (directory / name).resolve()
    require(path.is_relative_to(directory), tr("Input must be inside the review folder: {0}.", name))
    return path


def select_checks(ledger, surface_name):
    require(isinstance(ledger, dict), tr("The ledger must be an object."))
    selected = [surface for surface in records(ledger.get("surfaces"), "Ledger surfaces")
                if surface.get("name") == surface_name]
    require(len(selected) == 1, tr("Expected one ledger surface named {0}.", surface_name))
    checks = {}
    for item in records(selected[0].get("checks"), "Ledger checks"):
        key = identifier(item, "id")
        status = text(item, "status")
        require(key not in checks and status in STATUSES, tr("Invalid or duplicate check {0}.", key))
        checks[key] = {
            "id": key,
            "status": status,
            "evidence": item.get("evidence", "") if status == "pending" else text(item, "evidence"),
        }
        if "gap" in item:
            require(status == "finding", tr("Only a finding can carry a separate gap: {0}.", key))
            checks[key]["gap"] = text(item, "gap")
    require(checks, tr("Surface {0} has no audit checks.", surface_name))
    return checks


def validate_change(change):
    operation = text(change, "op")
    text(change, "target")
    require(operation in {"style", "text", "remove", "move"}, tr("Unknown operation: {0}.", operation))
    if operation == "style":
        props = change.get("properties")
        require(isinstance(props, dict) and props, tr("A style operation needs properties."))
        for key, value in props.items():
            require(PROPERTY.fullmatch(key), tr("Invalid CSS property: {0}.", key))
            require(value is None or isinstance(value, str) and value.strip(),
                    tr("{0} requires a CSS value or null.", key))
    elif operation == "text":
        require(isinstance(change.get("value"), str), tr("A text operation needs a string value."))
    elif operation == "move":
        text(change, "anchor")
        require(change.get("position") in ("before", "after", "append"),
                tr("A move operation needs before, after or append."))


def fingerprint(value):
    data = json.dumps(value, ensure_ascii=False).encode("utf-8")
    return hashlib.sha256(data).hexdigest()


def compile_payload(path, *, audit_ledger=None, review_id=None):
    directory = path.resolve().parent
    manifest = json_file(path)
    require(isinstance(manifest, dict), tr("The review manifest must be an object."))
    manifest_id = identifier(manifest, "reviewId")
    require(review_id is None or manifest_id == review_id,
            tr("Review ID must match run.json."))
    ledger_path = asset(directory, text(manifest, "ledger"))
    inputs = {path.resolve(), TEMPLATE, ledger_path}
    if audit_ledger is None:
        ledger = json_file(ledger_path)
    else:
        expected_path, ledger = audit_ledger
        require(ledger_path == expected_path, tr("Review ledger must be this run's coverage.json."))
    legacy = "surfaces" not in manifest
    surface_specs = ([{"id": "default", "title": text(manifest, "surface"), **{
        key: manifest[key] for key in ("baseline", "surface", "liveUrl") if key in manifest
    }}] if legacy else records(manifest["surfaces"], "Surfaces"))
    require(surface_specs, tr("The review requires at least one surface."))
    if audit_ledger is not None:
        require({text(spec, "surface") for spec in surface_specs}
                == {surface["name"] for surface in ledger["surfaces"]},
                tr("Include a baseline for every audited surface in review.json."))
    surfaces = {}
    for spec in surface_specs:
        key = identifier(spec, "id")
        require(key not in surfaces, tr("Duplicate surface ID: {0}.", key))
        baseline_path = asset(directory, text(spec, "baseline"))
        inputs.add(baseline_path)
        baseline = baseline_path.read_text(encoding="utf-8-sig")
        require(baseline.strip(), tr("The baseline HTML for {0} is empty.", key))
        surface = {
            "id": key, "title": text(spec, "title"), "baseline": baseline,
            "checks": list(select_checks(ledger, text(spec, "surface")).values()),
        }
        if "liveUrl" in spec:
            url = text(spec, "liveUrl")
            parts = urlsplit(url)
            require(parts.scheme in {"http", "https"} and parts.hostname and parts.username is None,
                    tr("{0} liveUrl must be an HTTP(S) URL without credentials.", key))
            surface["liveUrl"] = url
        surfaces[key] = surface

    fixes = {}
    plans = {}
    for fix in records(manifest.get("fixes"), "Fixes"):
        key = identifier(fix, "id")
        require(key not in fixes, tr("Duplicate correction ID: {0}.", key))
        revision = fix.get("revision")
        require(type(revision) is int and revision >= 1, tr("{0} needs a positive revision.", key))
        requires = unique_references(fix.get("requires", []), fixes, f"{key} requires")
        changes = fix.get("changesBySurface")
        if changes is None and len(surfaces) == 1:
            changes = {next(iter(surfaces)): fix.get("changes")}
        require(isinstance(changes, dict) and changes and changes.keys() <= surfaces.keys(),
                tr("{0} requires changes for known surfaces.", key))
        require("changesBySurface" not in fix or "changes" not in fix,
                tr("{0} must use either changes or changesBySurface.", key))
        for surface_id, operations in changes.items():
            require(operations, tr("{0} has no visual changes on {1}.", key, surface_id))
            for change in records(operations, f"{key} changes for {surface_id}"):
                validate_change(change)
        evidence = [check for surface_id in changes for check in surfaces[surface_id]["checks"]
                    if check["status"] == "finding"]
        linked = unique_references(fix.get("checkIds"), {check["id"] for check in evidence},
                                   f"{key} checkIds")
        require(linked, tr("{0} must link to a finding.", key))
        plan = records(fix.get("implementation", []), f"{key} implementation")
        for edit in plan:
            text(edit, "path")
            before = text(edit, "before")
            after = edit.get("after")
            require(isinstance(after, str) and after != before,
                    tr("{0} source edit needs a different after string.", key))
        plans[key] = plan
        staged = {
            "id": key, "revision": revision, "title": text(fix, "title"),
            "finding": text(fix, "finding"), "checkIds": linked, "requires": requires,
            "changesBySurface": changes,
        }
        dependencies = [fixes[dependency]["fingerprint"] for dependency in requires]
        if legacy and not plan:
            # Keep exports from the original portable single-surface builder valid.
            previous = {field: value for field, value in staged.items() if field != "changesBySurface"}
            previous["changes"] = changes["default"]
            by_check = {check["id"]: check for check in evidence}
            identity = [fingerprint(surfaces["default"]["baseline"]), previous,
                        [by_check[check_id] for check_id in linked], dependencies]
        else:
            identity = [staged, {sid: fingerprint(surfaces[sid]["baseline"]) for sid in changes},
                        [check for check in evidence if check["id"] in linked], dependencies, plan]
        staged["fingerprint"] = fingerprint(identity)
        fixes[key] = staged
    payload = {
        "reviewId": manifest_id, "title": text(manifest, "title"),
        "context": text(manifest, "context"), "surfaces": list(surfaces.values()),
        "fixes": list(fixes.values()),
    }
    return payload, plans, inputs


def render_page(payload, live=False):
    template = TEMPLATE.read_text(encoding="utf-8")
    catalog_path = TEMPLATE.with_name("review-locales.json")
    locales = json.loads(catalog_path.read_text(encoding="utf-8"))
    template = template.replace("__DEFAULT_LANG__", preference("LANG", locales, "en"), 1)
    template = template.replace("__DEFAULT_THEME__", preference("THEME", ("light", "dark", "system"), "system"), 1)
    template = template.replace("__REVIEW_LOCALES__", json.dumps(locales, ensure_ascii=True).replace("<", "\\u003c"), 1)
    if live:
        template = template.replace("default-src 'none'; script-src", "default-src 'none'; connect-src 'self'; script-src", 1)
    # Keep input HTML and copy inert inside the JSON script element.
    encoded = json.dumps(payload, ensure_ascii=True).replace("<", "\\u003c")
    return template.replace("__REVIEW_PAYLOAD__", encoded, 1)


def compile_review(path, output):
    payload, _, inputs = compile_payload(path)
    return write_review(payload, inputs, output)


def write_review(payload, inputs, output):
    require(output.resolve() not in inputs, tr("The output must be separate from the review inputs."))
    page = render_page(payload)
    if output.exists():
        previous = output.read_text(encoding="utf-8")
        generators = ("jdeslop", "jdeslop-review", "ui-anti-slop-review")
        require(any(f'<meta name="generator" content="{name}">' in previous for name in generators),
                tr("Refusing to replace a file that was not generated by this builder."))
    with output.open("w", encoding="utf-8", newline="\n") as target:
        target.write(page)
    return sum(len(surface["checks"]) for surface in payload["surfaces"]), len(payload["fixes"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    try:
        checks, fixes = compile_review(args.manifest, args.output)
    except (OSError, ValueError) as error:
        parser.exit(1, f"Review could not be built: {error}\n")
    print(tr("Built {0} with {1} checks and {2} corrections.", args.output, checks, fixes))


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Compose version-1 jtasks intent from a short spec and a plan written by to-plan.

The spec holds only what cannot be derived: the slicing, dependencies, executor choices, requirement coverage and
operations. Text is copied from the plan files by pointers, and repeated task content comes from group templates.
Nothing is executed or imported; with --jtasks the public CLI validates and previews the composed intent.
"""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys

CONTEXT_LISTS = ("requirements", "references", "dependency_artifacts", "operations", "configuration", "limits",
                 "resources")
INTENT_KEYS = ("schema_version", "completion", "operation_placement", "scheduling", "profile_defaults")
SPEC_KEYS = set(INTENT_KEYS) | {"compose_version", "plan", "output", "vars", "requirements", "context",
                                "auto_references", "executors", "unknown_capabilities", "lookups", "groups",
                                "tasks", "operations", "expect", "allow_uncovered"}
TEMPLATE_KEYS = {"title", "executor", "requirements", "use_cases", "depends_on", "admission", "context", "lookups",
                 "vars", "tags", "recovery", "reviews", "estimate_seconds", "catalog_scope", "unknown_capabilities"}
EDGE_KINDS = ("operation", "task_candidate", "task_completed")


class ComposeError(Exception):
    pass


def require(condition, message):
    if not condition:
        raise ComposeError(message)


def one_line(text):
    return re.sub(r"\s+", " ", text).strip()


# --- Markdown of a to-plan plan: headings, sections, tables, list items ---------------------------------------------
class Document:
    def __init__(self, path):
        require(path.is_file(), f"plan file not found: {path}")
        self.path = path
        self.lines = path.read_text(encoding="utf-8").splitlines()
        self.headings = []  # (line index, level, text)
        fenced = False
        for index, line in enumerate(self.lines):
            if line.lstrip().startswith("```"):
                fenced = not fenced
            match = None if fenced else re.match(r"(#{1,6}) +(.+?)\s*$", line)
            if match:
                self.headings.append((index, len(match.group(1)), match.group(2)))
        self._tables = None

    @staticmethod
    def _same(heading, wanted):
        return heading == wanted or re.sub(r"^\d+(\.\d+)*\.? +", "", heading) == wanted

    def title(self):
        require(self.headings and self.headings[0][1] == 1, f"{self.path.name}: no '# Title'")
        return self.headings[0][2]

    def span(self, wanted):
        found = [h for h in self.headings if self._same(h[2], wanted)]
        require(found, f"{self.path.name}: no section '{wanted}'")
        require(len(found) == 1, f"{self.path.name}: section '{wanted}' is ambiguous ({len(found)} headings)")
        start, level, _ = found[0]
        end = next((i for i, lvl, _ in self.headings if i > start and lvl <= level), len(self.lines))
        return start + 1, end

    def section(self, wanted):
        start, end = self.span(wanted)
        return one_line(" ".join(self.lines[start:end]))

    def raw_section(self, wanted):
        """The section as written, heading included, for a read file."""
        start, end = self.span(wanted)
        return "\n".join(self.lines[start - 1:end]).strip("\n")

    def raw_item(self, wanted, key):
        """One list item as written (its continuation lines included), for a read file."""
        start, end = self.span(wanted)
        found = []
        for index in range(start, end):
            match = re.match(r"(?:(\d+)\.|[-*]) +(?:\*\*(.+?)\*\*)?", self.lines[index])
            if match and str(key) in (match.group(1), match.group(2)):
                stop = index + 1
                while stop < end and self.lines[stop].strip() and not re.match(r"(?:\d+\.|[-*]) +", self.lines[stop]):
                    stop += 1
                found.append("\n".join(self.lines[index:stop]))
        require(len(found) == 1, f"{self.path.name}: {len(found)} items '{key}' in '{wanted}'")
        return found[0]

    def items(self, wanted):
        """List items of a section: (key, text). The key is the number, or the leading **bold** words of a bullet."""
        start, end = self.span(wanted)
        items, current = [], None
        for index in range(start, end):
            line = self.lines[index]
            match = re.match(r"(?:(\d+)\.|[-*]) +(.*)$", line)
            if match:
                current = [match.group(1), [match.group(2)]]
                items.append(current)
            elif not line.strip():
                following = next((l for l in self.lines[index + 1:end] if l.strip()), "")
                if not following.startswith((" ", "\t")):
                    current = None
            elif current is not None and (line.startswith((" ", "\t")) or self.lines[index - 1].strip()):
                current[1].append(line)
        result = []
        for number, parts in items:
            text = one_line(" ".join(parts))
            bold = re.match(r"\*\*(.+?)\*\*", text)
            key = number or (bold.group(1) if bold else None)
            if bold:
                text = bold.group(1) + text[bold.end():]
            result.append((key, text))
        return result

    def tables(self):
        """Every table row as (line index, {header: cell}, [cells])."""
        if self._tables is None:
            self._tables = []
            header = previous = None

            def split(line):
                return [c.strip().replace("\\|", "|") for c in re.split(r"(?<!\\)\|", line.strip())[1:-1]]

            def separator(cells):
                return bool(cells) and all(re.fullmatch(r":?-{3,}:?", c) for c in cells)

            def record(index, header, cells):
                # Beside the header names: {#1}, {#2}... by position, and {#section}, the heading the row is under.
                row = dict(zip(header, cells))
                row.update({f"#{n}": cell for n, cell in enumerate(cells, 1)})
                row["#section"] = next((text for at, _, text in reversed(self.headings) if at < index), "")
                row["#line"], row["#header"] = index, header_line[0]
                self._tables.append((index, row, cells))

            header_line = [None]

            for index, line in enumerate(self.lines):
                if not line.startswith("|"):
                    header = None
                    continue
                cells = split(line)
                following = self.lines[index + 1] if index + 1 < len(self.lines) else ""
                if header is None and following.startswith("|") and separator(split(following)):
                    header = previous = cells
                    header_line[0] = index
                elif header is None and previous is not None and len(cells) == len(previous):
                    # A row cut off from its table by a blank line still belongs to it.
                    header = previous
                    record(index, header, cells)
                elif header is None:
                    raise ComposeError(f"{self.path.name}:{index + 1}: a table row without a header")
                elif separator(cells):
                    continue
                else:
                    require(len(cells) == len(header),
                            f"{self.path.name}:{index + 1}: row has {len(cells)} cells, its table has {len(header)}")
                    record(index, header, cells)
        return self._tables


class Plan:
    def __init__(self, plan_file):
        self.dir = plan_file.parent
        self.main = plan_file.name
        self.documents = {}

    def document(self, name=None):
        name = name or self.main
        if name not in self.documents:
            self.documents[name] = Document(self.dir / name)
        return self.documents[name]

    def path(self, name):
        return (self.dir / name).as_posix()

    def source(self, pointer, where):
        """A text pointer: {"file", "section"} for a whole section, plus "item" for one list item of it."""
        require(isinstance(pointer, dict) and "section" in pointer and set(pointer) <= {"file", "section", "item"},
                f"{where}: a text pointer has 'section', optional 'file' and optional 'item'")
        document = self.document(pointer.get("file"))
        if "item" not in pointer:
            return document.section(pointer["section"])
        matches = [text for key, text in document.items(pointer["section"]) if key == str(pointer["item"])]
        require(len(matches) == 1,
                f"{where}: {len(matches)} items '{pointer['item']}' in '{pointer['section']}' of {document.path.name}")
        return matches[0]


# --- Templates: {id}, {plan_dir}, {var.NAME}, {LOOKUP.COLUMN}; doubled braces are literal ---------------------------
class Skip(Exception):
    """A list line whose optional or many lookup found nothing is left out."""


def render(template, resolve, where):
    def replace(match):
        if match.group(0) in ("{{", "}}"):
            return match.group(0)[0]
        return resolve(match.group(1))

    require("\n" not in template, f"{where}: a template is one line")
    stripped = re.sub(r"\{\{|\}\}|\{[^{}\n]+\}", "", template)
    require("{" not in stripped and "}" not in stripped, f"{where}: unbalanced brace (double it to keep it): {template[:80]}")
    return re.sub(r"\{\{|\}\}|\{([^{}\n]+)\}", replace, template)


class Reads:
    """Read files written beside the intent. One name holds one content; tasks that read the same text share it."""

    def __init__(self, folder):
        self.folder, self.files = folder, {}

    def add(self, name, content, where):
        file = name if name.endswith(".md") else name + ".md"
        require(self.files.get(file, content) == content,
                f"{where}: read file '{file}' would hold two different texts; give each its own name (for example with {{id}})")
        self.files[file] = content
        return (self.folder / file).as_posix()

    MANIFEST = ".compose-manifest.json"

    def check(self):
        problems = []
        for file, content in self.files.items():
            path = self.folder / file
            if not path.is_file() or path.read_text(encoding="utf-8") != content:
                problems.append(file)
        previous = self.previous()
        problems += [f"{file} (no longer composed)" for file in previous if file not in self.files and (self.folder / file).exists()]
        return problems

    def previous(self):
        manifest = self.folder / self.MANIFEST
        return json.loads(manifest.read_text(encoding="utf-8")) if manifest.is_file() else []

    def write(self):
        # Only files this helper wrote before are removed; anything else in the folder is left alone.
        for file in self.previous():
            if file not in self.files and (self.folder / file).is_file():
                (self.folder / file).unlink()
        if not self.files:
            if (self.folder / self.MANIFEST).is_file():
                (self.folder / self.MANIFEST).unlink()
            return
        self.folder.mkdir(parents=True, exist_ok=True)
        for file, content in self.files.items():
            (self.folder / file).write_text(content, encoding="utf-8", newline="\n")
        (self.folder / self.MANIFEST).write_text(json.dumps(sorted(self.files), indent=1) + "\n", encoding="utf-8", newline="\n")


class Scope:
    def __init__(self, plan, variables, lookups, task_id, where, reads=None):
        self.plan, self.variables, self.lookups, self.task_id, self.where = plan, variables, lookups, task_id, where
        self.reads = reads
        self.cache = {}

    def base(self, name):
        if name == "id" and self.task_id is not None:
            return self.task_id
        if name == "plan_dir":
            return self.plan.dir.as_posix()
        if name.startswith("var."):
            require(name[4:] in self.variables, f"{self.where}: unknown variable '{name[4:]}'")
            return str(self.variables[name[4:]])
        return None

    def rows(self, name):
        if name not in self.cache:
            lookup = self.lookups[name]
            require(isinstance(lookup, dict) and "file" in lookup and
                    set(lookup) <= {"file", "section", "column", "key", "many", "optional", "fields"},
                    f"lookup '{name}': needs 'file'; knows section, column, key, many, optional, fields")
            document = self.plan.document(lookup["file"])
            start, end = document.span(lookup["section"]) if "section" in lookup else (0, len(document.lines))
            key = render(lookup.get("key", "{id}"), self.plain, f"lookup '{name}'")
            column = lookup.get("column")
            found = []
            for index, row, cells in document.tables():
                if start <= index < end and (cells[0] if column is None else row.get(column)) == key:
                    found.append(row)
            if not lookup.get("many"):
                require(len(found) <= 1, f"{self.where}: lookup '{name}' matches {len(found)} rows for '{key}'")
                require(found or lookup.get("optional"), f"{self.where}: lookup '{name}' finds no row for '{key}'")
            self.cache[name] = found
        return self.cache[name]

    def plain(self, name):
        value = self.base(name)
        require(value is not None, f"{self.where}: unknown placeholder '{{{name}}}'")
        return value

    def resolve(self, name):
        value = self.base(name)
        if value is not None:
            return value
        lookup, _, field = name.partition(".")
        require(lookup in self.lookups and field, f"{self.where}: unknown placeholder '{{{name}}}'")
        rows = self.rows(lookup)
        definition = self.lookups[lookup]
        if not definition.get("many"):
            if not rows:
                raise Skip()
            require(field in rows[0], f"{self.where}: lookup '{lookup}' has no column '{field}' "
                                      f"(columns: {', '.join(rows[0])})")
            return rows[0][field]
        fields = definition.get("fields", {})
        require(field in fields, f"{self.where}: many-lookup '{lookup}' has no field '{field}'")
        spec = fields[field]
        parts = []
        for row in rows:
            if spec.get("require") and not row.get(spec["require"]):
                continue

            def cell(column, row=row):
                require(column in row, f"lookup '{lookup}': no column '{column}' (columns: {', '.join(row)})")
                return row[column]

            parts.append(render(spec["each"], cell, f"lookup '{lookup}' field '{field}'"))
        if not parts:
            raise Skip()
        return spec.get("join", ", ").join(parts)

    def read_file(self, value, where):
        """{"read": name, "title": ..., "from": [sources]}: a file holding only what the task reads, and the line
        that points the task to it."""
        require(set(value) <= {"read", "title", "from"} and value.get("from"),
                f"{where}: a read file has 'read' (its name), optional 'title' and a nonempty 'from'")
        name = render(value["read"], self.plain, where)
        require(re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", name), f"{where}: read file name '{name}' is not a plain file name")
        title = render(value["title"], self.resolve, where) if "title" in value else name
        parts = []
        for source in value["from"]:
            require(isinstance(source, dict) and len(set(source) & {"section", "rows", "text"}) == 1,
                    f"{where}: a read source is a section (optional file, item), 'rows' of a lookup or 'text'")
            if "text" in source:
                parts.append(render(source["text"], self.resolve, where))
            elif "rows" in source:
                require(source["rows"] in self.lookups, f"{where}: unknown lookup '{source['rows']}'")
                rows = self.rows(source["rows"])
                if not rows:
                    continue
                document = self.plan.document(self.lookups[source["rows"]]["file"])
                tables = {}
                for row in rows:
                    tables.setdefault(row["#header"], []).append(document.lines[row["#line"]])
                for header, lines in tables.items():
                    parts.append("\n".join([document.lines[header], document.lines[header + 1], *lines]))
            else:
                document = self.plan.document(source.get("file"))
                parts.append(document.raw_item(source["section"], source["item"]) if "item" in source
                             else document.raw_section(source["section"]))
        if not parts:
            raise Skip()
        return self.reads.add(name, f"# {title}\n\n" + "\n\n".join(parts) + "\n", where), title

    def text(self, value, where):
        if isinstance(value, dict) and "read" in value:
            path, title = self.read_file(value, where)
            return f"Read {path} ({title}): it holds only what this task needs from the plan."
        if isinstance(value, dict):
            return self.plan.source(value, where)
        require(isinstance(value, str), f"{where}: expected a string or a text pointer")
        return render(value, self.resolve, where)

    def lines(self, values, where):
        require(isinstance(values, list), f"{where}: expected a list")
        result = []
        for value in values:
            try:
                result.append(self.text(value, where))
            except Skip:
                pass
        return result


# --- Composition -----------------------------------------------------------------------------------------------------
def merged(group, task):
    """A task over its group: scalars replace, lists append after the group's, maps merge."""
    result = {}
    for key in list(group) + [k for k in task if k not in group]:
        if key in ("id", "group"):
            continue
        low, high = group.get(key), task.get(key)
        if key == "context":
            low, high = low or {}, high or {}
            context = {}
            for name in dict.fromkeys(list(low) + list(high)):
                if name == "brief":
                    context[name] = high.get(name, low.get(name))
                else:
                    context[name] = list(low.get(name, [])) + list(high.get(name, []))
            result[key] = context
        elif key == "admission":
            low, high = low or {}, high or {}
            result[key] = {**low, **high, "resources": list(low.get("resources", [])) + list(high.get("resources", []))}
        elif isinstance(low, list) or isinstance(high, list):
            result[key] = list(low or []) + list(high or [])
        elif key in ("lookups", "vars"):
            result[key] = {**(low or {}), **(high or {})}
        else:
            result[key] = high if key in task else low
    return result


def compose(spec, spec_path, output):
    require(isinstance(spec, dict), "the spec is a JSON object")
    require(spec.get("compose_version") == 1, "the spec needs compose_version: 1")
    unknown = set(spec) - SPEC_KEYS
    require(not unknown, f"unknown spec keys: {', '.join(sorted(unknown))}")
    for key in ("plan", "completion", "operation_placement", "tasks"):
        require(key in spec, f"the spec needs '{key}' (completion and operation_placement are the user's answers)")
    plan = Plan((spec_path.parent / spec["plan"]).resolve())
    main = plan.document()
    variables = spec.get("vars", {})
    top_lookups = spec.get("lookups", {})
    output = output.resolve()
    reads = Reads(output.parent / (output.stem + ".reads"))
    top = Scope(plan, variables, top_lookups, None, "context", reads)

    # Requirements: the plan's use cases by number, then literal or pointed ones.
    requirements, prefix, width = [], None, 2
    for index, entry in enumerate(spec.get("requirements", [])):
        where = f"requirements[{index}]"
        if "use_cases" in entry:
            require(prefix is None, "requirements: use_cases appears once")
            prefix = entry["use_cases"]
            cases = [(key, text) for key, text in main.items("Use cases") if key and key.isdigit()]
            require(cases, f"{main.path.name}: the 'Use cases' section has no numbered items")
            require([int(k) for k, _ in cases] == list(range(1, len(cases) + 1)),
                    f"{main.path.name}: use cases are not numbered 1..{len(cases)}")
            width = max(2, len(str(len(cases))))
            requirements += [{"id": f"{prefix}{int(k):0{width}d}", "text": text} for k, text in cases]
        else:
            require(set(entry) == {"id", "text"}, f"{where}: has 'id' and 'text', or 'use_cases'")
            requirements.append({"id": entry["id"], "text": top.text(entry["text"], where)})
    known = [r["id"] for r in requirements]
    require(len(known) == len(set(known)), "requirements: duplicate id")

    # Shared context: the plan's own paragraph for executors and its parts, then what the spec adds.
    context = dict(spec.get("context", {}))
    shared = {}
    if "brief" in context:
        shared["brief"] = top.text(context.pop("brief"), "context.brief")
    else:
        shared["brief"] = f"{main.title()}. {main.section('Context for executors')}"
    references = []
    if spec.get("auto_references", True):
        references.append(plan.path(plan.main))
        parts = [text for _, text in main.items("Parts")] if any(
            Document._same(h[2], "Parts") for h in main.headings) else []
        for text in parts:
            match = re.match(r"\[([^\]]+)\]\([^)]+\):\s*(.+)$", text)
            require(match, f"{main.path.name}: a Parts line is '[PLAN-x.md](PLAN-x.md): kind: what': {text[:60]}")
            references.append(f"{plan.path(match.group(1))} ({match.group(2)})")
    for name in CONTEXT_LISTS:
        values = top.lines(context.pop(name, []), f"context.{name}")
        if name == "references":
            values = references + values
        if values:
            shared[name] = values
    require(not context, f"context: unknown keys {', '.join(context)}")

    # Tasks.
    groups = spec.get("groups", {})
    executors = spec.get("executors", {})
    tasks, tags = [], {}
    for index, entry in enumerate(spec["tasks"]):
        require(isinstance(entry, dict) and "id" in entry, f"tasks[{index}]: needs an id")
        task_id = entry["id"]
        where = f"task {task_id}"
        group = {}
        if "group" in entry:
            require(entry["group"] in groups, f"{where}: unknown group '{entry['group']}'")
            group = groups[entry["group"]]
            tags.setdefault(entry["group"], []).append(task_id)
        for label, source in (("group", group), ("task", entry)):
            extra = set(source) - TEMPLATE_KEYS - {"id", "group"}
            require(not extra, f"{where}: unknown {label} keys {', '.join(sorted(extra))}")
        item = merged(group, entry)
        for tag in item.pop("tags", []):
            tags.setdefault(tag, []).append(task_id)
        scope = Scope(plan, {**variables, **item.pop("vars", {})}, {**top_lookups, **item.pop("lookups", {})},
                      task_id, where, reads)
        require("title" in item and "executor" in item, f"{where}: needs a title and an executor")
        task = {"id": task_id, "title": scope.text(item.pop("title"), where)}
        covers = list(item.pop("requirements", []))
        cases = item.pop("use_cases", [])
        require(not cases or prefix is not None, f"{where}: use_cases needs a use_cases entry in requirements")
        covers += [f"{prefix}{int(n):0{width}d}" for n in cases]
        missing = [r for r in covers if r not in known]
        require(not missing, f"{where}: unknown requirements {', '.join(missing)}")
        if covers:
            task["requirements"] = list(dict.fromkeys(covers))
        if item.get("depends_on"):
            task["depends_on"] = item["depends_on"]
        item.pop("depends_on", None)
        executor = item.pop("executor")
        if isinstance(executor, str):
            require(executor in executors, f"{where}: unknown executor '{executor}'")
            executor = executors[executor]
        task["executor"] = executor
        exception = item.pop("unknown_capabilities", spec.get("unknown_capabilities"))
        if exception:
            task["unknown_capabilities"] = exception
        template = item.pop("context", {})
        rendered = {}
        if "brief" in template:
            rendered["brief"] = scope.text(template["brief"], where)
        for name in CONTEXT_LISTS:
            values = scope.lines(template.get(name, []), f"{where} context.{name}")
            if values:
                rendered[name] = values
        extra = set(template) - set(CONTEXT_LISTS) - {"brief"}
        require(not extra, f"{where}: unknown context keys {', '.join(sorted(extra))}")
        if rendered:
            task["context"] = rendered
        admission = item.pop("admission", None)
        if admission and (admission.get("resources") or len(admission) > 1):
            task["admission"] = {**admission, "resources": scope.lines(admission.get("resources", []), where)}
        task.update(item)
        tasks.append(task)
    ids = [t["id"] for t in tasks]
    require(len(ids) == len(set(ids)), "tasks: duplicate id")

    def expand(names, where):
        result = []
        for name in names:
            if isinstance(name, str) and name.startswith("@"):
                require(name[1:] in tags, f"{where}: unknown group or tag '{name}'")
                result += tags[name[1:]]
            else:
                result.append(name)
        return list(dict.fromkeys(result))

    for task in tasks:
        if "depends_on" in task:
            where = f"task {task['id']}"
            task["depends_on"] = [d for d in expand(task["depends_on"], where) if d != task["id"]]
            missing = [d for d in task["depends_on"] if d not in ids]
            require(not missing, f"{where}: depends on unknown tasks {', '.join(missing)}")

    # Operations: literal, with @tag lists and "kind:id" dependency shorthand.
    operations = []
    for index, entry in enumerate(spec.get("operations", [])):
        operation = json.loads(json.dumps(entry))
        where = f"operation {operation.get('id', index)}"
        stage = operation.get("stage", {})
        for key in ("consumers", "tasks"):
            if key in stage:
                stage[key] = expand(stage[key], where)
                missing = [t for t in stage[key] if t not in ids]
                require(not missing, f"{where}: stage names unknown tasks {', '.join(missing)}")
        edges = []
        for edge in operation.get("depends_on", []):
            if isinstance(edge, str):
                kind, _, target = edge.partition(":")
                require(kind in EDGE_KINDS and target, f"{where}: a dependency is 'kind:id' with kind in {EDGE_KINDS}")
                edges += [{"kind": kind, "id": t} for t in expand([target], where)]
            else:
                edges.append(edge)
        if edges:
            operation["depends_on"] = edges
        for task in [operation.get("repair_task")] + [e["id"] for e in edges if e["kind"] != "operation"]:
            require(task is None or task in ids, f"{where}: names unknown task '{task}'")
        operations.append(operation)

    covered = {r for t in tasks for r in t.get("requirements", [])}
    uncovered = [r for r in known if r not in covered and r not in spec.get("allow_uncovered", [])]
    require(not uncovered, f"requirements no task covers: {', '.join(uncovered)} (cover them or list them in allow_uncovered)")
    expected = spec.get("expect", {})
    for key, actual in (("tasks", len(tasks)), ("operations", len(operations)), ("requirements", len(requirements))):
        require(key not in expected or expected[key] == actual, f"expect.{key} is {expected.get(key)}, composed {actual}")

    intent = {"schema_version": spec.get("schema_version", 1)}
    for key in INTENT_KEYS[1:]:
        if key in spec:
            intent[key] = spec[key]
    if requirements:
        intent["requirements"] = requirements
    intent["context"] = shared
    intent["tasks"] = tasks
    if operations:
        intent["operations"] = operations
    return intent, reads


def summary(intent):
    tasks = intent["tasks"]
    executors = {}
    for task in tasks:
        executor = task["executor"]
        label = " ".join(str(executor[k]) for k in ("kind", "alias", "model", "effort") if executor.get(k))
        executors[label] = executors.get(label, 0) + 1
    lines = [f"tasks={len(tasks)} operations={len(intent.get('operations', []))} "
             f"requirements={len(intent.get('requirements', []))} "
             f"roots={sum(1 for t in tasks if not t.get('depends_on'))}"]
    lines += [f"executor {label}: {count}" for label, count in sorted(executors.items())]
    return lines


def cli_check(jtasks, root, output):
    lines = []
    for command in (["validate"], ["operations", "preview"]):
        result = subprocess.run([jtasks, "--root", root, *command, str(output)], capture_output=True, text=True,
                                encoding="utf-8", errors="replace")
        name = " ".join(command)
        if result.returncode != 0:
            raise ComposeError(f"jtasks {name} refused the intent: {(result.stderr or result.stdout).strip()[:2000]}")
        data = json.loads(result.stdout)
        if command == ["validate"]:
            states = {}
            for capability in data.get("capabilities", []):
                state = (capability.get("validation") or {}).get("state", "not_applicable")
                states[state] = states.get(state, 0) + 1
            lines.append(f"jtasks validate: valid={data.get('valid')} tasks={data.get('tasks')} "
                         f"operations={data.get('operations')} " + " ".join(f"{k}={v}" for k, v in sorted(states.items())))
        else:
            for operation in data.get("operations", []):
                stage, initial = operation["stage"], operation["initial"]
                scope = stage.get("consumers") or stage.get("tasks") or [stage.get("task")]
                after = ",".join(edge["id"] for edge in operation.get("depends_on", []))
                lines.append(f"operation {operation['id']}: {stage['kind']} for {len(scope)} task(s)"
                             + (f", after {after}" if after else "")
                             + f"; {initial['program']} {' '.join(initial['args'])} (cwd {initial['cwd']}, "
                             f"{initial.get('deadline_ms')} ms); repair {operation.get('repair_task')}; "
                             f"required {operation['required']}")
    return lines


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("spec", type=Path)
    parser.add_argument("--out", type=Path, help="composed intent; default: the spec's 'output', beside the spec")
    parser.add_argument("--check", action="store_true", help="compose and compare with the existing output; write nothing")
    parser.add_argument("--jtasks", help="independent jtasks executable: validate and preview the composed intent")
    parser.add_argument("--root", default=".", help="container or checkout passed to jtasks as --root")
    arguments = parser.parse_args()
    try:
        require(arguments.spec.is_file(), f"spec not found: {arguments.spec}")
        spec = json.loads(arguments.spec.read_text(encoding="utf-8"))
        target = arguments.out or (arguments.spec.parent / spec["output"] if "output" in spec else None)
        require(target is not None, "no output: give 'output' in the spec or --out")
        intent, reads = compose(spec, arguments.spec.resolve(), target)
        text = json.dumps(intent, indent=1, ensure_ascii=False) + "\n"
        if arguments.check:
            require(target.is_file() and target.read_text(encoding="utf-8") == text,
                    f"{target} is not what the spec composes; compose again")
            stale = reads.check()
            require(not stale, f"read files are not what the spec composes ({', '.join(stale)}); compose again")
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text, encoding="utf-8", newline="\n")
            reads.write()
        print(f"{'checked' if arguments.check else 'wrote'} {target.as_posix()} ({len(text.encode('utf-8'))} bytes)")
        if reads.files:
            size = sum(len(c.encode("utf-8")) for c in reads.files.values())
            print(f"read files: {len(reads.files)} in {reads.folder.as_posix()} ({size} bytes)")
        for line in summary(intent):
            print(line)
        if arguments.jtasks:
            for line in cli_check(arguments.jtasks, arguments.root, target):
                print(line)
    except (ComposeError, json.JSONDecodeError, OSError) as error:
        print(f"compose: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

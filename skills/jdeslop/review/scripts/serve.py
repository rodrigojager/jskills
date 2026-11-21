"""Serve a local review and apply its prewritten source corrections serially."""

import argparse
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path, PurePosixPath, PureWindowsPath
import secrets
import sqlite3
import stat
import tempfile
from threading import Condition, Thread
from urllib.parse import urlsplit

from build import compile_payload, render_page, require
from review_locale import tr


@contextmanager
def own_state(state):
    """Prevent two workers from claiming or recovering the same review."""
    lock_path = state.with_name(state.name + ".lock")
    with lock_path.open("a+b") as lock:
        if os.name == "nt":
            import msvcrt

            lock.seek(0)
            try:
                msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
            except OSError as error:
                raise ValueError(tr("This review queue is already open.")) from error
        else:
            import fcntl

            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except OSError as error:
                raise ValueError(tr("This review queue is already open.")) from error
        yield


def source_path(root, name):
    relative = PurePosixPath(name)
    require(not relative.is_absolute() and not PureWindowsPath(name).drive and
            "\\" not in name and ":" not in name and
            all(part.casefold() not in {"..", ".git"} for part in relative.parts),
            tr("Invalid source path: {0}.", name))
    target = (root / relative).resolve()
    require(target.is_relative_to(root) and
            ".git" not in {part.casefold() for part in target.relative_to(root).parts},
            tr("Source path leaves the product tree: {0}.", name))
    require(target.is_file(), tr("Expected an existing source file: {0}.", name))
    return target


def replacement(path, content, before, after):
    # Keep existing line endings and any UTF-8 BOM untouched.
    newline = "\r\n" if b"\r\n" in content and b"\n" not in content.replace(b"\r\n", b"") else "\n"

    def encode(snippet):
        return snippet.replace("\r\n", "\n").replace("\n", newline).encode("utf-8")

    match = encode(before)
    require(content.count(match) == 1, tr("{0}: expected one exact match. Re-stage against current source.", path.name))
    return content.replace(match, encode(after), 1)


def replace_file(path, content):
    # Stage in the same directory so replacement is atomic for each file.
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix=".jdeslop-", delete=False) as staged:
            temporary = Path(staged.name)
            staged.write(content)
            staged.flush()
            os.fsync(staged.fileno())
        temporary.chmod(stat.S_IMODE(path.stat().st_mode))
        temporary.replace(path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def apply_plan(root, plan, protected):
    originals = {}
    updated = {}
    for edit in plan:
        path = source_path(root, edit["path"])
        require(path not in protected, tr("Source plan targets a review input: {0}.", edit['path']))
        if path not in originals:
            originals[path] = updated[path] = path.read_bytes()
        updated[path] = replacement(path, updated[path], edit["before"], edit["after"])
    written = []
    try:
        for path in updated:
            require(path.read_bytes() == originals[path], tr("{0} changed before writing. Re-stage the correction.", path.name))
        for path, content in updated.items():
            if content == originals[path]:
                continue
            require(path.read_bytes() == originals[path], tr("{0} changed while applying the correction.", path.name))
            replace_file(path, content)
            written.append(path)
    except (OSError, ValueError) as error:
        conflicts = []
        for path in reversed(written):
            try:
                if path.read_bytes() != updated[path]:
                    raise ValueError(tr("Source changed during rollback."))
                replace_file(path, originals[path])
            except (OSError, ValueError):
                conflicts.append(path.relative_to(root).as_posix())
        if conflicts:
            raise ValueError(tr("{0} Rollback incomplete in {1}. Inspect before re-staging.", error, ', '.join(conflicts))) from error
        raise
    require(written, tr("The prepared source plan makes no change."))
    return len(written)


def current_jobs(db, fixes):
    return {
        fingerprint: {"id": fix_id, "status": status, "detail": detail, "note": note}
        for fix_id, fingerprint, status, detail, note in db.execute(
            "SELECT id, fingerprint, status, detail, note FROM jobs ORDER BY sequence"
        )
        if fix_id in fixes and fixes[fix_id]["fingerprint"] == fingerprint
    }


def pending_prerequisite(fix, fixes, jobs):
    for fix_id in fix["requires"]:
        prerequisite = jobs.get(fixes[fix_id]["fingerprint"])
        if not prerequisite or prerequisite["status"] != "applied":
            return fix_id
    return None


def unavailable_prerequisite(fix, fixes, plans, jobs):
    for dependency in fix["requires"]:
        job = jobs.get(fixes[dependency]["fingerprint"])
        if job and job["status"] == "applied":
            continue
        if not plans[dependency]:
            return f"Prerequisite {dependency} has no source edit prepared."
        if job and job["status"] in {"failed", "interrupted"}:
            return f"Prerequisite {dependency} needs a new source revision."
        if issue := unavailable_prerequisite(fixes[dependency], fixes, plans, jobs):
            return issue
    return None


def describe_jobs(fixes, plans, jobs):
    described = []
    for job in jobs.values():
        fix = fixes[job["id"]]
        if job["status"] == "queued" and (dependency := pending_prerequisite(fix, fixes, jobs)):
            unavailable = unavailable_prerequisite(fix, fixes, plans, jobs)
            job = {**job, "status": "blocked" if unavailable else "waiting",
                   "detail": unavailable or f"Waiting for {dependency} to be applied."}
        described.append(job)
    return described


def read_queue(state, root, payload, plans):
    if not state.exists():
        return []
    fixes = {fix["id"]: fix for fix in payload["fixes"]}
    uri_path = state
    if os.name == "nt":
        # Rust canonicalizes paths into the Windows verbatim namespace.
        # SQLite's file URI parser expects a drive or UNC path.
        name = str(state)
        if name.startswith("\\\\?\\UNC\\"):
            uri_path = Path("\\\\" + name[8:])
        else:
            uri_path = Path(name.removeprefix("\\\\?\\"))
    uri = uri_path.as_uri()
    if uri.startswith("file://") and not uri.startswith("file:///"):
        uri = uri.replace("file://", "file:////", 1)
    db = sqlite3.connect(uri + "?mode=ro", uri=True)
    try:
        metadata = db.execute("SELECT root, id FROM review").fetchall()
        require(metadata == [(str(root), payload["reviewId"])],
                tr("Queue belongs to a different source root or review."))
        return describe_jobs(fixes, plans, current_jobs(db, fixes))
    finally:
        db.close()


class Queue:
    def __init__(self, state, root, payload, plans, inputs):
        self.state = state
        self.root = root
        self.fixes = {fix["id"]: fix for fix in payload["fixes"]}
        self.plans = plans
        self.protected = inputs | {state, state.with_name(state.name + ".lock"), Path(__file__).resolve()}
        self.changed = Condition()
        self.closed = False
        for plan in plans.values():
            for edit in plan:
                require(source_path(root, edit["path"]) not in self.protected,
                        tr("Source plan targets a review input: {0}.", edit['path']))

        with self.connect() as db:
            db.execute("CREATE TABLE IF NOT EXISTS review (root TEXT, id TEXT)")
            db.execute("""CREATE TABLE IF NOT EXISTS jobs (
                sequence INTEGER PRIMARY KEY, id TEXT NOT NULL, fingerprint TEXT UNIQUE NOT NULL,
                note TEXT NOT NULL, status TEXT NOT NULL, detail TEXT NOT NULL DEFAULT ''
            )""")
            metadata = db.execute("SELECT root, id FROM review").fetchall()
            identity = (str(root), payload["reviewId"])
            require(not metadata or metadata == [identity], tr("Queue belongs to a different source root or review."))
            if not metadata:
                db.execute("INSERT INTO review VALUES (?, ?)", identity)
            db.execute("""UPDATE jobs SET status = 'interrupted',
                       detail = 'Inspect source before preparing a new revision. This job was not repeated.'
                       WHERE status = 'applying'""")
        self.worker = Thread(target=self.work, name="jdeslop-source-queue")

    @contextmanager
    def connect(self):
        db = sqlite3.connect(self.state)
        try:
            with db:
                yield db
        finally:
            db.close()

    def enqueue(self, entries):
        require(isinstance(entries, list) and 0 < len(entries) <= len(self.fixes),
                tr("Expected one or more correction revisions."))
        requested = {}
        for entry in entries:
            require(isinstance(entry, dict) and isinstance(entry.get("id"), str), tr("Invalid correction."))
            fix = self.fixes.get(entry["id"])
            require(fix and self.plans[fix["id"]] and entry.get("fingerprint") == fix["fingerprint"],
                    tr("Correction has no prepared source plan or its revision changed. Reload the review."))
            note = entry.get("note", "")
            require(isinstance(note, str) and len(note) <= 8000, tr("The approval note exceeds 8,000 characters."))
            requested[fix["id"]] = note
        with self.changed:
            require(not self.closed, tr("The review server is shutting down."))
            with self.connect() as db:
                jobs = current_jobs(db, self.fixes)
                for fix_id, fix in self.fixes.items():
                    if fix_id in requested:
                        if fix["fingerprint"] not in jobs:
                            unavailable = unavailable_prerequisite(fix, self.fixes, self.plans, jobs)
                            require(not unavailable, tr("{0} cannot queue. {1}", fix_id, unavailable))
                        db.execute("""INSERT INTO jobs (id, fingerprint, note, status)
                                      VALUES (?, ?, ?, 'queued')
                                      ON CONFLICT(fingerprint) DO NOTHING""",
                                   (fix_id, fix["fingerprint"], requested[fix_id]))
            self.changed.notify()
        return self.status()

    def status(self):
        with self.connect() as db:
            return describe_jobs(self.fixes, self.plans, current_jobs(db, self.fixes))

    def claim(self):
        with self.connect() as db:
            jobs = current_jobs(db, self.fixes)
            for fingerprint, job in jobs.items():
                if job["status"] == "queued" and not pending_prerequisite(self.fixes[job["id"]], self.fixes, jobs):
                    db.execute("UPDATE jobs SET status = 'applying' WHERE fingerprint = ?", (fingerprint,))
                    return fingerprint, job["id"]
        return None

    def work(self):
        while True:
            with self.changed:
                if self.closed:
                    return
                selected = self.claim()
                if selected is None:
                    self.changed.wait()
                    continue
            fingerprint, fix_id = selected
            try:
                count = apply_plan(self.root, self.plans[fix_id], self.protected)
                status, detail = "applied", f"Changed {count} source file(s)."
            except (OSError, ValueError) as error:
                status, detail = "failed", str(error)
            with self.connect() as db:
                db.execute("UPDATE jobs SET status = ?, detail = ? WHERE fingerprint = ?",
                           (status, detail, fingerprint))

    def stop(self):
        with self.changed:
            self.closed = True
            self.changed.notify()
        self.worker.join()


def handler_for(page, token, queue):
    class Handler(BaseHTTPRequestHandler):
        def local_host(self):
            return f"127.0.0.1:{self.server.server_port}"

        def reply(self, status, content_type, content):
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(content)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header("X-Frame-Options", "DENY")
            self.end_headers()
            self.wfile.write(content)

        def json_reply(self, status, result):
            self.reply(status, "application/json; charset=utf-8", json.dumps(result).encode("utf-8"))

        def authorized(self):
            received = self.headers.get("X-Review-Token", "")
            return (self.headers.get("Host") == self.local_host() and
                    received.isascii() and secrets.compare_digest(token, received))

        def do_GET(self):
            if self.headers.get("Host") != self.local_host():
                self.send_error(403)
            elif urlsplit(self.path).path == "/":
                self.reply(200, "text/html; charset=utf-8", page)
            elif self.path == "/api/queue" and self.authorized():
                self.json_reply(200, {"jobs": queue.status()})
            else:
                self.send_error(404)

        def do_POST(self):
            if (self.path != "/api/queue" or not self.authorized() or
                    self.headers.get("Origin") != f"http://{self.local_host()}"):
                self.send_error(403)
                return
            try:
                length = int(self.headers.get("Content-Length", "0"))
                require(self.headers.get_content_type() == "application/json" and
                        0 < length <= 1024 * 1024, tr("Invalid approval payload."))
                entries = json.loads(self.rfile.read(length))
                self.json_reply(200, {"jobs": queue.enqueue(entries)})
            except (UnicodeError, ValueError) as error:
                self.json_reply(400, {"error": str(error)})

        def log_message(self, format, *args):
            pass
    return Handler


def serve_review(root, state, port, payload, plans, inputs):
    require(root.is_dir(), tr("Source root must be a directory."))
    require(bool(any(plans.values())), tr("The manifest has no prepared source edits. Use build.py for preview only."))
    require(state not in inputs, tr("The queue must have a separate state file."))
    require(0 <= port <= 65535, tr("Port must be between 0 and 65535."))
    token = secrets.token_urlsafe(32)
    live = {**payload, "sourceQueue": {"token": token, "available": [key for key, plan in plans.items() if plan]}}
    page = render_page(live, live=True).encode("utf-8")
    with own_state(state):
        with ThreadingHTTPServer(("127.0.0.1", port), BaseHTTPRequestHandler) as server:
            queue = Queue(state, root, payload, plans, inputs)
            server.RequestHandlerClass = handler_for(page, token, queue)
            queue.worker.start()
            try:
                print(tr("Review ready at http://127.0.0.1:{0}/", server.server_port), flush=True)
                server.serve_forever()
            except KeyboardInterrupt:
                pass
            finally:
                queue.stop()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--state", type=Path, required=True, help="Persistent SQLite state for this review.")
    parser.add_argument("--port", type=int, default=8765)
    args = parser.parse_args()
    try:
        serve_review(args.source_root.resolve(strict=True), args.state.resolve(), args.port,
                     *compile_payload(args.manifest))
    except (OSError, sqlite3.Error, ValueError) as error:
        parser.exit(1, f"Review could not be served: {error}\n")


if __name__ == "__main__":
    main()

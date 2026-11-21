"""Browser regression for queued source approvals and multi-page comparisons.

Run with Python Playwright and its Chromium browser installed:
    cargo build --locked --manifest-path jdeslop/cli/Cargo.toml
    python -B jdeslop/review/tests/test_review.py
"""

from contextlib import closing, contextmanager
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import sqlite3
import subprocess
import sys
from tempfile import TemporaryDirectory
from threading import Thread
import unittest
from urllib.request import Request, urlopen
from urllib.error import HTTPError

from playwright.sync_api import expect, sync_playwright


REVIEW = Path(__file__).resolve().parents[1]
EXAMPLES = REVIEW / "examples"
CLI = REVIEW.parent / "cli" / "target" / "debug" / (
    "jdeslop.exe" if sys.platform == "win32" else "jdeslop"
)
CLI_COMMAND = [str(CLI), "--python", sys.executable]


class QuietFiles(SimpleHTTPRequestHandler):
    def log_message(self, format, *args):
        pass


@contextmanager
def product_server(directory):
    with ThreadingHTTPServer(("127.0.0.1", 0), partial(QuietFiles, directory=directory)) as server:
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield f"http://127.0.0.1:{server.server_port}"
        finally:
            server.shutdown()
            thread.join()


@contextmanager
def review_server(directory, product, port=0, combined=False):
    if combined:
        command = [*CLI_COMMAND, "review", str(directory), "--port", str(port)]
    else:
        command = [sys.executable, "-B", str(REVIEW / "scripts" / "serve.py"),
                   str(directory / "review.json"), "--source-root", str(product),
                   "--state", str(directory / "queue.sqlite3"), "--port", str(port)]
    with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) as process:
        try:
            for line in process.stdout:
                if line.startswith("Review ready at "):
                    yield line.strip().removeprefix("Review ready at ")
                    return
            raise AssertionError(process.stderr.read())
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=10)


def save_json(path, value):
    path.write_text(json.dumps(value), encoding="utf-8")


def edit(path, before, after):
    return {"path": path, "before": before, "after": after}


def fixture(directory, product, live):
    snapshot = (EXAMPLES / "invoice.html").read_text(encoding="utf-8")
    snapshot = snapshot.replace("</main>", '<div style="height:1400px"></div></main>')
    app = snapshot.replace("</main>", """<dialog id="payment"><p>Payment details</p></dialog>
      </main><script>window.fixtureRan = true;
      pay.addEventListener('click', () => payment.showModal());</script>""")
    app = app.replace('</style>', '</style><link rel="stylesheet" href="/shared.css">')
    receipt = app.replace("Invoice #1048", "Receipt #1048")
    (product / "invoice.html").write_text(app, encoding="utf-8")
    (product / "receipt.html").write_text(receipt, encoding="utf-8")
    stylesheet = "#amount { margin-bottom: 23px; }\r\n#due { margin-bottom: 44px; }\r\n"
    (product / "shared.css").write_bytes(stylesheet.encode("utf-8"))
    (directory / "invoice.html").write_text(app, encoding="utf-8")
    (directory / "receipt.html").write_text(receipt, encoding="utf-8")
    audit = json.loads((EXAMPLES / "audit.json").read_text(encoding="utf-8"))
    audit["surfaces"].append({**audit["surfaces"][0], "name": "Receipt"})
    save_json(directory / "audit.json", audit)
    manifest = json.loads((EXAMPLES / "review.json").read_text(encoding="utf-8"))
    manifest["surfaces"] = [
        {"id": key, "title": title, "baseline": f"{key}.html", "surface": ledger,
         "liveUrl": f"{live}/{key}.html"}
        for key, title, ledger in [("invoice", "Invoice", "Invoice due"), ("receipt", "Receipt", "Receipt")]
    ]
    fixes = manifest["fixes"]
    for fix in fixes:
        changes = fix.pop("changes")
        fix["changesBySurface"] = {"invoice": changes}
        if fix["id"] == "F-01":
            fix["changesBySurface"]["receipt"] = changes
    fixes[0]["implementation"] = [
        edit("shared.css", stylesheet, stylesheet.replace("23px", "4px").replace("44px", "24px"))
    ]
    fixes[1]["implementation"] = [edit("invoice.html", '<p id="badge">Invoice overview</p>', "")]
    help_link = '<a id="help" href="#items">Questions about this invoice?</a>'
    payment = '<button id="pay" type="button">Continue</button>'
    fixes[2]["implementation"] = [
        edit("invoice.html", help_link + "\n        " + payment, payment + "\n        " + help_link)
    ]
    fixes[3]["requires"] = ["F-03"]
    fixes[3]["implementation"] = [edit("invoice.html", payment, payment.replace("Continue", "Pay invoice"))]
    fixes.extend([
        {**fixes[3], "id": "F-05", "title": "Conflict fixture", "requires": [],
         "implementation": [edit("receipt.html", "Receipt #1048", "New title"),
                            edit("invoice.html", "No such fragment", "Changed")]},
        {**fixes[3], "id": "F-06", "title": "Revision fixture", "requires": [],
         "implementation": [edit("receipt.html", "Receipt #1048", "Another title")]},
        {**fixes[3], "id": "F-07", "title": "Skipped prerequisite fixture", "requires": ["F-02"]},
        {**fixes[3], "id": "F-08", "title": "Indirect prerequisite fixture", "requires": ["F-07"]},
        {**fixes[3], "id": "F-09", "title": "Preview-only fixture", "requires": [], "implementation": []},
        {**fixes[3], "id": "F-10", "title": "Missing source prerequisite fixture", "requires": ["F-09"]},
        {**fixes[3], "id": "F-11", "title": "Failed prerequisite fixture", "requires": ["F-05"]},
    ])
    save_json(directory / "review.json", manifest)


def verify_browser(page, base, directory, product):
    page.goto(base)
    queue = page.locator("#queue")
    canvas = page.frame_locator("#canvas")
    queue.get_by_role("button", name="Group the amount").click()
    expect(canvas.locator("#amount")).to_have_css("margin-bottom", "4px")
    expect(canvas.locator("#amount")).to_have_attribute("data-review-target", "")
    assert page.evaluate("document.scrollingElement.scrollHeight === innerHeight")
    assert page.locator("#notice").evaluate("(notice) => notice.offsetHeight === 0")
    assert canvas.locator("body").evaluate("() => window.fixtureRan === undefined")
    canvas.locator("body").evaluate("() => window.scrollTo(0, 180)")
    page.get_by_role("button", name="Before fix").click()
    expect(canvas.locator("#amount")).to_have_css("margin-bottom", "23px")
    page.wait_for_function("document.querySelector('#canvas').contentWindow.scrollY === 180")
    expect(canvas.locator("#amount")).to_have_attribute("data-review-target", "")
    page.get_by_role("button", name="After fix").click()
    expect(canvas.locator("#amount")).to_have_css("margin-bottom", "4px")
    page.wait_for_function("document.querySelector('#canvas').contentWindow.scrollY === 180")
    page.locator("#surface").select_option("receipt")
    expect(canvas.locator("#invoice")).to_have_text("Receipt #1048")
    expect(canvas.locator("#amount")).to_have_css("margin-bottom", "4px")
    page.locator("#surface").select_option("invoice")
    expect(canvas.locator("#invoice")).to_have_text("Invoice #1048")
    page.wait_for_function("document.querySelector('#canvas').contentWindow.scrollY === 180")
    queue.get_by_role("button", name="Name the payment action").click()
    expect(canvas.locator("#intro")).to_have_count(0)
    page.get_by_role("checkbox", name="Include earlier fixes").uncheck()
    expect(canvas.locator("#intro")).to_be_visible()
    expect(canvas.locator("#actions > :first-child")).to_have_text("Pay invoice")
    page.get_by_role("button", name="Before fix").click()
    expect(canvas.locator("#actions > :first-child")).to_have_text("Continue")
    page.get_by_role("button", name="Original").click()
    expect(canvas.locator("#actions > :first-child")).to_have_attribute("id", "help")
    page.get_by_role("button", name="Play sequence").click()
    expect(page.locator("#position")).to_have_text("1 / 11")
    page.get_by_role("button", name="Pause sequence").click()
    expect(page.locator("#position")).to_have_text("1 / 11")
    page.get_by_role("button", name="Resume sequence").click()
    expect(page.locator("#position")).to_have_text("2 / 11")
    page.get_by_role("button", name="Pause sequence").click()
    queue.get_by_role("button", name="Remove the repeated").click()
    page.get_by_role("button", name="Skip", exact=True).click()
    queue.get_by_role("button", name="Revision fixture").click()
    revise = page.get_by_role("button", name="Request changes")
    approve = page.get_by_role("button", name="Approve and queue", exact=True)
    assert abs(revise.bounding_box()["y"] - page.locator("#after").bounding_box()["y"]) < 1
    assert approve.bounding_box()["x"] > page.locator("#after").bounding_box()["x"]
    revise.press("Enter")
    popover = page.get_by_role("dialog", name="Request changes")
    note = popover.get_by_role("textbox", name="What should change?")
    send = popover.get_by_role("button", name="Send request")
    expect(note).to_be_focused()
    expect(send).to_be_disabled()
    note.fill("  ")
    expect(send).to_be_disabled()
    note.fill("Try a more specific label.")
    send.click()
    expect(popover).to_be_hidden()
    expect(revise).to_be_focused()
    expect(queue.get_by_role("button", name="Revision fixture")).to_contain_text("Changes requested")
    for title, message in [("Indirect prerequisite", "Prerequisite F-02 was skipped"),
                           ("Missing source prerequisite", "F-09 has no source edit prepared")]:
        queue.get_by_role("button", name=title).click()
        expect(page.locator("#source-status")).to_contain_text(message)
        expect(page.get_by_role("button", name="Approve and queue", exact=True)).to_be_disabled()
    queue.get_by_role("button", name="Group the amount").click()
    page.get_by_role("button", name="Approve and queue", exact=True).click()
    page.get_by_role("button", name="Approve and queue all").click()
    expect(page.locator("#queue-summary")).to_have_text("3 applied · 1 failed · 1 blocked", timeout=15000)
    expect(queue.get_by_role("button", name="Remove the repeated")).to_contain_text("Skipped")
    expect(queue.get_by_role("button", name="Revision fixture")).to_contain_text("Changes requested")
    with closing(sqlite3.connect(directory / "queue.sqlite3")) as db:
        assert db.execute("SELECT id, status FROM jobs ORDER BY sequence").fetchall() == [
            ("F-01", "applied"), ("F-03", "applied"), ("F-04", "applied"), ("F-05", "failed"), ("F-11", "queued")]
    assert b"4px; }\r\n" in (product / "shared.css").read_bytes()
    assert "Receipt #1048" in (product / "receipt.html").read_text(encoding="utf-8")
    assert "New title" not in (product / "receipt.html").read_text(encoding="utf-8")
    assert "Invoice overview" in (product / "invoice.html").read_text(encoding="utf-8")

    with page.expect_popup() as popup:
        page.get_by_role("link", name="Open live page").click()
    live = popup.value
    expect(live.locator("#amount")).to_have_css("margin-bottom", "4px")
    expect(live.locator("#actions > :first-child")).to_have_text("Pay invoice")
    live.get_by_role("button", name="Pay invoice").press("Enter")
    expect(live.get_by_role("dialog")).to_be_visible()
    live.keyboard.press("Escape")
    expect(live.get_by_role("dialog")).to_be_hidden()
    live.close()


def cli(command, directory, *options):
    return subprocess.run([*CLI_COMMAND, command, str(directory), *options],
                          capture_output=True, text=True, timeout=10)


def stage_combined_fixture(directory, product, live):
    config = json.loads((directory / "run.json").read_text(encoding="utf-8"))
    fixture(directory, product, live)
    ledger = json.loads((directory / "coverage.json").read_text(encoding="utf-8"))
    legacy = json.loads((directory / "audit.json").read_text(encoding="utf-8"))
    for surface, existing in zip(ledger["surfaces"], legacy["surfaces"], strict=True):
        findings = {check["id"]: check for check in existing["checks"] if check["status"] == "finding"}
        for index, check in enumerate(surface["checks"]):
            # Synthetic fixture: only the demo findings are under test.
            if finding := findings.get(check["id"]):
                check.update(finding)
                for detail in check.get("details", []):
                    detail.update(status="n/a", evidence="Parent-level fixture finding.")
            elif index == 0:
                check.update(status="blocked", evidence="Fixture has no additional viewports.")
                check.pop("details", None)
            else:
                check.update(status="n/a", evidence="Outside the synthetic fixture.")
                check.pop("details", None)
    save_json(directory / "coverage.json", ledger)
    manifest = json.loads((directory / "review.json").read_text(encoding="utf-8"))
    manifest.update(reviewId=config["reviewId"], ledger="coverage.json")
    save_json(directory / "review.json", manifest)
    return manifest


class ReviewTest(unittest.TestCase):
    def test_portable_preview_keeps_decisions_without_source_access(self):
        with TemporaryDirectory(prefix="jdeslop-portable-") as directory, sync_playwright() as playwright:
            output = Path(directory) / "review.html"
            subprocess.run([sys.executable, "-B", str(REVIEW / "scripts" / "build.py"),
                            str(EXAMPLES / "review.json"), str(output)], check=True, capture_output=True)
            browser = playwright.chromium.launch()
            page = browser.new_page(viewport={"width": 390, "height": 844})
            page.goto(output.as_uri())
            page.get_by_role("button", name="Next step").click()
            expect(page.get_by_role("button", name="After fix")).to_have_attribute("aria-pressed", "true")
            expect(page.frame_locator("#canvas").locator("#amount")).to_have_css("margin-bottom", "4px")
            assert page.evaluate("document.scrollingElement.scrollHeight === innerHeight")
            width = page.get_by_role("combobox", name="Width")
            width.select_option("1200")
            assert page.locator(".stage").evaluate("(stage) => stage.scrollWidth > stage.clientWidth")
            assert page.evaluate("document.scrollingElement.scrollWidth === innerWidth")
            width.select_option("fit")
            expect(page.get_by_role("button", name="Approve and queue all")).to_be_hidden()
            revise = page.get_by_role("button", name="Request changes")
            popover = page.get_by_role("dialog", name="Request changes")
            note = popover.get_by_role("textbox", name="What should change?")
            revise.click()
            expect(revise).to_have_attribute("aria-expanded", "true")
            bounds = popover.bounding_box()
            assert bounds["x"] >= 0 and bounds["x"] + bounds["width"] <= 390
            assert bounds["y"] >= 0 and bounds["y"] + bounds["height"] <= 844
            note.fill("Discard this draft.")
            popover.get_by_role("button", name="Cancel").click()
            expect(popover).to_be_hidden()
            expect(revise).to_be_focused()
            revise.click()
            expect(note).to_have_value("")
            page.keyboard.press("Escape")
            expect(popover).to_be_hidden()
            expect(revise).to_be_focused()
            revise.click()
            page.locator("#title").click()
            expect(popover).to_be_hidden()
            page.get_by_role("button", name="Keep").click()
            page.reload()
            expect(page.locator("#queue").get_by_role("button", name="Group the amount")).to_contain_text("Kept")
            page.get_by_role("button", name="Coverage").click()
            expect(page.get_by_role("dialog")).to_contain_text("LAY-08")
            browser.close()

    def test_multi_surface_review_and_persistent_queue(self):
        with TemporaryDirectory(prefix="jdeslop-test-") as name:
            directory = Path(name)
            product = directory / "product"
            product.mkdir()
            with product_server(product) as live, sync_playwright() as playwright:
                fixture(directory, product, live)
                browser = playwright.chromium.launch()
                page = browser.new_page(viewport={"width": 1280, "height": 900})
                errors = []
                page.on("pageerror", lambda error: errors.append(str(error)))
                with review_server(directory, product) as base:
                    verify_browser(page, base, directory, product)
                    self.assertUnauthorized(base, page)
                    page.reload()
                    expect(page.locator("#queue-summary")).to_have_text("3 applied · 1 failed · 1 blocked")
                    expect(page.locator("#queue-all")).to_be_disabled()
                with review_server(directory, product, int(base.split(":")[-1].strip("/"))) as reopened:
                    page.goto(reopened)
                    expect(page.locator("#queue-summary")).to_have_text("3 applied · 1 failed · 1 blocked")
                    expect(page.locator("#queue-all")).to_be_disabled()
                self.assertEqual(errors, [])
                browser.close()

    def test_combined_cli_binds_audit_review_and_persistent_queue(self):
        with TemporaryDirectory(prefix="jdeslop-run-") as name:
            directory = Path(name) / "review run"
            product = Path(name) / "product"
            product.mkdir()
            created = cli("init", directory, "--source-root", str(product),
                          "--surface", "Invoice due", "--surface", "Receipt")
            self.assertEqual(created.returncode, 0, created.stderr)
            identity = (directory / "run.json").read_bytes()
            self.assertEqual(cli("init", directory, "--surface", "Other").returncode, 1)
            self.assertEqual((directory / "run.json").read_bytes(), identity)
            self.assertEqual(cli("audit", directory).returncode, 1)
            self.assertEqual(cli("review", directory, "--portable").returncode, 1)
            self.assertFalse((directory / "review.html").exists())

            with product_server(product) as live, sync_playwright() as playwright:
                manifest = stage_combined_fixture(directory, product, live)
                self.assertEqual(cli("audit", directory).returncode, 2)
                for patch, message in [({"ledger": "audit.json"}, "this run's coverage.json"),
                                       ({"reviewId": "another-run"}, "match run.json"),
                                       ({"surfaces": manifest["surfaces"][:1]}, "every audited surface")]:
                    save_json(directory / "review.json", {**manifest, **patch})
                    failure = cli("review", directory, "--portable")
                    self.assertEqual(failure.returncode, 1)
                    self.assertIn(message, failure.stderr)
                save_json(directory / "review.json", manifest)
                original = (product / "invoice.html").read_bytes()
                portable = cli("review", directory, "--portable")
                self.assertEqual(portable.returncode, 0, portable.stderr)
                self.assertTrue((directory / "review.html").is_file())
                self.assertFalse((directory / "queue.sqlite3").exists())
                status = cli("status", directory)
                self.assertEqual(status.returncode, 0, status.stderr)
                self.assertIn("F-01 r1 | awaiting approval", status.stdout)
                self.assertFalse((directory / "queue.sqlite3").exists())
                self.assertEqual((product / "invoice.html").read_bytes(), original)

                browser = playwright.chromium.launch()
                page = browser.new_page(viewport={"width": 1280, "height": 900})
                with review_server(directory, product, combined=True) as base:
                    self.assertEqual((product / "invoice.html").read_bytes(), original)
                    verify_browser(page, base, directory, product)
                    status = cli("status", directory)
                    self.assertEqual(status.returncode, 0, status.stderr)
                    self.assertIn("F-01 r1 | applied", status.stdout)
                    self.assertIn("F-05 r1 | failed", status.stdout)
                    self.assertIn("F-11 r1 | blocked", status.stdout)
                with review_server(directory, product, combined=True) as reopened:
                    page.goto(reopened)
                    expect(page.locator("#queue-summary")).to_have_text("3 applied · 1 failed · 1 blocked")
                browser.close()

                manifest["fixes"][0].update(revision=2, title="Refine the amount spacing")
                save_json(directory / "review.json", manifest)
                status = cli("status", directory)
                self.assertEqual(status.returncode, 0, status.stderr)
                self.assertIn("F-01 r2 | awaiting approval", status.stdout)

    def assertUnauthorized(self, base, page):
        token = page.locator("#review-data").evaluate("(node) => JSON.parse(node.textContent).sourceQueue.token")
        request = Request(base + "api/queue", method="POST", data=b"[]",
                          headers={"Content-Type": "application/json", "Origin": "https://example.invalid",
                                   "X-Review-Token": token})
        with self.assertRaises(HTTPError) as error:
            urlopen(request, timeout=5)
        with closing(error.exception) as response:
            self.assertEqual(response.code, 403)


if __name__ == "__main__":
    unittest.main()

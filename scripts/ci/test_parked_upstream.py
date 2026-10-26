"""parked_upstream.py: unit selection and the tracking issue (no network, no build)."""
import io
import json
from contextlib import redirect_stdout
from pathlib import Path
import tempfile
import unittest

import parked_upstream as p


def write(root, rel, text):
    path = Path(root) / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


PARKED = """[[mismatch]]
unit = "a.b"
case = "parked"
why = "C++ reads freed memory"
cpp_defect = "UB-1"
"""

PLAIN = """[[mismatch]]
unit = "c.d"
case = "plain"
why = "Rust diverges"
"""


class ParkedUpstreamTest(unittest.TestCase):
    def run_main(self, *argv):
        out = io.StringIO()
        with redirect_stdout(out):
            code = p.main(list(argv))
        return code, out.getvalue()

    def test_units_lists_only_units_with_a_cpp_defect(self):
        with tempfile.TemporaryDirectory() as root:
            write(root, p.KNOWN_DIR + "/a.b.toml", PARKED)
            write(root, p.KNOWN_DIR + "/c.d.toml", PLAIN)
            self.assertEqual(p.parked_cases(Path(root)), {"a.b": {"parked": "UB-1"}})
            self.assertEqual(self.run_main("units", "--root", root), (0, "a.b\n"))

    def test_main_runs_without_a_module_docstring(self):
        # `python3 -OO` strips docstrings; the argument parser must not need one.
        doc = p.__doc__
        try:
            for missing in (None, ""):
                p.__doc__ = missing
                with tempfile.TemporaryDirectory() as root:
                    write(root, p.KNOWN_DIR + "/a.b.toml", PARKED)
                    self.assertEqual(self.run_main("units", "--root", root), (0, "a.b\n"))
        finally:
            p.__doc__ = doc

    def test_issue_title_and_body(self):
        with tempfile.TemporaryDirectory() as root, tempfile.TemporaryDirectory() as out:
            write(root, p.KNOWN_DIR + "/a.b.toml", PARKED)
            reports = Path(out) / "reports"
            report = {"unit": "a.b", "cases": [
                {"name": "parked", "outcome": "cpp_defect", "pass": False}]}
            write(reports, "a.b.json", json.dumps(report))
            title, body = Path(out) / "title", Path(out) / "body"
            args = ("issue", "--root", root, "--report-dir", str(reports),
                    "--upstream-ref", "v1.2", "--upstream-sha", "abc",
                    "--title-out", str(title), "--body-out", str(body))
            # Still disagrees: the issue says nothing changed.
            self.assertEqual(self.run_main(*args)[0], 0)
            self.assertEqual(title.read_text(), "usd-brep v1.2 released: none changed\n")
            self.assertIn("nothing to adopt", body.read_text())
            # Agrees: the issue lists it.
            report["cases"][0].update(outcome="pass", **{"pass": True})
            write(reports, "a.b.json", json.dumps(report))
            self.assertEqual(self.run_main(*args)[0], 0)
            t = title.read_text()
            self.assertEqual(t, "usd-brep v1.2 released: 1 parked case now agrees, adopt?\n")
            self.assertTrue(t.startswith(p.TITLE_PREFIX) and p.TITLE_MARK in t)
            self.assertIn("| `a.b` | `parked` | `UB-1` |", body.read_text())
            self.assertIn("`abc`", body.read_text())
            # A missing report is an error, not silence.
            (reports / "a.b.json").unlink()
            self.assertEqual(self.run_main(*args)[0], 2)


if __name__ == "__main__":
    unittest.main()

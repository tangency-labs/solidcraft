"""Regression tests for the case counts published in plan/PROGRESS.md.

Run with: python3 -m unittest discover -s harness/plan
"""
import json
import pathlib
import tempfile
import unittest
from unittest import mock

import progress


class CaseCountsTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = pathlib.Path(self.tmp.name)
        (self.root / "plan").mkdir()
        (self.root / "reports/difftest").mkdir(parents=True)
        self.patch_root = mock.patch.object(progress, "ROOT", self.root)
        self.patch_root.start()
        self.addCleanup(self.patch_root.stop)

    def unit(self, uid, notes, status="partial"):
        return {"id": uid, "rust_module": uid.replace(".", "::"),
                "est_lines": 100, "layer": 0, "depends_on": [],
                "status": status, "notes": notes,
                "claimed_by": "", "claimed_at": ""}

    def report(self, uid, count):
        path = self.root / f"reports/difftest/{uid}.json"
        path.write_text(json.dumps({"unit": uid, "cases_total": count}))
        return path

    def render(self, units):
        with mock.patch.object(progress, "load_units", return_value=units), \
                mock.patch.object(progress.sys, "argv", ["progress.py"]):
            progress.main()
        return (self.root / "plan/PROGRESS.md").read_text()

    def test_default_reports_override_stress_runs_and_operation_numbers(self):
        # These prefixes reproduce the misleading counts committed at d045b293:
        # the prose regex reported the tail of 100,076 and the 0 from op-0.
        units = [
            self.unit("surface.offset_surface",
                      "GREEN (PR #62): 3077 difftest cases (77 fixtures + 3000 generated), "
                      "3077 pass bit-identical, 0 failed, 0 deferred, max_rel_err 0. "
                      "Extended 100,000-seed run: 100,076 cases, 100,076 pass, 0 failed.",
                      "green"),
            self.unit("topology.brep_p6",
                      "PARTIAL (PR #169): 498 difftest cases, 450 passed, 0 failed, "
                      "48 deferred. Of the explicit op-0 cases, 369 return before "
                      "acquiring a mark.")]
        self.report(units[0]["id"], 3077)
        self.report(units[1]["id"], 498)
        output = self.render(units)
        self.assertIn("`surface.offset_surface` (100 lines) — green, 3077 cases", output)
        self.assertIn("`topology.brep_p6` (100 lines) — partial, 498 cases", output)
        self.assertIn("2 / 2 units ported** (1 green, 1 partial)", output)

    def test_report_update_changes_count_without_editing_notes(self):
        unit = self.unit("curve.example", "An older run had 41 cases.")
        self.report(unit["id"], 42)
        self.assertIn("— partial, 42 cases", self.render([unit]))
        self.report(unit["id"], 43)
        self.assertIn("— partial, 43 cases", self.render([unit]))

    def test_missing_report_does_not_invent_a_count_from_notes(self):
        unit = self.unit("curve.example", "An earlier experiment had 41 cases.")
        output = self.render([unit])
        self.assertIn("`curve.example` (100 lines) — partial\n", output)
        self.assertNotIn("41 cases", output)

    def test_zero_total_is_distinct_from_a_missing_report(self):
        unit = self.unit("curve.example", "An earlier experiment had 41 cases.")
        self.report(unit["id"], 0)
        self.assertIn("`curve.example` (100 lines) — partial, 0 cases", self.render([unit]))

    def test_invalid_report_does_not_overwrite_existing_checklist(self):
        unit = self.unit("curve.example", "An earlier experiment had 41 cases.")
        path = self.report(unit["id"], 42)
        checklist = self.root / "plan/PROGRESS.md"
        bad_reports = [
            "{invalid JSON",
            "[]",
            json.dumps({"unit": "another.unit", "cases_total": 42}),
            json.dumps({"unit": unit["id"]}),
            *[json.dumps({"unit": unit["id"], "cases_total": count})
              for count in (-1, True, "42", 42.0)],
        ]
        for bad in bad_reports:
            with self.subTest(report=bad):
                path.write_text(bad)
                checklist.write_text("existing checklist\n")
                with self.assertRaisesRegex(ValueError, "curve.example.json"):
                    self.render([unit])
                self.assertEqual(checklist.read_text(), "existing checklist\n")


if __name__ == "__main__":
    unittest.main()

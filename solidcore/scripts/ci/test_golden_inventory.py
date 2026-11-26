"""Check published golden inventory without executing the kernel or downloading reports."""

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]


def module(name, path):
    """Import the real presentation code, without running its command entry point."""
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


class GoldenInventoryTests(unittest.TestCase):
    """Exercise the shared checklist/site input boundary with isolated records."""

    def setUp(self):
        """Prepare golden records alongside a deliberately stale legacy report."""
        self.temp = tempfile.TemporaryDirectory(prefix="golden inventory ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.directory = self.root / "crates/goldens/expected"
        self.directory.mkdir(parents=True)
        self.uid = "geom.fixture"
        self.rows = [{"case": "a_error", "error": "refused", "deferred": None},
                     {"case": "b_deferred", "error": None,
                      "deferred": {"unit": "curve.next", "symbol": "missing_fn"}},
                     {"case": "c_rejected", "error": None, "deferred": None, "rejected": "bad input"}]
        self.known = {self.uid: {"wrong": "solidcore#999: wrong result"}}
        self.write()
        report = self.root / "reports/difftest" / (self.uid + ".json")
        report.parent.mkdir(parents=True)
        report.write_text(json.dumps({"unit": self.uid, "cases_total": 999, "cases_passed": 999}))
        with patch.object(sys, "path", [str(ROOT / "harness/plan"), *sys.path]):
            self.progress = module("progress_inventory_test", ROOT / "harness/plan/progress.py")
            self.site = module("site_inventory_test", ROOT / "site/build.py")
        self.units = [{"id": self.uid, "status": "green"}]

    def write(self, header=None):
        """Write only fixture goldens and their known-failure list."""
        header = header if header is not None else {"unit": self.uid, "format": 1}
        (self.directory / (self.uid + ".jsonl")).write_text(
            "\n".join(json.dumps(row) for row in [header, *self.rows]) + "\n")
        (self.directory / "known_failing.json").write_text(json.dumps(self.known))

    def counts(self):
        """Use the real checklist reader, not an independent test implementation."""
        with patch.object(self.progress, "ROOT", self.root):
            return self.progress.load_case_counts(self.units)

    def inventory(self):
        """Find the shared production loader, with an assertion on pre-fix code."""
        loader = getattr(self.progress, "load_inventory", None)
        self.assertIsNotNone(loader, "the checklist must share the golden inventory loader")
        return loader(self.root)[self.uid]

    def test_counts_golden_records_not_the_header_or_legacy_passes(self):
        """Case counts come from committed records even when old reports exist."""
        self.assertEqual(self.counts(), {self.uid: 3})

    def test_error_deferral_rejection_and_known_failure_stay_distinct(self):
        """A stored refusal or known wrong case cannot be invented as a pass."""
        info = self.inventory()
        self.assertEqual({k: info[k] for k in ("records", "errors", "deferrals", "rejections")},
                         {"records": 3, "errors": 1, "deferrals": 1, "rejections": 1})
        self.assertEqual(info["known_failures"], self.known[self.uid])
        self.assertEqual(info["deferred_by_unit"], {"curve.next": 1})
        self.assertNotIn("cases_passed", info)

    def test_duplicate_case_refuses(self):
        """Duplicate identities cannot inflate published coverage."""
        self.rows.append(dict(self.rows[-1]))
        self.write()
        with self.assertRaises(ValueError):
            self.counts()

    def test_distinct_out_of_order_cases_refuse(self):
        """An unsorted inventory cannot publish, even without duplicate identities."""
        self.rows.reverse()
        self.write()
        with self.assertRaisesRegex(ValueError, r"geom\.fixture\.jsonl:3: case names must be sorted"):
            self.counts()

    def test_known_failure_unit_needs_a_golden_file(self):
        """A known-failure mapping cannot silently disappear with its unit's file."""
        (self.directory / (self.uid + ".jsonl")).unlink()
        with self.assertRaisesRegex(ValueError, r"known_failing\.json: known-failure unit has no golden file"):
            self.counts()

    def test_known_failures_need_a_mapping_at_the_top_level(self):
        """An array of reasons is not the documented unit-to-case mapping."""
        self.known = []
        self.write()
        with self.assertRaisesRegex(ValueError, r"known_failing\.json: expected a unit-to-case mapping"):
            self.counts()

    def test_missing_inventory_directory_refuses(self):
        """An omitted checkout directory must not become an empty publication."""
        with tempfile.TemporaryDirectory(prefix="missing golden inventory ") as empty:
            missing = Path(empty) / "crates/goldens/expected"
            with self.assertRaises(FileNotFoundError) as error:
                self.progress.load_inventory(Path(empty))
            self.assertIn(str(missing), str(error.exception))

    def test_known_failure_cannot_also_have_a_golden_record(self):
        """The inventory enforces the gate's no-blessed-known-failure boundary."""
        self.known[self.uid]["a_error"] = "solidcore#999: still wrong"
        self.write()
        with self.assertRaises(ValueError):
            self.counts()

    def test_wrong_header_unit_or_version_refuses(self):
        """A filename mismatch or unsupported format fails closed."""
        for header in ({"unit": "other.unit", "format": 1}, {"unit": self.uid, "format": 2},
                       {"unit": self.uid, "format": True}):
            with self.subTest(header=header):
                self.write(header)
                with self.assertRaises(ValueError):
                    self.counts()

    def test_malformed_json_refuses_instead_of_falling_back(self):
        """Broken golden input cannot be hidden by an available historical report."""
        (self.directory / (self.uid + ".jsonl")).write_text("not JSON\n")
        with self.assertRaises(ValueError):
            self.counts()

    def test_bad_deferral_or_error_types_refuse(self):
        """Only supported metadata types contribute to displayed outcome counts."""
        for bad in ({"case": "a", "deferred": {"unit": 42, "symbol": "fn"}},
                    {"case": "a", "error": False}, {"case": "a", "rejected": []}):
            with self.subTest(bad=bad):
                self.rows = [bad]
                self.write()
                with self.assertRaises(ValueError):
                    self.counts()

    def test_bad_known_failure_reason_refuses(self):
        """Malformed known-failure entries cannot disappear from publication."""
        self.known[self.uid]["wrong"] = 42
        self.write()
        with self.assertRaises(ValueError):
            self.counts()

    def test_empty_golden_has_zero_records_not_one_passing_header(self):
        """An all-known-failing unit's header is not a test outcome."""
        self.rows = []
        self.write()
        self.assertEqual(self.counts(), {self.uid: 0})

    def test_site_labels_inventory_and_links_exact_source_not_ci_results(self):
        """The shared counts are source evidence, never a manufactured runtime pass."""
        render = getattr(self.site, "golden_body", None)
        self.assertIsNotNone(render, "unit pages need an explicitly labelled inventory renderer")
        body = render(self.uid, self.inventory(), "a" * 40)
        self.assertIn("Golden records", body)
        self.assertIn("Known-failure entries", body)
        self.assertIn("not a runtime result", body)
        self.assertIn("/blob/" + "a" * 40 + "/crates/goldens/expected/", body)
        self.assertNotIn("<dt>Passed", body)
        self.assertNotIn("Max rel. error", body)

    def test_site_escapes_known_failure_text(self):
        """Issue/case metadata cannot inject HTML into a unit page."""
        render = getattr(self.site, "golden_body", None)
        self.assertIsNotNone(render)
        self.known[self.uid] = {'<script>case</script>': '<script>alert("x")</script>'}
        self.write()
        body = render(self.uid, self.inventory(), "b" * 40)
        self.assertNotIn("<script>", body)
        self.assertIn("&lt;script&gt;", body)


if __name__ == "__main__":
    unittest.main()

"""check_record_ids.py: duplicate record IDs and orphan addenda (file names only)."""
from pathlib import Path
import tempfile
import unittest

import check_record_ids as c


class CheckRecordIds(unittest.TestCase):
    def test_distinct_ids_pass(self):
        names = ["README.md", "WR-1--a.md", "WR-2--b.md", "UB-1--c.md"]
        self.assertEqual(c.check(names, legacy={}), [])

    def test_shared_id_fails(self):
        errors = c.check(["WR-1--a.md", "WR-1--b.md"], legacy={})
        self.assertEqual(len(errors), 1)
        self.assertIn("WR-1 is used by 2 records", errors[0])

    def test_same_number_other_series_passes(self):
        self.assertEqual(c.check(["WR-1--a.md", "UB-1--a.md"], legacy={}), [])

    def test_prefix_number_is_not_a_match(self):
        self.assertEqual(c.check(["WR-1--a.md", "WR-12--b.md"], legacy={}), [])

    def test_addendum_does_not_count_as_record(self):
        names = ["WR-1--a.md", "WR-1--addendum-2026-10-26-more.md"]
        self.assertEqual(c.check(names, legacy={}), [])

    def test_addendum_without_record_fails(self):
        errors = c.check(["WR-2--addendum-2026-10-26-more.md"], legacy={})
        self.assertEqual(len(errors), 1)
        self.assertIn("no record has that ID", errors[0])

    def test_malformed_addendum_fails(self):
        errors = c.check(["WR-1--a.md", "WR-1--addendum-more.md"], legacy={})
        self.assertEqual(len(errors), 1)
        self.assertIn("addendum is named", errors[0])

    def test_unnumbered_names_are_skipped(self):
        names = ["UB-new--a.md", "UB-new--b.md", "UNNUMBERED--c.md", "HANG-unnumbered--d.md"]
        self.assertEqual(c.check(names, legacy={}), [])

    def test_legacy_duplicate_passes_but_cannot_grow(self):
        legacy = {"UB-7": {"UB-7--a.md", "UB-7--b--dup2.md"}}
        self.assertEqual(c.check(["UB-7--a.md", "UB-7--b--dup2.md"], legacy=legacy), [])
        errors = c.check(["UB-7--a.md", "UB-7--b--dup2.md", "UB-7--c.md"], legacy=legacy)
        self.assertEqual(len(errors), 1)
        self.assertIn("UB-7--c.md an ID that is not in use", errors[0])

    def test_dotted_ids_are_distinct_records(self):
        names = ["WR-1250--a.md", "WR-1250.2--b.md", "WR-1250.3--c.md", "WR-12--d.md"]
        self.assertEqual(c.check(names, legacy={}), [])

    def test_shared_dotted_id_fails(self):
        errors = c.check(["WR-1250.2--a.md", "WR-1250.2--b.md"], legacy={})
        self.assertEqual(len(errors), 1)
        self.assertIn("WR-1250.2 is used by 2 records", errors[0])

    def test_dot_one_is_not_an_id(self):
        # The first record of a PR has no suffix; `.1` is not a valid form, so it is skipped.
        self.assertEqual(c.check(["WR-1250--a.md", "WR-1250.1--b.md"], legacy={}), [])

    def test_addendum_names_dotted_record(self):
        names = ["WR-1250.2--a.md", "WR-1250.2--addendum-2026-10-27-more.md"]
        self.assertEqual(c.check(names, legacy={}), [])
        errors = c.check(["WR-1250--a.md", "WR-1250.2--addendum-2026-10-27-more.md"], legacy={})
        self.assertEqual(len(errors), 1)
        self.assertIn("WR-1250.2, but no record", errors[0])

    def test_main_reads_directory(self):
        with tempfile.TemporaryDirectory() as tmp:
            d = Path(tmp) / c.ISSUES
            d.mkdir(parents=True)
            (d / "WR-1--a.md").write_text("x")
            self.assertEqual(c.main(["x", tmp]), 0)
            (d / "WR-1--b.md").write_text("x")
            self.assertEqual(c.main(["x", tmp]), 1)


if __name__ == "__main__":
    unittest.main()

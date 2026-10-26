"""English stays in its existing tree, including shared source documents."""

import tempfile
import unittest
from pathlib import Path

from source import english_page


class EnglishSourceTests(unittest.TestCase):
    def test_includes_expand_without_changing_originals_and_track_shared_files(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            original = root / "docs/book/src/dev/architecture.md"
            original.parent.mkdir(parents=True)
            wrapper = "# Architecture\n\n{{#include ../../../architecture.md:layers}}\n"
            original.write_text(wrapper, encoding="utf-8")
            shared = root / "docs/architecture.md"
            shared.write_text("Outside\n<!-- ANCHOR: layers -->\nLayer table\n<!-- ANCHOR_END: layers -->\nOutside\n", encoding="utf-8")
            text, dependencies = english_page(original, root)
            self.assertEqual(text, "# Architecture\n\nLayer table\n")
            self.assertEqual(dependencies, {"docs/book/src/dev/architecture.md", "docs/architecture.md"})
            self.assertEqual(original.read_text(encoding="utf-8"), wrapper)

    def test_recursive_includes_are_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            page = root / "page.md"
            page.write_text("{{#include page.md}}", encoding="utf-8")
            with self.assertRaises(ValueError):
                english_page(page, root)

    def test_line_range_include_is_supported(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "lines.md").write_text("one\ntwo\nthree\nfour\n", encoding="utf-8")
            page = root / "page.md"
            page.write_text("{{#include lines.md:2:3}}", encoding="utf-8")
            self.assertEqual(english_page(page, root)[0], "two\nthree\n")

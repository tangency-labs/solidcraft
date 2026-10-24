"""Regression checks for missing chapters and chapter-preserving language links."""

import importlib.util
import hashlib
import json
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("docs_build", Path(__file__).with_name("build.py"))
docs_build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(docs_build)


class LanguageBuildTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.write("docs/book/src/SUMMARY.md", "# Summary\n\n- [Start](guide/start.md)\n- [Other](guide/other.md)\n")
        self.write("docs/book/src/guide/start.md", "# Start\n\nEnglish {count}\n")
        self.write("docs/book/src/guide/other.md", "# Other\n\nCurrent English\n")
        self.write("docs/book/book.toml", '[book]\nlanguage = "en"\nsrc = "src"\n[output.html]\nsite-url = "/"\n')
        self.write("docs/book/theme/head.hbs", "<!-- shared -->\n")
        self.languages = [{"id": "en", "name": "English"}, {"id": "da", "name": "Dansk"}, {"id": "fr", "name": "Français"}]

    def write(self, name, content):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")

    def test_translated_page_and_missing_page_are_chosen_separately(self):
        self.write("docs/da/guide/start.md", "# Start på dansk\n\nDansk {count}\n")
        stage, report = docs_build.prepare(self.root, self.languages[1], self.languages)
        translated = (stage / "guide/start.md").read_text(encoding="utf-8")
        fallback = (stage / "guide/other.md").read_text(encoding="utf-8")
        self.assertIn("Dansk {count}", translated)
        self.assertNotIn('lang="en"', translated)
        self.assertIn("Current English", fallback)
        self.assertIn('lang="en"', fallback)
        self.assertEqual(sum(page["fallback"] for page in report), 1)
        self.assertIn("guide/other.md", (stage / "SUMMARY.md").read_text(encoding="utf-8"))

    def test_english_edits_reach_untranslated_pages_without_copies(self):
        stage, _ = docs_build.prepare(self.root, self.languages[2], self.languages)
        self.write("docs/book/src/guide/other.md", "# Other\n\nNew English\n")
        stage, _ = docs_build.prepare(self.root, self.languages[2], self.languages)
        self.assertIn("New English", (stage / "guide/other.md").read_text(encoding="utf-8"))
        self.assertFalse((self.root / "docs/fr/guide/other.md").exists())
        self.assertIn('language = "en"', (stage / "book.toml").read_text(encoding="utf-8"))

    def test_nested_chapter_links_keep_the_same_chapter(self):
        links = docs_build.language_links("guide/nested/start.md", "da", self.languages)
        self.assertIn("[English](../../../guide/nested/start.html)", links)
        self.assertIn("[Français](../../../fr/guide/nested/start.html)", links)
        self.assertIn("**Dansk**", links)
        english_links = docs_build.language_links("introduction.md", "en", self.languages)
        self.assertIn("[Dansk](da/introduction.html)", english_links)

    def test_shared_includes_keep_named_anchors(self):
        original = self.root / "docs/book/src/dev/architecture.md"
        staged = self.root / "target/docs-i18n/da/dev/architecture.md"
        result = docs_build.include_paths("{{#include ../../../architecture.md:architecture}}", original, staged)
        target = result.removeprefix("{{#include ").removesuffix("}}")
        filename, anchor = target.split(":")
        self.assertEqual((staged.parent / filename).resolve(), (self.root / "docs/architecture.md").resolve())
        self.assertEqual(anchor, "architecture")

    def test_new_english_chapter_is_not_hidden_by_an_older_translated_summary(self):
        english = "# User guide\n\n- [Start](guide/start.md)\n- [New](guide/new.md)\n"
        translated = "# Brugervejledning\n\n- [Kom i gang](guide/start.md)\n"
        result = docs_build.summary_text(english, translated, {"User guide": "Brugervejledning"})
        self.assertIn("[Kom i gang](guide/start.md)", result)
        self.assertIn("[New](guide/new.md)", result)
        self.assertIn("# Brugervejledning", result)

    def test_empty_translation_uses_english(self):
        self.write("docs/da/guide/start.md", "\n")
        stage, _ = docs_build.prepare(self.root, self.languages[1], self.languages)
        text = (stage / "guide/start.md").read_text(encoding="utf-8")
        self.assertIn("English {count}", text)
        self.assertIn('lang="en"', text)

    def test_editable_english_copy_is_marked_as_english_until_changed(self):
        self.write("docs/da/guide/start.md", "# Start\n\nEnglish {count}\n")
        stage, report = docs_build.prepare(self.root, self.languages[1], self.languages)
        self.assertIn('lang="en"', (stage / "guide/start.md").read_text(encoding="utf-8"))
        self.assertTrue(next(page for page in report if page["page"] == "guide/start.md")["fallback"])

    def test_tracked_old_english_copy_displays_current_english_before_sync(self):
        copied = "Old English\n"
        self.write("docs/da/guide/start.md", copied)
        self.write("docs/da/.translation.json", json.dumps({"pages": {"guide/start.md": {"copy": hashlib.sha256(copied.encode()).hexdigest()}}}))
        stage, report = docs_build.prepare(self.root, self.languages[1], self.languages)
        self.assertIn("English {count}", (stage / "guide/start.md").read_text(encoding="utf-8"))
        self.assertTrue(next(page for page in report if page["page"] == "guide/start.md")["fallback"])


if __name__ == "__main__":
    unittest.main()

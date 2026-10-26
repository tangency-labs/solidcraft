"""Check English synchronization without overwriting translations."""

import importlib.util
import json
import re
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

spec = importlib.util.spec_from_file_location("docs_sync", Path(__file__).with_name("sync.py"))
sync = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sync)


class SyncTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.write("docs/languages.json", json.dumps([{"id": "en", "name": "English"}, {"id": "da", "name": "Dansk"}]))
        self.write("docs/book/src/start.md", "English\n")
        self.git("init")
        self.git("config", "user.email", "test@example.com")
        self.git("config", "user.name", "Test")
        self.commit()

    def git(self, *args):
        return sync.git(self.root, *args)

    def write(self, path, text):
        path = self.root / path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def commit(self):
        self.git("add", ".")
        self.git("commit", "-m", "Test")

    def test_new_pages_copies_and_idempotency(self):
        sync.synchronize(self.root)
        self.write("docs/book/src/new.md", "New English\n")
        self.commit()
        result, text = sync.synchronize(self.root)
        self.assertEqual((self.root / "docs/da/new.md").read_text(), "New English\n")
        self.assertEqual(result, sync.synchronize(self.root)[0])
        self.assertEqual(result["da"]["commits_behind"], 1)
        self.assertIn("untranslated", text)

    def test_two_english_commits_count_without_code_commit_and_review(self):
        sync.synchronize(self.root)
        self.write("docs/da/start.md", "Dansk\n")
        sync.synchronize(self.root, review="da")
        self.commit()
        self.write("code.txt", "unrelated")
        self.commit()
        self.write("docs/book/src/start.md", "Changed English\n")
        self.commit()
        self.write("docs/book/src/start.md", "Changed twice\n")
        self.commit()
        result, _ = sync.synchronize(self.root)
        self.assertEqual(result["da"]["commits_behind"], 2)
        translation = (self.root / "docs/da/start.md").read_text(encoding="utf-8")
        self.assertTrue(translation.startswith("Dansk\n"))
        self.assertIn("Changed twice", translation)
        self.assertEqual(result["da"]["pending"][0]["reason"], "changed")
        self.write("docs/da/start.md", "Dansk opdateret\n")
        self.assertEqual(sync.synchronize(self.root, review="da")[0]["da"]["pending"], [])

    def test_new_and_changed_sections_are_inserted_once_without_old_unchanged_text(self):
        self.write("docs/book/src/start.md", "# Start\n\nUnchanged text.\n\nOld paragraph.\n")
        self.commit()
        sync.synchronize(self.root)
        self.write("docs/da/start.md", "# Start på dansk\n\nBevar denne oversættelse.\n")
        sync.synchronize(self.root, review="da")
        self.write("docs/book/src/start.md", "# Start\n\nUnchanged text.\n\nUpdated paragraph.\n\n## New section\n\nNew text.\n\n```json\n{\n\n  \"id\": \"solid.extrude\"\n}\n```\n")
        self.commit()
        sync.synchronize(self.root)
        target = self.root / "docs/da/start.md"
        text = target.read_text(encoding="utf-8")
        self.assertIn("Bevar denne oversættelse.", text)
        self.assertIn("Updated paragraph.", text)
        self.assertIn("## New section", text)
        self.assertIn('```json\n{\n\n  "id": "solid.extrude"\n}\n```', text)
        self.assertNotIn("Unchanged text.", text)
        self.assertNotIn("Old paragraph.", text)
        sync.synchronize(self.root)
        self.assertEqual(text, target.read_text(encoding="utf-8"))
        self.write("docs/book/src/start.md", "# Start\n\nUnchanged text.\n\nNewest paragraph.\n")
        self.commit()
        sync.synchronize(self.root)
        text = target.read_text(encoding="utf-8")
        self.assertIn("Newest paragraph.", text)
        self.assertNotIn("Updated paragraph.", text)
        self.assertEqual(text.count(sync.BEGIN), 1)

    def test_translator_edits_in_pending_block_are_preserved_and_review_requires_removal(self):
        sync.synchronize(self.root)
        self.write("docs/da/start.md", "Dansk\n")
        sync.synchronize(self.root, review="da")
        self.write("docs/book/src/start.md", "New English\n")
        self.commit()
        sync.synchronize(self.root)
        target = self.root / "docs/da/start.md"
        text = target.read_text(encoding="utf-8").replace("New English", "Ny dansk tekst")
        target.write_text(text, encoding="utf-8")
        self.write("docs/book/src/start.md", "Even newer English\n")
        self.commit()
        sync.synchronize(self.root)
        self.assertEqual(target.read_text(encoding="utf-8"), text)
        with self.assertRaises(ValueError):
            sync.synchronize(self.root, review="da")
        self.write("docs/da/start.md", "Dansk med de seneste ændringer\n")
        self.assertEqual(sync.synchronize(self.root, review="da")[0]["da"]["pending"], [])

    def test_new_summary_links_keep_translated_labels_and_headings(self):
        self.write("docs/book/src/SUMMARY.md", "# Guide\n\n- [Start](start.md)\n")
        self.commit()
        sync.synchronize(self.root)
        self.write("docs/da/SUMMARY.md", "# Vejledning\n\n- [Begynd her](start.md)\n")
        sync.synchronize(self.root, review="da")
        self.write("docs/book/src/new.md", "New English page\n")
        self.write("docs/book/src/SUMMARY.md", "# Guide\n\n- [Start](start.md)\n- [New page](new.md)\n")
        self.commit()
        sync.synchronize(self.root)
        summary = (self.root / "docs/da/SUMMARY.md").read_text(encoding="utf-8")
        self.assertIn("# Vejledning", summary)
        self.assertIn("[Begynd her](start.md)", summary)
        self.assertIn("[New page](new.md)", summary)
        self.assertNotIn(sync.BEGIN, summary)
        self.assertEqual((self.root / "docs/da/new.md").read_text(encoding="utf-8"), "New English page\n")

    def test_source_revert_removes_only_untouched_generated_update(self):
        sync.synchronize(self.root)
        self.write("docs/da/start.md", "Dansk\n")
        sync.synchronize(self.root, review="da")
        self.write("docs/book/src/start.md", "New English\n")
        self.commit()
        sync.synchronize(self.root)
        self.write("docs/book/src/start.md", "English\n")
        self.commit()
        result, _ = sync.synchronize(self.root)
        self.assertEqual((self.root / "docs/da/start.md").read_text(encoding="utf-8"), "Dansk\n")
        self.assertEqual(result["da"]["pending"], [])

    def test_changes_to_existing_shared_english_document_reach_translations(self):
        wrapper = "{{#include ../../../shared.md}}\n"
        self.write("docs/book/src/start.md", wrapper)
        self.write("shared.md", "Original shared English\n")
        self.commit()
        sync.synchronize(self.root)
        self.assertEqual((self.root / "docs/da/start.md").read_text(encoding="utf-8"), "Original shared English\n")
        self.write("docs/da/start.md", "Dansk oversættelse\n")
        sync.synchronize(self.root, review="da")
        self.commit()
        self.write("unrelated.txt", "Application change")
        self.commit()
        self.write("shared.md", "Updated shared English\n\nNew shared section\n")
        self.commit()
        result, _ = sync.synchronize(self.root)
        translated = (self.root / "docs/da/start.md").read_text(encoding="utf-8")
        self.assertTrue(translated.startswith("Dansk oversættelse"))
        self.assertIn("Updated shared English", translated)
        self.assertIn("New shared section", translated)
        self.assertEqual(result["da"]["commits_behind"], 1)
        self.assertEqual((self.root / "docs/book/src/start.md").read_text(encoding="utf-8"), wrapper)

    def test_untouched_copy_updates_but_stays_untranslated(self):
        sync.synchronize(self.root)
        self.write("docs/book/src/start.md", "Latest English\n")
        self.commit()
        result, _ = sync.synchronize(self.root)
        self.assertEqual((self.root / "docs/da/start.md").read_text(), "Latest English\n")
        self.assertEqual(result["da"]["pending"][0]["reason"], "untranslated")

    def test_removed_page_keeps_translation(self):
        sync.synchronize(self.root)
        (self.root / "docs/book/src/start.md").unlink()
        self.commit()
        result, _ = sync.synchronize(self.root)
        self.assertTrue((self.root / "docs/da/start.md").exists())
        self.assertIn("removed", result["da"]["pending"][0]["reason"])
        self.assertEqual(sync.synchronize(self.root, review="da")[0]["da"]["pending"], [])
        self.assertTrue((self.root / "docs/da/start.md").exists())

    def test_issue_updates_existing_bot_issue_instead_of_creating_duplicate(self):
        existing = [{"number": 12, "body": "<!-- docs-translation:da -->\nold", "state": "CLOSED"}]
        with patch.dict("os.environ", {"GITHUB_REPOSITORY": "example/docs"}), patch.object(subprocess, "check_output", side_effect=[json.dumps(existing), "", ""]) as command:
            sync.notify({"da": {"name": "Dansk", "commits_behind": 2, "pending": [{"page": "start.md", "reason": "changed"}]}})
        calls = [c.args[0] for c in command.call_args_list]
        self.assertIn("reopen", calls[1])
        self.assertIn("edit", calls[2])
        self.assertFalse(any("create" in c for c in calls))

    def test_finished_language_closes_reminder(self):
        existing = [{"number": 12, "body": "<!-- docs-translation:da -->", "state": "OPEN"}]
        with patch.dict("os.environ", {"GITHUB_REPOSITORY": "example/docs"}), patch.object(subprocess, "check_output", side_effect=[json.dumps(existing), ""]) as command:
            sync.notify({"da": {"name": "Dansk", "commits_behind": 0, "pending": []}})
        self.assertIn("close", command.call_args.args[0])


if __name__ == "__main__":
    unittest.main()

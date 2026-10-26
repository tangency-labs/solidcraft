"""The localized command catalogue must preserve executable reference content."""

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("docs_build", Path(__file__).with_name("build.py"))
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)


class ReferenceTests(unittest.TestCase):
    def test_localized_names_leave_ids_shortcuts_parameters_json_and_links_unchanged(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "docs/book/src/reference").mkdir(parents=True)
            (root / "assets/locales").mkdir(parents=True)
            tail = ' (E)<br>``distance: expr; body?: string``\n\n```json\n{"command":"solid.extrude","params":{"distance":20}}\n```\n\n```markdown\n- `solid.extrude` **Extrude**\n```\n\n[Details](../guide/features.md#extrude)\n'
            source = '# Command reference\n\nGenerated text.\n\n## SOLID › CREATE\n\n- `solid.extrude` **Extrude**' + tail
            (root / "docs/book/src/reference/commands.md").write_text(source, encoding="utf-8")
            for code, label in [("en", "Extrude"), ("da", "Ekstrudering")]:
                (root / "assets/locales" / (code + ".json")).write_text(json.dumps({"ui.extrude": label}), encoding="utf-8")
            result = build.command_reference("# Dansk\n\n{{#command-reference}}", root, "da")
            self.assertIn('- `solid.extrude` **Extrude — Ekstrudering**' + tail.rstrip(), result)
            self.assertIn("## SOLID › CREATE", result)
            self.assertNotIn("Generated text.", result)
            self.assertNotIn("{{#command-reference}}", result)

    def test_new_untranslated_label_is_retained(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "docs/book/src/reference").mkdir(parents=True)
            (root / "assets/locales").mkdir(parents=True)
            (root / "docs/book/src/reference/commands.md").write_text('# Commands\n\n- `new.command` **New Tool**\n', encoding="utf-8")
            for code in ["en", "da"]:
                (root / "assets/locales" / (code + ".json")).write_text('{}', encoding="utf-8")
            self.assertEqual(build.command_reference("{{#command-reference}}", root, "da"), '- `new.command` **New Tool**')

    def test_regular_pages_do_not_read_a_generated_catalogue(self):
        self.assertEqual(build.command_reference("A regular page", Path("missing"), "da"), "A regular page")


if __name__ == "__main__":
    unittest.main()

"""Exercise platform font selection without Pillow, fonts, or image rendering."""

import importlib.util
from pathlib import Path
import sys
import types
import unittest
from unittest.mock import Mock, patch


ROOT = Path(__file__).resolve().parents[2]
LINUX_REGULAR = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
LINUX_BOLD = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
MAC_REGULAR = "/System/Library/Fonts/Supplemental/Arial.ttf"
MAC_BOLD = "/System/Library/Fonts/Supplemental/Arial Bold.ttf"
WINDOWS_REGULAR = "C:/Windows/Fonts/arial.ttf"
WINDOWS_BOLD = "C:/Windows/Fonts/arialbd.ttf"


class FontSelectionTests(unittest.TestCase):
    def setUp(self):
        self.loader = Mock()
        self.loader.truetype.side_effect = lambda path, size: (path, size)
        self.loader.load_default.return_value = ("default",)
        pillow = types.ModuleType("PIL")
        pillow.Image = Mock()
        pillow.ImageDraw = Mock()
        pillow.ImageFont = self.loader
        spec = importlib.util.spec_from_file_location(
            "solidcore_og_image_font_test", ROOT / "site/gen-og-image.py"
        )
        self.generator = importlib.util.module_from_spec(spec)
        with patch.dict(sys.modules, {"PIL": pillow}):
            spec.loader.exec_module(self.generator)

    def select(self, available, size, bold=False):
        """Select a font while treating only the supplied paths as installed."""
        available_paths = {Path(candidate) for candidate in available}
        with patch.object(Path, "exists", lambda path: path in available_paths):
            return self.generator.font(size, bold=bold)

    def test_linux_candidates_keep_priority_for_both_styles(self):
        available = {
            LINUX_REGULAR, LINUX_BOLD, MAC_REGULAR, MAC_BOLD,
            WINDOWS_REGULAR, WINDOWS_BOLD,
        }
        self.assertEqual(self.select(available, 33), (LINUX_REGULAR, 33))
        self.assertEqual(self.select(available, 78, bold=True), (LINUX_BOLD, 78))
        self.loader.load_default.assert_not_called()

    def test_macos_regular_preserves_requested_size(self):
        self.assertEqual(self.select({MAC_REGULAR}, 33), (MAC_REGULAR, 33))

    def test_macos_bold_preserves_requested_size(self):
        self.assertEqual(self.select({MAC_BOLD}, 78, bold=True), (MAC_BOLD, 78))

    def test_windows_regular_preserves_requested_size(self):
        self.assertEqual(self.select({WINDOWS_REGULAR}, 27), (WINDOWS_REGULAR, 27))

    def test_windows_bold_preserves_requested_size(self):
        self.assertEqual(
            self.select({WINDOWS_BOLD}, 78, bold=True), (WINDOWS_BOLD, 78)
        )

    def test_missing_style_tries_the_next_family(self):
        self.assertEqual(
            self.select({LINUX_REGULAR, MAC_BOLD}, 78, bold=True), (MAC_BOLD, 78)
        )

    def test_regular_font_never_substitutes_for_missing_bold(self):
        self.assertEqual(self.select({MAC_REGULAR}, 78, bold=True), ("default",))
        self.loader.truetype.assert_not_called()

    def test_bold_font_never_substitutes_for_missing_regular(self):
        self.assertEqual(self.select({MAC_BOLD}, 33), ("default",))
        self.loader.truetype.assert_not_called()

    def test_no_available_font_keeps_the_default_fallback(self):
        self.assertEqual(self.select(set(), 33), ("default",))
        self.loader.truetype.assert_not_called()
        self.loader.load_default.assert_called_once_with()


if __name__ == "__main__":
    unittest.main()

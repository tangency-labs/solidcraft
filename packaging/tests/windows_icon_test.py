"""PE icon guard: Windows PE32/PE32+ resources, and missing/broken groups."""
import pathlib
import shutil
import struct
import subprocess
import tempfile
import unittest
import xml.etree.ElementTree as ET

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / "windows/check-icon.ps1"


def executable(pe64=True, icon=True, broken=False):
    data = bytearray(1024)
    def u16(at, value): struct.pack_into("<H", data, at, value)
    def u32(at, value): struct.pack_into("<I", data, at, value)
    u16(0, 0x5A4D)
    u32(0x3C, 0x80)
    u32(0x80, 0x4550)
    u16(0x86, 1)
    optional = 0x98
    size = 240 if pe64 else 224
    u16(0x94, size)
    u16(optional, 0x20B if pe64 else 0x10B)
    directories = optional + (112 if pe64 else 96)
    u32(directories + 16, 0x1000)
    u32(directories + 20, 512)
    section = optional + size
    u32(section + 12, 0x1000)
    u32(section + 16, 512)
    u32(section + 20, 512)
    base = 512
    def directory(relative, entries):
        u16(base + relative + 14, len(entries))
        for i, (name, offset) in enumerate(entries):
            u32(base + relative + 16 + 8*i, name)
            u32(base + relative + 20 + 8*i, offset)
    if icon:
        directory(0, [(3, 0x80000030), (14, 0x80000070)])
        directory(0x30, [(1, 0x80000050)])
        directory(0x50, [(1033, 0xB0)])
        directory(0x70, [(1, 0x80000090)])
        directory(0x90, [(1033, 0xC0)])
        u32(base + 0xB0, 0x1100)
        u32(base + 0xB4, 40)
        u32(base + 0xC0, 0x1140)
        u32(base + 0xC4, 20)
        u16(base + 0x142, 1)
        u16(base + 0x144, 1)
        u32(base + 0x14E, 40)
        u16(base + 0x152, 2 if broken else 1)
    return data


class IconGuard(unittest.TestCase):
    def run_guard(self, data):
        with tempfile.TemporaryDirectory() as folder:
            path = pathlib.Path(folder) / "app.exe"
            path.write_bytes(data)
            return subprocess.run([shutil.which("pwsh") or "powershell", "-NoProfile", "-File", str(SCRIPT), "-Path", str(path)], capture_output=True, text=True)

    def test_valid_pe32_and_pe64(self):
        for pe64 in (False, True):
            result = self.run_guard(executable(pe64))
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("1 images, 1 group", result.stdout)

    def test_generic_executable_is_rejected(self):
        result = self.run_guard(executable(icon=False))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("RT_ICON", result.stderr)

    def test_group_must_reference_a_real_image(self):
        result = self.run_guard(executable(broken=True))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing image", result.stderr)

    def test_truncated_executable_is_rejected(self):
        self.assertNotEqual(self.run_guard(b"MZ").returncode, 0)



class InstallerShortcutIcons(unittest.TestCase):
    def check_shortcuts(self, change=None):
        root = pathlib.Path(__file__).resolve().parents[2]
        document = ET.parse(root / "assets/platforms/windows/solidcraft.wxs")
        if change:
            shortcut = document.find(".//{http://wixtoolset.org/schemas/v4/wxs}Shortcut")
            change(shortcut)
        with tempfile.TemporaryDirectory() as folder:
            path = pathlib.Path(folder) / "installer.wxs"
            document.write(path, encoding="utf-8", xml_declaration=True)
            return subprocess.run([shutil.which("pwsh") or "powershell", "-NoProfile", "-File",
                str(root / "packaging/windows/check-shortcuts.ps1"), "-Path", str(path)], capture_output=True, text=True)

    def test_installed_shortcuts_use_the_target_executable_icon(self):
        result = self.check_shortcuts()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_a_product_code_scoped_icon_is_rejected(self):
        result = self.check_shortcuts(lambda shortcut: shortcut.set("Icon", "SolidcraftIcon.exe"))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("MSI-cached Icon", result.stderr)

    def test_an_advertised_shortcut_is_rejected(self):
        result = self.check_shortcuts(lambda shortcut: shortcut.set("Advertise", "yes"))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("changes identity", result.stderr)


if __name__ == "__main__":
    unittest.main()

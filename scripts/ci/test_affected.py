#!/usr/bin/env python3
"""Self-test for affected.py. Run: python3 -I scripts/ci/test_affected.py (from the repo root).

Uses a small synthetic workspace, then the real one (`cargo metadata --no-deps`).
"""
import copy
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import affected  # noqa: E402

R = "/ws"


def pkg(name, dir_, deps=()):
    return {
        "name": name,
        "manifest_path": f"{R}/{dir_}/Cargo.toml",
        "dependencies": [{"name": d, "path": f"{R}/{p}"} for d, p in deps] + [{"name": "serde"}],
    }


SYNTH = {
    "workspace_root": R,
    "packages": [
        pkg("base", "crates/base"),
        pkg("mid", "crates/mid", [("base", "crates/base")]),
        pkg("top", "apps/top", [("mid", "crates/mid")]),
        pkg("alone", "crates/alone"),
    ],
}


def windows_metadata(root):
    meta = copy.deepcopy(SYNTH)
    meta["workspace_root"] = root
    prefix = root.rstrip("\\/")
    for p in meta["packages"]:
        p["manifest_path"] = prefix + "\\" + p["manifest_path"][len(R) + 1:]
        for d in p["dependencies"]:
            if "path" in d:
                d["path"] = prefix.replace("\\", "/") + "/" + d["path"][len(R) + 1:].replace("/", "\\")
    return meta


class Synthetic(unittest.TestCase):
    def test_leaf_selects_only_itself(self):
        self.assertEqual(affected.select(SYNTH, ["apps/top/src/main.rs"]), (False, ["top"]))

    def test_reverse_dependencies_are_added(self):
        self.assertEqual(affected.select(SYNTH, ["crates/base/src/lib.rs"]), (False, ["base", "mid", "top"]))

    def test_unrelated_package_is_not_selected(self):
        full, pk = affected.select(SYNTH, ["crates/mid/Cargo.toml"])
        self.assertEqual((full, pk), (False, ["mid", "top"]))

    def test_full_triggers(self):
        for p in ["Cargo.lock", "Cargo.toml", ".cargo/config.toml", ".config/nextest.toml", "rust-toolchain.toml",
                  ".github/workflows/ci.yml", "vendor/x/lib.rs", "tests/oracle/a.json", "assets/a.png",
                  "scripts/ci/affected.py", "unknown.txt"]:
            self.assertEqual(affected.select(SYNTH, ["crates/alone/a.rs", p]), (True, []), p)

    def test_docs_only_selects_nothing(self):
        self.assertEqual(affected.select(SYNTH, ["docs/book/a.md", "README.md", "packaging/x.yml"]), (False, []))

    def test_no_code_paths_select_nothing(self):
        # What CI reads as "no code changed", on pull requests and pushes to main alike.
        for p in [".github/workflows/docker.yml", ".github/ISSUE_TEMPLATE/bug.yml", "scripts/claim/claim.js",
                  "scripts/triage/triage.test.js", "vendor/README.md", "SECURITY.md"]:
            self.assertEqual(affected.select(SYNTH, [p]), (False, []), p)

    def test_windows_metadata_selects_packages_and_preserves_triggers(self):
        for root in [r"C:\work space\solidcraft", r"\\server\shared work\solidcraft"]:
            for trailing in ["", "\\", "/"]:
                with self.subTest(root=root, trailing=trailing):
                    meta = windows_metadata(root + trailing)
                    self.assertEqual(affected.select(meta, ["apps/top/src/main.rs"]), (False, ["top"]))
                    self.assertEqual(affected.select(meta, ["crates/base/src/lib.rs"]), (False, ["base", "mid", "top"]))
                    self.assertEqual(affected.select(meta, ["docs/book/a.md", "README.md"]), (False, []))
                    for path in ["Cargo.lock", ".github/workflows/ci.yml", "unknown.txt"]:
                        self.assertEqual(affected.select(meta, [path]), (True, []), path)

    def test_windows_metadata_rejects_a_manifest_in_a_sibling_workspace(self):
        for root in [r"C:\work space\solidcraft", r"\\server\shared work\solidcraft"]:
            with self.subTest(root=root):
                meta = windows_metadata(root + "\\")
                sibling = root.replace("\\", "/") + "-sibling/crates/base/Cargo.toml"
                meta["packages"][0]["manifest_path"] = sibling
                with self.assertRaises(AssertionError):
                    affected.select(meta, ["crates/base/src/lib.rs"])


class RealWorkspace(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.meta = affected.load_metadata(None)

    def test_render_change_selects_render_and_dependents_not_kernel(self):
        full, pk = affected.select(self.meta, ["crates/render/src/lib.rs"])
        self.assertFalse(full)
        self.assertIn("solidcraft-render", pk)
        self.assertGreater(len(pk), 1)
        self.assertNotIn("solidcraft-kernel", pk)

    def test_cargo_lock_selects_full(self):
        self.assertEqual(affected.select(self.meta, ["Cargo.lock"]), (True, []))

    def test_docs_only_selects_nothing(self):
        self.assertEqual(affected.select(self.meta, ["docs/book/src/dev/testing.md", "CONTRIBUTING.md"]), (False, []))

    def test_engine_test_data_selects_the_engine_and_its_dependents_not_the_kernel(self):
        full, pk = affected.select(self.meta, ["tests/hole_matrix/known_failures.txt"])
        self.assertFalse(full)
        self.assertIn("solidcraft-engine", pk)
        self.assertNotIn("solidcraft-kernel", pk)
        self.assertNotIn("solidcraft-render", pk)

    def test_oracle_cases_select_the_cli_and_xtask_not_the_kernel(self):
        full, pk = affected.select(self.meta, ["tests/oracle/box/recipe.json"])
        self.assertFalse(full)
        self.assertIn("solidcraft-cli", pk)
        self.assertIn("xtask", pk)
        self.assertNotIn("solidcraft-kernel", pk)

    def test_embedded_root_files_select_their_crate(self):
        for path, pkg in [("ATTRIBUTION.md", "solidcraft-ui-egui"), ("LICENSE-MIT", "solidcraft-engine"),
                          ("NOTICE", "solidcraft-engine"), ("THIRD-PARTY-LICENSES.txt", "solidcraft-engine"),
                          ("skills/solidcraft-modeling/SKILL.md", "solidcraft-mcp")]:
            full, pk = affected.select(self.meta, [path])
            self.assertFalse(full, path)
            self.assertIn(pkg, pk, path)
            self.assertNotIn("solidcraft-kernel", pk, path)

    def test_unmapped_test_data_selects_full(self):
        self.assertEqual(affected.select(self.meta, ["tests/tools/x.json"]), (True, []))

    def test_vendored_egui_wgpu_selects_the_ui_and_apps_not_the_kernel(self):
        full, pk = affected.select(self.meta, ["vendor/egui-wgpu/src/winit.rs"])
        self.assertFalse(full)
        self.assertIn("solidcraft-ui-egui", pk)
        self.assertIn("solidcraft", pk)
        self.assertNotIn("solidcraft-kernel", pk)
        self.assertNotIn("solidcraft-engine", pk)

    def test_vendor_readme_alone_selects_nothing(self):
        self.assertEqual(affected.select(self.meta, ["vendor/README.md"]), (False, []))

    def test_translation_selects_the_ui_and_xtask_not_the_kernel(self):
        full, pk = affected.select(self.meta, ["assets/locales/ja.json", "assets/locales/it.json"])
        self.assertFalse(full)
        self.assertIn("solidcraft-ui-egui", pk)
        self.assertIn("xtask", pk)
        self.assertNotIn("solidcraft-kernel", pk)
        self.assertNotIn("solidcraft-engine", pk)


if __name__ == "__main__":
    unittest.main()

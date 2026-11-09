"""Regression checks for the authoring/native gate, without launching a build."""
from __future__ import annotations

import copy
from pathlib import Path
import sys
import tempfile
import unittest

import check_all


def complete_report():
    return {"schema": "solidcraft-native-starter-check/1", "runtime_ready": False,
            "surfaces": 48, "texture_inputs": 192, "height_sources_verified_not_bound": 48,
            "hdr_inputs": 24, "mip_levels": 1920,
            "textures": [{"surface": f"SURF-{i:03}", "role": role, "source_sha256": "a" * 64,
                          "normalized_sha256": "b" * 64, "mip_levels": 10}
                         for i in range(1, 49) for role in check_all.ROLES],
            "environments": [{"environment": f"HDR-{i:03}", "source_sha256": "c" * 64,
                              "decoded_f32_le_sha256": "d" * 64, "pixels": 1024 * 512}
                             for i in range(1, 25)]}


class GateChecks(unittest.TestCase):
    def test_complete_report_has_explicit_non_runtime_scope(self):
        check_all.check_native_report(complete_report())

    def test_partial_false_availability_or_wrong_counts_refuse(self):
        for field, value in (("runtime_ready", True), ("runtime_ready", 0),
                             ("texture_inputs", 191), ("texture_inputs", 192.0),
                             ("mip_levels", True), ("schema", "another/1")):
            report = complete_report()
            report[field] = value
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                check_all.check_native_report(report)

    def test_duplicate_receipts_cannot_hide_a_missing_input(self):
        for field in ("textures", "environments"):
            report = complete_report()
            report[field][-1] = copy.deepcopy(report[field][0])
            with self.subTest(field=field), self.assertRaises(ValueError):
                check_all.check_native_report(report)

    def test_bad_hashes_and_incomplete_pixel_receipts_refuse(self):
        for field, key, value in (("textures", "normalized_sha256", "X" * 64),
                                  ("textures", "mip_levels", 1),
                                  ("environments", "pixels", 1),
                                  ("environments", "decoded_f32_le_sha256", None)):
            report = complete_report()
            report[field][0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                check_all.check_native_report(report)

    def test_fingerprints_detect_changed_bytes_and_added_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            one, two = root / "one.bin", root / "two.bin"
            one.write_bytes(b"one")
            before = check_all.fingerprint(root, [one])
            one.write_bytes(b"two")
            self.assertNotEqual(before, check_all.fingerprint(root, [one]))
            two.write_bytes(b"more")
            self.assertNotEqual(check_all.fingerprint(root, [one]), check_all.fingerprint(root, [one, two]))
            self.assertEqual(set(before), {"one.bin"})

    def test_subprocess_failure_keeps_logs_and_nonzero_exit(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = check_all.run_stage("fixture", [sys.executable, "-c", "print('diagnostic'); raise SystemExit(7)"], root, root)
            self.assertEqual(result["exit_code"], 7)
            self.assertIn("diagnostic", (root / result["stdout"]).read_text())

    def test_physical_references_run_in_both_gate_modes(self):
        for authoring_only in (True, False):
            commands = dict(check_all.stages(authoring_only, False))
            self.assertIn("physical-tests", commands)
            self.assertIn("physical-references", commands)
            self.assertIn("validate", commands["physical-references"])
            self.assertNotIn("build", commands["physical-references"])

    def test_native_gate_keeps_default_and_optional_cpu_feature_coverage_separate(self):
        native = dict(check_all.stages(False, False))
        for name in ("render-clippy", "render-unit-tests"):
            self.assertNotIn("--features", native[name])
        for name in ("render-cpu-clippy", "render-cpu-unit-tests"):
            command = native[name]
            self.assertEqual(command[command.index("--features")+1], "raytrace-cpu")
            self.assertIn("--locked", command)
        authoring = dict(check_all.stages(True, False))
        self.assertNotIn("render-cpu-clippy", authoring)
        self.assertNotIn("render-cpu-unit-tests", authoring)

    def test_authoring_only_is_explicit_and_native_is_default(self):
        self.assertIn("native-admission", {name for name, _ in check_all.stages(False, False)})
        self.assertNotIn("native-admission", {name for name, _ in check_all.stages(True, False)})
        command = dict(check_all.stages(True, True))["environments-reproduce"]
        self.assertIn("--independent-decode", command)


if __name__ == "__main__":
    unittest.main()

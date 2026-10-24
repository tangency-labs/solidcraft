#!/usr/bin/env python3
"""Numerical and admission checks for original offline light authoring."""

from __future__ import annotations

import copy
import json
import math
import unittest
from unittest.mock import patch

import numpy as np

import lighting


def area(position=(0.0, 0.0, 2.0), size=(1.0, 1.0), target=(0.0, 0.0, 0.0)):
    return {"id": "fixture", "type": "rectangular_area", "position_b": list(position),
            "target_b": list(target), "size_b": {"width": size[0], "height": size[1]},
            "roll_rad": 0.0, "color": {"space": "linear_srgb", "value": [1.0, 1.0, 1.0]},
            "intensity": {"quantity": "reference_contribution_ev", "value": 0.0, "reference_id": "CAL-LIGHT-001"},
            "emission_sides": "front_only", "energy_owner": "analytic_light"}


class LightingChecks(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.recipe, cls.source, _ = lighting.load_inputs()
        cls.export, _ = lighting.read_json(lighting.EXPORT)

    def test_surface_measure_matches_closed_form_directional_sphere(self):
        normals, weights = lighting.sphere_samples(96)
        numeric = float(np.dot(np.maximum(0, normals[:, 2]), weights))
        expected = 1 / 9 + 4 / (9 * math.pi)
        self.assertLess(abs(numeric / expected - 1), 0.0003)

    def test_six_separate_projected_disk_views_match_surface_integral(self):
        # Different target/area/orientation cases exercise the change of measure.
        for source in (area(), area((-1.2, -1.0, 0.7), (0.15, 1.4)),
                       area((0.2, 0.3, 1.4), (1.4, 1.1), (0.0, 0.2, -0.2))):
            with self.subTest(position=source["position_b"]):
                surface = lighting.response(source, 72, 20)
                disks = lighting.disk_response(source)
                self.assertLess(abs(surface / disks - 1), 0.001)

    def test_far_field_small_area_converges_to_inverse_square(self):
        n = np.array([[0.0, 0.0, 1.0]])
        near = lighting.unit_irradiance(area((0, 0, 10.25), (0.01, 0.01)), n, 0.25, 12)[0]
        far = lighting.unit_irradiance(area((0, 0, 20.25), (0.01, 0.01)), n, 0.25, 12)[0]
        expected_near = 0.0001 / 100
        self.assertLess(abs(near / expected_near - 1), 1e-6)
        self.assertLess(abs(near / far - 4), 5e-6)

    def test_back_of_area_emits_no_energy(self):
        n = np.array([[0.0, 0.0, 1.0]])
        source = area(target=(0, 0, 3))
        self.assertEqual(float(lighting.unit_irradiance(source, n, 0.25, 12)[0]), 0)

    def test_uniform_disk_sun_matches_hemisphere_integral(self):
        source = self.source["items"][19]["lights"][0]
        resolved = lighting.resolve_light(source, self.recipe["calibration"], {})
        theta, weight = np.polynomial.legendre.leggauss(64)
        half_angle = source["angular_diameter_rad"] / 2
        theta = (theta + 1) * half_angle / 2
        integral = 2 * math.pi * float(np.sum(weight * np.cos(theta) * np.sin(theta))) * half_angle / 2
        self.assertLess(abs(resolved["emission"]["rgb"][0] * integral - 1.0), 1e-12)

    def test_double_ev_doubles_calibrated_radiance(self):
        source = area()
        cache = {}
        a = lighting.resolve_light(source, self.recipe["calibration"], cache)
        source["intensity"]["value"] = 1.0
        b = lighting.resolve_light(source, self.recipe["calibration"], cache)
        np.testing.assert_allclose(b["emission"]["rgb"], np.array(a["emission"]["rgb"]) * 2, rtol=1e-14)

    def test_areas_preserve_response_when_all_lengths_scale_together(self):
        # Homogeneous scaling preserves dA / r^2; area radiance is not point intensity.
        source = area()
        n, w = lighting.sphere_samples(48)
        original = float(np.dot(lighting.unit_irradiance(source, n, 0.25, 12), w))
        source["position_b"] = (np.array(source["position_b"]) * 3).tolist()
        source["target_b"] = (np.array(source["target_b"]) * 3).tolist()
        source["size_b"] = {k: v * 3 for k, v in source["size_b"].items()}
        scaled = float(np.dot(lighting.unit_irradiance(source, n, 0.75, 12), w))
        self.assertLess(abs(scaled / original - 1), 1e-12)

    def test_all_exported_area_frames_are_orthonormal(self):
        for rig in self.export["items"]:
            for source in rig["lights"] + rig["modifiers"]:
                if "width_axis" in source:
                    frame = np.array([source["width_axis"], source["height_axis"], source["normal"]])
                    np.testing.assert_allclose(frame @ frame.T, np.eye(3), atol=3e-11)
                    self.assertGreater(float(np.linalg.det(frame)), 0.9999999999)

    def test_export_calibrated_responses_meet_reference_and_convergence(self):
        source_rigs = {rig["id"]: rig for rig in self.source["items"]}
        for rig in self.export["items"]:
            source_lights = {light["id"]: light for light in source_rigs[rig["preset_id"]]["lights"]}
            for source in rig["lights"]:
                if "calibration" in source:
                    c = source["calibration"]
                    input_luminance = float(np.dot(source_lights[source["id"]]["color"]["value"], lighting.LUMA))
                    output_luminance = float(np.dot(source["emission"]["rgb"], lighting.LUMA))
                    # unit_radiance_response already includes the authored color.
                    gain = output_luminance / input_luminance
                    self.assertAlmostEqual(gain * c["unit_radiance_response"],
                                           c["target_six_view_mean_luminance"], places=9)
                    self.assertLess(c["higher_order_relative_difference"], self.recipe["calibration"]["max_relative_error"])

    def test_convergence_gate_uses_unrounded_difference(self):
        config = dict(self.recipe["calibration"], max_relative_error=0.0015)
        # The actual difference exceeds the gate but its reported 12-decimal
        # value equals the gate. Rounding must never admit this calculation.
        with patch.object(lighting, "response", side_effect=[1.0015000000002, 1.0]):
            with self.assertRaises(lighting.InvalidRecipe):
                lighting.resolve_light(area(), config, {})

    def test_colored_source_uses_luminance_once_and_keeps_chromaticity(self):
        for rgb in ([1.0, 0.2, 0.05], [0.0, 0.0, 1.0]):
            with self.subTest(rgb=rgb):
                source = area((-1.2, -1.0, 0.7), (0.4, 1.1))
                source["color"]["value"] = rgb
                source["intensity"]["value"] = -1.0
                resolved = lighting.resolve_light(source, self.recipe["calibration"], {})
                emitted = np.array(resolved["emission"]["rgb"])
                input_luminance = float(np.dot(rgb, lighting.LUMA))
                gain = float(np.dot(emitted, lighting.LUMA)) / input_luminance
                np.testing.assert_allclose(emitted, np.array(rgb) * gain, rtol=1e-14, atol=1e-14)
                # Independent six-disk integration includes the color itself.
                observed = lighting.disk_response(source) * gain
                self.assertLess(abs(observed / 0.5 - 1), 0.001)

    def test_incomplete_aperture_and_editorial_calibration_is_explicit(self):
        by_id = {i["preset_id"]: i for i in self.export["items"]}
        for preset in ("STU-018", "STU-019", "STU-024"):
            self.assertEqual(by_id[preset]["state"], "blocked_calibration")
            blocked = [i for i in by_id[preset]["lights"] if i["state"] == "blocked"]
            self.assertTrue(blocked)
            self.assertTrue(all(i["emission"] is None for i in blocked))

    def test_passive_cards_have_no_emission_and_are_excluded_from_calibration(self):
        for rig in self.export["items"]:
            for card in rig["modifiers"]:
                self.assertEqual(card["emission"], 0)
            for source in rig["lights"]:
                if "calibration" in source:
                    self.assertFalse(source["calibration"]["includes_passive_cards"])

    def test_lunar_recipes_add_no_ambient_or_duplicate_sun(self):
        by_id = {i["preset_id"]: i for i in self.export["items"]}
        self.assertEqual(len(by_id["STU-020"]["lights"]), 1)
        self.assertEqual(by_id["STU-020"]["lights"][0]["energy_owner"], "rig_sun")
        for preset in ("STU-021", "STU-022", "STU-023"):
            self.assertEqual(by_id[preset]["lights"], [])
        self.assertEqual(by_id["STU-021"]["state"], "requires_existing_environment_sun")
        self.assertEqual(by_id["STU-023"]["state"], "requires_existing_environment_sun")

    def test_near_field_source_cannot_use_enclosing_sphere(self):
        with self.assertRaises(lighting.InvalidRecipe):
            lighting.response(area((0, 0.025, 0), (0.8, 0.025)), 24, 8)

    def test_input_numbers_and_axes_are_strict(self):
        for value in (True, float("nan"), float("inf"), "1", -1, 10 ** 400, -(10 ** 400)):
            with self.subTest(value=value), self.assertRaises(lighting.InvalidRecipe):
                lighting.number(value, "positive bounded number", 0, 10)
        with self.assertRaises(lighting.InvalidRecipe):
            lighting.area_frame(area((0, 0, 0)))
        bad = area()
        bad["size_b"]["width"] = 1000000
        with self.assertRaises(lighting.InvalidRecipe):
            lighting.area_frame(bad)

    def test_duplicate_json_keys_are_rejected(self):
        with self.assertRaises(lighting.InvalidRecipe):
            json.loads('{"radiance": 1, "radiance": 2}', object_pairs_hook=lighting.unique_pairs)

    def test_hidden_fill_and_active_reflector_are_rejected(self):
        rig = copy.deepcopy(self.source["items"][19])
        rig["lights"].append(area())
        with self.assertRaises(lighting.InvalidRecipe):
            lighting.validate_source_rig(rig, 19)
        rig = copy.deepcopy(self.source["items"][0])
        rig["modifiers"][0]["emission"] = 1
        with self.assertRaises(lighting.InvalidRecipe):
            lighting.validate_source_rig(rig, 0)

    def test_all_records_retain_unbound_state(self):
        self.assertFalse(self.export["runtime_ready"])
        self.assertTrue(all(i["runtime_ready"] is False for i in self.export["items"]))
        self.assertEqual(len(self.export["items"]), 24)
        self.assertEqual(self.export["counts_by_state"], {
            "authored_numeric_unbound": 19, "blocked_calibration": 3,
            "requires_existing_environment_sun": 2})


if __name__ == "__main__":
    unittest.main()

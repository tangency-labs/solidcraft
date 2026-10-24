"""Analytic invariants and hostile-input checks for authored HDR resources."""

from __future__ import annotations

import copy
import json
import math
from pathlib import Path
import tempfile
import unittest

import numpy as np

import environments as env


class EnvironmentChecks(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.recipes, cls.raw = env.read_recipes()

    def changed_source(self, callback):
        value = copy.deepcopy(self.recipes)
        callback(value)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "recipes.json"
            path.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                env.read_recipes(path)

    def test_published_rgbe_example(self):
        # The public Radiance format note gives this exact value/byte example.
        actual = env.rgbe_pixels(np.array([[[1.0, 0.5, 0.25]]]))
        self.assertEqual(actual[0, 0].tolist(), [128, 64, 32, 129])

    def test_uniform_irradiance_is_pi_times_radiance(self):
        field = np.ones((128, 256, 3)) * [1.0, 0.5, 0.25]
        stats = env.metrics(field)
        np.testing.assert_allclose(stats["solid_angle_weighted_mean_rgb"], [1, 0.5, 0.25], atol=1e-8)
        for integral in stats["hemisphere_cosine_integrals_rgb"].values():
            np.testing.assert_allclose(integral, np.array([1, 0.5, 0.25]) * math.pi, rtol=1e-4)

    def test_known_cardinal_panel_direction(self):
        item = copy.deepcopy(self.recipes["items"][0])
        item["ambient"]["upper_linear_srgb"] = [0, 0, 0]
        item["ambient"]["lower_linear_srgb"] = [0, 0, 0]
        item["panels"][0].update(azimuth_deg=0, elevation_deg=0, radiance_linear_srgb=[8, 4, 2])
        rays = np.array([[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, 0, 1]])
        np.testing.assert_array_equal(env.evaluate(item, rays), [[8, 4, 2], [0, 0, 0], [0, 0, 0], [0, 0, 0]])

    def test_spherical_direction_grid_unit_vectors(self):
        grid = env.directions(256, 128)
        np.testing.assert_allclose(np.linalg.norm(grid, axis=-1), 1, atol=1e-14)
        self.assertGreater(grid[0, 0, 2], 0.999)
        self.assertLess(grid[-1, 0, 2], -0.999)
        self.assertGreater(grid[64, 128, 0], 0.999)
        self.assertGreater(grid[64, 192, 1], 0.999)

    def test_vacuum_cannot_gain_ambient_energy(self):
        self.changed_source(lambda d: d["items"][-1]["ambient"]["upper_linear_srgb"].__setitem__(0, 0.1))

    def test_measurement_and_binding_claims_cannot_change(self):
        self.changed_source(lambda d: d["authoring"].__setitem__("measurement_status", "measured"))
        self.changed_source(lambda d: d["authoring"].__setitem__("runtime_status", "validated"))

    def test_negative_nonfinite_and_boolean_radiance_rejected(self):
        for invalid in [-1, float("inf"), float("nan"), True]:
            self.changed_source(lambda d, value=invalid: d["items"][0]["ambient"]["upper_linear_srgb"].__setitem__(0, value))

    def test_huge_json_integers_rejected_without_float_overflow(self):
        for invalid in [10 ** 400, -(10 ** 400)]:
            self.changed_source(lambda d, value=invalid: d["items"][0]["ambient"]["upper_linear_srgb"].__setitem__(0, value))

    def test_path_traversal_unknown_fields_and_undersampled_panels_rejected(self):
        self.changed_source(lambda d: d["items"][0].__setitem__("slug", "../outside"))
        self.changed_source(lambda d: d["items"][0].__setitem__("calibrated", True))
        self.changed_source(lambda d: d["items"][0]["panels"][0].__setitem__("width_deg", 0.53))

    def test_source_projection_and_duplicate_ids_rejected(self):
        self.changed_source(lambda d: d["projection"].__setitem__("up_axis", "+Y"))
        self.changed_source(lambda d: d["items"][1].__setitem__("id", d["items"][0]["id"]))

    def test_duplicate_json_fields_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "recipes.json"
            path.write_bytes(self.raw.replace(b'"version": "1.0.0"', b'"version": "1.0.0", "version": "1.0.0"', 1))
            with self.assertRaises(ValueError):
                env.read_recipes(path)

    def test_hdr_truncation_trailing_data_and_corrections_rejected(self):
        valid = env.encode_hdr(np.ones((4, 8, 3)))
        for invalid in [valid[:-1], valid + b"trailing", valid.replace(b"EXPOSURE=1\n", b"EXPOSURE=1\nEXPOSURE=2\n")]:
            with self.assertRaises(ValueError):
                env.decode_hdr(invalid)

    def test_grid_and_radiance_allocation_bounds(self):
        for width, height in [(4096, 2048), (1024, 1024), (True, 1), (-2, -1)]:
            with self.assertRaises(ValueError):
                env.directions(width, height)
        with self.assertRaises(ValueError):
            env.rgbe_pixels(np.full((4, 8, 3), float("nan")))


if __name__ == "__main__":
    unittest.main()

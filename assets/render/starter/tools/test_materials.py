"""Analytic surface invariants and hostile-input checks for authored PBR maps.

These checks use known sRGB values and analytic periodic height fields rather
than treating a second call to the generator as an independent numerical oracle.
Export checks build one small resource in a temporary tree, never in the pack.
"""
from __future__ import annotations

import copy
import io
import json
import math
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import numpy as np
from PIL import Image

import materials as mat


def synthetic_item():
    """A bounded numerical fixture, not an additional published material."""
    return {
        "id": "SURF-001", "name": "Analytic test surface",
        "catalog_ids": ["MAT-031"], "domains": ["validation"],
        "pattern": "grain", "seed": 3187,
        "tile_mm": [40.0, 20.0], "height_span_mm": 0.2,
        "base_linear_low": [0.18, 0.18, 0.18],
        "base_linear_high": [0.18, 0.18, 0.18],
        "roughness": 0.18, "roughness_variation": 0.0,
        "metallic": 0.0, "cycles_u": 6, "cycles_v": 10,
        "limitations": ["Analytic fixture; no measured material claim."],
    }


class NumericalSurfaceChecks(unittest.TestCase):
    def test_srgb_reference_values_and_linear_data_separation(self):
        # Standard sRGB anchors: 18% linear gray is about 46.14% encoded gray.
        linear = np.array([0.0, 0.0031308, 0.18, 0.5, 1.0])
        expected = np.array([0.0, 0.040449936, 0.4613561295, 0.7353569831, 1.0])
        np.testing.assert_allclose(mat.srgb_encode(linear), expected, atol=1e-10)
        item = synthetic_item()
        item["metallic"] = 0.18
        maps = mat.make_maps(item, 256)
        self.assertTrue(np.all(maps["base_color"] == 118))
        self.assertTrue(np.all(maps["roughness"] == 46))
        self.assertTrue(np.all(maps["metallic"] == 46))

    def test_sinusoidal_normals_have_correct_axes_and_physical_magnitude(self):
        rows, cols = 512, 512
        u = (np.arange(cols)[None, :] + 0.5) / cols
        v = 1 - (np.arange(rows)[:, None] + 0.5) / rows
        height = 0.2 * np.sin(6 * np.pi * u) + 0.1 * np.sin(4 * np.pi * v)
        gradient_u = np.broadcast_to(0.2 * 6 * np.pi / 40 * np.cos(6 * np.pi * u), height.shape)
        gradient_v = np.broadcast_to(0.1 * 4 * np.pi / 20 * np.cos(4 * np.pi * v), height.shape)
        expected = np.stack((-gradient_u, -gradient_v, np.ones_like(height)), axis=-1)
        expected /= np.linalg.norm(expected, axis=-1, keepdims=True)
        actual = mat.normal_from_height(height, [40, 20])
        # Bound the finite-difference truncation error against the analytic derivative.
        np.testing.assert_allclose(actual, expected, atol=2.2e-5)
        self.assertLess(actual[0, 0, 0], 0)
        self.assertLess(actual[0, 0, 1], 0)
        self.assertTrue(np.all(actual[..., 2] > 0))
        np.testing.assert_allclose(np.linalg.norm(actual, axis=-1), 1, atol=2e-15)

    def test_physical_scaling_is_independent_of_image_resolution(self):
        slopes = []
        for resolution in (128, 256, 512):
            u = (np.arange(resolution)[None, :] + 0.5) / resolution
            height = np.broadcast_to(0.15 * np.sin(2 * np.pi * u), (32, resolution)).copy()
            normal = mat.normal_from_height(height, [30, 10])
            large_tile = mat.normal_from_height(height, [60, 20])
            scaled_both = mat.normal_from_height(height * 2, [60, 20])
            np.testing.assert_allclose(scaled_both, normal, atol=1e-15)
            np.testing.assert_allclose(large_tile[..., 0] / large_tile[..., 2],
                                       0.5 * normal[..., 0] / normal[..., 2], atol=1e-15)
            slopes.append(float(np.max(np.abs(normal[..., 0] / normal[..., 2]))))
        np.testing.assert_allclose(slopes, [0.15 * 2 * math.pi / 30] * 3, rtol=0.0008)

    def test_every_pattern_repeats_at_the_boundary_and_after_unit_translation(self):
        item = synthetic_item()
        u = np.array([0.013, 0.137, 0.317, 0.581, 0.923])
        v = np.array([0.797, 0.059, 0.419, 0.233, 0.887])
        for kind in sorted(mat.KINDS):
            with self.subTest(pattern=kind):
                item["pattern"] = kind
                item["cycles_u"], item["cycles_v"] = (8, 12) if kind == "weave_twill" else (6, 10)
                self.assertLess(mat.seams(item), 1e-10)
                original = mat.fields(item, u, v)
                for shifted in (mat.fields(item, u + 1, v), mat.fields(item, u, v - 1)):
                    for expected, actual in zip(original, shifted):
                        self.assertTrue(np.isfinite(actual).all())
                        self.assertTrue(np.all((actual >= 0) & (actual <= 1)))
                        np.testing.assert_allclose(actual, expected, atol=1e-10, rtol=0)

    def test_two_over_two_twill_has_a_diagonal_four_cell_repeat(self):
        item = synthetic_item()
        item.update(pattern="weave_twill", cycles_u=8, cycles_v=8)
        # Sample away from equal-height yarn intersections so the over/under
        # ownership is observable. Remove the unrelated fine noise for this test.
        u = (np.arange(8) + 0.25) / 8
        v = np.full(8, 0.125 / 8)

        def no_noise(x, y, _seed, _frequency=1):
            return np.zeros(np.broadcast_shapes(np.shape(x), np.shape(y)))

        with patch.object(mat, "noise", no_noise):
            twill = mat.fields(item, u, v)[0]
            diagonal = mat.fields(item, u + 1 / 8, v + 1 / 8)[0]
            shifted_four = mat.fields(item, u + 4 / 8, v)[0]
            item["pattern"] = "weave"
            plain = mat.fields(item, u, v)[0]
        np.testing.assert_allclose(diagonal, twill, atol=1e-14)
        np.testing.assert_allclose(shifted_four, twill, atol=1e-14)
        # The twill row has two consecutive low/high crossings cyclically;
        # a plain weave alternates at every crossing.
        twill_low = twill < (twill.min() + twill.max()) / 2
        plain_low = plain < (plain.min() + plain.max()) / 2
        np.testing.assert_array_equal(twill_low, [True, False, False, True] * 2)
        np.testing.assert_array_equal(plain_low, [True, False] * 4)
        self.assertFalse(np.allclose(twill, plain))

    def test_exported_height_and_normals_agree_with_analytic_surface(self):
        item = synthetic_item()
        item.update(tile_mm=[10.0, 20.0], height_span_mm=0.4)

        def analytic_fields(_item, u, v):
            height = 0.5 + 0.25 * np.sin(2 * np.pi * u) + 0.125 * np.sin(2 * np.pi * v)
            return height, np.full_like(height, 0.5), np.full_like(height, 0.4)

        with patch.object(mat, "fields", analytic_fields):
            maps = mat.make_maps(item, 256)
        raw = mat.png_bytes(maps["height"])
        with Image.open(io.BytesIO(raw)) as decoded:
            height = np.array(decoded)
        np.testing.assert_array_equal(height, maps["height"])
        self.assertEqual(height.dtype, np.uint16)
        u = (np.arange(256)[None, :] + 0.5) / 256
        v = 1 - (np.arange(256)[:, None] + 0.5) / 256
        expected_height = 0.1 * np.sin(2 * np.pi * u) + 0.05 * np.sin(2 * np.pi * v)
        decoded_mm = (height.astype(np.float64) / 65535 - 0.5) * 0.4
        np.testing.assert_allclose(decoded_mm, expected_height, atol=0.4 / (2 * 65535) + 1e-15)
        du = np.broadcast_to(0.1 * 2 * np.pi / 10 * np.cos(2 * np.pi * u), (256, 256))
        dv = np.broadcast_to(0.05 * 2 * np.pi / 20 * np.cos(2 * np.pi * v), (256, 256))
        expected_normal = np.stack((-du, -dv, np.ones_like(du)), axis=-1)
        expected_normal /= np.linalg.norm(expected_normal, axis=-1, keepdims=True)
        decoded_normal = maps["normal_open_gl"].astype(np.float64) / 255 * 2 - 1
        # One half-channel normal quantization step plus height/sampling error.
        np.testing.assert_allclose(decoded_normal, expected_normal, atol=0.0041)

    def test_zero_displacement_has_a_flat_normal_despite_surface_color(self):
        item = synthetic_item()
        item["height_span_mm"] = 0
        maps = mat.make_maps(item, 256)
        np.testing.assert_array_equal(maps["normal_open_gl"],
                                      np.broadcast_to([128, 128, 255], (256, 256, 3)))

    def test_png_preserves_16_bit_height_extrema_without_color_transform(self):
        source = np.array([[0, 1, 255], [256, 32768, 65535]], dtype=np.uint16)
        with Image.open(io.BytesIO(mat.png_bytes(source))) as decoded:
            self.assertEqual(decoded.format, "PNG")
            for metadata in ("icc_profile", "gamma", "exif"):
                self.assertNotIn(metadata, decoded.info)
            np.testing.assert_array_equal(np.array(decoded), source)


class SourceAdmissionChecks(unittest.TestCase):
    def setUp(self):
        self.source = {"schema": "solidcraft-authored-surfaces/1",
                       "generator_version": mat.VERSION, "resolution": 256,
                       "items": [synthetic_item()]}

    def rejects(self, change):
        candidate = copy.deepcopy(self.source)
        change(candidate)
        with self.assertRaises(ValueError):
            mat.validate_source(candidate)

    def test_nonobject_roots_and_nonobject_items_are_rejected_cleanly(self):
        for root in (None, [], "object", 17, True):
            with self.subTest(root=root), self.assertRaises(ValueError):
                mat.validate_source(root)
        for item in (None, [], "surface", 17, True):
            with self.subTest(item=item):
                self.rejects(lambda source, value=item: source.__setitem__("items", [value]))

    def test_weave_requires_even_repeat_cycles(self):
        self.source["items"][0]["pattern"] = "weave"
        mat.validate_source(self.source)
        for axis in ("cycles_u", "cycles_v"):
            with self.subTest(axis=axis):
                self.rejects(lambda source, key=axis: source["items"][0].__setitem__(key, 7))

    def test_twill_requires_a_multiple_of_four_repeat_cells(self):
        self.source["items"][0].update(pattern="weave_twill", cycles_u=8, cycles_v=12)
        mat.validate_source(self.source)
        for axis in ("cycles_u", "cycles_v"):
            for invalid in (2, 3, 6, 10):
                with self.subTest(axis=axis, cycles=invalid):
                    self.rejects(lambda source, key=axis, value=invalid:
                                 source["items"][0].__setitem__(key, value))

    def test_finite_numbers_and_booleans_do_not_alias_numeric_fields(self):
        mutations = [("height_span_mm", -1), ("height_span_mm", 10),
                     ("height_span_mm", float("nan")), ("roughness", float("inf")),
                     ("metallic", -0.01), ("metallic", 1.01), ("roughness", True),
                     ("seed", True), ("seed", -1), ("seed", 2**32),
                     ("cycles_u", True), ("cycles_u", 0), ("cycles_u", 33)]
        for key, value in mutations:
            with self.subTest(key=key, value=value):
                self.rejects(lambda source, k=key, v=value: source["items"][0].__setitem__(k, v))
        for value in (True, 0, -1, float("nan"), float("inf")):
            with self.subTest(tile=value):
                self.rejects(lambda source, v=value: source["items"][0]["tile_mm"].__setitem__(0, v))
        for value in (True, -0.1, 1.1, float("inf")):
            with self.subTest(color=value):
                self.rejects(lambda source, v=value: source["items"][0]["base_linear_low"].__setitem__(0, v))

    def test_allocation_limits_unsafe_ids_and_duplicate_resources(self):
        for value in (True, 128, 4096, -256):
            with self.subTest(resolution=value):
                self.rejects(lambda source, v=value: source.__setitem__("resolution", v))
        for value in ("../outside", "SURF-001/extra", "/SURF-001", "SURF-1", None):
            with self.subTest(resource_id=value):
                self.rejects(lambda source, v=value: source["items"][0].__setitem__("id", v))
        self.rejects(lambda source: source["items"].append(copy.deepcopy(source["items"][0])))
        self.rejects(lambda source: source.__setitem__("items", []))
        self.rejects(lambda source: source.__setitem__("items", [synthetic_item()] * 65))

    def test_fidelity_metadata_cannot_be_missing_empty_or_malformed(self):
        mutations = [("name", " "), ("limitations", []), ("limitations", [None]),
                     ("limitations", [""]), ("domains", "industrial"),
                     ("domains", []), ("domains", [False]),
                     ("catalog_ids", ["MAT-999"]), ("catalog_ids", [[]]),
                     ("pattern", []), ("normal_strength", 2)]
        for key, value in mutations:
            with self.subTest(key=key, value=value):
                self.rejects(lambda source, k=key, v=value: source["items"][0].__setitem__(k, v))

    def test_duplicate_json_keys_nonfinite_constants_and_oversized_input(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "source.json"
            for invalid in ('{"id": 1, "id": 2}', '{"value": NaN}', '{"value": Infinity}'):
                with self.subTest(input=invalid):
                    path.write_text(invalid, encoding="utf-8")
                    with self.assertRaises(ValueError):
                        mat.load_json(path)
            path.write_bytes(b" " * 4_000_001)
            with self.assertRaises(ValueError):
                mat.load_json(path)


class ExportAdmissionChecks(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        root = Path(self.temporary.name)
        pack = root / "assets/render/starter"
        recipes = pack / "materials/source/recipes.json"
        recipes.parent.mkdir(parents=True)
        catalog = root / "docs/render-materials/materials.json"
        catalog.parent.mkdir(parents=True)
        catalog.write_text(json.dumps({"items": [{"id": "MAT-031"}]}), encoding="utf-8")
        source = {"schema": "solidcraft-authored-surfaces/1",
                  "generator_version": mat.VERSION, "resolution": 256,
                  "items": [synthetic_item()]}
        recipes.write_text(json.dumps(source), encoding="utf-8")
        self.manifest_path = pack / "materials/manifest.json"
        self.overrides = patch.multiple(mat, ROOT=root, PACK=pack, RECIPES=recipes,
                                        EXPORTS=pack / "materials/exports", MANIFEST=self.manifest_path)
        self.overrides.start()
        self.addCleanup(self.overrides.stop)
        mat.build()
        self.manifest = mat.load_json(self.manifest_path)

    def altered_manifest_rejected(self, change):
        candidate = copy.deepcopy(self.manifest)
        change(candidate)
        self.manifest_path.write_text(json.dumps(candidate), encoding="utf-8")
        try:
            with self.assertRaises(ValueError):
                mat.validate()
        finally:
            self.manifest_path.write_text(json.dumps(self.manifest), encoding="utf-8")

    def test_png_roundtrip_and_reproduction_preserve_all_five_roles(self):
        result = mat.validate(reproduce=True)
        self.assertEqual(result["maps_checked"], 5)
        self.assertEqual(result["surfaces"], 1)
        self.assertFalse(result["runtime_ready"])
        self.assertTrue(result["pixel_reproduction"])

    def test_manifest_cannot_misdeclare_physical_or_fidelity_semantics(self):
        mutations = [("height_zero", 0), ("normal_convention", "DirectX -Y"),
                     ("repeat", ["clamp", "clamp"]), ("sample_locations", "corners"),
                     ("ambient_occlusion", {"constant": 0.25, "reason": "invented occlusion"}),
                     ("appearance_fidelity", "measured"), ("runtime_status", "ready"),
                     ("source_boundary_max_error", 0.5)]
        for key, value in mutations:
            with self.subTest(key=key):
                self.altered_manifest_rejected(lambda manifest, k=key, v=value:
                                               manifest["items"][0].__setitem__(k, v))

    def test_manifest_map_shape_encoding_and_extrema_must_match_pixels(self):
        mutations = [("width", 1024), ("height", 1024), ("channels", 4),
                     ("bit_depth", 16), ("encoding", "linear_data"),
                     ("minimum", 0), ("maximum", 255)]
        for key, value in mutations:
            with self.subTest(key=key):
                self.altered_manifest_rejected(lambda manifest, k=key, v=value:
                                               manifest["items"][0]["maps"]["base_color"].__setitem__(k, v))

    def test_export_paths_and_source_receipts_cannot_be_replaced(self):
        for invalid in ("../outside.png", "/tmp/outside.png",
                        "assets/render/starter/materials/exports/SURF-001/height.png"):
            with self.subTest(path=invalid):
                self.altered_manifest_rejected(lambda manifest, path=invalid:
                                               manifest["items"][0]["maps"]["base_color"].__setitem__("path", path))
        for key in ("source_sha256", "generator_sha256"):
            with self.subTest(receipt=key):
                self.altered_manifest_rejected(lambda manifest, field=key:
                                               manifest.__setitem__(field, "0" * 64))

    def test_corrupt_png_is_rejected_before_it_can_be_admitted(self):
        record = self.manifest["items"][0]["maps"]["height"]
        path = mat.ROOT / record["path"]
        original = path.read_bytes()
        try:
            path.write_bytes(original[:-10])
            with self.assertRaises(ValueError):
                mat.validate()
        finally:
            path.write_bytes(original)

    def test_color_profile_metadata_is_rejected_even_with_matching_file_receipt(self):
        record = self.manifest["items"][0]["maps"]["base_color"]
        path = mat.ROOT / record["path"]
        with Image.open(path) as opened:
            image = opened.copy()
        data = io.BytesIO()
        image.save(data, format="PNG", icc_profile=b"unexpected color-profile payload")
        raw = data.getvalue()
        path.write_bytes(raw)
        record["sha256"] = mat.digest(raw)
        record["size_bytes"] = len(raw)
        self.manifest_path.write_text(json.dumps(self.manifest), encoding="utf-8")
        with self.assertRaises(ValueError):
            mat.validate()


if __name__ == "__main__":
    unittest.main()

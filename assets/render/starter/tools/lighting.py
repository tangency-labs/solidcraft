#!/usr/bin/env python3
"""Original offline light-recipe calibration; not a SolidCraft runtime adapter.

The only rig geometry authority is docs/render-materials/studio-presets.json.
This tool integrates its CAL-LIGHT-001 sphere directly, in linear relative units.
It never reads a CAD document, changes camera settings, or installs a source.
"""

from __future__ import annotations

import argparse
from collections import Counter
from functools import lru_cache
import hashlib
import json
import math
from pathlib import Path
import sys
from typing import Any

import numpy as np

ROOT = Path(__file__).resolve().parents[4]
PACK = ROOT / "assets/render/starter/lighting"
RECIPE = PACK / "recipe.json"
EXPORT = PACK / "exports/rigs.json"
GENERATOR = "solidcraft-light-author/1.0.0"
MAX_INPUT_BYTES = 2 * 1024 * 1024
LUMA = np.array([0.2126, 0.7152, 0.0722], dtype=np.float64)


class InvalidRecipe(ValueError):
    """A bounded authoring input or its calibration cannot be admitted."""


def unique_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise InvalidRecipe(f"Duplicate JSON key: {key}")
        result[key] = value
    return result


def read_json(path: Path) -> tuple[dict[str, Any], str]:
    if path.is_symlink() or path.stat().st_size > MAX_INPUT_BYTES:
        raise InvalidRecipe(f"Expected a bounded regular JSON file: {path}")
    raw = path.read_bytes()
    try:
        value = json.loads(raw, object_pairs_hook=unique_pairs,
                           parse_constant=lambda s: (_ for _ in ()).throw(
                               InvalidRecipe(f"Non-finite JSON number: {s}")))
    except (UnicodeError, json.JSONDecodeError, RecursionError) as error:
        raise InvalidRecipe(f"Invalid JSON: {path.name}") from error
    if not isinstance(value, dict):
        raise InvalidRecipe(f"Expected JSON object: {path.name}")
    return value, hashlib.sha256(raw).hexdigest()


def number(value: Any, label: str, low: float, high: float) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise InvalidRecipe(f"{label}: expected a number")
    # Compare bounds before converting arbitrary Python/JSON integers to float.
    if not low <= value <= high or not math.isfinite(value):
        raise InvalidRecipe(f"{label}: expected finite value in [{low}, {high}]")
    return float(value)


def vector(value: Any, label: str, bound: float = 100.0) -> np.ndarray:
    if not isinstance(value, list) or len(value) != 3:
        raise InvalidRecipe(f"{label}: expected three components")
    return np.array([number(v, label, -bound, bound) for v in value])


def unit(value: np.ndarray, label: str) -> np.ndarray:
    length = float(np.linalg.norm(value))
    if not math.isfinite(length) or length < 1e-10:
        raise InvalidRecipe(f"{label}: degenerate direction")
    return value / length


def axes(normal: np.ndarray, roll: float) -> tuple[np.ndarray, np.ndarray]:
    """Source frame: width cross height is the outward emitting normal."""
    up = np.array([0.0, 0.0, 1.0])
    up -= normal * float(np.dot(up, normal))
    if float(np.linalg.norm(up)) < 1e-8:
        up = np.array([0.0, 1.0, 0.0])
        up -= normal * float(np.dot(up, normal))
    up = unit(up, "source up")
    right = unit(np.cross(up, normal), "source right")
    # Right-handed rotation about the emitting normal.
    return (math.cos(roll) * right + math.sin(roll) * up,
            -math.sin(roll) * right + math.cos(roll) * up)


def area_frame(light: dict[str, Any]) -> tuple[np.ndarray, ...]:
    position = vector(light.get("position_b"), "position_b")
    target = vector(light.get("target_b"), "target_b")
    normal = unit(target - position, "target minus position")
    roll = number(light.get("roll_rad"), "roll_rad", -2 * math.pi, 2 * math.pi)
    right, up = axes(normal, roll)
    size = light.get("size_b")
    if not isinstance(size, dict) or set(size) != {"width", "height"}:
        raise InvalidRecipe("area size_b must contain width and height")
    width = number(size["width"], "width", 0.0001, 20.0)
    height = number(size["height"], "height", 0.0001, 20.0)
    return position, target, normal, right, up, np.array([width, height])


def closest_area_distance(light: dict[str, Any]) -> float:
    position, _, _, right, up, size = area_frame(light)
    on_area = position.copy()
    for axis, half_size in zip((right, up), size / 2):
        on_area += axis * np.clip(-float(np.dot(position, axis)), -half_size, half_size)
    return float(np.linalg.norm(on_area))


@lru_cache(maxsize=8)
def sphere_samples(order: int) -> tuple[np.ndarray, np.ndarray]:
    if type(order) is not int or not 8 <= order <= 128:
        raise InvalidRecipe("Sphere integration order must be in 8..128")
    z, wz = np.polynomial.legendre.leggauss(order)
    phi = 2 * math.pi * (np.arange(2 * order) + 0.5) / (2 * order)
    zz, pp = np.meshgrid(z, phi, indexing="ij")
    radial = np.sqrt(np.maximum(0, 1 - zz * zz))
    normals = np.stack((radial * np.cos(pp), radial * np.sin(pp), zz), axis=-1).reshape(-1, 3)
    domega = np.repeat(wz, 2 * order) * (2 * math.pi / (2 * order))
    # Six orthographic masked disk means: sum cos+ = |nx|+|ny|+|nz|.
    weights = domega * np.abs(normals).sum(axis=1) / (6 * math.pi)
    return normals, weights


def area_samples(light: dict[str, Any], order: int) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    if type(order) is not int or not 4 <= order <= 32:
        raise InvalidRecipe("Area integration order must be in 4..32")
    position, _, normal, right, up, size = area_frame(light)
    a, wa = np.polynomial.legendre.leggauss(order)
    xx, yy = np.meshgrid(a * size[0] / 2, a * size[1] / 2, indexing="ij")
    points = position + xx.reshape(-1, 1) * right + yy.reshape(-1, 1) * up
    weights = np.outer(wa, wa).reshape(-1) * float(np.prod(size)) / 4
    return points, weights, normal


def unit_irradiance(light: dict[str, Any], surface_normals: np.ndarray,
                    sphere_radius: float, area_order: int) -> np.ndarray:
    """Irradiance for unit area radiance; geometric lengths consistently use B.

    Integrate cos(receiver) cos(emitter) / distance^2 dA. Convex-sphere
    self-occlusion is exactly represented by the receiver positive cosine.
    No cards, other objects, medium, environment, or camera response is present.
    """
    points, weights, emitter_normal = area_samples(light, area_order)
    receivers = surface_normals * sphere_radius
    result = np.zeros(len(surface_normals), dtype=np.float64)
    for start in range(0, len(points), 24):
        delta = points[start:start + 24, None, :] - receivers[None, :, :]
        r2 = np.sum(delta * delta, axis=2)
        if np.any(r2 <= 1e-16):
            raise InvalidRecipe("Emitter intersects a reference sphere sample")
        inv_distance = 1 / np.sqrt(r2)
        cosine_in = np.maximum(0, np.sum(delta * surface_normals[None, :, :], axis=2) * inv_distance)
        cosine_out = np.maximum(0, -np.sum(delta * emitter_normal, axis=2) * inv_distance)
        result += np.sum(weights[start:start + 24, None] * cosine_in * cosine_out / r2, axis=0)
    return result


def color(light: dict[str, Any]) -> np.ndarray:
    value = light.get("color")
    if not isinstance(value, dict) or value.get("space") != "linear_srgb":
        raise InvalidRecipe("Emitter color must declare linear_srgb")
    rgb = vector(value.get("value"), "linear source RGB", 1.0)
    if np.any(rgb < 0) or float(np.dot(rgb, LUMA)) <= 1e-9:
        raise InvalidRecipe("Emitter color must be nonnegative with positive luminance")
    return rgb


def response(light: dict[str, Any], sphere_order: int, area_order: int,
             radius: float = 0.25, reflectance: float = 0.18) -> float:
    if closest_area_distance(light) <= radius + 1e-8:
        raise InvalidRecipe("Emitter intersects or lies inside the CAL-LIGHT-001 sphere")
    normals, weights = sphere_samples(sphere_order)
    irradiance = unit_irradiance(light, normals, radius, area_order)
    result = reflectance / math.pi * float(np.dot(irradiance, weights)) * float(np.dot(color(light), LUMA))
    if not math.isfinite(result) or result <= 1e-12:
        raise InvalidRecipe("CAL-LIGHT-001 response is zero or invalid")
    return result


def disk_response(light: dict[str, Any], radial_order: int = 28,
                  angular_count: int = 112, area_order: int = 20) -> float:
    """Independent integration of six projected disks, without surface weights."""
    t, wt = np.polynomial.legendre.leggauss(radial_order)
    radius = np.sqrt((t + 1) / 2)
    phi = 2 * math.pi * (np.arange(angular_count) + 0.5) / angular_count
    rr, pp = np.meshgrid(radius, phi, indexing="ij")
    base = np.stack((rr * np.cos(pp), rr * np.sin(pp), np.sqrt(1 - rr * rr)), axis=-1).reshape(-1, 3)
    w = np.repeat(wt / 2, angular_count) / angular_count
    total = 0.0
    for view_axis in range(3):
        for sign in (-1, 1):
            normals = np.roll(base, view_axis + 1, axis=1)
            normals[:, view_axis] *= sign
            total += float(np.dot(unit_irradiance(light, normals, 0.25, area_order), w))
    return 0.18 / math.pi * total / 6 * float(np.dot(color(light), LUMA))


def floats(value: Any) -> Any:
    if isinstance(value, np.ndarray):
        return floats(value.tolist())
    if isinstance(value, (float, np.floating)):
        if not math.isfinite(value):
            raise InvalidRecipe("Non-finite export number")
        return float(f"{float(value):.12g}")
    if isinstance(value, dict):
        return {k: floats(v) for k, v in value.items()}
    if isinstance(value, list):
        return [floats(v) for v in value]
    return value


def encoded(value: dict[str, Any]) -> bytes:
    return (json.dumps(floats(value), ensure_ascii=True, indent=2, allow_nan=False) + "\n").encode()


def load_inputs() -> tuple[dict[str, Any], dict[str, Any], str]:
    recipe, recipe_sha = read_json(RECIPE)
    expected = {"schema", "version", "provenance", "source", "calibration", "runtime_binding"}
    if set(recipe) != expected or recipe["schema"] != "solidcraft-light-author-recipe/1":
        raise InvalidRecipe("Unexpected authoring recipe schema or fields")
    if recipe["runtime_binding"] != "unbound":
        raise InvalidRecipe("This authoring tool cannot claim runtime binding")
    source = recipe["source"]
    if set(source) != {"path", "sha256", "count"} or source["path"] != "docs/render-materials/studio-presets.json":
        raise InvalidRecipe("Expected canonical studio preset source")
    catalog, source_sha = read_json(ROOT / source["path"])
    if source_sha != source["sha256"]:
        raise InvalidRecipe("Canonical studio source changed; explicitly review and update its pin")
    if source["count"] != 24 or len(catalog.get("items", [])) != 24:
        raise InvalidRecipe("Expected the reviewed 24-rig inventory")
    config = recipe["calibration"]
    if set(config) != {"id", "extent_mm", "sphere_radius_b", "reflectance", "sphere_order", "area_order", "check_sphere_order", "check_area_order", "max_relative_error"}:
        raise InvalidRecipe("Unexpected calibration fields")
    if config["id"] != "CAL-LIGHT-001" or config["sphere_radius_b"] != 0.25 or config["reflectance"] != 0.18:
        raise InvalidRecipe("CAL-LIGHT-001 sphere definition must be preserved")
    number(config["extent_mm"], "extent_mm", 0.1, 100000)
    number(config["max_relative_error"], "max_relative_error", 0.00001, 0.005)
    for key, lower, upper in [("sphere_order", 8, 128), ("check_sphere_order", 8, 128),
                              ("area_order", 4, 32), ("check_area_order", 4, 32)]:
        if type(config[key]) is not int or not lower <= config[key] <= upper:
            raise InvalidRecipe(f"Invalid integration order: {key}")
    if config["check_sphere_order"] <= config["sphere_order"] or config["check_area_order"] <= config["area_order"]:
        raise InvalidRecipe("Convergence check must increase both integration orders")
    return recipe, catalog, recipe_sha


def validate_source_rig(rig: dict[str, Any], index: int) -> None:
    if rig.get("id") != f"STU-{index + 1:03d}" or rig.get("status") != "specified_unbound":
        raise InvalidRecipe("Unexpected rig identity or upstream availability")
    if rig.get("mode") not in {"studio", "lunar_physical", "lunar_editorial"}:
        raise InvalidRecipe("Unknown lighting mode")
    for key in ("lights", "modifiers"):
        if not isinstance(rig.get(key), list) or len(rig[key]) > 8:
            raise InvalidRecipe(f"Bounded {key} array required")
        ids = [entry.get("id") for entry in rig[key]]
        if any(not isinstance(i, str) or not i or len(i) > 64 for i in ids) or len(set(ids)) != len(ids):
            raise InvalidRecipe(f"Invalid {key} identities")
    for light in rig["lights"]:
        color(light)
        intensity = light.get("intensity", {})
        number(intensity.get("value"), "intensity", -20, 20)
        if light["type"] == "rectangular_area":
            area_frame(light)
            if intensity.get("quantity") != "reference_contribution_ev" or intensity.get("reference_id") not in {"CAL-LIGHT-001", "CAL-ENV-SUN-001"}:
                raise InvalidRecipe("Unexpected area normalization")
            if light.get("emission_sides") != "front_only" or light.get("energy_owner") != "analytic_light":
                raise InvalidRecipe("Area source must have one front-side energy owner")
        elif light["type"] == "directional_sun":
            d = vector(light.get("direction"), "incoming photon direction", 1)
            if abs(float(np.linalg.norm(d)) - 1) > 1e-9:
                raise InvalidRecipe("Sun direction must be unit length")
            number(light.get("angular_diameter_rad"), "solar disk angle", 0.0001, 0.1)
            if intensity.get("quantity") != "relative_solar_irradiance" or intensity["value"] < 0:
                raise InvalidRecipe("Invalid solar source quantity")
            if light.get("energy_owner") != "rig_sun":
                raise InvalidRecipe("Invalid solar source owner")
        else:
            raise InvalidRecipe("Unsupported source type; no silent point/area substitution")
    for card in rig["modifiers"]:
        if card.get("type") != "diffuse_card" or card.get("emission") != 0 or type(card.get("emission")) is bool:
            raise InvalidRecipe("Modifiers must be passive diffuse cards")
        vector(card.get("position_b"), "card position")
        normal = vector(card.get("normal"), "card normal", 1)
        if abs(float(np.linalg.norm(normal)) - 1) > 1e-9:
            raise InvalidRecipe("Card normal must be unit length")
        number(card.get("roll_rad"), "card roll", -2 * math.pi, 2 * math.pi)
        for k in ("width", "height"):
            number(card.get("size_b", {}).get(k), "card size", 0.0001, 20)
        material = card.get("material", {})
        rgb = vector(material.get("reflectance_linear_srgb"), "card reflectance", 1)
        if material.get("model") != "lambertian" or np.any(rgb < 0) or np.any(rgb >= 1):
            raise InvalidRecipe("Invalid passive Lambertian reflector")
    if rig["mode"] == "lunar_physical" and any(light["type"] != "directional_sun" for light in rig["lights"]):
        raise InvalidRecipe("Hidden lunar fill is forbidden")
    if sum(light["type"] == "directional_sun" for light in rig["lights"]) > 1:
        raise InvalidRecipe("Only one owned solar source is permitted")


def resolve_light(light: dict[str, Any], config: dict[str, Any], cache: dict[str, Any]) -> dict[str, Any]:
    extent = config["extent_mm"]
    if light["type"] == "directional_sun":
        radius = light["angular_diameter_rad"] / 2
        irradiance = light["intensity"]["value"]
        # Uniform disk: integral L*cos(theta) dOmega = pi*L*sin(radius)^2.
        radiance = irradiance / (math.pi * math.sin(radius) ** 2)
        return {"id": light["id"], "type": "uniform_distant_disk", "energy_owner": "rig_sun",
                "incoming_photon_direction": light["direction"], "angular_diameter_rad": light["angular_diameter_rad"],
                "emission": {"quantity": "relative_linear_srgb_radiance", "rgb": color(light) * radiance},
                "reference_normal_irradiance_rgb": color(light) * irradiance,
                "state": "authored_numeric", "normalization": "authored_relative_solar_irradiance_not_measured",
                "requires": ["no_other_solar_energy_owner", "finite_angular_emitter", "scene_visibility"],
                "spectrum": "neutral_RGB_authoring_reference_not_measured_solar_spectrum"}
    position, target, normal, right, up, size = area_frame(light)
    result = {"id": light["id"], "type": "rectangular_area", "energy_owner": "analytic_light",
              "position_mm": position * extent, "target_mm": target * extent,
              "normal": normal, "width_axis": right, "height_axis": up,
              "width_mm": size[0] * extent, "height_mm": size[1] * extent,
              "emission_sides": "front_only", "visible_geometry_adds_energy": False}
    if light["intensity"]["reference_id"] != "CAL-LIGHT-001":
        return {**result, "state": "blocked", "emission": None,
                "blockers": ["CAL-ENV-SUN-001_requires_measured_existing_environment_sun_response"],
                "requested_reference_ev": light["intensity"]["value"]}
    if closest_area_distance(light) <= config["sphere_radius_b"] + 1e-8:
        return {**result, "state": "blocked", "emission": None,
                "blockers": ["emitter_intersects_or_is_inside_CAL-LIGHT-001_sphere", "aperture_specific_calibration_must_be_defined_and_reviewed"],
                "requested_reference_ev": light["intensity"]["value"]}
    # Geometry/color are the expensive part; equivalent rigs reuse a calculation.
    cache_key = json.dumps({k: light[k] for k in ("position_b", "target_b", "size_b", "roll_rad", "color")}, sort_keys=True)
    if cache_key not in cache:
        primary = response(light, config["sphere_order"], config["area_order"])
        check = response(light, config["check_sphere_order"], config["check_area_order"])
        relative_error = abs(primary - check) / check
        if relative_error > config["max_relative_error"]:
            raise InvalidRecipe(f"Convergence check failed for {light['id']}: {relative_error}")
        cache[cache_key] = (primary, check, relative_error)
    primary, check, relative_error = cache[cache_key]
    target_response = 2 ** light["intensity"]["value"]
    gain = target_response / primary
    return {**result, "state": "authored_numeric", "emission": {"quantity": "relative_linear_srgb_radiance", "rgb": color(light) * gain},
            "calibration": {"reference_id": "CAL-LIGHT-001", "reference_ev": light["intensity"]["value"],
                            "target_six_view_mean_luminance": target_response,
                            "unit_radiance_response": primary, "higher_order_response": check,
                            # Reporting precision only. Acceptance above uses the
                            # unrounded result, preserving the configured gate.
                            "higher_order_relative_difference": round(relative_error, 12),
                            "method": "direct_Lambertian_sphere_and_finite_area_quadrature",
                            "includes_passive_cards": False, "runtime_render_verified": False}}


def make_export() -> dict[str, Any]:
    recipe, source, recipe_sha = load_inputs()
    config = recipe["calibration"]
    cache: dict[str, Any] = {}
    items = []
    for index, rig in enumerate(source["items"]):
        validate_source_rig(rig, index)
        lights = [resolve_light(light, config, cache) for light in rig["lights"]]
        blockers = sorted({b for light in lights for b in light.get("blockers", [])})
        state = "blocked_calibration" if blockers else "authored_numeric_unbound"
        if rig["id"] in {"STU-021", "STU-023"}:
            state = "requires_existing_environment_sun"
        requirements = list(rig["locks"])
        if rig["id"] in {"STU-021", "STU-023"}:
            requirements += ["Exactly one existing environment solar source must be resolved; this record adds no light."]
        if rig["id"] == "STU-022":
            requirements += ["Verify zero emitted energy from all scene and environment sources; a black background is insufficient."]
        modifiers = []
        for card in rig["modifiers"]:
            right, up = axes(np.array(card["normal"], dtype=float), card["roll_rad"])
            modifiers.append({"id": card["id"], "type": "diffuse_card", "role": card["role"],
                              "position_mm": np.array(card["position_b"]) * config["extent_mm"],
                              "normal": card["normal"], "width_axis": right, "height_axis": up,
                              "width_mm": card["size_b"]["width"] * config["extent_mm"],
                              "height_mm": card["size_b"]["height"] * config["extent_mm"],
                              "material": card["material"], "emission": 0,
                              "casts_shadow": card["casts_shadow"], "two_sided": card["two_sided"]})
        items.append({"id": f"RIG-{index + 1:03d}", "preset_id": rig["id"], "catalog_id": rig["catalog_id"],
                      "name": rig["name"], "mode": rig["mode"], "state": state, "runtime_ready": False,
                      "source_subject_scale_policy": rig["subject_scale_policy"],
                      "fixture_extent_mm": config["extent_mm"], "frame": {"up": "+Z", "front": "-Y", "right": "+X"},
                      "lights": lights, "modifiers": modifiers, "blockers": blockers,
                      "required_capabilities": rig["required_capabilities"], "scene_preconditions": requirements,
                      "validation_scope": "Isolated analytic reference fixture; no subject visibility, adapter, optics, CAD or runtime rendering verified."})
    return {"schema": "solidcraft-authored-light-rigs/1", "version": "1.0.0", "generator": GENERATOR,
            "generator_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            "recipe_sha256": recipe_sha, "source": recipe["source"], "calibration": config,
            "license": "MIT OR Apache-2.0", "authoring_method": "original_deterministic_numeric_integration",
            "radiometric_units": {"radiance": "arbitrary_consistent_scene_referred_linear_RGB_radiance_unit",
                                  "irradiance": "integral_of_relative_radiance_times_projected_solid_angle",
                                  "physical_photometry": "not_watts_lumens_candela_or_lux",
                                  "quantity_distinction": "Area radiance, point intensity and normal irradiance are different quantities; do not reuse gains between them."},
            "binding": "offline_authoring_resources_unbound_to_application", "runtime_ready": False,
            "counts_by_state": dict(Counter(i["state"] for i in items)), "items": items}


def build() -> dict[str, Any]:
    data = make_export()
    EXPORT.parent.mkdir(parents=True, exist_ok=True)
    if EXPORT.is_symlink():
        raise InvalidRecipe("Refusing a symlink export")
    EXPORT.write_bytes(encoded(data))
    return {"status": "PASS", "rigs": len(data["items"]), "counts_by_state": data["counts_by_state"],
            "export": str(EXPORT.relative_to(ROOT)), "sha256": hashlib.sha256(EXPORT.read_bytes()).hexdigest()}


def validate() -> dict[str, Any]:
    expected = encoded(make_export())
    if not EXPORT.is_file() or EXPORT.is_symlink() or EXPORT.read_bytes() != expected:
        raise InvalidRecipe("Lighting export differs from reviewed source and generator; run build")
    data = json.loads(expected)
    return {"status": "PASS", "rigs": len(data["items"]), "counts_by_state": data["counts_by_state"],
            "maximum_convergence_difference": max(light.get("calibration", {}).get("higher_order_relative_difference", 0)
                                                  for rig in data["items"] for light in rig["lights"]),
            "runtime_ready": False, "sha256": hashlib.sha256(expected).hexdigest()}


def review() -> dict[str, Any]:
    """Render numerical fixture diagnostics only, into ignored target/."""
    from PIL import Image, ImageDraw, ImageFont

    _, source, _ = load_inputs()
    data, _ = read_json(EXPORT)
    original = {rig["id"]: rig for rig in source["items"]}
    admitted = [rig for rig in data["items"] if rig["state"] == "authored_numeric_unbound"]
    width, height, tile_w, tile_h = 1104, 1284, 268, 230
    sheet = Image.new("RGB", (width, height), (15, 20, 28))
    draw = ImageDraw.Draw(sheet)
    font = ImageFont.load_default(size=13)
    title_font = ImageFont.load_default(size=23)
    draw.text((24, 20), "LIGHTING / ANALYTIC REFERENCE", font=title_font, fill=(234, 240, 248))
    draw.text((24, 55), "Lambertian sphere, fixed view and exposure. No passive cards, subject geometry or SolidCraft runtime.",
              font=font, fill=(163, 179, 198))
    size = 160
    axis = lighting_view = unit(np.array([1.0, -1.4, 0.8]), "review view")
    right, up = axes(axis, 0)
    grid = 2 * (np.arange(size) + 0.5) / size - 1
    xx, yy = np.meshgrid(grid, -grid)
    visible = xx * xx + yy * yy <= 1
    zz = np.sqrt(np.maximum(0, 1 - xx[visible] ** 2 - yy[visible] ** 2))
    normals = xx[visible, None] * right + yy[visible, None] * up + zz[:, None] * lighting_view
    for index, rig in enumerate(admitted):
        x, y = 16 + (index % 4) * tile_w, 88 + (index // 4) * tile_h
        draw.rounded_rectangle((x, y, x + tile_w - 10, y + tile_h - 10), radius=8, fill=(24, 32, 44))
        draw.text((x + 10, y + 9), rig["preset_id"] + "  " + rig["catalog_id"], font=font, fill=(104, 200, 231))
        words = rig["name"].split()
        line, lines = "", []
        for word in words:
            if len(line + " " + word) > 33:
                lines.append(line)
                line = word
            else:
                line = (line + " " + word).strip()
        lines.append(line)
        draw.multiline_text((x + 10, y + 29), "\n".join(lines[:2]), font=font, fill=(228, 235, 245), spacing=2)
        rgb = np.zeros((len(normals), 3))
        for emitter, authored in zip(rig["lights"], original[rig["preset_id"]]["lights"]):
            if emitter["type"] == "rectangular_area":
                e = unit_irradiance(authored, normals, 0.25, 12)
                rgb += 0.18 / math.pi * e[:, None] * np.array(emitter["emission"]["rgb"])
            elif emitter["type"] == "uniform_distant_disk":
                towards = -np.array(emitter["incoming_photon_direction"])
                disk_right, disk_up = axes(towards, 0)
                q, wq = np.polynomial.legendre.leggauss(8)
                theta = (q + 1) * emitter["angular_diameter_rad"] / 4
                angle_weight = wq * emitter["angular_diameter_rad"] / 4
                integral = np.zeros(len(normals))
                for t, wt in zip(theta, angle_weight):
                    for phi in 2 * math.pi * (np.arange(32) + 0.5) / 32:
                        ray = math.cos(t) * towards + math.sin(t) * (math.cos(phi) * disk_right + math.sin(phi) * disk_up)
                        integral += np.maximum(0, np.sum(normals * ray, axis=1)) * math.sin(t) * wt * 2 * math.pi / 32
                rgb += 0.18 / math.pi * integral[:, None] * np.array(emitter["emission"]["rgb"])
        mapped = rgb / (1 + rgb)
        srgb = np.where(mapped <= 0.0031308, 12.92 * mapped, 1.055 * np.power(mapped, 1 / 2.4) - 0.055)
        pixels = np.full((size, size, 3), (24, 32, 44), dtype=np.uint8)
        pixels[visible] = np.round(np.clip(srgb, 0, 1) * 255).astype(np.uint8)
        sheet.paste(Image.fromarray(pixels), (x + (tile_w - 10 - size) // 2, y + 60))
    draw.text((24, 1256), "Numerical diagnostic only. Sun uses authored irradiance; area fixtures use CAL-LIGHT-001 EV targets.",
              font=font, fill=(163, 179, 198))
    destination = ROOT / "target/render-starter-review/lighting-reference.png"
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.is_symlink():
        raise InvalidRecipe("Refusing a symlink review image")
    sheet.save(destination)
    return {"status": "PASS", "image": str(destination.relative_to(ROOT)), "fixtures": len(admitted),
            "review_renderer": "authoring_math_only", "runtime_render_verified": False}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("build", "validate", "review"))
    args = parser.parse_args()
    try:
        result = {"build": build, "validate": validate, "review": review}[args.command]()
        print(json.dumps(result, indent=2))
        return 0
    except (InvalidRecipe, OSError, KeyError, TypeError, ValueError) as error:
        print(json.dumps({"status": "FAIL", "error": str(error)}), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

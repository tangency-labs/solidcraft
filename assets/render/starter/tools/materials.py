#!/usr/bin/env python3
"""Author original periodic PBR data fields; no application runtime dependency.

The recipe JSON is the editable authority. PNGs are required authoring exports,
not inferred material measurements. All signal construction is original maths.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import math
from pathlib import Path
import re
import sys

import numpy as np
from PIL import Image, ImageDraw

VERSION = "1.0.0"
PACK = Path(__file__).resolve().parents[1]
ROOT = PACK.parents[2]
RECIPES = PACK / "materials/source/recipes.json"
EXPORTS = PACK / "materials/exports"
MANIFEST = PACK / "materials/manifest.json"
KINDS = {"brushed", "cross_brushed", "grain", "galvanized", "milled",
         "turned", "powder", "hammered", "polymer", "rubber", "grip",
         "wood", "plywood", "fibre", "weave", "weave_twill", "pcb", "fdm", "fdm_top",
         "foil", "regolith", "cork"}
ROLES = {"base_color": ("RGB", 8, "srgb"),
         "normal_open_gl": ("RGB", 8, "linear_data"),
         "roughness": ("L", 8, "linear_data"),
         "metallic": ("L", 8, "linear_data"),
         "height": ("I;16", 16, "linear_data")}
ITEM_KEYS = {"id", "name", "catalog_ids", "domains", "pattern", "seed", "tile_mm",
             "base_linear_low", "base_linear_high", "roughness", "roughness_variation",
             "metallic", "height_span_mm", "cycles_u", "cycles_v", "limitations"}
NORMAL_CONVENTION = "OpenGL +Y, +U right, +V image-up; renormalize after sampling"
AO = {"constant": 1.0, "reason": "No invented cavity occlusion or baked lighting."}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path):
    if path.stat().st_size > 4_000_000:
        raise ValueError("JSON source exceeds 4 MB authoring limit")
    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_object,
                      parse_constant=lambda x: (_ for _ in ()).throw(ValueError(f"nonfinite {x}")))


def number(value, lower, upper, label):
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError(f"{label}: expected finite number")
    if not lower <= value <= upper or not math.isfinite(value):
        raise ValueError(f"{label}: outside [{lower}, {upper}]")


def validate_source(data):
    if not isinstance(data, dict):
        raise ValueError("source must be a JSON object")
    if data.get("schema") != "solidcraft-authored-surfaces/1":
        raise ValueError("unknown authoring schema")
    if data.get("generator_version") != VERSION:
        raise ValueError("generator version mismatch")
    size = data.get("resolution")
    if type(size) is not int or size not in (256, 512, 1024, 2048):
        raise ValueError("resolution must be an admitted power of two (256..2048)")
    items = data.get("items")
    if not isinstance(items, list) or not 1 <= len(items) <= 64:
        raise ValueError("expected 1..64 surface recipes")
    ids = set()
    catalog = {x["id"] for x in load_json(ROOT / "docs/render-materials/materials.json")["items"]}
    for item in items:
        if not isinstance(item, dict) or set(item) != ITEM_KEYS:
            raise ValueError("surface recipe must have exactly the documented fields")
        rid = item.get("id", "")
        if not isinstance(rid, str) or not re.fullmatch(r"SURF-[0-9]{3}", rid) or rid in ids:
            raise ValueError("unsafe or duplicate resource id")
        ids.add(rid)
        if not isinstance(item.get("name"), str) or not item["name"].strip():
            raise ValueError(f"{rid}: missing name")
        if not isinstance(item.get("pattern"), str) or item["pattern"] not in KINDS:
            raise ValueError(f"{rid}: unknown generator pattern")
        if type(item.get("seed")) is not int or not 0 <= item["seed"] < 2**32:
            raise ValueError(f"{rid}: invalid seed")
        refs = item.get("catalog_ids")
        if not isinstance(refs, list) or not refs or any(not isinstance(x, str) or x not in catalog for x in refs):
            raise ValueError(f"{rid}: unknown material concept")
        for key in ("tile_mm", "base_linear_low", "base_linear_high"):
            values = item.get(key)
            count = 2 if key == "tile_mm" else 3
            if not isinstance(values, list) or len(values) != count:
                raise ValueError(f"{rid}: invalid {key}")
            for value in values:
                number(value, 0.001 if key == "tile_mm" else 0,
                       10000 if key == "tile_mm" else 1, key)
        number(item.get("height_span_mm"), 0, min(item["tile_mm"]) / 8, "height span")
        for key in ("roughness", "roughness_variation", "metallic"):
            number(item.get(key), 0, 1, key)
        for key in ("cycles_u", "cycles_v"):
            value = item.get(key)
            if type(value) is not int or not 1 <= value <= size // 8:
                raise ValueError(f"{rid}: {key} must be integer 1..resolution/8")
            if item["pattern"] == "weave" and value % 2:
                raise ValueError(f"{rid}: weave cycles must be even for a periodic crossing")
            if item["pattern"] == "weave_twill" and value % 4:
                raise ValueError(f"{rid}: 2/2 twill cycles must be multiples of four")
        for key in ("domains", "limitations"):
            values = item.get(key)
            if not isinstance(values, list) or not values or any(not isinstance(v, str) or not v.strip() for v in values):
                raise ValueError(f"{rid}: nonempty string {key} required")
    return data


def noise(u, v, seed, frequency=1):
    """Bounded periodic Fourier field; integer frequencies guarantee repeat."""
    rng = np.random.Generator(np.random.PCG64(seed))
    result = np.zeros(np.broadcast_shapes(np.shape(u), np.shape(v)), dtype=np.float64)
    total = 0.0
    for octave in range(3):
        for _ in range(4):
            a, b = rng.integers(-3, 4, size=2)
            if a == 0 and b == 0:
                a = 1
            phase = float(rng.integers(0, 2**32)) / 2**32
            weight = 0.5**octave
            result += weight * np.sin(2 * np.pi * (
                (a * u + b * v) * frequency * 2**octave + phase))
            total += weight
    return result / total


def fields(item, u, v):
    """Continuous periodic height/tone/roughness fields in [0,1]."""
    seed = item["seed"]
    coarse = noise(u, v, seed, 1)
    fine = noise(u, v, seed + 1, 8)
    cu, cv = item["cycles_u"], item["cycles_v"]
    kind = item["pattern"]
    tone = 0.5 + 0.35 * coarse + 0.15 * fine
    h = 0.5 + 0.25 * coarse + 0.25 * fine
    if kind in ("brushed", "cross_brushed"):
        a = np.sin(2 * np.pi * (cv * v + 0.08 * coarse))
        b = np.sin(2 * np.pi * (cu * u + 0.05 * coarse))
        h = 0.5 + 0.32 * a + 0.08 * fine
        if kind == "cross_brushed":
            h = 0.5 + 0.20 * a + 0.20 * b + 0.06 * fine
        tone = 0.5 + 0.06 * coarse + 0.05 * fine
    elif kind == "milled":
        phase = cu * u + 0.5 * np.cos(2 * np.pi * cv * v)
        h = 0.5 + 0.32 * np.sin(2 * np.pi * phase) + 0.08 * fine
        tone = 0.5 + 0.08 * coarse
    elif kind == "turned":
        h = 0.5 + 0.38 * np.cos(2 * np.pi * cv * v) + 0.08 * fine
        tone = 0.5 + 0.05 * coarse
    elif kind in ("grain", "powder", "polymer", "rubber"):
        h = 0.5 + 0.13 * coarse + 0.34 * fine
        tone = 0.5 + 0.08 * coarse + 0.08 * fine
    elif kind == "hammered":
        a = np.sin(2 * np.pi * cu * u + 0.5 * coarse)
        b = np.cos(2 * np.pi * cv * v + 0.5 * coarse)
        h = 0.5 + 0.35 * a * b + 0.06 * fine
        tone = 0.5 + 0.20 * coarse + 0.15 * a * b
    elif kind in ("galvanized", "cork"):
        # Original periodic Voronoi cells; no photographed spangle source.
        rng = np.random.Generator(np.random.PCG64(seed + 3))
        first = np.full(np.broadcast_shapes(np.shape(u), np.shape(v)), np.inf)
        second = first.copy()
        cell = np.zeros_like(first)
        for _ in range(36):
            x, y, color = rng.random(3)
            dx = (u - x + 0.5) % 1 - 0.5
            dy = (v - y + 0.5) % 1 - 0.5
            distance = dx * dx + dy * dy
            closer = distance < first
            second = np.where(closer, first, np.minimum(second, distance))
            cell = np.where(closer, color, cell)
            first = np.minimum(first, distance)
        edge = np.clip((second - first) * 400, 0, 1)
        h = 0.3 + 0.35 * edge + 0.12 * fine
        tone = 0.15 + 0.65 * cell + 0.08 * fine
    elif kind == "grip":
        a = np.cos(2 * np.pi * (cu * u + cv * v))
        b = np.cos(2 * np.pi * (cu * u - cv * v))
        h = 0.5 + 0.40 * a * b + 0.03 * fine
        tone = 0.5 + 0.08 * coarse
    elif kind == "wood":
        # Continuous sinusoidal latewood; period and warp share the same torus.
        warp = 0.22 * np.sin(2 * np.pi * v) + 0.12 * coarse
        grain = 0.5 + 0.5 * np.cos(2 * np.pi * (cu * u + warp))
        pores = np.power(grain, 8)
        tone = 0.12 + 0.74 * grain + 0.10 * fine
        h = 0.65 - 0.38 * pores + 0.05 * fine
    elif kind == "plywood":
        layers = 0.5 + 0.5 * np.cos(2 * np.pi * cv * v)
        grain = np.sin(2 * np.pi * (cu * u + 0.12 * coarse))
        tone = 0.15 + 0.65 * layers + 0.08 * grain
        h = 0.5 + 0.20 * layers + 0.08 * fine
    elif kind == "fibre":
        fibres = np.sin(2 * np.pi * (cu * u + cv * v))
        h = 0.5 + 0.20 * fine + 0.14 * fibres
        tone = 0.5 + 0.23 * coarse + 0.18 * fine
    elif kind in ("weave", "weave_twill"):
        # Alternating crossing envelope; even cycle counts make repeat coherent.
        a = 0.5 + 0.5 * np.cos(2 * np.pi * cu * u)
        b = 0.5 + 0.5 * np.cos(2 * np.pi * cv * v)
        crossing = (np.cos(np.pi / 2 * (cu * u - cv * v)) if kind == "weave_twill"
                    else np.sin(np.pi * cu * u) * np.sin(np.pi * cv * v))
        weight = 0.5 + 0.5 * np.tanh(8 * crossing)
        h = 0.20 + 0.58 * (weight * a + (1 - weight) * b) + 0.04 * fine
        tone = 0.3 + 0.4 * (weight * a + (1 - weight) * b) + 0.05 * fine
    elif kind == "pcb":
        # Continuous solder-mask finish only: no fabricated circuit/marking.
        h = 0.5 + 0.12 * coarse + 0.18 * fine
        tone = 0.5 + 0.12 * coarse + 0.10 * fine
    elif kind in ("fdm", "fdm_top"):
        phase = cv * v if kind == "fdm" else cu * u + cv * v
        bead = 0.5 + 0.5 * np.cos(2 * np.pi * phase + 0.03 * coarse)
        h = 0.15 + 0.75 * np.sqrt(np.maximum(0.0, bead)) + 0.015 * fine
        tone = 0.5 + 0.04 * fine
    elif kind == "foil":
        folds = (np.sin(2 * np.pi * (cu * u + v)) +
                 0.5 * np.sin(2 * np.pi * (u - cv * v))) / 1.5
        h = 0.5 + 0.34 * np.tanh(2 * folds) + 0.05 * fine
        tone = 0.5 + 0.03 * coarse
    elif kind == "regolith":
        h = 0.5 + 0.23 * coarse + 0.23 * fine
        tone = 0.5 + 0.28 * coarse + 0.18 * fine
    rough = np.clip(item["roughness"] + item["roughness_variation"] * (2 * h - 1),
                    0.02, 1.0)
    return np.clip(h, 0, 1), np.clip(tone, 0, 1), rough


def srgb_encode(linear):
    linear = np.asarray(linear, dtype=np.float64)
    return np.where(linear <= 0.0031308, 12.92 * linear,
                    1.055 * np.power(linear, 1 / 2.4) - 0.055)


def normal_from_height(height_mm, tile_mm):
    rows, cols = height_mm.shape
    du = (np.roll(height_mm, -1, axis=1) - np.roll(height_mm, 1, axis=1)) / (2 * tile_mm[0] / cols)
    # Image rows descend; +V is towards the top of the image.
    dv = (np.roll(height_mm, 1, axis=0) - np.roll(height_mm, -1, axis=0)) / (2 * tile_mm[1] / rows)
    normal = np.stack((-du, -dv, np.ones_like(height_mm)), axis=-1)
    return normal / np.linalg.norm(normal, axis=-1, keepdims=True)


def make_maps(item, size):
    u = (np.arange(size, dtype=np.float64)[None, :] + 0.5) / size
    v = 1 - (np.arange(size, dtype=np.float64)[:, None] + 0.5) / size
    height, tone, rough = fields(item, u, v)
    height16 = np.rint(height * 65535).astype(np.uint16)
    displacement = (height16.astype(np.float64) / 65535 - 0.5) * item["height_span_mm"]
    normal = normal_from_height(displacement, item["tile_mm"])
    low, high = np.asarray(item["base_linear_low"]), np.asarray(item["base_linear_high"])
    color = low + (high - low) * tone[..., None]
    return {"base_color": np.rint(np.clip(srgb_encode(color), 0, 1) * 255).astype(np.uint8),
            "normal_open_gl": np.rint((normal * 0.5 + 0.5) * 255).astype(np.uint8),
            "roughness": np.rint(rough * 255).astype(np.uint8),
            "metallic": np.full((size, size), round(item["metallic"] * 255), dtype=np.uint8),
            "height": height16}


def png_bytes(array):
    out = io.BytesIO()
    Image.fromarray(array).save(out, format="PNG", compress_level=9)
    return out.getvalue()


def seams(item):
    t = (np.arange(257, dtype=np.float64) + 0.5) / 257
    residual = 0.0
    for a, b in ((fields(item, 0.0, t), fields(item, 1.0, t)),
                 (fields(item, t, 0.0), fields(item, t, 1.0))):
        for x, y in zip(a, b):
            residual = max(residual, float(np.max(np.abs(x - y))))
    return residual


def build():
    data = validate_source(load_json(RECIPES))
    records = []
    for item in data["items"]:
        maps = make_maps(item, data["resolution"])
        record = {k: item[k] for k in ("id", "name", "catalog_ids", "domains", "tile_mm",
                  "height_span_mm", "limitations")}
        record.update({"resource_version": VERSION, "authoring_status": "authored",
                       "runtime_status": "unbound", "appearance_fidelity": "illustrative_approximation",
                       "seed": item["seed"], "pattern": item["pattern"],
                       "ambient_occlusion": AO,
                       "height_zero": 0.5, "normal_convention": NORMAL_CONVENTION,
                       "repeat": ["repeat", "repeat"], "sample_locations": "pixel_centers",
                       "source_boundary_max_error": seams(item),
                       "maps": {}})
        for role, array in maps.items():
            target = EXPORTS / item["id"] / f"{role}.png"
            target.parent.mkdir(parents=True, exist_ok=True)
            raw = png_bytes(array)
            target.write_bytes(raw)
            record["maps"][role] = {"path": target.relative_to(ROOT).as_posix(),
                "sha256": digest(raw), "decoded_sha256": digest(array.astype(array.dtype.newbyteorder("<")).tobytes()),
                "size_bytes": len(raw), "width": data["resolution"], "height": data["resolution"],
                "channels": 3 if array.ndim == 3 else 1, "bit_depth": ROLES[role][1],
                "encoding": ROLES[role][2], "minimum": int(array.min()), "maximum": int(array.max()),
                "consumer": "authoring_only_requires_explicit_height_adapter" if role == "height" else "bounded_texture_input_compatible_not_runtime_bound"}
        records.append(record)
    manifest = {"schema": "solidcraft-authored-surface-manifest/1", "generator_version": VERSION,
                "source_path": RECIPES.relative_to(ROOT).as_posix(),
                "source_sha256": digest(RECIPES.read_bytes()),
                "generator_sha256": digest(Path(__file__).read_bytes()),
                "license": "MIT OR Apache-2.0", "author": "Original SolidCraft contributor work",
                "source_inputs": "Original numerical fields; no external imagery, material library, or commercial-product output.",
                "items": records}
    MANIFEST.write_bytes((json.dumps(manifest, indent=2, allow_nan=False) + "\n").encode("utf-8"))
    return {"surfaces": len(records), "maps": len(records) * len(ROLES),
            "bytes": sum(m["size_bytes"] for r in records for m in r["maps"].values())}


def validate(reproduce=False):
    data = validate_source(load_json(RECIPES))
    manifest = load_json(MANIFEST)
    if not isinstance(manifest, dict) or manifest.get("schema") != "solidcraft-authored-surface-manifest/1":
        raise ValueError("unknown manifest schema")
    if manifest.get("source_sha256") != digest(RECIPES.read_bytes()):
        raise ValueError("source receipt mismatch")
    if manifest.get("generator_sha256") != digest(Path(__file__).read_bytes()):
        raise ValueError("generator receipt mismatch: rebuild exports")
    if manifest.get("generator_version") != VERSION or manifest.get("license") != "MIT OR Apache-2.0":
        raise ValueError("manifest version or license mismatch")
    if manifest.get("source_path") != RECIPES.relative_to(ROOT).as_posix():
        raise ValueError("manifest source path mismatch")
    records = manifest.get("items")
    if not isinstance(records, list) or len(records) != len(data["items"]):
        raise ValueError("manifest resource count mismatch")
    checked = 0
    for item, record in zip(data["items"], records):
        if not isinstance(record, dict):
            raise ValueError("manifest resource must be an object")
        rid = item["id"]
        for key in ("id", "name", "catalog_ids", "domains", "tile_mm", "height_span_mm", "limitations", "seed", "pattern"):
            if record.get(key) != item[key]:
                raise ValueError(f"{rid}: manifest/source mismatch {key}")
        if record.get("runtime_status") != "unbound" or record.get("authoring_status") != "authored":
            raise ValueError("unsupported runtime availability claim")
        semantics = {"resource_version": VERSION, "appearance_fidelity": "illustrative_approximation",
                     "height_zero": 0.5, "normal_convention": NORMAL_CONVENTION,
                     "repeat": ["repeat", "repeat"], "sample_locations": "pixel_centers",
                     "ambient_occlusion": AO}
        for key, value in semantics.items():
            if record.get(key) != value:
                raise ValueError(f"{rid}: changed surface semantics {key}")
        if not isinstance(record.get("maps"), dict) or set(record["maps"]) != set(ROLES):
            raise ValueError(f"{rid}: missing or extra map")
        seam = seams(item)
        claimed_seam = record.get("source_boundary_max_error")
        number(claimed_seam, 0, 1e-10, "source boundary error")
        if abs(claimed_seam - seam) > 1e-12:
            raise ValueError(f"{rid}: boundary evidence mismatch")
        if seam > 1e-10:
            raise ValueError(f"{rid}: discontinuous source boundary")
        expected = make_maps(item, data["resolution"]) if reproduce else None
        decoded = {}
        for role, spec in record["maps"].items():
            if not isinstance(spec, dict):
                raise ValueError("map metadata must be an object")
            path = EXPORTS / rid / f"{role}.png"
            if spec.get("path") != path.relative_to(ROOT).as_posix():
                raise ValueError(f"{rid}: unexpected map path")
            raw = path.read_bytes()
            if len(raw) > 16 * 1024 * 1024 or digest(raw) != spec["sha256"] or len(raw) != spec["size_bytes"]:
                raise ValueError(f"{rid}: corrupt {role}")
            with Image.open(io.BytesIO(raw)) as image:
                if image.format != "PNG" or image.size != (data["resolution"],) * 2:
                    raise ValueError("invalid PNG shape")
                if any(x in image.info for x in ("icc_profile", "exif", "gamma")):
                    raise ValueError("unexpected input color/orientation metadata")
                array = np.array(image)
            wanted_shape = (data["resolution"],) * 2 + ((3,) if role in ("base_color", "normal_open_gl") else ())
            if array.shape != wanted_shape or array.dtype != (np.uint16 if role == "height" else np.uint8):
                raise ValueError(f"{rid}: wrong decoded type or channels")
            if spec["bit_depth"] != ROLES[role][1] or spec["encoding"] != ROLES[role][2]:
                raise ValueError("map semantics mismatch")
            actual = {"width": data["resolution"], "height": data["resolution"],
                      "channels": 3 if array.ndim == 3 else 1,
                      "minimum": int(array.min()), "maximum": int(array.max()),
                      "consumer": "authoring_only_requires_explicit_height_adapter" if role == "height" else "bounded_texture_input_compatible_not_runtime_bound"}
            if any(spec.get(key) != value for key, value in actual.items()):
                raise ValueError(f"{rid}: map dimensions, range or consumer metadata mismatch")
            if digest(array.astype(array.dtype.newbyteorder("<")).tobytes()) != spec["decoded_sha256"]:
                raise ValueError("decoded pixel receipt mismatch")
            if expected is not None and not np.array_equal(array, expected[role]):
                raise ValueError(f"{rid}: regeneration changed pixels in {role}")
            decoded[role] = array
            checked += 1
        normal = decoded["normal_open_gl"].astype(np.float64) / 255 * 2 - 1
        lengths = np.linalg.norm(normal, axis=-1)
        if float(np.max(np.abs(lengths - 1))) > 0.007 or float(normal[..., 2].min()) <= 0:
            raise ValueError(f"{rid}: invalid normal vectors")
        h = (decoded["height"].astype(np.float64) / 65535 - 0.5) * item["height_span_mm"]
        computed = np.rint((normal_from_height(h, item["tile_mm"]) * 0.5 + 0.5) * 255).astype(np.uint8)
        if not np.array_equal(computed, decoded["normal_open_gl"]):
            raise ValueError(f"{rid}: normal/height physical-scale disagreement")
    return {"status": "pass", "surfaces": len(records), "maps_checked": checked,
            "pixel_reproduction": reproduce, "runtime_ready": False}


def preview():
    data = validate_source(load_json(RECIPES))
    tile, gap, label, columns = 160, 12, 44, 6
    rows = math.ceil(len(data["items"]) / columns)
    out = Image.new("RGB", (columns * (tile + gap) + gap, rows * (tile + label + gap) + 72), (19, 24, 32))
    draw = ImageDraw.Draw(out)
    draw.text((12, 12), "SolidCraft | Authored surface data, v1", fill=(237, 240, 245))
    draw.text((12, 32), "Unlit base-color maps. No scene render or measured material claim.", fill=(171, 184, 201))
    for index, item in enumerate(data["items"]):
        x = gap + (index % columns) * (tile + gap)
        y = 60 + (index // columns) * (tile + label + gap)
        with Image.open(EXPORTS / item["id"] / "base_color.png") as image:
            out.paste(image.resize((tile, tile)), (x, y))
        draw.text((x, y + tile + 5), item["id"] + "  " + item["name"][:21], fill=(228, 234, 240))
        draw.text((x, y + tile + 21), str(item["tile_mm"]) + " mm", fill=(153, 168, 188))
    target = ROOT / "target/render-starter-review/materials/base-color-contact-sheet.png"
    target.parent.mkdir(parents=True, exist_ok=True)
    out.save(target)
    return {"preview": str(target)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("build", "validate", "preview"))
    parser.add_argument("--reproduce", action="store_true")
    args = parser.parse_args()
    try:
        result = build() if args.command == "build" else validate(args.reproduce) if args.command == "validate" else preview()
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"material authoring failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2, allow_nan=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())

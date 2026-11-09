#!/usr/bin/env python3
"""Author and check original analytic, relative-radiance RGBE environment inputs.

Development tool only. JSON recipes are the editable authority. No renderer,
third-party imagery, measured photometry or application integration is implied.
Python 3.11+, NumPy and Pillow; ffmpeg is optional independent decode evidence.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import math
from pathlib import Path
import re
import shutil
import subprocess
import sys

import numpy as np
from PIL import Image, ImageDraw


ROOT = Path(__file__).resolve().parents[1]
REPO = ROOT.parents[2]
SOURCE = ROOT / "environments/source/recipes.json"
EXPORTS = ROOT / "environments/exports"
MANIFEST = ROOT / "environments/manifest.json"
REVIEW = REPO / "target/render-starter-review/environments"
SCHEMA = "solidcraft.render-environment-recipes/1"
VERSION = "1.0.0"
PRIMARIES = "0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290"
MAX_SOURCE = 1024 * 1024
MAX_ENCODED = 16 * 1024 * 1024
MAX_VALUE = 1000000.0


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def json_bytes(value: object) -> bytes:
    return (json.dumps(value, indent=2, allow_nan=False) + "\n").encode()


def unique_object(pairs: list[tuple[str, object]]) -> dict:
    result: dict = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON field: {key}")
        result[key] = value
    return result


def number(value: object, low: float, high: float, name: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError(f"{name}: expected a number")
    # Compare arbitrary-size JSON integers before converting inside isfinite().
    # Out-of-range integers can exceed Python's finite-float conversion range.
    if not low <= value <= high or not math.isfinite(value):
        raise ValueError(f"{name}: expected a finite value in [{low}, {high}]")
    return float(value)


def keys(value: object, expected: set[str], name: str) -> None:
    if not isinstance(value, dict) or set(value) != expected:
        raise ValueError(f"{name}: expected fields {sorted(expected)}")


def vector(value: object, low: float, high: float, name: str) -> None:
    if not isinstance(value, list) or len(value) != 3:
        raise ValueError(f"{name}: expected three channels")
    for channel in value:
        number(channel, low, high, name)


def read_recipes(path: Path = SOURCE) -> tuple[dict, bytes]:
    if path.stat().st_size > MAX_SOURCE:
        raise ValueError("recipe source exceeds 1 MiB")
    raw = path.read_bytes()
    doc = json.loads(raw, object_pairs_hook=unique_object,
                     parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)))
    keys(doc, {"schema", "version", "resolution", "authoring", "projection", "items"}, "source")
    if doc["schema"] != SCHEMA or doc["version"] != VERSION:
        raise ValueError("unsupported environment recipe schema or version")
    if doc["resolution"] != [1024, 512]:
        raise ValueError("this resource revision requires 1024 by 512 exports")
    keys(doc["authoring"], {"method", "license", "author", "date", "randomness", "radiance_units",
                            "measurement_status", "runtime_status", "sources", "reference_use"}, "authoring")
    for name, required in {"method": "original_analytic_procedural", "license": "MIT OR Apache-2.0",
                           "radiance_units": "authored_relative_units", "measurement_status": "not_measured",
                           "runtime_status": "authored_unbound"}.items():
        if doc["authoring"][name] != required:
            raise ValueError(f"authoring.{name}: unsupported declaration")
    projection = doc["projection"]
    keys(projection, {"type", "sample_location", "up_axis", "u_axis", "equations", "cardinal_directions",
                      "wrapping", "engine_binding"}, "projection")
    for name, required in {"type": "equirectangular", "sample_location": "pixel_centers", "up_axis": "+Z",
                           "u_axis": "+azimuth from +X toward +Y", "wrapping": "repeat U, clamp V",
                           "equations": "phi = 2*pi*(u-0.5); theta = pi*v; direction = (sin(theta)*cos(phi), sin(theta)*sin(phi), cos(theta))"}.items():
        if projection[name] != required:
            raise ValueError(f"projection.{name}: does not match the authoring convention")
    if projection["cardinal_directions"] != {"u=0.0,v=0.5": "-X; repeat seam", "u=0.25,v=0.5": "-Y",
                                              "u=0.5,v=0.5": "+X", "u=0.75,v=0.5": "+Y",
                                              "v=0.0": "+Z", "v=1.0": "-Z"}:
        raise ValueError("projection cardinal directions do not match the generator")
    if not isinstance(doc["items"], list) or not 1 <= len(doc["items"]) <= 64:
        raise ValueError("expected 1 to 64 environment recipes")
    expected = {"id", "slug", "name", "catalog_id", "concept_relation", "domains",
                "purpose", "ambient", "panels", "solar_disk", "energy_policy",
                "limitations", "seed"}
    seen_ids: set[str] = set()
    seen_slugs: set[str] = set()
    catalog = {item["id"] for item in json.loads((REPO / "docs/render-materials/environments.json").read_text())["items"]}
    for item in doc["items"]:
        keys(item, expected, "environment")
        if not isinstance(item["id"], str) or not re.fullmatch(r"HDR-[0-9]{3}", item["id"]) or item["id"] in seen_ids:
            raise ValueError("invalid or duplicate environment id")
        if not isinstance(item["slug"], str) or not re.fullmatch(r"[a-z][a-z0-9-]{2,63}", item["slug"]) or item["slug"] in seen_slugs:
            raise ValueError("invalid or duplicate environment slug")
        seen_ids.add(item["id"])
        seen_slugs.add(item["slug"])
        if item["catalog_id"] not in catalog or item["concept_relation"] != "illumination_variant_only":
            raise ValueError("invalid environment concept reference or relation")
        for field in ("name", "purpose", "energy_policy"):
            if not isinstance(item[field], str) or not item[field].strip() or len(item[field]) > 2000:
                raise ValueError(f"{item['id']}: invalid {field}")
        for field in ("domains", "limitations"):
            if not isinstance(item[field], list) or not item[field] or len(item[field]) > 20 or any(not isinstance(x, str) or not x.strip() for x in item[field]):
                raise ValueError(f"{item['id']}: invalid {field}")
        if item["solar_disk"] is not False or item["seed"] != 0 or isinstance(item["seed"], bool):
            raise ValueError("this deterministic analytic pack has no Sun disk or random seed")
        keys(item["ambient"], {"upper_linear_srgb", "lower_linear_srgb", "transition"}, "ambient")
        vector(item["ambient"]["upper_linear_srgb"], 0, 16, "upper ambient")
        vector(item["ambient"]["lower_linear_srgb"], 0, 16, "lower ambient")
        number(item["ambient"]["transition"], 0.05, 1, "ambient transition")
        if not isinstance(item["panels"], list) or len(item["panels"]) > 16:
            raise ValueError("at most 16 analytic directional patches are allowed")
        for panel in item["panels"]:
            keys(panel, {"azimuth_deg", "elevation_deg", "width_deg", "height_deg", "roll_deg", "edge_fraction", "radiance_linear_srgb"}, "panel")
            number(panel["azimuth_deg"], -180, 180, "panel azimuth")
            number(panel["elevation_deg"], -90, 90, "panel elevation")
            number(panel["width_deg"], 4, 150, "panel width")
            number(panel["height_deg"], 4, 150, "panel height")
            number(panel["roll_deg"], -180, 180, "panel roll")
            number(panel["edge_fraction"], 0.05, 0.5, "panel edge")
            vector(panel["radiance_linear_srgb"], 0, 4096, "panel radiance")
        if item["id"] == "HDR-024" and (item["panels"] or any(item["ambient"]["upper_linear_srgb"]) or any(item["ambient"]["lower_linear_srgb"])):
            raise ValueError("HDR-024 vacuum must remain exactly black")
    return doc, raw


def directions(width: int, height: int) -> np.ndarray:
    """Pixel centers: u=0.5 is +X, u=0.75 +Y; v=0 is +Z."""
    if isinstance(width, bool) or isinstance(height, bool) or not isinstance(width, int) or not isinstance(height, int) or width != 2 * height or not 8 <= width <= 2048:
        raise ValueError("direction grid requires bounded 2:1 integer dimensions")
    phi = 2 * np.pi * ((np.arange(width, dtype=np.float64) + 0.5) / width - 0.5)
    theta = np.pi * (np.arange(height, dtype=np.float64) + 0.5) / height
    return np.stack(np.broadcast_arrays(np.sin(theta)[:, None] * np.cos(phi)[None, :],
                                        np.sin(theta)[:, None] * np.sin(phi)[None, :],
                                        np.cos(theta)[:, None]), axis=-1)


def smoothstep(value: np.ndarray) -> np.ndarray:
    value = np.clip(value, 0, 1)
    return value * value * (3 - 2 * value)


def evaluate(item: dict, direction: np.ndarray) -> np.ndarray:
    """Original spherical lighting design, not an inferred physical room."""
    ambient = item["ambient"]
    t = smoothstep((direction[..., 2] / ambient["transition"] + 1) / 2)
    upper = np.asarray(ambient["upper_linear_srgb"], dtype=np.float64)
    lower = np.asarray(ambient["lower_linear_srgb"], dtype=np.float64)
    result = lower + t[..., None] * (upper - lower)
    for panel in item["panels"]:
        az, el, roll = np.deg2rad([panel["azimuth_deg"], panel["elevation_deg"], panel["roll_deg"]])
        forward = np.array([math.cos(el) * math.cos(az), math.cos(el) * math.sin(az), math.sin(el)])
        right = np.array([-math.sin(az), math.cos(az), 0.0])
        up = np.cross(forward, right)
        rotated_right = right * math.cos(roll) + up * math.sin(roll)
        rotated_up = -right * math.sin(roll) + up * math.cos(roll)
        facing = direction @ forward
        safe_facing = np.maximum(facing, 1e-12)
        x = np.arctan2(direction @ rotated_right, safe_facing)
        y = np.arctan2(direction @ rotated_up, safe_facing)
        half_width, half_height = np.deg2rad([panel["width_deg"], panel["height_deg"]]) / 2
        edge = panel["edge_fraction"]
        # A separable C1 edge avoids pixel-dependent hard source boundaries.
        wx = smoothstep((1 - np.abs(x) / half_width) / edge)
        wy = smoothstep((1 - np.abs(y) / half_height) / edge)
        mask = wx * wy * (facing > 0)
        result += mask[..., None] * np.asarray(panel["radiance_linear_srgb"])
    return np.round(result, 8)


def rgbe_pixels(rgb: np.ndarray) -> np.ndarray:
    if rgb.ndim != 3 or rgb.shape[2] != 3 or not np.isfinite(rgb).all() or (rgb < 0).any() or (rgb > MAX_VALUE).any():
        raise ValueError("invalid bounded linear radiance array")
    largest = rgb.max(axis=-1)
    mantissa, exponent = np.frexp(largest)
    scale = np.zeros_like(largest)
    nonzero = largest >= 1e-32
    scale[nonzero] = mantissa[nonzero] * 256 / largest[nonzero]
    encoded = np.zeros((*rgb.shape[:2], 4), dtype=np.uint8)
    encoded[..., :3] = np.floor(rgb * scale[..., None]).astype(np.uint8)
    encoded[..., 3][nonzero] = (exponent[nonzero] + 128).astype(np.uint8)
    return encoded


def encode_channel(channel: np.ndarray) -> bytes:
    data = channel.tobytes()
    output = bytearray()
    i = 0
    while i < len(data):
        run = 1
        while run < 127 and i + run < len(data) and data[i + run] == data[i]:
            run += 1
        if run >= 4:
            output.extend((128 + run, data[i]))
            i += run
            continue
        start = i
        i += run
        while i < len(data) and i - start < 128:
            following = 1
            while following < 4 and i + following < len(data) and data[i + following] == data[i]:
                following += 1
            if following == 4:
                break
            i += min(following, 128 - (i - start))
        output.append(i - start)
        output.extend(data[start:i])
    return bytes(output)


def encode_hdr(rgb: np.ndarray) -> bytes:
    height, width, _ = rgb.shape
    if width != height * 2 or not 8 <= width <= 2048 or height > 1024:
        raise ValueError("unsupported HDR dimensions")
    header = (f"#?RADIANCE\n# Original SolidCraft analytic relative-radiance environment\n"
              f"FORMAT=32-bit_rle_rgbe\nEXPOSURE=1\nCOLORCORR=1 1 1\n"
              f"PRIMARIES={PRIMARIES}\nPIXASPECT=1\n\n-Y {height} +X {width}\n").encode("ascii")
    pixels = rgbe_pixels(rgb)
    output = bytearray(header)
    for row in pixels:
        output.extend((2, 2, width >> 8, width & 255))
        for channel in range(4):
            output.extend(encode_channel(row[:, channel]))
    if len(output) > MAX_ENCODED:
        raise ValueError("encoded HDR exceeds input admission limit")
    return bytes(output)


def decode_hdr(data: bytes) -> np.ndarray:
    """Strict local stream check. Independent ffmpeg comparison is separate."""
    if len(data) > MAX_ENCODED:
        raise ValueError("HDR exceeds 16 MiB")
    stream = io.BytesIO(data)
    header = bytearray()
    while True:
        line = stream.readline(65537)
        if not line or len(header) + len(line) > 65536:
            raise ValueError("invalid HDR header")
        header.extend(line)
        if line == b"\n":
            break
    expected = (b"#?RADIANCE", b"FORMAT=32-bit_rle_rgbe", b"EXPOSURE=1", b"COLORCORR=1 1 1",
                f"PRIMARIES={PRIMARIES}".encode(), b"PIXASPECT=1")
    lines = bytes(header).splitlines()
    if any(lines.count(line) != 1 for line in expected):
        raise ValueError("missing or duplicate canonical HDR header field")
    if any(line and not line.startswith(b"#") and line not in expected for line in lines):
        raise ValueError("unsupported HDR header field or correction")
    match = re.fullmatch(rb"-Y ([0-9]+) \+X ([0-9]+)\n", stream.readline(100))
    if not match:
        raise ValueError("HDR orientation must be -Y +X")
    height, width = map(int, match.groups())
    if width != height * 2 or not 8 <= width <= 2048 or height > 1024:
        raise ValueError("invalid HDR dimensions")
    pixels = np.empty((height, width, 4), dtype=np.uint8)
    for y in range(height):
        if stream.read(4) != bytes((2, 2, width >> 8, width & 255)):
            raise ValueError("invalid HDR scanline marker")
        for component in range(4):
            x = 0
            while x < width:
                code = stream.read(1)
                if not code or code[0] == 0:
                    raise ValueError("truncated or empty HDR packet")
                count = code[0] - 128 if code[0] > 128 else code[0]
                if x + count > width:
                    raise ValueError("HDR run exceeds scanline")
                chunk = stream.read(1 if code[0] > 128 else count)
                if len(chunk) != (1 if code[0] > 128 else count):
                    raise ValueError("truncated HDR packet")
                pixels[y, x:x + count, component] = chunk[0] if code[0] > 128 else np.frombuffer(chunk, dtype=np.uint8)
                x += count
    if stream.read(1):
        raise ValueError("unexpected trailing HDR data")
    exponent = pixels[..., 3].astype(np.int32)
    scales = np.where(exponent == 0, 0.0, np.exp2(exponent - 136))
    rgb = pixels[..., :3].astype(np.float64) * scales[..., None]
    if not np.isfinite(rgb).all() or (rgb > MAX_VALUE).any():
        raise ValueError("decoded radiance exceeds authored range")
    return rgb


def metrics(rgb: np.ndarray) -> dict:
    height, width, _ = rgb.shape
    # Exact latitude cell solid angles; their full-sphere sum is 4*pi.
    edges = np.arange(height + 1, dtype=np.float64) * np.pi / height
    weights = (np.cos(edges[:-1]) - np.cos(edges[1:]))[:, None] * (2 * np.pi / width)
    mean = (rgb * weights[..., None]).sum(axis=(0, 1)) / (4 * np.pi)
    d = directions(width, height)
    irradiance = {}
    for label, axis in [("+X", [1, 0, 0]), ("-X", [-1, 0, 0]), ("+Y", [0, 1, 0]),
                        ("-Y", [0, -1, 0]), ("+Z", [0, 0, 1]), ("-Z", [0, 0, -1])]:
        value = (rgb * (np.maximum(d @ axis, 0) * weights)[..., None]).sum(axis=(0, 1))
        irradiance[label] = np.round(value, 8).tolist()
    return {"minimum_channel": float(rgb.min()), "maximum_channel": float(rgb.max()),
            "solid_angle_weighted_mean_rgb": np.round(mean, 8).tolist(),
            "hemisphere_cosine_integrals_rgb": irradiance,
            "integral_units": "relative_radiance_times_steradians; not lux or watts",
            "integral_method": "pixel-center cosine with exact cell solid angle"}


def item_record(item: dict, encoded: bytes, source_sha: str, generator_sha: str) -> dict:
    rgb = decode_hdr(encoded)
    return {"id": item["id"], "name": item["name"], "catalog_id": item["catalog_id"],
            "concept_relation": item["concept_relation"], "version": VERSION,
            "status": "authored_unbound", "content_available": True, "runtime_ready": False,
            "path": f"exports/{item['slug']}.hdr", "sha256": digest(encoded), "bytes": len(encoded),
            "dimensions": [rgb.shape[1], rgb.shape[0]], "role": "environment_radiance",
            "color_space": "linear_srgb", "transfer_function": "linear", "format": "Radiance RGBE RLE",
            "header_orientation": "-Y +X", "primaries_xy": [float(x) for x in PRIMARIES.split()],
            "exposure": 1.0, "color_correction": [1.0, 1.0, 1.0],
            "radiance_units": "authored_relative_units", "solar_disk": False,
            "energy_owner": "environment_map" if item["id"] != "HDR-024" else "none",
            "seed": 0, "source_sha256": source_sha, "generator_sha256": generator_sha,
            "statistics": metrics(rgb), "limitations": item["limitations"]}


def make_manifest(doc: dict, raw: bytes) -> dict:
    generator_sha = digest(Path(__file__).read_bytes())
    source_sha = digest(raw)
    rows = []
    for item in doc["items"]:
        path = EXPORTS / f"{item['slug']}.hdr"
        if path.stat().st_size > MAX_ENCODED:
            raise ValueError(f"{path.name}: exceeds HDR byte limit")
        rows.append(item_record(item, path.read_bytes(), source_sha, generator_sha))
    return {"schema": "solidcraft.render-environment-manifest/1", "version": VERSION,
            "status": "authored_unbound", "runtime_ready": False,
            "recipe_source": "source/recipes.json", "recipe_sha256": source_sha,
            "generator": "../tools/environments.py", "generator_sha256": generator_sha,
            "generator_version": VERSION, "license": "MIT OR Apache-2.0",
            "projection": doc["projection"], "items": rows}


def build() -> dict:
    doc, raw = read_recipes()
    EXPORTS.mkdir(parents=True, exist_ok=True)
    grid = directions(*doc["resolution"])
    for item in doc["items"]:
        rgb = evaluate(item, grid)
        (EXPORTS / f"{item['slug']}.hdr").write_bytes(encode_hdr(rgb))
    manifest = make_manifest(doc, raw)
    MANIFEST.write_bytes(json_bytes(manifest))
    return {"result": "PASS", "environments": len(manifest["items"]),
            "encoded_bytes": sum(item["bytes"] for item in manifest["items"]), "runtime_ready": False}


def independent_decode(path: Path, shape: tuple[int, ...]) -> np.ndarray:
    binary = shutil.which("ffmpeg")
    if binary is None:
        raise ValueError("ffmpeg is unavailable for independent RGBE decoding")
    proc = subprocess.run([binary, "-v", "error", "-threads", "1", "-i", str(path),
                           "-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "gbrpf32le", "pipe:1"],
                          check=True, capture_output=True, timeout=30)
    height, width, _ = shape
    if len(proc.stdout) != width * height * 12:
        raise ValueError("unexpected independent decoder output size")
    planes = np.frombuffer(proc.stdout, dtype="<f4").reshape(3, height, width)
    return np.stack([planes[2], planes[0], planes[1]], axis=-1).astype(np.float64)


def validate(external: bool = False, reproducible: bool = False) -> dict:
    doc, raw = read_recipes()
    manifest = json.loads(MANIFEST.read_bytes(), object_pairs_hook=unique_object)
    expected = make_manifest(doc, raw)
    if manifest != expected:
        raise ValueError("manifest is stale; review inputs and regenerate exports")
    expected_paths = {f"{item['slug']}.hdr" for item in doc["items"]}
    if {path.name for path in EXPORTS.iterdir()} != expected_paths:
        raise ValueError("unexpected or missing export file")
    grid = directions(*doc["resolution"])
    max_error = 0.0
    for item in doc["items"]:
        path = EXPORTS / f"{item['slug']}.hdr"
        encoded = path.read_bytes()
        decoded = decode_hdr(encoded)
        authored = evaluate(item, grid)
        # RGBE truncation error is at most 1/128 of the pixel's largest channel.
        tolerance = np.maximum(authored.max(axis=-1) / 128, 1e-30)
        error = np.abs(decoded - authored)
        if (error.max(axis=-1) > tolerance + 1e-12).any():
            raise ValueError(f"{item['id']}: RGBE quantization bound exceeded")
        max_error = max(max_error, float(error.max()))
        if reproducible and encode_hdr(authored) != encoded:
            raise ValueError(f"{item['id']}: export is not reproducible")
        if external:
            independent = independent_decode(path, decoded.shape)
            if not np.array_equal(independent, decoded):
                raise ValueError(f"{item['id']}: independent decoder disagrees")
        # Mathematical seam agreement, not the invalid assertion that neighboring
        # pixel-center samples on opposite sides of a seam must be identical.
        theta = (np.arange(513) / 512) * np.pi
        seam = np.stack([-np.sin(theta), np.zeros_like(theta), np.cos(theta)], axis=-1)
        other = seam.copy()
        other[:, 1] = -0.0
        if not np.array_equal(evaluate(item, seam), evaluate(item, other)):
            raise ValueError(f"{item['id']}: discontinuous longitude seam")
        if item["id"] == "HDR-024" and np.any(decoded):
            raise ValueError("vacuum export contains unwanted energy")
    return {"result": "PASS", "environments": len(doc["items"]), "manifest_and_streams": True,
            "source_regeneration": reproducible, "independent_ffmpeg_decode": external,
            "max_absolute_rgbe_error": max_error, "runtime_ready": False}


def preview() -> dict:
    doc, _ = read_recipes()
    REVIEW.mkdir(parents=True, exist_ok=True)
    width, height, columns = 320, 210, 4
    sheet = Image.new("RGB", (width * columns, height * math.ceil(len(doc["items"]) / columns)), (22, 27, 34))
    draw = ImageDraw.Draw(sheet)
    for index, item in enumerate(doc["items"]):
        rgb = decode_hdr((EXPORTS / f"{item['slug']}.hdr").read_bytes())
        # Fixed diagnostic exposure and curve, deliberately separate from exports.
        mapped = rgb / (1 + rgb)
        srgb = np.where(mapped <= 0.0031308, 12.92 * mapped, 1.055 * np.power(mapped, 1 / 2.4) - 0.055)
        image = Image.fromarray(np.round(srgb * 255).astype(np.uint8)).resize((312, 156), Image.Resampling.LANCZOS)
        x, y = (index % columns) * width + 4, (index // columns) * height + 4
        sheet.paste(image, (x, y))
        draw.text((x + 4, y + 160), item["id"] + " | " + item["name"], fill=(230, 235, 240))
        draw.text((x + 4, y + 176), "Synthetic HDR; fixed Reinhard preview", fill=(149, 169, 185))
    path = REVIEW / "environment-contact-sheet.png"
    sheet.save(path)
    return {"result": "PASS", "preview": str(path), "application_render": False}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["build", "validate", "preview"])
    parser.add_argument("--independent-decode", action="store_true", help="verify pixels with installed ffmpeg")
    parser.add_argument("--reproducible", action="store_true", help="regenerate each HDR in memory and compare bytes")
    args = parser.parse_args()
    try:
        result = build() if args.action == "build" else preview() if args.action == "preview" else validate(args.independent_decode, args.reproducible)
    except (OSError, ValueError, TypeError, KeyError, subprocess.SubprocessError) as exc:
        print(json.dumps({"result": "FAIL", "error": str(exc)}), file=sys.stderr)
        return 1
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

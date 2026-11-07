"""Compare equal-sample native SolidCraft raw EXRs and estimate seed-to-seed noise."""

import argparse
from collections import defaultdict
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import stat

import numpy as np
import OpenEXR


CHANNELS = ("R", "G", "B", "A")
SOFTWARE = {"cpu": "SolidCraft native CPU raytrace", "gpu": "SolidCraft native GPU path"}
CHROMATICITIES = (0.64, 0.33, 0.30, 0.60, 0.15, 0.06, 0.3127, 0.3290)
LUMINANCE_WEIGHTS = (0.2126, 0.7152, 0.0722)
ROW_BLOCK = 64
PERCENTILES = (50, 95, 99, 99.9)
PERCENTILE_LABELS = ("p50", "p95", "p99", "p99_9")


@dataclass(frozen=True)
class Master:
    backend: str
    seed: int
    path: Path
    identity: tuple[int, ...]


@dataclass(frozen=True)
class Region:
    name: str
    x: int
    y: int
    width: int
    height: int

    def crop(self, image):
        return image[..., self.y:self.y + self.height, self.x:self.x + self.width]


def luminance(rgba):
    return np.einsum("c,cij->ij", LUMINANCE_WEIGHTS, rgba[:3])


def percentiles(values):
    # Select all percentiles together using one packed copy of the region.
    quantiles = np.percentile(values.flatten(), PERCENTILES, overwrite_input=True)
    return dict(zip(PERCENTILE_LABELS, quantiles.tolist()))


def rms(values):
    return float(np.sqrt(np.einsum("ij,ij->", values, values) / values.size))


def canvas(master):
    with OpenEXR.File(str(master.path), header_only=True) as image:
        if len(image.parts) != 1:
            raise ValueError(f"{master.path}: expected one EXR part")
        header = image.header()
        lower, upper = map(tuple, header["dataWindow"])
        width, height = (int(value) + 1 for value in upper)
        if (lower != (0, 0) or tuple(map(tuple, header["displayWindow"])) != (lower, upper)
                or not (1 <= width <= 8192 and 1 <= height <= 8192 and width * height <= 16_777_216)):
            raise ValueError(f"{master.path}: expected a bounded zero-origin full canvas")
        if (header.get("compression") != OpenEXR.NO_COMPRESSION
                or header.get("lineOrder") != OpenEXR.INCREASING_Y
                or header.get("type", OpenEXR.scanlineimage) != OpenEXR.scanlineimage
                or header.get("pixelAspectRatio") != 1):
            raise ValueError(f"{master.path}: expected uncompressed native scanlines and square pixels")
        chromaticities = np.asarray(header.get("chromaticities", ())).reshape(-1)
        comments = header.get("comments", "")
        if (chromaticities.size != len(CHROMATICITIES)
                or not np.allclose(chromaticities, CHROMATICITIES, rtol=0, atol=1e-7)
                or "coverage-premultiplied" not in comments or "before exposure" not in comments):
            raise ValueError(f"{master.path}: expected raw linear sRGB/D65 with premultiplied coverage")
        if header.get("software") != SOFTWARE[master.backend]:
            raise ValueError(f"{master.path}: header does not match the declared {master.backend} backend")
    return width, height


def accumulate(master, mean, m2, count):
    _, height, width = mean.shape
    with OpenEXR.File(str(master.path), separate_channels=True) as image:
        channels = image.channels()
        if set(channels) != set(CHANNELS):
            raise ValueError(f"{master.path}: expected exactly R, G, B and A channels")
        pixels = [channels[name].pixels for name in CHANNELS]
        if (any(channel.xSampling != 1 or channel.ySampling != 1 for channel in channels.values())
                or any(channel.dtype != np.float32 or channel.shape != (height, width) for channel in pixels)):
            raise ValueError(f"{master.path}: expected full-resolution FLOAT32 channels")

        for first in range(0, height, ROW_BLOCK):
            rows = slice(first, first + ROW_BLOCK)
            sample = np.stack([channel[rows] for channel in pixels], dtype=np.float64)
            rgb, alpha = sample[:3], sample[3]
            if (not np.isfinite(sample).all() or (sample < 0).any() or (alpha > 1).any()
                    or ((alpha == 0) & (rgb != 0)).any()):
                raise ValueError(f"{master.path}: invalid raw radiance or coverage near row {first}")

            # Welford's central moment for luminance. Update RGB and coverage
            # means together so comparison stays in the same raw color space.
            current = mean[:, rows]
            delta = sample - current
            current += delta / count
            if m2 is not None and count > 1:
                m2[rows] += luminance(delta) ** 2 * ((count - 1) / count)


def file_identity(info):
    return info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns


def summarize(values, m2, count):
    light = luminance(values)
    alpha = values[3]
    light_rms = rms(light)
    variance = None if m2 is None else float(m2.mean() / (count - 1))
    noise = None if variance is None else float(np.sqrt(variance))
    return {
        "mean_rgb": values[:3].mean(axis=(1, 2)).tolist(),
        "luminance": {
            "mean": float(light.mean()), "rms": light_rms, **percentiles(light),
            "max": float(light.max()), "fraction_above_one": float(np.mean(light > 1)),
        },
        "mean_coverage": float(alpha.mean()),
        "nonzero_coverage_fraction": float(np.mean(alpha > 0)),
        "noise": None if noise is None else {
            "single_render_luminance_rms": noise,
            "single_render_luminance_rms_relative_to_signal_rms": noise / light_rms if light_rms else None,
            "mean_luminance_image_standard_error_rms": noise / float(np.sqrt(count)),
        },
    }


def summarize_backend(masters, regions, width, height):
    mean = np.zeros((len(CHANNELS), height, width), dtype=np.float64)
    m2 = np.zeros((height, width), dtype=np.float64) if len(masters) > 1 else None
    sources = []
    digests = set()
    for count, master in enumerate(masters, 1):
        with master.path.open("rb") as source:
            if file_identity(os.fstat(source.fileno())) != master.identity:
                raise ValueError(f"{master.path}: file changed before reading")
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        if digest in digests:
            raise ValueError(f"{master.path}: repeated EXR payload within one backend/sample group")
        digests.add(digest)
        accumulate(master, mean, m2, count)
        if file_identity(master.path.stat()) != master.identity:
            raise ValueError(f"{master.path}: file changed while reading")
        sources.append({"path": str(master.path), "seed": master.seed, "sha256": digest})

    summaries = {
        region.name: summarize(region.crop(mean), None if m2 is None else region.crop(m2), len(masters))
        for region in regions
    }
    return mean, {"sequences": sources, "regions": summaries}


def compare(control, candidate, signal_scale, *, delta_label="gpu_minus_cpu", scale_label="average_backend_rms"):
    rgb_delta = []
    rgb_rmse = []
    light = np.zeros(control.shape[1:], dtype=np.float64)
    delta = np.empty_like(light)
    for index, weight in enumerate(LUMINANCE_WEIGHTS):
        np.subtract(candidate[index], control[index], out=delta)
        rgb_delta.append(float(delta.mean()))
        rgb_rmse.append(rms(delta))
        delta *= weight
        light += delta
    error = rms(light)
    luminance_bias = float(light.mean())
    np.abs(light, out=light)
    np.subtract(candidate[3], control[3], out=delta)
    np.abs(delta, out=delta)
    return {
        f"mean_rgb_delta_{delta_label}": rgb_delta,
        "rgb_rmse_per_channel": rgb_rmse,
        f"luminance_mean_delta_{delta_label}": luminance_bias,
        "luminance_rmse": error,
        f"luminance_rmse_relative_to_{scale_label}": error / signal_scale if signal_scale else None,
        "absolute_luminance_delta": {**percentiles(light), "max": float(light.max())},
        "mean_absolute_coverage_delta": float(delta.mean()),
        "max_absolute_coverage_delta": float(delta.max()),
    }


def parse_inputs(image_args, region_args):
    groups = defaultdict(lambda: defaultdict(list))
    used_paths = set()
    seeds = set()
    for backend, spp_text, seed_text, filename in image_args:
        backend = backend.lower()
        spp, seed = int(spp_text), int(seed_text)
        if backend not in SOFTWARE or not 1 <= spp <= 65_536 or not 0 <= seed <= 0xFFFFFFFF:
            raise ValueError("--image requires cpu|gpu, SPP in 1..65536 and an unsigned 32-bit seed")
        path = Path(filename).resolve(strict=True)
        info = path.stat()
        if not stat.S_ISREG(info.st_mode):
            raise ValueError(f"{path}: expected a regular file")
        if path in used_paths:
            raise ValueError(f"{path}: each input EXR must be distinct")
        used_paths.add(path)
        key = (backend, spp, seed)
        if key in seeds:
            raise ValueError(f"{backend} at {spp} SPP: repeated seed {seed}")
        seeds.add(key)
        groups[spp][backend].append(Master(backend, seed, path, file_identity(info)))

    canvas_size = None
    for group in groups.values():
        for masters in group.values():
            masters.sort(key=lambda master: master.seed)
            for master in masters:
                size = canvas(master)
                if canvas_size is not None and canvas_size != size:
                    raise ValueError(f"{master.path}: all EXRs must have matching canvas dimensions")
                canvas_size = size

    width, height = canvas_size
    regions = [Region("full", 0, 0, width, height)]
    names = {"full"}
    for name, *bounds in region_args:
        region = Region(name, *(int(value) for value in bounds))
        if not name or name in names:
            raise ValueError(f"region name must be nonempty and unique: {name!r}")
        if (region.x < 0 or region.y < 0 or region.width < 1 or region.height < 1
                or region.x + region.width > width or region.y + region.height > height):
            raise ValueError(f"region {name!r} falls outside the {width} × {height} canvas")
        names.add(name)
        regions.append(region)
    return groups, regions, width, height


def report(image_args, region_args):
    groups, regions, width, height = parse_inputs(image_args, region_args)
    results = []
    for spp, group in sorted(groups.items()):
        means, backends = {}, {}
        for backend, masters in sorted(group.items()):
            means[backend], backends[backend] = summarize_backend(masters, regions, width, height)
        comparison = None
        if len(means) == 2:
            comparison = {}
            for region in regions:
                signal_scale = sum(b["regions"][region.name]["luminance"]["rms"] for b in backends.values()) / 2
                comparison[region.name] = compare(region.crop(means["cpu"]), region.crop(means["gpu"]), signal_scale)
        results.append({"samples_per_pixel": spp, "backends": backends, "gpu_vs_cpu": comparison})
        means.clear()
    return {
        "schema_version": 1,
        "canvas": {"width": width, "height": height},
        "regions": {region.name: [region.x, region.y, region.width, region.height] for region in regions},
        "reader": {"OpenEXR": OpenEXR.__version__, "NumPy": np.__version__},
        "metadata": "Sample counts and seeds are caller supplied. Backend comes from the native EXR header.",
        "sample_groups": results,
    }


def paired_report(pair, region_args):
    backend, spp, seed, control, candidate = pair
    control_groups, regions, width, height = parse_inputs([(backend, spp, seed, control)], region_args)
    candidate_groups, _, other_width, other_height = parse_inputs([(backend, spp, seed, candidate)], [])
    if (width, height) != (other_width, other_height):
        raise ValueError("control and candidate must have matching canvas dimensions")
    backend = backend.lower()
    control_masters = control_groups[int(spp)][backend]
    candidate_masters = candidate_groups[int(spp)][backend]
    if control_masters[0].identity == candidate_masters[0].identity:
        raise ValueError("control and candidate must be distinct EXR files")
    control_mean, control_summary = summarize_backend(control_masters, regions, width, height)
    candidate_mean, candidate_summary = summarize_backend(candidate_masters, regions, width, height)
    comparisons = {}
    for region in regions:
        scale = sum(summary["regions"][region.name]["luminance"]["rms"] for summary in (control_summary, candidate_summary)) / 2
        comparisons[region.name] = compare(
            region.crop(control_mean), region.crop(candidate_mean), scale,
            delta_label="candidate_minus_control", scale_label="average_signal_rms",
        )
    return {
        "schema_version": 1,
        "backend": backend,
        "samples_per_pixel": int(spp),
        "canvas": {"width": width, "height": height},
        "regions": {region.name: [region.x, region.y, region.width, region.height] for region in regions},
        "metadata": "Sample count and shared seed are caller supplied. Paired deltas do not estimate independent-seed noise.",
        "control": control_summary,
        "candidate": candidate_summary,
        "candidate_vs_control": comparisons,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--image", action="append", nargs=4, metavar=("BACKEND", "SPP", "SEED", "EXR"))
    mode.add_argument("--pair", nargs=5, metavar=("BACKEND", "SPP", "SEED", "CONTROL_EXR", "CANDIDATE_EXR"))
    parser.add_argument("--region", action="append", nargs=5, default=[], metavar=("NAME", "X", "Y", "WIDTH", "HEIGHT"))
    args = parser.parse_args()
    try:
        result = paired_report(args.pair, args.region) if args.pair else report(args.image, args.region)
        print(json.dumps(result, indent=2, allow_nan=False))
    except (OSError, RuntimeError, ValueError, KeyError) as error:
        parser.exit(2, f"{parser.prog}: {error}\n")


if __name__ == "__main__":
    main()

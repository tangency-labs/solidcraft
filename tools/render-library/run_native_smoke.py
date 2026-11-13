#!/usr/bin/env python3
"""Build only renderer libraries, then verify the authored map/HDR path on hardware.

No CAD engine, kernel, UI binary, release or source manifest is built/modified.
Requires an installed Vulkan hardware device and a new output directory.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import zlib

from generate import ROOT, generated, OUTPUT


def git(*args: str) -> str:
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def png_receipt(path: Path, expected: list[str]) -> dict:
    raw = path.read_bytes()
    if len(raw) > 16 * 1024 * 1024 or raw[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("Not a bounded PNG")
    pos = 8
    compressed = []
    kinds = []
    width = height = None
    while pos < len(raw):
        length = struct.unpack(">I", raw[pos:pos + 4])[0]
        kind = raw[pos + 4:pos + 8]
        data = raw[pos + 8:pos + 8 + length]
        crc = struct.unpack(">I", raw[pos + 8 + length:pos + 12 + length])[0]
        if zlib.crc32(kind + data) & 0xffffffff != crc:
            raise ValueError("Invalid PNG chunk CRC")
        kinds.append(kind)
        if kind == b"IHDR":
            width, height, depth, color, compression, filtering, interlace = struct.unpack(">IIBBBBB", data)
            if (width, height, depth, color, compression, filtering, interlace) != (320, 240, 8, 6, 0, 0, 0):
                raise ValueError("Unexpected image layout")
        if kind == b"IDAT":
            compressed.append(data)
        pos += length + 12
    if pos != len(raw) or not kinds or kinds[-1] != b"IEND" or b"sRGB" not in kinds:
        raise ValueError("Incomplete PNG or missing declared sRGB output")
    decoder = zlib.decompressobj()
    decoded = decoder.decompress(b"".join(compressed), 240 * (1 + 320 * 4) + 1)
    if len(decoded) != 240 * (1 + 320 * 4) or not decoder.eof or decoder.unconsumed_tail or decoder.unused_data:
        raise ValueError("Unexpected bounded PNG scanlines")
    digest = hashlib.sha256(raw).hexdigest()
    if expected[0] != digest or int(expected[1]) != len(raw) or expected[2:5] != ["320", "240", "gpu-path"]:
        raise ValueError("Exported PNG does not match the retained frame")
    return {"path": str(path), "sha256": digest, "bytes": len(raw), "width": width, "height": height,
            "executor": "gpu-path", "estimated_job_bytes": int(expected[5]), "crc_and_bounded_scanlines": "PASS"}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--target-dir", type=Path, default=ROOT / "target")
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    record = {"status": "RUNNING", "classification": "FOCUSED_NATIVE_RESOURCE_SMOKE",
              "not_a_benchmark": True, "application_ui_tested": False, "visual_review": "not_performed", "commands": []}
    receipt = out / "receipt.json"
    try:
        if git("status", "--porcelain"):
            raise RuntimeError("Commit the candidate source before running this exact-revision probe")
        record.update(head=git("rev-parse", "HEAD"), source_tree=git("rev-parse", "HEAD^{tree}"))
        source, source_receipt = generated()
        if OUTPUT.read_text(encoding="utf-8") != source:
            raise RuntimeError("Built-in asset lookup is stale")
        record["source_assets"] = source_receipt
        env = os.environ.copy()
        env.update(CARGO_TARGET_DIR=str(args.target_dir.resolve()), CARGO_BUILD_JOBS="2", CARGO_INCREMENTAL="0")
        command = ["cargo", "build", "--locked", "--profile", "ci", "-p", "solidcraft-gpu", "-p", "solidcraft-render",
                   "-p", "solidcraft-geom", "--lib", "--features", "solidcraft-render/raytrace-cpu", "--message-format=json"]
        with (out / "build.jsonl").open("w", encoding="utf-8") as stdout, (out / "build.log").open("w", encoding="utf-8") as stderr:
            result = subprocess.run(command, cwd=ROOT, env=env, stdout=stdout, stderr=stderr, stdin=subprocess.DEVNULL)
        record["commands"].append({"command": command, "exit_code": result.returncode})
        if result.returncode:
            raise RuntimeError("Renderer-only build failed; inspect build.log")
        artifacts = {}
        for line in (out / "build.jsonl").read_text(encoding="utf-8").splitlines():
            value = json.loads(line)
            if value.get("reason") == "compiler-artifact":
                name = value["target"]["name"]
                for filename in value["filenames"]:
                    if filename.endswith(".rlib"):
                        artifacts[name] = filename
        names = ["solidcraft_gpu", "solidcraft_render", "solidcraft_geom", "log"]
        if any(name not in artifacts for name in names):
            raise RuntimeError("Cargo did not report all exact library artifacts")
        binary = out / ("native_smoke.exe" if os.name == "nt" else "native_smoke")
        command = ["rustc", "--edition=2024", "-C", "opt-level=1", str(Path(__file__).with_name("native_smoke.rs")),
                   "-L", f"dependency={args.target_dir.resolve() / 'ci' / 'deps'}", "-o", str(binary)]
        for name in names:
            command += ["--extern", f"{name}={artifacts[name]}"]
        with (out / "probe-build.log").open("w", encoding="utf-8") as log:
            result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)
        record["commands"].append({"command": command, "exit_code": result.returncode})
        if result.returncode:
            raise RuntimeError("Native probe build failed; inspect probe-build.log")
        for key in list(env):
            if key.startswith(("SOLIDCRAFT_", "SOLVECRAFT_", "WGPU_")):
                env.pop(key)
        env.update(SOLIDCRAFT_GPU="hardware", SOLIDCRAFT_REQUIRE_GPU="1", WGPU_BACKEND="vulkan")
        command = [str(binary), str(ROOT), str(out)]
        with (out / "render.log").open("w", encoding="utf-8") as stdout, (out / "adapter.log").open("w", encoding="utf-8") as stderr:
            result = subprocess.run(command, cwd=ROOT, env=env, stdout=stdout, stderr=stderr, stdin=subprocess.DEVNULL)
        record["commands"].append({"command": command, "exit_code": result.returncode})
        if result.returncode:
            raise RuntimeError("Native GPU resource probe failed; inspect render.log and adapter.log")
        frames = []
        for line in (out / "render.log").read_text(encoding="utf-8").splitlines():
            fields = line.split("|")
            if fields[0] == "FRAME" and len(fields) == 8:
                frames.append(png_receipt(out / (fields[1] + ".png"), fields[2:]))
        log = (out / "render.log").read_text(encoding="utf-8")
        if len(frames) != 3 or "POOL|0" not in log or "STATUS|PASS" not in log:
            raise RuntimeError("Missing output or reservation-release evidence")
        adapter_lines = (out / "adapter.log").read_text(encoding="utf-8").splitlines()
        observations = [line for line in adapter_lines if re.fullmatch(r"\[info solidcraft_gpu::path\] gpu-path adapter: .+ \((DiscreteGpu|IntegratedGpu|VirtualGpu|Other)\)", line)]
        if len(observations) != 1:
            raise RuntimeError("Expected one non-software adapter observation from the renderer itself")
        if git("rev-parse", "HEAD^{tree}") != record["source_tree"] or git("status", "--porcelain"):
            raise RuntimeError("Candidate source changed during the probe")
        record.update(status="PASS", source_unchanged=True, frames=frames, adapter_observation=observations[0],
                      gpu_policy="hardware", backend_filter="vulkan", retained_pool_bytes_after_drop=0)
    except Exception as error:
        record.update(status="FAIL", error=str(error))
    finally:
        receipt.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(record, indent=2))
    return 0 if record["status"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())

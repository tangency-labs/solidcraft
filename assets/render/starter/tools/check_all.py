#!/usr/bin/env python3
"""Read-only authoring/native gate; receipts and logs stay under ignored target/."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[4]
PACK = Path(__file__).resolve().parents[1]
ROLES = ("base_color", "normal_open_gl", "roughness", "metallic")


def fingerprint(root: Path, paths: list[Path]) -> dict[str, str]:
    result = {}
    root = root.resolve()
    for path in sorted(paths):
        if path.is_symlink() or not path.resolve().is_relative_to(root):
            raise ValueError("Check inputs must be regular files inside the checkout")
        digest = hashlib.sha256()
        with path.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(chunk)
        result[path.relative_to(root).as_posix()] = digest.hexdigest()
    return result


def input_snapshot() -> dict[str, str]:
    paths = [p for p in PACK.rglob("*") if p.is_file()]
    paths += [p for p in (ROOT / "docs/render-materials").rglob("*") if p.is_file()]
    paths += [ROOT / name for name in (
        "Cargo.toml", "Cargo.lock", "crates/render/Cargo.toml",
        "crates/render/examples/validate_starter.rs", "ATTRIBUTION.md",
        ".gitattributes", "assets/render/.gitattributes", "rustfmt.toml",
        "crates/geom/Cargo.toml", "xtask/src/render_assets.rs",
    )]
    # Native source changes affect this gate even if the authored bytes did not change.
    paths += [p for p in (ROOT / "crates/render/src").rglob("*.rs")]
    paths += [p for p in (ROOT / "crates/geom/src").rglob("*.rs")]
    paths += [p for p in (ROOT / "assets/render/selene").rglob("*") if p.is_file()]
    paths += [p for p in (ROOT / "assets/render/physical").rglob("*") if p.is_file()]
    return fingerprint(ROOT, paths)


def check_native_report(report: dict) -> None:
    if report.get("schema") != "solidcraft-native-starter-check/1" or report.get("runtime_ready") is not False:
        raise ValueError("Native report has an invalid schema or runtime claim")
    expected = {"surfaces": 48, "texture_inputs": 192,
                "height_sources_verified_not_bound": 48, "hdr_inputs": 24,
                "mip_levels": 1920}
    for key, count in expected.items():
        if type(report.get(key)) is not int or report[key] != count:
            raise ValueError(f"Native report count mismatch: {key}")
    textures, hdrs = report.get("textures"), report.get("environments")
    if not isinstance(textures, list) or len(textures) != 192 or not isinstance(hdrs, list) or len(hdrs) != 24:
        raise ValueError("Native receipt lists are incomplete")
    expected_textures = {(f"SURF-{i:03}", role) for i in range(1, 49) for role in ROLES}
    if any(not isinstance(item, dict) for item in textures + hdrs):
        raise ValueError("Native receipts must be objects")
    if {(item.get("surface"), item.get("role")) for item in textures} != expected_textures:
        raise ValueError("Duplicate or missing native texture receipt")
    if {item.get("environment") for item in hdrs} != {f"HDR-{i:03}" for i in range(1, 25)}:
        raise ValueError("Duplicate or missing native HDR receipt")
    for item in textures:
        if type(item.get("mip_levels")) is not int or item["mip_levels"] != 10:
            raise ValueError("Incomplete native mip receipt")
    for item in hdrs:
        if type(item.get("pixels")) is not int or item["pixels"] != 1024 * 512:
            raise ValueError("Incomplete native HDR pixels")
    for item in textures + hdrs:
        for key in ("source_sha256", "normalized_sha256" if "surface" in item else "decoded_f32_le_sha256"):
            value = item.get(key)
            if not isinstance(value, str) or len(value) != 64 or any(c not in "0123456789abcdef" for c in value):
                raise ValueError("Malformed native SHA-256 receipt")


def run_stage(name: str, command: list[str], directory: Path, cwd: Path, timeout: int = 900) -> dict:
    started = time.monotonic()
    out_path, err_path = directory / f"{name}.stdout.txt", directory / f"{name}.stderr.txt"
    with out_path.open("wb") as out, err_path.open("wb") as err:
        try:
            process = subprocess.run(command, cwd=cwd, stdout=out, stderr=err, timeout=timeout, check=False)
            code = process.returncode
        except subprocess.TimeoutExpired:
            code = 124
            err.write(b"\nStage timed out; this is not a passing validation.\n")
    return {"name": name, "exit_code": code, "seconds": round(time.monotonic() - started, 3),
            "command": [Path(command[0]).name, *command[1:]],
            "stdout": out_path.name, "stderr": err_path.name}


def stages(authoring_only: bool, independent_decode: bool) -> list[tuple[str, list[str]]]:
    py = [sys.executable, "-B"]
    tools = "assets/render/starter/tools"
    commands = [
        ("python-tests", py + ["-m", "unittest", "discover", "-s", tools, "-p", "test_*.py"]),
        ("physical-tests", py + ["-m", "unittest", "discover", "-s", "assets/render/physical/tools", "-p", "test_*.py"]),
        ("physical-references", py + ["assets/render/physical/tools/physical.py", "validate"]),
        ("catalog", py + ["docs/render-materials/validate_catalog.py", "--json"]),
        ("selene-prepared", py + ["assets/render/selene/tools/validate_prepared.py", "--json"]),
        ("pack", py + [f"{tools}/validate_pack.py"]),
        ("materials-reproduce", py + [f"{tools}/materials.py", "validate", "--reproduce"]),
        ("environments-reproduce", py + [f"{tools}/environments.py", "validate", "--reproducible"]
         + (["--independent-decode"] if independent_decode else [])),
    ]
    if not authoring_only:
        cargo = ["cargo"]
        native = ["--locked", "--profile", "ci", "-p", "solidcraft-render"]
        commands += [
            ("format", cargo + ["fmt", "--all", "--", "--check"]),
            ("attribution", cargo + ["xtask", "assets"]),
            ("icons", cargo + ["xtask", "icons"]),
            ("render-clippy", cargo + ["clippy", *native, "--all-targets", "--", "-D", "warnings"]),
            ("render-unit-tests", cargo + ["test", *native, "--lib"]),
            # Keep optional CPU regressions exercised explicitly without making
            # the default renderer depend on a workspace's feature unification.
            ("render-cpu-clippy", cargo + ["clippy", *native, "--features", "raytrace-cpu", "--all-targets", "--", "-D", "warnings"]),
            ("render-cpu-unit-tests", cargo + ["test", *native, "--features", "raytrace-cpu", "--lib"]),
            ("native-regressions", cargo + ["test", *native, "--example", "validate_starter"]),
            ("native-admission", cargo + ["run", *native, "--example", "validate_starter"]),
        ]
    return commands


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--authoring-only", action="store_true", help="Explicitly omit native/repository checks; the report records this")
    parser.add_argument("--independent-decode", action="store_true", help="Also require an installed FFmpeg decoder; no silent skip")
    args = parser.parse_args()
    identifier = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    directory = ROOT / "target/render-starter-checks" / identifier
    directory.mkdir(parents=True, exist_ok=False)
    report = {"schema": "solidcraft-starter-gate/1", "status": "failed",
              "python": platform.python_version(), "platform": platform.system(),
              "native_requested": not args.authoring_only, "independent_ffmpeg_requested": args.independent_decode,
              "full_workspace_ci": "not_run_by_this_gate", "runtime_ready": False, "stages": []}
    try:
        before = input_snapshot()
        for name, command in stages(args.authoring_only, args.independent_decode):
            stage = run_stage(name, command, directory, ROOT)
            report["stages"].append(stage)
            print(json.dumps(stage), flush=True)
            if stage["exit_code"]:
                raise RuntimeError(f"Failed stage: {name}; see {directory.relative_to(ROOT) / stage['stderr']}")
            if name == "native-admission":
                native = json.loads((directory / stage["stdout"]).read_bytes())
                check_native_report(native)
                report["native_summary"] = {key: value for key, value in native.items() if key not in ("textures", "environments")}
        after = input_snapshot()
        if before != after:
            raise RuntimeError("Source inputs changed during the gate; discard this mixed-revision result")
        report["input_sha256"] = before
        report["source_inputs_unchanged"] = True
        report["status"] = "pass"
    except (OSError, ValueError, TypeError, RuntimeError) as error:
        report["error"] = str(error)
        print(str(error), file=sys.stderr)
    finally:
        path = directory / "report.json"
        path.write_bytes((json.dumps(report, indent=2, allow_nan=False) + "\n").encode("utf-8"))
        print(json.dumps({"status": report["status"], "report": path.relative_to(ROOT).as_posix(),
                          "native_requested": report["native_requested"], "runtime_ready": False}), flush=True)
    return 0 if report["status"] == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Check decoded cargo-auditable metadata in staged or unpacked release binaries."""

import argparse
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib


FAT_MAGIC = {bytes.fromhex(value) for value in ("cafebabe", "bebafeca", "cafebabf", "bfbafeca")}


def run(*args):
    result = subprocess.run(args, capture_output=True, text=True, check=False)
    if result.returncode:
        raise ValueError(f"{args[0]} failed: {result.stderr.strip()}")
    return result.stdout


def check_metadata(path, package, version):
    data = json.loads(run("rust-audit-info", str(path)))
    packages = data.get("packages") if isinstance(data, dict) else None
    if not isinstance(packages, list) or not packages or not all(isinstance(p, dict) for p in packages):
        raise ValueError(f"{path}: missing package records")
    roots = [p for p in packages if p.get("root") is True]
    if len(roots) != 1 or roots[0].get("name") != package or roots[0].get("version") != version:
        raise ValueError(f"{path}: expected one root package {package} {version}, got {roots}")
    if not all(isinstance(p.get(key), str) and p[key] for p in packages for key in ("name", "version")):
        raise ValueError(f"{path}: a package has no name or version")
    print(f"ok {path.name}: {package} {version}, {len(packages)} package records")


def check_binary(path, package, version):
    with path.open("rb") as binary:
        magic = binary.read(4)
    if magic not in FAT_MAGIC:
        check_metadata(path, package, version)
        return
    # rust-audit-info reads a thin Mach-O. Check every slice of the final universal
    # binary, so a good arm64 half cannot conceal an unauditable Intel half.
    arches = run("lipo", "-archs", str(path)).split()
    if not arches:
        raise ValueError(f"{path}: no architectures in universal binary")
    with tempfile.TemporaryDirectory(prefix="solidcraft-audit-") as directory:
        for arch in arches:
            thin = Path(directory) / f"slice-{arch}"
            run("lipo", str(path), "-thin", arch, "-output", str(thin))
            check_metadata(thin, package, version)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", required=True, help="expected Cargo package name")
    parser.add_argument("--version", help="expected Cargo version (default: workspace version)")
    parser.add_argument("binaries", type=Path, nargs="+")
    args = parser.parse_args()
    try:
        version = args.version
        if version is None:
            manifest = Path(__file__).resolve().parents[1] / "Cargo.toml"
            with manifest.open("rb") as source:
                version = tomllib.load(source)["workspace"]["package"]["version"]
        for binary in args.binaries:
            check_binary(binary, args.package, version)
    except (OSError, ValueError, KeyError) as error:
        print(f"audit metadata: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

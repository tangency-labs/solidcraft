#!/usr/bin/env python3
"""Compare actual production DoSweep results with unchanged pinned SMLib C++."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import random
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "bin/adapter-evidence"
RUST = ROOT / "bin/debug/examples/wire_sweep"
CPP = OUT / "wire-sweep-cpp"
PIN = next(line.split("=", 1)[1].strip() for line in (ROOT / "scripts/upstream-pin.env").read_text().splitlines()
           if line.startswith("UPSTREAM_COMMIT="))


def run(command):
    try:
        result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, timeout=20)
    except subprocess.TimeoutExpired:
        return {"execution_timeout_seconds": 20}
    if result.returncode:
        return {"execution_error": result.returncode, "stdout": result.stdout, "stderr": result.stderr}
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError:
        return {"invalid_json": result.stdout, "stderr": result.stderr}


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=OUT)
    parser.add_argument("--rust", type=Path, default=RUST)
    parser.add_argument("--cpp", type=Path, default=CPP)
    parser.add_argument("--cpp-dir", type=Path, default=ROOT / "bin/cpp")
    parser.add_argument("--cxx", default=os.environ.get("CXX", "g++"))
    options = parser.parse_args()
    revision = subprocess.check_output(["git", "-C", "upstream", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    changes = subprocess.check_output(["git", "-C", "upstream", "status", "--porcelain", "--", "source/SMLib", "source/SM_API"], cwd=ROOT, text=True)
    if revision != PIN or changes:
        raise SystemExit("C++ reference must be the unchanged pinned source")
    cases = []
    contexts = [(shared, sf, tf) for shared in [0, 1] for sf in [0, 1] for tf in [0, 1] if not shared or sf == tf]

    def add(name, p, vector, tol=1e-5, context=(1, 0, 0), repetitions=1):
        cases.append({"name": name, "args": [*p, *vector, tol, *context, repetitions]})

    for context in contexts:
        for repetitions in [0, 1, 2, 3]:
            add(f"contexts_{context}_repetitions_{repetitions}", [1, 2, 3], [10, -4, 6], context=context, repetitions=repetitions)
        for distance in [0.0, 1e-6, math.nextafter(2e-5, 0.0), 2e-5, math.nextafter(2e-5, math.inf)]:
            add(f"tolerance_boundary_{context}_{distance!r}", [0, 0, 0], [distance, 0, 0], context=context)
    rng = random.Random(204255)
    for scale in [1e-3, 1.0, 1e3, 1e6]:
        for n in range(6):
            p = [rng.uniform(-3, 3) * scale for _ in range(3)]
            v = [rng.uniform(-2, 2) * scale for _ in range(3)]
            for shared in [0, 1]:
                add(f"scale_{scale}_seed_{n}_shared_{shared}", p, v, min(1e-5, scale * 1e-4), (shared, 0, 0))

    results = []
    max_relative_error = 0.0
    values_compared = 0
    integers_compared = 0
    for case in cases:
        args = [repr(x) for x in case["args"]]
        native = run([str(options.cpp.resolve()), *args])
        rust = run([str(options.rust.resolve()), *args])
        errors = []
        if "ints" not in native or "ints" not in rust:
            errors.append("engine execution or output failure")
        else:
            if native["ints"] != rust["ints"]:
                errors.append("status/topology/maps/context flags differ")
            integers_compared += min(len(native["ints"]), len(rust["ints"]))
            if len(native["values"]) != len(rust["values"]):
                errors.append("different numeric output lengths")
            for index, (a, b) in enumerate(zip(native["values"], rust["values"])):
                values_compared += 1
                scale = max(abs(a), abs(b))
                error = abs(a - b)
                relative = error / scale if scale else 0.0
                max_relative_error = max(max_relative_error, relative)
                if not math.isfinite(a) or not math.isfinite(b) or error > 1e-9 * scale:
                    errors.append({"value_index": index, "cpp": a, "rust": b, "relative_error": relative})
        results.append({**case, "pass": not errors, "errors": errors, "cpp": native, "rust": rust})
    report = {
        "upstream_commit": revision,
        "reference_sources_modified": bool(changes),
        "platform": {"system": platform.system(), "machine": platform.machine()},
        "rust_compiler": subprocess.check_output(["rustc", "--version"], text=True).strip(),
        "cpp_compiler": subprocess.check_output([options.cxx, "--version"], text=True).splitlines()[0],
        "cpp_flags": "-std=c++17 -O2 -ffp-contract=off -fno-fast-math -w -DNDEBUG -DNOMINMAX -DSM_GRAPHICS_CALLBACKS=1 -D_GLIBCXX_USE_CXX11_ABI=1",
        "qualification": "Local native-executable parity on the recorded OS, architecture and compiler; translation of points to wire edges only",
        "comparison": "Exact live topology, normalized status, four sweep-map relationships and context flags; numeric values use 1e-9 relative, no absolute floor",
        "source_sha256": {str(p.relative_to(ROOT)): digest(p) for p in [
            ROOT / "crates/solidcore/examples/wire_sweep.rs",
            ROOT / "harness/cpp/wire_sweep_reference.cpp", Path(__file__).resolve(),
        ]},
        "cpp_library_sha256": digest(options.cpp_dir / "lib/libSMLib.a"),
        "cpp_executable_sha256": digest(options.cpp), "rust_executable_sha256": digest(options.rust),
        "case_count": len(results), "passed": sum(r["pass"] for r in results),
        "failed": sum(not r["pass"] for r in results),
        "successful_wire_cases": sum(r["pass"] and r["rust"]["ints"][0] == 0 and r["rust"]["ints"][6] > 0 for r in results),
        "values_compared": values_compared, "integers_compared": integers_compared,
        "max_relative_error": max_relative_error,
        "results": results,
    }
    options.output_dir.mkdir(parents=True, exist_ok=True)
    (options.output_dir / "wire-parity.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({k: v for k, v in report.items() if k != "results"}, indent=2))
    for r in results:
        if not r["pass"]:
            print(r["name"], r["errors"])
    return 1 if report["failed"] else 0


if __name__ == "__main__":
    sys.exit(main())

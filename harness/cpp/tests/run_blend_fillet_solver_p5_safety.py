#!/usr/bin/env python3
"""Standalone rollover FFI allocation and leak regressions (no workflow changes)."""
import argparse
import collections
import os
from pathlib import Path
import resource
import subprocess
import sys


def run(command, **kwargs):
    return subprocess.run(["nice", "-n", "10", *map(str, command)],
                          capture_output=True, text=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--asan", action="store_true", help="check direct destruction with ASan/LSan")
    args = parser.parse_args()
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    root = Path(__file__).resolve().parents[3]
    cpp = Path(os.environ.get("SOLIDCORE_CPP_DIR", root / "bin/cpp"))
    output = root / "bin/safety"
    output.mkdir(parents=True, exist_ok=True)
    binary = output / ("rollover-asan" if args.asan else "rollover-allocation")
    command = [os.environ.get("CXX", "c++"), "-std=c++17", "-O1", "-g",
               "-ffp-contract=off", "-fno-fast-math", "-DNDEBUG", "-DNOMINMAX",
               "-DSM_GRAPHICS_CALLBACKS=1", "-Iharness/cpp/shim",
               "-Iupstream/source/SMLib/inc", "-Iupstream/source/SM_API/inc",
               "harness/cpp/tests/blend_fillet_solver_p5_alloc.cpp",
               "harness/cpp/shim/topology_edgeuse_p1.cpp",
               "harness/cpp/shim/sc_shim.cpp",
               f"-L{cpp / 'lib'}", "-lSM_API", "-lSMLib",
               "-pthread", "-o", str(binary)]
    if sys.platform != "darwin":
        command += ["-Wl,--wrap=calloc"]
    if args.asan:
        command += ["-fsanitize=address", "-fno-omit-frame-pointer"]
    built = run(command, cwd=root, timeout=120)
    if built.returncode:
        raise RuntimeError(built.stdout + built.stderr)
    if args.asan:
        env = dict(os.environ, ASAN_OPTIONS="detect_leaks=1:fast_unwind_on_malloc=0")
        for mode, op in [("direct", 0), ("direct", 1), ("direct", 2), ("direct-target", 0)]:
            result = run([binary, mode, op], cwd=root, env=env, timeout=40)
            if result.returncode:
                raise RuntimeError(result.stdout + result.stderr)
            print(result.stdout.strip(), flush=True)
        print("ASAN/LSAN PASS: 4 direct calls, fixture destructors executed", flush=True)
        return
    for mode in ("guard", "descriptor"):
        checked = run([binary, mode, 0], cwd=root, timeout=40)
        if checked.returncode:
            raise RuntimeError(checked.stdout + checked.stderr)
        print(checked.stdout.strip(), flush=True)
    statuses = collections.Counter()
    count = 0
    for mode in ("new", "calloc"):
        for target in (False, True):
            for index in range(300):
                command = [binary, mode, index] + (["target"] if target else [])
                result = run(command, cwd=root, timeout=40)
                if result.returncode:
                    raise RuntimeError(f"failed {command}: {result.stdout}{result.stderr}")
                statuses[result.stdout.rsplit("status=", 1)[-1].strip()] += 1
                count += 1
    for signal in (0, 1):
        result = run([binary, "signal", signal], cwd=root, timeout=40)
        if result.returncode:
            raise RuntimeError(result.stdout + result.stderr)
    print(f"ALLOCATION PASS: {count} probes, 0 host crashes; statuses={dict(statuses)}")
    print("SIGNAL PASS: native SIGSEGV and SIGABRT contained as SC_ERR_EXCEPTION")
    result = run([binary, "signal", 3], cwd=root, timeout=40)
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    print("TIMEOUT PASS: stalled worker killed and reaped as SC_ERR_EXCEPTION", flush=True)
    result = run([binary, "exit", 0], cwd=root, timeout=40)
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    print("EXIT PASS: post-response global destructor cannot affect the worker", flush=True)


if __name__ == "__main__":
    main()

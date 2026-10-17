#!/usr/bin/env python3
"""Test build.sh orchestration without CMake, upstream sources, or a C++ build."""

import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest


MOCK_CMAKE = """
import json, os, pathlib, sys
args = sys.argv[1:]
phase = "build" if "--build" in args else "install" if "--install" in args else "configure"
with open(os.environ["CMAKE_CALLS"], "a") as log:
    log.write(json.dumps(args) + "\\n")
if os.environ.get("FAIL_PHASE") == phase:
    sys.exit(23 if phase == "configure" else 42)
if phase == "configure":
    build = pathlib.Path(args[args.index("-B") + 1])
    build.mkdir(parents=True, exist_ok=True)
    (build / "compile_commands.json").write_text("[]\\n")
"""


class BuildScriptTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="solidcore build test ")
        self.addCleanup(temporary.cleanup)
        self.temp = Path(temporary.name).resolve()
        self.repo = self.temp / "repository with spaces"
        self.harness = self.repo / "harness" / "cpp"
        self.harness.mkdir(parents=True)
        shutil.copy2(Path(__file__).with_name("build.sh"), self.harness / "build.sh")
        self.tools = self.temp / "tools with spaces"
        self.tools.mkdir()
        # Only these host utilities are exposed; host cmake/ninja cannot leak in.
        for name in ("cat", "cp", "cut", "date", "dirname", "grep", "mkdir", "nice", "rm"):
            executable = shutil.which(name)
            self.assertIsNotNone(executable, f"required test utility: {name}")
            (self.tools / name).symlink_to(executable)
        self.tool("sha256sum", "import hashlib, sys; print(hashlib.sha256(sys.stdin.buffer.read()).hexdigest())")
        self.tool("cmake", MOCK_CMAKE)
        self.log = self.temp / "cmake calls.jsonl"
        self.out = self.repo / "bin" / "cpp"

    def tool(self, name, code):
        path = self.tools / name
        path.write_text(
            "#!/bin/sh\nexec " + shlex.quote(sys.executable)
            + " -c " + shlex.quote(code) + ' "$@"\n'
        )
        path.chmod(0o755)

    def build_dir(self, out):
        key = hashlib.sha256(os.fsencode(self.harness)).hexdigest()[:8]
        return out / ("build-" + key)

    def run_build(self, *args, ninja=False, cmake=True, out=None, fail=""):
        if ninja:
            self.tool("ninja", 'raise SystemExit("ninja must not be executed")')
        if not cmake:
            (self.tools / "cmake").unlink()
        env = {
            "PATH": str(self.tools),
            "HOME": str(self.temp),
            "TMPDIR": str(self.temp),
            "LC_ALL": "C",
            "CMAKE_CALLS": str(self.log),
            "FAIL_PHASE": fail,
        }
        if out is not None:
            env["SOLIDCORE_CPP_DIR"] = str(out)
        return subprocess.run(
            ["/bin/bash", str(self.harness / "build.sh"), *args],
            cwd=self.repo, env=env, capture_output=True, text=True, timeout=15,
        )

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def assert_success(self, result, *, ninja=False, out=None, jobs="6", tests="ON"):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        out = self.out if out is None else out
        build = str(self.build_dir(out))
        configure = ["-S", str(self.harness), "-B", build]
        if ninja:
            configure += ["-G", "Ninja"]
        configure += [
            "-DCMAKE_BUILD_TYPE=Release",
            f"-DCMAKE_INSTALL_PREFIX={out}",
            f"-DSMLIB_UPSTREAM={self.repo / 'upstream'}",
            f"-DSC_BUILD_TESTS={tests}",
        ]
        self.assertEqual(self.calls(), [
            configure, ["--build", build, "--parallel", jobs], ["--install", build],
        ])
        self.assertEqual((out / "compile_commands.json").read_text(), "[]\n")
        self.assertTrue((out / "ffi.env").is_file())
        self.assertFalse((self.repo / "upstream").exists())

    def test_no_ninja_uses_default_generator(self):
        self.assert_success(self.run_build())

    def test_ninja_is_passed_exactly_once(self):
        self.assert_success(self.run_build(ninja=True), ninja=True)

    def test_flags_and_output_override_preserve_spaces(self):
        out = self.temp / "external output with spaces"
        result = self.run_build("--jobs", "3", "--no-tests", ninja=True, out=out)
        self.assert_success(result, ninja=True, out=out, jobs="3", tests="OFF")
        self.assertFalse(self.out.exists())

    def test_configure_failure_stops_build_and_install(self):
        result = self.run_build(ninja=True, fail="configure")
        self.assertEqual(result.returncode, 23, result.stdout + result.stderr)
        calls = self.calls()
        self.assertEqual(len(calls), 1, calls)
        self.assertEqual(calls[0][0], "-S")
        self.assertFalse((self.out / "compile_commands.json").exists())
        self.assertFalse((self.out / "ffi.env").exists())
        self.assertNotIn("C++ build finished", result.stdout)

    def test_build_failure_stops_install(self):
        result = self.run_build(ninja=True, fail="build")
        self.assertEqual(result.returncode, 42, result.stdout + result.stderr)
        calls = self.calls()
        self.assertEqual(len(calls), 2, calls)
        self.assertEqual(calls[0][0], "-S")
        self.assertEqual(calls[1], ["--build", str(self.build_dir(self.out)), "--parallel", "6"])
        self.assertNotIn("C++ build finished", result.stdout)

    def assert_missing_cmake(self, result):
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("cmake", (result.stdout + result.stderr).lower())
        self.assertEqual(self.calls(), [])

    def test_missing_cmake_does_not_create_output(self):
        result = self.run_build(ninja=True, cmake=False)
        self.assert_missing_cmake(result)
        self.assertFalse(self.out.parent.exists(), "missing CMake created output directories")

    def test_missing_cmake_does_not_clean_existing_output(self):
        build = self.build_dir(self.out)
        build.mkdir(parents=True)
        sentinel = build / "keep this file.txt"
        sentinel.write_text("existing build\n")
        result = self.run_build("--clean", ninja=True, cmake=False)
        self.assert_missing_cmake(result)
        self.assertTrue(sentinel.is_file(), "missing CMake removed the existing build")
        self.assertEqual(sentinel.read_text(), "existing build\n")
        self.assertEqual(set(self.out.iterdir()), {build})
        self.assertEqual(set(build.iterdir()), {sentinel})


if __name__ == "__main__":
    unittest.main(verbosity=2)

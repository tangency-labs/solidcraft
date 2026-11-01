"""Exercise release selection and package build metadata without GitHub or repository mutations."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "nightly.sh"


class PackageVersion(unittest.TestCase):
    def build_versions(self, overrides):
        env = os.environ.copy()
        for key in ("SOLIDCRAFT_VERSION", "SOLVECRAFT_VERSION"):
            env.pop(key, None)
        env.update(overrides)
        with tempfile.TemporaryDirectory() as directory:
            env["DIST"] = directory
            result = subprocess.run(
                ["bash", "-c", '. "$1"; python3 -c \'import json, os; '
                 'print(json.dumps([os.environ.get("VERSION"), os.environ.get("SOLIDCRAFT_VERSION")]))\'',
                 "package-version", str(SCRIPT.parent / "env.sh")],
                env=env, capture_output=True, text=True, check=True,
            )
        return json.loads(result.stdout)

    def test_default_package_version_reaches_the_compiler(self):
        manifest = tomllib.loads((SCRIPT.parent.parent / "Cargo.toml").read_text())
        version = manifest["workspace"]["package"]["version"]
        self.assertEqual(self.build_versions({}), [version, version])

    def test_legacy_version_override_reaches_the_compiler(self):
        version = "0.28.0-nightly.20261031.1"
        self.assertEqual(self.build_versions({"SOLVECRAFT_VERSION": version}), [version, version])
        self.assertEqual(self.build_versions({"SOLIDCRAFT_VERSION": "", "SOLVECRAFT_VERSION": version}), [version, version])

    def test_canonical_version_override_wins_for_package_and_compiler(self):
        self.assertEqual(self.build_versions({"SOLIDCRAFT_VERSION": "0.29.0-rc.1", "SOLVECRAFT_VERSION": "0.28.0"}),
                         ["0.29.0-rc.1", "0.29.0-rc.1"])


def release(tag, published, *, draft=False, prerelease=False):
    return {"tag_name": tag, "published_at": published, "draft": draft,
            "prerelease": prerelease, "target_commitish": "main"}


class NightlyDecision(unittest.TestCase):
    def run_decision(self, pages, refs=None, *, event="schedule", sha="current", api_fail=False):
        self.assertIsNotNone(shutil.which("jq"), "install jq to run packaging checks")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "output"
            summary = root / "summary"
            calls = root / "calls"
            output.touch()
            summary.touch()
            for command, body in {
                "gh": '''import json, os, sys
args = sys.argv[1:]
with open(os.environ["CALLS"], "a") as log: log.write(json.dumps(args) + "\\n")
if args != ["api", "--paginate", "--slurp", "repos/test/project/releases?per_page=100"]:
    sys.exit("unexpected gh invocation (slurp and jq must not be combined)")
if os.environ["API_FAIL"] == "1": sys.exit("mock API failure")
print(os.environ["PAGES"])
''',
                "git": '''import json, os, sys
args = sys.argv[1:]
with open(os.environ["CALLS"], "a") as log: log.write(json.dumps(args) + "\\n")
refs = json.loads(os.environ["REFS"])
if len(args) != 3 or args[:2] != ["rev-parse", "--verify"] or args[2] not in refs:
    sys.exit("missing tag")
print(refs[args[2]])
''',
            }.items():
                path = root / command
                path.write_text("#!/usr/bin/env python3\n" + body, encoding="utf-8")
                path.chmod(0o755)
            env = {"PATH": str(root) + os.pathsep + os.environ["PATH"],
                   "GITHUB_EVENT_NAME": event, "GITHUB_REPOSITORY": "test/project",
                   "GITHUB_SHA": sha, "GITHUB_OUTPUT": str(output),
                   "GITHUB_STEP_SUMMARY": str(summary), "CALLS": str(calls),
                   "PAGES": json.dumps(pages), "REFS": json.dumps(refs or {}),
                   "API_FAIL": "1" if api_fail else "0"}
            result = subprocess.run(["bash", str(SCRIPT)], env=env, capture_output=True, text=True, check=False)
            return result, output.read_text(), summary.read_text(), calls.read_text() if calls.exists() else ""

    def test_newest_published_prerelease_across_pages_skips_exact_tag_commit(self):
        pages = [[release("v1", "2026-01-01T00:00:00Z")],
                 [release("v2-nightly", "2026-01-03T00:00:00Z", prerelease=True),
                  release("v3-draft", "2026-01-04T00:00:00Z", draft=True)]]
        result, output, summary, calls = self.run_decision(pages, {"refs/tags/v2-nightly^{commit}": "current"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(output, "build=false\n")
        self.assertIn("v2-nightly", summary)
        self.assertIn("refs/tags/v2-nightly^{commit}", calls)
        self.assertNotIn("v3-draft^{commit}", calls)

    def test_changed_commit_builds(self):
        result, output, _, _ = self.run_decision([[release("v1", "2026-01-01T00:00:00Z")]],
                                                {"refs/tags/v1^{commit}": "previous"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(output, "build=true\n")

    def test_no_published_release_builds(self):
        for pages in [[], [[]], [[release("draft", None, draft=True)]]]:
            with self.subTest(pages=pages):
                result, output, _, calls = self.run_decision(pages)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(output, "build=true\n")
                self.assertNotIn("rev-parse", calls)

    def test_non_schedule_does_not_query_github(self):
        for event in ["push", "workflow_dispatch"]:
            with self.subTest(event=event):
                result, output, _, calls = self.run_decision([], event=event, api_fail=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(output, "build=true\n")
                self.assertEqual(calls, "")

    def test_api_failure_does_not_report_build_or_skip(self):
        result, output, _, _ = self.run_decision([], api_fail=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(output, "")

    def test_missing_tag_fails_instead_of_using_moving_target_commitish(self):
        result, output, _, _ = self.run_decision([[release("missing", "2026-01-01T00:00:00Z")]])
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(output, "")


if __name__ == "__main__":
    unittest.main()

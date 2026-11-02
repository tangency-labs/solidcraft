"""fetch-reports.sh: portable replacement, rollback and fallback (no network)."""
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "fetch-reports.sh"
REAL_MV = shutil.which("mv")
PYTHON = sys.executable

# A BSD-style mv: it rejects GNU's -t/--target-directory, and can simulate a
# publish failure (a source name) or a rollback failure (anything under backup).
FAKE_MV = """#!__PYTHON__
import os
import sys

args = sys.argv[1:]
if "-t" in args or "--target-directory" in args:
    sys.stderr.write("mv: illegal option -- t\\n")
    sys.exit(64)
failure = os.environ.get("FAKE_MV_FAIL_SUBSTRING")
if failure and any(failure in arg for arg in args):
    sys.stderr.write("mv: simulated publish failure\\n")
    sys.exit(1)
if os.environ.get("FAKE_MV_FAIL_BACKUP") and any("/backup." in arg for arg in args):
    sys.stderr.write("mv: simulated rollback failure\\n")
    sys.exit(1)
os.execv(__REAL_MV__, [__REAL_MV__] + args)
"""
FAKE_MV = FAKE_MV.replace("__PYTHON__", PYTHON).replace("__REAL_MV__", repr(REAL_MV))

# A gh that only answers the calls fetch-reports.sh makes. Staged artifacts live
# under FAKE_GH_STAGING/<run id>; every call is logged for the filter/retry tests.
FAKE_GH = """#!__PYTHON__
import os
from pathlib import Path
import shutil
import sys

args = sys.argv[1:]
log = os.environ.get("FAKE_GH_LOG")
if log:
    with open(log, "a", encoding="utf-8") as stream:
        stream.write("\\t".join(args) + "\\n")

if args[:1] == ["api"] and len(args) >= 2:
    url = args[1]
    if "/actions/workflows/ci.yml/runs?" in url:
        for run in os.environ.get("FAKE_GH_RUNS", "").split(","):
            if run:
                print(run)
        sys.exit(0)
    if "/actions/runs/" in url and "/artifacts?name=difftest-reports" in url:
        run = url.split("/runs/", 1)[1].split("/", 1)[0]
        if run in os.environ.get("FAKE_GH_NO_ARTIFACT", "").split(","):
            sys.exit(0)
        print("artifact-" + run)
        sys.exit(0)
    sys.exit(1)

if args[:2] == ["run", "download"]:
    run = args[2]
    if run in os.environ.get("FAKE_GH_DOWNLOAD_FAIL", "").split(","):
        sys.stderr.write("gh: simulated download failure\\n")
        sys.exit(1)
    out = None
    for index, arg in enumerate(args):
        if arg == "-D" and index + 1 < len(args):
            out = args[index + 1]
    if out is None:
        sys.exit(2)
    dst = Path(out)
    dst.mkdir(parents=True, exist_ok=True)
    src = Path(os.environ["FAKE_GH_STAGING"]) / run
    if src.is_dir():
        for item in sorted(src.iterdir()):
            if item.is_dir():
                shutil.copytree(item, dst / item.name)
            else:
                shutil.copy2(item, dst / item.name, follow_symlinks=False)
    sys.exit(0)

sys.exit(2)
"""
FAKE_GH = FAKE_GH.replace("__PYTHON__", PYTHON)


class FetchReportsTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.dest = self.repo / "reports/difftest"
        self.staging = self.root / "staging"
        self.staging.mkdir()
        self.worktmp = self.root / "tmp"
        self.worktmp.mkdir()
        self.fakebin = self.root / "bin"
        self.fakebin.mkdir()

        self._script(self.fakebin / "gh", FAKE_GH)
        self._script(self.fakebin / "mv", FAKE_MV)

        self.git("init", "--quiet")
        self.git("-c", "user.name=Fetch test", "-c", "user.email=fetch@example.invalid",
                 "commit", "--quiet", "--allow-empty", "-m", "base")

        self.env = dict(os.environ)
        self.env.update(
            PATH=str(self.fakebin) + os.pathsep + os.environ["PATH"],
            TMPDIR=str(self.worktmp),
            GITHUB_REPOSITORY="tangency-labs/solidcore",
            FAKE_GH_STAGING=str(self.staging),
            FAKE_GH_LOG=str(self.root / "gh.log"),
        )
        for name in ("FAKE_MV_FAIL_SUBSTRING", "FAKE_MV_FAIL_BACKUP"):
            self.env.pop(name, None)

    def tearDown(self):
        self._tmp.cleanup()

    @staticmethod
    def _script(path, text):
        path.write_text(text)
        path.chmod(0o755)

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.repo, text=True).strip()

    def write_files(self, base, files):
        for name, text in files.items():
            path = base / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)

    def stage(self, run, files=None):
        base = self.staging / run
        base.mkdir(parents=True, exist_ok=True)
        self.write_files(base, files or {})

    def run_script(self, dest=None, **env):
        script_env = dict(self.env)
        script_env.update({key: str(value) for key, value in env.items()})
        return subprocess.run(["bash", str(SCRIPT), str(dest or self.dest)],
                              cwd=self.repo, env=script_env, text=True,
                              capture_output=True)

    def test_bsd_mv_installs_reports_with_spaces_newlines_and_leading_dash(self):
        self.write_files(self.dest, {"old.json": "old", "notes.txt": "keep",
                                     "times.tsv": "old\t1\n"})
        self.write_files(self.dest / "nested", {"keep.txt": "nested"})
        reports = {"new report.json": "new", "-dash.json": "dash",
                   "line\nbreak.json": "line", "times.tsv": "new\t2\n"}
        self.stage("101", reports)

        result = self.run_script(FAKE_GH_RUNS="101")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.dest / "old.json").exists())
        for name, text in reports.items():
            self.assertEqual((self.dest / name).read_text(), text)
        self.assertEqual((self.dest / "notes.txt").read_text(), "keep")
        self.assertEqual((self.dest / "nested/keep.txt").read_text(), "nested")
        log = (self.root / "gh.log").read_text()
        self.assertIn("workflows/ci.yml/runs?branch=main&event=push&per_page=20", log)

    def test_zero_reports_preserves_old_set(self):
        self.write_files(self.dest, {"old.json": "old", "notes.txt": "keep"})
        self.stage("101")

        result = self.run_script(FAKE_GH_RUNS="101")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no reports", result.stderr)
        self.assertEqual((self.dest / "old.json").read_text(), "old")
        self.assertEqual((self.dest / "notes.txt").read_text(), "keep")

    def test_download_failure_preserves_old_set(self):
        self.write_files(self.dest, {"old.json": "old", "notes.txt": "keep"})
        self.stage("101", {"new.json": "new"})

        result = self.run_script(FAKE_GH_RUNS="101", FAKE_GH_DOWNLOAD_FAIL="101")

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.dest / "old.json").read_text(), "old")
        self.assertEqual((self.dest / "notes.txt").read_text(), "keep")
        self.assertFalse((self.dest / "new.json").exists())

    def test_staging_directory_is_rejected_before_dest_is_touched(self):
        self.write_files(self.dest, {"old.json": "old", "notes.txt": "keep"})
        self.stage("101", {"new.json": "new"})
        (self.staging / "101/dir.json").mkdir(parents=True)
        self.write_files(self.staging / "101/dir.json", {"child.txt": "x"})

        result = self.run_script(FAKE_GH_RUNS="101")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("refusing directory named dir.json", result.stderr)
        self.assertEqual((self.dest / "old.json").read_text(), "old")
        self.assertEqual((self.dest / "notes.txt").read_text(), "keep")
        self.assertFalse((self.dest / "new.json").exists())

    def test_destination_directory_named_json_is_preserved(self):
        self.write_files(self.dest, {"old.json": "old", "notes.txt": "keep"})
        (self.dest / "new.json").mkdir()
        self.write_files(self.dest / "new.json", {"keep.txt": "subdir"})
        self.stage("101", {"new.json": "new", "next.json": "next"})

        result = self.run_script(FAKE_GH_RUNS="101")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("refusing to replace directory", result.stderr)
        self.assertEqual((self.dest / "old.json").read_text(), "old")
        self.assertEqual((self.dest / "notes.txt").read_text(), "keep")
        self.assertEqual((self.dest / "new.json/keep.txt").read_text(), "subdir")
        self.assertFalse((self.dest / "next.json").exists())

    def test_staged_times_directory_is_rejected_before_dest_is_touched(self):
        self.write_files(self.dest, {"old.json": "old", "times.tsv": "old\n",
                                     "notes.txt": "keep"})
        self.stage("101", {"new.json": "new", "times.tsv/child.txt": "directory"})

        result = self.run_script(FAKE_GH_RUNS="101")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("refusing directory named times.tsv", result.stderr)
        self.assertEqual((self.dest / "old.json").read_text(), "old")
        self.assertEqual((self.dest / "times.tsv").read_text(), "old\n")
        self.assertEqual((self.dest / "notes.txt").read_text(), "keep")
        self.assertFalse((self.dest / "new.json").exists())

    def test_publish_failure_rolls_back_the_previous_set(self):
        self.write_files(self.dest, {"a.json": "old-a", "b.json": "old-b",
                                     "extra.txt": "keep", "times.tsv": "old\n"})
        self.stage("101", {"a-new.json": "new-a", "z-new.json": "new-z"})

        result = self.run_script(FAKE_GH_RUNS="101",
                                 FAKE_MV_FAIL_SUBSTRING="z-new.json")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("previous reports restored", result.stderr)
        self.assertEqual((self.dest / "a.json").read_text(), "old-a")
        self.assertEqual((self.dest / "b.json").read_text(), "old-b")
        self.assertEqual((self.dest / "extra.txt").read_text(), "keep")
        self.assertEqual((self.dest / "times.tsv").read_text(), "old\n")
        self.assertFalse((self.dest / "a-new.json").exists())
        self.assertFalse((self.dest / "z-new.json").exists())

    def test_publish_failure_restores_overwritten_report_names(self):
        self.write_files(self.dest, {"a.json": "old-a", "b.json": "old-b",
                                     "extra.txt": "keep", "times.tsv": "old\n"})
        self.stage("101", {"a.json": "new-a", "z-new.json": "new-z",
                           "times.tsv": "new\n"})

        result = self.run_script(FAKE_GH_RUNS="101",
                                 FAKE_MV_FAIL_SUBSTRING="z-new.json")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("previous reports restored", result.stderr)
        self.assertEqual((self.dest / "a.json").read_text(), "old-a")
        self.assertEqual((self.dest / "b.json").read_text(), "old-b")
        self.assertEqual((self.dest / "extra.txt").read_text(), "keep")
        self.assertEqual((self.dest / "times.tsv").read_text(), "old\n")
        self.assertFalse((self.dest / "z-new.json").exists())

    def test_timing_publish_failure_restores_reports_and_timing(self):
        self.write_files(self.dest, {"a.json": "old-a", "b.json": "old-b",
                                     "times.tsv": "old\n", "notes.txt": "keep"})
        self.stage("101", {"a.json": "new-a", "new.json": "new",
                           "times.tsv": "new\n"})

        result = self.run_script(FAKE_GH_RUNS="101",
                                 FAKE_MV_FAIL_SUBSTRING="/run/times.tsv")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("previous reports restored", result.stderr)
        self.assertEqual((self.dest / "a.json").read_text(), "old-a")
        self.assertEqual((self.dest / "b.json").read_text(), "old-b")
        self.assertEqual((self.dest / "times.tsv").read_text(), "old\n")
        self.assertEqual((self.dest / "notes.txt").read_text(), "keep")
        self.assertFalse((self.dest / "new.json").exists())

    def test_failed_rollback_keeps_and_reports_the_backup(self):
        self.write_files(self.dest, {"old.json": "old"})
        self.stage("101", {"z-new.json": "new"})

        result = self.run_script(FAKE_GH_RUNS="101",
                                 FAKE_MV_FAIL_SUBSTRING="z-new.json",
                                 FAKE_MV_FAIL_BACKUP="1")

        self.assertNotEqual(result.returncode, 0)
        match = re.search(r"kept in ([^\n]+)", result.stderr)
        self.assertIsNotNone(match, result.stderr)
        backup = Path(match.group(1))
        self.assertTrue(backup.is_dir())
        self.assertEqual((backup / "old.json").read_text(), "old")

    def test_missing_times_keeps_the_previous_times_and_extra_files(self):
        self.write_files(self.dest, {"old.json": "old", "times.tsv": "old\n",
                                     "notes.txt": "keep"})
        self.stage("101", {"new.json": "new"})

        result = self.run_script(FAKE_GH_RUNS="101")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.dest / "new.json").read_text(), "new")
        self.assertFalse((self.dest / "old.json").exists())
        self.assertEqual((self.dest / "times.tsv").read_text(), "old\n")
        self.assertEqual((self.dest / "notes.txt").read_text(), "keep")

    def test_symlinks_are_moved_as_links_and_outside_targets_are_untouched(self):
        outside = self.root / "outside.json"
        outside.write_text("outside")
        self.write_files(self.dest, {"old.json": "old"})
        (self.dest / "linked.json").symlink_to(outside)
        self.stage("101", {"new.json": "new"})
        (self.staging / "101/staged.json").symlink_to(outside)

        result = self.run_script(FAKE_GH_RUNS="101")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.dest / "linked.json").exists())
        self.assertFalse((self.dest / "linked.json").is_symlink())
        self.assertTrue((self.dest / "staged.json").is_symlink())
        self.assertEqual(os.readlink(self.dest / "staged.json"), str(outside))
        self.assertEqual(outside.read_text(), "outside")

    def test_candidate_retry_after_a_failed_download(self):
        self.write_files(self.dest, {"old.json": "old"})
        self.stage("101", {"first.json": "first"})
        self.stage("202", {"second.json": "second"})

        result = self.run_script(FAKE_GH_RUNS="101,202",
                                 FAKE_GH_DOWNLOAD_FAIL="101")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.dest / "second.json").read_text(), "second")
        self.assertFalse((self.dest / "first.json").exists())
        self.assertFalse((self.dest / "old.json").exists())
        log = (self.root / "gh.log").read_text()
        self.assertIn("run\tdownload\t101", log)
        self.assertIn("run\tdownload\t202", log)

    def test_candidate_retry_after_an_empty_artifact(self):
        self.write_files(self.dest, {"old.json": "old"})
        self.stage("101")
        self.stage("202", {"second.json": "second"})

        result = self.run_script(FAKE_GH_RUNS="101,202")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.dest / "second.json").read_text(), "second")
        self.assertFalse((self.dest / "old.json").exists())

    def test_history_fallback_installs_the_committed_set(self):
        self.write_files(self.repo / "reports/difftest", {"hist.json": "hist"})
        self.git("add", "reports/difftest/hist.json")
        self.git("-c", "user.name=Fetch test", "-c", "user.email=fetch@example.invalid",
                 "commit", "--quiet", "-m", "historical reports")
        target = self.root / "install"
        self.write_files(target, {"old.json": "old", "notes.txt": "keep"})

        result = self.run_script(dest=target, FAKE_GH_RUNS="")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((target / "hist.json").read_text(), "hist")
        self.assertFalse((target / "old.json").exists())
        self.assertEqual((target / "notes.txt").read_text(), "keep")


if __name__ == "__main__":
    unittest.main()

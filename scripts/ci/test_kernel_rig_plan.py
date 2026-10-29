#!/usr/bin/env python3
"""Self-test for kernel_rig_plan.py. Run: python3 -I scripts/ci/test_kernel_rig_plan.py (from the
repo root).

Uses a small synthetic tests/kernel-cases/, then the real one: every area has its PATHS and
KNOWN_FAILING, every glob names files that exist, and the workflow's trigger paths cover them all.
"""
import os
import re
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kernel_rig_plan as plan  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
WORKFLOW = os.path.join(ROOT, ".github", "workflows", "kernel-rig.yml")


def write(root, rel, text=""):
    path = os.path.join(root, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)


class Glob(unittest.TestCase):
    def test_star_stays_in_a_folder_and_double_star_crosses(self):
        self.assertTrue(plan.glob_regex("crates/kernel/src/blend*.rs").match("crates/kernel/src/blend_smlib.rs"))
        self.assertFalse(plan.glob_regex("crates/kernel/src/*.rs").match("crates/kernel/src/tess/grid.rs"))
        self.assertTrue(plan.glob_regex("crates/kernel/src/tess/**").match("crates/kernel/src/tess/a/b.rs"))
        self.assertFalse(plan.glob_regex("crates/kernel/src/tess/**").match("crates/kernel/src/tess.rs"))
        self.assertTrue(plan.glob_regex("tests/**/recipe.json").match("tests/recipe.json"))
        self.assertTrue(plan.glob_regex("tests/**/recipe.json").match("tests/a/b/recipe.json"))
        self.assertFalse(plan.glob_regex("a.rs").match("a.rsx"))
        self.assertFalse(plan.glob_regex("a.rs").match("b/a.rs"))


class Synthetic(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        r = self.root = self.tmp.name
        write(r, "tests/kernel-cases/PATHS-ALL", "# shared\ncrates/geom/**\n\n.github/workflows/kernel-rig.yml\n")
        write(r, "tests/kernel-cases/fillets/PATHS", "# blends\ncrates/kernel/src/blend*.rs\n")
        write(r, "tests/kernel-cases/shell/PATHS", "crates/kernel/src/offset*.rs\ncrates/kernel/src/blend_smlib.rs\n")
        write(r, "tests/kernel-cases/ladder/ladder.json", "{}")
        write(r, "tests/kernel-cases/README.md")

    def tearDown(self):
        self.tmp.cleanup()

    def select(self, *paths):
        return plan.select(self.root, list(paths))

    def test_an_area_runs_on_its_paths_with_the_ladder(self):
        self.assertEqual(self.select("crates/kernel/src/blend.rs"), (["fillets", "ladder"], True))
        self.assertEqual(self.select("crates/kernel/src/blend_smlib.rs"), (["fillets", "shell", "ladder"], True))

    def test_a_change_to_an_areas_cases_runs_it(self):
        self.assertEqual(self.select("tests/kernel-cases/shell/1-x/expected.json"), (["shell", "ladder"], True))
        self.assertEqual(self.select("tests/kernel-cases/shell/PATHS"), (["shell", "ladder"], True))
        self.assertEqual(self.select("tests/kernel-cases/ladder/golden.json"), (["ladder"], True))

    def test_the_shared_core_runs_every_area(self):
        for p in ["crates/geom/src/brep.rs", ".github/workflows/kernel-rig.yml"]:
            self.assertEqual(self.select("docs/a.md", p), (["fillets", "shell", "ladder"], True), p)

    def test_unrelated_changes_run_nothing(self):
        self.assertEqual(self.select("crates/ui-egui/src/app.rs", "docs/a.md", "tests/kernel-cases/README.md"), ([], False))

    def test_a_registry_or_case_change_builds_for_the_coverage_check(self):
        self.assertEqual(self.select("crates/engine/src/cmd/view_navigation.rs"), ([], True))
        self.assertEqual(self.select("tests/kernel-cases/EXEMPT"), ([], True))

    def test_everything_runs_every_area(self):
        self.assertEqual(plan.select(self.root, [], everything=True), (["fillets", "shell", "ladder"], True))

    def test_a_new_area_joins_from_its_folder(self):
        write(self.root, "tests/kernel-cases/surfaces/PATHS", "crates/kernel/src/surface_*.rs\n")
        self.assertEqual(self.select("crates/kernel/src/surface_trim.rs"), (["surfaces", "ladder"], True))
        self.assertEqual(plan.select(self.root, [], everything=True)[0], ["fillets", "shell", "surfaces", "ladder"])

    def test_an_area_without_paths_is_an_error(self):
        os.makedirs(os.path.join(self.root, "tests/kernel-cases/plastic"))
        with self.assertRaises(SystemExit) as e:
            self.select("crates/geom/src/brep.rs")
        self.assertIn("plastic/PATHS is missing", str(e.exception))


def trigger_globs():
    """The pull_request trigger paths of the workflow (the push trigger lists the same)."""
    with open(WORKFLOW, encoding="utf-8") as f:
        text = f.read()
    events = {}
    for event in ("pull_request", "push"):
        block = re.search(rf"^  {event}:\n((?:    .*\n|\s*\n)+)", text, re.M)
        assert block, event
        paths = block.group(1).split("paths:", 1)[1]
        events[event] = re.findall(r'^\s+- "([^"]+)"', paths, re.M)
    return events


def sample(glob):
    """A path the glob matches: each wildcard filled in."""
    return glob.replace("**", "x/y").replace("*", "x").replace("?", "x")


class Real(unittest.TestCase):
    def test_every_area_has_its_lists_and_every_glob_names_files(self):
        # Tracked files and new ones not yet committed (not the ignored ones).
        ls = ["git", "ls-files", "--cached", "--others", "--exclude-standard"]
        tracked = subprocess.run(ls, cwd=ROOT, check=True, capture_output=True, text=True).stdout.split("\n")
        shared = plan.read_globs(os.path.join(ROOT, plan.CASES, "PATHS-ALL"))
        areas = plan.areas(ROOT)
        self.assertTrue(areas)
        for area, globs in {"PATHS-ALL": shared, **areas}.items():
            if area != "PATHS-ALL":
                self.assertTrue(os.path.isfile(os.path.join(ROOT, plan.CASES, area, "KNOWN_FAILING")), area)
            for g in globs:
                with self.subTest(area=area, glob=g):
                    rx = plan.glob_regex(g)
                    self.assertTrue(any(rx.match(t) for t in tracked), f"{area}: {g} matches no file")

    def test_the_workflow_triggers_on_every_glob(self):
        events = trigger_globs()
        self.assertEqual(events["pull_request"], events["push"])
        triggers = [plan.glob_regex(g) for g in events["pull_request"]]
        globs = [*plan.read_globs(os.path.join(ROOT, plan.CASES, "PATHS-ALL")), *plan.COVERAGE]
        for area, own in plan.areas(ROOT).items():
            globs += [*own, f"{plan.CASES}/{area}/**"]
        for g in globs:
            with self.subTest(glob=g):
                self.assertTrue(any(t.match(sample(g)) for t in triggers), f"{g}: not under the workflow's trigger paths")

    def test_a_ui_change_runs_nothing(self):
        self.assertEqual(plan.select(ROOT, ["crates/ui-egui/src/app.rs", "docs/kernel-cases.md", "README.md"]), ([], False))


if __name__ == "__main__":
    unittest.main()

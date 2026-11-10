#!/usr/bin/env python3
"""Self-test for main_merge.py. Run: python3 -I scripts/ci/test_main_merge.py (from the repo root).

Builds small throwaway git repositories; needs only git.
"""
import os
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import main_merge  # noqa: E402


class Repo:
    def __init__(self, root):
        self.root = root
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.email", "t@example.com")
        self.git("config", "user.name", "t")
        self.git("config", "commit.gpgsign", "false")
        # No background gc or maintenance: a detached `git gc --auto` writing into .git while
        # tearDown deletes the directory fails the test ("Directory not empty: '.git'").
        self.git("config", "gc.auto", "0")
        self.git("config", "maintenance.auto", "false")

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, capture_output=True, text=True, check=True).stdout.strip()

    def write(self, path, text):
        with open(os.path.join(self.root, path), "w") as f:
            f.write(text)

    def commit(self, msg, **files):
        for path, text in files.items():
            self.write(path, text)
        self.git("add", "-A")
        self.git("commit", "-q", "-m", msg)
        return self.git("rev-parse", "HEAD")


LINES = "".join(f"line {i}\n" for i in range(40))


class SamePullRequest(unittest.TestCase):
    def test_listed_pull_request_must_match(self):
        self.assertTrue(main_merge.same_pull_request({"pull_requests": [{"number": 7}]}, 7, "x/y", "b"))
        self.assertFalse(main_merge.same_pull_request({"pull_requests": [{"number": 8}]}, 7, "x/y", "b"))

    def test_fork_run_matches_by_head_repository_and_branch(self):
        run = {"pull_requests": [], "head_repository": {"full_name": "fork/solidcraft"}, "head_branch": "fix"}
        self.assertTrue(main_merge.same_pull_request(run, 7, "fork/solidcraft", "fix"))
        self.assertFalse(main_merge.same_pull_request(run, 7, "fork/solidcraft", "other"))
        self.assertFalse(main_merge.same_pull_request(run, 7, "someone/solidcraft", "fix"))


class MergeOfMain(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.r = Repo(self.tmp.name)
        self.r.commit("base", **{"a.txt": LINES, "b.txt": LINES})
        self.r.git("checkout", "-q", "-b", "pr")
        self.change = self.r.commit("pr change", **{"a.txt": LINES.replace("line 5\n", "line five\n")})
        self.cwd = os.getcwd()
        os.chdir(self.tmp.name)

    def tearDown(self):
        os.chdir(self.cwd)
        self.tmp.cleanup()

    def main_commit(self, **files):
        self.r.git("checkout", "-q", "main")
        sha = self.r.commit("main moves", **files)
        self.r.git("checkout", "-q", "pr")
        return sha

    def merge_main(self):
        self.r.git("merge", "-q", "--no-edit", "main")
        return self.r.git("rev-parse", "HEAD")

    def test_clean_merge_of_unrelated_main_change_is_carried(self):
        base = self.main_commit(**{"b.txt": LINES.replace("line 30\n", "line thirty\n")})
        head = self.merge_main()
        self.assertEqual(main_merge.merge_of_main(base, head), self.change)

    def test_consecutive_merges_of_main_chain_back_to_the_change(self):
        self.main_commit(**{"b.txt": LINES.replace("line 30\n", "line thirty\n")})
        first = self.merge_main()
        base = self.main_commit(**{"b.txt": LINES.replace("line 30\n", "line thirty\n").replace("line 35\n", "line 35b\n")})
        head = self.merge_main()
        self.assertEqual(main_merge.merge_chain(base, head), [first, self.change])

    def test_chain_stops_at_an_ordinary_commit(self):
        self.main_commit(**{"b.txt": LINES.replace("line 30\n", "line thirty\n")})
        self.merge_main()
        later = self.r.commit("more work", **{"a.txt": LINES.replace("line 5\n", "line five\n").replace("line 20\n", "line twenty\n")})
        base = self.main_commit(**{"b.txt": LINES.replace("line 30\n", "line thirty\n").replace("line 35\n", "line 35b\n")})
        head = self.merge_main()
        self.assertEqual(main_merge.merge_chain(base, head), [later])

    def test_conflict_resolution_is_not_carried(self):
        base = self.main_commit(**{"a.txt": LINES.replace("line 5\n", "line 5 on main\n")})
        subprocess.run(["git", "merge", "-q", "main"], cwd=self.tmp.name, capture_output=True)
        self.r.write("a.txt", LINES.replace("line 5\n", "line five on both\n"))
        self.r.git("add", "-A")
        self.r.git("commit", "-q", "--no-edit")
        head = self.r.git("rev-parse", "HEAD")
        self.assertIsNone(main_merge.merge_of_main(base, head))

    def test_main_change_next_to_the_change_is_not_carried(self):
        # Main edits a context line of the pull request's hunk: the change's patch differs.
        base = self.main_commit(**{"a.txt": LINES.replace("line 7\n", "line seven\n")})
        head = self.merge_main()
        self.assertIsNone(main_merge.merge_of_main(base, head))

    def test_whitespace_change_in_the_change_is_not_carried(self):
        # A conflict resolution that only respaces the change's own line still changes the change.
        base = self.main_commit(**{"a.txt": LINES.replace("line 5\n", "line 5 on main\n")})
        subprocess.run(["git", "merge", "-q", "main"], cwd=self.tmp.name, capture_output=True)
        self.r.write("a.txt", LINES.replace("line 5\n", "line  five\n"))
        self.r.git("add", "-A")
        self.r.git("commit", "-q", "--no-edit")
        head = self.r.git("rev-parse", "HEAD")
        self.assertIsNone(main_merge.merge_of_main(base, head))

    def test_ordinary_commit_is_not_carried(self):
        head = self.r.commit("more", **{"a.txt": LINES.replace("line 5\n", "line 5b\n")})
        base = self.r.git("rev-parse", "main")
        self.assertIsNone(main_merge.merge_of_main(base, head))

    def test_merge_of_another_branch_is_not_carried(self):
        base = self.r.git("rev-parse", "main")
        self.r.git("checkout", "-q", "-b", "other", "main")
        self.r.commit("other work", **{"c.txt": "x\n"})
        self.r.git("checkout", "-q", "pr")
        self.r.git("merge", "-q", "--no-edit", "other")
        head = self.r.git("rev-parse", "HEAD")
        self.assertIsNone(main_merge.merge_of_main(base, head))

    def test_change_with_empty_diff_is_not_carried(self):
        self.r.git("checkout", "-q", "-b", "empty", "main")
        base = self.main_commit(**{"b.txt": LINES.replace("line 30\n", "line thirty\n")})
        self.r.git("checkout", "-q", "empty")
        self.r.git("merge", "-q", "--no-ff", "--no-edit", "main")
        head = self.r.git("rev-parse", "HEAD")
        self.assertIsNone(main_merge.merge_of_main(base, head))


if __name__ == "__main__":
    unittest.main()

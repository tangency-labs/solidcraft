#!/usr/bin/env python3
"""Select the kernel rig areas a pull request affects (.github/workflows/kernel-rig.yml).

Usage: kernel_rig_plan.py (--all | --paths FILE) [--root DIR] [--github-output FILE]

FILE holds the changed paths relative to the repository root, separated by NUL (git diff -z);
"-" reads standard input. --all selects every area (pushes, merge groups, the release gate, a run
by hand). --root is the repository (default: two folders up from this script).

The areas are the folders of tests/kernel-cases/ (the ladder aside), so a new area joins by
having a folder with its cases, its KNOWN_FAILING and its PATHS. A changed path selects:
- every area, when it matches tests/kernel-cases/PATHS-ALL (the shared core: SMLib, geom, the
  kernel's core, the rig's grader and runner, the workflow);
- an area, when it is under tests/kernel-cases/<area>/ or matches that area's PATHS: one git
  pathspec glob a line (`*` within a folder, `**` across folders), `#` starts a comment.
The ladder spans every operation: it runs whenever any area does, and on a change under
tests/kernel-cases/ladder/.

The coverage check (`cargo xtask kernel-cases --coverage`) runs in the build job. The build runs
when a shard does, or when the command registry or the cases may have changed (COVERAGE below),
so a new command without a case fails on its own pull request.

Prints and optionally appends to $GITHUB_OUTPUT:
  shards=["booleans", ..., "ladder"]   the shard matrix, JSON (empty list: no shard runs)
  build=true|false                     the build job (and the coverage check) runs
"""
import argparse
import json
import os
import re
import sys

CASES = "tests/kernel-cases"
LADDER = "ladder"
# Changes here may add or remove a registry command, or the case that covers one: the coverage
# check must see them even when no area runs.
COVERAGE = (
    "crates/engine/src/cmd/**",
    "tests/kernel-cases/EXEMPT",
    "tests/kernel-cases/**/recipe.json",
    "xtask/src/kernel_cases.rs",
)


def glob_regex(glob):
    """A git pathspec glob (`:(glob)` magic) as a regular expression over a whole path: `**/`
    matches any folders (none too), a trailing `/**` everything below, `*` and `?` stay within
    one folder."""
    out, i = [], 0
    while i < len(glob):
        if glob.startswith("**/", i):
            out.append("(?:.*/)?")
            i += 3
        elif glob.startswith("**", i):
            out.append(".*")
            i += 2
        elif glob[i] == "*":
            out.append("[^/]*")
            i += 1
        elif glob[i] == "?":
            out.append("[^/]")
            i += 1
        else:
            out.append(re.escape(glob[i]))
            i += 1
    return re.compile("".join(out) + r"\Z")


def read_globs(path):
    """The globs of a PATHS file: one a line, blank lines and `#` comments skipped."""
    with open(path, encoding="utf-8") as f:
        lines = [line.strip() for line in f]
    globs = [line for line in lines if line and not line.startswith("#")]
    for g in globs:
        if g.startswith("/") or g.startswith(":") or "\\" in g:
            raise SystemExit(f"{path}: `{g}`: a glob is relative to the repository root, with / and no pathspec magic")
    return globs


def areas(root):
    """Every area (a folder of tests/kernel-cases/ but the ladder) with its PATHS globs."""
    base = os.path.join(root, CASES)
    out = {}
    for name in sorted(os.listdir(base)):
        if name == LADDER or not os.path.isdir(os.path.join(base, name)):
            continue
        paths = os.path.join(base, name, "PATHS")
        if not os.path.isfile(paths):
            raise SystemExit(f"{CASES}/{name}/PATHS is missing: every area lists the source files whose change reruns it")
        out[name] = read_globs(paths)
    return out


def matches(globs, path):
    return any(glob_regex(g).match(path) for g in globs)


def select(root, paths, everything=False):
    """Return (shards, build): the areas to run, in order and the ladder last, and whether the
    build job runs."""
    by_area = areas(root)
    if everything:
        return [*by_area, LADDER], True
    shared = read_globs(os.path.join(root, CASES, "PATHS-ALL"))
    hit, ladder = set(), False
    for path in paths:
        if matches(shared, path):
            return [*by_area, LADDER], True
        if path.startswith(f"{CASES}/{LADDER}/"):
            ladder = True
        for area, globs in by_area.items():
            if path.startswith(f"{CASES}/{area}/") or matches(globs, path):
                hit.add(area)
    shards = [a for a in by_area if a in hit]
    if shards or ladder:
        shards.append(LADDER)
    return shards, bool(shards) or any(matches(COVERAGE, p) for p in paths)


def read_paths(src):
    data = sys.stdin.buffer.read() if src == "-" else open(src, "rb").read()
    return [p.decode() for p in data.split(b"\0") if p]


def main():
    ap = argparse.ArgumentParser()
    group = ap.add_mutually_exclusive_group(required=True)
    group.add_argument("--paths")
    group.add_argument("--all", action="store_true")
    ap.add_argument("--root", default=os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
    ap.add_argument("--github-output")
    a = ap.parse_args()
    shards, build = select(a.root, [] if a.all else read_paths(a.paths), everything=a.all)
    lines = [f"shards={json.dumps(shards)}", f"build={'true' if build else 'false'}"]
    print("\n".join(lines))
    if a.github_output:
        with open(a.github_output, "a", encoding="utf-8") as f:
            f.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()

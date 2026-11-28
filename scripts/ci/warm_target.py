#!/usr/bin/env python3
"""Let Cargo reuse a target directory that a Blacksmith sticky disk carries from main.

    python3 -I scripts/ci/warm_target.py prepare --target DIR [--writer]
    python3 -I scripts/ci/warm_target.py seal --target DIR

A fresh checkout gives every file the checkout's time, so Cargo, which compares a source's mtime
with the artefact built from it, would rebuild every workspace crate even when the disk holds
artefacts built from the very same source. `prepare` fixes that against the tree the disk was
built from: it keeps that tree's index (`git ls-files -s`: mode, blob id and path of every
tracked file) in DIR/.warm-index, sets every tracked file and directory (and the git log the
build scripts watch) to a time long before any artefact, then gives every file whose blob differs
from that index, or that the index lacks, the current time. Cargo then rebuilds exactly the crates
whose sources changed since that tree, and what depends on them.

A writer (main) cannot know before the build whether it will finish, so `prepare --writer` adds
every file it marks as changed to DIR/.warm-pending and writes the new index at once; `seal`,
run only when the build passed, clears the pending list. A disk committed after a failed build
still has its pending files marked as changed in the next run, so a crate the failed build left
half-done is rebuilt whatever its file looks like then.

With no index on the disk (a new key, or a disk from before this script) nothing is touched: the
files keep their checkout times and the workspace crates are rebuilt, as without the disk.
A writer whose disk is more than USAGE_LIMIT full empties it first (one cold build).

`prepare` writes `warm=true` to $GITHUB_OUTPUT when the disk held a tree to compare against, and
`warm=false` otherwise, so a job can build less on a cold disk.
"""
import argparse
import os
import shutil
import subprocess
import sys
import time

INDEX = ".warm-index"
PENDING = ".warm-pending"
# 2001-09-09: older than any artefact a runner can have built.
OLD = 1_000_000_000
USAGE_LIMIT = 0.85


def git(*args):
    return subprocess.run(["git", *args], capture_output=True, check=True).stdout


def index_entries(raw):
    """{path: "mode blob"} from `git ls-files -s -z` output."""
    out = {}
    for rec in raw.split(b"\0"):
        if not rec:
            continue
        meta, _, path = rec.partition(b"\t")
        mode, blob, _stage = meta.split(b" ")
        out[path.decode("utf-8", "surrogateescape")] = (mode + b" " + blob).decode()
    return out


def set_time(path, t):
    try:
        os.utime(path, (t, t), follow_symlinks=False)
    except (FileNotFoundError, NotImplementedError, PermissionError):
        pass


def disk_full(target):
    try:
        u = shutil.disk_usage(target)
    except OSError:
        return False
    return u.total > 0 and u.used / u.total > USAGE_LIMIT


def empty_dir(target):
    for name in os.listdir(target):
        p = os.path.join(target, name)
        if name == "lost+found":
            continue
        if os.path.isdir(p) and not os.path.islink(p):
            shutil.rmtree(p, ignore_errors=True)
        else:
            os.unlink(p)


def output(warm):
    path = os.environ.get("GITHUB_OUTPUT")
    if path:
        with open(path, "a", encoding="utf-8") as f:
            f.write(f"warm={'true' if warm else 'false'}\n")


def prepare(target, writer):
    os.makedirs(target, exist_ok=True)
    try:
        u = shutil.disk_usage(target)
        print(f"warm-target: {target}: {u.used / 2**30:.1f} of {u.total / 2**30:.1f} GiB used")
    except OSError:
        pass
    if writer and disk_full(target):
        print(f"warm-target: {target} is over {USAGE_LIMIT:.0%} full; emptying it for a cold build")
        empty_dir(target)
    raw = git("ls-files", "-s", "-z")
    now = index_entries(raw)
    index_path = os.path.join(target, INDEX)
    pending_path = os.path.join(target, PENDING)
    try:
        with open(index_path, "rb") as f:
            old = index_entries(f.read())
    except FileNotFoundError:
        old = None
    pending = set()
    if os.path.exists(pending_path):
        with open(pending_path, encoding="utf-8", errors="surrogateescape") as f:
            pending = {line.rstrip("\n") for line in f if line.strip()}
    if old is None:
        print(f"warm-target: no {INDEX} in {target}: the workspace crates build from scratch")
        changed = set(now)
        if writer:
            write_state(target, raw, changed)
        output(False)
        return
    changed = {p for p, e in now.items() if old.get(p) != e} | (pending & set(now))
    removed = [p for p in old if p not in now] + [p for p in pending if p not in now]
    # Everything tracked, and every directory that holds it (build scripts that watch a
    # directory read its entries' times), to the old time; then what changed to now.
    dirs = set()
    for p in now:
        set_time(p, OLD)
        d = os.path.dirname(p)
        while d and d not in dirs:
            dirs.add(d)
            d = os.path.dirname(d)
    for d in dirs:
        set_time(d, OLD)
    set_time(".", OLD)
    # The build scripts that stamp the commit watch the git log; a new checkout always rewrites it.
    git_dir = git("rev-parse", "--git-dir").decode().strip()
    set_time(os.path.join(git_dir, "logs", "HEAD"), OLD)
    t = time.time()
    for p in changed:
        set_time(p, t)
    for p in removed:
        d = os.path.dirname(p)
        while d and not os.path.isdir(d):
            d = os.path.dirname(d)
        set_time(d or ".", t)
    print(f"warm-target: {len(changed)} file(s) changed and {len(removed)} removed since the tree in {target}")
    for p in sorted(changed)[:40]:
        print(f"  changed: {p}")
    if len(changed) > 40:
        print(f"  ... and {len(changed) - 40} more")
    if writer:
        write_state(target, raw, changed)
    output(True)


def write_state(target, raw, changed):
    pending_path = os.path.join(target, PENDING)
    with open(pending_path, "w", encoding="utf-8", errors="surrogateescape") as f:
        for p in sorted(changed):
            f.write(p + "\n")
    with open(os.path.join(target, INDEX), "wb") as f:
        f.write(raw)


def seal(target):
    p = os.path.join(target, PENDING)
    if os.path.exists(p):
        os.unlink(p)
    print(f"warm-target: {target} sealed at {git('rev-parse', 'HEAD').decode().strip()}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("action", choices=["prepare", "seal"])
    ap.add_argument("--target", required=True)
    ap.add_argument("--writer", action="store_true")
    a = ap.parse_args()
    try:
        if a.action == "prepare":
            prepare(a.target, a.writer)
        else:
            seal(a.target)
    except (subprocess.CalledProcessError, OSError, ValueError) as e:
        # A cache is an optimisation: never fail the job over it. Without the times set, Cargo
        # rebuilds what it cannot prove fresh, which is always safe.
        print(f"warm-target: {a.action} failed ({e}); building without the warm state", file=sys.stderr)


if __name__ == "__main__":
    main()

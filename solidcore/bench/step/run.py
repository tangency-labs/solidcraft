#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""STEP reader bake-off: run one or more readers over the corpus fetch.sh built.

Each reader is LABEL=COMMAND, a command that takes a STEP file as its last argument and prints
JSON with `bodies`: a list of objects with `volume_mm3`, `faces` and `closed` (as
`solidcraft-cli eval FILE` does). A `.p21` file is passed as a `.step` copy.

    bench/step/run.py --corpus bench/step/corpus --out /tmp/bake \\
        --reader base=/path/to/old/solidcraft-cli --reader new=/path/to/new/solidcraft-cli

Writes OUT/per-file.tsv (one row per file, one column group per reader) and OUT/summary.md:
files opened (at least one body), refused, timed out, every body closed, reference volumes matched
(references.tsv, within 0.1%), and wall time per opened file (median, 90th percentile, slowest).
Wall time includes process start, so treat it beside the counts, not as a profile.
"""

import argparse
import concurrent.futures
import json
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))


def run_one(label, command, path, timeout):
    with tempfile.TemporaryDirectory() as tmp:
        name = os.path.basename(path)
        if name.lower().endswith(".p21"):
            name = name[:-4] + ".step"
        local = os.path.join(tmp, name)
        shutil.copyfile(path, local)
        out = os.path.join(tmp, "out.json")
        start = time.monotonic()
        try:
            p = subprocess.run(command + ["eval", local, "--json-out", out], capture_output=True, timeout=timeout, cwd=tmp)
        except subprocess.TimeoutExpired:
            return {"status": "timeout", "wall_s": timeout}
        wall = time.monotonic() - start
        try:
            with open(out) as f:
                data = json.load(f)
        except (OSError, ValueError):
            err = (p.stderr.decode("utf-8", "replace").strip().splitlines() or ["no output"])[-1]
            return {"status": "crashed" if p.returncode < 0 else "refused", "wall_s": wall, "error": err[:200]}
        bodies = data.get("bodies") or []
        if not bodies:
            return {"status": "refused", "wall_s": wall, "error": "no bodies"}
        vol = sum(b.get("volume_mm3") or 0.0 for b in bodies)
        faces = sum(b.get("faces") or 0 for b in bodies)
        closed = sum(1 for b in bodies if b.get("closed", b.get("solid")) is True)
        return {"status": "opened", "bodies": len(bodies), "closed": closed, "faces": faces, "volume": vol, "wall_s": wall}


def references():
    refs = {}
    with open(os.path.join(HERE, "references.tsv")) as f:
        for line in f:
            if line.startswith("#") or not line.strip():
                continue
            sha, vol, _ = line.rstrip("\n").split("\t", 2)
            refs[sha] = float(vol)
    return refs


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--corpus", default=os.path.join(HERE, "corpus"))
    ap.add_argument("--out", required=True)
    ap.add_argument("--reader", action="append", required=True, help="LABEL=COMMAND")
    ap.add_argument("--timeout", type=float, default=120.0)
    ap.add_argument("--jobs", type=int, default=4)
    a = ap.parse_args()
    readers = []
    for r in a.reader:
        label, _, cmd = r.partition("=")
        readers.append((label, cmd.split()))
    with open(os.path.join(a.corpus, "manifest.tsv")) as f:
        files = [line.rstrip("\n").split("\t") for line in f.readlines()[1:]]
    refs = references()
    results = {}
    with concurrent.futures.ThreadPoolExecutor(a.jobs) as pool:
        futs = {pool.submit(run_one, label, cmd, os.path.join(a.corpus, rel), a.timeout): (label, rel) for label, cmd in readers for _, _, rel, _ in files}
        for fut in concurrent.futures.as_completed(futs):
            results[futs[fut]] = fut.result()
    os.makedirs(a.out, exist_ok=True)
    cols = ["status", "bodies", "closed", "faces", "volume", "wall_s", "error"]
    with open(os.path.join(a.out, "per-file.tsv"), "w") as f:
        f.write("\t".join(["file", "section", "ref_volume"] + [f"{l}:{c}" for l, _ in readers for c in cols]) + "\n")
        for sha, section, rel, _ in files:
            row = [rel, section, str(refs.get(sha, ""))]
            for label, _ in readers:
                r = results[(label, rel)]
                row += [str(r.get(c, "")) for c in cols]
            f.write("\t".join(row) + "\n")
    # Files any reader opens: the rest hold no importable solid for anyone.
    live = [(sha, rel) for sha, _, rel, _ in files if any(results[(l, rel)]["status"] == "opened" for l, _ in readers)]
    lines = [f"# STEP reader bake-off\n", f"{len(files)} files; {len(live)} opened by at least one reader; timeout {a.timeout:g} s, {a.jobs} at once.\n",
             "| | " + " | ".join(l for l, _ in readers) + " |", "|---|" + "---|" * len(readers)]

    def row(name, fn):
        lines.append(f"| {name} | " + " | ".join(str(fn(l)) for l, _ in readers) + " |")

    def of(l, st):
        return [rel for _, rel in live if results[(l, rel)]["status"] == st]

    row("opened", lambda l: len(of(l, "opened")))
    row("refused", lambda l: len(of(l, "refused")))
    row("timed out", lambda l: len(of(l, "timeout")))
    row("crashed", lambda l: len(of(l, "crashed")))
    row("opened, every body closed", lambda l: sum(1 for rel in of(l, "opened") if results[(l, rel)]["closed"] == results[(l, rel)]["bodies"]))
    ref_live = [(sha, rel) for sha, rel in live if sha in refs]

    def matched(l):
        n = 0
        for sha, rel in ref_live:
            r = results[(l, rel)]
            if r["status"] == "opened" and abs(r["volume"] - refs[sha]) <= 1e-3 * abs(refs[sha]):
                n += 1
        return f"{n} of {len(ref_live)}"

    row("reference volume within 0.1%", matched)

    def times(l):
        return sorted(results[(l, rel)]["wall_s"] for rel in of(l, "opened"))

    row("median wall time (s)", lambda l: f"{statistics.median(times(l)):.3f}" if times(l) else "-")
    row("90th percentile (s)", lambda l: f"{times(l)[int(0.9 * (len(times(l)) - 1))]:.2f}" if times(l) else "-")
    row("slowest (s)", lambda l: f"{times(l)[-1]:.2f}" if times(l) else "-")
    if len(readers) == 2:
        (la, _), (lb, _) = readers
        lines.append("")
        for name, x, y in ((f"Opened by {la} only", la, lb), (f"Opened by {lb} only", lb, la)):
            only = [rel for _, rel in live if results[(x, rel)]["status"] == "opened" and results[(y, rel)]["status"] != "opened"]
            lines.append(f"{name} ({len(only)}): " + (", ".join(f"`{r}`" for r in only) or "none"))
        lines.append("")
        both = [rel for _, rel in live if results[(la, rel)]["status"] == results[(lb, rel)]["status"] == "opened"]
        diff = [rel for rel in both if abs(results[(la, rel)]["volume"] - results[(lb, rel)]["volume"]) > 1e-3 * max(abs(results[(la, rel)]["volume"]), 1e-9)]
        lines.append(f"Opened by both with volumes more than 0.1% apart ({len(diff)} of {len(both)}): " + (", ".join(f"`{r}`" for r in diff) or "none"))
    with open(os.path.join(a.out, "summary.md"), "w") as f:
        f.write("\n".join(lines) + "\n")
    print("\n".join(lines))


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Symbolise sampler.py output and print self / inclusive tables.

usage: analyze.py PROFILE.json [--top N] [--focus SUBSTR] [--lines] [--libc]
  --focus S : only samples whose stack contains a function matching S
  --lines   : self time by file:line of the innermost frame
  --libc    : also print libc (malloc/memset/...) self samples by Rust caller
Needs gimli addr2line (`cargo install addr2line --features bin`); binutils
addr2line is far too slow on the difftest binary.
"""
import collections
import os
import json
import re
import subprocess
import sys


def short(name):
    name = re.sub(r"::h[0-9a-f]{16}$", "", name)
    return name if len(name) < 160 else name[:157] + "..."


def main():
    path = sys.argv[1]
    top = int(sys.argv[sys.argv.index("--top") + 1]) if "--top" in sys.argv else 40
    focus = sys.argv[sys.argv.index("--focus") + 1] if "--focus" in sys.argv else None
    lines_mode = "--lines" in sys.argv
    with open(path) as f:
        d = json.load(f)
    exe = None
    segs = []
    for ln in d["maps"].splitlines():
        p = ln.split()
        if len(p) < 6:
            continue
        lo, hi = (int(x, 16) for x in p[0].split("-"))
        segs.append((lo, hi, int(p[2], 16), p[5]))
    exe_name = d["exe"].split("/")[-1]
    exe = next(s[3] for s in segs if s[3].endswith("/" + exe_name))
    base = min(lo - off for lo, hi, off, name in segs if name == exe)

    def locate(a):
        for lo, hi, off, name in segs:
            if lo <= a < hi:
                return name
        return None

    addrs = set()
    for st in d["samples"]:
        for i, a in enumerate(st):
            addrs.add(a if i == 0 else a - 1)
    exe_addrs = sorted(a for a in addrs if locate(a) == exe)
    sym = {}
    # addr2line -i prints inline chain innermost first, then the containing function.
    proc = subprocess.run(
        [os.path.expanduser("~/.cargo/bin/addr2line"), "-f", "-C", "-i", "-a", "-e", exe] + [hex(a - base) for a in exe_addrs],
        capture_output=True, text=True)
    cur = None
    out = proc.stdout.splitlines()
    i = 0
    rev = {a - base: a for a in exe_addrs}
    while i < len(out):
        l = out[i]
        if l.startswith("0x") and (i + 1 < len(out)):
            cur = rev.get(int(l, 16))
            sym[cur] = []
            i += 1
            continue
        fn = l
        loc = out[i + 1] if i + 1 < len(out) else "?"
        sym[cur].append((short(fn), re.sub(r".*/(crates|library|src)/", r"\1/", loc)))
        i += 2
    for a in addrs:
        if a not in sym:
            sym[a] = [("[" + (locate(a) or "?").split("/")[-1] + "]", "?")]

    libc_callers = collections.Counter()
    selfc = collections.Counter()
    incl = collections.Counter()
    n = 0
    for st in d["samples"]:
        frames = []
        for i, a in enumerate(st):
            frames.extend(sym[a if i == 0 else a - 1])
        if focus and not any(focus in f for f, _ in frames):
            continue
        n += 1
        selfc[frames[0][1] if lines_mode else frames[0][0]] += 1
        if frames[0][0].startswith("[lib"):
            # innermost non-libc frame plus its caller
            rest = [f for f, _ in frames if not f.startswith("[lib")]
            libc_callers[" <- ".join(rest[:2])] += 1
        for f in set(f for f, _ in frames):
            incl[f] += 1
    print(f"samples: {n} (of {len(d['samples'])})")
    print("\n== self ==")
    for f, c in selfc.most_common(top):
        print(f"{100.0 * c / max(n, 1):6.1f}% {c:6d}  {f}")
    if "--libc" in sys.argv:
        print("\n== libc self samples by caller ==")
        for f, c in libc_callers.most_common(top):
            print(f"{100.0 * c / max(n, 1):6.1f}% {c:6d}  {f}")
    print("\n== inclusive ==")
    for f, c in incl.most_common(top):
        print(f"{100.0 * c / max(n, 1):6.1f}% {c:6d}  {f}")


main()

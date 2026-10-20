#!/usr/bin/env python3
"""Top-function tables from a samply profile (Firefox processed format), no GUI needed.

usage: samply_top.py PROFILE.json.gz [--top N] [--focus SUBSTR] [--thread NAME] [--callees SUBSTR] [--alloc]
Record with `samply record --save-only --unstable-presymbolicate -o P.json.gz -- <cmd>`.
The main binary's frames are expanded to inline level with gimli `addr2line -i`
(`cargo install addr2line --features bin`); other libraries use the `.syms.json`
sidecar (function level). Prints self (innermost frame) and inclusive (once per
sample) percentages.
"""
import bisect
import collections
import gzip
import json
import os
import re
import subprocess
import sys


def short(name):
    name = re.sub(r"::h[0-9a-f]{16}$", "", name)
    name = re.sub(r"<impl [^>]*>", "<impl>", name)
    return name if len(name) < 150 else name[:147] + "..."


def arg(flag, default):
    return sys.argv[sys.argv.index(flag) + 1] if flag in sys.argv else default


def main():
    path = sys.argv[1]
    top = int(arg("--top", "40"))
    focus = arg("--focus", None)
    want_thread = arg("--thread", None)
    prof = json.load(gzip.open(path) if path.endswith(".gz") else open(path))
    syms_path = re.sub(r"\.gz$", "", path) + ".syms.json"
    if not os.path.exists(syms_path):
        syms_path = path.replace(".json.gz", ".json.syms.json")
    syms = json.load(open(syms_path)) if os.path.exists(syms_path) else {"string_table": [], "data": []}
    st = syms["string_table"]
    tables = {}
    for e in syms["data"]:
        rows = sorted((r["rva"], r["size"], r["symbol"]) for r in e["symbol_table"])
        tables[e["debug_name"]] = ([r[0] for r in rows], rows)
    libs = prof["libs"]
    main_lib = None
    # samples: list of frame lists (outermost first) as (libname, rva, is_leaf)
    stacks = collections.Counter()
    for t in prof["threads"]:
        if want_thread and t["name"] != want_thread:
            continue
        ft, stt, res = t["frameTable"], t["stackTable"], t["resourceTable"]
        funcs = t["funcTable"]
        memo = {}

        def frames(s):
            out = []
            while s is not None:
                f = stt["frame"][s]
                func = ft["func"][f]
                r = funcs["resource"][func]
                lib = libs[res["lib"][r]]["debugName"] if r is not None and r >= 0 and res["lib"][r] is not None else "?"
                out.append((lib, ft["address"][f]))
                s = stt["prefix"][s]
            return tuple(reversed(out))

        smp = t["samples"]
        weights = smp.get("weight") or [1] * smp["length"]
        for s, w in zip(smp["stack"], weights):
            if s is None:
                continue
            if s not in memo:
                memo[s] = frames(s)
            stacks[memo[s]] += w or 1
    total = sum(stacks.values())
    exe = next((l for l in libs if l["debugName"] == "difftest"), None)
    addrs = set()
    for fr, _ in stacks.items():
        for i, (lib, a) in enumerate(fr):
            if exe and lib == exe["debugName"] and a >= 0:
                addrs.add(a if i == len(fr) - 1 else a - 1)
    sym = {}
    if exe and addrs:
        alist = sorted(addrs)
        proc = subprocess.run(
            [os.path.expanduser("~/.cargo/bin/addr2line"), "-f", "-C", "-i", "-a", "-e", exe["path"]] + [hex(a) for a in alist],
            capture_output=True, text=True)
        out = proc.stdout.splitlines()
        cur = None
        i = 0
        while i < len(out):
            l = out[i]
            if l.startswith("0x"):
                cur = int(l, 16)
                sym[cur] = []
                i += 1
                continue
            sym[cur].append(short(l))
            i += 2

    def name_of(lib, a, leaf):
        if exe and lib == exe["debugName"]:
            k = a if leaf else a - 1
            chain = sym.get(k)
            if chain:
                return list(reversed(chain))  # outermost first
        tb = tables.get(lib)
        if tb:
            j = bisect.bisect_right(tb[0], a) - 1
            if j >= 0:
                rva, size, si = tb[1][j]
                if a < rva + max(size, 1) and si < len(st):
                    return [short(st[si]) + " [" + lib + "]"]
        return ["[" + lib + "]"]

    selfc = collections.Counter()
    incl = collections.Counter()
    split = collections.Counter()
    callees = collections.Counter()
    allocs = collections.Counter()
    alloc_mode = "--alloc" in sys.argv
    callees_of = arg("--callees", None)
    n = 0
    for fr, w in stacks.items():
        names = []
        for i, (lib, a) in enumerate(fr):
            names.extend(name_of(lib, a, i == len(fr) - 1))
        inner = names[-1] if names else "?"
        cxx = "(" in inner.split("<")[0] or "libstdc++" in inner
        if any("::run_cpp" in x for x in names):
            split["run_cpp"] += w
        elif cxx or any(x.startswith("solidcore_ffi::") for x in names):
            split["C++ outside run_cpp (FFI calls from run_rust, or truncated C++ stacks)"] += w
        elif any(x.startswith("solidcore::") or x.startswith("<solidcore::") for x in names):
            split["Rust kernel (solidcore::)"] += w
        elif any("difftest::" in x for x in names):
            split["harness (difftest)"] += w
        else:
            split["libc / unattributed"] += w
        if focus and not any(focus in x for x in names):
            continue
        n += w
        if callees_of:
            idx = [i for i, x in enumerate(names) if callees_of in x]
            if idx:
                j = idx[-1] + 1
                callees[names[j] if j < len(names) else "(self)"] += w
        selfc[names[-1] if names else "?"] += w
        if alloc_mode and names and re.search(r"malloc|free|realloc|calloc|memset|memcpy|memmove|finish_grow|deallocate|try_allocate_in|__rust_alloc|__rust_dealloc|__rust_realloc|alloc_shim|fun_[0-9a-f]+ \[libc", names[-1]):
            owner = next((x for x in reversed(names) if x.startswith(("solidcore::", "<solidcore::", "difftest::", "<difftest::")) or re.match(r"^[A-Za-z_]+\w*\(", x)), "?")
            allocs[owner] += w
        for x in set(names):
            incl[x] += w
    print(f"samples: {total} total, {n} selected" + (f" (focus {focus!r})" if focus else ""))
    print("\n## split (all samples)\n| % | samples | where |\n|---:|---:|---|")
    for k, v in sorted(split.items(), key=lambda kv: -kv[1]):
        print(f"| {100*v/max(total,1):.1f} | {v} | {k} |")
    if callees_of:
        print(f"\n## callees of {callees_of!r} (next frame below its innermost occurrence)\n| % | samples | callee |\n|---:|---:|---|")
        tot = sum(callees.values())
        for k, v in callees.most_common(top):
            print(f"| {100*v/max(tot,1):.1f} | {v} | `{k}` |")
    if alloc_mode:
        print(f"\n## allocator / memset samples by nearest Rust (or C++) owner\n| % of selected | samples | owner |\n|---:|---:|---|")
        for k, v in allocs.most_common(top):
            print(f"| {100*v/max(n,1):.1f} | {v} | `{k}` |")
    print(f"\n## self (top {top})\n| % | samples | function |\n|---:|---:|---|")
    for k, v in selfc.most_common(top):
        print(f"| {100*v/max(n,1):.1f} | {v} | `{k}` |")
    print(f"\n## inclusive (top {top})\n| % | samples | function |\n|---:|---:|---|")
    for k, v in incl.most_common(top):
        print(f"| {100*v/max(n,1):.1f} | {v} | `{k}` |")


if __name__ == "__main__":
    main()

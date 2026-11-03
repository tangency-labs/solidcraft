#!/usr/bin/env python3
"""Compare independent Rust/native executive dump output at strict relative tolerance."""
import math
import re
import sys

def parse(path, native):
    blocks = []
    current = None
    for line in open(path, encoding="utf-8"):
        if line.startswith("status " if native else "code "):
            pattern = (r"status (\d+) code (\d+) fit (\S+) comment (.*)" if native
                       else r"code (\d+) fit (\S+) comment (.*)")
            m = re.match(pattern, line)
            if native:
                status, code, fit, comment = m.groups()
                assert (int(status) == 1000) == (int(code) == 0), line
            else:
                code, fit, comment = m.groups()
            current = dict(code=int(code), fit=float(fit), comment=comment.strip(), vertices=[])
            blocks.append(current)
        elif current is not None and ("volume_status" in line or line.startswith("check ")):
            m = re.search(r"volume (\S+) faces (\d+) edges (\d+) vertices (\d+)", line)
            if m:
                current["volume"] = float(m[1])
                current["counts"] = tuple(map(int, m.groups()[1:]))
            if not native:
                assert "check []" in line, line
        elif current is not None and line.startswith("v "):
            current["vertices"].append(tuple(map(float, line.split()[1:])))
    return blocks

def close(a, b):
    return math.isfinite(a) and math.isfinite(b) and abs(a-b) <= 1e-9 * max(abs(a), abs(b))

assert sys.argv[3:] in ([], ["--native-left"]), "optional argument: --native-left"
rust, native = parse(sys.argv[1], bool(sys.argv[3:])), parse(sys.argv[2], True)
assert len(rust) == len(native) == 13, (len(rust), len(native))
vertices = bodies = 0
for case, (r, n) in enumerate(zip(rust, native)):
    # The documented pre-existing SM_FILERR_CORNER_UNSUPPORTED numbering difference.
    assert r["code"] == n["code"] or (r["code"], n["code"]) == (9, 10), (case, r, n)
    assert close(r["fit"], n["fit"]), (case, "fit", r["fit"], n["fit"])
    assert r["comment"] == n["comment"], (case, "comment", r["comment"], n["comment"])
    assert ("volume" in r) == ("volume" in n), case
    if "volume" not in r:
        continue
    bodies += 1
    assert r["counts"] == n["counts"], (case, r["counts"], n["counts"])
    assert close(r["volume"], n["volume"]), (case, "volume", r["volume"], n["volume"])
    remaining = n["vertices"].copy()
    for point in r["vertices"]:
        match = next((i for i, other in enumerate(remaining) if all(close(a,b) for a,b in zip(point,other))), None)
        assert match is not None, (case, "unmatched vertex", point, remaining)
        remaining.pop(match)
        vertices += 1
    assert not remaining, (case, remaining)
print(f"PASS native executive: 13 cases, {bodies} bodies, {vertices} vertices; statuses/comments/fit/counts/volumes/coordinates agree at 1e-9 relative")

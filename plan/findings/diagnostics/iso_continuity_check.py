"""Check the fixed/varying coordinate evidence without a geometry solver.

The C^(degree-multiplicity) theorem guarantees these interior labels for the
clamped tensor definitions. This checks the metadata dispatch, not whether a
particular geometric specialization is smoother or an extremum is certified.
Optional --native checks a fresh public-API replay against every frozen result.
"""
import argparse
from collections import Counter
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--records", type=Path, default=Path(__file__).with_name("iso_continuity_native_records.jsonl"))
parser.add_argument("--native", type=Path)
args = parser.parse_args()
records = [json.loads(line) for line in args.records.read_text(encoding="utf-8-sig").splitlines()]
replay = None
if args.native:
    replay = {row["case"]: row["out"] for row in map(json.loads, args.native.read_text(encoding="utf-8-sig").splitlines())}
    assert set(replay) == {row["case"] for row in records}

def labels(surface, coordinate):
    degree = surface["p" if coordinate == "u" else "q"]
    counts = sorted(Counter(surface[coordinate]).items())
    assert counts[0][1] == degree + 1 == counts[-1][1]
    interior = []
    for _, multiplicity in counts[1:-1]:
        order = degree - multiplicity
        assert order >= 0
        interior.append({0: 2, 1: 7, 2: 10, 3: 12}[min(order, 3)])
    return [0, min(interior, default=13), len(counts), 1, *interior, 1]

changed = 0
for row in records:
    curve = row["input"]["curve"]
    fixed = "u" if curve["sp"] == 0 else "v"
    varying = "v" if curve["sp"] == 0 else "u"
    assert row["before"] == labels(curve["srf"], fixed), row["case"]
    assert row["after"] == labels(curve["srf"], varying), row["case"]
    if replay is not None:
        assert replay[row["case"]] == row["after"], row["case"]
    changed += row["before"] != row["after"]
assert len(records) == 120 and changed == 99
print(f"{len(records)} continuity records verified; {changed} corrected, {len(records)-changed} unchanged")

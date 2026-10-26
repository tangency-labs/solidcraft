#!/usr/bin/env python3
"""Fails if two plan/upstream-issues/ records share an ID, or an addendum names no record.

A record is `<SERIES>-<N>--<slug>.md` (e.g. `WR-262--extrusion-drop-point-fast.md`); its ID is
`<SERIES>-<N>`. An addendum is `<SERIES>-<N>--addendum-<YYYY-MM-DD>-<slug>.md` and must name
the ID of an existing record. Names without a numeric ID (`UB-new--`, `UB-unnumbered--`,
`HANG-unnumbered--`, `UNNUMBERED--`) and README.md are not checked.

IDs that were already shared when this check was added are frozen in LEGACY below. Merged
records are never renamed (their IDs are cited elsewhere), so these stay, but no further
record may join them. plan/findings/ has no ID series (names are `<unit>--<slug>.md`), so
nothing there is checked.

Usage: check_record_ids.py [<repo root>]   (default: the current directory)
"""
import re
import sys
from collections import defaultdict
from pathlib import Path

ISSUES = Path("plan/upstream-issues")
RECORD = re.compile(r"^([A-Z]+-[0-9]+)--(.+)\.md$")
ADDENDUM = re.compile(r"^([A-Z]+-[0-9]+)--addendum-[0-9]{4}-[0-9]{2}-[0-9]{2}-.+\.md$")
ADDENDUM_LIKE = re.compile(r"--addendum")

# Records sharing an ID when this check was added (2026-10-26). The `--dupN` names come from
# the archive split (README.md); the others were filed later under an ID already in use.
LEGACY = {
    "UB-100": {
        "UB-100--smderivsurfdefinition-getsamplevalues-dereferences-a-missing.md",
        "UB-100--smpolybrep-calculateboundingbox-dereferences-the-null-point--dup2.md",
    },
    "UB-175": {
        "UB-175--smprototopologymanager-frees-proto-edges-before-the-proto.md",
        "UB-175--tiny-booleans-reach-the-teardown-through-the-public-api.md",
    },
    "UB-185": {
        "UB-185--fillet-executive-initialized-flag-is-indeterminate-before--dup2.md",
        "UB-185--smfilletvertex-calcmategeom-recurses-without-bound-through-a.md",
    },
    "UB-204": {
        "UB-204--smfilletsolver-getsurface-recurses-without-bound-through-the--dup2.md",
        "UB-204--smmatrix-rows-cols-leaves-its-inline-storage-uninitialised.md",
    },
    "UB-210": {
        "UB-210--smbrep-deleteedge-reads-psurvf-outside-the-merge-branch-that.md",
        "UB-210--smfilletedge-calcfilletintfillet-reads-the-context-of-a--dup2.md",
    },
    "UB-238": {
        "UB-238--setintersectioncurvethroughpoint-builds-the-new-uv-trim--dup3.md",
        "UB-238--smbsplinecurve-createcanonical-corrupts-the-heap-for-a-nan.md",
        "UB-238--smfilletgeom-makerailedgeuse-leaves-a-deleted-vertex-use--dup4.md",
        "UB-238--vertex-vertex-and-edge-edge-topology-solvers-dereference-a--dup2.md",
    },
    "UB-256": {
        "UB-256--insert-point-class-stale-interval-reference.md",
        "UB-256--offset-piecewise-merge-deleted-face-uaf.md",
    },
    "WR-190": {
        "WR-190--smfilletedge-calcblendingrail-builds-its-hermite-fallback-as--dup2.md",
        "WR-190--smfilletsolver-recordfilleterror-reports-rail-0-for-rail-1.md",
    },
    "WR-254": {
        "WR-254--manifoldboolean-keeps-a-face-lying-in-the-slicing-sheet-in.md",
        "WR-254--smfilletcorner-makeanalyticcornersurface-reports-its-ser-ner--dup2.md",
    },
}


def check(names, legacy=None):
    """Returns the error messages for the file names of plan/upstream-issues/."""
    legacy = LEGACY if legacy is None else legacy
    records = defaultdict(list)
    addenda = []
    errors = []
    for name in sorted(names):
        if ADDENDUM_LIKE.search(name):
            m = ADDENDUM.match(name)
            if m:
                addenda.append((name, m.group(1)))
            elif RECORD.match(name):
                errors.append(
                    f"{name}: an addendum is named `<ID>--addendum-<YYYY-MM-DD>-<slug>.md`"
                )
            continue
        m = RECORD.match(name)
        if m:
            records[m.group(1)].append(name)
    for rid, files in sorted(records.items()):
        if len(files) < 2 or set(files) <= legacy.get(rid, set()):
            continue
        new = [f for f in files if f not in legacy.get(rid, set())]
        errors.append(
            f"{rid} is used by {len(files)} records: {', '.join(files)}. "
            f"Give {', '.join(new)} the next free ID in its series; to update a record, "
            "add an addendum file instead."
        )
    for name, rid in addenda:
        if rid not in records:
            errors.append(f"{name}: addendum names {rid}, but no record has that ID")
    return errors


def main(argv):
    root = Path(argv[1]) if len(argv) > 1 else Path(".")
    names = [p.name for p in (root / ISSUES).iterdir() if p.is_file()]
    errors = check(names)
    for e in errors:
        print(f"::error file={ISSUES}::{e}")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

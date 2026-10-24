#!/usr/bin/env python3
"""Select every numeric green/partial unit; a missing runner is a hard failure."""
import pathlib
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[2] / "harness/plan"))
from units import load_units


def select(plans, runners):
    selected = [p["id"] for p in plans if p.get("status") in ("green", "partial") and p["id"] != "audit.recursion"]
    missing = set(selected) - set(runners)
    if missing or not selected:
        raise ValueError("missing numeric runners or empty selection: " + repr(sorted(missing)))
    return sorted(selected)


if __name__ == "__main__":
    try:
        listing = subprocess.check_output(["bin/difftest/difftest", "--list"], text=True)
        runners = [line.split("\t")[0] for line in listing.splitlines() if "\t" in line]
        print("\n".join(select(load_units(), runners)))
    except (ValueError, subprocess.CalledProcessError) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)

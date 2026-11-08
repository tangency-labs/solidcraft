#!/usr/bin/env python3
"""Splits the old single-file plan/modules.toml into plan/units/<id>.toml (one per unit).

Each unit's lines are copied verbatim (comment lines that follow a unit's fields stay
with it); every file is checked to parse to exactly the fields of its [[unit]] table.

    python3 scripts/units-split.py [--from plan/modules.toml] [--keep]

Without --keep the source file is removed (`git rm` it yourself when it is tracked).
Unit files that already exist are overwritten; files of units no longer in the source
are left alone (and reported).
"""
import argparse
import pathlib
import sys

sys.dont_write_bytecode = True
ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "harness/plan"))
import units  # noqa: E402


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--from", dest="src", default=str(ROOT / "plan/modules.toml"))
    ap.add_argument("--keep", action="store_true", help="keep the source file")
    a = ap.parse_args()
    src = pathlib.Path(a.src)
    files = units.split_modules_toml(src.read_text())
    out = units.units_dir()
    out.mkdir(parents=True, exist_ok=True)
    for uid, text in files.items():
        units.unit_path(uid).write_text(text)
    stale = sorted(p.stem for p in out.glob("*.toml") if p.stem not in files)
    for s in stale:
        print(f"note: plan/units/{s}.toml is not in {src.name}", file=sys.stderr)
    if not a.keep:
        src.unlink()
    print(f"{len(files)} unit files -> {out.relative_to(ROOT)}/")


if __name__ == "__main__":
    main()

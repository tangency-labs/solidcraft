#!/usr/bin/env python3
"""Parked difftest cases against upstream's latest C++ (the parked-upstream workflow).

A known-mismatch entry with `cpp_defect = "<id>"` is parked: the gate does not run it
(crates/difftest/README.md, "Known mismatches and parked C++ defects"). When upstream
usd-brep changes (a new tag or a new HEAD), the workflow builds the C++ reference from
that commit and runs `difftest --run-parked` on the units this script lists; a parked case
that now agrees with Rust is reported as an ordinary `pass`, and this script writes the
title and body of the one tracking issue,
"usd-brep <ref> released: N parked cases now agree, adopt?" (or "... none changed").

  parked_upstream.py units [--root DIR]
      the unit ids with at least one parked case, one per line
  parked_upstream.py issue --report-dir DIR --upstream-ref REF --upstream-sha SHA
                          --title-out FILE --body-out FILE [--root DIR]
      writes the issue title and body; exit 2 when a unit's report is missing (the run
      did not produce it)
"""

import argparse
import json
import pathlib
import sys
import tomllib

KNOWN_DIR = "crates/difftest/known_mismatches"

# Every title starts with this and contains TITLE_MARK, so the workflow finds the one
# open tracking issue again whatever the ref and count.
TITLE_PREFIX = "usd-brep "
TITLE_MARK = " released: "


def parked_cases(root):
    """{unit: {case: cpp_defect id}} for every entry that has a `cpp_defect`."""
    out = {}
    for path in sorted((root / KNOWN_DIR).glob("*.toml")):
        with path.open("rb") as f:
            data = tomllib.load(f)
        for entry in data.get("mismatch", []):
            defect = entry.get("cpp_defect")
            if defect:
                out.setdefault(entry["unit"], {})[entry["case"]] = defect
    return out


def agreeing(report_dir, parked):
    """[(unit, case, id)] of parked cases whose `--run-parked` outcome is `pass`, and the
    units without a report."""
    found, missing = [], []
    for unit, cases in sorted(parked.items()):
        path = report_dir / f"{unit}.json"
        if not path.is_file():
            missing.append(unit)
            continue
        report = json.loads(path.read_text())
        for c in report.get("cases", []):
            if c.get("outcome") == "pass" and c.get("name") in cases:
                found.append((unit, c["name"], cases[c["name"]]))
    return found, missing


def title(found, ref):
    if not found:
        return f"{TITLE_PREFIX}{ref}{TITLE_MARK}none changed"
    n = len(found)
    cases = "case now agrees" if n == 1 else "cases now agree"
    return f"{TITLE_PREFIX}{ref}{TITLE_MARK}{n} parked {cases}, adopt?"


def body(found, parked, ref, sha):
    total = sum(len(c) for c in parked.values())
    lines = [
        f"Upstream usd-brep changed (`{ref}`, commit `{sha}`). The parked-upstream "
        f"workflow built the C++ reference from it and ran the {total} parked difftest "
        "case(s) (known mismatches with a `cpp_defect`).",
        "",
    ]
    if not found:
        lines.append("None of them agrees with Rust yet: nothing to adopt.")
    else:
        lines += [
            "These now agree with Rust:",
            "",
            "| unit | case | upstream issue |",
            "|---|---|---|",
        ]
        lines += [f"| `{u}` | `{c}` | `{i}` |" for u, c, i in found]
        lines += [
            "",
            "To adopt a fix once our pin moves to that upstream commit: delete the case's "
            "`cpp_defect` line (or its whole entry) in "
            "`crates/difftest/known_mismatches/<unit>.toml`, and update the upstream issue "
            "record in `plan/upstream-issues/`.",
        ]
    lines += ["", "This issue is rewritten each time upstream changes."]
    return "\n".join(lines) + "\n"


def main(argv=None):
    # (no docstring under `python3 -OO`)
    summary = (__doc__ or "").strip().splitlines()
    p = argparse.ArgumentParser(description=summary[0] if summary else None)
    p.add_argument("command", choices=["units", "issue"])
    p.add_argument("--root", type=pathlib.Path, default=pathlib.Path("."))
    p.add_argument("--report-dir", type=pathlib.Path)
    p.add_argument("--upstream-ref", default="HEAD")
    p.add_argument("--upstream-sha", default="unknown")
    p.add_argument("--title-out", type=pathlib.Path)
    p.add_argument("--body-out", type=pathlib.Path)
    a = p.parse_args(argv)
    parked = parked_cases(a.root)
    if a.command == "units":
        for unit in sorted(parked):
            print(unit)
        return 0
    if a.report_dir is None or a.title_out is None or a.body_out is None:
        p.error("issue needs --report-dir, --title-out and --body-out")
    found, missing = agreeing(a.report_dir, parked)
    if missing:
        print("parked_upstream: no report for " + ", ".join(missing), file=sys.stderr)
        return 2
    a.title_out.write_text(title(found, a.upstream_ref) + "\n")
    a.body_out.write_text(body(found, parked, a.upstream_ref, a.upstream_sha))
    return 0


if __name__ == "__main__":
    sys.exit(main())

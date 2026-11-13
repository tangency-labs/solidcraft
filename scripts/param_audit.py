#!/usr/bin/env python3
"""Summarise a parameter-contract audit: the calls that break a command's contract, by issue.

Usage: param_audit.py AUDIT_DIR [OUT_JSON]

AUDIT_DIR is the directory `SOLIDCRAFT_PARAM_AUDIT` named for a test run built with the engine's
`param-audit` feature (see crates/engine/src/schema/record.rs). Each process writes one
audit-<pid>.jsonl, repeating a signature's line as its count grows, so this keeps the highest
count per signature in each file and adds the files up.

The report has three parts:
  SUCCEEDS TODAY  calls that work now and that strict would refuse: a form the contract is
                  missing (fix the contract) or a mistake the handler ignores (fix the caller)
  ALREADY FAILS   calls the handler rejects anyway
  REFUSED         calls strict refused (tests that run with strict on)

OUT_JSON, when given, receives every signature with its summed count.
"""
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path


def load(audit_dir):
    signatures = {}
    malformed = []
    files = sorted(audit_dir.glob("audit-*.jsonl"))
    for f in files:
        per_file = {}
        for n, line in enumerate(f.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
            if not line.strip():
                continue
            try:
                r = json.loads(line)
                key = json.dumps([r["command"], r["issues"], r["outcome"], r["source"]])
            except (json.JSONDecodeError, KeyError) as e:
                malformed.append(f"{f.name}:{n}: {e}")
                continue
            if key not in per_file or r["count"] > per_file[key]["count"]:
                per_file[key] = r
        for key, r in per_file.items():
            if key in signatures:
                signatures[key] = {**signatures[key], "count": signatures[key]["count"] + r["count"]}
            else:
                signatures[key] = dict(r)
    return files, signatures, malformed


def by_issue(records):
    counts = Counter()
    examples = defaultdict(list)
    for r in records:
        for issue in r["issues"]:
            counts[(r["command"], issue)] += r["count"]
            if len(examples[(r["command"], issue)]) < 3:
                examples[(r["command"], issue)].append(f"{r['source']}: {r['sample_params'][:300]}")
    return counts, examples


def main():
    if len(sys.argv) not in (2, 3):
        sys.exit(__doc__.split("\n\n")[1])
    files, signatures, malformed = load(Path(sys.argv[1]))
    records = list(signatures.values())
    parts = [
        ("SUCCEEDS TODAY, STRICT WOULD REFUSE", [r for r in records if r["outcome"] == "ok"], True),
        ("ALREADY FAILS TODAY", [r for r in records if r["outcome"] not in ("ok", "refused")], False),
        ("REFUSED BY STRICT", [r for r in records if r["outcome"] == "refused"], False),
    ]
    print(f"files: {len(files)}  signatures: {len(signatures)}  malformed lines: {len(malformed)}")
    for m in malformed[:10]:
        print("  malformed:", m)
    for label, part, show_examples in parts:
        counts, examples = by_issue(part)
        print(f"\n== {label}: {len(part)} signatures, at least {sum(r['count'] for r in part)} calls")
        for (command, issue), n in counts.most_common():
            print(f"  {n:>7}  {command:14}  {issue}")
            if show_examples:
                for e in examples[(command, issue)]:
                    print(f"           e.g. {e}")
    if len(sys.argv) == 3:
        out = {"signatures": records, "malformed": malformed}
        Path(sys.argv[2]).write_text(json.dumps(out, indent=1), encoding="utf-8")


if __name__ == "__main__":
    main()

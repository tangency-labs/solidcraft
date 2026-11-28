"""Summarise bench results (JSONL from run.py): per setup, per task set, and the comparisons the
experiment asks for.

    python bench/report.py bench/results/haiku.jsonl [--markdown out.md]
"""

from __future__ import annotations

import json
import statistics
import sys
from collections import defaultdict

ARMS = {
    "A": "Plain MCP",
    "B": "scx v15.1 (math only)",
    "C": "scx + scientific stack",
    "D": "Full API (Python)",
    "E": "Full API + scx vocabulary",
}
SETS = ["simple", "big", "relations", "edit", "numeric", "realworld"]


def load(paths: list[str]) -> list[dict]:
    rows = []
    for p in paths:
        with open(p, encoding="utf-8") as f:
            rows += [json.loads(line) for line in f if line.strip()]
    return rows


def mean(xs: list[float]) -> float:
    return statistics.fmean(xs) if xs else float("nan")


def summary(rows: list[dict]) -> dict:
    return {
        "runs": len(rows),
        "pass": sum(r["pass"] for r in rows),
        "cost": mean([r["cost_usd"] or 0 for r in rows]),
        "input": mean([r["input_tokens"] for r in rows]),
        "output": mean([r["output_tokens"] or 0 for r in rows]),
        "calls": mean([r["tool_calls"] for r in rows]),
        "wall": mean([r["wall_s"] for r in rows]),
        "stack": sum(bool(r.get("uses_stack")) for r in rows),
        "checks": mean([r["score"] for r in rows if "score" in r]) if any("score" in r for r in rows) else None,
    }


def table(rows: list[dict], key, title: str) -> list[str]:
    scored = any("score" in r for r in rows)
    out = [
        f"### {title}",
        "",
        "| Setup | Passes |" + (" Checks passed |" if scored else "") + " Cost / task | Input tokens / task | Output tokens | Tool calls | Wall s | Used numpy/scipy/pandas |",
        "|---|---:|" + ("---:|" if scored else "") + "---:|---:|---:|---:|---:|---:|",
    ]
    by = defaultdict(list)
    for r in rows:
        by[key(r)].append(r)
    base = summary(by.get("A", [])) if "A" in by else None
    for arm in ARMS:
        if arm not in by:
            continue
        s = summary(by[arm])
        rel = lambda k: f" ({(s[k] / base[k] - 1) * 100:+.0f}%)" if base and base[k] and arm != "A" else ""  # noqa: E731
        out.append(
            f"| {arm} {ARMS[arm]} | {s['pass']}/{s['runs']} |" + (f" {s['checks']:.1%} |" if scored else "") + f" {s['cost'] * 100:.2f}¢{rel('cost')} | {s['input'] / 1000:.0f}k{rel('input')} | {s['output']:.0f} | {s['calls']:.1f} | {s['wall']:.0f} | {s['stack']} |"
        )
    return out + [""]


def pairs(rows: list[dict]) -> list[str]:
    """The experiment's three comparisons, on input tokens and cost, overall and per set."""
    out = ["### Comparisons (change in mean cost and input tokens; passes)", "", "| Comparison | What it measures | Set | Cost | Input tokens | Passes |", "|---|---|---|---:|---:|---|"]
    by = defaultdict(list)
    for r in rows:
        by[(r["arm"], r["set"])].append(r)
        by[(r["arm"], "all")].append(r)
    for a, b, what in [("B", "C", "the scientific stack, same vocabulary"), ("D", "E", "the vocabulary, on a full API"), ("C", "D", "full API without vocabulary vs scx + stack"), ("A", "D", "full API vs plain MCP")]:
        for st in ["all", *SETS]:
            ra, rb = by.get((a, st)), by.get((b, st))
            if not ra or not rb:
                continue
            sa, sb = summary(ra), summary(rb)
            d = lambda k: f"{(sb[k] / sa[k] - 1) * 100:+.0f}%" if sa[k] else "n/a"  # noqa: E731
            out.append(f"| {a} → {b} | {what} | {st} | {d('cost')} | {d('input')} | {sa['pass']}/{sa['runs']} → {sb['pass']}/{sb['runs']} |")
    return out + [""]


def per_task(rows: list[dict]) -> list[str]:
    arms = [a for a in ARMS if any(r["arm"] == a for r in rows)]
    out = ["### Per task (passes, mean cost in ¢)", "", "| Set | Task | " + " | ".join(arms) + " |", "|---|---|" + "---:|" * len(arms)]
    by = defaultdict(list)
    for r in rows:
        by[(r["set"], r["task"], r["arm"])].append(r)
    for st, task in sorted({(r["set"], r["task"]) for r in rows}, key=lambda x: (SETS.index(x[0]) if x[0] in SETS else 99, x[1])):
        cells = []
        for a in arms:
            rs = by.get((st, task, a), [])
            cells.append(f"{sum(r['pass'] for r in rs)}/{len(rs)}, {mean([r['cost_usd'] or 0 for r in rs]) * 100:.2f}" if rs else "")
        out.append(f"| {st} | {task} | " + " | ".join(cells) + " |")
    return out + [""]


def failures(rows: list[dict]) -> list[str]:
    out = ["### Failures", ""]
    for r in sorted((r for r in rows if not r["pass"]), key=lambda r: (r["arm"], r["set"], r["task"], r["rep"])):
        out.append(f"- {r['arm']} {r['set']}/{r['task']} r{r['rep']}: {r['detail']}{' (timed out)' if r.get('timed_out') else ''}")
    return out + [""]


def main() -> int:
    args = sys.argv[1:]
    md = None
    if "--markdown" in args:
        i = args.index("--markdown")
        md = args[i + 1]
        del args[i : i + 2]
    rows = load(args)
    models = sorted({r["model"] for r in rows})
    lines = [f"## Results: {', '.join(models)}, {len(rows)} runs", ""]
    lines += table(rows, lambda r: r["arm"], "All tasks")
    for st in SETS:
        sub = [r for r in rows if r["set"] == st]
        if sub:
            lines += table(sub, lambda r: r["arm"], f"Set: {st}")
    lines += pairs(rows) + per_task(rows) + failures(rows)
    text = "\n".join(lines)
    print(text)
    if md:
        with open(md, "w", encoding="utf-8") as f:
            f.write(text + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())

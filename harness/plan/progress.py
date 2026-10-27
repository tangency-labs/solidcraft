#!/usr/bin/env python3
"""Regenerate plan/PROGRESS.md from plan/units/*.toml and the difftest reports.

The Pages workflow (.github/workflows/pages.yml) runs this on main and publishes the result
on the site as PROGRESS.md; plan/PROGRESS.md is gitignored and never committed. Locally:
    python3 harness/plan/progress.py          # from plan/units and reports/difftest/
    python3 harness/plan/progress.py --prs    # also mark units with an open "port: <id>" PR
Output is deterministic (no timestamps); statuses, claims and report counts drive changes.
Reports are not committed: CI uploads them from main (artifact `difftest-reports`) and
`scripts/fetch-reports.sh` downloads the latest set into reports/difftest/.

The counting helpers (`summary`, `is_ready`, `module`, `DONE`) are also imported by the
GitHub Pages site generator (site/build.py), so both report the same numbers.
"""
import collections
import json
import subprocess
import sys
import pathlib
import re

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import units as plan_units  # noqa: E402  (the shared plan loader)

ROOT = pathlib.Path(__file__).resolve().parents[2]
DONE = ("green", "partial")
ORDER = ["geom", "nurbs", "curve", "surface", "topology", "intersect", "primitive",
         "query", "mass", "sweep", "heal", "offset", "boolean", "blend"]
ICON = {"green": "✅", "partial": "🟡", "claimed": "🔨", "blocked": "⛔", "todo": "⏳"}


def load_units():
    """Units from plan/units/*.toml (sorted by id). `est_lines` is recomputed from
    harness/callgraph/units.json (the functions each unit owns at the pinned upstream
    commit, scripts/upstream-pin.env) for every unit listed there, so the line counts
    follow the pin; units the call graph does not list keep their toml value."""
    units = plan_units.load_units(ROOT)
    lines = plan_units.callgraph_lines(ROOT)
    for u in units:
        if u["id"] in lines:
            u["est_lines"] = lines[u["id"]]
    return units


def load_case_counts(units):
    """Default-run counts from reports/difftest/; omit units without a report.

    Notes can describe stress runs or contain operation numbers (e.g. "op-0 cases"),
    so they are not a source for the published case count. Reject invalid reports
    before replacing the checklist, rather than publishing a misleading number.
    """
    counts = {}
    for u in units:
        if u["status"] not in DONE:
            continue
        path = ROOT / "reports/difftest" / f"{u['id']}.json"
        if not path.exists():
            continue
        try:
            report = json.loads(path.read_text())
        except json.JSONDecodeError as exc:
            raise ValueError(f"{path}: invalid difftest report") from exc
        if not isinstance(report, dict) or report.get("unit") != u["id"]:
            raise ValueError(f"{path}: report unit must match {u['id']!r}")
        count = report.get("cases_total")
        if type(count) is not int or count < 0:
            raise ValueError(f"{path}: cases_total must be a non-negative integer")
        counts[u["id"]] = count
    return counts


def module(u):
    return u["rust_module"].split("::")[0]


def is_ready(u, status):
    """A todo unit whose depends_on are all green/partial (`status`: id -> status)."""
    return u["status"] == "todo" and all(status.get(d) in DONE for d in u["depends_on"])


def summary(units):
    """Headline numbers: unit counts by status and upstream lines ported."""
    total = sum(u["est_lines"] for u in units)
    done = sum(u["est_lines"] for u in units if u["status"] in DONE)
    green_lines = sum(u["est_lines"] for u in units if u["status"] == "green")
    count = collections.Counter(u["status"] for u in units)
    return {"units": len(units), "total_lines": total, "done_lines": done,
            "green_lines": green_lines, "count": count}


def main():
    units = load_units()
    if "--prs" in sys.argv:
        prs = json.loads(subprocess.run(
            ["gh", "pr", "list", "--state", "open", "--limit", "500",
             "--json", "number,title,author,isDraft"],
            check=True, capture_output=True, text=True).stdout)
        by_unit = {}
        for pr in sorted(prs, key=lambda p: p["number"]):
            m = re.fullmatch(r"port:\s*(\S+)\s*", pr["title"])
            if m and m.group(1) not in by_unit:
                by_unit[m.group(1)] = pr
        for u in units:
            pr = by_unit.get(u["id"])
            if pr and u["status"] == "todo":
                u["status"] = "claimed"
                u["claimed_by"] = f"#{pr['number']} @{pr['author']['login']}" + (" (draft)" if pr["isDraft"] else " (ready for review)")
                u["claimed_at"] = ""
    case_counts = load_case_counts(units)
    status = {u["id"]: u["status"] for u in units}

    def ready(u):
        return is_ready(u, status)

    def cases(u):
        count = case_counts.get(u["id"])
        return f", {count} cases" if count is not None else ""

    def line(u):
        s = u["status"]
        box = "x" if s == "green" else " "
        tag = {"green": f"green{cases(u)}", "partial": f"partial{cases(u)}",
               "claimed": f"in progress: {u['claimed_by']}" + (f" since {u['claimed_at']}" if u["claimed_at"] else ""),
               "blocked": "blocked (see notes / plan/findings.md)"}.get(s, "ready" if ready(u) else "waiting")
        icon = "▶️" if ready(u) else ICON.get(s, "")
        return f"- [{box}] {icon} `{u['id']}` ({u['est_lines']} lines) — {tag}"

    s = summary(units)
    total, done, count = s["total_lines"], s["done_lines"], s["count"]
    out = [
        "# Port progress",
        "",
        "Generated from `plan/units/*.toml` and CI's `reports/difftest/*.json` by "
        "`harness/plan/progress.py`; do not edit by hand.",
        "",
        f"**{count['green'] + count['partial']} / {len(units)} units ported** "
        f"({count['green']} green, {count['partial']} partial), "
        f"**{done:,} / {total:,} upstream lines ({100 * done / total:.1f}%)**. "
        f"{count['claimed']} claimed, {count['blocked']} blocked, {count['todo']} todo.",
        "",
        "Legend: ✅ green (identical to C++) · 🟡 partial (passes; some cases wait on unported code) · "
        "🔨 claimed · ▶️ ready (all dependencies done) · ⏳ claimable, dependencies still missing · ⛔ blocked.",
        "",
        "## Ready to claim now",
        "",
    ]
    rdy = sorted((u for u in units if ready(u)), key=lambda u: (u["layer"], u["id"]))
    out += [line(u) for u in rdy] or ["_None right now; see the next section._"]
    out += ["", "## Claimable now, dependencies still missing", "",
            "Anything `todo` can be claimed. These need stubs for, or co-porting of, the "
            "listed dependencies (AGENTS.md section 4).", ""]
    waiting = sorted((u for u in units if u["status"] == "todo" and not ready(u)),
                     key=lambda u: (u["layer"], u["id"]))
    for u in waiting:
        miss = [d for d in u["depends_on"] if status.get(d) not in DONE]
        shown = ", ".join(f"`{d}`" for d in miss[:4]) + (f" +{len(miss) - 4} more" if len(miss) > 4 else "")
        out.append(f"- [ ] `{u['id']}` ({u['est_lines']} lines) — missing: {shown}")
    out += ["", "## In progress", ""]
    out += [line(u) for u in units if u["status"] == "claimed"] or ["_None._"]
    out += ["", "## By module", ""]
    mods = ORDER + sorted({module(u) for u in units} - set(ORDER))
    for m in mods:
        us = [u for u in units if module(u) == m]
        if not us:
            continue
        d = sum(1 for u in us if u["status"] in DONE)
        out += [f"### {m} ({d}/{len(us)})", ""]
        out += [line(u) for u in sorted(us, key=lambda u: (u["layer"], u["id"]))]
        out.append("")
    (ROOT / "plan/PROGRESS.md").write_text("\n".join(out).rstrip() + "\n")


if __name__ == "__main__":
    main()

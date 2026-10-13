#!/usr/bin/env python3
"""Regenerate plan/PROGRESS.md (a human-readable checklist) from plan/modules.toml.

CI (.github/workflows/progress.yml) runs this on main and commits the result; PRs never
edit plan/PROGRESS.md. Locally:
    python3 harness/plan/progress.py          # from plan/modules.toml only
    python3 harness/plan/progress.py --prs    # also mark units with an open "port: <id>" PR
Output is deterministic (no timestamps), so it only changes when statuses or claims change.
"""
import collections
import json
import subprocess
import sys
import pathlib
import re
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[2]
units = tomllib.loads((ROOT / "plan/modules.toml").read_text())["unit"]
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
status = {u["id"]: u["status"] for u in units}
DONE = ("green", "partial")
ORDER = ["geom", "nurbs", "curve", "surface", "topology", "intersect", "primitive",
         "query", "mass", "sweep", "heal", "offset", "boolean", "blend"]
ICON = {"green": "✅", "partial": "🟡", "claimed": "🔨", "blocked": "⛔", "todo": "⏳"}


def module(u):
    return u["rust_module"].split("::")[0]


def ready(u):
    return u["status"] == "todo" and all(status.get(d) in DONE for d in u["depends_on"])


def cases(u):
    m = re.search(r"(\d+) cases", u["notes"])
    return f", {m.group(1)} cases" if m else ""


def line(u):
    s = u["status"]
    box = "x" if s == "green" else " "
    tag = {"green": f"green{cases(u)}", "partial": f"partial{cases(u)}",
           "claimed": f"in progress: {u['claimed_by']}" + (f" since {u['claimed_at']}" if u["claimed_at"] else ""),
           "blocked": "blocked (see notes / plan/findings.md)"}.get(s, "ready" if ready(u) else "waiting")
    icon = "▶️" if ready(u) else ICON.get(s, "")
    return f"- [{box}] {icon} `{u['id']}` ({u['est_lines']} lines) — {tag}"


total = sum(u["est_lines"] for u in units)
done = sum(u["est_lines"] for u in units if u["status"] in DONE)
count = collections.Counter(u["status"] for u in units)
out = [
    "# Port progress",
    "",
    "Generated from `plan/modules.toml` by `harness/plan/progress.py`; do not edit by hand.",
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

#!/usr/bin/env python3
"""Build the solidcore progress site (GitHub Pages) from data already in the repo.

    python3 site/build.py                 # -> bin/site/
    python3 site/build.py --out DIR       # elsewhere

Standard library only (Python 3.11+ for tomllib). Every number comes from the repo:
plan/modules.toml, plan/PROGRESS.md (claims), harness/callgraph/units.json,
reports/difftest/*.json, reports/ssi-corpus.json, plan/upstream-issues.md, the
module file headers under crates/solidcore/src and git history (timeline). Output is
deterministic: sorted, and the only date is the commit date of HEAD.

All repo text (notes, issue entries, headers) is HTML-escaped before a small markdown
subset (inline code, bold, italic, http(s) links, nested bullet lists) is applied, so no
raw HTML from the data reaches the page.
"""
import argparse
import collections
import html
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.dont_write_bytecode = True  # keep harness/plan clean
sys.path.insert(0, str(ROOT / "harness/plan"))
import progress  # noqa: E402  (reuse the checklist's counting logic)

REPO = os.environ.get("GITHUB_REPOSITORY", "bherbruck/solidcore")
REPO_URL = f"https://github.com/{REPO}"
UPSTREAM_URL = "https://github.com/NVIDIA-Omniverse/usd-brep"
UPSTREAM_COMMIT = "c9c5979f04cea8c38213004d4f04df0dcfb40d05"
STATUSES = ["green", "partial", "claimed", "blocked", "todo"]
STATUS_TEXT = {"green": "green", "partial": "partial", "claimed": "claimed",
               "blocked": "blocked", "todo": "todo"}
SERIES = ["UB", "HANG", "ND", "WR", "DOC"]
SERIES_TEXT = {"UB": "Crash / undefined behaviour", "HANG": "Hang", "ND": "Nondeterminism",
               "WR": "Wrong result", "DOC": "Docs"}
MAX_TIMELINE_POINTS = 120

esc = html.escape


# ---------------------------------------------------------------- git

def git(*args, inp=None):
    return subprocess.run(["git", "-C", str(ROOT), *args], input=inp, check=True,
                          capture_output=True, text=True).stdout


def head_info():
    sha, date = git("log", "-1", "--format=%H %cI").split()
    return sha, date


def timeline():
    """[(unix time, summary)] for plan/modules.toml along main's first-parent history.

    Consecutive commits with the same modules.toml blob are dropped, then the history is
    thinned to at most MAX_TIMELINE_POINTS states (the last state in each time bucket),
    always keeping the first and last.
    """
    log = git("log", "--first-parent", "--format=%H %ct", "HEAD").split("\n")
    commits = [tuple(line.split()) for line in reversed(log) if line.strip()]
    if not commits:
        return []
    req = "".join(f"{h}:plan/modules.toml\n" for h, _ in commits)
    checks = git("cat-file", "--batch-check", inp=req).split("\n")
    states = []  # (time, blob)
    last = None
    for (h, t), chk in zip(commits, checks):
        parts = chk.split()
        if len(parts) < 2 or parts[1] != "blob":
            continue
        if parts[0] != last:
            states.append((int(t), parts[0]))
            last = parts[0]
    if len(states) > MAX_TIMELINE_POINTS:
        t0, t1 = states[0][0], states[-1][0]
        width = max(1, (t1 - t0) / (MAX_TIMELINE_POINTS - 1))
        buckets = {}
        for t, b in states:
            buckets[int((t - t0) / width)] = (t, b)
        thinned = [states[0]] + [buckets[k] for k in sorted(buckets)]
        states = sorted(set(thinned), key=lambda s: s[0])
    raw = subprocess.run(["git", "-C", str(ROOT), "cat-file", "--batch"], check=True,
                         capture_output=True, input="".join(f"{b}\n" for _, b in states).encode()).stdout
    out, pos = [], 0
    for t, _ in states:
        nl = raw.index(b"\n", pos)
        size = int(raw[pos:nl].split()[2])
        text = raw[nl + 1:nl + 1 + size].decode()
        pos = nl + 1 + size + 1
        try:
            units = progress.load_units(text)
        except Exception:  # an early revision may not parse; skip it
            continue
        out.append((t, progress.summary(units)))
    return out


# ---------------------------------------------------------------- data

def load_claims():
    """id -> claim text from the bot-maintained "In progress" section of PROGRESS.md."""
    claims = {}
    path = ROOT / "plan/PROGRESS.md"
    if not path.exists():
        return claims
    section = False
    for line in path.read_text().splitlines():
        if line.startswith("## "):
            section = line.strip() == "## In progress"
            continue
        m = section and re.match(r"- \[.\] \S* ?`([^`]+)` \(\d+ lines\) — in progress: (.*)$", line)
        if m:
            claims[m.group(1)] = m.group(2).strip()
    return claims


def load_reports():
    reports = {}
    for p in sorted((ROOT / "reports/difftest").glob("*.json")):
        d = json.loads(p.read_text())
        d.pop("cases", None)
        reports[d.get("unit", p.stem)] = (d, p.relative_to(ROOT).as_posix())
    return reports


def module_path(u):
    return "crates/solidcore/src/" + u["rust_module"].replace("::", "/") + ".rs"


def header_table(path):
    """Rows (rust, upstream) of the `//! | Rust | Upstream |` table in a module header."""
    p = ROOT / path
    if not p.exists():
        return []
    rows = []
    for line in p.read_text().splitlines():
        if not line.startswith("//!"):
            if line.strip():
                break
            continue
        s = line[3:].strip()
        if not (s.startswith("|") and s.endswith("|")):
            continue
        cells = [c.strip() for c in s[1:-1].split("|")]
        if len(cells) != 2 or set(cells[0]) <= set("-: "):
            continue
        if cells[0] == "Rust" and cells[1] == "Upstream":
            continue
        rows.append((cells[0], cells[1]))
    return rows


def header_lookup(rows):
    """short upstream name -> rust cell, from backticked names in the Upstream column."""
    look = {}
    for rust, up in rows:
        for tok in re.findall(r"`([^`]+)`", up):
            name = re.sub(r"\(.*", "", tok).strip()
            for key in (name, name.split("::")[-1]):
                if key and key not in look:
                    look[key] = rust
    return look


# ---------------------------------------------------------------- markdown subset

def inline(text):
    """Escape `text`, then render `code`, **bold**, *italic* and http(s) links."""
    out = []
    for i, part in enumerate(re.split(r"(`[^`]*`)", text)):
        if i % 2:
            out.append(f"<code>{esc(part[1:-1])}</code>")
            continue
        s = esc(part, quote=False)
        s = re.sub(r"\[([^\]]+)\]\((https?://[^)\s]+)\)",
                   lambda m: f'<a href="{esc(m.group(2))}">{m.group(1)}</a>', s)
        s = re.sub(r"\*\*(.+?)\*\*", r"<strong>\1</strong>", s)
        s = re.sub(r"(?<![\w*])\*(?!\s)(.+?)(?<!\s)\*(?![\w*])", r"<em>\1</em>", s)
        out.append(s)
    return "".join(out)


def markdown(text):
    """Paragraphs and (nested) `- ` bullet lists; headings are handled by the caller."""
    out, para, stack = [], [], []  # stack: indents of open <ul>

    def flush_para():
        if para:
            out.append("<p>" + inline(" ".join(para)) + "</p>")
            para.clear()

    for raw in text.splitlines():
        if not raw.strip():
            flush_para()
            continue
        indent = len(raw) - len(raw.lstrip(" "))
        s = raw.strip()
        m = re.match(r"[-*] (.*)", s)
        if m:
            flush_para()
            while stack and stack[-1] > indent:
                out.append("</li></ul>")
                stack.pop()
            if stack and stack[-1] == indent:
                out.append("</li><li>")
            else:
                out.append("<ul><li>")
                stack.append(indent)
            out.append(inline(m.group(1)))
        elif stack and indent > 0:
            out.append(" " + inline(s))  # continuation of the current item
        else:
            while stack:
                out.append("</li></ul>")
                stack.pop()
            para.append(s)
    flush_para()
    out.extend("</li></ul>" for _ in stack)
    return "\n".join(out)


def parse_issues():
    text = (ROOT / "plan/upstream-issues.md").read_text()
    lines = text.splitlines()
    intro, entries = [], []
    severity = area = ""
    cur = None
    for line in lines:
        if line.startswith("# "):
            continue
        h = re.match(r"(#{2,4}) (.*)$", line)
        if h:
            level, title = len(h.group(1)), h.group(2).strip()
            if level == 2:
                severity, area = title, ""
            elif level == 3:
                area = title
            if level == 4:
                m = re.match(r"([A-Z]+)-(\d+)\.\s*(.*)$", title)
                cur = {"series": m.group(1) if m else "OTHER", "num": int(m.group(2)) if m else 0,
                       "id": f"{m.group(1)}-{m.group(2)}" if m else title,
                       "title": m.group(3) if m else title, "severity": severity, "area": area,
                       "body": []}
                entries.append(cur)
            else:
                cur = None
            continue
        if cur is not None:
            cur["body"].append(line)
        elif not entries and not severity:
            intro.append(line)
    for e in entries:
        body = "\n".join(e["body"]).strip()
        e["body"] = body
        fields = dict(re.findall(r"^- \*\*([^*]+?):\*\*\s*(.*)$", body, re.M))
        conf = fields.get("Confirmed", "").strip().lower()
        e["confirmed"] = "yes" if conf.startswith("yes") else ("no" if conf.startswith("no") else "unknown")
        e["how_found"] = fields.get("How found", "")
        e["psirt"] = "[PSIRT]" in e["title"]
    return "\n".join(intro).strip(), entries


# ---------------------------------------------------------------- page shell

def page(title, body, depth, nav, head_sha, head_date, description=""):
    up = "../" * depth
    links = [("index.html", "Overview", "overview"), ("units/index.html", "Units", "units"),
             ("issues.html", "Upstream issues", "issues"), ("ssi.html", "SSI corpus", "ssi")]
    navhtml = "".join(
        f'<a href="{up}{href}"{" aria-current=\"page\"" if key == nav else ""}>{label}</a>'
        for href, label, key in links)
    short = head_sha[:8]
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{esc(title)}</title>
<meta name="description" content="{esc(description or title)}">
<link rel="stylesheet" href="{up}assets/style.css">
<link rel="icon" href="{up}assets/logo.svg" type="image/svg+xml">
</head>
<body>
<header class="top"><div class="wrap">
<a class="brand" href="{up}index.html"><img src="{up}assets/logo.svg" alt="" width="28" height="28">solidcore</a>
<nav>{navhtml}<a href="{REPO_URL}">GitHub</a></nav>
</div></header>
<main class="wrap">
{body}
</main>
<footer class="wrap">Generated from <a href="{REPO_URL}/tree/{head_sha}"><code>{short}</code></a>
({esc(head_date)}) by <code>site/build.py</code>. Apache-2.0. Not affiliated with or endorsed by NVIDIA.</footer>
<script src="{up}assets/app.js"></script>
</body>
</html>
"""


def badge(status):
    return f'<span class="badge s-{esc(status)}">{esc(STATUS_TEXT.get(status, status))}</span>'


def fmt_int(n):
    return f"{n:,}"


# ---------------------------------------------------------------- chart

def chart_svg(points):
    """Stacked area: % of upstream lines green, and green + partial, over time."""
    if len(points) < 2:
        return "<p>Not enough history for a timeline.</p>"
    W, H, L, R, T, B = 720, 260, 44, 16, 12, 30
    t0, t1 = points[0][0], points[-1][0]
    span = max(1, t1 - t0)

    def x(t):
        return L + (W - L - R) * (t - t0) / span

    def y(p):
        return T + (H - T - B) * (1 - p / 100)

    data = []
    for t, s in points:
        tot = s["total_lines"] or 1
        data.append((t, 100 * s["green_lines"] / tot, 100 * s["done_lines"] / tot,
                     s["count"]["green"] + s["count"]["partial"], s["units"]))

    def path(vals):
        return " ".join(f"{x(t):.1f},{y(v):.1f}" for t, v in vals)

    green = [(d[0], d[1]) for d in data]
    done = [(d[0], d[2]) for d in data]
    base = f"{x(t1):.1f},{y(0):.1f} {x(t0):.1f},{y(0):.1f}"
    parts = [f'<svg class="chart" viewBox="0 0 {W} {H}" role="img" '
             f'aria-labelledby="chart-title" data-points="{esc(json.dumps([[d[0], round(d[1], 2), round(d[2], 2), d[3], d[4]] for d in data]))}" '
             f'data-x0="{t0}" data-x1="{t1}" data-l="{L}" data-r="{R}" data-w="{W}">',
             '<title id="chart-title">Share of upstream lines ported over time</title>']
    for p in range(0, 101, 25):
        parts.append(f'<line class="grid" x1="{L}" x2="{W - R}" y1="{y(p):.1f}" y2="{y(p):.1f}"/>'
                     f'<text class="tick" x="{L - 6}" y="{y(p) + 4:.1f}" text-anchor="end">{p}%</text>')
    # day ticks
    day = 86400
    first = (t0 // day + 1) * day
    ticks = list(range(first, t1 + 1, day))
    step = max(1, len(ticks) // 7 + (1 if len(ticks) % 7 else 0))
    for t in ticks[::step]:
        label = time.strftime("%b %d", time.gmtime(t))
        parts.append(f'<line class="grid v" x1="{x(t):.1f}" x2="{x(t):.1f}" y1="{T}" y2="{H - B}"/>'
                     f'<text class="tick" x="{x(t):.1f}" y="{H - B + 18}" text-anchor="middle">{label}</text>')
    parts.append(f'<polygon class="area partial" points="{path(done)} {base}"/>')
    parts.append(f'<polygon class="area green" points="{path(green)} {base}"/>')
    parts.append(f'<polyline class="line partial" points="{path(done)}"/>')
    parts.append(f'<polyline class="line green" points="{path(green)}"/>')
    lt, lg, ld = data[-1][0], data[-1][1], data[-1][2]
    parts.append(f'<circle class="dot partial" cx="{x(lt):.1f}" cy="{y(ld):.1f}" r="4"/>')
    parts.append(f'<circle class="dot green" cx="{x(lt):.1f}" cy="{y(lg):.1f}" r="4"/>')
    parts.append(f'<line class="cross" x1="0" x2="0" y1="{T}" y2="{H - B}" visibility="hidden"/>')
    parts.append(f'<rect class="hit" x="{L}" y="{T}" width="{W - L - R}" height="{H - T - B}"/>')
    parts.append("</svg>")
    legend = ('<div class="legend"><span><i class="sw green"></i>green (bit-identical to C++)</span>'
              '<span><i class="sw partial"></i>green + partial (ported)</span></div>')
    return (legend + '<div class="chartbox">' + "".join(parts) +
            '<div class="tip" hidden></div></div>')


def chart_table(points):
    rows = []
    for t, s in points:
        tot = s["total_lines"] or 1
        rows.append(f"<tr><td>{time.strftime('%Y-%m-%d %H:%M', time.gmtime(t))} UTC</td>"
                    f"<td class=num>{100 * s['green_lines'] / tot:.1f}%</td>"
                    f"<td class=num>{100 * s['done_lines'] / tot:.1f}%</td>"
                    f"<td class=num>{s['count']['green'] + s['count']['partial']} / {s['units']}</td></tr>")
    return ('<details class="tableview"><summary>Timeline as a table</summary><div class="scroll"><table>'
            '<thead><tr><th>Commit time</th><th>Green lines</th><th>Ported lines</th><th>Units ported</th></tr></thead>'
            "<tbody>" + "".join(rows) + "</tbody></table></div></details>")


# ---------------------------------------------------------------- pages

def build(out):
    t_start = time.time()
    head_sha, head_date = head_info()
    units = progress.load_units()
    claims = load_claims()
    for u in units:
        if u["status"] == "todo" and u["id"] in claims:
            u["status"] = "claimed"
            u["claim"] = claims[u["id"]]
    by_id = {u["id"]: u for u in units}
    status = {u["id"]: u["status"] for u in units}
    s = progress.summary(units)
    callgraph = json.loads((ROOT / "harness/callgraph/units.json").read_text())
    reports = load_reports()
    ssi = json.loads((ROOT / "reports/ssi-corpus.json").read_text())
    intro_md, issues = parse_issues()
    points = timeline()

    dependents = collections.defaultdict(list)
    soft_dependents = collections.defaultdict(list)
    for u in units:
        for d in u["depends_on"]:
            dependents[d].append(u["id"])
        for d in u.get("soft_depends_on", []):
            soft_dependents[d].append(u["id"])

    if out.exists():
        shutil.rmtree(out)
    (out / "units").mkdir(parents=True)
    (out / "assets").mkdir()
    for f in ("style.css", "app.js"):
        shutil.copy(ROOT / "site/assets" / f, out / "assets" / f)
    shutil.copy(ROOT / "assets/logo.svg", out / "assets/logo.svg")
    P = lambda *a, **k: page(*a, head_sha=head_sha, head_date=head_date, **k)  # noqa: E731

    def write(rel, text):
        (out / rel).write_text(text)

    # ---- overview
    count = s["count"]
    pct = 100 * s["done_lines"] / s["total_lines"]
    gpct = 100 * s["green_lines"] / s["total_lines"]
    ready_n = sum(1 for u in units if progress.is_ready(u, status))
    tiles = "".join(
        f'<div class="tile"><div class="k">{badge(st)}</div><div class="v">{count[st]}</div>'
        f'<div class="sub">units</div></div>'
        for st in ["green", "partial", "claimed", "todo"] + (["blocked"] if count["blocked"] else []))
    mods = progress.ORDER + sorted({progress.module(u) for u in units} - set(progress.ORDER))
    modrows = []
    for m in mods:
        us = [u for u in units if progress.module(u) == m]
        if not us:
            continue
        ms = progress.summary(us)
        tot = ms["total_lines"] or 1
        g = 100 * ms["green_lines"] / tot
        p = 100 * (ms["done_lines"] - ms["green_lines"]) / tot
        modrows.append(
            f'<tr><td><a href="units/index.html?module={esc(m)}">{esc(m)}</a></td>'
            f'<td class=num>{ms["count"]["green"] + ms["count"]["partial"]} / {len(us)}</td>'
            f'<td class=num>{fmt_int(ms["done_lines"])} / {fmt_int(ms["total_lines"])}</td>'
            f'<td class="barcell"><div class="bar" title="{g:.1f}% green, {p:.1f}% partial">'
            f'<i class="green" style="width:{g:.2f}%"></i><i class="partial" style="width:{p:.2f}%"></i></div></td>'
            f'<td class=num>{100 * ms["done_lines"] / tot:.0f}%</td></tr>')
    ssum = ssi.get("summary", {})
    verdicts = sorted(set(ssum.get("cpp", {})) | set(ssum.get("rust", {})))
    ssi_rows = "".join(f"<tr><td>{esc(v)}</td><td class=num>{ssum.get('cpp', {}).get(v, 0)}</td>"
                       f"<td class=num>{ssum.get('rust', {}).get(v, 0)}</td></tr>" for v in verdicts)
    par = ssum.get("parity", {})
    body = f"""
<section class="hero">
<h1>solidcore</h1>
<p class="lede">solidcore is a Rust port of NVIDIA's SMLib, the boundary-representation (B-rep) solid
modeling kernel from <a href="{UPSTREAM_URL}">usd-brep</a> (Apache-2.0), written in safe Rust: no
<code>unsafe</code>, no panics on bad input, deterministic, and buildable for WebAssembly. The upstream
code is split into {s['units']} port units. A unit only counts as <strong>green</strong> when the
<em>difftest</em> gate runs the Rust and the upstream C++ (through FFI) on the same inputs (upstream tests,
fixtures and fixed-seed generated cases) and the topology is identical and every value agrees within
1e-9 relative. A <strong>partial</strong> unit passes every case it can run; its other cases are
<em>deferred</em> because they reach code that is not ported yet. This page is regenerated from the
<a href="{REPO_URL}">repository</a> on every push to <code>main</code>.</p>
</section>
<section class="kpis">
<div class="tile big"><div class="k">Upstream lines ported</div><div class="v">{pct:.1f}%</div>
<div class="sub">{fmt_int(s['done_lines'])} of {fmt_int(s['total_lines'])} lines in green or partial units;
{gpct:.1f}% green</div></div>
{tiles}
<div class="tile"><div class="k">ready to claim</div><div class="v">{ready_n}</div><div class="sub">todo with all dependencies done</div></div>
</section>
<section>
<h2>Progress over time</h2>
<p class="muted">Share of in-scope upstream lines in green and partial units, from
<code>plan/modules.toml</code> along <code>main</code>'s history ({len(points)} sampled states).</p>
{chart_svg(points)}
{chart_table(points)}
</section>
<section>
<h2>By module</h2>
<div class="scroll"><table class="modtable"><thead><tr><th>Module</th><th>Units ported</th><th>Lines</th>
<th class="barcell">Progress</th><th>%</th></tr></thead><tbody>{''.join(modrows)}</tbody></table></div>
<p class="legend"><span><i class="sw green"></i>green</span><span><i class="sw partial"></i>partial</span></p>
</section>
<section>
<h2>Surface/surface intersection corpus</h2>
<p>{len(ssi.get('cases', []))} cases from the external
<a href="{esc(ssi.get('corpus', {}).get('repo', ''))}">ssi-corpus</a>, scored against known true intersections.
Rust and C++ are compared where the Rust path is ported: {par.get('compared', 0)} compared,
{par.get('identical', 0)} identical, {par.get('mismatched', 0)} mismatched.
<a href="ssi.html">All cases</a>.</p>
<div class="scroll narrow"><table><thead><tr><th>Verdict</th><th>C++</th><th>Rust</th></tr></thead>
<tbody>{ssi_rows}</tbody></table></div>
</section>
<section>
<h2>Elsewhere</h2>
<ul>
<li><a href="units/index.html">All {len(units)} units</a>, with difftest results and blockers.</li>
<li><a href="issues.html">{len(issues)} upstream issues</a> found while porting.</li>
<li><a href="{REPO_URL}">Repository</a>: <a href="{REPO_URL}/blob/main/AGENTS.md">how to port a unit</a>,
<a href="{REPO_URL}/blob/main/plan/PROGRESS.md">checklist</a>.</li>
</ul>
</section>
"""
    write("index.html", P("solidcore progress", body, 0, "overview",
                          description="Port progress of solidcore, a Rust port of NVIDIA's SMLib."))

    # ---- units table
    modules = sorted({progress.module(u) for u in units})
    rows = []
    for u in sorted(units, key=lambda u: (u["layer"], u["id"])):
        rep = reports.get(u["id"], (None, None))[0]
        nfn = len(callgraph.get(u["id"], {}).get("functions", []))
        if rep:
            cp, cd, cf = rep.get("cases_passed", 0), rep.get("cases_deferred", 0), rep.get("cases_failed", 0)
            blockers = sorted(rep.get("deferred_by_unit", {}).items(), key=lambda kv: (-kv[1], kv[0]))
            btxt = ", ".join(f'<a href="{esc(b)}.html">{esc(b)}</a>&nbsp;({n})' for b, n in blockers[:3])
            if len(blockers) > 3:
                btxt += f" +{len(blockers) - 3}"
            nums = f"<td class=num>{cp}</td><td class=num>{cd}</td><td class=num>{cf}</td>"
        else:
            btxt = ""
            nums = '<td class=num data-v="-1">–</td><td class=num data-v="-1">–</td><td class=num data-v="-1">–</td>'
        ready = progress.is_ready(u, status)
        rows.append(
            f'<tr data-status="{u["status"]}" data-module="{esc(progress.module(u))}">'
            f'<td><a href="{esc(u["id"])}.html"><code>{esc(u["id"])}</code></a>{" <span class=tag>ready</span>" if ready else ""}</td>'
            f'<td data-v="{STATUSES.index(u["status"]) if u["status"] in STATUSES else 9}">{badge(u["status"])}</td>'
            f'<td class=num>{u["layer"]}</td><td class=num data-v="{u["est_lines"]}">{fmt_int(u["est_lines"])}</td>'
            f'<td class=num>{nfn}</td>{nums}<td class=blk>{btxt}</td></tr>')
    status_opts = "".join(f'<option value="{st}">{st} ({count[st]})</option>' for st in STATUSES if count[st])
    mod_opts = "".join(f'<option value="{esc(m)}">{esc(m)}</option>' for m in modules)
    body = f"""
<h1>Units</h1>
<p class="muted">{len(units)} port units from <code>plan/modules.toml</code>. Cases come from
<code>reports/difftest/&lt;unit&gt;.json</code>; blockers are the units deferred cases wait on (with case
counts). Click a column heading to sort.</p>
<div class="filters" data-table="units">
<label>Search <input type="search" name="q" placeholder="unit id"></label>
<label>Status <select name="status"><option value="">all</option>{status_opts}</select></label>
<label>Module <select name="module"><option value="">all</option>{mod_opts}</select></label>
<span class="count" aria-live="polite"></span>
</div>
<div class="scroll"><table id="units" class="sortable">
<thead><tr><th data-sort="text">Unit</th><th data-sort="num">Status</th><th data-sort="num">Layer</th>
<th data-sort="num">Lines</th><th data-sort="num">Fns</th><th data-sort="num">Passed</th>
<th data-sort="num">Deferred</th><th data-sort="num">Failed</th><th>Top blockers</th></tr></thead>
<tbody>{''.join(rows)}</tbody></table></div>
"""
    write("units/index.html", P("Units · solidcore", body, 1, "units"))

    # ---- unit pages
    def unit_links(ids):
        if not ids:
            return '<span class="muted">none</span>'
        return " ".join(f'<a class="chip" href="{esc(i)}.html">{esc(i)} {badge(status.get(i, "?"))}</a>'
                        for i in sorted(ids))

    for u in units:
        uid = u["id"]
        cg = callgraph.get(uid, {})
        src = module_path(u)
        exists = "(" not in src and (ROOT / src).exists()
        hrows = header_table(src) if exists else []
        look = header_lookup(hrows)
        rep, rep_path = reports.get(uid, (None, None))
        fns = sorted(cg.get("functions", []), key=lambda f: (f["file"], f["line"], f["name"]))
        frows = []
        for f in fns:
            short = f["name"].split("::")[-1]
            rust = look.get(f["name"]) or look.get(short)
            frows.append(
                f'<tr><td><code>{esc(f["name"])}</code></td>'
                f'<td><a href="{UPSTREAM_URL}/blob/{UPSTREAM_COMMIT}/{esc(f["file"])}#L{f["line"]}">'
                f'{esc(f["file"].split("/")[-1])}:{f["line"]}</a></td><td class=num>{f["lines"]}</td>'
                f'<td>{inline(rust) if rust else ""}</td></tr>')
        ftable = (f'<div class="scroll"><table class="sortable"><thead><tr><th data-sort="text">Upstream function</th>'
                  f'<th data-sort="text">Defined at</th><th data-sort="num">Lines</th><th data-sort="text">Rust (from the file header)</th>'
                  f'</tr></thead><tbody>{"".join(frows)}</tbody></table></div>') if frows else \
            '<p class="muted">No upstream functions listed in <code>harness/callgraph/units.json</code>.</p>'
        htable = ""
        if hrows:
            htable = ('<details><summary>Module file-header table ({} rows)</summary><div class="scroll"><table>'
                      '<thead><tr><th>Rust</th><th>Upstream</th></tr></thead><tbody>{}</tbody></table></div></details>').format(
                len(hrows), "".join(f"<tr><td>{inline(r)}</td><td>{inline(p)}</td></tr>" for r, p in hrows))
        if rep:
            dbu = sorted(rep.get("deferred_by_unit", {}).items(), key=lambda kv: (-kv[1], kv[0]))
            dtable = "".join(f'<tr><td><a href="{esc(b)}.html"><code>{esc(b)}</code></a> {badge(status.get(b, "?"))}</td>'
                             f'<td class=num>{n}</td></tr>' for b, n in dbu)
            floors = rep.get("abs_floors") or []
            diff = f"""
<dl class="facts">
<div><dt>Cases</dt><dd>{fmt_int(rep.get('cases_total', 0))}</dd></div>
<div><dt>Passed</dt><dd>{fmt_int(rep.get('cases_passed', 0))}</dd></div>
<div><dt>Deferred</dt><dd>{fmt_int(rep.get('cases_deferred', 0))}</dd></div>
<div><dt>Failed</dt><dd>{fmt_int(rep.get('cases_failed', 0))}</dd></div>
<div><dt>Both errored</dt><dd>{fmt_int(rep.get('cases_both_errored', 0))}</dd></div>
<div><dt>Max rel. error</dt><dd>{esc(repr(rep.get('max_rel_err')))}</dd></div>
<div><dt>Tolerance</dt><dd>{esc(repr(rep.get('rel_tol')))} rel.</dd></div>
<div><dt>Seeds</dt><dd>{esc(str(rep.get('seeds', '')))}</dd></div>
</dl>
{('<p>Absolute floors: <code>' + esc(json.dumps(floors)) + '</code></p>') if floors else ''}
<h3>Blocked by</h3>
{('<div class="scroll narrow"><table><thead><tr><th>Unported unit</th><th>Deferred cases</th></tr></thead><tbody>' + dtable + '</tbody></table></div>') if dtable else '<p class="muted">Nothing: no deferred cases.</p>'}
<p><a href="{REPO_URL}/blob/{head_sha}/{esc(rep_path)}">Report JSON</a> (per-case results)</p>"""
        else:
            diff = '<p class="muted">No difftest report yet.</p>'
        claim = f'<p>Claimed: {inline(u["claim"])}</p>' if u.get("claim") else ""
        srclink = (f'<a href="{REPO_URL}/blob/{head_sha}/{esc(src)}"><code>{esc(src)}</code></a>' if exists
                   else '<span class="muted">no single module (crate-wide unit)</span>' if "(" in src
                   else f'<code>{esc(src)}</code> <span class="muted">(not written yet)</span>')
        upfiles = ", ".join(f'<a href="{UPSTREAM_URL}/blob/{UPSTREAM_COMMIT}/{esc(f)}"><code>{esc(f)}</code></a>'
                            for f in u.get("upstream", []))
        tests = ", ".join(f"<code>{esc(t)}</code>" for t in u.get("tests", [])) or '<span class="muted">none</span>'
        body = f"""
<p class="crumbs"><a href="index.html">Units</a> / {esc(progress.module(u))}</p>
<h1><code>{esc(uid)}</code> {badge(u['status'])}{' <span class=tag>ready</span>' if progress.is_ready(u, status) else ''}</h1>
<dl class="facts">
<div><dt>Layer</dt><dd>{u['layer']}</dd></div>
<div><dt>Upstream lines</dt><dd>{fmt_int(u['est_lines'])}</dd></div>
<div><dt>Functions</dt><dd>{len(fns)}</dd></div>
<div><dt>Rust module</dt><dd><code>{esc(u['rust_module'])}</code></dd></div>
</dl>
{claim}
<p>Source: {srclink}</p>
<p>Upstream: {upfiles or '<span class="muted">none</span>'}</p>
<h2>Notes</h2>
<p class="notes">{inline(u.get('notes', '')) or '<span class="muted">none</span>'}</p>
<h2>Difftest</h2>
{diff}
<h2>Dependencies</h2>
<p><strong>Depends on:</strong> {unit_links(u['depends_on'])}</p>
<p><strong>Soft dependencies:</strong> {unit_links(u.get('soft_depends_on', []))}</p>
<p><strong>Dependents:</strong> {unit_links(dependents.get(uid, []))}</p>
<p><strong>Soft dependents:</strong> {unit_links(soft_dependents.get(uid, []))}</p>
<h2>Upstream functions</h2>
{ftable}
{htable}
<h2>Upstream tests reaching this unit</h2>
<p class="tests">{tests}</p>
"""
        write(f"units/{uid}.html", P(f"{uid} · solidcore", body, 1, "units"))

    # ---- issues
    sc = collections.Counter(e["series"] for e in issues)
    cc = collections.Counter(e["confirmed"] for e in issues)
    arts = []
    seen = collections.Counter()
    for e in issues:
        seen[e["id"]] += 1
        e["anchor"] = e["id"] if seen[e["id"]] == 1 else f'{e["id"]}-dup{seen[e["id"]]}'
    dups = sorted(k for k, n in seen.items() if n > 1)
    for e in issues:
        meta = (f'<span class="tag">{esc(SERIES_TEXT.get(e["series"], e["series"]))}</span>'
                f'<span class="tag">{esc(e["area"])}</span>' if e["area"] else
                f'<span class="tag">{esc(SERIES_TEXT.get(e["series"], e["series"]))}</span>')
        meta += f'<span class="tag c-{e["confirmed"]}">confirmed: {e["confirmed"]}</span>'
        arts.append(
            f'<article class="issue" id="{esc(e["anchor"])}" data-series="{esc(e["series"])}" '
            f'data-confirmed="{e["confirmed"]}">'
            f'<h3><a class="anchor" href="#{esc(e["anchor"])}">{esc(e["id"])}.</a> {inline(e["title"])}</h3>'
            f'<div class="meta">{meta}</div>{markdown(e["body"])}</article>')
    ser_opts = "".join(f'<option value="{k}">{k}: {esc(SERIES_TEXT[k])} ({sc[k]})</option>' for k in SERIES if sc[k])
    ser_opts += "".join(f'<option value="{esc(k)}">{esc(k)} ({sc[k]})</option>' for k in sorted(set(sc) - set(SERIES)))
    body = f"""
<h1>Upstream issues</h1>
<p class="muted">{len(issues)} entries from <a href="{REPO_URL}/blob/main/plan/upstream-issues.md"><code>plan/upstream-issues.md</code></a>,
rendered as written: {', '.join(f'{sc[k]} {k}' for k in SERIES if sc[k])}; {cc['yes']} confirmed by a C++ run.</p>
{('<p class="warn">Duplicate IDs in the source file: ' + ', '.join(esc(d) for d in dups) + '. Both entries are shown; the later one is linked as <code>#&lt;id&gt;-dup2</code>.</p>') if dups else ''}
<details class="intro"><summary>About this list</summary>{markdown(intro_md)}</details>
<div class="filters" data-list="issues">
<label>Search <input type="search" name="q" placeholder="text, symbol or file"></label>
<label>Series <select name="series"><option value="">all</option>{ser_opts}</select></label>
<label>Confirmed <select name="confirmed"><option value="">all</option>
<option value="yes">yes ({cc['yes']})</option><option value="no">no ({cc['no']})</option>
{f'<option value="unknown">unknown ({cc["unknown"]})</option>' if cc['unknown'] else ''}</select></label>
<span class="count" aria-live="polite"></span>
</div>
<div id="issues">{''.join(arts)}</div>
"""
    write("issues.html", P("Upstream issues · solidcore", body, 0, "issues"))

    # ---- ssi corpus
    cases = sorted(ssi.get("cases", []), key=lambda c: c["id"])
    classes = sorted({c.get("class", "") for c in cases})
    crow = []
    for c in cases:
        cpp, rust = c.get("cpp", {}), c.get("rust", {})
        rv = rust.get("verdict", "")
        rdet = ""
        if rust.get("deferred"):
            d = rust["deferred"]
            rdet = (f'<div class="small">waits on <a href="units/{esc(d.get("unit", ""))}.html">{esc(d.get("unit", ""))}</a>: '
                    f'<code>{esc(d.get("symbol", ""))}</code></div>')
        flags = cpp.get("flags", [])
        rflags = rust.get("flags", []) if rv != "DEFERRED" else []
        ftxt = "".join(f"<li>{esc(f)}</li>" for f in flags)
        if rflags and rflags != flags:
            ftxt += "".join(f"<li>Rust: {esc(f)}</li>" for f in rflags)
        par = c.get("parity") or {}
        ptxt = ("identical" if par.get("pass") and not par.get("mismatches") else
                f"{par.get('mismatches')} mismatches" if par else "")
        crow.append(
            f'<tr data-class="{esc(c.get("class", ""))}" data-cpp="{esc(cpp.get("verdict", ""))}" data-rust="{esc(rv)}">'
            f'<td><code>{esc(c["id"])}</code></td><td>{esc(c.get("class", ""))}</td>'
            f'<td>{esc(" × ".join(c.get("surfaces", [])))}</td>'
            f'<td><span class="v v-{esc(cpp.get("verdict", "").replace(" ", "-"))}">{esc(cpp.get("verdict", ""))}</span>'
            f'<div class="small">{esc(cpp.get("status_name", ""))}</div></td>'
            f'<td><span class="v v-{esc(rv.replace(" ", "-"))}">{esc(rv)}</span>{rdet}</td>'
            f'<td>{esc(ptxt)}</td><td class="flags">{"<ul>" + ftxt + "</ul>" if ftxt else ""}</td></tr>')
    cls_opts = "".join(f'<option value="{esc(k)}">{esc(k)}</option>' for k in classes)
    cv_opts = "".join(f'<option value="{esc(k)}">{esc(k)} ({n})</option>' for k, n in sorted(ssum.get("cpp", {}).items()))
    rv_opts = "".join(f'<option value="{esc(k)}">{esc(k)} ({n})</option>' for k, n in sorted(ssum.get("rust", {}).items()))
    corpus = ssi.get("corpus", {})
    body = f"""
<h1>SSI corpus</h1>
<p class="muted">{len(cases)} surface/surface intersection cases from
<a href="{esc(corpus.get('repo', ''))}">{esc(corpus.get('repo', ''))}</a> at <code>{esc(str(corpus.get('commit', ''))[:12])}</code>,
from <a href="{REPO_URL}/blob/main/reports/ssi-corpus.json"><code>reports/ssi-corpus.json</code></a>. Each output is
scored against the known intersection ({esc(str(ssi.get('samples_per_curve', '')))} samples per curve, accuracy
tolerance {esc(format(ssi.get('acc_tol', 0), 'g'))}, approximation tolerance {esc(str(ssi.get('approx_tol', '')))}). Flags are
the C++ result's.</p>
<div class="scroll narrow"><table><thead><tr><th>Verdict</th><th>C++</th><th>Rust</th></tr></thead>
<tbody>{ssi_rows}</tbody></table></div>
<div class="filters" data-table="ssi">
<label>Search <input type="search" name="q" placeholder="case id or surface"></label>
<label>Class <select name="class"><option value="">all</option>{cls_opts}</select></label>
<label>C++ <select name="cpp"><option value="">all</option>{cv_opts}</select></label>
<label>Rust <select name="rust"><option value="">all</option>{rv_opts}</select></label>
<span class="count" aria-live="polite"></span>
</div>
<div class="scroll"><table id="ssi" class="sortable"><thead><tr><th data-sort="text">Case</th><th data-sort="text">Class</th>
<th data-sort="text">Surfaces</th><th data-sort="text">C++</th><th data-sort="text">Rust</th><th data-sort="text">Parity</th><th>Flags</th></tr></thead>
<tbody>{''.join(crow)}</tbody></table></div>
"""
    write("ssi.html", P("SSI corpus · solidcore", body, 0, "ssi"))
    return {"units": len(units), "issues": len(issues), "ssi": len(cases), "timeline": len(points),
            "seconds": time.time() - t_start}


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--out", default=str(ROOT / "bin/site"))
    a = ap.parse_args()
    r = build(pathlib.Path(a.out).resolve())
    print(f"site: {r['units']} unit pages, {r['issues']} issues, {r['ssi']} SSI cases, "
          f"{r['timeline']} timeline points -> {a.out} ({r['seconds']:.1f}s)")


if __name__ == "__main__":
    main()

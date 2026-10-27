#!/usr/bin/env python3
"""Build the solidcore progress site (GitHub Pages) from data already in the repo.

    python3 site/build.py                 # -> bin/site/
    python3 site/build.py --out DIR       # elsewhere

Standard library only (Python 3.11+ for tomllib). Every number comes from the repo:
plan/units/*.toml, plan/PROGRESS.md (claims; generated, not committed), harness/callgraph/units.json (upstream
line counts at the pin), scripts/upstream-pin.env (upstream version and commit),
reports/difftest/*.json (not committed: CI's artifact, see scripts/fetch-reports.sh),
reports/solids/solids.json (not committed: CI's artifact, see scripts/fetch-solids.sh),
reports/ssi-corpus.json, plan/upstream-issues/*.md, the
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
import units as plan_units  # noqa: E402  (the shared plan loader)

REPO = os.environ.get("GITHUB_REPOSITORY", "tangency-labs/solidcore")
REPO_URL = f"https://github.com/{REPO}"
VIEWER_URL = "https://tangency-labs.github.io/solidcore-viewer/"
PIN = plan_units.load_pin(ROOT)  # scripts/upstream-pin.env: the one place the pin is set
UPSTREAM_URL = PIN["UPSTREAM_REPO"]
UPSTREAM_VERSION = PIN["UPSTREAM_VERSION"]
UPSTREAM_COMMIT = PIN["UPSTREAM_COMMIT"]
UPSTREAM_PIN_HTML = (f'<a href="{UPSTREAM_URL}/tree/{UPSTREAM_COMMIT}">usd-brep v{html.escape(UPSTREAM_VERSION)} '
                     f'(<code>{UPSTREAM_COMMIT[:7]}</code>)</a>')
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


def cat_blobs(oids):
    """{oid: text} for git blobs, read in one `git cat-file --batch`."""
    oids = list(dict.fromkeys(oids))
    if not oids:
        return {}
    raw = subprocess.run(["git", "-C", str(ROOT), "cat-file", "--batch"], check=True,
                         capture_output=True, input="".join(f"{o}\n" for o in oids).encode()).stdout
    out, pos = {}, 0
    for o in oids:
        nl = raw.index(b"\n", pos)
        size = int(raw[pos:nl].split()[2])
        out[o] = raw[nl + 1:nl + 1 + size].decode()
        pos = nl + 1 + size + 1
    return out


def timeline():
    """[(unix time, units)] for the plan along main's first-parent history.

    The plan is the tree plan/units/ (one file per unit) or, before the split, the single
    file plan/modules.toml. Consecutive commits with the same plan object are dropped,
    then the history is thinned to at most MAX_TIMELINE_POINTS states (the last state in
    each time bucket), always keeping the first and last.
    """
    log = git("log", "--first-parent", "--format=%H %ct", "HEAD").split("\n")
    commits = [tuple(line.split()) for line in reversed(log) if line.strip()]
    if not commits:
        return []
    req = "".join(f"{h}:plan/units\n{h}:plan/modules.toml\n" for h, _ in commits)
    checks = git("cat-file", "--batch-check", inp=req).split("\n")
    states = []  # (time, oid, kind)
    last = None
    for i, (h, t) in enumerate(commits):
        for chk in checks[2 * i:2 * i + 2]:
            parts = chk.split()
            if len(parts) >= 2 and parts[1] in ("tree", "blob"):
                if parts[0] != last:
                    states.append((int(t), parts[0], parts[1]))
                    last = parts[0]
                break
    if len(states) > MAX_TIMELINE_POINTS:
        t0, t1 = states[0][0], states[-1][0]
        width = max(1, (t1 - t0) / (MAX_TIMELINE_POINTS - 1))
        buckets = {}
        for s in states:
            buckets[int((s[0] - t0) / width)] = s
        thinned = [states[0]] + [buckets[k] for k in sorted(buckets)]
        states = sorted(set(thinned), key=lambda s: s[0])
    trees = {}
    for _, oid, kind in states:
        if kind == "tree":
            entries = [ln.split(None, 3) for ln in git("ls-tree", oid).splitlines()]
            trees[oid] = {e[3][:-5]: e[2] for e in entries
                          if len(e) == 4 and e[1] == "blob" and e[3].endswith(".toml")}
    blobs = cat_blobs([oid for _, oid, kind in states if kind == "blob"] +
                      [b for files in trees.values() for b in files.values()])
    out = []
    for t, oid, kind in states:
        try:
            if kind == "blob":
                units = plan_units.parse_modules_toml(blobs[oid])
            else:
                units = plan_units.units_from_texts({k: blobs[b] for k, b in trees[oid].items()})
        except Exception:  # an early revision may not parse; skip it
            continue
        out.append((t, units))
    return out


def area_slug(m):
    """File-name and id-safe form of a module name ("(crate-wide)" -> "crate-wide")."""
    return re.sub(r"[^A-Za-z0-9_-]+", "-", m).strip("-") or "module"


def unit_module(u):
    """Area of a unit (first segment of its rust_module), tolerant of old revisions."""
    return u.get("rust_module", u.get("id", "")).split("::")[0]


def aggregate(samples):
    """One pass over the sampled states of the plan.

    Returns (overall, by_area, by_unit): overall is [(t, summary)], by_area maps an area
    to [(t, summary)] for the states in which it has units, and by_unit maps a unit id to
    [(t, status, est_lines)] for the states in which it exists.
    """
    overall, by_area, by_unit = [], collections.defaultdict(list), collections.defaultdict(list)
    for t, units in samples:
        overall.append((t, progress.summary(units)))
        groups = collections.defaultdict(list)
        for u in units:
            groups[unit_module(u)].append(u)
            by_unit[u["id"]].append((t, u.get("status", "todo"), u.get("est_lines", 0)))
        for m, us in groups.items():
            by_area[m].append((t, progress.summary(us)))
    return overall, by_area, by_unit


# ---------------------------------------------------------------- data

def load_claims():
    """id -> claim text from the "In progress" section of the generated PROGRESS.md."""
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
    """unit -> (report without its cases, path) from reports/difftest/. The reports are
    not committed; CI fetches main's latest set first (scripts/fetch-reports.sh)."""
    reports = {}
    for p in sorted((ROOT / "reports/difftest").glob("*.json")):
        d = json.loads(p.read_text())
        d.pop("cases", None)
        reports[d.get("unit", p.stem)] = (d, p)
    return reports


SOLIDS_SCHEMA = 1


def load_solids():
    """The primitives example's report (reports/solids/solids.json), or None.

    Not committed; CI fetches main's latest with scripts/fetch-solids.sh. A missing or
    unreadable file, or another schema version, means "no data yet".
    """
    p = ROOT / "reports/solids/solids.json"
    try:
        d = json.loads(p.read_text())
    except (OSError, ValueError):
        return None
    if not isinstance(d, dict) or d.get("schema") != SOLIDS_SCHEMA:
        return None
    return d


def solids_counts(sol):
    """(total, built, passed) over the primitives."""
    prims = sol.get("primitives", []) if sol else []
    return (len(prims), sum(1 for x in prims if x.get("built")),
            sum(1 for x in prims if x.get("pass")))


def fmt_num(x):
    """A measured or analytic value, compactly."""
    if x is None:
        return "–"
    if isinstance(x, bool):
        return "yes" if x else "no"
    if isinstance(x, int):
        return str(x)
    if isinstance(x, float):
        return f"{x:.10g}"
    if isinstance(x, list):
        return "(" + ", ".join(fmt_num(v) for v in x) + ")"
    if isinstance(x, dict) and "min" in x:
        return f"{fmt_num(x.get('min'))} – {fmt_num(x.get('max'))}"
    return str(x)


SOLID_CHECKS = [("manifold", "Closed manifold"), ("euler", "Euler"), ("counts", "F, E, V"),
                ("volume", "Volume"), ("area", "Area"), ("bbox", "Bounding box")]


def solids_body(sol, depth=0):
    """The Solids page body (also says "no data yet" when `sol` is None)."""
    up = "../" * depth
    corpus = (sol or {}).get("corpus") or {}
    models = corpus.get("models") or []
    if models:
        cpass = sum(1 for m in models if m.get("pass"))
        crows = "".join(
            f'<tr><td>{esc(str(m.get("name", "")))}</td>'
            f'<td><span class="v v-{"PASS" if m.get("pass") else "FAIL"}">{"PASS" if m.get("pass") else "FAIL"}</span></td>'
            + "".join(f'<td class=num>{esc(fmt_num((m.get("actual") or {}).get(k)))}'
                      f'<div class="small">expected {esc(fmt_num((m.get("expected") or {}).get(k)))}</div></td>'
                      for k in ("volume", "area", "center_of_mass"))
            + "</tr>" for m in models)
        corpus_html = (f'<p>{cpass} / {len(models)} models pass.</p><div class="scroll"><table><thead><tr>'
                       '<th>Model</th><th>Result</th><th>Volume</th><th>Area</th><th>Centre of mass</th>'
                       f'</tr></thead><tbody>{crows}</tbody></table></div>')
    else:
        corpus_html = """<p class="warn">Placeholder: pass rate TBD, waiting on the choice of a corpus of real models.</p>
<p class="muted">Each model will be read, its mass properties computed, and the result compared with the
reference: per model the JSON's <code>corpus.models[]</code> records <code>name</code>, <code>source</code>,
<code>built</code>, <code>pass</code>, <code>blocker</code>, and <code>expected</code> vs <code>actual</code>
<code>volume</code>, <code>area</code> and <code>center_of_mass</code> with their <code>rel_err</code>.</p>"""
    corpus_section = f'<h2 id="corpus">Corpus (real models)</h2>\n{corpus_html}'
    intro = f"""<h1>Solids</h1>
<p class="muted">Units passing difftest says the port matches the C++ function by function; this page says
whether whole solids come out. The <code>primitives</code> example
(<a href="{REPO_URL}/tree/main/crates/solidcore/examples/primitives"><code>crates/solidcore/examples/primitives</code></a>)
builds each primitive through the public API in pure Rust and checks that it is a closed manifold solid with the
analytic Euler-Poincare characteristic, volume, area and bounding box (relative tolerance
{esc(fmt_num((sol or {}).get("rel_tol", 1e-9)))}). A primitive that stops at code not ported yet names the
unit and upstream symbol it is blocked on.</p>"""
    if not sol:
        return (intro + '<p class="warn">No data yet: <code>reports/solids/solids.json</code> is missing. CI uploads it '
                'from every push to <code>main</code> (<code>scripts/fetch-solids.sh</code>); locally, run '
                '<code>cargo run -p solidcore --example primitives -- --json reports/solids/solids.json</code>.</p>\n'
                + corpus_section)
    total, built, passed = solids_counts(sol)
    prims = sol.get("primitives", [])
    blockers = collections.Counter((x.get("blocker") or {}).get("unit") for x in prims if x.get("blocker"))
    rows = []
    for x in prims:
        blk = x.get("blocker")
        if x.get("pass"):
            st = '<span class="badge s-green">passes</span>'
        elif x.get("built"):
            st = '<span class="badge s-partial">built, fails</span>'
        else:
            st = '<span class="badge s-blocked">blocked</span>'
        if blk:
            btxt = (f'<a href="{up}units/{esc(str(blk.get("unit", "")))}.html"><code>{esc(str(blk.get("unit", "")))}</code></a>'
                    f'<div class="small"><code>{esc(str(blk.get("upstream", "")))}</code>'
                    f'{" (while measuring)" if blk.get("stage") == "measure" else ""}</div>')
        elif x.get("error"):
            btxt = f'<div class="small"><code>{esc(str(x["error"]))}</code></div>'
        else:
            btxt = '<span class="muted">none</span>'
        checks = {c.get("name"): c for c in x.get("checks", [])}
        an = x.get("analytic") or {}
        expected_of = {"manifold": True, "euler": an.get("euler"), "counts": an.get("counts"),
                       "volume": an.get("volume"), "area": an.get("area"), "bbox": an.get("bbox")}
        cells = []
        for key, _ in SOLID_CHECKS:
            c = checks.get(key)
            if c is None:
                exp = expected_of.get(key)
                cells.append('<td><span class="muted">–</span>' + (
                    f'<div class="small">expected {esc(fmt_num(exp))}</div>' if exp is not None else "") + "</td>")
                continue
            if c.get("actual") is None:
                verdict = '<span class="v v-DEFERRED">not measured</span>'
            elif c.get("pass"):
                verdict = '<span class="v v-PASS">PASS</span>'
            else:
                verdict = '<span class="v v-FAIL">FAIL</span>'
            detail = f'{esc(fmt_num(c.get("actual")))}' if c.get("actual") is not None else ""
            sub = f'expected {esc(fmt_num(c.get("expected")))}'
            if c.get("rel_err") is not None:
                sub += f'; rel. err {c["rel_err"]:.2e}'
            msg = c.get("message") or ""
            title = f' title="{esc(msg)}"' if msg else ""
            cells.append(f'<td{title}>{verdict} {detail}<div class="small">{sub}</div></td>')
        rows.append(f'<tr><td>{esc(str(x.get("name", "")))}</td><td>{st}</td><td>{btxt}</td>{"".join(cells)}</tr>')
    head = "".join(f"<th>{esc(label)}</th>" for _, label in SOLID_CHECKS)
    btiles = ", ".join(f'<a href="{up}units/{esc(u)}.html"><code>{esc(u)}</code></a> ({n})'
                       for u, n in sorted(blockers.items(), key=lambda kv: (-kv[1], kv[0])))
    return f"""{intro}
<section class="kpis">
<div class="tile big"><div class="k">Solids (primitives)</div><div class="v">{passed} / {total}</div>
<div class="sub">pass every check; {built} / {total} build</div></div>
<div class="tile"><div class="k">build</div><div class="v">{built}</div><div class="sub">of {total} primitives</div></div>
<div class="tile"><div class="k">pass</div><div class="v">{passed}</div><div class="sub">of {total} primitives</div></div>
<div class="tile"><div class="k">not built</div><div class="v">{total - built}</div><div class="sub">build stops at an error</div></div>
</section>
<p><strong>Solids: {built}/{total} build, {passed}/{total} pass.</strong>
{('Blocked on: ' + btiles + '.') if btiles else ''}</p>
<h2>Primitives</h2>
<p class="muted">From <a href="{up}reports/solids.json"><code>reports/solids.json</code></a> (CI's latest
<code>main</code> run). Each check shows the measured value, the analytic one and the relative error; a check
that could not be measured says so. Hover a failed check for its message.</p>
<div class="scroll"><table><thead><tr><th>Primitive</th><th>Status</th><th>Blocker</th>{head}</tr></thead>
<tbody>{"".join(rows)}</tbody></table></div>
{corpus_section}
"""


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
    """Upstream issues: the intro is plan/upstream-issues/README.md (without its title),
    the entries are the files in that directory (parse_issue_files)."""
    readme = ROOT / "plan/upstream-issues/README.md"
    lines = readme.read_text().splitlines() if readme.exists() else []
    intro = "\n".join(l for l in lines if not l.startswith("<!-- archive:"))
    intro = re.sub(r"\A# [^\n]*\n", "", intro).strip()
    return intro, parse_issue_files()


def issue_file_order(path):
    """Name order, except that `<ID>--...--dupN.md` follows the other files of its ID, so
    the site's `#<id>-dupN` anchors keep pointing at the same entries as in the archive."""
    m = re.search(r"--dup(\d+)$", path.stem)
    return (path.stem.split("--")[0], int(m.group(1)) if m else 1, path.name)


def parse_issue_files():
    """Entries: one file each under plan/upstream-issues/ (plan/upstream-issues/README.md).

    A file is a `#### <ID>. <title>` heading followed by the usual field bullets, starting
    with `- **Severity:**` and `- **Area:**` (the section headings of the former archive).
    Only those leading Severity/Area lines are header fields; a Severity bullet further
    down (archive entries have their own) stays in the body. Files are read in name order
    (issue_file_order) so the page is deterministic.
    """
    entries = []
    for path in sorted((ROOT / "plan/upstream-issues").glob("*.md"), key=issue_file_order):
        if path.name == "README.md":
            continue
        lines = path.read_text().splitlines()
        head = next((i for i, l in enumerate(lines) if l.startswith("#### ")), None)
        if head is None:
            continue
        title = lines[head][5:].strip()
        m = re.match(r"([A-Z]+)-(\d+)\.\s*(.*)$", title)
        header, i = {}, head + 1
        while i < len(lines):
            f = re.match(r"^- \*\*(Severity|Area):\*\*\s*(.*)$", lines[i])
            if lines[i].strip() and (not f or f.group(1) in header):
                break
            if f:
                header[f.group(1)] = f.group(2).strip()
            i += 1
        body = "\n".join(lines[i:]).strip()
        fields = dict(re.findall(r"^- \*\*([^*]+?):\*\*\s*(.*)$", body, re.M))
        for k in ("Severity", "Area"):
            if k not in header and k in fields:  # header field not at the top: take it out
                header[k] = fields[k].strip()
                body = re.sub(rf"^- \*\*{k}:\*\*.*\n?", "", body, count=1, flags=re.M).strip()
        conf = fields.get("Confirmed", "").strip().lower()
        entries.append({
            "series": m.group(1) if m else "OTHER", "num": int(m.group(2)) if m else 0,
            "id": f"{m.group(1)}-{m.group(2)}" if m else title,
            "title": m.group(3) if m else title,
            "severity": header.get("Severity", ""), "area": header.get("Area", ""),
            "body": body,
            "confirmed": "yes" if conf.startswith("yes") else ("no" if conf.startswith("no") else "unknown"),
            "how_found": fields.get("How found", ""),
            "psirt": "[PSIRT]" in title,
        })
    return entries


# ---------------------------------------------------------------- page shell

def page(title, body, depth, nav, head_sha, head_date, description="", path="", scripts=()):
    up = "../" * depth
    links = [("index.html", "Overview", "overview"), ("units/index.html", "Units", "units"),
             ("graph.html", "Graph", "graph"), ("solids.html", "Solids", "solids"),
             ("issues.html", "Upstream issues", "issues"), ("ssi.html", "SSI corpus", "ssi")]
    extra_js = "".join(f'\n<script src="{up}assets/{js}"></script>' for js in scripts)
    navhtml = "".join(
        f'<a href="{up}{href}"{" aria-current=\"page\"" if key == nav else ""}>{label}</a>'
        for href, label, key in links)
    short = head_sha[:8]
    site_url = f"https://{REPO.split('/')[0]}.github.io/{REPO.split('/')[1]}/"
    canon_url = f"{site_url}{path}" if path else site_url
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{esc(title)}</title>
<meta name="description" content="{esc(description or title)}">
<meta name="theme-color" content="#141413">
<meta property="og:type" content="website">
<meta property="og:site_name" content="solidcore">
<meta property="og:url" content="{canon_url}">
<meta property="og:title" content="{esc(title)}">
<meta property="og:description" content="{esc(description or title)}">
<meta property="og:image" content="{site_url}assets/og-image.png">
<meta property="og:image:width" content="1200">
<meta property="og:image:height" content="630">
<meta property="og:image:type" content="image/png">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="{esc(title)}">
<meta name="twitter:description" content="{esc(description or title)}">
<meta name="twitter:image" content="{site_url}assets/og-image.png">
<link rel="stylesheet" href="{up}assets/style.css">
<link rel="icon" href="{up}assets/logo.svg" type="image/svg+xml">
</head>
<body>
<header class="top"><div class="wrap">
<a class="brand" href="{up}index.html"><img src="{up}assets/logo.svg" alt="" width="28" height="28">solidcore</a>
<nav>{navhtml}<a href="{VIEWER_URL}">Model viewer</a><a href="{REPO_URL}">GitHub</a></nav>
</div></header>
<main class="wrap">
{body}
</main>
<footer class="wrap">Generated from <a href="{REPO_URL}/tree/{head_sha}"><code>{short}</code></a>
({esc(head_date)}) by <code>site/build.py</code>; upstream reference {UPSTREAM_PIN_HTML}.
Apache-2.0. Not affiliated with or endorsed by NVIDIA.</footer>
<script src="{up}assets/app.js"></script>{extra_js}
</body>
</html>
"""


def badge(status):
    return f'<span class="badge s-{esc(status)}">{esc(STATUS_TEXT.get(status, status))}</span>'


def fmt_int(n):
    return f"{n:,}"


# ---------------------------------------------------------------- chart

def chart_svg(points, cid="chart", title="Share of upstream lines ported over time"):
    """Stacked area: % of upstream lines green, and green + partial, over time.

    `cid` makes the SVG title id unique when a page holds several charts.
    """
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
             f'aria-labelledby="{esc(cid)}-title" data-points="{esc(json.dumps([[d[0], round(d[1], 2), round(d[2], 2), d[3], d[4]] for d in data]))}" '
             f'data-x0="{t0}" data-x1="{t1}" data-l="{L}" data-r="{R}" data-w="{W}">',
             f'<title id="{esc(cid)}-title">{esc(title)}</title>']
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
    legend = ('<div class="legend"><span><i class="sw green"></i>green (matches C++ within 1e-9)</span>'
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


def status_steps(hist, t0, t1):
    """Compact status timeline for one unit: a step bar plus the list of transitions.

    `hist` is [(t, status, est_lines)] over the sampled states the unit exists in; the bar
    spans the whole sampled history [t0, t1], so units added later start with a gap.
    """
    if not hist or t1 <= t0:
        return '<p class="muted">No history for this unit yet.</p>'
    runs = []  # [t, status, lines] at each status change
    for t, st, n in hist:
        if not runs or runs[-1][1] != st:
            runs.append([t, st, n])
    span = t1 - t0
    segs = []
    if runs[0][0] > t0:
        w = 100 * (runs[0][0] - t0) / span
        segs.append(f'<i class="st-none" style="width:{w:.2f}%" title="not in the plan yet"></i>')
    for i, (t, st, n) in enumerate(runs):
        end = runs[i + 1][0] if i + 1 < len(runs) else t1
        w = 100 * (end - t) / span
        segs.append(f'<i class="st-{esc(st)}" style="width:{w:.2f}%" '
                    f'title="{esc(st)} from {time.strftime("%Y-%m-%d", time.gmtime(t))}"></i>')
    label = ", then ".join(f"{st} from {time.strftime('%Y-%m-%d', time.gmtime(t))}" for t, st, _ in runs)
    items = "".join(f"<li><time>{time.strftime('%Y-%m-%d %H:%M', time.gmtime(t))} UTC</time> {badge(st)}"
                    f" <span class=muted>{fmt_int(n)} lines</span></li>" for t, st, n in runs)
    d0, d1 = (time.strftime("%b %d", time.gmtime(v)) for v in (t0, t1))
    return (f'<div class="steps" role="img" aria-label="Status history: {esc(label)}">{"".join(segs)}</div>'
            f'<div class="steps-axis small"><span>{d0}</span><span>{d1}</span></div>'
            f'<ol class="steplist">{items}</ol>')


# ---------------------------------------------------------------- dependency graph

DONE = ("green", "partial")


def graph_kind(u, status):
    """done / claimed / ready / waiting / blocked, for the graph legend and counts."""
    if u["status"] in DONE:
        return "done"
    if u["status"] == "claimed":
        return "claimed"
    if u["status"] == "blocked":
        return "blocked"
    return "ready" if progress.is_ready(u, status) else "waiting"


def build_graph(out, units, status, dependents, mods, head_sha):
    """Write assets/graph.json (units and their depends_on edges); return page data.

    `blocking` counts the not-done units that depend on a unit, directly or not.
    """
    by_id = {u["id"]: u for u in units}

    def blocking(uid):
        seen, stack = set(), [uid]
        while stack:
            for d in dependents.get(stack.pop(), []):
                if d not in seen and status.get(d) not in DONE:
                    seen.add(d)
                    stack.append(d)
        return len(seen)

    nodes = []
    for u in sorted(units, key=lambda u: (u["layer"], u["id"])):
        kind = graph_kind(u, status)
        nodes.append({
            "id": u["id"], "area": progress.module(u), "layer": u["layer"], "status": u["status"],
            "kind": kind, "lines": u["est_lines"], "claim": u.get("claim", ""),
            "ready": u["status"] not in DONE and all(status.get(d) in DONE for d in u["depends_on"]),
            "blocked_by": sum(1 for d in u["depends_on"] if status.get(d) not in DONE),
            "blocking": blocking(u["id"]) if u["status"] not in DONE else 0,
            "deps": sorted(d for d in u["depends_on"] if d in by_id),
        })
    areas = [m for m in mods if any(n["area"] == m for n in nodes)]
    data = {"commit": head_sha, "areas": areas, "nodes": nodes}
    (out / "assets/graph.json").write_text(json.dumps(data, separators=(",", ":"), sort_keys=True))
    return {"nodes": nodes, "areas": areas,
            "edges": sum(len(n["deps"]) for n in nodes),
            "count": collections.Counter(n["kind"] for n in nodes)}


def graph_body(g):
    nodes, count = g["nodes"], g["count"]
    ready = [n for n in nodes if n["kind"] == "ready"]
    ready.sort(key=lambda n: (-n["blocking"], n["layer"], n["id"]))
    todo = sorted((n for n in nodes if n["kind"] != "done"), key=lambda n: (-n["blocking"], n["layer"], n["id"]))[:15]

    def row(n):
        claim = f'<div class="small">claimed: {inline(n["claim"])}</div>' if n["claim"] else ""
        return (f'<tr><td><a href="units/{esc(n["id"])}.html"><code>{esc(n["id"])}</code></a>{claim}</td>'
                f'<td>{badge(n["status"])}</td><td class=num>{fmt_int(n["lines"])}</td>'
                f'<td class=num>{n["blocked_by"]}</td><td class=num>{n["blocking"]}</td></tr>')

    head = ('<thead><tr><th data-sort="text">Unit</th><th data-sort="text">Status</th><th data-sort="num">Lines</th>'
            '<th data-sort="num">Blocked by</th><th data-sort="num">Blocking</th></tr></thead>')
    area_opts = "".join(f'<option value="{esc(m)}">{esc(m)}</option>' for m in g["areas"])
    tiles = "".join(
        f'<div class="tile"><div class="k"><i class="gsw {k}"></i>{label}</div><div class="v">{count[k]}</div>'
        f'<div class="sub">{sub}</div></div>'
        for k, label, sub in [("done", "done", "green or partial"),
                              ("ready", "ready", "todo, every dependency done"),
                              ("claimed", "claimed", f"todo with an open port: PR ({sum(1 for n in nodes if n['kind'] == 'claimed' and n['ready'])} with every dependency done)"),
                              ("waiting", "waiting", "todo, a dependency not done")]
        + ([("blocked", "blocked", "see notes")] if count["blocked"] else []))
    return f"""
<h1>Dependency graph</h1>
<p class="muted">{len(nodes)} units and their {fmt_int(g['edges'])} <code>depends_on</code> edges from
<code>plan/units/</code> (soft dependencies are not drawn). Units cluster by module, and dependency
edges pull related modules together. Data: <a href="assets/graph.json"><code>assets/graph.json</code></a>.</p>
<section class="kpis">{tiles}</section>
<div class="graphui" hidden>
<div class="filters">
<label>Search <input type="search" id="g-search" placeholder="unit id"></label>
<label>Module <select id="g-area"><option value="">all</option>{area_opts}</select></label>
<span class="count small">Scroll or pinch to zoom, drag to pan. Click a unit for its dependencies and dependents.</span>
</div>
<div class="legend">
<span><i class="gsw done"></i>green</span><span><i class="gsw partial"></i>partial</span>
<span><i class="gsw todo"></i>todo</span><span><i class="gsw ready"></i>every dependency done (ready)</span>
<span><i class="gsw claimed"></i>claimed (open PR)</span>
<span><i class="gsw up"></i>dependency of selection</span><span><i class="gsw down"></i>dependent of selection</span>
</div>
<div class="graphbox"><svg id="graph" role="img" aria-labelledby="graph-title"><title id="graph-title">Port units and
their dependencies</title></svg><div class="tip" hidden></div></div>
<div id="g-info" class="ginfo" aria-live="polite"><p class="muted">No unit selected.</p></div>
</div>
<noscript><p class="muted">The interactive graph needs JavaScript; the lists below have the same data.</p></noscript>
<h2>Ready to claim</h2>
<p class="muted">Todo units whose hard dependencies are all green or partial, most-blocking first. Claim one
with a draft PR titled <code>port: &lt;unit id&gt;</code> (<a href="{REPO_URL}/blob/main/AGENTS.md">how</a>).</p>
{('<div class="scroll"><table class="sortable">' + head + '<tbody>' + ''.join(row(n) for n in ready) + '</tbody></table></div>') if ready else '<p class="muted">None right now.</p>'}
<h2>Most-blocking unfinished units</h2>
<p class="muted">Units not yet green or partial, by how many unfinished units depend on them (directly or not).</p>
{('<div class="scroll"><table class="sortable">' + head + '<tbody>' + ''.join(row(n) for n in todo) + '</tbody></table></div>') if todo else '<p class="muted">None.</p>'}
"""


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
    solids = load_solids()
    ssi = json.loads((ROOT / "reports/ssi-corpus.json").read_text())
    intro_md, issues = parse_issues()
    points, area_points, unit_hist = aggregate(timeline())
    tl0, tl1 = (points[0][0], points[-1][0]) if points else (0, 0)

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
    (out / "areas").mkdir()
    (out / "assets").mkdir()
    for f in ("style.css", "app.js", "graph.js", "og-image.png"):
        shutil.copy(ROOT / "site/assets" / f, out / "assets" / f)
    shutil.copytree(ROOT / "site/assets/vendor", out / "assets/vendor")
    shutil.copy(ROOT / "assets/logo.svg", out / "assets/logo.svg")
    P = lambda *a, **k: page(*a, head_sha=head_sha, head_date=head_date, **k)  # noqa: E731

    def write(rel, text):
        (out / rel).write_text(text)

    # The checklist harness/plan/progress.py generated (not committed; pages.yml runs it).
    checklist = ROOT / "plan/PROGRESS.md"
    if checklist.exists():
        shutil.copy(checklist, out / "PROGRESS.md")

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
            f'<tr><td><a href="areas/{area_slug(m)}.html">{esc(m)}</a></td>'
            f'<td class=num>{ms["count"]["green"] + ms["count"]["partial"]} / {len(us)}</td>'
            f'<td class=num>{fmt_int(ms["done_lines"])} / {fmt_int(ms["total_lines"])}</td>'
            f'<td class="barcell"><div class="bar" title="{g:.1f}% green, {p:.1f}% partial">'
            f'<i class="green" style="width:{g:.2f}%"></i><i class="partial" style="width:{p:.2f}%"></i></div></td>'
            f'<td class=num>{100 * ms["done_lines"] / tot:.0f}%</td></tr>')
    present = [m for m in mods if any(progress.module(u) == m for u in units)]
    area_opts = "".join(f'<option value="{esc(m)}">{esc(m)}</option>' for m in present)
    area_sets = "".join(
        f'<div class="chartset" data-area="{esc(m)}" hidden>'
        f'{chart_svg(area_points.get(m, []), f"chart-{area_slug(m)}", f"Share of {m} upstream lines ported over time")}'
        f'<p class="small"><a href="areas/{area_slug(m)}.html">{esc(m)} module page</a></p></div>'
        for m in present)
    ssum = ssi.get("summary", {})
    verdicts = sorted(set(ssum.get("cpp", {})) | set(ssum.get("rust", {})))
    ssi_rows = "".join(f"<tr><td>{esc(v)}</td><td class=num>{ssum.get('cpp', {}).get(v, 0)}</td>"
                       f"<td class=num>{ssum.get('rust', {}).get(v, 0)}</td></tr>" for v in verdicts)
    par = ssum.get("parity", {})
    s_total, s_built, s_passed = solids_counts(solids)
    solids_tile = (f'<div class="tile"><div class="k"><a href="solids.html">Solids</a></div>'
                   + (f'<div class="v">{s_passed} / {s_total}</div><div class="sub">primitives pass; '
                      f'{s_built} / {s_total} build</div>' if solids else
                      '<div class="v">–</div><div class="sub">no data yet</div>') + '</div>')
    body = f"""
<section class="hero">
<h1>solidcore</h1>
<p class="lede">solidcore is a Rust port of NVIDIA's SMLib, the boundary-representation (B-rep) solid
modeling kernel from <a href="{UPSTREAM_URL}">usd-brep</a> (Apache-2.0; ported from and tested against
{UPSTREAM_PIN_HTML}), written in safe Rust: no
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
<div class="sub">{fmt_int(s['done_lines'])} of {fmt_int(s['total_lines'])} lines of usd-brep
v{esc(UPSTREAM_VERSION)} in green or partial units; {gpct:.1f}% green</div></div>
{tiles}
<div class="tile"><div class="k">ready to claim</div><div class="v">{ready_n}</div><div class="sub">todo with all dependencies done</div></div>
{solids_tile}
</section>
{reference_history_html()}<section>
<h2>Progress over time</h2>
<p class="muted">Share of in-scope upstream lines in green and partial units, from
<code>plan/units/</code> along <code>main</code>'s history ({len(points)} sampled states).</p>
<div class="chartpick" hidden><label>Module <select data-charts="overview-charts">
<option value="all">all units</option>{area_opts}</select></label></div>
<div id="overview-charts">
<div class="chartset" data-area="all">{chart_svg(points, "chart-all")}</div>
{area_sets}
</div>
{chart_table(points)}
</section>
<section>
<h2>By module</h2>
<p class="muted">Click a module for its own progress chart and unit list.</p>
<div class="scroll"><table class="modtable"><thead><tr><th>Module</th><th>Units ported</th><th>Lines</th>
<th class="barcell">Progress</th><th>%</th></tr></thead><tbody>{''.join(modrows)}</tbody></table></div>
<p class="legend"><span><i class="sw green"></i>green</span><span><i class="sw partial"></i>partial</span></p>
</section>
<section>
<h2>Surface/surface intersection corpus</h2>
<p>{len(ssi.get('cases', []))} cases from the external
<a href="{esc(ssi.get('corpus', {}).get('repo', ''))}">ssi-corpus</a>, scored against known true intersections.
Rust and C++ are compared where the Rust path is ported: {par.get('compared', 0)} compared,
{par.get('identical', 0)} agree within 1e-9, {par.get('mismatched', 0)} mismatched.
<a href="ssi.html">All cases</a>.</p>
<div class="scroll narrow"><table><thead><tr><th>Verdict</th><th>C++</th><th>Rust</th></tr></thead>
<tbody>{ssi_rows}</tbody></table></div>
</section>
<section>
<h2>Elsewhere</h2>
<ul>
<li><a href="units/index.html">All {len(units)} units</a>, with difftest results and blockers.</li>
<li><a href="graph.html">Dependency graph</a>: what is ready to claim and what blocks the most.</li>
<li><a href="solids.html">Solids</a>: {f"{s_built}/{s_total} primitives build, {s_passed}/{s_total} pass" if solids else "no data yet"}, and what blocks each.</li>
<li><a href="issues.html">{len(issues)} upstream issues</a> found while porting.</li>
<li><a href="{VIEWER_URL}">Model viewer</a>: the upstream test models rendered by SMLib and by solidcore, side by side.</li>
<li><a href="{REPO_URL}">Repository</a>: <a href="{REPO_URL}/blob/main/AGENTS.md">how to port a unit</a>,
<a href="PROGRESS.md">checklist</a>.</li>
</ul>
</section>
"""
    write("index.html", P("solidcore progress", body, 0, "overview",
                          description="Port progress of solidcore, a Rust port of NVIDIA's SMLib."))

    # ---- module (area) pages
    for m in present:
        us = sorted((u for u in units if progress.module(u) == m), key=lambda u: (u["layer"], u["id"]))
        ms = progress.summary(us)
        tot = ms["total_lines"] or 1
        apts = area_points.get(m, [])
        urows = "".join(
            f'<tr><td><a href="../units/{esc(u["id"])}.html"><code>{esc(u["id"])}</code></a>'
            f'{" <span class=tag>ready</span>" if progress.is_ready(u, status) else ""}</td>'
            f'<td data-v="{STATUSES.index(u["status"]) if u["status"] in STATUSES else 9}">{badge(u["status"])}</td>'
            f'<td class=num>{u["layer"]}</td><td class=num data-v="{u["est_lines"]}">{fmt_int(u["est_lines"])}</td></tr>'
            for u in us)
        body = f"""
<p class="crumbs"><a href="../index.html">Overview</a> / module</p>
<h1>{esc(m)}</h1>
<dl class="facts">
<div><dt>Units ported</dt><dd>{ms['count']['green'] + ms['count']['partial']} / {len(us)}</dd></div>
<div><dt>Green</dt><dd>{ms['count']['green']}</dd></div>
<div><dt>Partial</dt><dd>{ms['count']['partial']}</dd></div>
<div><dt>Upstream lines ported</dt><dd>{fmt_int(ms['done_lines'])} / {fmt_int(ms['total_lines'])}
({100 * ms['done_lines'] / tot:.1f}%)</dd></div>
<div><dt>Lines green</dt><dd>{100 * ms['green_lines'] / tot:.1f}%</dd></div>
</dl>
<h2>Progress over time</h2>
<p class="muted">Share of this module's upstream lines in green and partial units, from
<code>plan/units/</code> along <code>main</code>'s history ({len(apts)} sampled states).</p>
{chart_svg(apts, f"chart-{area_slug(m)}", f"Share of {m} upstream lines ported over time")}
{chart_table(apts)}
<h2>Units</h2>
<p class="muted">{len(us)} units; <a href="../units/index.html?module={esc(m)}">open in the full units table</a>
for difftest counts and blockers.</p>
<div class="scroll"><table class="sortable"><thead><tr><th data-sort="text">Unit</th><th data-sort="num">Status</th>
<th data-sort="num">Layer</th><th data-sort="num">Lines</th></tr></thead><tbody>{urows}</tbody></table></div>
"""
        write(f"areas/{area_slug(m)}.html", P(f"{m} · solidcore", body, 1, "overview", path=f"areas/{area_slug(m)}.html",
                                   description=f"Port progress of the {m} module of solidcore."))

    # ---- units table
    modules = sorted({progress.module(u) for u in units})
    rows = []
    for u in sorted(units, key=lambda u: (u["layer"], u["id"])):
        rep = reports.get(u["id"], (None, None))[0]
        nfn = len(callgraph.get(u["id"], {}).get("functions", []))
        if rep:
            # a confirmed C++ defect run anyway (cpp_defect) counts as passed; a parked one
            # (not run) is neither passed nor failed and is listed on the unit page
            cp = rep.get("cases_passed", 0) + rep.get("cases_cpp_defect", 0)
            cd, cf = rep.get("cases_deferred", 0), rep.get("cases_failed", 0)
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
<p class="muted">{len(units)} port units from <code>plan/units/</code>. Cases come from
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
    write("units/index.html", P("Units · solidcore", body, 1, "units", path="units/index.html"))

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
        if rep_path is not None:
            (out / "reports").mkdir(parents=True, exist_ok=True)
            shutil.copy(rep_path, out / "reports" / f"{uid}.json")
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
            defects = [c for c in rep.get("cases", []) if isinstance(c, dict) and c.get("outcome") == "cpp_defect"]
            cpp_defects = ("<p>Cases where the C++ reference is verified wrong (a confirmed upstream issue; "
                           "the Rust result is kept and the case counts as passed): "
                           + ", ".join(f'<code>{esc(c.get("name", ""))}</code> ({esc(c.get("cpp_defect", ""))})'
                                       for c in defects) + "</p>") if defects else ""
            parked = [c for c in rep.get("cases", []) if isinstance(c, dict) and c.get("outcome") == "parked"]
            parked_cases = ("<p>Parked cases: the C++ reference is verified wrong (a confirmed upstream "
                            "issue), so they are not run in the gate and count neither as passed nor as "
                            "failed. A nightly job runs them against upstream's latest release and reports "
                            "any that start to agree; removing the entry's <code>cpp_defect</code> runs one "
                            "again. "
                            + ", ".join(f'<code>{esc(c.get("name", ""))}</code> ({esc(c.get("cpp_defect", ""))})'
                                        for c in parked) + "</p>") if parked else ""
            diff = f"""
<dl class="facts">
<div><dt>Cases</dt><dd>{fmt_int(rep.get('cases_total', 0))}</dd></div>
<div><dt>Passed</dt><dd>{fmt_int(rep.get('cases_passed', 0))}</dd></div>
{f"<div><dt>C++ defect (counted as passed)</dt><dd>{fmt_int(rep['cases_cpp_defect'])}</dd></div>" if rep.get('cases_cpp_defect') else ''}
{f"<div><dt>Parked (C++ defect, not run)</dt><dd>{fmt_int(rep['cases_parked'])}</dd></div>" if rep.get('cases_parked') else ''}
{f"<div><dt>Known mismatch</dt><dd>{fmt_int(rep['cases_known_mismatch'])}</dd></div>" if rep.get('cases_known_mismatch') else ''}
<div><dt>Deferred</dt><dd>{fmt_int(rep.get('cases_deferred', 0))}</dd></div>
<div><dt>Failed</dt><dd>{fmt_int(rep.get('cases_failed', 0))}</dd></div>
<div><dt>Both errored</dt><dd>{fmt_int(rep.get('cases_both_errored', 0))}</dd></div>
<div><dt>Max rel. error</dt><dd>{esc(repr(rep.get('max_rel_err')))}</dd></div>
<div><dt>Tolerance</dt><dd>{esc(repr(rep.get('rel_tol')))} rel.</dd></div>
<div><dt>Seeds</dt><dd>{esc(str(rep.get('seeds', '')))}</dd></div>
</dl>
{('<p>Absolute floors: <code>' + esc(json.dumps(floors)) + '</code></p>') if floors else ''}
{cpp_defects}
{parked_cases}
<h3>Blocked by</h3>
{('<div class="scroll narrow"><table><thead><tr><th>Unported unit</th><th>Deferred cases</th></tr></thead><tbody>' + dtable + '</tbody></table></div>') if dtable else '<p class="muted">Nothing: no deferred cases.</p>'}
<p><a href="../reports/{esc(uid)}.json">Report JSON</a> (per-case results, from CI's latest <code>main</code> run)</p>"""
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
<p class="crumbs"><a href="index.html">Units</a> / <a href="../areas/{area_slug(progress.module(u))}.html">{esc(progress.module(u))}</a></p>
<h1><code>{esc(uid)}</code> {badge(u['status'])}{' <span class=tag>ready</span>' if progress.is_ready(u, status) else ''}</h1>
<dl class="facts">
<div><dt>Layer</dt><dd>{u['layer']}</dd></div>
<div><dt>Upstream lines</dt><dd>{fmt_int(u['est_lines'])}</dd></div>
<div><dt>Functions</dt><dd>{len(fns)}</dd></div>
<div><dt>Rust module</dt><dd><code>{esc(u['rust_module'])}</code></dd></div>
</dl>
{claim}
<h2>Status history</h2>
{status_steps(unit_hist.get(uid, []), tl0, tl1)}
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
        write(f"units/{uid}.html", P(f"{uid} · solidcore", body, 1, "units", path=f"units/{uid}.html"))

    # ---- dependency graph
    gstats = build_graph(out, units, status, dependents, mods, head_sha)
    write("graph.html", P("Dependency graph · solidcore", graph_body(gstats), 0, "graph", path="graph.html",
                          description="Port units of solidcore and their dependencies, by status.",
                          scripts=("vendor/d3-force.bundle.min.js", "graph.js")))

    # ---- solids
    if solids:
        (out / "reports").mkdir(parents=True, exist_ok=True)
        (out / "reports/solids.json").write_text(json.dumps(solids, indent=1, sort_keys=True))
    write("solids.html", P("Solids · solidcore", solids_body(solids), 0, "solids", path="solids.html",
                           description="Which solids solidcore builds in pure Rust, and what blocks the rest."))

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
<p class="muted">{len(issues)} entries from <a href="{REPO_URL}/tree/main/plan/upstream-issues"><code>plan/upstream-issues/</code></a>
(one file each), rendered as written: {', '.join(f'{sc[k]} {k}' for k in SERIES if sc[k])}; {cc['yes']} confirmed by a C++ run.</p>
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
    write("issues.html", P("Upstream issues · solidcore", body, 0, "issues", path="issues.html"))

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
        ptxt = ("agree" if par.get("pass") and not par.get("mismatches") else
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
    write("ssi.html", P("SSI corpus · solidcore", body, 0, "ssi", path="ssi.html"))
    return {"units": len(units), "areas": len(present), "issues": len(issues), "ssi": len(cases),
            "solids": "{1}/{0} build, {2}/{0} pass".format(*solids_counts(solids)) if solids else "no data",
            "timeline": len(points),
            "seconds": time.time() - t_start}



def reference_history_html():
    """Rows for site/reference-history/*.json: one file per past upstream pin."""
    rows = []
    for f in sorted((ROOT / "site" / "reference-history").glob("*.json")):
        h = json.loads(f.read_text())
        pk, fn, nx = h.get("peak", {}), h.get("final", {}), h.get("superseded_by", {})
        peak = (f'{pk.get("green")}/{pk.get("units")} green on {esc(pk.get("date", ""))} '
                f'(<a href="{REPO_URL}/pull/{pk.get("pr")}">#{pk.get("pr")}</a>, '
                f'<code>{esc(pk.get("commit", ""))}</code>)') if pk else ""
        final = (f'{fn.get("green")} green, {fn.get("partial", 0)} partial at '
                 f'<code>{esc(fn.get("commit", ""))}</code>: {esc(fn.get("note", ""))}') if fn else ""
        moved = (f'v{esc(nx.get("version", ""))} on {esc(nx.get("date", ""))} '
                 f'(<a href="{REPO_URL}/pull/{nx.get("pr")}">#{nx.get("pr")}</a>)') if nx else ""
        rows.append(f'<tr><td>v{esc(h["version"])} <code>{esc(h.get("commit", ""))}</code></td>'
                    f'<td>{peak}</td><td>{final}</td><td>{moved}</td></tr>')
    if not rows:
        return ""
    return ('<section>\n<h2>Earlier upstream pins</h2>\n<p class="muted">Difftest results against '
            'previous C++ references, kept after the pin moved on.</p>\n<div class="scroll"><table>'
            '<thead><tr><th>Pin</th><th>Best</th><th>Last state</th><th>Moved to</th></tr></thead><tbody>'
            + "".join(rows) + '</tbody></table></div>\n</section>\n')

def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--out", default=str(ROOT / "bin/site"))
    a = ap.parse_args()
    r = build(pathlib.Path(a.out).resolve())
    print(f"site: {r['units']} unit pages, {r['areas']} module pages, {r['issues']} issues, {r['ssi']} SSI cases, solids {r['solids']}, "
          f"{r['timeline']} timeline points -> {a.out} ({r['seconds']:.1f}s)")


if __name__ == "__main__":
    main()

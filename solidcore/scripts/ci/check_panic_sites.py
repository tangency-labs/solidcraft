#!/usr/bin/env python3
"""Fails if library code gains a panic-prone call that the deny lints don't see (#2490).

The workspace denies unwrap/expect/panic/indexing. What they miss: integer `/` and `%` (panic
on a zero divisor), and std calls that panic on a bad index, length or bound (`Vec::remove`,
`insert`, `chunks`/`windows`, `f64::clamp`, `swap`, `split_at`, ...) or on capacity overflow
(`with_capacity`). Each such call in non-test library code of the crates applications use
(CRATES) is listed in plan/panic-sites.tsv. A new site fails this check, unless a mechanical
rule (AUTO_PROVEN: a literal `windows(2)`, `clamp(-1.0, 1.0)`, divisor) proves it cannot
panic. A listed site that is gone is only reported: removing a site never fails a build, and
the next `--update` drops its row.

Sites come from one `cargo clippy --lib --message-format=json` run, so every receiver type
is resolved: a `.remove(` on a `BTreeMap` (returns Option) is not a `Vec::remove`. The method
calls are clippy's `disallowed_methods` with the paths in METHODS (in a temporary clippy.toml,
so the normal clippy run is unaffected); `/` and `%` are `integer_division_remainder_used`,
which fires on integer operands only, and `/=`, `%=` the matching `arithmetic_side_effects`
sites (see DIVISION). Overflow (the rest of `arithmetic_side_effects`, ~10k sites) is not
listed: CI's tests run with overflow checks on, so an overflow they reach already fails.

A row is `file, fn, pattern, n, status, code, reason`. `fn` is the enclosing function path
(`Type::method`, `outer::inner`; `-` outside any function) and `n` counts the pattern's sites
within it in source order, so the key survives edits elsewhere in the file. `code` is the
call's text. `status` is `open`, or `proven` with a `reason` when the site cannot panic
(constant non-zero divisor, length checked just above); proven rows are exempt from the
ratchet, but their `code` must still match, so a mark cannot slide onto another call.
With --base, an `open` row must already be open in the base list (by key, or by pattern
and code when the function moved), so a new site cannot be listed as open to pass.

Usage: check_panic_sites.py [--base <rev>] [--update] [--clippy-json <file>] [<repo root>]
  --update       rewrite the list from the tree: drops gone sites, adds new ones as open
                 (or proven, for the mechanical rules in AUTO_PROVEN), keeps marks.
  --clippy-json  read clippy's JSON messages from a file instead of running cargo.
"""
import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

LIST = Path("plan/panic-sites.tsv")
# solidcore and the STEP crates SolidCraft links. nurbs-grid-mesh is not here: nothing
# depends on it, and docs/layers.md has it merged into solidcore-tess and then deleted.
CRATES = ["solidcore", "step-brep", "step-import", "step-export"]
COLUMNS = ["file", "fn", "pattern", "n", "status", "code", "reason"]

# clippy `disallowed-methods` path -> pattern name. Only calls that can panic on their
# arguments; the panic-free namesakes (maps, sets, `VecDeque::remove`) are other paths.
METHODS = {
    "alloc::vec::Vec::remove": "Vec::remove",
    "alloc::vec::Vec::insert": "Vec::insert",
    "alloc::vec::Vec::swap_remove": "Vec::swap_remove",
    "alloc::vec::Vec::drain": "Vec::drain",
    "alloc::vec::Vec::split_off": "Vec::split_off",
    "alloc::vec::Vec::with_capacity": "with_capacity",
    "alloc::string::String::with_capacity": "with_capacity",
    "alloc::collections::VecDeque::with_capacity": "with_capacity",
    "alloc::string::String::remove": "String::remove",
    "alloc::string::String::insert": "String::insert",
    "alloc::string::String::insert_str": "String::insert",
    "alloc::string::String::truncate": "String::truncate",
    "alloc::collections::VecDeque::insert": "VecDeque::insert",
    "alloc::collections::VecDeque::swap": "swap",
    "slice::swap": "swap",
    "slice::chunks": "chunks",
    "slice::chunks_mut": "chunks",
    "slice::chunks_exact": "chunks",
    "slice::chunks_exact_mut": "chunks",
    "slice::rchunks": "chunks",
    "slice::windows": "windows",
    "slice::split_at": "split_at",
    "slice::split_at_mut": "split_at",
    "slice::copy_from_slice": "copy_from_slice",
    "slice::clone_from_slice": "copy_from_slice",
    "slice::rotate_left": "rotate",
    "slice::rotate_right": "rotate",
    "f64::clamp": "f64::clamp",
    "f32::clamp": "f64::clamp",
    "core::cmp::Ord::clamp": "Ord::clamp",
    "core::iter::Iterator::step_by": "step_by",
    "usize::div_ceil": "int-div",
    "u32::div_ceil": "int-div",
    "u64::div_ceil": "int-div",
    "usize::div_euclid": "int-div",
    "i32::div_euclid": "int-div",
    "i64::div_euclid": "int-div",
    "usize::rem_euclid": "int-rem",
    "i32::rem_euclid": "int-rem",
    "i64::rem_euclid": "int-rem",
}
LINTS = [
    "clippy::disallowed_methods",
    "clippy::integer_division_remainder_used",
    "clippy::arithmetic_side_effects",
]
# integer_division_remainder_used names the operator in its message: "use of `%` has been
# disallowed". It fires on integer operands only (std's Div/Rem impls), not on floats, but
# not on `/=` and `%=` either. Those come from arithmetic_side_effects, which skips floats and
# a non-zero literal divisor; the only other type with DivAssign/RemAssign is Vec3, allowed.
DIVISION = {"/": "int-div", "%": "int-rem"}
COMPOUND = re.compile(r"^[^=]*?([/%])=")
SIDE_EFFECTS_ALLOWED = ["Vec3", "geom::vector3d::Vec3", "solidcore::geom::vector3d::Vec3"]

INT = r"(?:_?(?:u8|u16|u32|u64|usize|i8|i16|i32|i64|isize))?"
LIT = r"[1-9][0-9_]*" + INT
FLT = r"-?[0-9][0-9_]*(?:\.[0-9_]*)?(?:e-?[0-9]+)?(?:_?f64|_?f32)?"
# A product or sum of positive literals, e.g. `(4 * 32)`: non-zero, and rustc rejects an
# overflowing constant expression at compile time.
CONST_EXPR = r"\(\s*" + LIT + r"(?:\s*[*+]\s*" + LIT + r")*\s*\)"
CONST_NAME = r"(?:Self::)?([A-Z][A-Z0-9_]*)"
# Mechanical proofs from the call's text alone, applied by --update to new rows.
AUTO_PROVEN = [
    ("int-div", r"/=?\s*(?:" + LIT + "|" + CONST_EXPR + r")$", "constant non-zero divisor"),
    ("int-rem", r"%=?\s*(?:" + LIT + "|" + CONST_EXPR + r")$", "constant non-zero divisor"),
    ("int-div", r"/=?\s*[\w.]+\.max\(" + LIT + r"\)$", "divisor is max(.., >= 1)"),
    ("int-rem", r"%=?\s*[\w.]+\.max\(" + LIT + r"\)$", "divisor is max(.., >= 1)"),
    ("chunks", r"\(" + LIT + r"\)$", "constant non-zero size"),
    ("windows", r"\(" + LIT + r"\)$", "constant non-zero size"),
    ("step_by", r"\(" + LIT + r"\)$", "constant non-zero step"),
    ("with_capacity", r"\((?:" + LIT + r"|0" + INT + r")\)$", "constant capacity"),
    ("Vec::insert", r"^insert\(0" + INT + r",", "index 0 is never past the end"),
]
CLAMP = re.compile(r"\(\s*(" + FLT + r")\s*,\s*(" + FLT + r")\s*\)$")
DIVISOR_CONST = re.compile(r"[/%]=?\s*" + CONST_NAME + r"$")


def const_value(src, name):
    """The value of `const NAME: <int> = <literal>;` if `src` defines NAME exactly once."""
    defs = re.findall(
        r"\bconst\s+" + name + r"\s*:\s*[iu](?:8|16|32|64|size)\s*=\s*([0-9][0-9_]*)" + INT + r"\s*;",
        src,
    )
    # every binding of the name, function and closure parameters included: a parameter
    # that shadows the constant makes the proof unsound (no scope resolution here)
    names = re.findall(
        r"\b(?:const|static|let(?:\s+mut)?)\s+" + name + r"\b"
        r"|[(,|]\s*(?:mut\s+)?" + name + r"\s*:",
        src,
    )
    return int(defs[0].replace("_", "")) if len(defs) == 1 and len(names) == 1 else None


def auto_reason(pattern, code, src=""):
    """The reason a site provably cannot panic, from its text alone, or None."""
    # `x /= n / 2`: a compound assignment's divisor is all the text after its operator, so
    # the proof must match from the operator on, not at the last `/` (the divisor's own)
    op = COMPOUND.match(code) if pattern in ("int-div", "int-rem") else None
    text, find = (code[op.start(1):], re.match) if op else (code, re.search)
    for pat, rx, reason in AUTO_PROVEN:
        if pat == pattern and find(rx, text):
            return reason
    m = find(DIVISOR_CONST, text) if pattern in ("int-div", "int-rem") else None
    if m and const_value(src, m.group(1)):
        return f"divisor is the non-zero constant {m.group(1)}"
    m = CLAMP.search(code) if pattern in ("f64::clamp", "Ord::clamp") else None
    if m:
        lo, hi = (float(re.sub(r"_|f64|f32", "", g)) for g in m.groups())
        if lo <= hi:
            return "constant bounds, min <= max"
    return None


# ---- source structure: the function enclosing an offset -----------------------------------


def blank_literals(src):
    """`src` with comments, strings and char literals replaced by spaces (newlines kept)."""
    out = list(src)
    i, n = 0, len(src)

    def blank(a, b):
        for k in range(a, min(b, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            blank(i, j)
            i = j
        elif src.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            blank(i, j)
            i = j
        elif (m := re.compile(r'(?:b|c)?r(#*)"').match(src, i)) and (
            i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")
        ):
            end = src.find('"' + m.group(1), m.end())
            j = n if end < 0 else end + 1 + len(m.group(1))
            blank(i, j)
            i = j
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            blank(i, j + 1)
            i = j + 1
        elif c == "'":
            if src.startswith("\\", i + 1):
                j = src.find("'", i + 3 if src.startswith("\\'", i + 1) else i + 2)
                j = n if j < 0 else j
                blank(i, j + 1)
                i = j + 1
            elif i + 2 < n and src[i + 2] == "'":
                blank(i, i + 3)
                i += 3
            else:
                i += 1  # a lifetime
        else:
            i += 1
    return "".join(out)


ITEM = re.compile(r"\b(fn|impl|trait|mod)\b|[{};()\[\]]")


def impl_name(header):
    """`Type` for `impl<..> Trait for path::Type<..> where ..`."""
    depth, flat = 0, []
    for ch in header:
        if ch == "<":
            depth += 1
        elif ch == ">":
            depth -= 1
        elif depth == 0:
            flat.append(ch)
    flat = re.split(r"\bwhere\b", "".join(flat))[0]
    flat = re.split(r"\bfor\b", flat)[-1]
    names = re.findall(r"[A-Za-z_]\w*", flat)
    names = [x for x in names if x not in ("dyn", "mut", "const", "unsafe")]
    return names[-1] if names else "impl"


def fn_spans(src):
    """(start, end, path) of every fn/impl/trait/mod body, outermost first."""
    text = blank_literals(src)
    stack, spans = [], []
    pending, paren = None, 0
    for m in ITEM.finditer(text):
        tok = m.group(0)
        if tok in "([":
            paren += 1
        elif tok in ")]":
            paren -= 1
        elif tok == "{":
            if pending and pending[2] == paren:
                stack.append((m.start(), pending[0], pending[1]))
            else:
                stack.append((m.start(), None, None))
            pending = None
        elif tok == "}":
            if stack:
                start, kind, name = stack.pop()
                if kind:
                    names = [s[2] for s in stack if s[1]] + [name]
                    spans.append((start, m.end(), "::".join(names)))
        elif tok == ";":
            if pending and pending[2] == paren:
                pending = None
        elif pending and pending[0] == "fn":
            continue  # `impl Trait` / `fn(..)` types inside a signature
        elif tok == "fn":
            nm = re.compile(r"\s*([A-Za-z_]\w*)").match(text, m.end())
            if nm:
                pending = ("fn", nm.group(1), paren)
        elif tok in ("trait", "mod"):
            nm = re.compile(r"\s*([A-Za-z_]\w*)").match(text, m.end())
            if nm:
                pending = (tok, nm.group(1), paren)
        elif tok == "impl":
            brace = text.find("{", m.end())
            semi = text.find(";", m.end())
            if brace >= 0 and (semi < 0 or brace < semi):
                pending = ("impl", impl_name(text[m.end() : brace]), paren)
    spans.sort()
    return spans


def enclosing(spans, offset):
    best = None
    for start, end, path in spans:
        if start > offset:
            break
        if offset < end and (best is None or start >= best[0]):
            best = (start, end, path)
    return best[2] if best else "-"


# ---- clippy messages -> sites ----------------------------------------------------------------

def normalize(code):
    """`code` on one line, single-spaced, without parentheses around the whole of it."""
    code = " ".join(code.split()).replace("( ", "(").replace(" )", ")")
    while code.startswith("(") and code.endswith(")"):
        depth = 0
        for k, ch in enumerate(code):
            depth += {"(": 1, ")": -1}.get(ch, 0)
            if depth == 0 and k < len(code) - 1:
                return code
        code = code[1:-1].strip()
    return code


def call_args(src, start):
    """The `(..)` (after an optional `::<..>`) that begins at byte `start` of `src`."""
    i = start
    if src.startswith(b"::<", i):
        depth, i = 1, i + 3
        while i < len(src) and depth:
            depth += {ord("<"): 1, ord(">"): -1}.get(src[i], 0)
            i += 1
    if not src.startswith(b"(", i):
        return b""
    depth, j = 0, i
    while j < len(src):
        depth += {ord("("): 1, ord(")"): -1}.get(src[j], 0)
        j += 1
        if depth == 0:
            return src[i:j]
    return b""


def span_code(span, src):
    """The text of the span; for a method or function path, with its call's arguments."""
    a, b = span["byte_start"], span["byte_end"]
    return src[a:b] + call_args(src, b)


def library_span(span, prefixes):
    """The span itself, or the macro call site it was expanded from, inside library code."""
    while span is not None:
        if span["file_name"].startswith(prefixes):
            return span
        exp = span.get("expansion")
        span = exp["span"] if exp else None
    return None


def raw_sites(messages, prefixes, read_bytes):
    """{(file, line, col, pattern): code} from clippy's JSON messages."""
    sites = {}
    for msg in messages:
        if msg.get("reason") != "compiler-message":
            continue
        diag = msg["message"]
        lint = (diag.get("code") or {}).get("code")
        if lint not in LINTS:
            continue
        primary = next((s for s in diag["spans"] if s["is_primary"]), None)
        span = library_span(primary, prefixes) if primary else None
        if span is None:
            continue
        code = span_code(span, read_bytes(span["file_name"]))
        code = normalize(code.decode("utf-8", "replace"))
        m = re.search(r"`([^`]+)`", diag["message"])
        if lint == "clippy::disallowed_methods":
            pattern = METHODS.get(m.group(1)) if m else None
        elif lint == "clippy::integer_division_remainder_used":
            pattern = DIVISION.get(m.group(1)) if m else None
        else:
            op = COMPOUND.match(code)
            pattern = DIVISION.get(op.group(1)) if op else None
        if pattern:
            sites[(span["file_name"], span["line_start"], span["column_start"], pattern)] = code
    return sites


def keyed_sites(raw, read_source):
    """[row dict] with fn path and occurrence index, sorted by key."""
    by_file = defaultdict(list)
    for (file, line, col, pattern), code in raw.items():
        by_file[file].append((line, col, pattern, code))
    rows = []
    for file, items in sorted(by_file.items()):
        src = read_source(file)
        starts = [0]
        for ln in src.splitlines(keepends=True):
            starts.append(starts[-1] + len(ln))
        spans = fn_spans(src)
        counts = Counter()
        for line, col, pattern, code in sorted(items):
            fn = enclosing(spans, starts[line - 1] + col - 1)
            counts[(fn, pattern)] += 1
            rows.append(
                {"file": file, "fn": fn, "pattern": pattern, "n": str(counts[(fn, pattern)]),
                 "status": "open", "code": code, "reason": ""}
            )
    return rows


def run_clippy(root):
    conf = (root / "clippy.toml").read_text()
    if "disallowed-methods" in conf:
        raise SystemExit("clippy.toml sets disallowed-methods; merge it into METHODS here first")
    with tempfile.TemporaryDirectory() as tmp:
        paths = ",\n".join(f'  "{p}"' for p in METHODS)
        allowed = ", ".join(f'"{t}"' for t in SIDE_EFFECTS_ALLOWED)
        Path(tmp, "clippy.toml").write_text(
            f"{conf}\ndisallowed-methods = [\n{paths},\n]\n"
            f"arithmetic-side-effects-allowed = [{allowed}]\n"
        )
        cmd = ["cargo", "clippy", "--lib", "--message-format=json"]
        for crate in CRATES:
            cmd += ["-p", crate]
        # a renamed or misspelt lint would otherwise pass with no sites at all
        cmd += ["--", "-D", "unknown_lints"] + [arg for lint in LINTS for arg in ("-W", lint)]
        env = dict(os.environ, CLIPPY_CONF_DIR=tmp)
        res = subprocess.run(cmd, cwd=root, env=env, stdout=subprocess.PIPE, text=True)
    if res.returncode != 0:
        raise SystemExit(f"cargo clippy failed ({res.returncode})")
    return parse_messages(res.stdout)


def parse_messages(text):
    out = []
    for line in text.splitlines():
        if line.startswith("{"):
            out.append(json.loads(line))
    unknown = [
        m["message"]["message"] for m in out
        if m.get("reason") == "compiler-message"
        and "does not refer to" in m["message"]["message"]
    ]
    if unknown:
        raise SystemExit("clippy did not resolve a METHODS path: " + "; ".join(unknown))
    lints = [
        m["message"]["message"] for m in out
        if m.get("reason") == "compiler-message"
        and m["message"]["message"].startswith("unknown lint")
    ]
    if lints:
        raise SystemExit("clippy does not know a LINTS name: " + "; ".join(lints))
    return out


# ---- the list ------------------------------------------------------------------------------


def key(row):
    return (row["file"], row["fn"], row["pattern"], row["n"])


def read_list(text):
    rows = []
    for line in text.splitlines():
        if not line or line.startswith("#"):
            continue
        cells = line.split("\t")
        if cells == COLUMNS:
            continue
        cells += [""] * (len(COLUMNS) - len(cells))
        rows.append(dict(zip(COLUMNS, cells)))
    return rows


HEADER = (
    "# Panic-prone calls in library code that no deny lint catches (#2490). Written by\n"
    "# scripts/ci/check_panic_sites.py --update; may only shrink. Fix a site with a typed\n"
    "# SolidError (checked arithmetic, .get(), a length check) and drop its row. `proven` marks\n"
    "# a site that cannot panic, with the reason; it is exempt from the ratchet.\n"
)


def write_list(rows):
    lines = [HEADER, "\t".join(COLUMNS) + "\n"]
    for r in sorted(rows, key=lambda r: (r["file"], r["fn"], r["pattern"], int(r["n"]))):
        lines.append("\t".join(r[c] for c in COLUMNS) + "\n")
    return "".join(lines)


def sig(row):
    """A site's identity for matching the tree with the list: its function, pattern and
    call text, not its occurrence index `n`, which a site added or removed earlier in
    the same function renumbers."""
    return (row["file"], row["fn"], row["pattern"], row["code"])


def excess(rows, others):
    """The rows of `rows` beyond the count of their `sig` in `others` (the last ones of
    each signature, in key order)."""
    room = Counter(sig(r) for r in others)
    seen = Counter()
    out = []
    for r in sorted(rows, key=key):
        seen[sig(r)] += 1
        if seen[sig(r)] > room[sig(r)]:
            out.append(r)
    return out


def check(current, listed, base=None, read_source=lambda f: ""):
    """Error messages: sites beyond the list's count of their call text in their function
    that no mechanical rule proves, malformed rows, new open rows. Matching by call text
    rather than by key means a renumbered site is never new, and a `proven` mark covers
    only the call text it was written for."""
    errors = []
    for r in excess(current, listed):
        if auto_reason(r["pattern"], r["code"], read_source(r["file"])) is not None:
            continue
        errors.append(
            f"{r['file']}: new panic-prone call `{r['code']}` ({r['pattern']}) in {r['fn']}. "
            "Return a typed SolidError instead (checked arithmetic, .get(), a length check). "
            "If it provably cannot panic, run `python3 scripts/ci/check_panic_sites.py "
            "--update` and mark its row in plan/panic-sites.tsv `proven` with the reason."
        )
    for r in listed:
        k = key(r)
        if r["status"] not in ("open", "proven"):
            errors.append(f"{r['file']}: row {k} has status `{r['status']}`, not open/proven")
        elif r["status"] == "proven" and not r["reason"].strip():
            errors.append(f"{r['file']}: proven row {k} has no reason")
    if base is not None:
        errors += ratchet(listed, base)
    return errors


def stale(current, listed):
    """A notice for rows that no longer match a site, or None."""
    gone = excess(listed, current)
    if not gone:
        return None
    names = ", ".join(f"{r['file']} {r['fn']} {r['pattern']} #{r['n']}" for r in gone[:5])
    more = f" and {len(gone) - 5} more" if len(gone) > 5 else ""
    return (
        f"{len(gone)} row(s) of plan/panic-sites.tsv no longer match a site ({names}{more}); "
        "`python3 scripts/ci/check_panic_sites.py --update` drops them."
    )


def ratchet(listed, base):
    """Open rows that were not open in the base list: matched by `sig` as a multiset (so
    a renumbering is no new row), then by pattern + call text among the leftovers (a
    function or file moved or renamed)."""
    base_open = [r for r in base if r["status"] == "open"]
    new_open = excess([r for r in listed if r["status"] == "open"], base_open)
    left = excess(base_open, [r for r in listed if r["status"] == "open"])
    spare = Counter((r["pattern"], r["code"]) for r in left)
    errors = []
    for r in new_open:
        if spare[(r["pattern"], r["code"])] > 0:
            spare[(r["pattern"], r["code"])] -= 1
            continue
        errors.append(
            f"{r['file']}: plan/panic-sites.tsv gains an open row ({r['fn']} {r['pattern']} "
            f"`{r['code']}`). The list may only shrink: fix the call with a typed SolidError, "
            "or mark it `proven` with the reason it cannot panic."
        )
    return errors


def update(current, listed, read_source=lambda f: ""):
    """The new list: current sites, keeping each mark whose code still matches, at its key
    or (after a renumbering) at a site with the same code in the same function."""
    lst = {key(r): r for r in listed if r["status"] == "proven"}
    kept = {key(r) for r in current if key(r) in lst and lst[key(r)]["code"] == r["code"]}
    marks = defaultdict(list)
    for k, r in lst.items():
        if k not in kept:
            marks[(r["file"], r["fn"], r["pattern"], r["code"])].append(r)
    out = []
    for r in current:
        r = dict(r)
        same = (r["file"], r["fn"], r["pattern"], r["code"])
        if key(r) in kept:
            r["status"], r["reason"] = "proven", lst[key(r)]["reason"]
        elif marks.get(same):
            r["status"], r["reason"] = "proven", marks[same].pop(0)["reason"]
        elif (reason := auto_reason(r["pattern"], r["code"], read_source(r["file"]))) is not None:
            r["status"], r["reason"] = "proven", reason
        out.append(r)
    return out


def summary(rows):
    by = defaultdict(Counter)
    for r in rows:
        by[r["pattern"]][r["status"]] += 1
    width = max((len(p) for p in by), default=0)
    lines = [f"{'pattern':<{width}}  open  proven"]
    for p in sorted(by):
        lines.append(f"{p:<{width}}  {by[p]['open']:>4}  {by[p]['proven']:>6}")
    total = Counter(r["status"] for r in rows)
    lines.append(f"{'total':<{width}}  {total['open']:>4}  {total['proven']:>6}")
    return "\n".join(lines)


def base_list(root, rev):
    git = ["git", "-C", str(root)]
    if subprocess.run(git + ["cat-file", "-e", f"{rev}^{{commit}}"]).returncode != 0:
        raise SystemExit(f"base revision {rev} is not in this clone (fetch it first)")
    res = subprocess.run(git + ["show", f"{rev}:{LIST}"], stdout=subprocess.PIPE,
                         stderr=subprocess.DEVNULL, text=True)
    return read_list(res.stdout) if res.returncode == 0 else None


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("root", nargs="?", default=".")
    ap.add_argument("--base")
    ap.add_argument("--update", action="store_true")
    ap.add_argument("--clippy-json")
    args = ap.parse_args(argv[1:])
    root = Path(args.root)
    if args.clippy_json:
        messages = parse_messages(Path(args.clippy_json).read_text())
    else:
        messages = run_clippy(root)
    prefixes = tuple(f"crates/{c}/src/" for c in CRATES)
    sources = {}

    def read_bytes(file):
        if file not in sources:
            sources[file] = (root / file).read_bytes()
        return sources[file]

    def read_source(file):
        return read_bytes(file).decode("utf-8", "replace")

    current = keyed_sites(raw_sites(messages, prefixes, read_bytes), read_source)
    path = root / LIST
    listed = read_list(path.read_text()) if path.exists() else []
    if args.update:
        rows = update(current, listed, read_source)
        path.write_text(write_list(rows))
        print(f"wrote {LIST}: {len(rows)} sites\n{summary(rows)}")
        return 0
    base = base_list(root, args.base) if args.base else None
    errors = check(current, listed, base, read_source)
    for e in errors:
        print(f"::error file={LIST}::{e}")
    if (notice := stale(current, listed)) is not None:
        print(f"::notice file={LIST}::{notice}")
    print(f"{LIST}:\n{summary(listed)}")
    if base is None and args.base:
        print(f"{args.base} has no {LIST}; the open-row ratchet starts with this list")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

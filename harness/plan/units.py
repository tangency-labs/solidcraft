#!/usr/bin/env python3
"""Shared loader and writer for the port plan: one file per unit, `plan/units/<id>.toml`.

Format: `plan/modules.schema.md`. Each file holds one unit's fields as top-level keys
(`id`, `upstream`, `rust_module`, ...); `id` must equal the file name. Every tool that
reads the plan goes through this module (harness/plan/progress.py, site/build.py,
harness/plan/soften.py, harness/callgraph/plan_units.py, scripts/units-split.py,
scripts/units-migrate-branch.sh); the Rust side (`crates/difftest/src/plan.rs`) follows
the same rules. Units are always returned sorted by id.

Before the split (PR "infra: stop shared-file conflicts") the plan was the single file
`plan/modules.toml` with `[[unit]]` tables; `parse_modules_toml` and `split_modules_toml`
still read that layout for old revisions (site timeline, branch migration).

    python3 harness/plan/units.py check                       # parse every unit file
    python3 harness/plan/units.py migrate BASE TIP [--root R]  # see units-migrate-branch.sh
"""
import json
import pathlib
import re
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[2]
UNITS_DIR = pathlib.PurePosixPath("plan/units")
# Canonical key order of a unit file (plan/modules.schema.md).
FIELDS = ["id", "upstream", "rust_module", "depends_on", "soft_depends_on", "est_lines",
          "layer", "tests", "status", "claimed_by", "claimed_at", "notes"]
STATUSES = ["todo", "claimed", "partial", "green", "blocked", "out_of_scope"]
_ID_RE = re.compile(r"^[A-Za-z0-9_.\-]+$")


# ---------------------------------------------------------------- reading

def parse_unit(text, name="<unit>"):
    """One unit from the text of a `plan/units/<id>.toml` file."""
    u = tomllib.loads(text)
    if not isinstance(u.get("id"), str):
        raise ValueError(f"{name}: missing id")
    return u


def units_dir(root=None):
    return pathlib.Path(root or ROOT) / UNITS_DIR


def load_units(root=None):
    """All units of the working tree at `root` (default: the repository), sorted by id."""
    out = []
    for p in sorted(units_dir(root).glob("*.toml")):
        u = parse_unit(p.read_text(), str(p))
        if u["id"] != p.stem:
            raise ValueError(f"{p}: id {u['id']!r} does not match the file name")
        out.append(u)
    return out


def units_from_texts(texts):
    """Units from {file stem: text} (e.g. blobs of a git tree), sorted by id."""
    return [parse_unit(texts[k], k) for k in sorted(texts)]


def parse_modules_toml(text):
    """Units of the old single-file `plan/modules.toml`, sorted by id."""
    return sorted(tomllib.loads(text).get("unit", []), key=lambda u: u["id"])


def split_modules_toml(text):
    """{id: unit file text} from the old `plan/modules.toml`, keeping each unit's lines
    (including comment lines that follow its fields) verbatim."""
    blocks = re.split(r"(?m)^\[\[unit\]\][ \t]*\n", text)
    out = {}
    for b in blocks[1:]:
        body = b.rstrip() + "\n"
        u = parse_unit(body)
        if u["id"] in out:
            raise ValueError(f"duplicate unit {u['id']}")
        out[u["id"]] = body
    parsed = {u["id"]: u for u in tomllib.loads(text).get("unit", [])}
    if set(parsed) != set(out):
        raise ValueError("split lost units: " + ", ".join(sorted(set(parsed) ^ set(out))))
    for uid, body in out.items():
        if tomllib.loads(body) != parsed[uid]:
            raise ValueError(f"unit {uid}: split text does not round-trip")
    return out


# ---------------------------------------------------------------- writing

def toml_value(v):
    """Canonical one-line TOML for the value types unit files use."""
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, int):
        return str(v)
    if isinstance(v, str):
        return json.dumps(v, ensure_ascii=False).replace("\x7f", "\\u007f")
    if isinstance(v, list):
        return "[" + ", ".join(toml_value(x) for x in v) + "]"
    raise TypeError(f"unsupported value {v!r}")


def dump_unit(u):
    """Canonical text of a unit file (FIELDS order, then any extra keys)."""
    keys = [k for k in FIELDS if k in u] + [k for k in u if k not in FIELDS]
    return "".join(f"{k} = {toml_value(u[k])}\n" for k in keys)


def set_field(text, key, value):
    """`text` with `key = value` replaced in place (or appended after the last key line),
    leaving every other line, including comments, untouched."""
    line = f"{key} = {toml_value(value)}"
    pat = re.compile(rf"(?m)^{re.escape(key)}[ \t]*=.*$")
    if pat.search(text):
        return pat.sub(lambda _: line, text, count=1)
    lines = text.splitlines()
    last = max((i for i, l in enumerate(lines) if re.match(r"^[A-Za-z0-9_\-]+[ \t]*=", l)), default=-1)
    lines.insert(last + 1, line)
    return "\n".join(lines) + "\n"


def del_field(text, key):
    return re.sub(rf"(?m)^{re.escape(key)}[ \t]*=.*\n?", "", text, count=1)


def unit_path(uid, root=None):
    if not _ID_RE.match(uid):
        raise ValueError(f"bad unit id {uid!r}")
    return units_dir(root) / f"{uid}.toml"


def write_unit(u, root=None):
    """Writes a unit; an existing file keeps its layout and comments, only changed
    fields are rewritten."""
    p = unit_path(u["id"], root)
    if p.exists():
        text = p.read_text()
        old = parse_unit(text, str(p))
        for k in old:
            if k not in u:
                text = del_field(text, k)
        for k in [k for k in FIELDS if k in u] + [k for k in u if k not in FIELDS]:
            if old.get(k) != u[k]:
                text = set_field(text, k, u[k])
    else:
        p.parent.mkdir(parents=True, exist_ok=True)
        text = dump_unit(u)
    if parse_unit(text, str(p)) != u:
        raise ValueError(f"{p}: rewrite does not round-trip")
    p.write_text(text)
    return p


# ---------------------------------------------------------------- branch migration

def migrate(base_text, tip_text, root=None):
    """Applies a branch's `plan/modules.toml` edits (`tip_text` vs its merge base
    `base_text`) to the per-unit files under `root`, field by field (3-way: a field the
    branch did not change keeps main's value). Returns (changed ids, conflicts); a
    conflict (main and the branch both changed a field differently) takes the branch's
    value and is reported."""
    base = {u["id"]: u for u in parse_modules_toml(base_text)} if base_text.strip() else {}
    tip = {u["id"]: u for u in parse_modules_toml(tip_text)}
    changed, conflicts = [], []
    for uid in sorted(set(base) | set(tip)):
        b, t = base.get(uid), tip.get(uid)
        if b == t:
            continue
        p = unit_path(uid, root)
        if t is None:  # the branch removed the unit
            if p.exists():
                p.unlink()
            changed.append(uid)
            continue
        cur = parse_unit(p.read_text(), str(p)) if p.exists() else None
        if cur is None:
            if b is not None:
                conflicts.append(f"{uid}: removed on main but edited on the branch (restored)")
            write_unit(t, root)
            changed.append(uid)
            continue
        new = dict(cur)
        for k in [k for k in FIELDS if k in t or (b and k in b)] + \
                 [k for k in list(t) + list(b or {}) if k not in FIELDS]:
            bv, tv = (b or {}).get(k), t.get(k)
            if bv == tv:
                continue
            if cur.get(k) not in (bv, tv):
                conflicts.append(f"{uid}.{k}: changed on main and on the branch (kept the branch's value)")
            if tv is None:
                new.pop(k, None)
            else:
                new[k] = tv
        if new != cur:
            write_unit(new, root)
        changed.append(uid)
    return changed, conflicts


def main(argv):
    if argv[:1] == ["check"]:
        us = load_units()
        bad = [f"{u['id']}: status {u.get('status')!r}" for u in us if u.get("status", "todo") not in STATUSES]
        for b in bad:
            print(b, file=sys.stderr)
        print(f"{len(us)} units OK" if not bad else f"{len(bad)} bad unit(s)")
        return 1 if bad else 0
    if argv[:1] == ["migrate"] and len(argv) >= 3:
        root = argv[argv.index("--root") + 1] if "--root" in argv else None
        base_text = pathlib.Path(argv[1]).read_text()
        tip_text = pathlib.Path(argv[2]).read_text()
        changed, conflicts = migrate(base_text, tip_text, root)
        for uid in changed:
            print(f"plan/units/{uid}.toml")
        for c in conflicts:
            print(f"CONFLICT {c}", file=sys.stderr)
        return 3 if conflicts else 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.dont_write_bytecode = True
    sys.exit(main(sys.argv[1:]))

"""Tiny preprocessor-condition tracker for text scans (hazard grep, written-static detection).

`inactive_lines(path)` returns the 1-based line numbers that the reference build compiles out,
evaluating #if/#ifdef/#ifndef/#elif/#else against the macro set that `g++ -dM -E` reports for an
SMLib TU with the reference flags. Conditions it cannot evaluate are treated as active (conservative
for hazard reporting).
"""
import functools
import os
import re
import subprocess

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
UP = os.path.join(ROOT, "upstream")
FLAGS = ["-DNDEBUG", "-DNOMINMAX", "-DSM_GRAPHICS_CALLBACKS=1", "-D_GLIBCXX_USE_CXX11_ABI=1", "-std=c++17"]


@functools.lru_cache(maxsize=1)
def macros():
    tu = os.path.join(UP, "source", "SMLib", "src", "SmBrep.cpp")
    out = subprocess.run(["g++"] + FLAGS + ["-I", os.path.join(UP, "source", "SMLib", "inc"), "-dM", "-E", tu],
                         capture_output=True, text=True, check=False).stdout
    m = {}
    for ln in out.splitlines():
        parts = ln.split(None, 2)
        if len(parts) >= 2 and parts[0] == "#define":
            name = parts[1].split("(")[0]
            m[name] = parts[2].strip() if len(parts) > 2 else ""
    return m


def _eval(expr, m):
    """Return True/False/None(unknown)."""
    expr = expr.split("//")[0].split("/*")[0].strip()
    expr = re.sub(r"defined\s*\(\s*(\w+)\s*\)", lambda mo: "1" if mo.group(1) in m else "0", expr)
    expr = re.sub(r"defined\s+(\w+)", lambda mo: "1" if mo.group(1) in m else "0", expr)

    def ident(mo):
        w = mo.group(0)
        if w.isdigit():
            return w
        if w in m:
            v = m[w]
            return v if re.fullmatch(r"-?\d+", v or "") else ("1" if v == "" else "@")
        return "0"  # undefined identifiers evaluate to 0 in #if
    py = re.sub(r"\b[A-Za-z_]\w*\b|\b\d+\b", ident, expr)
    if "@" in py or re.search(r"[^\d\s()!&|<>=+\-*/]", py):
        return None
    py = py.replace("&&", " and ").replace("||", " or ")
    py = re.sub(r"!(?!=)", " not ", py)
    try:
        return bool(eval(py, {"__builtins__": {}}, {}))  # noqa: S307 - digits/operators only
    except Exception:
        return None


@functools.lru_cache(maxsize=None)
def inactive_lines(relpath):
    m = macros()
    with open(os.path.join(UP, relpath), errors="replace") as fh:
        lines = fh.read().split("\n")
    stack = []  # (parent_active, this_branch_active, any_branch_taken)
    active = True
    dead = set()
    for i, ln in enumerate(lines, 1):
        s = ln.strip()
        mo = re.match(r"#\s*(ifdef|ifndef|if|elif|else|endif)\b(.*)", s)
        if mo:
            kw, rest = mo.group(1), mo.group(2).strip()
            if kw in ("ifdef", "ifndef", "if"):
                if kw == "ifdef":
                    c = rest.split()[0] in m if rest else None
                elif kw == "ifndef":
                    c = (rest.split()[0] not in m) if rest else None
                else:
                    c = _eval(rest, m)
                br = True if c is None else c
                stack.append((active, br, br or c is None))
                active = active and br
            elif kw == "elif" and stack:
                pa, _, taken = stack[-1]
                c = _eval(rest, m)
                br = (not taken) and (True if c is None else c)
                stack[-1] = (pa, br, taken or br)
                active = pa and br
            elif kw == "else" and stack:
                pa, _, taken = stack[-1]
                br = not taken
                stack[-1] = (pa, br, True)
                active = pa and br
            elif kw == "endif" and stack:
                pa, _, _ = stack.pop()
                active = pa
            continue
        if not active:
            dead.add(i)
    return frozenset(dead)

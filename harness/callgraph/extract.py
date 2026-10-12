#!/usr/bin/env python3
"""Extract a function-level call graph from the upstream SMLib/SM_API C++ sources.

Uses libclang (Python bindings, `pip install libclang`) against bin/cpp/compile_commands.json.
For every translation unit it records:
  * function/method definitions located in upstream/ (USR, qualified name, file, line span,
    virtual flag, overridden methods, owning class),
  * call edges from each definition body (CALL_EXPR, references to functions/methods as
    values, `new` -> constructor, `delete` -> destructor),
  * class records (bases) for conservative virtual dispatch,
  * namespace-scope / static-member / function-static variables (global-state hazards).

Output: one JSON per TU under bin/callgraph/tu/, merged later by build_graph.py.

Usage: extract.py [--jobs N] [--filter SUBSTR]
"""
import argparse
import re
import json
import os
import shlex
import sys
import time
from multiprocessing import Pool

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
UPSTREAM = os.path.join(ROOT, "upstream")
OUT = os.path.join(ROOT, "bin", "callgraph", "tu")
CCDB = os.path.join(ROOT, "bin", "cpp", "compile_commands.json")
LIBCLANG = os.environ.get("LIBCLANG", "/home/linuxbrew/.linuxbrew/opt/llvm/lib/libclang.so")
RESOURCE_DIR = os.environ.get("CLANG_RESOURCE_DIR", "/home/linuxbrew/.linuxbrew/Cellar/llvm/21.1.8/lib/clang/21")

# libstdc++ include dirs matching the g++ used by harness/cpp/build.sh (from `g++ -E -v`).
SYS_INCLUDES = os.environ.get("CXX_SYS_INCLUDES", "/usr/include/c++/13:/usr/include/x86_64-linux-gnu/c++/13:"
                              "/usr/include/c++/13/backward").split(":")

# Only these upstream trees are licence-cleared for analysis.
ALLOWED = [
    os.path.join(UPSTREAM, "source", "SMLib") + "/",
    os.path.join(UPSTREAM, "source", "SM_API") + "/",
    os.path.join(UPSTREAM, "tests", "SM_API_test") + "/",
    os.path.join(UPSTREAM, "tests", "prog_test") + "/",
    os.path.join(ROOT, "harness") + "/",
]


def allowed(path):
    return path is not None and any(path.startswith(p) for p in ALLOWED)


def rel(path):
    if path.startswith(UPSTREAM + "/"):
        return path[len(UPSTREAM) + 1:]
    if path.startswith(ROOT + "/"):
        return path[len(ROOT) + 1:]
    return path


def args_for(entry):
    toks = shlex.split(entry["command"]) if "command" in entry else list(entry["arguments"])
    out = []
    skip = False
    for t in toks[1:]:
        if skip:
            skip = False
            continue
        if t in ("-o", "-c"):
            skip = t == "-o"
            continue
        if t == entry["file"] or t.endswith(".cpp"):
            continue
        if t.startswith("-O") or t == "-g" or t.startswith("-W") or t == "-w" or t.startswith("-f"):
            continue
        out.append(t)
    out += ["-resource-dir", RESOURCE_DIR, "-w", "-x", "c++", "-nostdinc++"]
    for d in SYS_INCLUDES:
        out += ["-isystem", d]
    return out


def work(entry):
    import clang.cindex as ci

    if not ci.Config.loaded:
        ci.Config.set_library_file(LIBCLANG)
    CK = ci.CursorKind
    FUNC_KINDS = {CK.FUNCTION_DECL, CK.CXX_METHOD, CK.CONSTRUCTOR, CK.DESTRUCTOR,
                  CK.FUNCTION_TEMPLATE, CK.CONVERSION_FUNCTION}
    CLASS_KINDS = {CK.CLASS_DECL, CK.STRUCT_DECL, CK.CLASS_TEMPLATE, CK.UNION_DECL,
                   CK.CLASS_TEMPLATE_PARTIAL_SPECIALIZATION}

    src = entry["file"]
    name = rel(src).replace("/", "__")
    outp = os.path.join(OUT, name + ".json")
    t0 = time.time()
    idx = ci.Index.create()
    try:
        tu = idx.parse(src, args=args_for(entry),
                       options=ci.TranslationUnit.PARSE_SKIP_FUNCTION_BODIES * 0)
    except ci.TranslationUnitLoadError as e:
        return (src, "LOADERR " + str(e))
    diags = [str(d) for d in tu.diagnostics if d.severity >= ci.Diagnostic.Error][:20]

    funcs = {}
    edges = {}
    classes = {}
    globals_ = []
    names = {}  # callee USR -> qualified name (for template-instantiation fallback)
    seen_hdr = set()
    stats = {"dead_if": 0}

    def fpath(c):
        f = c.location.file
        return f.name if f else None

    def qual(c):
        parts = []
        p = c
        while p is not None and p.kind != CK.TRANSLATION_UNIT:
            if p.spelling:
                parts.append(p.spelling)
            p = p.semantic_parent
        return "::".join(reversed(parts))

    def dtor_usr_of_type(ty):
        t = ty.get_canonical()
        if t.kind == ci.TypeKind.POINTER:
            t = t.get_pointee().get_canonical()
        decl = t.get_declaration()
        if decl is None or decl.kind not in CLASS_KINDS:
            return None
        for ch in decl.get_children():
            if ch.kind == CK.DESTRUCTOR:
                return ch.get_usr()
        return "DTOR:" + decl.get_usr()

    FALSE_LITS = {"FALSE", "false", "0", "SM_FALSE", "NL_NO"}
    file_tokens_cache = {}
    false_cache = {}

    def is_assigned_elsewhere(var, scope_cursor):
        """True if `var` is assigned / address-taken / incremented anywhere in scope tokens."""
        nm = var.spelling
        decl_line, decl_col = var.location.line, var.location.column
        toks = [t for t in scope_cursor.get_tokens()]
        for i, t in enumerate(toks):
            if t.spelling != nm:
                continue
            if t.location.line == decl_line and t.location.column == decl_col:
                continue
            nxt = toks[i + 1].spelling if i + 1 < len(toks) else ""
            prv = toks[i - 1].spelling if i > 0 else ""
            if nxt == "=" and i + 2 < len(toks) and toks[i + 2].spelling in FALSE_LITS and \
                    (i + 3 >= len(toks) or toks[i + 3].spelling in (";", ",", ")")):
                continue
            if nxt in ("=", "|=", "&=", "^=", "+=", "-=", "++", "--") or prv in ("&", "++", "--"):
                return True
        return False

    def const_false_var(var, func_cursor):
        """Local or file-static flag initialised to a false literal and never written again."""
        key = var.get_usr() or (var.spelling, var.location.line)
        if key in false_cache:
            return false_cache[key]
        res = False
        try:
            toks = [t.spelling for t in var.get_tokens()]
            if "=" in toks:
                init = toks[toks.index("=") + 1:]
                init = [x for x in init if x not in (";", "(", ")")]
                if len(init) == 1 and init[0] in FALSE_LITS:
                    sp = var.semantic_parent
                    if sp is not None and sp.kind in FUNC_KINDS:
                        res = not is_assigned_elsewhere(var, func_cursor)
                    elif sp is not None and sp.kind == CK.TRANSLATION_UNIT and fpath(var) == src:
                        res = not is_assigned_elsewhere(var, tu.cursor)
        except Exception:
            res = False
        false_cache[key] = res
        return res

    DEBUG_NAME = re.compile(r"^(m_|s_)?(s?b|i|l|si|sl)?Debug[A-Za-z0-9_]*$")
    DEBUG_CALLS = {"DebugLevel", "GetDebugLevel"}
    CMP = {">", "<", ">=", "<=", "==", "!="}
    ZERO, FALSE_, TRUE_, UNK = "ZERO", "F", "T", "U"

    def dead_condition(cond, func_cursor):
        """True when an if-condition is provably false in a release build: built only from
        debug flags (bDebugMe-style names, DebugLevel()) and never-written FALSE-initialised
        locals, combined with !, &&, ||, comparisons against non-negative literals."""
        toks = [t.spelling for t in cond.get_tokens()]
        if not toks:
            return False
        var_refs = {}
        stack = [cond]
        while stack:
            x = stack.pop()
            if x.kind == CK.DECL_REF_EXPR and x.referenced is not None and x.referenced.kind == CK.VAR_DECL:
                var_refs.setdefault(x.spelling, x.referenced)
            stack.extend(x.get_children())
        pos = [0]

        def peek():
            return toks[pos[0]] if pos[0] < len(toks) else None

        def take():
            t = peek()
            pos[0] += 1
            return t

        def atom():
            t = peek()
            if t == "(":
                take()
                v = p_or()
                if peek() == ")":
                    take()
                return v
            if t == "!":
                take()
                v = p_cmp()
                return TRUE_ if v in (ZERO, FALSE_) else UNK
            parts, last_call = [], None
            while peek() is not None and peek() not in CMP and peek() not in ("&&", "||", ")", "?", ":"):
                x = take()
                if x == "(":
                    depth = 1
                    last_call = parts[-1] if parts else None
                    while peek() is not None and depth:
                        y = take()
                        depth += (y == "(") - (y == ")")
                    parts.append("()")
                    continue
                parts.append(x)
            if not parts:
                return UNK
            if len(parts) == 1:
                nm = parts[0]
                if nm in FALSE_LITS:
                    return ZERO
                if DEBUG_NAME.match(nm):
                    return ZERO
                r = var_refs.get(nm)
                if r is not None and const_false_var(r, func_cursor):
                    return ZERO
                if nm.isdigit():
                    return ("NUM", int(nm))
                return UNK
            if parts[-1] == "()" and last_call in DEBUG_CALLS:
                return ZERO
            if parts[-1] != "()" and DEBUG_NAME.match(parts[-1]) and parts[-2] in ("->", "."):
                return ZERO
            return UNK

        def p_cmp():
            left = atom()
            while peek() in CMP:
                op = take()
                right = atom()
                if left == ZERO and isinstance(right, tuple):
                    n = right[1]
                    if (op in (">", "!=") and n >= 0) or (op == ">=" and n > 0) or (op == "==" and n != 0):
                        left = FALSE_
                        continue
                if left == ZERO and right == ZERO and op in (">", "!=", "<"):
                    left = FALSE_
                    continue
                left = UNK
            return left

        def p_and():
            v = p_cmp()
            while peek() == "&&":
                take()
                w = p_cmp()
                v = FALSE_ if (v in (ZERO, FALSE_) or w in (ZERO, FALSE_)) else UNK
            return v

        def p_or():
            v = p_and()
            while peek() == "||":
                take()
                w = p_and()
                v = FALSE_ if (v in (ZERO, FALSE_) and w in (ZERO, FALSE_)) else UNK
            return v

        try:
            v = p_or()
        except Exception:
            return False
        return v in (ZERO, FALSE_) and pos[0] >= len(toks)

    def is_qualified_member(mref):
        """`Base::Method(...)` style call (non-virtual dispatch)."""
        try:
            toks = [t.spelling for t in mref.get_tokens()]
        except Exception:
            return False
        nm = mref.spelling
        for i, t in enumerate(toks):
            if t == nm and i > 0 and toks[i - 1] == "::":
                return True
        return False

    def note_callee(ref, acc, prefix=""):
        u = ref.canonical.get_usr()
        if not u:
            return
        tmpl = ref.specialized_template if hasattr(ref, "specialized_template") else None
        if tmpl is not None and tmpl.get_usr():
            u = tmpl.canonical.get_usr()
        acc.add(prefix + u)
        if u not in names:
            names[u] = qual(ref)

    def walk_body(c, caller, acc, func_cursor):
        for ch in c.get_children():
            k = ch.kind
            if k == CK.IF_STMT:
                kids = list(ch.get_children())
                if len(kids) >= 2 and dead_condition(kids[0], func_cursor):
                    stats["dead_if"] += 1
                    # walk condition and else-branch only
                    walk_body_one(kids[0], caller, acc, func_cursor)
                    for e in kids[2:]:
                        walk_body_one(e, caller, acc, func_cursor)
                    continue
            if k == CK.MEMBER_REF_EXPR:
                ref = ch.referenced
                if ref is not None and ref.kind in FUNC_KINDS:
                    virt = False
                    try:
                        virt = ref.is_virtual_method()
                    except Exception:
                        pass
                    if virt and is_qualified_member(ch):
                        note_callee(ref, acc, "NV:")
                    else:
                        note_callee(ref, acc)
            elif k in (CK.CALL_EXPR, CK.DECL_REF_EXPR, CK.CXX_NEW_EXPR, CK.OVERLOADED_DECL_REF):
                ref = ch.referenced
                if ref is not None and ref.kind in FUNC_KINDS:
                    has_member = any(x.kind == CK.MEMBER_REF_EXPR and x.referenced == ref
                                     for x in ch.get_children()) if k == CK.CALL_EXPR else False
                    if not has_member:
                        q = False
                        if k == CK.DECL_REF_EXPR:
                            try:
                                q = ref.is_virtual_method() and is_qualified_member(ch)
                            except Exception:
                                q = False
                        note_callee(ref, acc, "NV:" if q else "")
                if k == CK.OVERLOADED_DECL_REF:
                    for i in range(ch.get_num_overloaded_decls() if hasattr(ch, "get_num_overloaded_decls") else 0):
                        d = ch.get_overloaded_decl(i)
                        if d.kind in FUNC_KINDS and d.get_usr():
                            acc.add(d.canonical.get_usr())
            elif k == CK.CXX_DELETE_EXPR:
                for sub in ch.get_children():
                    try:
                        u = dtor_usr_of_type(sub.type)
                    except Exception:
                        u = None
                    if u:
                        acc.add(u)
                    break
            elif k == CK.VAR_DECL and ch.storage_class == ci.StorageClass.STATIC:
                globals_.append({"kind": "static_local", "name": ch.spelling, "in": caller,
                                 "file": rel(fpath(ch) or ""), "line": ch.location.line,
                                 "const": ch.type.is_const_qualified(), "type": ch.type.spelling})
            walk_body(ch, caller, acc, func_cursor)

    def walk_body_one(node, caller, acc, func_cursor):
        """Walk `node` itself plus its subtree."""
        class _Wrap:
            def __init__(self, n):
                self.n = n

            def get_children(self):
                return [self.n]
        walk_body(_Wrap(node), caller, acc, func_cursor)

    def record_func(c):
        usr = c.canonical.get_usr() or c.get_usr()
        if not usr:
            return
        p = fpath(c)
        if not c.is_definition():
            return
        if p != src:
            key = (usr, p, c.extent.start.line)
            if key in seen_hdr:
                return
            seen_hdr.add(key)
        sem = c.semantic_parent
        cls = sem.get_usr() if sem is not None and sem.kind in CLASS_KINDS else ""
        ov = []
        try:
            ov = [o.canonical.get_usr() for o in c.get_overridden_cursors()] if c.kind == CK.CXX_METHOD or c.kind == CK.DESTRUCTOR else []
        except Exception:
            ov = []
        virt = False
        try:
            virt = c.is_virtual_method() if c.kind in (CK.CXX_METHOD, CK.DESTRUCTOR) else False
        except Exception:
            virt = False
        funcs[usr] = {
            "name": qual(c),
            "file": rel(p),
            "line": c.extent.start.line,
            "end": c.extent.end.line,
            "kind": str(c.kind).split(".")[-1],
            "class": cls,
            "virtual": bool(virt or ov),
            "overrides": ov,
            "static": c.storage_class == ci.StorageClass.STATIC if c.kind == CK.FUNCTION_DECL else False,
        }
        acc = set()
        walk_body(c, usr, acc, c)
        if c.kind == CK.CONSTRUCTOR and sem is not None:
            # Objects constructed are destroyed: tie ctor -> dtor conservatively.
            for ch in sem.get_children():
                if ch.kind == CK.DESTRUCTOR:
                    acc.add(ch.canonical.get_usr())
            acc.add("DTOR:" + sem.get_usr())
        edges.setdefault(usr, set()).update(acc)

    def record_class(c):
        usr = c.get_usr()
        if not usr or not c.is_definition():
            return
        if usr in classes:
            return
        bases = []
        dtor = None
        for ch in c.get_children():
            if ch.kind == CK.CXX_BASE_SPECIFIER:
                d = ch.type.get_canonical().get_declaration()
                if d is not None and d.get_usr():
                    bases.append(d.get_usr())
            elif ch.kind == CK.DESTRUCTOR:
                dtor = ch.canonical.get_usr()
        classes[usr] = {"name": qual(c), "file": rel(fpath(c) or ""), "line": c.location.line,
                        "bases": bases, "dtor": dtor}

    def visit(c):
        for ch in c.get_children():
            p = fpath(ch)
            if not allowed(p):
                continue
            k = ch.kind
            if k in FUNC_KINDS:
                record_func(ch)
            elif k in CLASS_KINDS:
                record_class(ch)
                visit(ch)
            elif k in (CK.NAMESPACE, CK.LINKAGE_SPEC, CK.UNEXPOSED_DECL):
                visit(ch)
            elif k == CK.VAR_DECL:
                if ch.is_definition() if hasattr(ch, "is_definition") else True:
                    sp = ch.semantic_parent
                    globals_.append({"kind": "static_member" if sp is not None and sp.kind in CLASS_KINDS else "namespace_var",
                                     "name": qual(ch), "in": "", "file": rel(p), "line": ch.location.line,
                                     "const": ch.type.is_const_qualified(), "type": ch.type.spelling})

    visit(tu.cursor)
    os.makedirs(OUT, exist_ok=True)
    with open(outp, "w") as f:
        json.dump({"tu": rel(src), "diags": diags, "funcs": funcs,
                   "edges": {k: sorted(v) for k, v in edges.items()},
                   "classes": classes, "globals": globals_, "names": names,
                   "secs": round(time.time() - t0, 1), "stats": stats}, f)
    return (src, "ok %d funcs %.1fs %d errs %d dead-ifs" % (len(funcs), time.time() - t0, len(diags), stats["dead_if"]))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--filter", default="")
    a = ap.parse_args()
    db = json.load(open(CCDB))
    db = [e for e in db if allowed(e["file"]) and a.filter in e["file"]]
    db.sort(key=lambda e: -os.path.getsize(e["file"]))
    with Pool(a.jobs) as p:
        for src, msg in p.imap_unordered(work, db):
            print(rel(src), msg, flush=True)


if __name__ == "__main__":
    main()

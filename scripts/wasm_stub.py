#!/usr/bin/env python3
"""Writes the C++ stub module for a wasm32 `difftest` (see scripts/wasm-digest.sh).

    python3 scripts/wasm_stub.py <difftest.wasm> <out.wat>

On wasm32 the `solidcore-ffi` shims are not linked: every `sc_*` function stays an
import from module `env`. This reads those imports (with their exact signatures) and
writes a WAT module, preloaded as `env`, in which each one counts the call and returns
`SC_ERR_UNLINKED` (-1009; nothing for a void shim). `sc_wasm_unlinked_calls()` returns
the count, so `difftest --digest-dir` marks a case whose Rust side reached C++ as
`needs-cpp` instead of digesting a stub result. Standard library only.
"""

import sys

SC_ERR_UNLINKED = -1009
COUNTER = "sc_wasm_unlinked_calls"
VALTYPES = {0x7F: "i32", 0x7E: "i64", 0x7D: "f32", 0x7C: "f64"}


def leb(buf, pos):
    """Unsigned LEB128 at `pos`: (value, next position)."""
    val = shift = 0
    while True:
        b = buf[pos]
        pos += 1
        val |= (b & 0x7F) << shift
        shift += 7
        if b < 0x80:
            return val, pos


def name(buf, pos):
    n, pos = leb(buf, pos)
    return buf[pos : pos + n].decode("utf-8"), pos + n


def env_imports(buf):
    """[(field, params, results)] of the function imports from module `env`."""
    if buf[:8] != b"\0asm\1\0\0\0":
        sys.exit("wasm_stub: not a wasm module (version 1)")
    types, imports = [], []
    pos = 8
    while pos < len(buf):
        sid = buf[pos]
        size, pos = leb(buf, pos + 1)
        end = pos + size
        if sid == 1:  # type section
            count, p = leb(buf, pos)
            for _ in range(count):
                if buf[p] != 0x60:
                    sys.exit("wasm_stub: unsupported type form")
                sig = []
                p += 1
                for _ in range(2):
                    n, p = leb(buf, p)
                    sig.append([VALTYPES.get(t) for t in buf[p : p + n]])
                    p += n
                types.append(sig)
        elif sid == 2:  # import section
            count, p = leb(buf, pos)
            for _ in range(count):
                module, p = name(buf, p)
                field, p = name(buf, p)
                kind = buf[p]
                p += 1
                if kind == 0:
                    idx, p = leb(buf, p)
                    if module == "env":
                        imports.append((field, *types[idx]))
                elif kind == 1:  # table: reftype + limits
                    p += 1
                    flags, p = leb(buf, p)
                    _, p = leb(buf, p)
                    if flags & 1:
                        _, p = leb(buf, p)
                elif kind == 2:  # memory: limits
                    flags, p = leb(buf, p)
                    _, p = leb(buf, p)
                    if flags & 1:
                        _, p = leb(buf, p)
                elif kind == 3:  # global: valtype + mutability
                    p += 2
                else:
                    sys.exit(f"wasm_stub: unsupported import kind {kind}")
            break
        pos = end
    return imports


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    with open(sys.argv[1], "rb") as f:
        imports = env_imports(f.read())
    out = [
        "(module",
        "  (global $n (mut i64) (i64.const 0))",
        f'  (func (export "{COUNTER}") (result i64) global.get $n)',
    ]
    for field, params, results in imports:
        if field == COUNTER:
            continue
        if not field.startswith("sc_"):
            sys.exit(f"wasm_stub: unexpected env import `{field}` (not a shim)")
        if None in params or None in results or len(results) > 1:
            sys.exit(f"wasm_stub: unsupported signature of `{field}`")
        sig = "".join(f" (param {t})" for t in params)
        sig += "".join(f" (result {t})" for t in results)
        ret = f" {results[0]}.const {SC_ERR_UNLINKED}" if results else ""
        out.append(
            f'  (func (export "{field}"){sig}'
            f" global.get $n i64.const 1 i64.add global.set $n{ret})"
        )
    out.append(")")
    with open(sys.argv[2], "w") as f:
        f.write("\n".join(out) + "\n")
    print(f"wasm_stub: {len(imports)} env imports -> {sys.argv[2]}")


if __name__ == "__main__":
    main()

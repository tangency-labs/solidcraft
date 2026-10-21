# harness/callgraph

Function-level call graph of upstream SMLib + SM_API and the port plan derived from it.
Owner: agent B. Only licence-cleared trees are parsed (`source/SMLib`, `source/SM_API`,
`tests/SM_API_test`, `tests/prog_test`).

```
harness/callgraph/run.sh          # full regeneration (about 3-4 min on 6 cores)
python3 harness/callgraph/why.py 'SmFilletIntersector::FlushCurve'   # shortest call path from an entry point
python3 harness/callgraph/why.py SmPoly.cpp --group boolean           # restrict to one entry-point group
```

| step | script | output (bin/callgraph/ unless noted) |
|---|---|---|
| 1 | `extract.py` | `tu/*.json`: per-TU definitions, call edges, classes, globals (libclang over `bin/cpp/compile_commands.json`) |
| 2 | `build_graph.py` | `graph.json`, `closure.json` (CHA + RTA closures from `entrypoints.py` roots), `files.json`, `resolve.json`, `overriders.json` |
| 3 | `plan_units.py` | `plan/units/<id>.toml`, `plan/order.md`, `harness/callgraph/units.json`, `plan_summary.json` |
| 4 | `write_closure.py` | `plan/closure.md`, `harness/callgraph/unused_functions.txt` |

Other files: `entrypoints.py` (in-scope SM_API entry points and kernel roots), `pp.py` (`#if`
evaluator for text scans), `scc_probe.py` (function-level SCC statistics), `why.py`.

`units.json` maps every unit id in `plan/units/` to the exact upstream functions (qualified
name, file, line, line count, clang USR) it owns. A unit that names a file in `upstream` owns only
the listed functions of that file; other functions of the same file belong to other units or are
out of scope (`unused_functions.txt`).

Tooling: libclang 21 from Homebrew LLVM (`LIBCLANG`, `CLANG_RESOURCE_DIR` override the paths) with
the `libclang` pip bindings in `bin/callgraph-venv`; libstdc++ 13 headers (`CXX_SYS_INCLUDES`).

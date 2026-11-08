# Plan unit file schema (shared contract)

The port plan is one TOML file per unit, `plan/units/<id>.toml` (until 2026-10 it was the
single file `plan/modules.toml` with one `[[unit]]` table per unit; the fields are the
same, now as top-level keys). The file name must equal `id`. A PR edits only the files
of the units it changes, so parallel PRs do not conflict.

```toml
id = "nlib.knots"                 # dotted, stable, unique; = the file name
upstream = ["source/SMLib/src/..."]  # upstream file(s) relative to upstream/
rust_module = "nurbs::knots"      # target path inside crates/solidcore
depends_on = []                   # unit ids that must be green/partial before claiming
soft_depends_on = []              # light deps (<=2 calls, no type needed): stub as back-edges, never block
est_lines = 400                   # upstream lines in scope
layer = 0                         # topological layer from plan/order.md
tests = []                        # upstream tests/fixtures exercising it
status = "todo"                   # todo | claimed | partial | green | blocked | out_of_scope
claimed_by = ""
claimed_at = ""
notes = ""
```

Comment lines (`# ...`) after the fields are allowed and kept by the tools.

Readers and writers all go through one loader: `harness/plan/units.py` (Python:
`load_units`, `write_unit`; `python3 harness/plan/units.py check` validates every file)
and `crates/difftest/src/plan.rs` (difftest `--green`). Units are listed in id order.

- `scripts/units-split.py`: one-time split of the old `plan/modules.toml` into
  `plan/units/` (each unit's lines copied verbatim and checked to round-trip).
- `scripts/units-migrate-branch.sh`: for a branch started before the split, moves its
  `plan/modules.toml` edits (relative to its merge base with main) into the unit files;
  run it after merging main (AGENTS.md section 4).

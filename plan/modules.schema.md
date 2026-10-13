# plan/modules.toml schema (shared contract)

```toml
[[unit]]
id = "nlib.knots"                 # dotted, stable, unique
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

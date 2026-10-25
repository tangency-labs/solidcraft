# blend.fillet_solver_p5 — integrating geom p3/p6 (PR #208)

After #247 landed, rollover recovery delegates `TopologySplit` and
`ReCalcFilletGeom` to geom p3 with the caller Kernel, live solver and actual
pseudo-B-rep geometry id. The split uses the upstream default geometry type and
keeps partial edits on failure; registration in the solver list still occurs only
after successful return. Recalculation forwards both the analytic flag and optional
march direction with the upstream default reference curve (`None`).

The geom p2 live-solver adapter also uses p6's newly available side-edge rail index
and rail/edge-use intersection. Indexed and live-solver rail-index calls share one
body; resolution happens only on the upstream fallback for incomplete rails.
Rust regressions check split-time geometry allocation, solver id 37, the default
geometry type and the removed end use before the infinite-region dependency;
recalculation reaches the real missing-edge-curve error. These do not claim native
coverage of completed rollover repair.

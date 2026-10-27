# blend.fillet_geom_p3: ReCalcFilletGeom's CalcCrossSection never needs a neighbouring geometry's solver (2026-10-27)

Unit: blend.fillet_geom_p3 (`SmFilletGeom::ReCalcFilletGeom`). The callee, `SmFilletEdge::CalcCrossSection`, is in blend.fillet_geom_p5.
Site: `crates/solidcore/src/blend/fillet_geom/p3.rs`, the `UNPORTED(blend.fillet_geom_p5)` lookup in `re_calc_fillet_geom`.
PR: #1288.

## The stub

`ReCalcFilletGeom` ends by calling `pE->CalcCrossSection(crContext)` with no corner for every `SM_FE_CROSS_SECTION` edge in its own `m_vEdges` (`upstream/source/SMLib/src/SmFilletGeom.cpp:11774-11785`). `CalcCrossSection` (`SmFilletGeom.cpp:5619`) reads:

- `pFilSolver1 = pFilletGeom1->GetFilletSolver()`, for the primary use's geometry.
- `pFilSolver2 = pFilletGeom2->GetFilletSolver()`, only when the mate use's geometry exists and differs from the primary's (`:5646-5650`). Without a corner, the only thing it reads from that solver is `GetThisApproxTol3d()`.

The Rust port gets only the recalculated geometry's own solver. Any other solver id returns `Unported`.

## Why upstream never reaches it

A fillet geometry's `m_vEdges` is filled in exactly two places in `SmFilletGeom.cpp`:

1. `SmFilletGeom::CreateEndCurve` (`:10162`, `m_vEdges.Add` at `:10318`). The new edge's primary use is never given a fillet geometry, so `CalcCrossSection` takes the `pFilletGeom1 == NULL` branch, which reads no solver.
2. `SmFilletGeom::TopologySplit` (`:12596`, the divide edge at `:12733-12761`). The primary use gets the owning geometry (`:12751`) and the edge goes into that geometry's `m_vEdges` (`:12754`). The mate use is deliberately left without a geometry: `//WARNING: Do NOT add this line: // pMateEU->SetFilletGeom(pFG2);` (`:12758-12760`). Both geometries of a split also belong to the same solver.

The `SetFilletGeom` calls on mate uses in `SmFilletCorner.cpp` (`:4417`, `:4541`, `:5121`, `:5785`, `:6837`, `:8446`) all act on edges that the corner adds to its own `m_vEdges` (for example `:4418`, `:4542`, `:5124`). Those edges are computed through `SmFilletEdge::CalcEdgeGeom`, which dispatches to `CalcCrossSection(crContext, m_cpCorner)` (`SmFilletGeom.cpp:5299`), not through `ReCalcFilletGeom`. Upstream has no `SetFilletSolver`, so a geometry never moves to another solver.

So in `ReCalcFilletGeom`, `pFilSolver2` is either never read or is the recalculated geometry's own solver. The existing lookup already returns that solver. The stub is unreachable by construction upstream.

## Runtime check

Probe on main 84d080fbd: a temporary `eprintln!` at the stub, then the full difftest of each unit below. It recorded 0 hits in all eight:

- blend.fillet_geom_p3
- blend.fillet_executive_p1
- blend.fillet_executive_p3
- blend.fillet_corner_p1
- blend.fillet_corner_p2
- blend.fillet_corner_p3
- blend.fillet_solver_p5
- blend.fillet_standard_solver_p2

The blend.fillet_geom_p3 runner's `ReCalc` models never fill a geometry's edge list, so they do not reach the loop either.

## Consequence

The `UNPORTED` tag stays in place. If it were ever hit, it would surface as `Unported` rather than a guessed result. Wiring it for real would need a neighbour-solver lookup parameter on `re_calc_fillet_geom` that no production caller could fill, and only a synthetic runner model could exercise it.

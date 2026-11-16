# blend.fillet_geom_p3: neighbour-solver CalcCrossSection stub re-checked, still unreachable (2026-10-30)

Updates: plan/findings/blend.fillet_geom_p3--calc-cross-section-neighbour-unreachable.md

This re-check covers the `UNPORTED(blend.fillet_geom_p5)` lookup in `re_calc_fillet_geom`
(`crates/solidcore/src/blend/fillet_geom/p3.rs`) on main d4cb45e1f, after the fillet
fixtures added since the original check (84d080fbd). The tag stays.

## Additional static check: writers outside the corner

The original record covered the `m_vEdges` writers and the corner's `SetFilletGeom`
calls. A neighbour solver is needed only if a use of an edge in some geometry's
`m_vEdges` belongs to a geometry of a *different* solver. Every remaining
`SetFilletGeom` site under `source/SMLib/src` either keeps a single solver or acts on an
edge that is never in a geometry's `m_vEdges`:

- `SmFilletGeom.cpp:10053`, `:12714`: rail edge uses (`MakeRailEdgeuse`, the split rail
  end). Rails are never in `m_vEdges`.
- `SmFilletGeom.cpp:12824` (`TopologySplit`): moves `m_vRefEUs` side uses, which can
  include a divide edge's mate, to `rpNewFilletGeom`. That geometry is created by the
  same solver (`new SmFilletGeom(m_pFilletSolver, ..)`, `:12603`), so the mate's geometry shares
  the recalculated geometry's solver.
- `SmFilletSolver.cpp:6049`, `:6819`: rail uses. `:6860`, `:6884`: `SM_FE_ON_EXTEND_EDGE`
  and `SM_FE_SPLIT_FACE` support edges, which are not cross-section edges and are not
  in `m_vEdges`. `rpSideFG` is created by the same solver (`new SmFilletGeom(this, ..)`,
  `:6813`).
- `SmFilletExecutive.cpp:4286-4292` (`SM_MODIFY_CORNEREDGE_FILLET_RELATION`) acts on a
  corner's `m_vEdges[0]` (`:4450`, `:4463`) and only moves the primary's geometry to the
  mate's or clears it. `:4427-4428` turns a squeezed rail into a cross-section edge and
  clears both uses' geometries. In neither case does a use gain a geometry of another
  solver.

`SmFilletGeom::m_pFilletSolver` is assigned only in the constructor
(`SmFilletGeom.cpp:9018`). Upstream has no `SetFilletSolver`.

## Runtime check

Temporary `eprintln!` probes went at the cross-section loop body, at the neighbour
lookup (the stub) and at the `re_calc_fillet_geom` entry, followed by a serial
`--unit` difftest of each unit below. All passed with the probes in place, and the
probes were then removed.

| unit | loop hits (own solver) | stub hits |
|------|-----------------------:|----------:|
| blend.fillet_geom_p3 | 0 | 0 |
| blend.fillet_executive_p1 | 0 | 0 |
| blend.fillet_executive_p2 | 0 | 0 |
| blend.fillet_executive_p3 | 3 | 0 |
| blend.fillet_corner_p1 | 0 | 0 |
| blend.fillet_corner_p2 | 0 | 0 |
| blend.fillet_corner_p3 | 0 | 0 |
| blend.fillet_solver_p4 | 0 | 0 |
| blend.fillet_solver_p5 | 0 | 0 |
| blend.fillet_standard_solver_p2 | 0 | 0 |
| topology.api_files | 0 | 0 |
| topology.api_projection | 0 | 0 |
| topology.brep_cutting | 0 | 0 |
| topology.brep_planar_section | 0 | 0 |
| primitive.api_trim_ops | 0 | 0 |

The loop is now reached: 3 cross-section edges in blend.fillet_executive_p3, compared
against C++ and passing. Each one resolved to the recalculated geometry's own solver,
which the existing lookup returns. No case asked for another solver.

## Consequence

The conclusion is unchanged. Wiring a real neighbour lookup would mean threading the
executive's solver store through all five `re_calc_fillet_geom` callers, while the
caller's own solver is mutably borrowed, for a branch that no upstream topology can
produce. Only a synthetic model could exercise it, so no difftest case or regression
test can reach it with an upstream-valid input. The `UNPORTED` tag and its reason
comment stay as they are.

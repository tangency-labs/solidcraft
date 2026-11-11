# Fillet session: retained solver reaches geometry-owned offset-surface boundary

The actual constant-radius box-edge setup (`10 x 20 x 30`, first owning edge,
radius `0.5`, circular/G1) now reaches `CalcCornerVertGeom` through the production
session, after ordered parameter setup, corner creation and first ordinary Nx1
topology. The retained concrete solver runs through the live rail provider; it
returns `Unported("SmFilletGeom::GetOffsetSurface", "blend.fillet_geom_p1")` when
native-order `m_pFGForSurfaces` selects the real geometry. `FilletSolver::surface`
and `surface_mut` do not yet resolve that geometry-owned slot.

The normal native full-API box fixture succeeds; a separate direct vertex-stage
comparison is being added. This is a missing live-object context boundary, not
successful end-to-end execution. Resolution must preserve actual geometry-owned
offset surfaces, solver fallback and write-through semantics (`SmFilletSolver.cpp`
130–162; `SmFilletGeom.cpp` 9672–9680). Clearing the selected geometry, copying a
solver base, or swapping unrelated surface state would conceal the missing path.

### Real fillet first-corner vertex calculation reaches offset-surface context (2026-10-24)

The additive `real-box-first-vertex-geometry-actual-session` case runs shared
parameter setup, actual corner creation, direct first Nx1 topology, then that
corner's actual vertex calculation on both sides. It does not claim the complete
executive prelude. Native returns success and processes both fillet vertices at
(9.5, 0, 30) and (10, 0, 29.5), with zero tolerance and original parameters 9.5
and 29.5. Rust reaches genuine `SmFilletGeom::GetOffsetSurface` in
`blend.fillet_geom_p1`; the case remains deferred, with no fabricated result.

The snapshot reads initialized fillet-vertex status/processed fields and samples
geometry only after processing. It never reads the uninitialized native corner
status or unevaluated pseudo-edge geometry. Full p3: 297 cases, 249 value passes,
48 deferred, zero failures. All five earlier real-solid cases retain their full
CPP and Rust output records byte-for-byte; no earlier record fields were removed.

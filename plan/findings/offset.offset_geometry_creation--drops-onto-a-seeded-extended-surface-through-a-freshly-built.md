# offset.offset_geometry_creation: drops onto a seeded extended surface through a freshly built face cache (2026-10-25)

`gen0248`, `gen0346`, `gen0350`, `gen0508` (generated, `offset.offset_geometry_creation`)
were deferred on main where `TrimExtendedEdge`'s `pOtherExtSurf->DropAndTrimCurve`
traced on the surface's cache: the runner seeds every base with itself
(`m_vSurfExtSurf.Insert(s, s)`), so the "extended" surface is the face's own surface
and `smsurf_GetSurfaceCache` returns the face's trimmed cache. The drop now gets that
cache (`OffsetBrep::prepare_surface_solve_cache`, built fresh at the drop) and 33 of
the 45 deferred cases pass; these four disagree in the recorded state:
`values.state.len` C++ 168 vs Rust 188 (`gen0248`), 547 vs 615 (`gen0346`), 14190 vs
14210 (`gen0508`); `gen0350` keeps the length but `values.state[311]` is C++
`2.9631480111470525e-2` vs Rust `4.1052170113668345e-2` (12 values differ, max rel err
0.278). Suspected cause: upstream's face cache is the one the face has held since an
earlier op of the script (its tree and `m_bFaceWasModified` / point-test state carry
over the script's edits), while the port builds a new cache with point testing on at
each drop, so the trace trims the dropped curve differently. The trimmed-cache
history work (`smsurf_GetSurfaceCache` keeping a surface's cache across operations) is
the likely fix; until then the cases are known mismatches.

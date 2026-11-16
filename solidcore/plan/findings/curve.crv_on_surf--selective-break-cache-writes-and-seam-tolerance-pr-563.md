# curve.crv_on_surf: selective break-cache writes and seam tolerance (PR #563)

`SmCrvOnSurf::GetKnots` writes its break cache from const queries. The Rust
`Curve` dispatch previously computed those knots on a copy, and the base
`DropPoint` also ran on a copy. Wiring UV discontinuity classification alone
therefore exposed the 17 drop-state mismatches recorded above. The value-only
break cache now retains shared-query writes, and the base drop transfers its
actual break state back even on error. A successful local solve that never
queries knots leaves the cache untouched. Clones keep independent caches;
`breaks()` returns an owned snapshot so callers cannot hold an internal borrow
across a geometry query. The existing approximation-cache copy semantics of
the `*_ref` approximation/drop wrappers remain outside this change.

Native Linux ARM64 against main `3e811d0c7`: `curve.crv_on_surf` passes all
2107 cases with values, bit-identically, with zero deferred/failed cases. All
1800 baseline passes remain passing and all 307 classification deferrals pass.
Perturbation detects 2107/2107. No case, guard or comparison tolerance changed.

Wiring this exposed 40 `heal.face_props` and two `intersect.curve_class_p4`
seam-property mismatches in CI run 39864112887. For example, `heal.face_props`
gen0023 (seed 16562362309779922966) is a tiny revolution surface with face
tolerance 1.0: C++ reports missing/near-miss seams in both directions (flags
1, direction 3), Rust neither (flags 0, direction 4). The classification itself
returned success but left every interval Unknown. `HasSeamProblem` cloned the
face's surface, dropping its owner before `surface_zone_tol_3d` and selecting
system tolerance instead of the face tolerance. Borrowing the actual surface
matches the upstream call and preserves its exact geometry and ownership.

With that correction the five focused units have zero mismatches:
`heal.face_props` 1430 pass / 98 deferred; `intersect.curve_class_p4` 2914 / 287;
`topology.face_p4` 5258 / 248; `curve.curve_p2` 3029 / 0; `curve.crv_on_surf`
2107 / 0. Against main's 89c94bf4b reports (same kernel as 3e811d0c7), the first
two gain 113 and 149 passes respectively and retain every baseline pass.
The exposed seam mismatches reach genuine existing deeper stubs after the
tolerance correction; they are relocated deferrals, not additional pass gains.

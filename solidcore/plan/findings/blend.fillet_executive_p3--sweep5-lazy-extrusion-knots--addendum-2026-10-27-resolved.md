# Resolved: sweep5 lazy surface-of-extrusion knots (2026-10-27)

Updates: plan/findings/blend.fillet_executive_p3--sweep5-lazy-extrusion-knots.md

`sweep5-letter_g-e1-r1.0` and `sweep5-bottle-e29-r1.0` now pass (topology, geometry and mass
agree). Their known-mismatch entries are removed.

Fix: the three kernel-free knot readers these cases reach now take the kernel and call
`Surface::knots_with_nurb`, as upstream's virtual `GetKnots` makes a lazy surface's NURB
(`GetOrCreateGwNurbPointer`):

- `SurfaceEvaluate::knots` (used by `SmCurve::SurfaceMaxDistanceBetween`'s sample count):
  the `Surface` impl calls `knots_with_nurb`, and the offset evaluator's impl calls it on
  its base or extension (`SmOffsetSurface::GetKnots` forwards to `m_pSurface->GetKnots`).
- `Surface::snap_to_knots` (`SmSurface::SnapToKnots`).
- `Surface::domain_with_continuity_from_cache` / `_using` (`GetDomainWithContinuity`).

In the probe sweep the same fix clears letter_g edges 2, 3, 5, 13, 15, 16 and ext_w_hole
edge 4 (single edge, r=1.0). letter_g e17 and ext_w_hole e14 now complete too, and expose
other mismatches: e17 joins the mate-held UV trim mass group
(`plan/findings/blend.fillet_executive_p3--sweep5-mass-on-mate-held-uv-trims.md`), and e14
has its own finding
(`plan/findings/blend.fillet_executive_p3--sweep5-ext-w-hole-e14-near-zero-vertex.md`).

Other kernel-free knot readers listed in `surface/surface/p2.rs` (`SurfaceCache::surface_continuity`,
the offset and surface-in-volume inherent `knots`, `IsoCurve::knots`) are unchanged. No
sweep case reaches them.

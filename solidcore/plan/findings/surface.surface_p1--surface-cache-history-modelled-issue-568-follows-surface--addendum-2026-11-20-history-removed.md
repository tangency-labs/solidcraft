# surface.surface_p1 / surface.surface_p11: surface cache history no longer modelled (addendum to "surface cache history modelled")

The cache-history model is removed. A surface's continuity arrays are now a pure function of the surface. `GetDomainWithContinuity`, `LiftCurve` and the drop's boundary breaks read them without any cache, so they give the same answer whatever ran before. This deliberately deviates from upstream. Upstream reads its cache's arrays without `GetTree`, and those arrays are empty until something builds the tree.

## Model as built

- `Surface::continuities` (`surface/surface/p6.rs`) computes the U and V arrays once. It keeps them in a memo beside the singularities and boundary memos, keyed like `CurveMemo`.
  - Exact copies (`clone_for_evaluation`, `clone_exact_bounded`) share the memo.
  - `Clone` and every edit start an empty one.
- `BuildTreeBase` takes its arrays from that memo.
- `GetDomainWithContinuity` and `LiftCurve` read `self.continuities(k)`. `DomainWithContinuity` over a held cache and `FindBoundaryStartPoints` read `cache.surface().continuities(k)`. A cache's surface is an exact copy of the surface it was made for, so these are the arrays its tree build would have stored.
- These readers take no solve context and make no cache. So a continuity read never builds a face's cache tree or makes UV trim curves on the body.
  - Upstream's `LiftCurve` made such a cache outside a Boolean operation, through `smsurf_GetSurfaceCache`.
  - The port did so before this change only when the surface's slot said built.
- Removed:
  - the surface's remembered tree state: `CacheTreeState`, its slot field and accessors, `note_surface_cache`, `cache_tree_built_on_get`, the state copied by `copy_cache_slot_of`;
  - `continuity_surface_cache`;
  - `lift_curve_with_context` and `domain_with_continuity_with_context`.

## Failure modes

- If the continuity computation fails, it now fails the reader during a Boolean operation too, as it already did outside one. Before, the empty arrays of a cache without a tree let the reader go on.
- The knot query of a surface kind without one fails the same way. The fork's uninitialised domain when `GetKnots` fails (WR-663) can no longer be reached, because the continuities query the knots first. That branch is removed.

## `surface.surface_p1` `DomCont`

The `history` input runs real calls before the query:
- the surface's cache requested during a Boolean operation, so its tree is deferred and its arrays are empty;
- then `1`: the query during the Boolean operation; `2`: `GetTree`, then the query during the Boolean operation; `3`: the query after it.

Every `domhistNNNN` expectation equals the answer with no history. That equality is the check.

15 outputs changed: 0009, 0011, 0023, 0043, 0049, 0061, 0062, 0063, 0067, 0070, 0071, 0073, 0078, 0079 and 0083.
- **Proof.** The kernel before this change, run with no history, writes a byte-identical expected file. The old outputs came from the empty arrays of a cache without a tree.
- **0070 and 0079.** Both are curve-bounded surfaces, which answer no `GetKnots`: the base-class `SM_ERR`, as upstream's `SmCurveBoundedSurface`, which does not override it.
  - The old value was an inverted box with status 0 (the WR-663 branch, reached only without a tree). The new value is status 1001, which main already gave with no history.
  - Neither is the right answer. The exact answer is the basis surface's continuity domain; 0070's basis has a C0 crease at u = 0.50087.
  - The change removes the history dependence only. Making curve-bounded surfaces answer their basis's knots is a separate change.

## `offset.offset_geometry_creation`: 5 changed cases

The record is compared with main's op by op, field by field:

| Case | Op | Field | Main | New | Class |
|---|---|---|---|---|---|
| gen0266 | ExtendTrimEdges | status | 0 | 1001 | the refusal (below) |
| gen0266 | ExtendTrimEdges | edge curve, interval, tolerance, vertices | the extended curve (22 control points) | the original curve (12 control points) | follows from the refusal |
| gen0266 | ExtendTrimEdges | UV trim curves of the 2 edge uses | present | none | trim curves the lift's cache build made |
| gen0152 | ExtendTrimEdges, MateEdges | UV trim curves of the 4 edge uses | present (9 control points) | none | trim curves the lift's cache build made |
| gen0462 | ExtendTrimEdges | UV trim curves of the 2 edge uses | present | none | trim curves the lift's cache build made |
| gen0462 | ExtendTrimEdges | edge tolerance | 0.993976 | 2e-5 | tolerance raised by those creations |
| gen0678 | ExtendTrimEdges | UV trim curves of the 2 edge uses | present | none | trim curves the lift's cache build made |
| gen0760 | ExtendTrimEdges | UV trim curves of the 2 edge uses | present | none | trim curves the lift's cache build made |
| gen0760 | ExtendTrimEdges | edge tolerance | 1.08423 | 1e-5 | tolerance raised by those creations |

No other op status changes, and no other 3D geometry changes.
- **Check.** Putting back only the old context cache build before the offset lift (a temporary edit) makes the four non-gen0266 cases equal main's records again.
- **gen0266 refusal.**
  - `LiftCurve`'s only segment starts at uv (18.834, 34.638), direction (0.6726, -0.7357), over [-43.708, 49.763]. The natural domain is [-0.05, 1.05]².
  - The line meets u ∈ [-0.05, 1.05] only where v ≈ 55, and v ∈ [-0.05, 1.05] only where u ≈ 49.5. So no point of the segment is on the surface.
  - The old lift's curve was 1.847e10 off the surface against a tolerance of 3.3e-6 (largest distance from S(uv(t)) over 2001 t, measured with a temporary probe). The refusal is correct.
- **Per-op record lengths in gen0266, main vs new.** Header 1; VertexOffset 2; AddCurve 3 and 3; HaveTangentGaps 2; MateEdges 88; ExtendTrimEdges 183 vs 87; UpdateFaceDomain 2. Total 284 vs 188.
- **A note on the harness.** Every record block holds min, max and sum. The control points carry an `f64::MIN` weight sentinel, so the sums are ±inf or NaN, and the golden pins little beyond the counts. This weakness predates the change.

## Not changed

The fast-drop face test's point-test bit still depends on whether a face's cache was built, on faces without loops. See `plan/findings/blend.fillet_executive_p3--rollover-404-face-uv-domain-vmin--addendum-2026-11-20-fast-drop-read-kept.md`.

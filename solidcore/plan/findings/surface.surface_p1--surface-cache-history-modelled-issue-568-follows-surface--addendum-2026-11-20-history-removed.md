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

The exception is a history step that fails, so the query never runs. Its record is the marker `-1` (`DOM_CONT_HISTORY_FAILED`), not a status. Only `GetTree` of a curve-bounded surface fails there, because it needs the knots such a surface does not answer (#20163). That holds for 0052, 0074 and 0075 under history `2`, with status 1001 and the same reason on main. Main's harness wrote 1001 for the failed step, which happened to equal the query's own answer without history. Any other failed history step is a harness error.

15 outputs changed: 0009, 0011, 0023, 0043, 0049, 0061, 0062, 0063, 0067, 0070, 0071, 0073, 0078, 0079 and 0083.
- **Proof.** The kernel before this change, run with no history, writes a byte-identical expected file. The old outputs came from the empty arrays of a cache without a tree.
- **0070 and 0079.** Both are curve-bounded surfaces, which answer no `GetKnots`: the base-class `SM_ERR`, as upstream's `SmCurveBoundedSurface`, which does not override it.
  - Both run history `1`, and the query is reached on main and here (checked with a temporary print).
  - The old value was an inverted box with status 0 (the WR-663 branch, reached only without a tree). The new value is status 1001, which main already gave with no history.
  - Neither is the right answer. The exact answer is the basis surface's continuity domain; 0070's basis has a C0 crease at u = 0.50087.
  - The change removes the history dependence only. Making curve-bounded surfaces answer their basis's knots is a separate change.

## `offset.offset_geometry_creation`: 5 changed cases

The record is compared with main's op by op, field by field:

| Case | Op | Field | Main | New | Class |
|---|---|---|---|---|---|
| gen0266 | ExtendTrimEdges | status | 0 | 1001 | the refusal (below) |
| gen0266 | ExtendTrimEdges | edge curve, interval, tolerance, vertices | the extended curve (22 control points) | the original curve (12 control points) | follows from the refusal |
| gen0266 | ExtendTrimEdges | UV trim curve of one of the edge's 2 uses | present | none | made by the lift's cache build |
| gen0152 | ExtendTrimEdges, MateEdges | UV trim curve of one of the edge's 2 uses (the edge is recorded twice per op) | present (9 control points) | none | made by the lift's cache build |
| gen0462 | ExtendTrimEdges | UV trim curve of one of the edge's 2 uses | present | none | made by the lift's cache build |
| gen0462 | ExtendTrimEdges | edge tolerance | 0.993976 | 2e-5 | raised by that trim's creation |
| gen0678 | ExtendTrimEdges | UV trim curve of one of the edge's 4 uses | present | none | made by the lift's cache build |
| gen0760 | ExtendTrimEdges | UV trim curve of one of the edge's 2 uses | present | none | made by the lift's cache build |
| gen0760 | ExtendTrimEdges | edge tolerance | 1.08423 | 1e-5 | raised by that trim's creation |

No other op status changes, and no other 3D geometry changes.
- **Check.** Putting back only the old context cache build before the offset lift (a temporary edit) makes the four non-gen0266 cases equal main's records again.

### Measured gaps, and when or whether

A temporary probe in the harness measured, for the edge `ExtendTrimEdges` extends, the largest distance from its 3D curve over its interval to the surface of each face it is used on. It ran before and after the op, on main and on this branch.
- **Upper end:** a proved bound, `curve_face_gap_bound_within`, tight to a thousandth.
- **Lower end:** over 401 samples, each sample drops onto the surface, and the largest certified lower bound of the distance is kept (`surface_distance_lower_bound`).
- At the end of the script it listed every edge's tolerance and how many of its uses hold a UV trim.

The "before" values are the same on both kernels.

| Case | Use given a trim by main's lift | Largest gap, curve to surface | Tolerance before the op | After, main | After, new | A later op makes it anyway? |
|---|---|---|---|---|---|---|
| gen0152 | eu0 of b1 e0 | at most 8.9e-16 at the samples (the proved bound, 0.0315, is loose here) | 1e-5 | 1e-5 | 1e-5 | **When:** yes. UpdateFaceDomain, two ops later, makes the trim. The last op, ReplaceEdgeCurves, then drops it on both kernels. At the end every edge's tolerance and trim count equal main's. |
| gen0266 | eu0 of b1 e0 | main, on its extended curve: [0.0130130, 0.0130251]; new, on the original curve: [3.71918e-3, 3.72368e-3] | 4.05e-3 | 8.10e-3 (below its own curve's gap) | 4.05e-3 (covers it) | **Whether:** no. Only UpdateFaceDomain follows, and it makes neither. This case follows from the refusal. |
| gen0462 | eu0 of b1 e0 | [0.496512, 0.496855] | 2e-5 | 0.993976 | 2e-5 | **Whether:** no. Only VertexOffset follows, and it makes neither. |
| gen0678 | eu3 of b1 e0 | at most 4.2e-15 at the samples (bound 1.9e-13) | 1e-5 | 1e-5 | 1e-5 | **Whether:** no. It is the last op. |
| gen0760 | eu1 of b0 e1 | [0.6260133565455, 0.6260133565484] | 1e-5 | 1.084233 | 1e-5 | **Whether:** no. It is the last op. |

- **gen0462 and gen0760: the gap is real.**
  - The script sets the edge's curve (ReplaceEdgeCurves, SetEdgeCurve), and that curve lies 0.4965 and 0.6260 off its face before the op, on both kernels. The tolerance there, 2e-5 and 1e-5, does not cover the gap.
  - On main, the lift built the face's cache tree as a side effect. That build made a UV trim on one use, and making the trim raised the edge's tolerance to cover the gap: 2.002 and 1.732 times it.
  - The new record is therefore not "more correct". It is what the operations do without the raise that a read-only lift used to apply as a side effect. The edge leaves the op with a tolerance that does not cover its gap, as it entered it.
- **gen0152 and gen0678: only the trim's timing changes.** Their curves lie on the surface (within 1e-15 at the samples), and no tolerance moves.
  - In gen0152 the trim appears two ops later.
  - In gen0678 no later op asks for it. A lazy UV trim is made when something needs it.
- **gen0266:**
  - Main's tolerance, 8.10e-3, does not cover its own extended curve's gap of 0.0130.
  - The original curve, which this change keeps, is covered by the tolerance it has.

### gen0266 refusal
- `LiftCurve`'s only segment starts at uv (18.834, 34.638), direction (0.6726, -0.7357), over [-43.708, 49.763]. The natural domain is [-0.05, 1.05]².
- The line meets u ∈ [-0.05, 1.05] only where v ≈ 55, and v ∈ [-0.05, 1.05] only where u ≈ 49.5. So no point of the segment is on the surface.
- The old lift's curve was 1.847e10 off the surface against a tolerance of 3.3e-6 (largest distance from S(uv(t)) over 2001 t, measured with a temporary probe). The refusal is correct.
- **Per-op record lengths in gen0266, main vs new.** Header 1; VertexOffset 2; AddCurve 3 and 3; HaveTangentGaps 2; MateEdges 88; ExtendTrimEdges 183 vs 87; UpdateFaceDomain 2. Total 284 vs 188.
- **A note on the harness.** Every record block holds min, max and sum. The control points carry an `f64::MIN` weight sentinel, so the sums are ±inf or NaN, and the golden pins little beyond the counts. This weakness predates the change.

## Side effect removed with the context-free readers

Main's context-free `GetDomainWithContinuity` and `LiftCurve` called `get_surface_cache`. Through `keep_standalone_trim_cache`, that call stored a trimmed cache in the surface's slot whenever the surface's owner was cleared. The readers no longer make a cache, so that store is gone.

Tracing found no reader that depends on it:
- The owner is cleared only in three scopes: `create_uv_trim_curves_body` (`topology/face/p3.rs`), `topology/edgeuse/p3.rs` and the gap snapshot's `clear_surface_owner` (`query/face/p2.rs`).
- None of the 22 non-test call sites of the lift or the domain read is inside one of them.
- The drop passes its own cache. Its `None` branch is test-only.
- The B-rep constructor's ownerless lift runs on `surface.clone()`, whose slot starts empty, with the continuity checks skipped.

## Not changed

The fast-drop face test's point-test bit still depends on whether a face's cache was built, on faces without loops. See `plan/findings/blend.fillet_executive_p3--rollover-404-face-uv-domain-vmin--addendum-2026-11-20-fast-drop-read-kept.md`.

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
- A curve-bounded surface is the exception for the continuity domain: `GetDomainWithContinuity` and `DomainWithContinuity` peel its curve-bounded layers (`Surface::geometry`) and answer for the basis, with the basis's own continuities. That is exact: the kind holds only its basis, its boundary curves and `implicit_outer`; every evaluator passes `uv` to the basis unchanged; its natural domain is the basis's; and every edit that would reparameterise it is refused for the kind (file and line in the doc comment of `Surface::domain_with_continuity`). Nothing else of the kind changes: `GetKnots`, its continuities, its iso curves and its cache tree still give the base-class error, so `LiftCurve` with the continuity checks on, the cache tree build and the drop's boundary start points still fail on it as before. `LiftCurve` with the checks skipped now lifts on it as on the bare basis.

## `surface.surface_p1` `DomCont`

The `history` input runs real calls before the query:
- the surface's cache requested during a Boolean operation, so its tree is deferred and its arrays are empty;
- then `1`: the query during the Boolean operation; `2`: `GetTree`, then the query during the Boolean operation; `3`: the query after it.

Every `domhistNNNN` expectation equals the answer with no history. That equality is the check.

The exception is a history step that fails. Its record is the marker `-1` (`DOM_CONT_HISTORY_FAILED`), not a status, followed by the query's own record, so the kernel's answer stays pinned there too. Only `GetTree` of a curve-bounded surface fails there, because the tree build needs the knots such a surface does not answer (https://github.com/tangency-labs/solidcraft/issues/20163, a general follow-up). That holds for 0052, 0074 and 0075 under history `2`, as on main. Main's harness wrote the failed step's 1001 and never ran the query. Any other failed history step is a harness error.

Against main, 8 cases are blessed with new values and 30 move to known failing (12 of them changed here, 18 unchanged from main). The old values came from the empty arrays of a cache without a tree, or from the knot query a curve-bounded surface does not answer. Proof of history-freedom: the harness's standing check, on every run (a case with history also queries a fresh surface without history; any difference is a harness error).

- **8 blessed:** the 7 below, each start point on the natural domain, and domhist0052 (curve-bounded, history 2, start point off the basis's domain in u by 1.80): its record is the marker, then status 0 and [1.40601, 1.96957] × [0, 1], the span next to the point. It is history-free and equals the fresh bare basis's answer, but it starts off the domain and is not shown wrong; what an off-domain start should get is #20239's question.
  - **2 interior-knot cases on B-splines:** 0078 and 0083. The new value is the C-span around the point, correct from the knots.
  - **5 on curve-bounded surfaces** (0017, 0070, 0074 and 0075 under a history, gen0471 without): each equals `GetDomainWithContinuity` of the bare basis made fresh (the goldens test `curve_bounded_domhist_rows_answer_as_the_fresh_basis`). Worked from the basis's knots:
    - 0070 (C1C2): u knots 0, 0.50087, 1 at degree 1, the interior one C0, a real crease; the point's u = 0.6557 is past it, so u in [0.50087, 1]. The v interior knot 2.2743 is C1C2G3, so v is the whole [0.92601, 2.41351].
    - 0017 (C1C2C3): v interior knots 0.55656 and 0.77821 are C1; v = 0.968 is past both, so v in [0.77821, 1]; u has no interior knot, [0, 1].
    - 0074 (G1G2G3): v interior knot 0.42025 is G1; v = 0.7528 is past it, so v in [0.42025, 1]; u is the whole [0.55760, 3.53109]. The record is the marker, then status 0 and that domain.
    - 0075 (C1C2G3): u interior knot 0.84648 is G1, u = 0.2158 is before it, so u in [0, 0.84648]; v interior knots 0.74962 and 0.77701 are C1, v = 0.8061 is past both, so v in [0.77701, 1.67144]. The record is the marker, then status 0 and that domain.
    - gen0471 (C1G2G3): u interior knots 0.31642 and 0.76448 are C1, u = 0.9789 is past both, so u in [0.76448, 1]; v is the whole [-0.00948, 0.66687].
    - Main answered status 1001 there (the kind's `GetKnots`), and for 0074 and 0075 never ran the query.
  - In these seven the first and last knots are the natural bounds, so where the end-knot comparison of WR-20239 fires at an on-domain start point it sets a bound to the natural bound and does not move the answer.
- **30 known failing, no golden line** (`expected/known_failing.json`, under https://github.com/tangency-labs/solidcraft/issues/20239): the `DomCont` cases whose answer is shown wrong, all with start points off the natural domain. 12 are cases this change moves (0009, 0011, 0023, 0043, 0049, 0061, 0062, 0063, 0067, 0071, 0073, 0079); 18 are unchanged from main and removed only because the same evidence shows their values wrong (0002, 0004, 0006, 0021, 0028, 0029, 0035, 0051, 0057, 0068, 0081, 0082, 0086, 0088, 0089, 0090, 0093, 0095).
  - Evidence per case, named in its entry: (a) the answer changes when the direction is scaled by 1e-3, 1 and 1e3, the same request about the same surface (13 cases; the end-knot test compares a component of the direction with the knot value); or (c) the answer is a box of zero width in u or v at status 0, neither a refusal nor a span, so wrong whether #20239 decides on a refusal or a clamp (17 cases).
  - A box that does not contain an off-domain start point is not by itself counted as wrong: under a sign-based clamp, the fix #20239 proposes, such a box can be exactly right.
  - A case is one line, so 0061, 0062 and 0071, off in one direction only, are listed whole.
  - The goldens test `known_failing_domcont_cases_are_shown_wrong` holds the evidence both ways over every `DomCont` case, STEP surfaces included: listed exactly when the answer varies under the scaling or is degenerate; every case with a golden line is scale-invariant and non-degenerate.
  - 9 off-domain cases are neither and keep their lines: 0000, 0026, 0040, 0044, 0053, 0058 (the full natural domain), 0046, 0052, 0085 (the span next to the point). The 8 other than 0052 are main's lines, byte for byte. The 6 off-domain cases on STEP surfaces (0005, 0013, 0018, 0020, 0055, 0084) answer the whole STEP domain and keep their lines too.

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

**Statuses** (records read with a temporary print on both kernels, `[op code, status]` at the op's record index):

| Case | ExtendTrimEdges, main | ExtendTrimEdges, new |
|---|---|---|
| gen0152 | index 10: `[12, 1001]` | index 10: `[12, 1001]` |
| gen0266 | index 99: `[12, 0]` | index 99: `[12, 1001]` |
| gen0462 | index 225: `[12, 1001]` | index 225: `[12, 1001]` |
| gen0678 | index 352: `[12, 1001]` | index 352: `[12, 1001]` |
| gen0760 | index 71: `[12, 1001]` | index 71: `[12, 1001]` |

So in four of the five cases `ExtendTrimEdges` refuses on both kernels, and the edge's curve and interval are unchanged. On main the refused operation still edited its input: the lift's face-cache build made a UV trim on one edge use and, in gen0462 and gen0760, raised the edge tolerance (2e-5 -> 0.993976, 1e-5 -> 1.08423). Here a refusal leaves its input exactly as it came in. That is "a refusal no longer edits its input", a more correct result.

**Contract.** Upstream's `ExtendTrimEdges` (`source/SMLib/src/SmOffsetGeometryCreation.cpp:1384`) sets UV trims only through `ReplaceEdgeCurves`, on its success path (`:1719` and `:1759`). The lift made a trimmed tree in this golden only because the goldens shim seeds the extended-surface cache with each face's own surface (the unit's module doc, `insert_extended_surface`), so `GetCreateExtendedSurface` returns the face's surface. In the real offset executive the extension is a new store surface (`offset/offset_executive/edgeuse.rs:1385-1392`), owned by no face, so its cache build makes no face trims. The solid offset also runs under the Boolean flag (`with_doing_boolean`, `offset/offset_executive/p2.rs:816`), which defers that build; the extended offset (`do_extended_offset`) leaves the flag as it is. So no real offset, shell or thicken lost a trim or a raise.
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

- **gen0462 and gen0760: the input is invalid as the script makes it.**
  - The script's raw setters (ReplaceEdgeCurves, SetEdgeCurve) put a curve 0.4965 and 0.6260 off its face before the op, on both kernels, against an edge tolerance of 2e-5 and 1e-5.
  - On main the refused op's lift built the face's cache tree as a side effect; the build made a UV trim on one use, and making it raised the tolerance to cover the gap (2.002 and 1.732 times it).
  - Here the refusal leaves the invalid input as it came in.
- **gen0152 and gen0678: only the trim's timing changes.** Their curves lie on the surface (within 1e-15 at the samples), and no tolerance moves.
  - In gen0152 the trim appears two ops later.
  - In gen0678 no later op asks for it. A lazy UV trim is made when something needs it.
- **gen0266:**
  - Main's tolerance, 8.10e-3, does not cover its own extended curve's gap of 0.0130.
  - The original curve, which this change keeps, is covered by the tolerance it has.

### gen0266 refusal
- `LiftCurve`'s only segment starts at uv (18.834, 34.638), direction (0.6726, -0.7357), over [-43.708, 49.763]. The natural domain is [-0.05, 1.05]².
- The line meets u ∈ [-0.05, 1.05] only where v ≈ 55, and v ∈ [-0.05, 1.05] only where u ≈ 49.5. So no point of the segment is on the surface.
- The old lift's curve was 1.847e10 off the surface against a tolerance of 3.3e-6 (largest distance from S(uv(t)) over 2001 t, measured with a temporary probe). The refusal is right by geometry.
- **But the code reaches it through WR-20239.** The lift asks `GetDomainWithContinuity` for the segment's domain with the UV curve's derivative, not normalised, as the direction. Both bounds collapse only because that derivative's components compare as they do with the first and last knots. A faster parameterisation of the same segment would be lifted onto the off-surface curve again (https://github.com/tangency-labs/solidcraft/issues/20239).
- **Per-op record lengths in gen0266, main vs new.** Header 1; VertexOffset 2; AddCurve 3 and 3; HaveTangentGaps 2; MateEdges 88; ExtendTrimEdges 183 vs 87; UpdateFaceDomain 2. Total 284 vs 188.
- **A note on the harness.** Every record block holds min, max and sum. The control points carry an `f64::MIN` weight sentinel, so the sums are ±inf or NaN, and the golden pins little beyond the counts. This weakness predates the change; the same class of harness weakness is tracked in https://github.com/tangency-labs/solidcraft/issues/17001.

## Side effect removed with the context-free readers

Main's context-free `GetDomainWithContinuity` and `LiftCurve` called `get_surface_cache`. Through `keep_standalone_trim_cache`, that call stored a trimmed cache in the surface's slot whenever the surface's owner was cleared. The readers no longer make a cache, so that store is gone.

Tracing found no reader that depends on it:
- The owner is cleared only in three scopes: `create_uv_trim_curves_body` (`topology/face/p3.rs`), `topology/edgeuse/p3.rs` and the gap snapshot's `clear_surface_owner` (`query/face/p2.rs`).
- None of the 22 non-test call sites of the lift or the domain read is inside one of them.
- The drop passes its own cache. Its `None` branch is test-only.
- The B-rep constructor's ownerless lift runs on `surface.clone()`, whose slot starts empty, with the continuity checks skipped.

## Not changed

The fast-drop face test's point-test bit still depends on whether a face's cache was built, on faces without loops. See `plan/findings/blend.fillet_executive_p3--rollover-404-face-uv-domain-vmin--addendum-2026-11-20-fast-drop-read-kept.md`.

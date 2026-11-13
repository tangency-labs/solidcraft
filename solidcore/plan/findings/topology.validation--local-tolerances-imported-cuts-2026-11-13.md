# Local vertex uncertainty and imported cuts (2026-11-13)

The kernel's tolerant-model validity checks must use the local uncertainty already
stored on a vertex. A finite local radius larger than 1,000 body tolerances does
not, by itself, show that the boundary is wrong. Geometric gaps still need bounds.
The contract is described in [Validity and tolerances](../../docs/validity-and-tolerances.md).

## Tolerance writers and the diagnostic

The STEP reader measures endpoint gaps, propagates edge tolerances to vertices,
and covers vertex-loop surface gaps (`solidcore-step/src/topology.rs`). Trusted
flat import converts the record tolerance to a kernel radius and skips healing.
The exported reader record for the reported socket has four relevant vertex
tolerances around 0.008220648 mm, converted to radii around 0.004110324 mm.

Two later writers can produce the reported radii 0.0056613767 and 0.0055817271 mm:

- A vertex/face projection can take the global fallback in
  `topology/vertexuse.rs::compute_uv_point`. Its face-cache lookup reaches
  `query/face/p2.rs::BrepCacheFace::create_uv_trim_curves`, which requests tolerance
  adjustment. `topology/face/p3.rs::create_uv_trim_curves_body` then raises a vertex
  radius to twice its curve-end gap. Merely measuring a cold face can therefore
  change its stored uncertainty.
- Boolean result processing in SolidCraft's `crates/smlib/src/core/boolean.rs`
  calls `validate_and_update_tolerances`, which also grows vertex tolerances to
  twice their edge or face gaps.

The measured endpoint gaps are 0.0028306883266788 and 0.0027908635563428 mm;
doubling them explains the reported values. The first invocation on the reported
body has not been isolated. The application's failure message prints the result's
report. Its first-operand annotation independently checks whether that operand
fails validation; it does not compare the two failure lists.

Trim-cache tolerance growth is an established mechanism, also documented in
[the cache-history finding](general--trimmed-surface-cache-history-tolerance-growth-deferral.md).
Pinned C++ was not executed for this investigation. This finding records the
Rust validity contract and evidence, without claiming a new native defect
confirmation. Related upstream records are WR-381 (shared junction uncertainty),
WR-93 (open-loop reporting), and WR-679 (the tolerance updater's half-radius test).

## Local geometric bounds

The vertex ratio limit is replaced by a finite-value check. The existing lower
bound remains. Face and solved-corner gaps use the larger of the vertex radius
and the body's existing intersection tolerance. Strict corner queries used by
repair retain their existing semantics.

For a loop junction, the previous curve end and next curve start must name one
topological vertex. Each endpoint must lie within that vertex's radius plus its
own edge zone. The triangle inequality then bounds their mutual gap by
`2 * vertex_radius + previous_edge_zone + next_edge_zone`. Distinct vertices and
endpoints outside those individual bounds do not acquire this allowance. The
check adds no stored tolerance and moves no point.

The existing tests in `tests/issue_1419_brep_check.rs` reproduce the four ratio
failures with a 10 mm cube and body tolerance 0.000005 mm, then exercise a real
0.002 mm corner gap and a radius-1.5 mm through cut. Its analytic volume is
`1000 - pi * 1.5^2 * 10 = 929.314165294230 mm^3`, with allowed error 0.000001 mm^3.
The final measured volume is 929.314165296871 mm³ (absolute error about
0.000000002642 mm³). All six whole-body tests and all thirteen loop-use tests
pass, as does all-target clippy with warnings denied.
The out-of-bound fixture prepares query caches and restores its recorded radius
before checking, so lazy query healing cannot repair the deliberately invalid
input. `topology::loopuse::tests` covers both loop directions, distinct vertices,
and individual endpoint bounds.

## Application measurements

The public socket import is one closed body with 46 faces and volume
1785.049600532154 mm³. An XY circle centered at `[5, 5]`, radius 1.5 mm, cut through
symmetrically, now evaluates with normal validation: one closed body, 48 faces,
126 edges, 82 vertices, volume 1756.560081917680 mm³. It removes
28.489518614474 mm³ and reports no cut error or warning.

The old executable with validation disabled produces exactly the same volume and
topology. The correction therefore admits that existing geometric result.
YZ through also succeeds, measuring 1770.807836546366 mm³. XZ through is
effectively unchanged. The symmetric 10 mm evaluation measures
1785.055655493262 mm³, a 0.006054961109 mm³ increase; the old unchecked executable
produces exactly the same value and topology. That existing measurement drift is
not resolved by this validity change.

The exact-box mutation with a finite 0.1 mm vertex radius now has no validity
failure. The old golden `box_mutation1_capi` encodes two lines: ratio failure 82
and skipped `AssertValid`. Those expectations need deliberate baseline review;
the boundary itself is unchanged.

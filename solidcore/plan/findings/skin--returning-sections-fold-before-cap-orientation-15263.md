# Returning loft sections fold before cap orientation (#15263)

The supplied design's shell has consistent coedge directions and negative signed
volume, but it is not a regular solid turned inside out. A side surface folds and
its two boundary curves cross. Neither whole-shell reversal nor cap-only reversal
can meet the requested positive-volume, downward-cap and valid-solid conditions.
The repair now aligns section winding and seam correspondence before skinning;
all requested targeted tests, clippy and same-main record comparisons now pass.
The historical approximately 1338 mm^3 target remains incompatible with the
repaired quadratic arch; see the exact volume proof below.

## Input and topology

- First section: square `(0, 0, 0)` to `(10, 10, 0)`.
- Middle section: square on `x = 15`, spanning `y = 0..10`, `z = 5..15`, facing +X.
- Last section: square `(20, 0, 0)` to `(30, 10, 0)`.
- Both end sections are separate regions of one XY sketch.

The installed 0.40.1 application's native B-rep cache has six faces and twelve
edges. Every edge has two incident coedges traversed in opposite directions after
applying the shell's face senses. The finite region is material. The cap normals
are +Z at the start and -Z at the end. This is not an individually reversed cap
relative to its neighbours, or an all-void classification.

## Exact side-surface counterexample

The side at `y = 0` is one polynomial quadratic-by-linear Bezier patch. Its control
rows, from the native cache, are:

| U row | V start | V end |
| --- | --- | --- |
| 0 | `(0, 0, 0)` | `(10, 0, 0)` |
| 1 | `(21.353285428169606, 0, 30.128263466804633)` | `(11.267776450299852, 0, 10.042754488934877)` |
| 2 | `(20, 0, 0)` | `(30, 0, 0)` |

There are no rational weights. U knots are `[0, 0, 0, 1, 1, 1]`; V spans one knot
interval. With both parameters normalized to `[0, 1]`, the patch is
`S(u, v) = (1-v) C0(u) + v C1(u)`, where each column defines one quadratic Bezier
curve. The listed decimals round-trip the stored binary64 values; interpreting
those values as exact rationals permits polynomial arithmetic without quadrature
or a display mesh.

For `N = Su cross Sv`, at `v = 1/2`, `N.y` is positive at `u = 0`
(`401.7101795573951`) and negative at `u = 1` (`-401.7101795573951`). Both other
components are identically zero. Continuity therefore proves an interior zero
Jacobian. Its location is approximately `u = 0.8226544502170969`, at
`(21.83548648323003, 0, 5.860714741924799)`.

The two boundary curves also intersect. This can be checked directly from the
control rows: write

```text
C0(u).x = A1*u + A2*u^2       C0(u).z = a*u*(1-u)
C1(w).x = 10 + B1*w + B2*w^2  C1(w).z = b*w*(1-w)
q = (A2/a - B2/b) / 20
t(w) = w + 1/2 + q*b*w*(1-w)
F(w) = a*t(w)*(1-t(w)) - b*w*(1-w)
```

Here the coefficients are obtained from the control rows by the usual quadratic
Bernstein-to-power conversion. Equal X and Z require `u = t(w)` and `F(w) = 0`.
Exact rational evaluation gives `F(69034/100000) > 0` and
`F(69035/100000) < 0`; the corresponding outer parameter is in `(0.92277, 0.92279)`.
Thus an interior crossing exists. Bisection places it at
`u = 0.9227812880161866`, `w = 0.6903471893406292`, approximately
`(20.073604775974665, 0, 4.2936380288953595)`.

## Signed volume and the repair boundary

Integrating `z * (Su cross Sv).z` over the four oriented polynomial side patches,
with the two caps at `z = 0` contributing zero, gives the exact rational volume

```text
-26522299491339669455399088577359 / 19807040628566084398385987584 mm^3
= -1339.0339318579836 mm^3
```

Whole-shell reversal makes this integral positive but preserves the fold and
crossing, and changes the end cap to +Z. Reversing only the start cap breaks its
coedge agreement with its neighbours. A valid arch requires correcting section
winding and seam correspondence before creating the skin. Reversing a closed
profile alone preserves its seam point and does not suffice: in the application's
correspondence, the boundary starting at `(0, 0, 0)` ends at `(20, 0, 0)`; the
untwisted arch would instead pair it with `(30, 0, 0)`.

The historical 0.30.1 measurement is `1338.1541291849142 mm^3`, area
`1219.4898001265678 mm^2`. Those measurements are a comparison, not a correctness
proof for the current side patches.

## Section correspondence repair

`primitive_creation::p3::construct::create_skin_from_faces_traced` now carries each
section's planar normal and exact face centroid to the next section. The new
alignment applies to analytic planar profile faces; other face-based skins keep
their previous correspondence. For the
centroid displacement, opposite signs of the previous and current normal's dot
products mean the current loop needs reversal. Degree-one profiles use exact polygon area moments for their centroids (including
a whole polygon stored as one B-spline edge); curved profiles use the existing
face-centroid integrator. The first loop keeps its winding.
The current aligned normal becomes the reference for the next section, including
the last section.

At a turn or a winding reversal, the loop's cyclic seam is selected by the minimum
sum of distances to corresponding points on the previous joined curve. The points
are exact curve evaluations at knot-span starts and midpoints; no tessellation is
used. Curves and their source edge IDs reverse and rotate together, before join
breaks are recorded. A polygon supplied as one closed B-spline is split at its distinct
knots so its corners are also seam candidates. Every split piece keeps the original
edge source. Already aligned parallel sections keep their original correspondence.

No cap is reversed in isolation, and nothing re-orients the shell after stitching: an
earlier version of this fix also classified the exterior again when a capped skin's
signed volume came out negative, but with the correspondence fixed no test reaches that
step (solidcore skin 11, engine loft 42 and kernel loft 39 all pass without it), so it
was removed rather than kept as a fallback. The C++ defect is recorded as WR-778 in
`plan/upstream-issues/`. Centroid and volume integration use relative accuracy `1e-9`, independently
of the skin approximation tolerance (`0` requests exact section interpolation).
The first post-change run exposed why those quantities must not share one value:
passing zero to mass integration caused `Mass(StepControl)` in all three existing
parallel-section tests. That wiring was corrected before the queued retries. The next run passed all three
parallel-section cases and the four-edge arch regression at `2666.6666666666665 mm^3`,
with both caps -Z. Whole degree-one B-spline loops still hit step control during
centroid integration, including the existing kinked-curve case. The exact area-moment
centroid path addresses that representation. The final targeted rerun passed both
representations and the existing kinked-curve case.

For the untwisted symmetric quadratic square arch, the outer and inner boundaries
in the XZ plane are

```text
outer(t) = (30t, 60t(1-t))
inner(t) = (10 + 10t, 20t(1-t)),  0 <= t <= 1.
```

Both hit the prescribed middle section at `t = 1/2`. Their enclosed area is
`integral(z_outer dx_outer - z_inner dx_inner) = 300 - 100/3 = 800/3 mm^2`.
The width is 10 mm, so the exact volume is **8000/3 = 2666.6666666666665 mm^3**.
The exact area is
`200 + 1600/3 + 200 sqrt(5) + 100 asinh(2) = 1324.9104763511723 mm^2`.
Thus retaining the historical approximately 1338 mm^3 measurement conflicts
with the untwisted quadratic interpolation through these sections. The exact fold
proof above applies to the current 0.40.1 native record; the 0.30.1 measurement
alone does not prove that older kernel's boundary was valid. The regressions assert the independently derived `8000/3 mm^3` expectation; no
tolerance is widened to hide that difference.

## Baseline regression and validation

The existing `skin_sources.rs` test file now builds the three raw square profiles,
checks six faces, manifold closure, opposite radial edge-use directions, positive
volume, both cap normals -Z, and an outward solid. The raw API's correspondence
differs from the application's: it gives signed volume `-1072.1233698200338 mm^3`
and cap normals +Z/-Z. Its cap-normal assertion fails. Its level-0 validity report
is empty, demonstrating why that report alone does not settle the geometry.

The existing engine `tests/loft_spacing.rs` file now reproduces the same-sketch
end sections through commands and requires successful positive mass properties.
It fails with the reported `invalid input: no mass` error.

Results before the production repair:

- `cargo test --manifest-path solidcore/Cargo.toml -p solidcore --test skin_sources --test skin_parallel_sections -- --nocapture`: ten existing tests pass; the new regression fails.
- `cargo test -p solidcraft-kernel loft`: 39 pass.
- `cargo test -p solidcraft-engine loft`: 41 existing tests pass; the new regression fails.
- Clippy with `--all-targets -- -D warnings` passes for solidcore (its own manifest) and solidcraft-engine.
- Formatting of the changed Rust files and `git diff --check` pass.

Final post-change results are recorded below. The repaired volume, 8000/3 mm^3, differs from 0.30.1's
1338.154 mm^3: 0.30.1 built the same folded skin, which happened to close into a valid but pinched solid.

## Same-main comparison procedure

The installed 0.40.1 CLI has commit `0c5292285a7e9331a780c11a2d55f22d8e1c9118`.
Current main is
`30c28c11eaee3281046b7ab959c860d1de0b3faa`. Their embedded model-source
fingerprints differ. Matching version numbers therefore cannot establish the
required byte identity against current main.

The comparison builds a CLI with the two production files restored temporarily
from HEAD, keeps that binary aside, and restores the exact patched source
bytes in a `finally` block. It then builds the patched CLI. Five ordinary lofts cover ascending and descending prisms, ascending
and descending three-section waists, and a rectangular ring. Each replay saves
its cache immediately after loft creation, before any measurement can warm
caches. The comparison requires both the complete canonical solid records and
the raw `cache/bodies.bin` payloads to be byte identical. All five comparisons passed; hashes are below.

## Post-change results

The engine loft run passed all 42 tests, including the new command regression,
hollow-elbow cuts, rings, joins, chamfers and rolling-ball fillets:

```text
cargo test -p solidcraft-engine loft -- --nocapture
42 passed; 0 failed
```

The new command regression measured one closed solid with six faces, twelve edges
and one shell. Both end-cap normals were -Z. The measured volume was
`2666.666666666667 mm^3`, area `1324.9104763513217 mm^2`, center of mass
`(15, 5, 6.500000000000001) mm`, and mass `3.4400000000000004 g` at the default
`1.29 g/cm^3` density. Volume, area and centroid agree with the independent
analytic arch calculation above.

The final solidcore run passed all eleven tests (three parallel-section tests and
eight source-tracing tests), including both four-edge and whole-curve arch profiles:

```text
cargo test --manifest-path solidcore/Cargo.toml -p solidcore \
  --test skin_sources --test skin_parallel_sections -- --nocapture
skin_parallel_sections: 3 passed; 0 failed
skin_sources: 8 passed; 0 failed
```

Both arch representations had six faces, four traced side faces with three section
sources each, manifold closure, consistent radial coedge directions, both caps
outward -Z, and a certified outward solid. Signed volumes were
`2666.6666666666665 mm^3` (four edges) and `2666.666666666667 mm^3`
(one closed curve). This run includes the exact degree-one centroid path.

Kernel loft validation also passed, including the existing loft record test:

```text
cargo test -p solidcraft-kernel loft
39 passed; 0 failed
record::tests::loft_sections ... ok
```

The record test checks reversed and rotated square sections, two and three
sections, circle-to-square provenance and loft-to-point provenance. The same run
passed existing non-convex section pairing, reversed section order, holed profiles,
rail lofts, joins and planar-side cases.

Both changed crates passed clippy across all targets with warnings denied:

```text
cargo clippy --manifest-path solidcore/Cargo.toml -p solidcore --all-targets -- -D warnings

cargo clippy -p solidcraft-engine --all-targets -- -D warnings
```

The final changed-file rustfmt checks (solidcore edition 2021, engine edition 2024)
and `git diff --check` also passed. The same-main ordinary-record comparison and original-design replay
also passed, as recorded below.

The pristine same-main build restored the exact patched source bytes afterwards:

```text
p2/skin.rs sha256 5df6610e42e7e3626972f662ab9a960cc4878a7b3f1993ca0e02b0ed8a173acc
p3/construct.rs sha256 40b0a21b3703085906300e55871a62c7d21cb86dff8af2f215e088cdd6a9a0b7
```

A fresh replay of `.loft-uturn-repro.sc` with that same-main binary produced a
native solid exactly equal to the installed 0.40.1 diagnostic solid above, including
all stored geometry and topology. This confirms that the exact negative-volume,
zero-Jacobian and crossing-boundary proof applies to current main too.

## Final native-record verification and evidence

A fresh replay of the supplied design was made with the patched CLI. The design has no initial geometry cache,
so this is rebuilt geometry. (a small exact verifier over the saved native B-rep)

The verifier reads the native B-rep, checks the full rectangular trims and unit
weights, then treats stored binary64 control points as exact rationals. It
certified six faces, one shell, a finite material region and two outward -Z caps.
For the `y = 0` side, exact Bernstein subdivision certified a nonzero Jacobian
with the correct outward -Y sense over the entire parameter square (three boxes,
absolute Jacobian lower bound 100). The two arch boundary curves were proved
disjoint using fifteen exact parameter boxes, eight separated leaves. The signed
volume contributions from the nonzero patches were `3000` and `-1000/3 mm^3`.
Their sum is exactly **8000/3 mm^3**, with zero difference from the independent
analytic expectation. No mesh enters these checks.

The fresh CLI measurement agrees with the engine regression:

- One closed body; six faces, twelve edges, eight vertices, one shell.
- Volume `2666.666666666667 mm^3`; area `1324.9104763513217 mm^2`.
- Center of mass `(15, 5, 6.500000000000001) mm`.
- Mass `3.4400000000000004 g` at `1.29 g/cm^3`.
- Bounds `(0, 0, 0)` to `(30, 10, 15) mm`.

Before and after images were captured from real CLI runs with the same isometric view,
1200 x 900 size, axes and grid hidden, and the software GPU renderer. Both were
visually inspected; the after image shows the arch without the crossed side.

## Ordinary lofts are byte identical

A script compared a CLI built from current main with the patched CLI, design by design.

All five complete canonical solid records **and** raw `cache/bodies.bin` payloads
were byte identical. Metadata and runtime fields also matched. These records were
saved before measurement. Ascending and descending cases check both directions
of travel; the waists check three sections; the ring checks inner and outer loops.

| Case | Bytes | Native payload SHA-256 (same before and after) |
| --- | ---: | --- |
| prism-up | 12864 | `0252af67bb7369d5d025075705759785a3c0db2a7401e7043d0a08eca2dd9eeb` |
| prism-down | 12864 | `57143f9aee8db38f553cf291e46e8984298867cafc231b96c2dd0f8b404dfcab` |
| waist-up | 14732 | `1f24db440a0b0002e3b4e0dd046d203781492d4752539521ea00602feafdd037` |
| waist-down | 14732 | `c53625541c80f3ead3af2fb3dc610c5c51012e76c6b18eafd49a72ce4a50f2eb` |
| ring | 11962 | `3a1d39bc83c9be938b5c107e0645ea833b2b2a4a8ec7fab4d86552173e7688e0` |

## Volume against 0.30.1

The repaired solid has volume 8000/3 mm^3, proved independently of the build. 0.30.1 measured 1338.154 mm^3 for the
same design because it built the same folded skin, which happened to close into a valid but pinched solid; the
regressions assert the proved value.

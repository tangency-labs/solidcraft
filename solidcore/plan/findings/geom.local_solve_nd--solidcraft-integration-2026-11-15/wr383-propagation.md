# WR383 propagation audit: plane trim and matrix-transform goldens

This is a private measurement audit for SolidCraft PR #14102. No production source, index, golden expectation, branch or public application was changed. No Cargo build was run. The typed observer links the already built Rust 1.99.1 goldens libraries; its compilation and invocation are recorded in `unbundled-validation/nov15/paired/golden-review/typed-invocation.json` (run from the SolidCraft worktree so the pinned compiler is selected).

The raw inputs match exactly in main, N-D-only and paired phases. Both complete raw outputs and typed outputs match between main and N-D-only. These two differences therefore arise when WR383 is also made unconditional, not from a different generated input.

## surface.plane_p2/gen0599: independently supported measured expectation

The operation is `Plane::trim_curve_to_plane_domain` on a degree-one, four-control-point nonrational B-spline with requested `tol_3d = 0.5`. This API intersects the four rectangle boundaries within the supplied tolerance, keeps classified inside/coincident intervals, and emits a degenerate curve where consecutive outside intervals meet. Its contract is an approximate trim, so an emitted contact point need not be an exact boundary crossing.

Main and N-D-only return four curves. The first three survive bit for bit. The old final two-span piece on `[0.4856981795830564, 1]` becomes one isolated approximate contact plus two line pieces on `[0.773285216527619, 0.859375]` and `[0.859375, 1]`. Thus the paired result has six curves: five degree-one spans and one point, not twelve independent segments. The degenerate point's own domain `[0,1]` is not a parameter interval on the original input curve; its original parameter is approximately `0.4856981795830564`.

The independent certificate treats every stored binary64 input as its exact dyadic rational. It solves the affine plane Gram system with Python `Fraction`, clips each source line analytically against the rectangle, and bounds distances over whole spans by convexity. It does not call kernel intersection/drop functions and does not use sampling for acceptance. The constructed plane axes, after `Plane::new` canonicalization, are the actual plane definition measured by the typed observer; all three phases use identical axes.

* Projected source hard-rectangle clip: `[0.0347076336502816, 0.20171281084005982]` and `[0.7732852165276186, 1]`.
* Input polyline's maximum off-plane residual: `2.2073784383315977e-15` mm.
* Entire new output remains on the input polyline within `6.115966546072321e-15` mm (endpoint convexity on matching degree-one spans).
* Entire new output is within `0.49230242729914814` mm of an explicit point in the closed rectangle, plus a conservative binary64 plane-evaluation envelope `3.341522978729647e-12` mm. The resulting bound `0.4923024273024897` mm is strictly below the original `0.5` mm tolerance.
* The exact projected inside intervals are covered apart from an endpoint rounding gap of at most `8.2740383433019e-15` mm.
* The isolated point is `0.49230242729914814` mm from the rectangle witness. It is an approximate contact, not an exact intersection.

The old output also fits inside the broad supplied distance tolerance; this audit does not claim every removed old point was individually invalid. It establishes the new returned set satisfies the API's geometric constraints and retains the exact inside portions. The oriented-range-end correctness justification is the separate WR383 endpoint invariant, rather than equality to a previous golden or to C++.

Scientific plots use actual typed control polygons, common axes, and a parameter strip for the analytic clip versus the actual returned intervals. The V axis is explicitly expanded for readability. Both PNGs were opened and inspected:

* `plane0599-before.png`
* `plane0599-after.png`
* Certificate `plane0599-certificate.json`; source `unbundled-validation/plane0599-audit.py`.

## topology.brep_mirror/mat0076: success returns an invalid body, already on main

Do not bless this as a correct body. `transform_matrix` returns `Ok(())` for `Solids/revolution.smb`, `rebuild=false`, `editing=true`, `scale_tol=true`. The exact finite matrix is preserved in the JSON report and raw input. A separate re-read with a separate Kernel verifies that the input body passes Level 2 (`pointers_status=1000`, `assert_valid=Some(true)`, zero reports) in all three phase libraries. This check runs independently of the operation's measurement order.

The main/N-D-only result fails Level 2 with 16 reports. The paired result fails with 17: it preserves all 16 prior failures and adds one V-direction UV loop gap. Main already has a 3D loop edge gap `3.419267465560401 > 2.1633192809335735e-5`; therefore main's old status-zero output is not an acceptable correctness baseline either. The observed new UV gap is `4.309257867596689 > 3.4474062940770893` on loop 3.

The vertices, seven 3D surface supports, all 15 3D edge-curve supports, graph/owner identities and integer output are unchanged between main and paired. Vertex coordinates agree with an independent direct row-vector affine transform to maximum error `1.0695084443661336e-14` mm. Only edge 2's active interval changes from `[0, 34.47406294077089]` to `[4.3092578675963615, 34.47406294077089]`; both of its pcurves change to the same interval. This is not an edge/pcurve interval mismatch and must not be attributed to converter issue #13634.

At the new minimum, edge 2's point is `[49.26619729443704, 3.4304399562576973, 15.899275723659056]`. Its start vertex 4 remains at `[49.27490402347648, 3.4185897092312554, 15.817108013410422]` with tolerance `0.18099110331624244`; the measured new start/vertex gap is `0.08347315791712485` mm, within that stored tolerance. The face-2 pcurve-to-edge endpoint gap is `0.05003135184253591` mm, also within the same edge tolerance, and face 3's is `1.6110097386971715e-14` mm. These local bounds do not certify the full body, whose other loops already fail and whose new UV loop fails Level 2. There is no supported claim here that the matrix creates a valid sheet or that rank deficiency/SVD is the root cause.

The appropriate typed invariant is: independently verify the source body's Level-2 validity; if the transformation reports success, require the output to be Level-2 valid or produce a precisely tracked executed known-failure classification. Preserve the raw measured invalid output as evidence, not as a newly correct expected B-rep. The existing `BrepCheck::is_valid` / `failure_count` APIs supply that predicate; interval equality alone would miss this case.

All main output failures (the paired result contains the same failures plus the one above):

| owner | name | value | threshold | message |
|---|---|---|---|---|
| SmLoop:5 | Edge/Edge Gap | 3.419267465560401 | 2.1633192809335735e-05 | Gap between edge endPoints exceeds tolerance |
| SmLoop:5 | UVEdge/UVEdge Gap | 3.991909817225893 | 1.0000000000000002 | too large U Dir gap between loop Edge endPoints that are not on a singularity |
| SmLoop:8 | UVEdge/UVEdge Gap | 19.31370849898476 | 1.931370849898476 | too large U Dir gap between loop Edge endPoints that are not on a singularity |
| SmEdge:0 | Tolerance | 1.1951670514644068 | 0.01 | Edge tolerance relative to brep tolerance |
| SmEdge:1 | Tolerance | 1.0841157326229656 | 0.01 | Edge tolerance relative to brep tolerance |
| SmEdge:2 | Tolerance | 0.18099110331624244 | 0.01 | Edge tolerance relative to brep tolerance |
| SmEdge:12 | Tolerance | 18.00000000000001 | 0.01 | Edge tolerance relative to brep tolerance |
| SmEdge:14 | Tolerance | 3.349868850652827 | 0.01 | Edge tolerance relative to brep tolerance |
| SmVertex:0 | Tolerance | 1.1951670514644068 | 0.02 | Vertex tolerance is too wide to excuse by clearance |
| SmVertex:1 | Tolerance | 1.1951670514644068 | 0.02 | Vertex tolerance is too wide to excuse by clearance |
| SmVertex:2 | Tolerance | 1.0841157326229656 | 0.02 | Vertex tolerance is too wide to excuse by clearance |
| SmVertex:3 | Tolerance | 1.0841157326229656 | 0.02 | Vertex tolerance is too wide to excuse by clearance |
| SmVertex:4 | Tolerance | 0.18099110331624244 | 0.02 | Vertex tolerance is too wide to excuse by clearance |
| SmVertex:5 | Tolerance | 0.18099110331624244 | 0.02 | Vertex tolerance is too wide to excuse by clearance |
| SmVertex:8 | Tolerance | 18.00000000000001 | 0.02 | Vertex tolerance is too wide to excuse by clearance |
| SmVertex:9 | Tolerance | 18.00000000000001 | 0.02 | Vertex tolerance is too wide to excuse by clearance |

New paired failure:

| owner | name | value | threshold | message |
|---|---|---|---|---|
| SmLoop:3 | UVEdge/UVEdge Gap | 4.309257867596689 | 3.4474062940770893 | too large V Dir gap between loop Edge endPoints that are not on a singularity |

Full typed Debug checks, endpoints and definitions: `unbundled-validation/nov15/paired/golden-review/{main,nd-only,after}-typed.json`. The invalid transformation is tracked in [SolidCraft #17271](https://github.com/tangency-labs/solidcraft/issues/17271). This audit proposes no geometry repair.

## Matrix-case pictures and exact profile equivalence

Actual kernel bodies were rendered using already built, coherent baseline CI `solidcore`, `solidcore-convert` and `solidcore-tess` libraries, preserving the exact labels `main-ci-off-off` and `main-ci-both-old-options-on-on`. They are not relabelled as current-default output. The complete input, vertices/tolerances, every 3D edge support and active interval, all seven surface supports, all UV trim definitions/domains and endpoint gaps, and the full Level-2 Debug check match the corresponding main/paired goldens-profile measurements exactly. Only the outer `Result::Ok` wrapper is normalized for the Debug comparison. This supports a transitive comparison while preserving the actual provenance.

Both mesh conversions succeed (2086 and 2234 display triangles) despite the invalid B-reps. Meshing success and the two similar pictures do not establish validity. Both checks contain respectively 16/17 physical AssertReports; `BrepCheck::failure_count()` counts 17/18 lines because it adds the synthetic `AssertValid skipped: ValidatePointers failed` line. A tracker must distinguish these counts.

The camera is derived from the matrix: look normal to the plane spanned by its first two row vectors, azimuth -160.19156514528365 degrees, elevation 5.698029155488025 degrees. Both native geometry frames match bit for bit; the nearly flattened shape can be inspected from that view. Both pictures were opened and inspected.

* `mat0076-main-ci-off-off.png`
* `mat0076-main-ci-both-old-options-on-on.png`
* Metadata: the `mat0076.json` next to each PNG; all-exact proof `mat0076-render-provenance.json`; commands, compiler/library provenance and hashes `mat0076-render-provenance.json`.
* Private source: `unbundled-validation/render-mat0076.rs`.

## Archive scope

This report records the investigation before the integration change. Helper programs,
full typed Debug snapshots and build output referenced by the historical audit are not
bundled. Paths beginning `unbundled-validation/` are sanitized provenance identifiers.
The compact numerical archive is [wr383-propagation.json](wr383-propagation.json);
the matrix-render provenance and exact phase equivalence are in
[mat0076-render-provenance.json](mat0076-render-provenance.json).

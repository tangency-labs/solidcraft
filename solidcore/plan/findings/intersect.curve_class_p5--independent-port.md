# intersect.curve_class_p5: independent port

Part 5 remains independent of the open p2/p7 PRs. Part 6 landed independently
through PR #138 and is inherited from main. Homogenize, FixEndPointClass,
InsertCurveInterval and MergeInterval now call those real methods; no p6 stubs
remain. Both proto-topology p3 extraction sites call mutable ExtractGeometry
with geometry and default optional outputs, discarding temporary curves as native
does. Their deeper p2/p7 dependencies remain explicit.

Validation on numerical source 9e495b12 uses Ubuntu 24.04.5 x86_64, GCC 13.3.0,
Rust 1.98.0 and clean pinned upstream c9c5979. All nine affected native runners
complete with zero failures. P5 has 1,081 cases (281 fixed + 800 generated),
275 bit-identical comparisons and 806 deferrals: p2 493, p7 312 and generic
curve.crv_on_surf dispatch 1. Perturbation detects all 275 compared cases.
Its regenerated report replaces the pre-p6 report. The other eight reports
(p6, p1, p3, point classification, face query and proto-topology p1/p2/p3)
regenerate byte-identical and are excluded from this PR's changes.

Formatting, strict plain and native-enabled workspace Clippy, 1,215 kernel
tests, WASM and the focused native FFI malformed-input regression pass. The
adapter-only opt-level-0 override reduces build memory; the numerical kernel
remains opt-level 1 and the C++ reference retains its standard flags. No absolute
floors, workflow changes or build-script changes are submitted. Full CI evidence
is tracked in PR #150 separately from these focused runs.

The eight newly compared generated cases are gen0138, gen0288, gen0363, gen0393,
gen0470, gen0583, gen0665 and gen0741. These passes certify only the exercised
subset. All 24 zero_tangent fixtures still defer to p2 InsertAt, so they do not
certify tangent-flag behavior. Fixup and merge comparisons include no-op/skip
paths; nontrivial extraction, repair and insertion can still defer deeper into
p2/p7. Both ignore-tangency calls request nonzero tangents as the base declaration
does. B-spline one-derivative evaluation recursively uses its subclass default
FALSE; the port preserves that native quirk.

The edge-target insertion branch remains unported: vertex snapping,
sm_RefineCCIToSurfaces and the 2D class object need p2 InsertAt and p7 point-index
queries. The entire owned SSS block (SmCurveClass.cpp:14826-15095) remains
unported before mutation. It needs mutable curve/B-rep access and a mate-lookup
adapter for the ported GetOtherMate/GetBrepMate helpers in
boolean.topology_intersector_p1. Knot snapping, surface solves, through-point
edits and edgeuse UV-trim clearing also still need ownership wiring here.
This block does not call sm_RefineCCIToSurfaces. Concrete-Surface `CrvOnSurf`
construction is now wired, independently of that remaining owned work.
Part 5 remains partial despite all 1,081 current fixture/generated cases passing:
its owned SSS block and nonempty large-gap path are not certified by that corpus.

Missing vertex-point and vertex/face-gap callback data returns InvalidInput.
Big-gap lookup requires an explicit classification face or direct face owner of
the surface. With no face, its cutoff retains the default SmXSectTol3d, twice
the system zone tolerance. A unit regression checks that selection against the
ordinary tolerance and changed system tolerance; the attempted geometric probe
was filtered earlier by GlobalPointSolve. Nonempty large-gap projection remains
uncertified: the native vertices have no vertexuse and the Rust runner supplies
no vertex/face-gap callback. Rust continues on an ownerless surface, where native
crashes (UB-126).

Small shared interfaces are recorded as CO-PORTED in the owning units' notes:
the three ClassGeometry callbacks and optional topology-tolerance dispatch,
inline SetInterval and curve-resolver visibility on p1, and face-query vertex
handle mapping. classification_vertex_point avoids the ProtoBrep method-name
collision. The header records the object_index > 1 restriction, unresolved-curve
error in p5_closed instead of native FALSE, and the runner's zero face tolerance
that is not compared against native.

The copied native sm_InsertSolutions helper remains verbatim pinned upstream.
UB-125/126 are source-found null dereferences whose crashes were not executed;
UB-134 was reproduced during the original bundled validation. Its fixture and
report now point to UB-134. The maintainer has assigned UB-134 to this PR and will
renumber the parallel claim in #148. No bundled numerical results certify this
independent tree. The maintainer confirmed all review items resolved before the
independent p6 integration.

# PR #233 scale-reference review fix and current dependency contracts (2026-10-19)

The second review identified another derived-curve slice: `CreateCurveSweep`'s
scale reference. Upstream `SmPrimitiveCreation.cpp:4970-4986` calls the original
`pScaleReference` virtually, while `pPathCurve` (:4906), profiles (:4926) and
`pScaleCurve` (:4958) are deliberately copy-constructed as `SmBSplineCurve`.
Both public Rust sweep/preparation entry points now take `Option<&Curve>` for
that reference, and the private scale sampler uses it directly. Path/scale
copies, sample order, distance normalization and interpolation remain unchanged.
An exact Rust preparation regression uses an imported line with a two-ULP cached
NURB discrepancy: the analytic endpoint scale is 2, and slicing that reference
produces a different endpoint. It does not certify native construction, scaling
or topology behind the existing deferrals. Real derived native fixtures remain
follow-up work once construction stops deferring, as requested in the review.

The branch incorporates canonical main `4db3d660`, reviewed primitive dependency
#213 (`92005b59`) and sweep dependency #222 (`86c7eb33`). The latter already wires
the available copying, face-building, removal and binormal calls mentioned in
the second review; it retains production-adapter follow-up #255. Main's newer
B-rep APIs additionally require mutable destination contexts and explicit
notification records/attributes/identities. Those are now passed through both
sweep borrow callbacks and the p7 curve-owner wrapper. Primitive p1 cylindrical
boxes, pole-free spheres and planar boolean tails pass their caller's notification
environment; face identities come from that environment, not inferred arena keys.
Fixture adapters own explicit notification state with no registered observers.
No substitute production context or notification store is created in the defaults.

The p3 defaults for wire creation, stitch/orient, whole-B-rep transform and face
composites now invoke the available p5/face-p3 ports. Intersector deletion requires
a context-only scoped intersector and caller-resolved handles rather than an Unported marker
for the now-partial p2 unit. The empty fixture explicitly declines that unsupported
service if unexpectedly reached. Successful production topology still requires
a concrete PrimitiveAccess/SweepAccess adapter.


Local verification of this integration: formatting, normal/native Clippy,
1,440 kernel tests, wasm32 and the p3 native FFI guard regression pass. All six
affected native gates have zero failures and max_rel_err 0: p3 293/284 pass/8
deferred/1 rejected; p1 773/689/75/9; topology sweep 568/554/14/0;
API B-rep 573/306/267/0; API surfaces 882/831/51/0; B-rep p7 316/197/119/0.
Sensitivity detects all 284 comparable p3 cases. Fuzz 300: 78 executed, 163
rejected, 59 malformed; zero differences, crashes, panics or timeouts. The
full-gate/CI evidence for the tested PR head is recorded in the PR discussion.


### Further PR #233 audit: virtual NURB acquisition in swung construction

`SmBSplineSurfaceNLib.cpp:6224-6225` calls `GetOrCreateGwNurbPointer` on the
original section and trajectory objects, in that order. A bounded `SmLine`
constructed without its NURB is valid there: the virtual `MakeNurb` fills the
cache. Copying its empty B-spline base first loses that virtual constructor and
makes Rust fail before the topology dependency. Swung type checks now retain
the derived objects through scale initialization; the builder then requests the
section NURB and trajectory NURB in upstream order. Only missing analytic caches
are generated on owned copies. The original owner and caller Kernel are forwarded,
ordinary MakeNurb status is ignored as upstream, and Unported remains explicit.
Existing cached NURBs are never regenerated. Curve-sweep path/profile/scale
copy-constructor semantics remain unchanged.

Two direct Rust regressions pass: lazy line profile, trajectory and both together
produce 25 surface samples identical to eagerly built NURBs in each variant;
a cached line whose X differs by two ULPs retains that exact difference in the
constructed surface. The total kernel gate now passes 1,442 tests, alongside
normal/native Clippy, wasm32 and the native p3 FFI guard regression. Updated
native/full-gate/CI evidence is reported for the final PR head. Successful
native topology and production-adapter coverage remain deferred.

### Third PR #233 review: current main, stitch adapters and negative tolerances

Canonical main `95b823ed` is merged, retaining both the primitive findings and
main's new sections, including `heal.stitch_p2`. Both primitive linear-cap and
topology-sweep defaults now invoke the landed `Stitch::do_stitching` instead of
deferring to a partial unit. Their shared callback qualifies each entity with
the actual target B-rep before forwarding selection and map mutations to
`TopologySweep::select_topology_to_keep`. No guessed object keys, copied Kernel
or default selection callback is used. Linear-cap options match pinned
`SmPrimitiveCreation.cpp:1977-1986`; general sweeps keep the native constructor
defaults and supplied manifold flag. Their next dependency is the stitch port's
`SmBrep::Notify` (`topology.brep_p2`), not the production adapter tracked in #255.
That adapter remains necessary for production construction callers.

The op-4 and op-6 FFI guards accept finite negative approximation tolerances,
retaining degree, count, geometry and non-finite checks. Three native fixtures
add one bit-identical empty swung return (no profiles, so tolerance is unused)
and two construction deferrals (skin and nonempty swung).
P3 now has **296 cases: 285 pass, 10 deferred, 1 UB-78 rejection, 0 failed**,
maximum relative error zero and no absolute floors. These comparisons still
do not certify successful stitching or primitive topology construction.
Reversed circular-arc inputs remain unrepresentable by this fixture format.

Formatting (including the included runner/FFI files), plain/native strict
workspace Clippy, all **1,497** kernel tests, wasm32 and the native FFI guard
regression pass. Four Rust regressions cover the two callback behaviors and
both adapter defaults; the latter verify flag restoration on notification
deferral. All seven affected native gates pass their comparable cases with
maximum relative error zero. Sensitivity detects **285/285** comparable p3
cases (10 deferred, 1 rejected). Fuzz 300 executes **85**, rejects **161** and
classifies **54** as malformed, with **0** differences, crashes, panics or
timeouts. Full-gate and actual-head CI results are recorded in the PR discussion.

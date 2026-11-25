# offset.offset_geometry_creation (`SmOffsetGeometryCreation.cpp`, PR #156, 2026-10-16; partial, no disagreements)

**Shape of the port.** `SmOffsetGeometryCreation` / `SmLocalOperationGeometryCreation`
are generic over caller handles (`O: ClassObject + SolutionObject`), as the proto
topology and the topology intersector: the two B-reps are reached through the
`OffsetBrep<O>` trait (each method names the ported topology function it stands for)
and the executive's stored data (`GetInset`, `GetExtendConvexEdges`, `GetShellFaces`,
`GetOffsetEdgeuse`, `GetCreateExtendedSurface`; the tangency tolerance is the
creation's own) through `OffsetExecutive<O>`, which its owner implements over the ported
`offset::offset_executive::p1::OffsetExecutive` (first a co-port of that unit's
accessors, before #175 merged). `SmSSIData` (no owning
unit) is `SsiData<O>`. The curves and surfaces the unit makes go to the owner's stores.

**Difftest.** The shim builds the original and the "offset" B-rep from real `.smb`
files (or a wire edge / single face / the `prog_test` network), makes a real
`SmOffsetExecutive` whose `m_vEUToEU` relation is filled with the given edge-use pairs
through pointer-to-member access (`MapEdgeuses` belongs to `offset.offset_executive_p1`),
and runs scripts that first shape the fixture (store curves, `SmEdge::SetCurve` of an
offset edge with a copy of a store curve, `SetUVTrimCurve`, marks) and then call the
unit's functions. Crafted rails (three circles in one plane, parallel lines, rails
closer than ten tolerances, mismatched intervals) reach the torus, self-intersecting
torus, cylinder, "raise the tolerances" and "non-synchronised" branches of
`EdgeuseOffset`; three rational arcs through an offset corner reach the sphere of
`VertexOffset`.

**Upstream hangs (HANG-17; HANG-16).** `SmOffsetExecutive::GetCreateExtendedSurface`
extends the face's surface by five offset distances (`SmBSplineSurface::CreateExtendedSurface`,
`SM_CT_G1R`) unless its `m_vSurfExtSurf` cache already holds the surface, in which case it
returns the cached one without extending (`SmOffsetExecutive.cpp:2123`). Two things make the
extension not return. (1) With a negative offset distance the per-side loop `while (dExtDist <
dDist/2.0) { dExtDist *= 2.0; ... }` holds from the start and keeps holding while the extensions
succeed, so the distance doubles without bound on any face, planes included (three generated
cases at -0.25 ran past 60 s, gdb backtraces in `ST_surxsdr` under `CreateExtendedSurface`); this
is HANG-16 ("Direct negative-distance surface extension moves away from its loop's exit", from
PR #175), which the HANG-17 entry in `plan/upstream-issues.md` references. Real callers build the
geometry creation with `fabs(dOffsetDistance)`, so a negative creator distance (as the generator
uses) is outside the executive's internal contract. (2) HANG-17: with a
positive distance `ST_surxsdr` splits a large extension and recurses twice per level to depth
10, so a surface whose iso-curves are short relative to the distance runs through up to `2^11`
extensions (28 minutes, killed). The shim seeds the cache before every op with every face
surface of both B-reps and every store surface mapped to itself, and the runner seeds the ported
executive's cache (`OffsetExecutive::insert_extended_surface`) the same way: neither side ever
extends, `GetCreateExtendedSurface` returns the face's own surface on both, and every op is
compared past that call with no guard status and no over-rejection (an earlier version refused these calls with a `-5` status on both sides,
which turned 21 would-be deferrals into constant-status passes and refused, through a
closed-surface proxy, a third of the probe faces that complete; the pre-review removed it).

**Local creation.** `SmLocalOperationGeometryCreation(dist, approx, faces, surfaces)`
copies its two lists at construction and its `FaceuseOffset` returns the surface listed
for the face (`SM_ERR` for an unlisted face). Its `m_bMakeAnalytics` is `FALSE` from the
constructor and neither side calls `SetMakeAnalytics` on it, so the analytic branch (which
reparameterises, copies and deletes the listed surface) is not exercised. The listed
surfaces must be store surfaces: three generated cases that listed a face's own surface
segfaulted (the analytic copy would delete the face's surface; without it the shim freed
the store's copy of the pointer at teardown), so the FFI and the runner refuse such pairs.
The store is filled by the script's ops, so the header's surface handles are resolved
again before each local op on both sides (the local creation is rebuilt then; a listed
face whose surface is not there yet is refused with `-2`, as upstream dereferences it);
the fixed cases `local_offset_*` cover the listed, unlisted and not-yet-added surfaces.

**Fixes the first run forced.** The 12 disagreements of the first complete run were:
`values.state.len` differences from the shim emitting an op's out-parameters after an
error status (the rule above), a library `InvalidInput` reported as the runner's `-2`
where upstream returned status 1007 (the module's own null dereferences now carry the
`UNDEFINED_UPSTREAM` operation, which the runner reports as `-2`; a callee's
`SM_ERR_INVALID_INPUT` stays 1007), UV trim curves of a line or conic kind refused by a
`CurveKind::BSpline`-only match (`SM_CAST_PTR(SmBSplineCurve, ..)` accepts every
subclass and the curve keeps its kind), the ruled-plane call of `EdgeuseOffset` propagating the
`SM_ERR` of a zero `Unitize` instead of taking upstream's `!= SM_SUCCESS` fallback,
`VertexOffset` re-reading a replaced arc inside its plane loop (upstream keeps `pArc`),
and WR-103 below. Three segfaults and three stalls of that run are the local-pair and
hang-guard items above.

**Changes outside the unit.** `nurbs::b_spline_surface::p2::create_sphere_from_arcs` (green,
#13) captured the first arc's centre by value; upstream's `rOrigin` is a reference into the
frame every later `IsArc` overwrites, so its centre check is dead and its sphere centre is
the last arc's (WR-103). A generated `VertexOffset` on circles about two centres exposed it
(the C++ builds the sphere, the port said `SM_ERR`); the port now reproduces the aliasing
(reads the current frame's origin at each use), takes its arcs as `Curve`s so that
`IsArc` dispatches on each arc's kind (`SmCircle::IsArc` is exact; the sphere is revolved
from the arc's NURB part), and that unit gains a fixture case with three equal-radius arcs
about two centres; its difftest stays green.

**Solutions need an object.** `TrimExtendedEdge` intersects each tangent UV curve with
the extended copy of the edge use's UV trim curve, and `EdgeuseOffset` ray-fires the
self-intersecting torus arc; both curves are temporaries upstream but still objects, and
the solver's duplicate comparison (`SmSolution::GetParameterInterval`) dereferences every
solution's object for its natural interval. The port first gave those solutions no
object (`None`), and `Solution::parameter_interval` then reported `InvalidInput` (as 1007)
where the C++ found a trim interval (the 4000-seed case gen2353, 1 of the 4 the
pre-review found; the other 3 were deferrals once the hang guards went). The temporaries
now go to the owner's curve store and the solutions name them.

**Pre-review fixes (1156d7fe -> this push).** The reviewer's must-fix list, all applied:
`FaceuseOffset` dispatches `CreateOffsetSurface` on the surface kind (plane, surface of
revolution and B-spline overrides, else the base) and passes `m_dThisApproxTol3d / 10`, so
plane faces are offset and self-intersected instead of deferred under `surface.offset_surface`;
the edge curves `SM_CAST_PTR(SmBSplineCurve, ..)` casts keep their kind (a line's
`IsArc`/`IsLine`/`CalculateBoundingBox`/`Trim`, a circle's `IsArc`, take the override; only
`new SmBSplineCurve(*p)` and the static B-spline routines see the NURB part);
`CreateSphereFromArcs`' `Unported` propagates instead of becoming `SM_ERR`; every
`Trim(SmExtent1d &)` sequence shares one interval so a knot snap reaches the next trim;
the rail evaluations use `SmBSplineCurve::Evaluate`'s `bNonZeroTangents = FALSE`; the hang
guards are gone (above); `GetOffsetEdgeuse` gives an offset edge use only when exactly
one is related (the relation is many-to-many: the generator keeps repeated pairs and the
runner's adapter holds a list per original); the surface outputs carry the NURB of a
plain B-spline surface or a surface of revolution beside the 3x3 sample; and
`ExtendTrimEdges` takes the offset edge use as upstream does (null is undefined only
where upstream dereferences it).

**Guards.** The topology-walking functions dereference edge, curve, vertex use, mate,
CW/CCW neighbour, face use, face and surface pointers without checking, so the shim
refuses them (`-2`) unless every edge use of both B-reps is a loop edge use with all of
those (wire, single-face and network B-reps therefore only exercise the refusal; the
runner mirrors the predicate on the imported B-reps). `VertexOffset` is the exception: it
walks no loops (`IsLaminaVertex` is `GetEdges` + `IsLamina`), so it keeps only its
per-edge checks (a curve on each edge; for a single edge, a face with surface and a
B-spline curve) and runs on wire and network vertices too (maintainer review of #156). `ReplaceEdgeCurves` with a 3D
curve and a null UV curve dereferences the UV curve only when the curves run opposite:
the shim replicates the shape test (end-point matches, then the tangent dot product)
and refuses only then. These refusals are C++ crash guards, not `UNPORTED` deferrals.

**Offset rail mapping, now wired.** `EdgeuseOffset` constructs the actual
`SmOffsetSurface`/`CrvOnSurf` rail and runs its supplied-knot approximation. The old
non-B-spline `curve.crv_on_surf` gate is removed; those cases now reach the genuine
face-owner/tolerance dependency in `topology.face_p2`.

**Remaining deferred (Rust `Unported`, never compared).**
`SmFace::CreateUVTrimCurves` (`topology.face_p3`, inside `UpdateFaceSurfaceDomain`'s
`CalculateUVDomainFromUVTrimCurves`, wired since `topology.face_p1` merged); the
trimmed surface cache of a face's own surface (its construction first reaches
`SmTol::GetZoneTol3d(const SmObject*)`, labelled `surface.surface_cache_p2` on main since
`topology.brep_p1` merged (`topology.brep_p1` before), the label all 78 print; then the
face's B-rep editing flag and `SmFace::CreateUVTrimCurves`): the seeded
`GetCreateExtendedSurface` hands `MateEdges` / `TrimExtendedEdge` the face's surface, and the
surface intersections build that cache; `SmSurface::DropCurve` and the drop-curve tracer
(`intersect.surface_drop_lib_p3`, `intersect.surface_drop_curve`) inside `CreateUVTrimCurve`;
the face handle of a surface's cache (`topology.face_p2`); `SmBrep::Notify` and `MateLaminaEdgeToSurface`
(`topology.brep_p2`: the notification after an in-place edit, the inset-on-shell-face
branch of `EdgeuseOffset`); `SmCircularCrossSectionFSG` (`blend.fillet_standard_solver_p1`,
the general cap of `EdgeuseOffset`); `SmSphere::IsNurbSurfaceSphere` (`surface.sphere`; the
same two cases stopped at `SmCone::IsNurbSurfaceCone`, `surface.cone_p2`, until #153 merged).
`SmSurface::GlobalCurveIntersect` (`surface.surface_p8`) is a real call, only reached
after the extended surface.

**Offset surfaces.** `SmSurface::CreateOffsetSurface` dispatches through
`Surface::create_offset_surface` (the plane, surface-of-revolution and B-spline
overrides, else the base `SmOffsetSurface`), and `SetAllowExtension(FALSE)` is applied to
an analytic copy that is an offset surface, as both `FaceuseOffset`s do. Wiring
`surface.offset_surface` moved its 31 deferrals: 21 to `curve.crv_on_surf` (the rail
above), 4 to `topology.brep_p2`, 1 to `topology.face_p3`, 1 to
`harness:analytic-geometry-export` (`gen0044`, which reaches `CreateOffsetSurface` on an
inexact face after the rail), and 4 now pass (`gen0268`, `gen0373`, `gen0401`, `gen0638`:
the offset of an exact B-spline face, compared through its 3x3 samples; a review probe that
scales the port's offset distance by 1 + 1e-9 fails two of them, so they compare values
that depend on the offset surface's geometry). `SetAllowExtension(FALSE)` is unobserved
by construction: the `SmOffsetSurface` constructor already sets it, the copy keeps it,
and the exported samples lie inside the domain, where the extension never applies (the
review flipped both sites to `TRUE` and the unit's results did not change); its placement
is verified by reading only.

**Not compared by construction.** The script's store curves are plain
`SmBSplineCurve`s, so a line or conic registered as a 3D or UV curve is never
compared (the port slices such a curve to its B-spline part exactly as the
`new SmBSplineCurve(*p)` copies upstream do; the edge curves of the file B-reps, which
include `SmLine`s, are compared). An offset surface's export is its kind code and 3x3
sample (no NURB block), on both sides.

**Analytic faces.** The `.smb` importer keeps a plane exactly but brings every other
`SmBSplineSurface` subclass (cylinder, cone, sphere, torus, revolution, ...) in as its
B-spline part; `import_with_types` also returns each face's upstream surface type code.
Where upstream dispatches a subclass-overridden virtual on such a face's own surface,
the runner defers under `harness:analytic-geometry-export` instead of letting the port
run the B-spline override: `CreateOffsetSurface` in the non-local `FaceuseOffset` (60
cases) and `IsClosed` in `SmEdge::IsSeam` on an `SmSphere` face (3 cases; only
`SmSphere` overrides `IsClosed`, so the other inexact faces are compared). Exporting the
analytic kinds exactly would turn these 63 back into compares. No op creates faces, so a
face handle always names an imported face; a non-local `FaceuseOffset` on a face outside
the import table is refused as a harness error rather than assumed exact.

**Hang guards (`-6`, the script stops; the Rust defers).** `difftest --fuzz 2000` found
two upstream hangs (`plan/upstream-issues.md`): HANG-26, the self-intersection of the
offset of an `SmSurfOfRevolution` face (slow at ordinary distances, erratic at large
ones): the shim does not run a non-local `FaceuseOffset` on a face that is neither an
`SmPlane` nor a plain `SmBSplineSurface`, which the Rust already defers under
`harness:analytic-geometry-export`, so nothing comparable is lost; HANG-27,
`SmCrvOnSurf::ApproximateCurve` of the offset-surface rail, which grows with the offset
distance: an `EdgeuseOffset` with `|distance| > 1e6` stops with `-6` and the Rust defers
under `upstream:HANG-27` ("EdgeuseOffset beyond the HANG-27 distance bound"; the guard
sits at op entry, so it also stops calls that would not build a rail). The bound is chosen for the fuzz time budget (20 s per side),
not the termination boundary (about 1e250); the generator's distances (0.001 to 100)
are far below it. Non-finite numbers (and magnitudes above 1e300) are malformed
input to the FFI, refused before either side runs. `gen0577`
takes 18 s in C++ (0.03 s in Rust) and is kept: it finishes inside the budget and is not
a hang.

**Performance: `gen0353` and PR #224.** One `TrimExtendedEdge` of `gen0353` took 27.2 s
in Rust against 0.03 s in C++, which failed the fuzz time budget: the port rebuilt the
curve cache for every `GlobalPointSolve` inside `GlobalCoincidenceChecker`'s split
recursion, where upstream keeps it on the curve. PR #224 (`perf/curve-cache-reuse`,
merged into this branch, which depends on it) keeps one cache per curve for the
recursion; reports are byte-identical and `gen0353` now runs in under a second.

**Wiring round: `offset.offset_executive_p1` (#175), `topology.edgeuse_p3` (#117), #147.**
`GetCreateExtendedSurface` goes through `OffsetExecutive<O>::get_create_extended_surface`
(the executive is now `&mut`); the runner wraps #175's `OffsetExecutive` behind the trait
(its typed ids each name one B-rep, the script's handles span both, so a handle is folded
into an id and back) and seeds its cache as the shim seeds `m_vSurfExtSurf`.
`NormalizedEvaluate` and `CreateUVTrimCurve` go through two `OffsetBrep` methods over
`topology::edgeuse::p3` with upstream's arguments at every call site (3D evaluation, no
derivative, null optional outputs); #117's `&mut Brep` signatures make the seam,
tangent-sector and convex-sector methods and `HaveTangentGaps` take the B-rep mutably.
The first complete run passed 445, failed 5 (gen0153, gen0230, gen0266, gen0550, gen0583,
all on `Endcap.smb`): with the cache seeded, `GetCreateExtendedSurface` returns the face's
own surface, so C++ `MateEdges`' `GlobalSurfaceIntersect` builds that face's trimmed surface
cache, whose `ImplantFace` runs `SmFace::CreateUVTrimCurves` (UV trim curves attached, the
offset edge's tolerance raised from 0.004051 to 0.007557) and finds the intersection curve;
the port's `make_trim_srf_cache` built its cache on `surface.clone()`, which drops the owner
(the copy constructor), so the cache ran as a standalone surface and found no curve. PR
#147 builds the caches on `clone_for_evaluation()`, which keeps the owner; merged here, the
five defer at `SmTol::GetZoneTol3d` and 17 cases that had passed through the standalone path
defer too (97 cases newly pass). Main's other units barely reach that path: on main
cd483d51 + #147 (b396cdd3) + this PR every regenerated `green` report has main's per-case
outcomes except `nurbs.b_spline_surface_p2`'s WR-103 case (this PR) and
`topology.brep_p1` (merged after the first comparison), where #147's change moves one
pass (`Endcap_parallel_proj_1`) and 36 deferrals to the same `GetZoneTol3d` stub. #175's own
`UNPORTED(offset.offset_geometry_creation)` `EdgeuseOffset` / `VertexOffset` calls stay
stubbed: they need an `OffsetBrep` over the executive's original and offset B-reps and
#175's code after `EdgeuseOffset` (cpp:2956-3071), which reaches `topology.brep_p7/p8`; a
follow-up once this PR merges.

**Results.** 874 cases (5 upstream-derived fixtures, 69 cases built in code, 800 generated
two-B-rep scripts): 565 pass bit for bit (`max_rel_err 0`), 0 fail, 309 deferred, 0 cases
where both sides reject the script (441 / 433 before main e5f3e441, where #149 wired
`topology.brep_p2`'s stubs: the report equals main's #149 re-run case for case plus this
branch's 13 cases). 560 passes compare an upstream result (one of them, `network_vertex_three_edges`, an
error status from `VertexOffset`'s 3-edge branch); 2 (`gen0264`, `gen0685`) are guard
agreements: the shim refuses the only op as an upstream null dereference, so no C++ runs
and both sides compare `[0, op, -2]` (they count under `with_values=565 no_values=0`
because the record has three values; `gen0191` and `gen0255`, passing since #149, are
three-value records too, a single refused or erroring op on both sides, not probed).
Deferred, by the unit that blocks: `harness:analytic-geometry-export` 107, `geom.tol` 82
(`SmTol::GetZoneTol3d(const SmObject*)`, labelled `surface.surface_cache_p2` before main's
relabelling), `topology.face_p4` 43, `curve.crv_on_surf` 27, `intersect.surface_drop_curve`
17, `topology.face_p2` 13, `blend.fillet_standard_solver_p1` 11, `intersect.surface_drop_lib_p3`
7, `surface.sphere` 2 (`topology.brep_p2` 166 before #149).
Success-path coverage (counted at this head): 327 of the 565 passes complete at least one
successful call among the unit's ops (the rest compare matching `-2` refusals or matching
upstream error statuses). Per op, the passing cases that contain a success / a refusal / an
upstream error (a case can count in several): `FaceuseOffset` 47 / 19 / 0, `VertexOffset`
77 / 20 / 42, `ReplaceEdgeCurves` 99 / 12 / 0, `TrimExtendedEdge` 62 / 55 / 0,
`ExtendTrimEdges` 6 / 44 / 12, `HaveTangentGaps` 74 / 39 / 0, `EdgeuseOffset` 53 / 29 / 13,
`LocalFaceuseOffset` 6 / 47 / 68 (`HaveTangentGaps`' points: upstream ignores
`NormalizedEvaluate`'s status and a failed evaluation leaves `sPnt` at `SM_UNDEF_DOUBLE`, which
the port mirrors; an evaluator that wrote part of the point before failing would diverge, a
theoretical case no input reaches); no successful pass: `UpdateFaceSurfaceDomain` (73
refusals), `MateEdges` (24 refusals, 67 upstream errors). (This PR's earliest per-op
counts swapped the `FaceuseOffset` and `ReplaceEdgeCurves` labels.) A 4000-seed sweep
(4074 cases) passes 2650, fails 0, defers 1424, with no C++ crash or stall. Perturb:
`SENSITIVE 565/565`. `--fuzz 2000`'s verdict depends on the draw (see the fuzz paragraph
below: a far-control-point mutation of a `trim_extended_edge` script is a known Rust-only
slow path that can exceed the 20 s budget). `--fuzz 2000` at the #156 head: `FUZZ-PASS
cases=2000 rejected=224 malformed=627 ran=1149 differs=0 crashes=0 panics=0 timeouts=0`;
one earlier run of the same
tree had a single timeout, `fuzz01488` (a one-field mutation of `gen0577`, whose C++ side
takes about 19 s alone against the 20 s budget, the Rust 0.16 s): load-dependent, as
`gen0577` itself. At the follow-up head bc409b78 (882 cases, so a different draw of bases):
`FUZZ-FAIL cases=2000 rejected=195 malformed=620 ran=1183 differs=0 crashes=0 panics=0
timeouts=2`, both known and neither about this unit's code: `fuzz00434` is `gen0577` with `inset` and
`extend` set, whose C++ `ExtendTrimEdges` on `Mouse.smb` takes 19.5 s alone (23.5 s for the
whole script, 20.7 s without the flags; the Rust defers at `surface.surface_cache_p2`),
the same budget flake as `fuzz01488` (the harness has no per-case fuzz exclusion or
budget knob; `fuzz_cases` draws any base; a draw that zeroes `gen0577`'s header is the same
risk: its C++ side takes 27 s at `dist = 0` and 19 s at `approx = 0` on a quiet pod, 24 s and
31 s in the review); `fuzz01278` is `gen0332` with one control point
of an `add_curve` 2D cubic moved to `y = 1e300`, where the C++ `TrimExtendedEdge` returns
status 0 in under a second and the Rust side runs for minutes in `curve.curve_p8`'s
`global_curve_intersect` (see the `curve.curve_p8` entry at the end of this file). At the
873-case head the first run drew a third timeout, `fuzz00405` (`FUZZ-FAIL cases=2000
rejected=203 malformed=627 ran=1169 differs=0 crashes=0 panics=0 timeouts=1`): `gen0650`
with `approx = -1` and one control point of an `add_curve` 2D cubic at `-1e300`. The FFI
refuses the negative tolerance, but the fuzz runs the Rust side on a header the FFI refused
(it skips Rust only after a fixture-constructor rejection), and the Rust side took 21 s. The
runner now mirrors the FFI's refusal (`approx_negative` on both sides, as for the B-rep
guards), so neither side runs such a draw. The slowness itself is not the negative
tolerance: the same script at any accepted tolerance (`0`, `1e-300`, `1e-12`, `1e-6`, `1e-4`,
`1e-2`; the reviewer's timings) takes 36 to 41 s in Rust against about 5 s in C++ and passes
with equal values, and with the control point back in range it returns at once on both
sides. It is the far control point in `make_curve_cache`'s tessellation, the same Rust-only
slow path as `fuzz01278` and `curve.curve_p5`'s `fuzz00504`: see the `curve.curve_p8` entry
at the end of this file, which holds the shared explanation; no guard in this unit. The
final head's verdict therefore depends on the draw, and this head's draw shows it:
`FUZZ-FAIL cases=2000 rejected=196 malformed=628 ran=1175 differs=1 crashes=0 panics=0
timeouts=1`. The timeout, `fuzz01915`, is that family's third draw: `gen0330` (`torus.smb`,
`dist 100`, `approx 1e-4`, an accepted header) with `/ops/0/curve/cpts/2/1 = 1e+300` into
`trim_extended_edge`; the Rust side takes 29 s for the script (gdb at 12 s: the same
`make_curve_cache` tessellation stack as `fuzz00405`) and then defers at the torus face's
`FaceuseOffset` as its base does, while the C++ side returns within the budget. The
`differs`, a note and not a failure (fuzz inputs may lie outside the generator's domain), is
`fuzz01241`: `local_offset_late_surface` with the added plane's normal at `(0, 1e300, 1)`,
whose squared length overflows; every value agrees except `state[15]`, exactly 0 in C++
and `-1.52e-55` in Rust, a rounding residue at overflowed scale (the generator's normals are
unit scale); not investigated further.
Valgrind: 54 sampled shim programs before the wiring round, then 64 on the new paths (54
newly passing cases, up to ten per op family plus eight built-in ones, and ten new
deferrals including the five above), then the 13 cases the review follow-up changes: clean.
**Review follow-up (maintainer review of #156).** The shim and runner no longer refuse
`VertexOffset` on B-reps whose edge uses are not all loop edge uses (see Guards): 8
generated cases now compare its result on wire / network / face B-reps, and four new
built-in cases run it on the wire network's and a single wire's vertices (the one with
three of the network's edges compares the 3-edge branch's error status). `HaveTangentGaps`
accepts `n = 0` (upstream evaluates once at `0 / 0`; `cone_have_tangent_gaps_no_samples`).
Second round (re-reviews of 9f50dfcc and bc409b78): the run header no longer refuses
`dist == 0` or `approx == 0` (only a negative tolerance is refused). Every generated script
was run at each zero (a throwaway probe, not committed): all 800 return on both sides and
agree bit for bit (406 compared at either value, the rest defer where their base defers), no
hang; the C++ side of `gen0577` is the only slow one (27 s at `dist = 0`, 19 s at
`approx = 0` on a quiet pod; 24 s and 31 s in the review). With the `Getters` op's echo of
the header left out, a zero distance changes 67 compared results (and 74 deferred ones),
every one a script with a `FaceuseOffset` (the offset surface at distance 0) or a
`VertexOffset` (the radius-0 arc, status 1007); `EdgeuseOffset` reads the distance on its
rail path only (`SmCrvOnSurf` over a non-B-spline surface, deferred), and `ReplaceEdgeCurves`,
`TrimExtendedEdge` and the plain `EdgeuseOffset` path never read it (the reviewer's reading
of the upstream code). A zero tolerance changes 2 compared results (and 11 deferred), both
through `VertexOffset`. The 8 `*_zero_distance` / `*_zero_approx` cases (`zero_header_cases`,
bases named explicitly) keep bases whose compared output changes at zero: at `dist = 0`
`cone_vertex_offset` (the radius-0 arc, status 1007), `gen0700` (one `FaceuseOffset` of a
plane, `cone.smb`), `gen0107` (one `VertexOffset`), `gen0012` (`UpdateFaceDomain`,
`FaceuseOffset` of a plane, `TrimExtendedEdge`, `MateEdges`; 515 values) and `gen0638` (one
`FaceuseOffset` of `ellips_ts2.smb`'s B-spline face, the one non-planar offset surface among
them: a plane's offset grid only shifts by the distance); at `approx = 0`
`cone_trim_extended_edge` (`TrimExtendedEdge`'s `d3DTol`, ten times the tolerance),
`gen0775` (one `VertexOffset`) and `gen0651` (`EdgeuseOffset` then `VertexOffset`); all 8
pass. (The previous draft had 16 such cases on one built-in per op
family; only the two built-ins kept here reached the zero, the others equalled their bases
or deferred where their bases defer.) HANG-9 (`CreateExtendedSurface` at distance 0) is not reached: the
executive's extended-surface cache is seeded, and the unit's own calls pass an extension
domain, not the offset distance.
`nurbs.b_spline_surface_p2` (touched): 764 cases, 759 pass, 5 deferred; its
`three_arcs_about_two_centres` case (arcs about two centres, the first two sharing their
start point) compares 94 values and fails with the first arc's centre captured by value, so
it pins WR-103. All green/partial units: `green-par: 234 units, failed=0` on main e5f3e441 +
this PR (#147 merged).

# primitive.primitive_creation_p1 (PR #213, 2026-10-17; partial)

Default native differential run: **689 cases, 667 pass, 0 fail, 17 deferred,
5 rejected**, maximum relative error **0**, no absolute floors. The passing set
contains 479 cases comparing 21,076 floats and 188 integer-record cases. None of
the passes is an empty output or a pair of hard-coded error results.

The shim includes the unmodified pinned `SmPrimitiveCreation.cpp`, with the
primitive class and its external analytic-sweep helper renamed. File-local
`sm_FindPivotPoint`, `ExtendPathCurves` and `PositionProfileCurves` therefore run
upstream code, including its default arguments and retained local buffers.

Observed comparisons in `fixtures/primitive.primitive_creation_p1/`:

| Path | Witness and compared effect |
|------|-----------------------------|
| Constructor, `InitEntityMaps`, `CopyEntityMaps` | `maps_clear*_copy*` and generated records compare tag, region selection, all four map counts and every lookup in both primitive and initially seeded sweep maps. This detects reversed copy direction and retained target entries. |
| Pivot branches | `pivot_start`, `pivot_end`, `pivot_fallback`, `pivot_squared_below`, `pivot_squared_above`, `pivot_negative` compare the returned point. The threshold fixtures distinguish squared distance compared directly with the supplied tolerance from a squared tolerance. |
| Extension decisions | `extend_angle_*` straddle the two-degree angular test, including antiparallel lines; closed/periodic single-curve cases exercise end extension versus skip. Natural intervals and five position/tangent samples of every output curve expose extension and order. Circular joins and scales `1e-6`/`1e6` are included. |
| Profile placement | Polygon, circular, tilted, curved-path, tied-tangent, vertical/skew/negative-X path, empty and pre-existing-output fixtures compare every output interval and five position/tangent samples. Existing outputs are transformed too; only the input profiles determine the bounding-box centre. |
| Defined error/state paths | Invalid cylindrical-box dimensions/ranges, degenerate cones and tori compare the actual returned status, edit mode, map count and topology counts. Empty `Boolean2D` cases cover all five operations and an already-populated result slot. |
| Analytic rotation rejection | Axis-coincident, too-short and axis-crossing lines, a quarter arc and a semicircle on the wrong axis compare actual statuses and unchanged face/edit state. |

The 600 fixed-seed generated cases exercise maps and both path helpers plus pivot
selection. Generation is independent of the native return value. Primitive inputs
also re-express `TestSmCreatePlane` and `TestSmCreatePlanarCircle` from
`tests/SM_API_test/src/TestSmPrimitives.cpp`.

**Deferred coverage is not parity.** `topology.brep_p3` blocks five static B-rep
constructors, including the zero-radius circle; `topology.brep_p7` blocks the first
trimmed cylindrical-box face; `topology.brep_p8` blocks nine face/planar-face edits;
`surface.sphere` blocks two canonical sphere calls. Native constructors run on
these inputs, but no primitive topology, sphere-patch geometry, cap, boolean
insertion/classification or downstream topology mutation is certified here.
The corresponding Rust control flow is present behind dependency stubs.

**UB-57 preflight.** The full-span cylindrical-box fixture fails the native
`smgu_CircleFrom3Points` solve. `SmCircle` ignores that status and reads the
unwritten radius in release builds. The shim probes the same bottom/top, inner/outer
three-point circles, only after the primitive's validation would succeed. A failed
solve is rejected before the undefined constructor; it is excluded from parity and
perturbation. The guard does not use a blanket near-360-degree exclusion. Layout
errors and native exceptions remain harness failures, not construction rejections.

**Integration.** This branch merges dependency PR #145 for B-rep edit state and
ordered face collections. The old primitive-constructor markers in `sweep.api_brep`
and `sweep.api_surfaces` now point at `PrimitiveCreation::new`; actual construction
there still awaits the preceding `topology.brep_p3` constructor and the owned p2/p3
sweep overloads. Their differential reports are re-run. `EntityMaps` is the shared
four-map payload for the future topology sweep, recorded in its plan notes.

**Representation.** Marks are caller-owned per arena, as in the topology units.
Both operands of the planar boolean use the supplied Kernel; the mark lock and
context/edit flags restore on error. On success the result takes ownership of the
primary arena and the consumed input slots become empty Rust arenas. Empty-input
returns retain the previous result slot. Concrete cone/torus geometry is retained
through explicit pending face-construction adapters rather than converted to a
B-spline stand-in.


**Extreme finite input regressions.** `overflow.json` retains all eight differences
from the first 1,000-case fuzz run. Four profile placements now preserve the X
axis already written/unitized by `MakeUnitOrthoVectors` before a later failure;
its ignored status does not restore the old X axis. Their coordinates and
derivatives agree in the differential comparison, including the `1e300` origins. Four extension
cases reach a native G1 fallback that reads a nonexistent third control point
of a two-point line after `N_VectorNormalize` fails. The shim preflights the exact
join/closed/periodic extension sequence, rejects those four before UB and keeps
huge unextended lines in the passing set. This issue is recorded by its function
name in `plan/upstream-issues.md` (no new global UB number allocated).

**Owned-function audit.** All 19 callgraph-owned functions are mapped through their
final non-debug return. Constructor/destructor/map members live in `p1.rs` (Rust
ownership drops the maps); the three file-local helpers retain their loop buffers
there. `primitives.rs` contains static rectangle/circle, box/cylindrical box,
cone/sphere/torus, the protected curve linear sweep, pole-free sphere assembly,
elliptic-ended cone and analytic rotational sweep. The planar boolean's validation
and coverage passes are in `primitives.rs`; `boolean.rs` contains intersection,
radial-sector relation, classification, copying-before-deletion and final
stitch/removal control. Face relations reuse the existing `boolean::topology_intersector::MateMap`
rather than duplicating its storage/conflict rules. The unavailable call adapters
retain the supplied Kernel, explicit intersection tolerances and geometry/domain
parameters. Downstream continuation exists after each adapter,
including static circle's edit reset and linear-sweep caps/stitch after cylinder
or extrusion faces; those paths remain explicitly uncertified until dependencies
are wired and compared.


**Final local checks.** Workspace fmt and Clippy pass; generated FFI/runner files
also pass explicit rustfmt and native `cpp` Clippy. All 1,292 kernel tests, wasm32
check and four native FFI tests pass. Perturbation detects all 667 compared cases
(17 deferred, five rejected, no unperturbed case). The repeat 1,000-case FFI fuzz
run reports 580 rejected layouts/constructions, 279 malformed records and 141
executed comparisons: zero differences, crashes, panics or timeouts.


Affected-unit reruns also pass their non-deferred gates with maximum relative
error zero: `sweep.api_brep` 573 cases / 306 pass / 267 deferred;
`sweep.api_surfaces` 882 / 831 / 51; `topology.brep_p1` 2,343 / 1,717 / 626.
The existing B-rep report retains its 60 status-only passing records; these are
not added to the primitive unit's geometry-coverage claims.


The full required `--green --no-report` run completed successfully on the
integrated branch: **202 units, 108 passed, 94 partial, zero failed**. Partial
units retain their dependency deferrals; this result does not promote them to
green or certify their deferred mutations.


### PR #213 integration with main d045b293 (2026-10-18)

Merged the native shell and closure-split ports with the PR's ordered B-rep
collections. `GetShells` now uses the region lists, attribute filtering and the
native context mark lock; shell wire-edge lookup uses the explicit B-rep edge
list. Primitive copying/planarity and cross-arena radial-sector calls retain the
caller Kernel and follow the new APIs. The canonical sphere constructor is now
wired to the concrete `Sphere` type, preserving its natural domain and deferring
only the unavailable `CreateFaceFromSurface` call (`topology.brep_p8`).

The primitive result remains 689 cases: 667 pass, 0 fail, 17 deferred and 5 rejected,
max_rel_err 0. Deferrals are now topology.brep_p3 (5), topology.brep_p7 (1) and
topology.brep_p8 (11). This does not certify primitive topology or the deferred
sphere face construction. Refreshed integration counts and dependency breakdowns
for the affected units are in `plan/modules.toml` and `reports/difftest/`.

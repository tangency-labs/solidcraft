<!-- SPDX-License-Identifier: Apache-2.0 -->

# Layers: one job, one place

This page is the rule set for solidcore's crates and for the applications built on them. Every job lives in exactly one layer. **Two implementations of one job are a bug.** A
layer that needs another layer's job calls that layer; it never carries its own copy.

## 1. The layers

| Layer | Crate | Its job | Examples | Never in it |
|---|---|---|---|---|
| Kernel | `solidcore` | Every geometric computation: build, evaluate, measure, intersect, fit | **build:** skin, sweep, blend, fillet, offset, booleans, trim, sew, curves and surfaces from control points and knots; **evaluate:** points, derivatives, normals, curvature, frames, normal sections, projection; **measure:** distance, angle, area, volume, mass properties, continuity across an edge (gap, normal angle, curvature difference); **intersect:** curve/surface, surface/surface, ray/body; **fit:** knot insertion and removal, degree elevation, Greville, interpolation, approximation, constrained fits (an edge row matched to a target at G0/G1/G2); **check** | anything app-specific: defaults, command shapes, UI |
| Record | `solidcore-record` | Geometry as data | `Brep`, `Surface`, `Curve`; serde; structural validation (counts, indices, finite numbers); the neutral STEP model and its parser | any maths |
| Ops | `solidcore-ops` | The modelling API both apps call: record ↔ kernel conversion, and every workflow operation as a sequence of kernel calls | the align command (picks the edges and the order, calls the kernel's constrained fit); the edge-continuity check (picks sample parameters and tolerances, calls the kernel's continuity measure, compares and reports); CV-curve editing (defaults, calls the kernel's constructor); comb values (calls curvature and normal, scales for display); importing a file (parses into the record, then calls the kernel to build, sew and heal) | any geometric quantity computed itself (see below) |
| Tess | `solidcore-tess` | Geometry made drawable | display meshes; isoparm and curve polylines | modelling or measurement decisions |
| View | `tlabs-view` | Drawing on the GPU | shading, edges, lines; the camera's ray for a pick (the hit itself is the kernel's ray/body intersection, called through ops) | geometry maths on the model |
| Kit | `tlabs-kit` | Interface components | ribbon, panels, history bar, preferences, menus | app state, geometry |
| Link | `tlabs-link` | Talking between apps | publish, withdraw, revisions | app types |
| Apps | the applications built on solidcore (SolidCraft and others) | State, and mapping input to commands | document, selection, command ids, settings, construction history and replay | any geometry computation |

### The line between kernel and ops

**The kernel computes; ops decides and sequences.** A geometric quantity is anything computed from coordinates: a point, a vector, a length, an angle, a curvature, a curve or surface, or a solve. It is computed in the kernel and nowhere else.

Ops may:
- choose: which entities, which parameters and sample positions, which defaults;
- sequence kernel calls and pass their results on;
- convert between the record and the kernel;
- compare numbers the kernel returned with tolerances, and report;
- scale a returned number for display.

Ops may not compute a geometric quantity itself, however simple. No dot or cross product, no normalising, no square root or trigonometry on coordinates, no linear solve, no derivative formula. When a workflow needs one the kernel lacks, it becomes a kernel primitive with its own tests, and ops calls it.

The two tests:
- **Kernel:** it computes geometry, and any client could use it.
- **Ops:** it is a recipe of kernel calls that serves a modelling workflow, with no geometric quantity of its own.

A job in both rows is a mistake: a primitive has one name, in the kernel. An ops operation built on it is named for its workflow (the align command, not "align"), never as a second version of the primitive.

### Primitives the kernel gains from the move

These are computed today in an app or in ops-shaped code. Each becomes a kernel primitive before its caller moves:
- **A constrained fit:** an edge row of a surface matched to a target edge at G0, G1 or G2. Its internals: the solve at the Greville parameters, rows moved along the target normal, the clamped end-derivative formulas.
- **A continuity measure across an edge** at given parameters: the gap, the normal angle and the curvature difference.
- **Ray/body intersection** for picking (SolidCraft's pickers).
- **Classification of a face's surface for offsetting, distances, and sheet-metal bounds** (SolidCraft's `crates/kernel`), each where it is a computation and not a recipe.
- **Evaluation and exact NURBS forms of analytic surfaces** (SolidCraft's record, `solidcraft_geom::brep::{eval, form}`).

## 2. Owner decisions (2026-11-02)

- **Meshing is solidcore's.** There is one mesher, `solidcore-tess`, and it is SolidCraft's tess. That tess meshes every face in its own (u, v): structured grids, Coons blends and cut grids first, with constrained Delaunay plus refinement as the general fallback. The `nurbs-grid-mesh` crate is a second (u, v) grid mesher, so it is a duplicate. Its ideas (grid lines sized from the second-derivative control points, refinement checked at edge and diagonal midpoints, row-at-a-time evaluation, the `deviation` measure) are taken into `solidcore-tess` only where they measure better than its own grid paths on the same faces. Then the crate is deleted. A comparison against SMLib's tessellator doesn't decide this, because SMLib's is constrained Delaunay only.
- **Importers are solidcore's.** There is one STEP implementation. Its plain-data model is in `solidcore-record`, and building bodies from it is in `solidcore-ops`. SolidCraft's `crates/step` and solidcore's `step-*` crates merge into it; the others are deleted. Every other importer (IGES, STL and so on) follows the same split.
- **Surface/surface intersection is solidcore's.** There is one implementation, in the kernel. solidcore's `intersect` module and the `ssi` crate (with its `solidcore-ssi` bridge) merge into it; the other is deleted.

Which copy survives in each merge is decided per job, from measurements: the rig and the SSI corpus. The decision is recorded on the job's tracking issue.

## 3. Dependencies

Allowed edges between workspace crates, enforced by `cargo xtask layers` in both workspaces:

```
solidcore-record   -> (no geometry crate)
solidcore          -> (none of the others)
solidcore-ops      -> solidcore, solidcore-record
solidcore-tess     -> solidcore, solidcore-record
tlabs-view         -> solidcore-tess (the meshes and polylines it draws)
tlabs-kit          -> (none)
tlabs-link         -> (none of the geometry crates; it carries bytes and revisions)
apps               -> solidcore-ops, solidcore-record, solidcore-tess, tlabs-*
```

- **Apps never depend on `solidcore` directly.** A modelling call goes through `solidcore-ops`. A number an app needs (a distance, an area, a nearest face) is an ops call.
- **The kernel depends on no other layer.** Record ↔ kernel conversion is ops' job, so the kernel never sees the record.
- **Tess calls the kernel's evaluator.** Its own fast evaluator is allowed only if it is private, display-only, and never reachable from ops or the apps. A display mesh never decides anything a model depends on.

## 4. What each rule is enforced by

| Rule | Check |
|---|---|
| The dependency edges in §3 | `cargo xtask layers` (a table of crates and allowed edges, run in CI in both workspaces) |
| The record and ops compute no geometric quantity | clippy `disallowed-methods` in `solidcore-record` and `solidcore-ops`: `f64::{sqrt, cbrt, hypot, sin, cos, tan, asin, acos, atan, atan2, powi, powf, exp, ln, mul_add}`, and the kernel's vector and matrix methods (`dot`, `cross`, `normalize`, `length`, `transform`, …). Neither crate depends on a maths crate. Comparing a returned number with a tolerance, and scaling it for display, stay allowed |
| Apps compute no geometry | `cargo xtask app-geometry` in each app: a shrink-only allowlist of the remaining sites (as SolidCraft's `xtask/mesh-decisions.txt`). The list never grows; each refactor PR deletes its lines |
| Display meshes decide nothing | SolidCraft's existing `cargo xtask mesh-decisions`, kept until the apps compute no geometry at all |
| One implementation per job | `docs/jobs.toml`: each job names its one crate and module. `cargo xtask jobs` fails when a job's public entry point is defined anywhere else |

## 5. How the refactor proceeds

- **One job per pull request.** Move the job to its layer, switch every caller, and **delete the old copy in the same pull request**. No period with two copies, no flag choosing between them, no compatibility shim.
- **Tests move with the code,** into the layer's own test files. The job's rig cases must give the same results before and after: same counts, same values to their tolerances. The PR shows the run.
- **Shrink the ledgers.** Each PR deletes its lines from `xtask/app-geometry.txt` (and `mesh-decisions.txt` where it applies). A PR that adds a line is refused.
- **Claim the job** on its tracking issue before starting, and name the copy that survives with its measurement.
- **Contributors do the moves.** The maintainers decide which copy survives, review, and merge.

## 6. Order

1. `solidcore-record`, data only: SolidCraft's `solidcraft_geom::brep` without its evaluator and NURBS-form construction, which move into the kernel.
2. `solidcore-tess`: the one mesher.
3. The kernel's new primitives (§1, "Primitives the kernel gains"), each with its tests. Then `solidcore-ops` gets the record ↔ kernel conversion, and the workflows rebuilt on those primitives:
   - SolidCraft's: tangent chains (the walk is ops; the tangency angle at each vertex is the kernel's measure), the offset and shell recipes, and pickers;
   - the surface-modelling workflows: the skin, align and continuity-check commands, CV-curve editing, and combs.
4. The intersection merge in the kernel.
5. The STEP merge across record and ops.
6. The apps' remaining geometry, line by line from `app-geometry.txt`, until the list is empty and `mesh-decisions` can retire.

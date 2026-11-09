# RFC 0003: solidcore consolidation, one implementation per job

| | |
|---|---|
| Status | Draft, open for comment |
| Started | 2026-11-08 |
| Repositories | SolidCraft (`crates/`), solidcore (vendored at `solidcore/`) |
| Authority | `solidcore/docs/layers.md` |
| Discussion | the pull request that adds this file, then the tracking issue of each job |

`layers.md` says each job lives in one layer: the kernel computes, ops decides and sequences, record is data, convert is the one record↔kernel conversion, tess draws, and apps hold state with no geometry math. SolidCraft still carries its own copy of most of those jobs. This RFC lists those copies, orders the work of removing them, and sets the gate each PR must pass. It fits inside the plan already agreed for solidcore-ops: the local-operation sprint, the solidcore#3005 follow-ups, stage 3b, blends last, then the switch commit. It adds no stage of its own.

## Goals

**solidcore was vendored into `solidcore/` on 2026-11-08. This RFC lists which SolidCraft geometry code moves into `solidcore/`, where it lands per `solidcore/docs/layers.md`, and in what order. Nothing already in `solidcore/` moves out.** For this plan, that means:
- **ops is the complete public modelling API, not SolidCraft's API.** It covers every operation a CAD app needs: primitives, booleans, fillets and chamfers, shell and offset, sweeps and lofts, surface construction, sewing and healing, measuring, import and export. Nothing SolidCraft-specific goes in it (no defaults, command ids or UI; layers.md's "never in it" column).
- **Stable and versioned:** semver on ops, tess and record, with `cargo-semver-checks` in CI and a changelog.
- **Documented with examples:** rustdoc on every public item, and a small `examples/` set that runs in CI.
- **Usable outside SolidCraft:** a second consumer exercises the API in CI, so assumptions only SolidCraft makes get caught.

Each consolidation PR below keeps ops free of shapes that only fit SolidCraft: parameters a general caller would pass, results a general caller can read.

## 1. Inventory

Sizes are non-test lines on main `1c8fdde4ba` (tests in brackets). "Callers" counts the source files that use the code, by crate. Measured comparisons come from the mesher bake-off (#13625: run 2026-11-07 on SolidCraft main `11abdba1b` and solidcore `a445bda`; 1,267 bodies, display and export settings) and the STEP bake-off (#13624: run 2026-11-07 on SolidCraft main `dc679cae3` and solidcore main `f2bbc48ab`; 320 files, 219 with a solid).

### 1a. Jobs solidcore already has

| Job | SolidCraft copy (lines) | Layer | solidcore counterpart | Better, by measurement | Callers |
|---|---|---|---|---|---|
| Meshing | `kernel/src/tess/` (6.2k [1.1k]); `geom/src/tess.rs` + `tess/` (1.6k: plan, fast evaluator, partials) | tess | `solidcore-tess` (same planner, 15k with tests) | Tie on triangles (2,471 of 2,530 runs match, none >5% apart). solidcore-tess is more robust (meshes `rod_doubled_edge`, reports empty faces) and 10–60x slower on spline bodies (`eval::collapse`, #13655). Base: SolidCraft's tess; add solidcore-tess's robustness (`rod_doubled_edge`, empty-face reports) | `Body::mesh`/`mesh_plan`: engine 50, kernel 37, ui-egui 30, doc 23, io 3, gpu 2 |
| Mesh simplification | `geom/src/simplify.rs` (0.55k) | tess | `solidcore-tess/src/simplify.rs` | Not measured | gpu 1, ui-egui 1 |
| Second (u, v) grid mesher | none in SolidCraft | tess | `nurbs-grid-mesh` (2.3k) | Loses: T-junctions on ~60% of closed bodies, over the chord on 70 bodies vs 5. Keep `deviation` as a test checker; delete after a 300 x 300-pole test | none |
| STEP read | `crates/step` (7.3k [1.8k]); `kernel/src/step_in` (1.7k [3.1k]) | record (model, parser), ops (build) | `solidcore-record::step`, `step-import`, `step-brep` (11k) | Core opens 218 vs 196 and refuses 0 vs 23, closes 12 bodies SC leaves open. SC is 3x faster (median 0.14 s vs 0.43 s), opens `ap203/rear` (core times out), matched 22 of 23 reference volumes vs 21. Placements (`cylinder_twice`) fixed on `kernel` by #13582. Base: SolidCraft's reader; add core's topology-first reading (0 refused, the 12 bodies it closes) | kernel 1 (import), engine 1 |
| STEP write | `kernel/src/step_out.rs` + `step/src/write*.rs` (1.6k) | record (writer), ops (from a body) | `step-export` (2.3k) | Not measured | kernel |
| Record↔kernel conversion | `smlib/src/core/convert.rs`, `import.rs`, `import/analytic.rs`, `export.rs`, `flat.rs`, `pcurves.rs` (5.9k) | convert | `solidcore-convert` (2.4k) | Parity on the corpus not yet shown (#13666); flat-array labels lose a far-off sphere (#13668) | smlib only |
| Modelling calls (booleans, fillet, chamfer, shell, offset, sweeps, split, trim, merge, history, check) | rest of `crates/smlib` (9.9k [6.7k]) | ops | `solidcore-ops` (12.4k): the same port of the C API's logic | Same port; the switch commit decides | `crates/kernel` only (ADR 0002) |
| Evaluation and exact NURBS forms of analytic curves and surfaces | `geom/src/brep/{eval,form,normal_section,geometry,label,check}.rs` (1.9k); `step/src/eval.rs` (0.2k) | kernel | `solidcore::eval` (form, normal_section, uv, surface, pole, collapse) | Same design; solidcore's adds pole handling, and its `collapse` per point is the tess cost above | kernel 33, geom 8, smlib 7, step 5, engine 4, doc 2 |
| Mass properties, distance, bounds | `kernel/src/{measure,distance,prism_measure}.rs` (1.6k); `smlib/src/measure.rs` + `core/measure.rs` (0.7k); `geom/src/{inertia,ball}.rs` (0.4k) | kernel; ops for the report | `solidcore-ops::measure` (1.0k: mass, area, face area/centroid/plane normal, edge length/middle, classify point) | Exact on both sides; no comparison needed | engine 5, smlib 5, io 2, doc 1 |
| Surface/surface intersection | `smlib/src/ssi.rs` + `core/ssi.rs` (0.5k, wrappers); `kernel/src/intersection.rs` (meeting edges by mesh) | kernel | `solidcore::intersect`, and inside solidcore a second copy: `ssi` + `solidcore-ssi` (13.6k) | solidcore's internal merge (layers §2), measured on `tests/ssi-corpus`. SolidCraft keeps no SSI of its own | kernel 1, smlib 7 |
| Offset and shell recipes | `kernel/src/{offset,offset_backend,surface_offset,polyhedron,shell_groups,surface_kind}.rs` (1.1k); `smlib/src/core/offset.rs` (0.7k) | ops recipe; face classification for offsetting is a kernel primitive | `solidcore-ops::offset`, kernel `offset/` | ops shell refuses a stock the kernel builds (#13652) | kernel |
| Sweeps and lofts | `kernel/src/{smlib_sweeps,sweep_path,point_path,helix,loft_blend,loft_rails}.rs` (4.0k); `smlib/src/core/sweep.rs` + `sweeps.rs` (2.0k) | kernel (stations from laws, rails), ops recipe | `solidcore-ops::sweep` (1.5k), `procedural_sweep` | Stations from laws open on `kernel` (#13688, #13664); lofts with rails and multi-section blends have none (#13663) | kernel, engine |
| Continuity and combs | `kernel/src/{continuity,continuity_order,curvature_comb}.rs` (1.1k); `geom/src/linkage.rs` (0.3k) | kernel measure; ops report and comb scaling | open on `kernel`: #13925 (#13661) | Not measured | engine 6, ui-egui 3, kernel 1 |
| Blends | `kernel/src/{blend,blend_corner,blend_smlib,blend_variable,corner_g2}.rs` (4.0k); `smlib/src/core/blend.rs` (1.4k [1.0k]) | kernel; ops for the command | `solidcore-ops::blend` (1.8k), kernel `blend/` | Waits for the new blend algorithm (generic-blend.md) | kernel |
| Provenance | `kernel/src/{provenance,topo}.rs` (0.9k, by meshes) | kernel history, read through ops | `solidcore-ops::provenance` (0.2k), record `history` | Exact history in solidcore; ops does not yet expose caller tags (#13671) | kernel |

### 1b. Jobs solidcore does not have yet

| Job | SolidCraft code (lines) | Layer | Notes | Callers |
|---|---|---|---|---|
| Mesh-based model decisions | `xtask/mesh-decisions.txt`: 181 functions (doc 96, engine 51, kernel 34). In the kernel crate: `Body::{faces, edges, meets, meets_each, volume_at}`, `measure`, `topo`, `provenance`, blend and offset picks. Their mesh tools: `geom/src/{mesh,bvh,mesh_distance,compare}.rs` (2.4k) | ops queries over kernel measures; the mesh tools stay for mesh bodies (row below) | `Body::meets` has its kernel half merged on `kernel` (#13554, #13660). About 43 lines pick by nearness, 42 measure (volume, gap, clearance), 38 name or trace | everywhere |
| Picking (ray/body) | `ui-egui/src/viewport.rs` `pick_hits` on display meshes, `geom::Mesh::raycast`, `bvh.rs`; engine `face_at_state`, `frames::frame_at`, `sketch_inspect::face_at` | hover may use the display mesh; the pick decision (which face or edge, the point) is a kernel ray/body call through ops | `solidcore::intersect::ray_tracer` exists; no ops entry | ui-egui, engine, doc |
| Tangent chains | four walks: `kernel/src/blend.rs::tangent_chain`, `geom::Mesh::tangent_chain`, `doc/src/eval.rs::{tangent_face_chain, smooth_chain}`, `ui-egui/src/context_menu.rs::tangent_chain`; tangency test `kernel/src/tangent_edge.rs` (0.4k) | ops walk; kernel tangency measure | #13662, no PR | engine 2, geom 2, ui-egui 3 |
| Display-only edges and isoparms | `kernel/src/{smooth_edges,iso}.rs` (1.0k) | tess | regression #13542 lives here | render, ui-egui |
| Surface construction | `geom/src/{fill,fill_iso,fill_general,fill_matched,network,blend}.rs`, `kernel/src/{fill,boundary_network,boundary,bridge,matchsurf,fair}.rs` (12.7k); `geom/src/{bspline,spline_space,tspline/}` + `kernel/src/tspline.rs` (3.6k); `geom/src/{arcfit,catmull_rom}.rs` (0.5k) | kernel primitives, with thin ops commands: layers §1 lists "curves and surfaces from control points", "skin" and "constrained fits (an edge row matched to a target at G0/G1/G2)", which is Match Surface | solidcore has curve and surface fitting (`nurbs/crv_fit`, `srf_fit`), no curve-network fill, no T-splines | kernel, engine, doc |
| Planar builds, sewing, certificates | `kernel/src/{prism,prism_curve,planar_sources,planar_regularize,sew}.rs` (4.6k) | kernel ("build, sew and heal", layers §1) | solidcore has `prism`, `Boolean2D`, stitching; overlap not measured | kernel |
| Local surface commands | `kernel/src/{surfaces,surface_trim,surface_untrim,surface_extend,surface_ruled,splitface,delete_face,emboss,lip,repair}.rs` (8.2k) | ops recipes over kernel primitives; any math they need becomes a kernel primitive first | ops has `split`, `untrim`, `extend`, `stitch` | kernel, engine |
| Hidden lines, sections, projections | `geom/src/hlr/` (4.5k, own ray/surface intersection and inversion), `geom/src/section.rs`; `doc/src/project.rs` silhouettes and sections by mesh, `project3d.rs` | kernel (silhouettes, sections, visibility); tess (the drawable polylines); ops ("make a drawing view") | kernel has `surface_silhouette`; drawings work is on the `drawings` branch | kernel, doc |
| Sheet metal | `doc/src/sheet.rs` (10.3k: flat pattern, bend zones, fold maps), `sheet_convert.rs` | kernel (unfold and fold maths); ops (flange, bend, unfold commands); the app keeps the sheet settings (K-factor, gauge) and passes them in | no counterpart | doc, engine |
| IGES | `kernel/src/{iges_in,iges_in/assembly,iges_out}.rs` (2.7k) | record (model, parser), ops (build): "every other importer (IGES, STL and so on) follows the same split" (layers §2) | no counterpart | kernel |
| Mesh-body tools | `kernel/src/{meshbody,mesh_convert,polybool}.rs` (1.0k); `geom/src/{mesh,bvh,mesh_distance,compare}.rs` (2.4k); mesh formats in `io` (STL, 3MF, glTF, PLY, OBJ) | mesh formats: record (model, parser) and ops (build), as with IGES (layers §2). Mesh-body tools: app, for now: layers.md covers B-reps, and SolidCraft may keep mesh bodies. Their uses on B-rep bodies go with the mesh decisions above | none needed | kernel, engine, io |

Out of scope for this RFC: `crates/sketch` (the 2D constraint solver, 32k), `crates/dynamics`, and the camera and lighting maths in `render` and `gpu`.

**Totals.** About 108k non-test lines in SolidCraft overlap a solidcore job or are geometry outside solidcore: about 56k with a solidcore counterpart (1a), about 52k without (1b), not counting the 147 mesh-decision functions in `doc` and `engine`. The seam allowlist has one line (`crates/smlib -> solidcore`), and no SolidCraft crate calls `solidcore-ops` or `solidcore-tess` yet.

## 2. Order of work

Every PR uses the gates below, plus the gate named in its row.

| Gate | Check |
|---|---|
| Rig | `cargo test -p solidcore-ops --test kernel_cases -- --ignored` and `cargo xtask kernel-cases`: counts unchanged or better, values to their tolerances |
| Fillet | the Fillet rig workflow unchanged or better |
| Ladder | `cargo xtask kernel-cases --ladder --gate` unchanged |
| Known | `tests/KNOWN_FAILING_TESTS` and `kernel_cases/known_failing.json` only shrink |
| Ledgers | `solidcore-seam.txt`, `mesh-decisions.txt`, `app-geometry.txt` only shrink; the PR deletes its own lines |
| Speed | no slower on the job's bake-off corpus, shown as a count (evaluations, subdivisions) with wall time beside it |

### Phase 0: guards (main, all in parallel, start now)

| PR | What | Gate |
|---|---|---|
| 0.1 | `cargo xtask app-geometry` and `xtask/app-geometry.txt` (layers §4): a shrink-only list of functions in `doc`, `engine`, `ui-egui`, `io`, `kernel` and `geom` that compute geometry (vector maths on coordinates, solves, mesh queries), seeded from §1. CI posts per-crate counts on each PR | lists the current sites; fails on a new one |
| 0.2 | `solidcore/docs/jobs.toml` and `cargo xtask jobs`: each job names its one crate and module; the duplicates in §1a are listed with the PR that removes them, and the list only shrinks | fails on a new public entry point outside its job's module |
| 0.3 | The seam check also fails on a new use of `solidcraft-geom`'s evaluator, tess or mesh queries from a crate that does not already use them | ledger counts unchanged |
| 0.4 | solidcore stays self-contained: CI exports `solidcore/` alone (`git archive`) into an empty folder and runs `cargo check --workspace --all-targets --locked` there, and `cargo xtask layers` fails on any `Cargo.toml` under `solidcore/` with a path outside `solidcore/` or a `solidcraft-*` dependency | the export builds alone; a planted path dependency into `crates/` fails |

### Phase 1: jobs both sides have (SolidCraft's code as the base)

| PR | What | Branch | Gate | Parallel |
|---|---|---|---|---|
| 1.1 | SolidCraft's mesher (`kernel/src/tess/`, `geom/src/tess.rs` and `tess/`, with its fast evaluator and the #13266 flat-face fix) moves into `solidcore-tess` as its base | kernel | bake-off speed and triangle counts match SolidCraft's tess on the 1,267-body corpus; the 20 x 10 x 5 box exports 12 triangles; tessellation cases | 1.4, 1.5 |
| 1.2 | Add solidcore-tess's wins to the merged crate: meshes `rod_doubled_edge`, reports empty faces; one planner and one evaluator remain | kernel | the two bodies above; no slower and no more cracks than 1.1 on the corpus | after 1.1 |
| 1.3 | SolidCraft meshes through the merged `solidcore-tess`; delete `kernel/src/tess/`, `geom/src/tess.rs` and `tess/`, `geom/src/simplify.rs` if solidcore's simplify ties | main | display-mesh and export tests; triangle counts within 5% of the bake-off; cracks no more than in the bake-off (3 of 1,176) | after 1.1, 1.2 |
| 1.4 | `nurbs-grid-mesh`: the 300 x 300-pole test, `deviation` kept as a test checker, crate deleted | kernel | the test recorded on #13625 | any |
| 1.5 | STEP reader: SolidCraft's reader (`crates/step`, `kernel/src/step_in`) moves into `solidcore-record` (model, parser) and ops (build) as the base | kernel | `tests/step-conformance`, `tests/step-regressions` unchanged; median 0.14 s held; `rear` opens | 1.1–1.4 |
| 1.6 | Add core's topology-first reading to the merged reader (refuses 0, closes the 12 bodies); SolidCraft reads STEP through it; delete `crates/step` reading and `kernel/src/step_in` | main | `tests/step-conformance`, `tests/step-regressions`, `kernel-cases/import` unchanged or better; bake-off: opens ≥218, refuses 0, ≥22 of 23 volumes | after 1.5 |
| 1.7 | STEP write: SolidCraft's writer is the base; measure both by round trip on the corpus and add `step-export`'s wins | main or kernel by the survivor | round-trip volumes and face counts | after 1.6 |
| 1.8 | Evaluation: one `solidcore::eval`, SolidCraft's `geom::brep::{eval, form, normal_section}` as the base plus solidcore's pole handling; callers move to ops and tess queries; the geom copies are deleted | main | Rig; record checks unchanged | after 1.3 |
| 1.9 | IGES: model and parser into `solidcore-record`, building into ops, the same split as STEP; delete `kernel/src/iges_*` | kernel, then main for the callers | the IGES import tests and `kernel-cases/import` unchanged or better | after 1.6 |

### Phase 2: ops recipes (stage 3b)

Each PR adds the kernel primitive it needs (with tests), the ops recipe, and switches SolidCraft's callers where the seam allows; the SolidCraft copy goes in the same PR, or in the switch commit if only `crates/smlib` calls it.

| PR | Issue | What | Branch |
|---|---|---|---|
| 2.1 | #13659 | ops computes no geometry: the remaining operator sites, `clippy::float_arithmetic` | kernel |
| 2.2 | #13666 | `solidcore-convert` parity on the corpus with the export report and history pieces; fixes #13668 | kernel |
| 2.3 | #13665, #13652 | offset and shell recipes; offset-copy history from the kernel | kernel |
| 2.4 | #13664 (#13688) | sweep stations from laws | kernel |
| 2.5 | #13663 | lofts along rails, multi-section blends | kernel |
| 2.6 | #13662 | the tangent-chain walk; replaces the four walks | kernel, then main for the callers |
| 2.7 | #13661 (#13925) | continuity report and combs | kernel, then main |
| 2.8 | #13671, #13672 | provenance tags and caller-owned sessions | kernel |
| 2.9 | new | surface construction as kernel primitives (fills, curve networks, bridge, Match Surface as the constrained fit, fair, T-splines), each command a thin ops call; one family per PR (§4) | kernel, then main |
| 2.10 | new | planar builds, sewing and the prism certificates as kernel primitives ("build, sew and heal") | kernel, then main |
| 2.11 | new | local surface commands (trim, untrim, extend, ruled, split, delete face, emboss, lip, repair) as ops recipes; any new math is a kernel primitive first, in its own PR | kernel, then main |
| 2.12 | new | sheet metal: fold and unfold maths in the kernel; flange, bend and unfold commands in ops; K-factor and gauge passed in from the document | kernel, then main |
| 2.13 | new | hidden lines, sections, projections: silhouettes, sections and visibility in the kernel, polylines in tess, "make a drawing view" in ops; replaces `geom/src/hlr` and the mesh-based `doc/src/project*.rs` paths. Lands through the `drawings` branch | kernel, then drawings |

2.1 to 2.5, 2.8 and 2.10 to 2.13 run in parallel. 2.6 and 2.7 need 2.1's lint. 2.9 starts once jyviko answers §4.

### Phase 3: exact queries in place of mesh decisions

| PR | What | Branch | Before the switch? |
|---|---|---|---|
| 3.1 | `Body::{faces, edges, volume_at}` through `solidcore-ops::measure` | main | yes (stage 3b) |
| 3.2 | `Body::{meets, meets_each}`, `shell_groups::meshes_meet` through the exact contact test (#13660) | main | yes |
| 3.3 | Kernel-crate picks by mesh (blend, offset, surface trim, untrim, lip, topo, provenance): picks by exact distance and history | main | yes |
| 3.4 | Picking: a kernel ray/body primitive and an ops pick; viewport, engine and frames switch | kernel, then main | no |
| 3.5 | Engine measures: clearance, interference, contact sweep, motion, compare, join pieces | main | no |
| 3.6 | Doc placement and naming: `eval.rs`, `eval_more.rs`, `naming.rs`, `name_lookup.rs`, `project*.rs`, joints | main | no |

3.1 to 3.3 run in parallel; 3.5 and 3.6 split by file, one cluster of mesh-decision lines per PR.

### Phase 4: blends

After the new blend algorithm lands in the kernel: the fillet, chamfer, variable and corner commands become ops recipes; `kernel/src/blend*.rs`, `corner_g2.rs` and `smlib/src/core/blend.rs` are deleted. The open regressions #13536–#13539 and #13541–#13542 are fixed first or stay listed in `KNOWN_FAILING_TESTS`; none is added. Gates: Rig and Fillet.

### Phase 5: the switch commit

jyviko's commit in SolidCraft's kernel crate: about 100 places that reach into the kernel body call `solidcore-ops` instead, with no temporary public API in ops and no per-family switching. `crates/smlib` (22k with tests) and the `crates/kernel` copies are deleted, and the seam allowlist becomes empty. Gate: every gate above, the same counts and values.

### Phase 6: solidcore as a product

| PR | What | Gate | Parallel |
|---|---|---|---|
| 6.1 | Semver on `solidcore-ops`, `solidcore-tess` and `solidcore-record`: `cargo-semver-checks` in CI and a changelog per crate | a breaking change without a major bump fails | from now |
| 6.2 | Rustdoc on every public item (`missing_docs` denied in the three crates) | `cargo doc` with `-D warnings` | 6.1 |
| 6.3 | `examples/`: make a part, fillet it, export STEP, mesh it; each runs in CI | the examples run and check their results | after 1.3, 1.6 |
| 6.4 | A second consumer in CI (a small example app, or the existing harness) that uses only the public API | builds and passes without SolidCraft | after 6.3 |
| 6.5 | Later: our own C ABI and Python bindings, and a wasm package. Never OCP, pythonOCC or any GPL/LGPL wrapper, and nobody reads them | binding tests in CI | after Phase 5 |

6.1 and 6.2 can start now. Each later API addition lands with its docs and example.

### Phase 7: the rest

The remaining lines of `app-geometry.txt` and `mesh-decisions.txt`, one cluster per PR, until both are empty and `mesh-decisions` retires (layers §6).

## 3. Rules for the agents doing it

- **SolidCraft's code is the base.** For every job: `git mv` SolidCraft's implementation into `solidcore/` (the move alone in its own commit, so blame keeps its authors), add what measures better in solidcore's existing copies, and unify them into one crate per job. solidcore's duplicate copies (two STEP readers, three meshers and the rest) are deleted and replaced.
- **Literature first.** Show the problem, read the papers that solve it, and say in the issue how the design follows them (solidcore `AGENTS.md` §1).
- **No fallbacks or try-chains.** One construction per job. An old path is deleted when its replacement lands, never kept behind it (§4b).
- **One topic per PR**: one job moved, its callers switched, the old copy deleted, its ledger lines removed.
- **ops stays general.** Nothing SolidCraft-specific goes in it: no defaults, command ids, UI or document types. SolidCraft passes those in as parameters.
- **Branches.** PRs target `main`. Only multi-step kernel work that isn't safe on `main` until every step has landed goes to the `kernel` branch.
- **No compatibility flags.** No switch between old and new, no `UpstreamFixes` field, no preference for the old result.
- **Claim first** on the job's tracking issue, and name what is added from the other copy, with its measurement.
- **Naming survives topology changes.** When a move changes a body's topology (as truck's split cylinder sides did in R70), add the face-name mapping in SolidCraft's naming layer (`crates/doc/src/naming.rs`, `history_names.rs`) so saved references resolve. Never bend the kernel's topology to match the old names.
- **Tests move with the code**, into the layer's test files; kernel cases go in `solidcore/crates/solidcore/tests/kernel_cases/`.

## 4. Open question for jyviko

**Surface construction** (§1b, about 16k lines: fills, curve networks, bridge, Match Surface, fair, T-splines): do we port the existing code into the kernel with its tests, or redesign it in solidcore first?

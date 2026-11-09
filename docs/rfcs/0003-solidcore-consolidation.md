# RFC 0003: solidcore consolidation, one implementation per job

| | |
|---|---|
| Status | Draft, open for comment |
| Started | 2026-11-08 |
| Repositories | SolidCraft (`crates/`), solidcore (vendored at `solidcore/`) |
| Authority | `solidcore/docs/layers.md` |
| Discussion | the pull request that adds this file, then the tracking issue of each job |

`layers.md` says each job lives in one layer: the kernel computes, ops decides and sequences, record is data, convert is the one record↔kernel conversion, tess draws, and apps hold state with no geometry math. Today SolidCraft still carries its own copy of most of those jobs. This RFC lists those copies, orders the work of removing them, and sets the gate each PR must pass. It fits inside the plan already agreed for solidcore-ops: the local-operation sprint, the solidcore#3005 follow-ups, stage 3b, blends last, then the switch commit. It adds no stage of its own.

## 1. Inventory

Sizes are non-test lines on main `1c8fdde4ba` (tests in brackets). "Callers" counts the source files that use the code, by crate. Measured comparisons come from the mesher bake-off (#13625: 1,267 bodies, display and export settings) and the STEP bake-off (#13624: 320 files, 219 with a solid).

### 1a. Jobs solidcore already has

| Job | SolidCraft copy (lines) | Layer | solidcore counterpart | Better, by measurement | Callers |
|---|---|---|---|---|---|
| Meshing | `kernel/src/tess/` (6.2k [1.1k]); `geom/src/tess.rs` + `tess/` (1.6k: plan, fast evaluator, partials) | tess | `solidcore-tess` (same planner, 15k with tests) | Tie on triangles (2,471 of 2,530 runs match, none >5% apart). solidcore-tess is more robust (meshes `rod_doubled_edge`, reports empty faces) and 10–60x slower on spline bodies (`eval::collapse`, #13655). Decided: solidcore-tess | `Body::mesh`/`mesh_plan`: engine 50, kernel 37, ui-egui 30, doc 23, io 3, gpu 2 |
| Mesh simplification | `geom/src/simplify.rs` (0.55k) | tess | `solidcore-tess/src/simplify.rs` | Not measured | gpu 1, ui-egui 1 |
| Second (u, v) grid mesher | none in SolidCraft | tess | `nurbs-grid-mesh` (2.3k) | Loses: T-junctions on ~60% of closed bodies, over the chord on 70 bodies vs 5. Keep `deviation` as a test checker; delete after a 300 x 300-pole test | none |
| STEP read | `crates/step` (7.3k [1.8k]); `kernel/src/step_in` (1.7k [3.1k]) | record (model, parser), ops (build) | `solidcore-record::step`, `step-import`, `step-brep` (11k) | Core opens 218 vs 196 and refuses 0 vs 23, closes 12 bodies SC leaves open. SC is 3x faster (median 0.14 s vs 0.43 s), opens `ap203/rear` (core times out), matched 22 of 23 reference volumes vs 21. Placements (`cylinder_twice`) fixed on `kernel` by #13582. Decided: core topology-first | kernel 1 (import), engine 1 |
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
| Mesh-based model decisions | `xtask/mesh-decisions.txt`: 181 functions (doc 96, engine 51, kernel 34). In the kernel crate: `Body::{faces, edges, meets, meets_each, volume_at}`, `measure`, `topo`, `provenance`, blend and offset picks. Their mesh tools: `geom/src/{mesh,bvh,mesh_distance,compare}.rs` (2.4k) | ops queries over kernel measures | `Body::meets` has its kernel half merged on `kernel` (#13554, #13660). About 43 lines pick by nearness, 42 measure (volume, gap, clearance), 38 name or trace | everywhere |
| Picking (ray/body) | `ui-egui/src/viewport.rs` `pick_hits` on display meshes, `geom::Mesh::raycast`, `bvh.rs`; engine `face_at_state`, `frames::frame_at`, `sketch_inspect::face_at` | kernel primitive (layers §1); ops query | `solidcore::intersect::ray_tracer` exists; no ops entry | ui-egui, engine, doc |
| Tangent chains | four walks: `kernel/src/blend.rs::tangent_chain`, `geom::Mesh::tangent_chain`, `doc/src/eval.rs::{tangent_face_chain, smooth_chain}`, `ui-egui/src/context_menu.rs::tangent_chain`; tangency test `kernel/src/tangent_edge.rs` (0.4k) | ops walk; kernel tangency measure | #13662, no PR | engine 2, geom 2, ui-egui 3 |
| Display-only edges and isoparms | `kernel/src/{smooth_edges,iso}.rs` (1.0k) | tess | regression #13542 lives here | render, ui-egui |
| Surface construction | `geom/src/{fill,fill_iso,fill_general,fill_matched,network,blend}.rs`, `kernel/src/{fill,boundary_network,boundary,bridge,matchsurf,fair}.rs` (12.7k); `geom/src/{bspline,spline_space,tspline/}` + `kernel/src/tspline.rs` (3.6k); `geom/src/{arcfit,catmull_rom}.rs` (0.5k) | kernel (build, fit); ops (the commands) | solidcore has curve and surface fitting (`nurbs/crv_fit`, `srf_fit`), no curve-network fill, no T-splines | kernel, engine, doc |
| Planar builds, sewing, certificates | `kernel/src/{prism,prism_curve,planar_sources,planar_regularize,polybool,sew,mesh_convert}.rs` (5.5k) | kernel | solidcore has `prism`, `Boolean2D`, stitching; overlap not measured | kernel |
| Local surface commands | `kernel/src/{surfaces,surface_trim,surface_untrim,surface_extend,surface_ruled,splitface,delete_face,emboss,lip,repair}.rs` (8.2k) | ops recipes | ops has `split`, `untrim`, `extend`, `stitch` | kernel, engine |
| Hidden lines, sections, projections | `geom/src/hlr/` (4.5k, own ray/surface intersection and inversion), `geom/src/section.rs`; `doc/src/project.rs` silhouettes and sections by mesh, `project3d.rs` | kernel (silhouette, section); ops (the view recipe) | kernel has `surface_silhouette`; drawings work is on the `drawings` branch | kernel, doc |
| Sheet metal | `doc/src/sheet.rs` (10.3k: flat pattern, bend zones, fold maps), `sheet_convert.rs` | app rules; folding and the folded solid are kernel transforms and ops builds | no counterpart | doc, engine |
| IGES | `kernel/src/{iges_in,iges_in/assembly,iges_out}.rs` (2.7k) | record, ops (same split as STEP) | no counterpart | kernel |

Out of scope here, flagged for jyviko (§4): `crates/sketch` (2D constraint solver, 32k), `crates/dynamics`, camera and lighting maths in `render` and `gpu`, and the mesh file formats in `io` (STL, 3MF, glTF, PLY, OBJ), which are data.

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
| 0.1 | `cargo xtask app-geometry` and `xtask/app-geometry.txt` (layers §4): a shrink-only list of functions in `doc`, `engine`, `ui-egui`, `io`, `kernel` and `geom` that compute geometry (vector maths on coordinates, solves, mesh queries), seeded from §1. CI posts per-crate counts on each PR | lists today's sites; fails on a new one |
| 0.2 | `solidcore/docs/jobs.toml` and `cargo xtask jobs`: each job names its one crate and module; the duplicates in §1a are listed with the PR that removes them, and the list only shrinks | fails on a new public entry point outside its job's module |
| 0.3 | The seam check also fails on a new use of `solidcraft-geom`'s evaluator, tess or mesh queries from a crate that does not use them today | ledger counts unchanged |

### Phase 1: jobs solidcore already wins or ties

| PR | What | Branch | Gate | Parallel |
|---|---|---|---|---|
| 1.1 | solidcore-tess speed (#13655): cache `eval::collapse` per surface or boundary row, or port SolidCraft's evaluator (`EvalSurface::derivs`, `basis_ders_into`, `degenerate_within`) as tess's private, display-only evaluator | kernel | Speed: no body more than 1.2x SolidCraft's tess on the 1,267-body corpus; triangle counts unchanged | 1.2, 1.4 |
| 1.2 | solidcore-tess flat faces with no size cap (port of #13266 into `planner/size.rs`) | kernel | the 20 x 10 x 5 box exports 12 triangles; tessellation cases | 1.1 |
| 1.3 | SolidCraft meshes through `solidcore-tess`; delete `kernel/src/tess/`, `geom/src/tess.rs` and `tess/`, `geom/src/simplify.rs` if solidcore's simplify ties | main | display-mesh and export tests; triangle counts within 5% of the bake-off; cracks no more than today (3 of 1,176) | after 1.1, 1.2 |
| 1.4 | `nurbs-grid-mesh`: the 300 x 300-pole test, `deviation` kept as a test checker, crate deleted | kernel | the test recorded on #13625 | any |
| 1.5 | STEP reader: fix the `ap203/rear` timeout (to be filed) and close the speed gap by its root cause | kernel | `rear` opens; median within 1.5x of 0.14 s on the 320 files | 1.1–1.4 |
| 1.6 | SolidCraft reads STEP through `solidcore-record` + ops build; delete `crates/step` reading and `kernel/src/step_in` | main | `tests/step-conformance`, `tests/step-regressions`, `kernel-cases/import` unchanged or better; bake-off: opens ≥218, refuses 0, ≥22 of 23 volumes | after 1.5 |
| 1.7 | STEP write: measure both writers by round trip on the corpus, keep one, delete the other | main or kernel by the survivor | round-trip volumes and face counts | after 1.6 |
| 1.8 | Evaluation: SolidCraft's callers of `geom::brep::{eval, form, normal_section}` move to ops and tess queries over `solidcore::eval`; the geom copies are deleted | main | Rig; record checks unchanged | after 1.3 |

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
| 2.9 | new | surface construction as kernel primitives: fills, curve networks, bridge, match, fair, T-splines (§4 q3) | kernel |

2.1 to 2.5 and 2.8 run in parallel. 2.6 and 2.7 need 2.1's lint. 2.9 waits for jyviko's answer to q3.

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

### Phase 6: the rest

The remaining lines of `app-geometry.txt` and `mesh-decisions.txt`, one cluster per PR, until both are empty and `mesh-decisions` retires (layers §6).

## 3. Rules for the agents doing it

- **Literature first.** Show the problem, read the papers that solve it, and say in the issue how the design follows them (solidcore `AGENTS.md` §1).
- **No fallbacks or try-chains.** One construction per job. An old path is deleted when its replacement lands, never kept behind it (§4b).
- **One topic per PR**: one job moved, its callers switched, the old copy deleted, its ledger lines removed.
- **Branches.** Kernel-only PRs (`solidcore/` alone) target `kernel`. PRs that also touch SolidCraft's crates target `main`.
- **No compatibility flags.** No switch between old and new, no `UpstreamFixes` field, no preference for the old result.
- **Claim first** on the job's tracking issue, and name the surviving copy with its measurement.
- **Naming survives topology changes.** When a move changes a body's topology (as truck's split cylinder sides did in R70), add the face-name mapping in SolidCraft's naming layer (`crates/doc/src/naming.rs`, `history_names.rs`) so saved references resolve. Never bend the kernel's topology to match the old names.
- **Tests move with the code**, into the layer's test files; kernel cases go in `solidcore/crates/solidcore/tests/kernel_cases/`.

## 4. Open questions for jyviko

1. **Surface construction with no counterpart** (§1b, about 16k lines: fills, curve networks, bridge, match, fair, T-splines): port it into the kernel as it is, with its tests, or design it anew in solidcore first?
2. **Picking:** may hover picking stay on the display mesh's face ids, with only the picked point and any decision taken from a kernel ray/body call?
3. **The kernel crate after the switch:** does `crates/kernel` remain as an app crate under the seam rule, or do its commands all move into ops?
4. **STEP write:** is a round-trip measurement enough to choose between `step-export` and SolidCraft's writer?
5. **Scope:** are `crates/sketch`'s 2D solver and sheet metal's fold maps app geometry to move, or app logic over kernel transforms?
6. **Evaluation (1.8):** before the switch, or with it?

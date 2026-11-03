# Kernel: moving from truck to SMLib

How SolidCraft's kernel moved from truck to SMLib (usd-brep, Apache-2.0), as it was written up while the work was on the `kernel-smlib` branch (tracked in #4012, now closed: the branch landed on `develop`). It is a record of the steps and their pictures, not the current contract: that is [native-brep.md](native-brep.md), with the case ladder in [kernel-ladder.md](kernel-ladder.md) and [kernel-cases.md](kernel-cases.md). truck has since been removed.

The pictures live on the `pr-media-kernel-smlib` branch.


## Where it started (2026-10-21)

### Schema 4: a plain B-rep that mirrors SMLib's model
`solidcraft_geom::brep` now holds:
- regions and shells;
- oriented faces, and faces with parameter domains;
- edge and vertex loops;
- oriented edges, each carrying a **pcurve** (the edge in the face's (u, v));
- edges with intervals, and vertices;
- tolerances per body, per edge and per vertex.

Names follow STEP's B-rep vocabulary. Analytic curves and surfaces are unbounded; their bounds live in the edge interval and the face domain. A validator enforces one **canonical order** of faces, edges and vertices, which persistent names depend on. The history types moved beside it, to `solidcraft_geom::brep::history`. The contract is in docs/native-brep.md and tangency-labs/solidcore#342.

truck bodies round-trip through schema 4 exactly, and the model cache stores it. Every corpus design, sample and imported assembly reopens from the cache with bit-identical volumes, areas and face names:

| Filleted box | Cylinder cut by a tilted box |
|---|---|
| ![box-fillet](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/brep-schema-box-fillet.png) | ![cyl-cut](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/brep-schema-cyl-cut.png) |
| ![box-hole](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/brep-schema-box-hole.png) | ![corpus](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/brep-schema-corpus.png) |

### Pcurves for every edge
Pcurves are exact on planes. Elsewhere they're a cubic 2D B-spline fitted within tolerance, never a polyline. A seam edge gets two pcurves one period apart, and poles are handled. Each face's loops, drawn in its (u, v):

![cut](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/pcurves-cut-side.png)
![cylinder](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/pcurves-cylinder-side.png)
![sphere](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/pcurves-sphere-side.png)
![torus](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/pcurves-torus-side.png)
![seam](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/pcurves-seam-cylinder.png)

### A naming guard for the move
`apps/solidcraft-cli/tests/naming_corpus.rs` rebuilds 154 designs and checks every face and edge name, and every stored reference, against `tests/naming/`. Every operation that switches to SMLib must keep them. Re-record with `NAMING_CORPUS_BLESS=1`.

![names](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/naming-naming-split-pieces-face-names.png)
![diff](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/naming-test-failure-diff.png)

### Next, as planned then
- SMLib linked statically behind C shims (`solidcraft-smlib`).
- SMLib bodies exported to schema 4 and shown in SolidCraft.
- Booleans, fillet, chamfer and shell through SMLib, compared side by side with truck.

## Updates

### SMLib B-reps in SolidCraft (2026-10-21)

Each body below was made by SMLib, converted to schema 4 (canonical order, our own pcurves, `validate()` and `check_geometry()` passing), decoded into a SolidCraft `Body`, and rendered next to truck's primitive of the same size. The faces, edges and vertices are SMLib's own: a cylinder is 3 faces, a whole sphere 1 face bounded by its seam, a torus 1 face.

| | |
|---|---|
| ![box](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/smlib-box.png) | ![cylinder](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/smlib-cylinder.png) |
| ![sphere](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/smlib-sphere.png) | ![frustum](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/smlib-frustum.png) |
| ![cone](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/smlib-cone.png) | ![torus](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/smlib-torus.png) |

The torus's and the cylinder's loops in (u, v), from our pcurve routine:

![torus-uv](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/smlib-torus-uv.png)
![cylinder-uv](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/smlib-cylinder-uv.png)

The SMLib volumes are SMLib's exact mass properties and match the closed form. "Decoded" is SolidCraft's mesh-based measure of the decoded body, within 2e-4 relative. These pictures come from a first JSON export; the same path now goes through the fork's flat converter (`capi-flat`) and is being rebuilt there.

Also on `kernel-smlib` since the last update:
- the link crate (`solidcraft-smlib`);
- the STEP geometry crate (`solidcraft-step`);
- history out of SMLib operations, and a side-by-side runner for truck and SMLib booleans;
- `Brep::check_geometry`, run before any record is decoded;
- cones that narrow along their axis.

### Landed on `kernel-smlib`: SMLib bodies into SolidCraft through schema 4, and back (2026-10-21)

(4d72f7e8, 1522e133). SMLib B-reps leave the C API as the fork's flat arrays, a port of SMLib's own converter without USD. They become schema 4 records: canonical order, our own pcurves, `validate()` and `check_geometry()` passing. With the kernel's `smlib` feature, `body_from_smlib` decodes them into a `Body`.

The reverse direction is in too. A round-trip test sends every primitive (box, cylinder, sphere, cone, two frustums, torus) from SMLib to schema 4 and back into SMLib, and each keeps its counts and its exact volume within 1e-7, and passes SMLib's own check at every level. The pictures above are re-rendered on this path. SMLib's whole sphere is now bounded correctly by its seam in (u, v):

![sphere-uv](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/pole-seam-sphere-seam-uv.png)

### SMLib operations in SolidCraft (2026-10-21)

(`kernel-smlib` 4067ab4c). Each result was built live by SMLib, exported to schema 4, decoded into a `Body` and rendered. Next to it is truck's result of the same operation on the same inputs. SMLib's own check passes and schema 4's `check_geometry` is ok in all nine.

| | |
|---|---|
| ![union](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/smlib-ops-render-union.png) | ![slot](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/smlib-ops-render-subtract_slot.png) |
| ![split](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/smlib-ops-render-subtract_split.png) | ![intersect](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/smlib-ops-render-intersect.png) |
| ![hole](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/smlib-ops-render-subtract_hole.png) | ![fillet](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/smlib-ops-render-fillet_all_edges.png) |
| ![chamfer](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/smlib-ops-render-chamfer_one_edge.png) | ![shell](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/smlib-ops-render-shell_inward.png) |

SMLib's volumes equal the analytic values in every case. Truck's are mesh-measured and close. The face counts match, except that SMLib keeps a through-hole as one cylindrical face with a seam where truck uses two halves. History is classified differently in two places. SMLib keeps an untouched face that truck calls Modified. A shell's inner walls are Split for SMLib and Generated (offsets) for truck. No face name moves because of it, since names are built from origins.

**A bug this found, being fixed now.** SMLib builds a hole-rim fillet as a NURBS torus that closes on itself in u. Our pcurve routine gives NURBS directions no period, so both uses of that face's seam got the same pcurve, the face enclosed nothing and wasn't drawn:

![rim](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/smlib-ops-render-fillet_hole_rim.png)

### Fixed: SMLib's hole-rim fillet (2026-10-21)

(`kernel-smlib` 4067ab4c..0eacff3e). A spline surface that closes on itself is now periodic over its knot interval for pcurves and tessellation, so the fillet torus's two seam uses sit one period apart. The decoded volume is 6989.0174 against SMLib's 6988.9956 (3e-6), where it was 6839.73. `check_geometry` now also refuses a loop that encloses no (u, v) area or turns the wrong way.

| before | after |
|---|---|
| ![before](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/rim-before.png) | ![after](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/rim-after.png) |

![uv](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/rim-uv.png)

### truck and SMLib booleans, side by side (2026-10-21)

(runner on `kernel-smlib` 8665746e: `cargo run --release -p solidcraft-kernel --features smlib --example smlib_booleans -- --seeds 128`). 411 operations: 128 random box/cylinder/sphere/torus pairs and 9 named cases, each as union, intersection and cut. Each kernel is timed on the boolean alone.

| | median | worst | total |
|---|---|---|---|
| truck boolean | 8.8 ms | 6.2 s | 44.0 s |
| SMLib boolean | 1.8 ms | 25 ms | 1.1 s |
| moving operands into SMLib | 2.6 ms | 91 ms | |
| moving the result back | 4.4 ms | 522 ms | |

![times](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/perf-times.png)

**The cut sequence:** a block, a joined boss, then three cuts, the last splitting the block. truck slows down with each cut and SMLib stays flat. SMLib matches the analytic volume exactly at every step, with the same face and edge counts as truck.

| step | truck | SMLib |
|---|---|---|
| join Ø20 boss | 9 ms | 5 ms |
| slot | 61 ms | 5 ms |
| Ø6 cross hole | 378 ms | 7 ms |
| splitting slot | 383 ms | 4 ms |

![sequence](https://raw.githubusercontent.com/tangency-labs/solidcraft/pr-media-kernel-smlib/perf-times-sequence.png)

**Where SMLib isn't at parity yet:**
- **11 of 411 operations fail on SMLib,** where a cylinder or torus is tangent to a planar face. truck completes all of them. Filed as tangency-labs/solidcore#383.
- **About 15 SMLib volumes are wrong although the body is right:** the same body measured after export matches. Filed as tangency-labs/solidcore#384.

**Also to fix on our side** (the conversion back to SolidCraft):
- results built from SMLib's own spheres and tori;
- an edge that runs along only part of its spline;
- a slow export of a one-face torus.

On the 27 named operations, SMLib's volumes are within 1e-6 of the analytic values, and truck's within 1e-3 (it measures from a tessellation).

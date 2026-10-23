# solidcore status

A short summary of where the port stands. Per-unit detail lives elsewhere:

- **Live progress site:** [tangency-labs.github.io/solidcore](https://tangency-labs.github.io/solidcore/),
  regenerated on every merge to `main` (unit status, difftest results, upstream issues).
- **`plan/PROGRESS.md`:** the generated checklist. CI regenerates it; never edit it by
  hand. The copy on `main` was last regenerated on 2026-10-21 and lags the site.
- **`plan/units/<id>.toml`:** each unit's status and notes.
- **`plan/findings.md`:** Rust-vs-C++ disagreements, harness notes and handoffs
  (append-only). **`plan/upstream-issues.md`:** upstream C++ defects.
- **`plan/STATUS-history.md`:** this file's earlier contents (2026-10-12 to 2026-10-22),
  archived unchanged.

## Snapshot: main CI on `e33985411` (2026-10-23)

From the difftest reports of CI run 40029966086 on `main` (the merge of PR #670).

- **Units:** 261, all ported (0 `todo`). 260 have a difftest runner: **199 green**,
  **61 partial**, 0 failing. `audit.recursion` has no runner and is green in its toml.
- **Cases:** 554,873 in all: 550,446 pass, **0 fail**, 4,186 deferred (they reach a call
  the port still stubs), 234 rejected by FFI guards (inputs that are undefined or crash
  in C++), 7 known mismatches.
- **Known mismatches** (`crates/difftest/known_mismatches.toml`, each with a findings
  entry): `blend.fillet_intersector` `rv_next_clipped_step` (the clipped `StepSolve`
  branch differs by a few ULP), and six `topology.brep_p3` TurnToNURBS tracker cases
  (upstream curve and surface edits send `SM_NO_PRE_EDIT`/`SM_NO_POST_EDIT`
  notifications; the port's geometry edits send none).
- **Status drift:** `blend.fillet_executive_p1` passes all 61 cases with nothing deferred
  but is still `partial` in its toml. Draft PR #665 is working on that unit.

## Cross-cutting blockers

A deferred case is tagged with the unit that owns the stubbed call it reached
(`UNPORTED(<unit>)`). Summed over all units, the tags that hold back the most cases are:
`topology.face_p2` 750, `topology.face_p4` 613, `topology.edge` 542,
`intersect.curve_class_p4` 406, `surface.trim_srf_cache` 250,
`intersect.curve_class_p6` 149, `topology.vertex` 143, `query.face_p2` 138,
`primitive.primitive_creation_p1` 135, `blend.fillet_corner_p2` 110,
`topology.edgeuse_p3` 100. Every unit is ported, so these are stubs between ported units
that nobody has wired yet (cycle back-edges and co-port leftovers; some owners, such as
`topology.edge`, `surface.trim_srf_cache`, `topology.vertex` and `topology.edgeuse_p3`,
are already green), not missing units. Another 98 cases (all of `topology.face_p1`'s
deferrals) stay behind the C++ crash guard for `UB-137` (`SmFace::SplitAtSeam`
recursing on another face's seam classifications).

## Partial units

Sorted by deferred cases. "Main blockers" are the stub tags of the deferred cases, largest
first (`cpp-guard:` / `cpp-crash-guard:` = inputs kept away from a C++ crash, `harness:`
= a gap in the difftest harness, `upstream:` = an upstream hang).

| Unit | Cases | Passed | Deferred | Main blockers (deferred cases) |
|---|---:|---:|---:|---|
| `primitive.api_primitives` | 1981 | 1623 | 358 | `surface.trim_srf_cache` 133, `topology.face_p4` 112, `topology.face_p2` 62, 2 more |
| `intersect.curve_class_p4` | 3201 | 2921 | 280 | `topology.face_p4` 280 |
| `topology.brep_p10` | 1404 | 1154 | 233 | `intersect.curve_class_p4` 150, `topology.face_p4` 72, `topology.face_p2` 7, 2 more |
| `query.api_queries` | 1203 | 988 | 215 | `topology.face_p2` 157, `topology.edge` 31, `topology.edgeuse_p3` 27 |
| `topology.face_p3` | 1781 | 1604 | 177 | `topology.edge` 127, `topology.face_p2` 50 |
| `boolean.merge_p4` | 311 | 140 | 168 | `topology.edge` 77, `topology.vertex` 71, `topology.brep_p1` 20 |
| `offset.offset_executive_p2` | 322 | 141 | 164 | `offset.offset_executive_p1` 76, `topology.brep_p4` 42, `topology.brep_p3` 33, 2 more |
| `topology.brep_p1` | 2627 | 2461 | 150 | `topology.face_p2` 117, `intersect.curve_class_p4` 12, `topology.edge` 10, 3 more |
| `offset.offset_executive_p1` | 1048 | 909 | 138 | `topology.brep_p8` 48, `topology.edge` 46, `query.face_p2` 44 |
| `boolean.api_brep` | 287 | 151 | 136 | `primitive.primitive_creation_p1` 135, `topology.brep_p3` 1 |
| `boolean.topology_intersector_p2` | 910 | 775 | 135 | `topology.face_p2` 68, `topology.edge` 40, `topology.brep_p1` 13, 4 more |
| `heal.heal_data_p1` | 495 | 367 | 128 | `intersect.curve_class_p4` 109, `topology.face_p4` 8, `primitive.api_primitives` 7, 1 more |
| `blend.fillet_corner_p3` | 1005 | 799 | 126 | `blend.fillet_corner_p2` 107, `blend.fillet_geom_p5` 13, `blend.fillet_executive_p1` 6 |
| `primitive.primitive_creation_p2` | 328 | 195 | 113 | `surface.trim_srf_cache` 57, `boolean.merge_p2` 34, `topology.face_p4` 14, 1 more |
| `intersect.curve_class_p6` | 4438 | 4326 | 112 | `intersect.curve_class_p6` 92, `topology.vertex` 20 |
| `query.face_p3` | 1199 | 1087 | 112 | `topology.edge` 42, `query.face_p2` 37, `surface.surface_p2` 22, 1 more |
| `boolean.topology_intersector_p3` | 271 | 166 | 105 | `intersect.curve_class_p6` 57, `topology.vertex` 27, `topology.face_p2` 15, 1 more |
| `query.face_p4` | 500 | 395 | 105 | `intersect.curve_class_p4` 90, `topology.face_p4` 12, `surface.surface_p2` 3 |
| `topology.face_p1` | 2315 | 2217 | 98 | `cpp-crash-guard:UB-137` 98 |
| `heal.face_props` | 1528 | 1431 | 97 | `topology.face_p4` 82, `surface.surface_p2` 15 |
| `topology.face_p2` | 2692 | 2596 | 96 | `topology.edge` 47, `query.face_p2` 33, `topology.face_p4` 16 |
| `topology.face_p4` | 5506 | 5426 | 80 | `topology.edge` 80 |
| `blend.fillet_geom_p6` | 400 | 322 | 78 | `harness:analytic-geometry-export` 61, `blend.fillet_executive_p1` 9, `blend.fillet_geom_p1` 8 |
| `topology.proto_topology_p2` | 1533 | 1462 | 71 | `topology.face_p1` 45, `topology.brep_p1` 25, `topology.vertex` 1 |
| `offset.offset_geometry_creation` | 898 | 832 | 66 | `topology.face_p2` 49, `upstream:HANG-26` 16, `surface.trim_srf_cache` 1 |
| `topology.brep_p7` | 348 | 285 | 63 | `intersect.curve_class_p4` 26, `topology.edge` 20, `query.face_p2` 6, 4 more |
| `boolean.merge_p3` | 423 | 361 | 62 | `boolean.topology_intersector_p2` 48, `topology.vertex` 9, `topology.edge` 5 |
| `sweep.api_brep` | 573 | 514 | 59 | `surface.trim_srf_cache` 45, `boolean.merge_p2` 8, `topology.edgeuse_p3` 6 |
| `query.brep` | 1172 | 1115 | 57 | `topology.face_p2` 55, `topology.edgeuse_p3` 2 |
| `blend.fillet_executive_p3` | 291 | 245 | 46 | `blend.fillet_executive_p1` 46 |
| `topology.brep_p3` | 897 | 853 | 38 | `topology.face_p2` 38; 6 known mismatch |
| `topology.brep_p2` | 1709 | 1673 | 36 | `topology.face_p2` 35, `topology.brep_p2` 1 |
| `geom.tools` | 915 | 883 | 32 | `topology.face_p2` 32 |
| `intersect.topology_solver_p2` | 2429 | 2401 | 28 | `surface.surface_p2` 12, `query.face_p2` 8, `topology.face_p2` 8 |
| `primitive.primitive_creation_p1` | 773 | 740 | 24 | `boolean.topology_intersector_p2` 10, `surface.trim_srf_cache` 8, `topology.face_p2` 4, 1 more |
| `heal.heal_data_p3` | 478 | 457 | 21 | `intersect.curve_class_p4` 19, `topology.face_p2` 2 |
| `boolean.topology_intersector_p1` | 830 | 810 | 20 | `topology.face_p2` 16, `query.face_p2` 3, `topology.brep_p5` 1 |
| `blend.fillet_corner_p2` | 468 | 446 | 19 | `blend.fillet_geom_p5` 17, `blend.fillet_corner_p2` 1, `blend.fillet_executive_p1` 1 |
| `query.face_p2` | 1344 | 1326 | 18 | `topology.edge` 14, `query.face_p1` 4 |
| `blend.fillet_executive_p2` | 275 | 259 | 16 | `blend.fillet_executive_p1` 12, `blend.fillet_standard_solver_p2` 4 |
| `blend.fillet_geom_p3` | 325 | 302 | 16 | `intersect.surface_intersector_p4` 12, `blend.fillet_corner_p1` 2, `blend.fillet_corner_p2` 2 |
| `topology.brep_p4` | 327 | 297 | 15 | `topology.face_p2` 9, `surface.trim_srf_cache` 3, `topology.edgeuse_p3` 3 |
| `blend.fillet_solver_p2` | 311 | 301 | 10 | `blend.fillet_executive_p1` 10 |
| `topology.brep_p5` | 797 | 787 | 10 | `topology.brep_p1` 8, `topology.brep_p4` 2 |
| `topology.brep_p6` | 595 | 586 | 9 | `topology.edgeuse_p3` 9 |
| `intersect.curve_class_p3` | 3708 | 3700 | 8 | `topology.vertex` 8 |
| `boolean.merge_p1` | 248 | 241 | 7 | `boolean.topology_intersector_p3` 4, `topology.brep_p5` 2, `topology.face_p2` 1 |
| `blend.fillet_corner_p1` | 174 | 170 | 4 | `blend.fillet_geom_p3` 3, `blend.fillet_corner_p1` 1 |
| `blend.fillet_geom_p4` | 400 | 396 | 4 | `topology.face_p2` 4 |
| `blend.fillet_solver_p5` | 100 | 97 | 3 | `query.face_p1` 3 |
| `blend.hcr` | 434 | 431 | 3 | `topology.face_p2` 3 |
| `boolean.merge_p2` | 45 | 42 | 3 | `topology.face_p2` 2, `topology.edgeuse_p3` 1 |
| `intersect.curve_class_p1` | 2125 | 2122 | 3 | `topology.edge` 2, `topology.face_p2` 1 |
| `sweep.api_surfaces` | 882 | 879 | 3 | `surface.trim_srf_cache` 3 |
| `query.poly` | 687 | 685 | 2 | `topology.face_p2` 2 |
| `topology.shell` | 911 | 909 | 2 | `topology.face_p2` 2 |
| `intersect.curve_class_p7` | 1516 | 1515 | 1 | `topology.vertex` 1 |
| `topology.brep_p8` | 339 | 338 | 1 | `topology.face_p2` 1 |
| `topology.brep_p9` | 684 | 683 | 1 | `topology.face_p2` 1 |
| `topology.topology_traverser` | 909 | 908 | 1 | `topology.edge` 1 |
| `blend.fillet_intersector` | 1292 | 1291 | 0 | 1 known mismatch |

## Keeping this file current

Refresh the snapshot from a recent main CI run (`gh run list --branch main`, then
`gh run download <id> -n difftest-reports`) instead of appending "latest" lines. Handoff
notes and investigations belong in `plan/findings.md` or the PR, not here.

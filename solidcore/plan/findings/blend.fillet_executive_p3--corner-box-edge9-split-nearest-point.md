# blend.fillet_executive_p3: corner_box edge 9 moves 2.5e-7 with the nearest-point split (2026-11-05)

**Unit:** `blend.fillet_executive_p3`, case `source-file-corner_box-edge9`
(`fixtures/blend.fillet_executive_p3/solid_sources.json#49`: `SmApiFilletEdges` on
`upstream/TestFiles/pt_TestFiles/Fillet/corner_box.smb`, edge 9, radius 0.1, continuity 1,
cross-section 1, thumbweight 1).

**What moved.** The case passed against the pinned C++ until tangency-labs/solidcore#2735
(f06f897ea; CI run 41988264776 passes it with max relative error 1.26e-12, run 41991964300 on #2735
fails it at 2.4777e-7). #2735 replaced the split parameter of `MergeIntoObject`
(`intersect::point_class::split_parameter`, WR-751): an edge is split where its curve passes nearest
the classified point when that is nearer than the classification's `TParam` and within the edge's
tolerance. Here the fillet's insertion classifies a point to edge 11 of the body (interval [0,1],
tolerance 1e-5) at `TParam` 0.9528318886810855, whose curve point is 1.0104522409903893e-6 from the
classified point (-4.4, -8.833331897760253, 6.900000000000005); the edge passes the point at
t = 0.9528316525973994, 7.680867880835109e-7 away. Both are inside the edge's tolerance, so the C++
result is not wrong by its own check: this is the same construction split at a nearer parameter,
not a C++ defect, and the case is a `known_mismatch`, not `cpp_defect`.

**The values** (`values.result_geometry`, the result body's record: the split edge's interval end
and its three sample points at 17 %, 37 % and 83 % of the interval; the merged vertex itself is the
same on both sides):

| field | C++ (split at `TParam`) | solidcore (split nearest the point) | rel. err |
|---|---|---|---|
| `result_geometry[212]` (interval end) | 9.528318886810855e-1 | 9.528316525973994e-1 | 2.48e-7 |
| `result_geometry[216]` | -7.323210121796129e0 | -7.323210042135989e0 | 1.09e-8 |
| `result_geometry[217]` | 5.2984399281785395e0 | 5.298439849859102e0 | 1.48e-8 |
| `result_geometry[220]` | -7.698594922016614e0 | -7.6985947511846256e0 | 2.22e-8 |
| `result_geometry[221]` | 5.673118944415815e0 | 5.673118771175247e0 | 3.05e-8 |
| `result_geometry[224]` | -8.535953990996873e0 | -8.535953626197246e0 | 4.27e-8 |
| `result_geometry[225]` | 6.56059790921124e0 | 6.560597502391114e0 | 6.20e-8 |

The other 509 values of the record, the topology and the mass properties agree.

**How the old result was reproduced.** Main before #2735 (47184ba3e) is the old construction,
upstream's split at `TParam`, and passes the case against the same pinned reference (CI run
41988264776). Locally, the two distances above were measured by printing `split_parameter`'s
candidates while running the case on main 554443501 with `difftest --features cpp --unit
blend.fillet_executive_p3 --filter corner_box-edge9`; the C++ value in the table is the split at
`TParam`, the Rust value the split at the nearer parameter.

**Recorded by** the follow-up to #2735 for tangency-labs/solidcore#2794 (the WR-751 addendum
`plan/upstream-issues/WR-751--addendum-2026-11-05-difftest-cases.md` lists the six cases of the
same change where the C++ is wrong by its own check).

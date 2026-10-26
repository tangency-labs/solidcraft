# blend.fillet_executive_p3: bottle edge 29 at r=0.1, one vertex tolerance differs (2026-10-26)

Unit: `blend.fillet_executive_p3` (real-solid end-to-end runner).

Inputs (`real_solid`): `{"source": {"kind": "file", "path": {"$upstream_file":
"Fillet/bottle.smb"}}, "radius": 0.1, "xsect": 1, "continuity": 1, "thumbweight": 1,
"edges": [29]}`.

The fillet reaches `SmFilletCorner::CalcCornerEdgeGeom`'s global-merge branch: a failed
`SM_FE_FILLET_X_SIDE_FACE` corner edge is deleted from the pseudo-B-rep
(`SmBrep::DeleteEdge`, now `FilletBrep::delete_wire_fillet_edge`). Before that deletion
was ported the case was deferred.

Both sides succeed with identical topology and 842 of 843 values agreeing; the one
difference is `values.result_geometry[107]`, the tolerance of the result's 27th vertex
(`util::primitive::append` writes x, y, z, tolerance per vertex):

| | value |
|---|---|
| C++ | `1.0747627626592576e-2` |
| Rust | `1.6040861374381612e-4` |

The same edge at r=2.0, and 21 other DeleteEdge cases (bottle e4/e6/e7/e8/e16/e19/e27/
e30/e32, sphere_diff_3cylinders e1/e2/e4/e5 at r=0.1/0.5/2.0), agree exactly
(`fixtures/blend.fillet_executive_p3/stubs_round4.json`, `stubs4-delete-edge-*`).

Suspected cause: the merge's vertex tolerance growth (a vertex the C++ widens to about
1e-2 while merging the fillet faces into the target) rather than the deletion itself,
since every point and the topology agree. Not investigated further. The case is not
committed as a fixture: it would fail the gate, and it is not a known C++ defect. Add it
back with the fix.

Found in the round-4 stub sweep of the fillet executive p1 integration branch.

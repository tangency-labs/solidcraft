# geom.tools (partial; no disagreements)

- **913 cases, 721 pass, 192 deferred** (`topology.vertex` 60, `topology.brep_p1` 64,
  `curve.curve_p2` 68). `FindFirstPointWithinTolToPnt` is fully compared; the vertex,
  coincident-edge, edge and face searches are deferred until vertex/edge tolerances, edge
  geometry, curve evaluation/drops, `SmBrep::GetEdges`/`GetTolerance` and
  `SmTopologySolver::BrepPointSolve` land. Their C++ side already runs on real B-reps
  (`cone`, `sphere`, `disk`, `plane_square`, `torus`, `box_with_large_fillet`, `Mouse`,
  `plane_circ` `.smb`); the Rust runner searches placeholder edges because it cannot read
  `.smb` yet. Whoever ports `.smb` reading should switch the `coincident`/`edge`/`face`
  cases of `crates/difftest/src/units/geom_tools.rs` to the real B-rep.
- **`FindFirstPointWithinTolToPnt` never reports a double hit.** Both branches of the
  `closest < 0.5 * second` test set `rbIsFoundArg = TRUE` (comment: two loops may touch at
  a vertex), so `found` is just `closest < tol`. Ported as is.
- **`FindCoincidentEdge` leaks a rejected candidate's orientation**: `rbSameOrient` is
  written before the max-distance check, and with no edge found `rbSameOrient` /
  `rlFoundIndex` keep whatever they held. The port returns `None`; the shim reports them
  only on a hit.
- **`SmBrep::ReadFromFile` crashes on old (`DB version 30`) ASCII files**: `SmallBox.smb`
  and `Platform.smb` (pt_TestFiles) dereference a NULL `SmShellData` in
  `SmTopologyData::ReadFromDB` (`SmBrepData.cpp:4307`), or fail with a non-success status,
  depending on heap state. They are not used as fixtures.

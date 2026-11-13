# primitive.api_primitives: box, cylinder, cone and pyramid stop at SmSurface::DropCurve (2026-10-22)

- The `GlueVertices` vertex-list stub and the 36 heal.stitch_p2 count mismatches were
  already resolved on main by the #441 follow-up (root cause: `GlueEdgesGeneral` did not
  take the glued edge off `Brep::edge_list`; see "topology.brep_p4: the last stubs").
  On main d2028ca56: heal.stitch_p2 556 cases, 505 pass, 0 failed, 51 deferred.
- Every valid solid primitive (`box_5_10_20`, `cone_5_10_20`, `cyl_box_valid`,
  `pyramid_valid`, `box_far_origin`, `cylinder_brep_copy`, `cone_brep_copy`) is now
  deferred on `SmSurface::DropCurve` (unit `intersect.surface_drop_lib_p3`, `todo`, one
  2062-line function). The site reached is `topology/edgeuse/p3.rs`
  `surface_drop_curve` (`SmEdgeuse::CreateUVTrimCurve`), for all four primitives. The
  passing box/cylinder/cone/pyramid cases are input-validation error statuses only.
- `crates/solidcore/examples/primitives` (and the ignored test
  `crates/solidcore/tests/primitives_solid.rs`) check closed manifold solid, Euler-Poincare,
  volume, area and bounding box against the analytic values once the build succeeds.
  No STL: upstream's open SMLib has no B-rep face tessellation to a mesh.

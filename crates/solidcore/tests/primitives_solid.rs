//! The `primitives` example's checks as a test: box, cylinder, cone, pyramid, sphere and
//! torus are closed manifold solids with the analytic Euler characteristic, volume, area
//! and bounding box.

#[path = "../examples/primitives/checks.rs"]
#[allow(dead_code)] // the example also uses its per-check detail (--json)
mod checks;

#[test]
#[ignore = "blocked: SmSurface::DropCurve (intersect.surface_drop_lib_p3) is unported; \
            SmEdgeuse::CreateUVTrimCurve stops every primitive build"]
fn primitives_are_closed_solids_with_analytic_properties() {
    let failures: Vec<String> = checks::specs()
        .iter()
        .filter_map(|s| checks::check(s).err().map(|e| format!("{}: {e}", s.name)))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
#[ignore = "blocked: SmSurface::DropCurve (intersect.surface_drop_lib_p3, PR #463); with it, \
            sphere and torus stop at SmTol::GetZoneTol3d of a ray-fire solution object \
            (geom.tol, find_infinite_region), the no-pole sphere at SmEdge::GetTolerance in \
            stitching (topology.edge) and the partial ones at SmFace::CurveOnClassify \
            (query.face_p2, PrimitiveModel::axis_inside_face)"]
fn spheres_and_tori_are_closed_solids_with_analytic_properties() {
    let failures: Vec<String> = checks::curved_specs()
        .iter()
        .filter_map(|s| checks::check(s).err().map(|e| format!("{}: {e}", s.name)))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

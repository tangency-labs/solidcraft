//! The `primitives` example's checks as a test: box, cylinder, cone and pyramid are
//! closed manifold solids with the analytic Euler characteristic, volume, area and
//! bounding box.

#[path = "../examples/primitives/checks.rs"]
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

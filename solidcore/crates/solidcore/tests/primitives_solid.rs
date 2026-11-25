//! The `primitives` example's checks as a test: box, cylinder, cone, pyramid, sphere and
//! torus are closed manifold solids with the analytic Euler characteristic, volume, area
//! and bounding box.

#[path = "../examples/primitives/checks.rs"]
#[allow(dead_code)] // the example also uses its per-check detail (--json)
mod checks;

#[test]
fn complete_spheres_and_tori_have_analytic_properties() {
    let specs = checks::curved_specs();
    for name in ["sphere r3", "sphere r3 (no pole)", "torus R5 r1.5"] {
        let spec = specs.iter().find(|spec| spec.name == name).unwrap();
        checks::check(spec).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn primitives_are_closed_solids_with_analytic_properties() {
    let failures: Vec<String> = checks::specs()
        .iter()
        .filter_map(|s| checks::check(s).err().map(|e| format!("{}: {e}", s.name)))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn precise_sphere_properties_scale_and_translate() {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::mass::integrator::properties::{
        brep_properties, MassPropertyBehavior, PropertyAttributes,
    };
    use solidcore::primitive::api_primitives::{
        create_sphere, create_sphere_no_pole_with_tolerance,
    };
    use solidcore::query::brep::{calculate_bounding_box, BoundingBoxOptions};
    use solidcore::{math::Vec3, Kernel};

    // At radius 3 the analytic area and volume are equal, so also test radii
    // that distinguish the two properties and their different scaling laws.
    for (center, radius) in [(Vec3::ZERO, 2.0), (Vec3::new(1.0, -2.0, 0.5), 5.0)] {
        for no_pole in [false, true] {
            // 1.0.1's error scales cannot converge at 1e-10 on a sphere (WR-472)
            let mut k = Kernel::default();
            let mut b = if no_pole {
                create_sphere_no_pole_with_tolerance(&mut k, center, radius, 1e-9)
            } else {
                create_sphere(&mut k, center, radius)
            }
            .unwrap();
            let bbox = calculate_bounding_box(&k, &b, BoundingBoxOptions::default()).unwrap();
            let store = AttributeStore::default();
            let key_of = |_| ObjectKey(0);
            let properties = brep_properties(
                &k,
                &mut b,
                1e-10,
                bbox.mid(),
                MassPropertyBehavior::default(),
                &PropertyAttributes {
                    store: &store,
                    key_of: &key_of,
                },
            )
            .unwrap();
            let area = 4.0 * std::f64::consts::PI * radius * radius;
            let volume = area * radius / 3.0;
            assert!((properties.area - area).abs() <= checks::REL_TOL * area);
            assert!((properties.volume - volume).abs() <= checks::REL_TOL * volume);
        }
    }
}

#[test]
fn partial_spheres_and_tori_have_analytic_properties() {
    let specs = checks::curved_specs();
    for name in ["partial sphere r3 0..90", "partial torus R5 r1.5 0..90"] {
        let spec = specs.iter().find(|spec| spec.name == name).unwrap();
        checks::check(spec).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn pole_free_sphere_tolerance_is_explicit_and_validated() {
    use solidcore::primitive::api_primitives::{
        create_sphere_no_pole, create_sphere_no_pole_with_tolerance,
    };
    use solidcore::{Kernel, SolidError};
    let center = solidcore::math::Vec3::ZERO;
    let mut k = Kernel::default();
    for tolerance in [
        0.0,
        -1e-9,
        solidcore::math::EFF_ZERO,
        f64::NAN,
        f64::INFINITY,
    ] {
        assert!(matches!(
            create_sphere_no_pole_with_tolerance(&mut k, center, 3.0, tolerance),
            Err(SolidError::InvalidInput { .. })
        ));
    }
    let default = create_sphere_no_pole(&mut k, center, 3.0).unwrap();
    let explicit = create_sphere_no_pole_with_tolerance(&mut k, center, 3.0, 1e-9).unwrap();
    assert_eq!(default.tolerance(), 1e-5);
    assert_eq!(explicit.tolerance(), 1e-9);
    assert_eq!(default.face_list().len(), explicit.face_list().len());
    assert_eq!(default.edge_list().len(), explicit.edge_list().len());
    assert_eq!(default.vertex_list().len(), explicit.vertex_list().len());
    for (id, edgeuse) in explicit.edgeuses().iter() {
        let Some(face) = solidcore::topology::edgeuse::p1::face(&explicit, id).unwrap() else {
            continue;
        };
        let edge = explicit.edge(edgeuse.edge.unwrap()).unwrap();
        let curve = edge.curve.as_ref().unwrap();
        let trim = solidcore::topology::edgeuse::p1::uv_trim_curve(&explicit, id)
            .unwrap()
            .0
            .unwrap();
        let surface = explicit.face(face).unwrap().surface.as_ref().unwrap();
        for sample in 0..=32 {
            let t = edge.interval.evaluate(f64::from(sample) / 32.0);
            let uv = trim.evaluate_point(&k, t).unwrap();
            let on_surface = surface
                .evaluate_point(&k, solidcore::math::Point2::new(uv.x, uv.y))
                .unwrap();
            let on_edge = curve.evaluate_point(&k, t).unwrap();
            assert!(on_surface.distance(on_edge) <= 1e-9, "{id:?} at {t}");
        }
    }
    let next_default = create_sphere_no_pole(&mut k, center, 3.0).unwrap();
    assert_eq!(next_default.tolerance(), default.tolerance());
}

/// tangency-labs/solidcraft#18634: a box is judged by its size, not by its distance from
/// the origin. Each cube here is many times the B-rep tolerance (1e-5) on every side and
/// its far corner is exact in f64 (`c + s` rounds to within 1e-6 of the true sum at every
/// offset below), so it exists; before the fix `create_box` refused the rows at about
/// 1e-9 times the offset and below ("surface degenerates to a curve/point", whose floors
/// scaled with `1 + |coord|`), and built the rows between that and about
/// `sqrt(1e-12 * offset)` with faces the level-2 check reported as crossing or missing a
/// seam (`is_periodic`'s position test scaled the same way).
#[test]
fn boxes_far_from_the_origin_build_closed_and_valid() {
    use solidcore::math::Point3;
    use solidcore::primitive::api_primitives::{create_box, create_cylindrical_box};
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    use solidcore::Kernel;

    // sides from 2e-4 (above the level-2 check's degenerate-point tolerance, 1e-4, as at
    // the origin) to the 5e3 cube at 1e12 of the issue
    let rows: [(f64, &[f64]); 7] = [
        (1e3, &[2e-4, 1e-2, 1.0]),
        (1e4, &[2e-4, 1e-3]),
        (1e5, &[2e-4, 3e-4, 1e-2]),
        (1e6, &[2e-4, 1e-3, 1.5e-3]),
        (1e7, &[1e-3, 1e-2]),
        (1e8, &[1e-3, 1e-2, 1e-1]),
        (1e12, &[5e3]),
    ];
    let mut failures = Vec::new();
    for (c, sides) in rows {
        for &s in sides {
            let at = Point3::new(c, c, c);
            // the exact rounding error of the far corner `c + s` (Knuth's TwoSum)
            let sum = c + s;
            let back = sum - c;
            assert!(
                ((c - (sum - back)) + (s - back)).abs() < 1e-6,
                "{s:e} at {c:e}"
            );
            let k = &mut Kernel::default();
            match create_box(k, at, s, s, s) {
                Err(e) => failures.push(format!("box {s:e} at {c:e}: {e}")),
                Ok(mut b) => {
                    let counts = (
                        b.face_list().len(),
                        b.edge_list().len(),
                        b.vertex_list().len(),
                    );
                    let check = check_brep(k, &mut b, CheckLevel::Level2).unwrap();
                    if counts != (6, 12, 8) || !check.is_valid() {
                        failures.push(format!(
                            "box {s:e} at {c:e}: {counts:?} {:?}",
                            check.lines()
                        ));
                    }
                }
            }
        }
    }
    // quarter cylindrical boxes (length, inner and outer radius) at the same offsets: their
    // cylinder faces' tangent test scaled with the corner's position too
    for c in [1e3, 1e6, 1e8] {
        for (length, inner, outer) in [(1.0, 1.0, 2.0), (1e-3, 1.0, 2.0), (1.0, 1e-3, 2e-3)] {
            let at = Point3::new(c, c, c);
            let k = &mut Kernel::default();
            let row = format!("cylindrical box {length:e} x {inner:e}..{outer:e} at {c:e}");
            match create_cylindrical_box(k, at, length, inner, outer, 0.0, 90.0) {
                Err(e) => failures.push(format!("{row}: {e}")),
                Ok(mut b) => {
                    let faces = b.face_list().len();
                    let check = check_brep(k, &mut b, CheckLevel::Level2).unwrap();
                    if faces != 6 || !check.is_valid() {
                        failures.push(format!("{row}: {faces} {:?}", check.lines()));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// tangency-labs/solidcraft#18634, the issue's second table: cubes of 1e-4 and 5e-5 at
/// (1e4, 1e4, 1e4), at or below the level-2 check's degenerate-point tolerance (1e-4).
/// Their answer is the one the same cube gets at the origin (the same face, edge and
/// vertex counts and level-2 verdict, or the same refusal), so where the cube is does not
/// decide it.
#[test]
fn small_boxes_at_1e4_answer_as_at_the_origin() {
    use solidcore::math::Point3;
    use solidcore::primitive::api_primitives::create_box;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    use solidcore::Kernel;

    let answer = |at: Point3, s: f64| -> Result<((usize, usize, usize), bool), String> {
        let k = &mut Kernel::default();
        let mut b = create_box(k, at, s, s, s).map_err(|e| e.to_string())?;
        let counts = (
            b.face_list().len(),
            b.edge_list().len(),
            b.vertex_list().len(),
        );
        Ok((
            counts,
            check_brep(k, &mut b, CheckLevel::Level2)
                .unwrap()
                .is_valid(),
        ))
    };
    for s in [1e-4, 5e-5] {
        let near = answer(Point3::new(0.0, 0.0, 0.0), s);
        let far = answer(Point3::new(1e4, 1e4, 1e4), s);
        assert_eq!(far, near, "{s:e}");
    }
}

//! #2349: a real teardrop prism reaches the closed corner's non-G1 angle path.
//!
//! One cubic, with no internal kink, has coincident endpoints and tangents
//! (60,-30,0), (-60,-30,0). Extrusion retains a single closed rim and a seam.
//! Run the ignored future contract with `--ignored` to reproduce the old failure.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_corner::p3::FilletCornerKind;
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::{FilletEdgeType, FilletVertexType};
use solidcore::curve::curve::Curve;
use solidcore::kernel::UpstreamFixes;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

// Match #2499's generic-blend profile; its geometric-G2 experiment is separate.
fn new_blend_kernel() -> Kernel {
    let mut k = Kernel::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *k.fixes.flag_mut(name).unwrap() = true;
        }
    }
    k
}

fn teardrop(k: &mut Kernel) -> Brep {
    let points = [
        Point3::ZERO,
        Point3::new(20.0, -10.0, 0.0),
        Point3::new(20.0, 10.0, 0.0),
        Point3::ZERO,
    ];
    let profile = Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 3,
            control_points: &points,
            form: BSplineCurveForm::default(),
            knot_multiplicities: &[4, 4],
            knots: &[0.0, 1.0],
            knot_type: KnotType::default(),
            weights: None,
            trim: None,
        })
        .unwrap(),
    );
    let b = create_taper_extrude(k, &[Some(&profile)], 10.0, 0.0, 3, false).unwrap();
    // Explicit regeneration of the shared Rust/C++ input fixture, if requested.
    if let Ok(path) = std::env::var("SOLIDCORE_2349_WRITE_FIXTURE") {
        use solidcore::geom::attribute::{AttributeStore, ObjectKey};
        use solidcore::topology::brep_data_write::{write_brep_to_file, FromBrepEnv};
        use solidcore::topology::database_io::FileType;
        use solidcore::topology::topology::NotifyRecords;
        let attributes = AttributeStore::new();
        let records = NotifyRecords::new();
        let env = FromBrepEnv {
            attributes: &attributes,
            records: &records,
            key_of: &|e| b.owner_identity(e),
            curve_key: &|_| ObjectKey(0),
            uv_curve_key: &|_| ObjectKey(0),
            surface_key: &|_| ObjectKey(0),
        };
        std::fs::write(
            path,
            write_brep_to_file(k, &b, &env, FileType::Ascii).unwrap(),
        )
        .unwrap();
    }
    b
}

fn top_rim(k: &Kernel, b: &Brep) -> EdgeId {
    *b.edge_list()
        .iter()
        .find(|&&e| {
            let c = edge::curve(b, e).unwrap().unwrap();
            let ivl = edge::interval(b, e).unwrap();
            let a = c.evaluate_point(k, ivl.min()).unwrap();
            let z = c.evaluate_point(k, ivl.max()).unwrap();
            (a.z - 10.0).abs() < 1e-8 && (z.z - 10.0).abs() < 1e-8
        })
        .unwrap()
}

#[test]
fn teardrop_reaches_the_real_closed_angle_topology() {
    let mut k = Kernel::default();
    let mut b = teardrop(&mut k);
    assert_eq!(
        (
            b.face_list().len(),
            b.edge_list().len(),
            b.vertex_list().len()
        ),
        (3, 3, 2)
    );
    assert!(check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines()
        .is_empty());
    let rim = top_rim(&k, &b);
    assert_eq!(
        edge::start_vertex(&b, rim).unwrap(),
        edge::end_vertex(&b, rim).unwrap()
    );
    let c = edge::curve(&b, rim).unwrap().unwrap();
    let ivl = edge::interval(&b, rim).unwrap();
    let a = c.evaluate(&k, ivl.min(), 1, true, true).unwrap()[1];
    let z = c.evaluate(&k, ivl.max(), 1, true, true).unwrap()[1];
    assert!((a.dot(z) / (a.length() * z.length()) + 0.6).abs() < 1e-12);
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_constant_radius_edges(&mut k, &[rim], 0.5, 1, 1, 1.0)
        .unwrap();
    s.create_corners(&k, None).unwrap();
    let ids = s.fillet_corners().unwrap();
    assert_eq!(ids.len(), 1);
    let corner = s.corner(ids[0]).unwrap();
    assert_eq!(corner.kind, FilletCornerKind::NByOneClosed);
    assert_eq!(corner.solver_eus.as_ref().unwrap().len(), 1);
    assert_eq!(corner.all_eus.as_ref().unwrap().len(), 2);
    s.create_corner_vertices(&mut k).unwrap();
    let corner = s.corner(ids[0]).unwrap();
    assert!(!corner.g1_case);
    assert!(!corner.blending);
    assert_eq!(corner.vertices.len(), 2);
    assert_eq!(corner.edges.len(), 1);
    let pseudo = s.pseudo_brep();
    assert_eq!(
        pseudo
            .fillet_edge(corner.edges[0])
            .unwrap()
            .fillet_edge_type(),
        FilletEdgeType::FilletXFillet
    );
    assert!(corner
        .vertices
        .iter()
        .any(|&v| pseudo.fillet_vertex(v).unwrap().fillet_vertex_type()
            == FilletVertexType::FilletXFillet));
    assert!(pseudo.geoms().iter().any(|g| g.need_split));
}

#[test]
fn small_closed_angle_round_builds_a_valid_body() {
    for mut k in [Kernel::default(), new_blend_kernel()] {
        let mut b = teardrop(&mut k);
        let rim = top_rim(&k, &b);
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_constant_radius_edges(&mut k, &[rim], 0.5, 1, 1, 1.0)
            .unwrap();
        s.do_fillet(&mut k).unwrap();
        drop(s);
        assert_eq!(b.face_list().len(), 5);
        assert!(check_brep(&mut k, &mut b, CheckLevel::Level2)
            .unwrap()
            .lines()
            .is_empty());
    }
}

#[test]
fn closed_angle_solver_still_allocates_unlinked_zero_tolerance_split_vertices() {
    for mut k in [Kernel::default(), new_blend_kernel()] {
        let mut b = teardrop(&mut k);
        let rim = top_rim(&k, &b);
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_constant_radius_edges(&mut k, &[rim], 0.5, 1, 1, 1.0)
            .unwrap();
        s.create_corners(&k, None).unwrap();
        s.process_smooth_edge_sequences(&mut k).unwrap();
        s.create_corner_vertices(&mut k).unwrap();
        s.adjust_corner_topology(&mut k).unwrap();
        let id = s.fillet_corners().unwrap()[0];
        let original = s.corner(id).unwrap().vertices.clone();
        assert!(s.pseudo_brep().geoms().iter().any(|g| g.need_split));
        s.calculate_solver_geometry(&mut k).unwrap();
        let split: Vec<_> = s
            .corner(id)
            .unwrap()
            .vertices
            .iter()
            .copied()
            .filter(|v| !original.contains(v))
            .collect();
        assert_eq!(split.len(), 2, "the closed round must split both rails");
        for v in split {
            let pseudo = s.pseudo_brep().brep();
            assert_eq!(pseudo.vertex(v).unwrap().tolerance, 0.0);
            assert!(!pseudo.vertex_list().contains(&v));
        }
        s.calculate_corner_edges(&mut k).unwrap();
        assert!(s.corner(id).unwrap().blend.is_none());
        assert_eq!(s.corner(id).unwrap().kind, FilletCornerKind::NByOneClosed);
    }
}

#[test]
#[ignore = "#2349: exact closed-angle blend event requires certified curved-spine self-mitre (#2307 merged)"]
fn closed_angle_is_a_blend_event_instead_of_the_legacy_corner() {
    let mut k = Kernel::default();
    let mut b = teardrop(&mut k);
    let rim = top_rim(&k, &b);
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_constant_radius_edges(&mut k, &[rim], 0.5, 1, 1, 1.0)
        .unwrap();
    s.create_corners(&k, None).unwrap();
    let ids = s.fillet_corners().unwrap();
    assert_eq!(ids.len(), 1);
    assert!(
        s.corner(ids[0]).unwrap().blend.is_some(),
        "closed non-G1 rim still uses the legacy Nx1Closed construction"
    );
}

#[test]
#[ignore = "#2349: closed mitre event and sized refusal require certified self-mitre and fit-size proof (#2307 merged)"]
fn closed_angle_round_builds_validly_or_refuses_with_a_fit_size() {
    let mut k = Kernel::default();
    k.fixes.fillet_gn_section_generator = true;
    let mut b = teardrop(&mut k);
    let rim = top_rim(&k, &b);
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_constant_radius_edges(&mut k, &[rim], 3.0, 1, 1, 1.0)
        .unwrap();
    let result = s.do_fillet(&mut k);
    if result.is_err() {
        let fit = s.fit_size();
        eprintln!(
            "closed angle: {result:?}; fit={fit:?}; {:?}",
            s.error_info()
        );
        assert!(
            fit.is_some(),
            "closed angle failed without a fit size: {result:?}; {:?}",
            s.error_info()
        );
        let size = fit.unwrap().size;
        assert!(size.is_finite() && size > 0.0 && size < 3.0);
        drop(s);
        // A refusal must name a size that actually builds on a fresh operand.
        let mut smaller = teardrop(&mut k);
        let rim = top_rim(&k, &smaller);
        let mut retry = FilletSession::new(&mut k, &mut smaller).unwrap();
        retry
            .prepare_constant_radius_edges(&mut k, &[rim], size, 1, 1, 1.0)
            .unwrap();
        retry.do_fillet(&mut k).unwrap();
        drop(retry);
        let lines = check_brep(&mut k, &mut smaller, CheckLevel::Level2)
            .unwrap()
            .lines();
        assert!(
            lines.is_empty(),
            "suggested radius {size} builds invalidly: {lines:?}"
        );
    } else {
        drop(s);
        let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
            .unwrap()
            .lines();
        assert!(lines.is_empty(), "{lines:?}");
    }
}

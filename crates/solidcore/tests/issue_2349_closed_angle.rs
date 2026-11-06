//! #2349: a real teardrop prism reaches the closed corner's non-G1 angle path.
//!
//! One cubic, with no internal kink, has coincident endpoints and tangents
//! (60,-30,0), (-60,-30,0). Extrusion retains a single closed rim and a seam.
//! Its tip is the blend corner's closed Turn, Mitred (#2349): the round meets
//! itself on its mitre, as two rounds turning at a side edge do.
//! Run the ignored contract with `--ignored` to reproduce the refusal at radius 3
//! (#2404).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::blend_corner::{BlendCorner, BlendCornerEvent, BlendTurn};
use solidcore::blend::fillet_corner::p3::{FilletCorner, FilletCornerKind};
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::{FilletCornerType, FilletEdgeType, FilletVertexType};
use solidcore::curve::curve::Curve;
use solidcore::kernel::UpstreamFixes;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, vertex, EdgeId};
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
    assert_closed_turn(corner);
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
        assert_closed_turn(s.corner(id).unwrap());
    }
}

/// The blend corner's closed Turn, Mitred: one closed blend edge use meeting itself at
/// an angle, a blend corner (`NByTwo` by type) like the closed Join.
fn assert_closed_turn(corner: &FilletCorner) {
    assert_eq!(
        corner.blend,
        Some(BlendCorner {
            event: BlendCornerEvent::Turn,
            turn: BlendTurn::Mitre,
            ..BlendCorner::default()
        })
    );
    assert_eq!(corner.kind, FilletCornerKind::NByTwo);
    assert_eq!(corner.corner_type(), FilletCornerType::NByTwo);
    assert!(corner.is_closed_turn());
    assert!(!corner.is_closed_join());
}

#[test]
fn closed_angle_is_the_blend_corners_mitred_turn() {
    let mut k = Kernel::default();
    let mut b = teardrop(&mut k);
    let rim = top_rim(&k, &b);
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_constant_radius_edges(&mut k, &[rim], 0.5, 1, 1, 1.0)
        .unwrap();
    s.create_corners(&k, None).unwrap();
    let ids = s.fillet_corners().unwrap();
    assert_eq!(ids.len(), 1);
    assert_closed_turn(s.corner(ids[0]).unwrap());
}

#[test]
#[ignore = "#2404: the fold refusal names its closed bound 1.875 (less rounding), where the round does not build"]
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

/// The teardrop's profile, the cubic with poles (0,0), (20,-10), (20,10), (0,0).
fn gamma(t: f64) -> (f64, f64) {
    let s = 1.0 - t;
    let b = [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t];
    let p = [(0.0, 0.0), (20.0, -10.0), (20.0, 10.0), (0.0, 0.0)];
    b.iter()
        .zip(p)
        .fold((0.0, 0.0), |(x, y), (w, (px, py))| (x + w * px, y + w * py))
}

/// The in-plane distance from (x, y) to the profile over its parameters in [lo, hi]:
/// the nearest of 20000 samples, refined by golden section to rounding.
fn distance_to_profile(x: f64, y: f64, lo: f64, hi: f64) -> f64 {
    let d = |t: f64| {
        let (gx, gy) = gamma(t);
        (gx - x).hypot(gy - y)
    };
    let n = 20000;
    let h = (hi - lo) / f64::from(n);
    let t0 = (0..=n)
        .map(|i| lo + h * f64::from(i))
        .min_by(|&a, &b| d(a).total_cmp(&d(b)))
        .unwrap();
    let (mut a, mut b) = ((t0 - h).max(lo), (t0 + h).min(hi));
    for _ in 0..200 {
        let m1 = a + (b - a) * 0.381_966_011_250_105;
        let m2 = a + (b - a) * 0.618_033_988_749_895;
        if d(m1) < d(m2) {
            b = m2;
        } else {
            a = m1;
        }
    }
    d(0.5 * (a + b))
}

/// How far `p` is from each end of the true round of radius r: the part over the
/// profile's first half (t in [0, 1/2]) and over its second. The round between the top
/// plane z = 10 and the wall is the ball's circle in each plane square to the profile:
/// a point at height z is on it where its distance to the profile is the circle's inset
/// r (1 - cos a), sin a = (z - 10 + r) / r. On the mitre both are 0.
fn off_the_ends(p: Point3, r: f64) -> (f64, f64) {
    let sin_a = ((p.z - 10.0 + r) / r).clamp(-1.0, 1.0);
    let inset = r * (1.0 - (1.0 - sin_a * sin_a).sqrt());
    (
        (distance_to_profile(p.x, p.y, 0.0, 0.5) - inset).abs(),
        (distance_to_profile(p.x, p.y, 0.5, 1.0) - inset).abs(),
    )
}

/// The closed Turn's mitre of radius r on the filleted teardrop: its side vertex (the
/// one nearest (0, 0, 10 - r)), the mitre edge from it to the top face, and the body.
fn closed_mitre(k: &mut Kernel, r: f64) -> (Brep, Point3, EdgeId) {
    let mut b = teardrop(k);
    let rim = top_rim(k, &b);
    let mut s = FilletSession::new(k, &mut b).unwrap();
    s.prepare_constant_radius_edges(k, &[rim], r, 1, 1, 1.0)
        .unwrap();
    s.do_fillet(k).unwrap();
    drop(s);
    let side = Point3::new(0.0, 0.0, 10.0 - r);
    let v = *b
        .vertex_list()
        .iter()
        .min_by(|&&a, &&c| {
            let da = vertex::point(&b, a).unwrap().distance(side);
            da.total_cmp(&vertex::point(&b, c).unwrap().distance(side))
        })
        .unwrap();
    let mitre = vertex::edges(&b, v, None)
        .unwrap()
        .into_iter()
        .find(|&e| {
            let w = edge::other_vertex(&b, e, Some(v)).unwrap().unwrap();
            vertex::point(&b, w).unwrap().z > 10.0 - 1e-6
        })
        .expect("the mitre runs from the side vertex to the top face");
    let at = vertex::point(&b, v).unwrap();
    (b, at, mitre)
}

/// The closed Turn's side vertex by its closed form. The round's rail on the wall is
/// the ball's foot at the height of its centre, z = 10 - r (the wall is vertical), and
/// it crosses the seam, the wall's line through the tip (0, 0): at (0, 0, 10 - r).
#[test]
fn closed_mitres_side_vertex_is_where_the_rail_crosses_the_seam() {
    for r in [0.5, 1.0, 1.5] {
        for mut k in [Kernel::default(), new_blend_kernel()] {
            let (_, at, _) = closed_mitre(&mut k, r);
            assert!(
                at.distance(Point3::new(0.0, 0.0, 10.0 - r)) <= 1e-12,
                "r {r}: side vertex {at:?}"
            );
        }
    }
}

/// The mitre is where the round meets itself: every point of it is on both ends of the
/// true round (`off_the_ends`), to the mitre edge's own tolerance. The teardrop is
/// symmetric about the plane y = 0, so the mitre also lies on that plane.
///
/// Known failing (plan/findings/blend.fillet_corner_p3--issue-2349-closed-round-start-span.md):
/// the round's first span from the tip is off the true round by up to 7.9e-4 at r 0.5
/// (its start derivative is tilted away from the profile's tangent between the rails),
/// so the mitre, on the other end, is that far off this one.
#[test]
#[ignore = "#2349: the closed round's first span from the tip is off the true round (finding issue-2349-closed-round-start-span)"]
fn closed_mitre_is_on_both_ends_of_the_round() {
    for r in [0.5, 1.0, 1.5] {
        let mut k = new_blend_kernel();
        let (b, _, mitre) = closed_mitre(&mut k, r);
        let tol = edge::tolerance(&b, mitre).unwrap();
        let c = edge::curve(&b, mitre).unwrap().unwrap();
        let ivl = edge::interval(&b, mitre).unwrap();
        for i in 0..=32 {
            let p = c
                .evaluate_point(&k, ivl.evaluate(f64::from(i) / 32.0))
                .unwrap();
            let (first, second) = off_the_ends(p, r);
            assert!(
                first <= tol && second <= tol && p.y.abs() <= tol,
                "r {r}: mitre point {p:?} is {first:e} off the first end, {second:e} off the second (tolerance {tol:e})"
            );
        }
    }
}

//! WR-685 law-corner addendum (#2273; the fork's tangency-labs/smlib 7babe17, 58d03ef and
//! 62fcc82 in 5e8c9c1): a variable round whose radius law has a corner is the union of
//! its balls there. Each side is its own exact piece, ending on the corner ball's touch
//! circle of its own side through the same two rail points. Where the law dips, the
//! circle after lies ahead of the one before and the ball's sphere between them is a face
//! of its own (`sphere_zone`); where it peaks, the sides cross.
//!
//! The corner's geometry (`solidcore::blend::law_corners`) is checked here against closed
//! forms and the fork's build; the round is skinned piece by piece between the law's
//! corners, split there (tangency-labs/smlib@6eecf04 and 5fbffc9) and built there
//! (`BuildLawCorners`, 7babe17), and checked as the union of its balls, with the fork's
//! capi cases as a cross-check of the volume.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::definition_check::{bare_blend_faces, BlendFaceLog};
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSolverType;
use solidcore::blend::fillet_standard_solver::p1::{CircularCrossSectionFsg, FilletLaw};
use solidcore::blend::law_corners::{
    curve_between, find_law_corner_pieces, has_built_law_corners, sphere_zone, ArcApproximation,
    LawCorner, LawCornerKind, LawCornerPieces,
};
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::vector2d::Point2;
use solidcore::intersect::point_class::{PointClassType, PointClassification};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::p1::{
    BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
};
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::nurbs::knots::Side;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::surface_closest_point;
use solidcore::query::face::p2::{point_classify, PointClassifyOptions};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p1::calculate_uv_domain_from_uv_trim_curves;
use solidcore::topology::{edge, EdgeId, FaceId};
use solidcore::{Brep, Kernel};

// ---- the corner ball's zone at a dip ------------------------------------------------------

/// The dip corner of the law 4 -> 2 -> 4 on the 20 cube's edge y = z = 20, at x = 10: the
/// corner ball's centre and radius, and the two rail points.
const CENTRE: Point3 = Point3::new(10.0, 18.0, 18.0);
const ONE: Point3 = Point3::new(10.0, 18.0, 20.0);
const TWO: Point3 = Point3::new(10.0, 20.0, 18.0);

/// The middles of the two sides' touch arcs at the corner, as the fork's scratch driver
/// computed them: the side before lies in the plane `(p - CENTRE).(1, 0.2, 0.2) = 0.4`
/// (the envelope's `<p - c, c'> = -r r'`, r falling 0.2 a unit), the side after in
/// `(p - CENTRE).(1, -0.2, -0.2) = -0.4`.
const ON_FIRST: Point3 = Point3::new(9.8362146258571865, 19.409463435357033, 19.409463435357033);
const ON_SECOND: Point3 = Point3::new(10.163785374142813, 19.409463435357033, 19.409463435357033);

/// The fork's `sm_SphereZone` for those points (5e8c9c1 built locally, the function
/// copied into a scratch driver as it is static there): the control net row by row in u,
/// Euclidean, and its weights.
const FORK_NET: [(f64, f64, f64, f64); 9] = [
    (10.0, 20.0, 18.0, 8.0),
    (9.5999999999999996, 20.0, 20.0, 1.9611613513818402),
    (10.0, 18.0, 20.0, 1.0),
    (10.0, 20.0, 18.0, 7.8446454055273609),
    (10.0, 20.0, 20.0, 2.0),
    (10.0, 18.0, 20.0, 0.98058067569092011),
    (10.0, 20.0, 18.0, 8.0),
    (10.4, 20.0, 20.0, 1.9611613513818402),
    (10.0, 18.0, 20.0, 1.0),
];

#[test]
fn a_dips_zone_is_the_fork_s_patch_bit_for_bit() {
    let zone = sphere_zone(CENTRE, 2.0, ONE, TWO, ON_FIRST, ON_SECOND).unwrap();
    // the zone keeps its ball
    assert_eq!((zone.centre(), zone.radius()), (CENTRE, 2.0));
    let net = zone.net().control_point_net().unwrap();
    assert_eq!((net.u_count, net.v_count), (3, 3));
    for (i, &(x, y, z, w)) in FORK_NET.iter().enumerate() {
        let p = net.points[i];
        assert_eq!(
            [
                p.x.to_bits(),
                p.y.to_bits(),
                p.z.to_bits(),
                net.weights[i].to_bits()
            ],
            [x.to_bits(), y.to_bits(), z.to_bits(), w.to_bits()],
            "control point {i}: {p:?} w {} against the fork's ({x}, {y}, {z}) w {w}",
            net.weights[i]
        );
    }
}

#[test]
fn a_dips_zone_lies_on_the_corner_ball_between_its_touch_circles() {
    let zone = sphere_zone(CENTRE, 2.0, ONE, TWO, ON_FIRST, ON_SECOND).unwrap();
    let zone = zone.net();
    let before = Vec3::new(1.0, 0.2, 0.2);
    let after = Vec3::new(1.0, -0.2, -0.2);
    for i in 0..=8 {
        for j in 0..=8 {
            let (u, v) = (f64::from(i) / 8.0, f64::from(j) / 8.0);
            let x = zone.evaluate_point(Point2::new(u, v)).unwrap();
            // on the ball
            assert!(
                ((x - CENTRE).length() - 2.0).abs() < 1e-13,
                "{u} {v}: {x:?}"
            );
            // the u ends are the two touch circles
            if i == 0 {
                assert!(((x - CENTRE).dot(before) - 0.4).abs() < 1e-13, "{v}: {x:?}");
            }
            if i == 8 {
                assert!(((x - CENTRE).dot(after) + 0.4).abs() < 1e-13, "{v}: {x:?}");
            }
            // the v ends are poles at the rail points
            if j == 0 {
                assert!(x.distance(TWO) < 1e-13, "{u}: {x:?}");
            }
            if j == 8 {
                assert!(x.distance(ONE) < 1e-13, "{u}: {x:?}");
            }
            // between the circles: ahead of the first, behind the second along the edge
            assert!(
                x.x >= ON_FIRST.x - 1e-12 && x.x <= ON_SECOND.x + 1e-12,
                "{x:?}"
            );
        }
    }
    // the ends' arcs run through the arcs' middles given
    for (u, on) in [(0.0, ON_FIRST), (1.0, ON_SECOND)] {
        let near = (0..=1000)
            .map(|j| {
                let v = f64::from(j) / 1000.0;
                zone.evaluate_point(Point2::new(u, v)).unwrap().distance(on)
            })
            .fold(f64::INFINITY, f64::min);
        assert!(near < 1e-2, "{u}: {near}");
    }
}

#[test]
fn a_zone_needs_two_rail_points_and_a_wedge_under_a_half_turn() {
    // one point twice
    assert!(sphere_zone(CENTRE, 2.0, ONE, ONE, ON_FIRST, ON_SECOND).is_err());
    // the two arcs of one circle between the rail points: inverted, opposite rays from the
    // image of the second point, a half turn
    let n = Vec3::new(1.0, 0.2, 0.2);
    let circle_centre = CENTRE + n * (0.4 / n.dot(n));
    let other_arc = circle_centre * 2.0 - ON_FIRST;
    assert!(sphere_zone(CENTRE, 2.0, ONE, TWO, ON_FIRST, other_arc).is_err());
    // the given pair is well inside a half turn
    assert!(sphere_zone(CENTRE, 2.0, ONE, TWO, ON_FIRST, ON_SECOND).is_ok());
    // a ball with no radius
    assert!(sphere_zone(CENTRE, 0.0, ONE, TWO, ON_FIRST, ON_SECOND).is_err());
}

#[test]
fn a_crossing_curve_runs_from_the_edge_s_start_to_its_end() {
    let k = Kernel::default();
    let (a, b) = (Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 2.0, 3.0));
    let line = Curve::from(BSplineCurve::create_line_segment(3, a, b).unwrap());
    let same = curve_between(&k, &line, a, b, 1e-6)
        .unwrap()
        .expect("forward");
    let ivl = same.natural_interval();
    assert_eq!(same.evaluate_point(&k, ivl.min()).unwrap(), a);
    let back = curve_between(&k, &line, b, a, 1e-6)
        .unwrap()
        .expect("reversed");
    let ivl = back.natural_interval();
    assert!(back.evaluate_point(&k, ivl.min()).unwrap().distance(b) < 1e-15);
    assert!(back.evaluate_point(&k, ivl.max()).unwrap().distance(a) < 1e-15);
    // ends within the tolerance only
    let off = Point3::new(1.0, 2.0, 3.0 + 1e-3);
    assert!(curve_between(&k, &line, a, off, 1e-6).unwrap().is_none());
    assert!(curve_between(&k, &line, a, off, 1e-2).unwrap().is_some());
}

// ---- each corner's kind, from the round's pieces ------------------------------------------

/// The degree-1 curve through `points` at the parameters `at`.
fn polyline(points: &[Point3], at: &[f64]) -> BSplineCurve {
    let mut mults = vec![1; at.len()];
    mults[0] = 2;
    *mults.last_mut().unwrap() = 2;
    BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 1,
        control_points: points,
        form: BSplineCurveForm::default(),
        knot_multiplicities: &mults,
        knots: at,
        knot_type: KnotType::default(),
        weights: None,
        trim: None,
    })
    .unwrap()
}

/// A round on the 20 cube's edge y = z = 20 whose law is linear between the radii `r` at
/// x = 0, 10 and 20: the centre line and the rails on y = 20 and z = 20, each parameter
/// x, whole and per piece.
fn law_curves(r: [f64; 3]) -> ([BSplineCurve; 3], Vec<[BSplineCurve; 3]>) {
    let x = [0.0, 10.0, 20.0];
    let centre = |i: usize| Point3::new(x[i], 20.0 - r[i], 20.0 - r[i]);
    let rail1 = |i: usize| Point3::new(x[i], 20.0, 20.0 - r[i]);
    let rail2 = |i: usize| Point3::new(x[i], 20.0 - r[i], 20.0);
    let whole = |f: &dyn Fn(usize) -> Point3| polyline(&[f(0), f(1), f(2)], &x);
    let piece = |f: &dyn Fn(usize) -> Point3, i: usize| polyline(&[f(i), f(i + 1)], &x[i..i + 2]);
    let pieces = (0..2)
        .map(|i| [piece(&centre, i), piece(&rail1, i), piece(&rail2, i)])
        .collect();
    ([whole(&centre), whole(&rail1), whole(&rail2)], pieces)
}

const APPROX_TOL: f64 = 1e-5;

/// The fork's section: exact arcs (its `SmArcSection` default).
const EXACT: ArcApproximation = ArcApproximation {
    approximate_arcs: false,
    tol_percent_radius: 0.05,
};

/// The round's pieces skinned on the balls' touch circles (the G^n section flag on, a
/// variable-radius solver), joined as the fork's `CircleSurface` joins them, and each
/// corner classified.
fn law_corner_pieces(k: &Kernel, r: [f64; 3]) -> (LawCornerPieces, BSplineSurface) {
    let ([center_line, rail1, _], pieces) = law_curves(r);
    let generator = CircularCrossSectionFsg {
        approximate_arcs: false,
        ..CircularCrossSectionFsg::default()
    };
    let mut surfaces = Vec::new();
    let mut joined: Option<BSplineSurface> = None;
    let mut joined_ends = Vec::new();
    for [mut c, mut r1, mut r2] in pieces {
        let mut piece = generator
            .create_surface(
                k,
                APPROX_TOL,
                FilletSolverType::VariableRadius,
                &mut c,
                &mut r1,
                &mut r2,
            )
            .unwrap();
        surfaces.push(Surface::from(piece.clone()));
        joined = Some(match joined {
            None => piece,
            Some(mut j) => j.join_surface(&mut piece, SurfParamType::U, 0.0).unwrap(),
        });
        joined_ends.push(joined.as_ref().unwrap().natural_uv_domain().max().x);
    }
    let kept = LawCornerPieces::classify(
        k,
        &center_line,
        &rail1,
        &[0.0, 10.0, 20.0],
        surfaces,
        joined_ends,
        APPROX_TOL,
        EXACT,
    )
    .unwrap();
    (kept, joined.unwrap())
}

fn gn_kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_gn_section_generator = true;
    k
}

#[test]
fn a_law_that_dips_has_the_corner_ball_s_zone_between_its_sides() {
    let k = gn_kernel();
    let (kept, _) = law_corner_pieces(&k, [4.0, 2.0, 4.0]);
    assert_eq!(
        kept.corners,
        vec![LawCorner {
            kind: LawCornerKind::Dip,
            centre: CENTRE,
            radius: 2.0
        }]
    );
    assert_eq!(kept.tol_3d, APPROX_TOL);
    assert!(kept.has_dip_or_peak());
    // the sides end on the touch circles the zone runs between
    let before = &kept.pieces[0];
    let after = &kept.pieces[1];
    for (side, at_max, plane, rhs) in [
        (before, true, Vec3::new(1.0, 0.2, 0.2), 0.4),
        (after, false, Vec3::new(1.0, -0.2, -0.2), -0.4),
    ] {
        let d = side.natural_uv_domain_with_nurb(&k).unwrap();
        let u = if at_max { d.max().x } else { d.min().x };
        for j in 0..=8 {
            let v = d.min().y + (d.max().y - d.min().y) * f64::from(j) / 8.0;
            let x = side.evaluate_point(&k, Point2::new(u, v)).unwrap();
            assert!(((x - CENTRE).length() - 2.0).abs() < 1e-9, "{x:?}");
            assert!(((x - CENTRE).dot(plane) - rhs).abs() < 1e-9, "{x:?}");
        }
    }
}

#[test]
fn a_law_that_peaks_has_sides_that_run_past_each_other() {
    let k = gn_kernel();
    let (kept, _) = law_corner_pieces(&k, [2.0, 4.0, 2.0]);
    assert_eq!(
        kept.corners,
        vec![LawCorner {
            kind: LawCornerKind::Peak,
            centre: Point3::new(10.0, 16.0, 16.0),
            radius: 4.0
        }]
    );
}

#[test]
fn a_constant_radius_corner_matches() {
    let k = gn_kernel();
    let (kept, _) = law_corner_pieces(&k, [3.0, 3.0, 3.0]);
    let kinds: Vec<_> = kept.corners.iter().map(|corner| corner.kind).collect();
    assert_eq!(kinds, vec![LawCornerKind::Matching]);
    assert!(!kept.has_dip_or_peak());
    assert!(!has_built_law_corners(std::slice::from_ref(&kept)));
}

#[test]
fn approximated_arcs_widen_the_pieces_accuracy() {
    let k = gn_kernel();
    let (kept, _) = law_corner_pieces(&k, [4.0, 2.0, 4.0]);
    let ([center_line, rail1, _], _) = law_curves([4.0, 2.0, 4.0]);
    let approx = LawCornerPieces::classify(
        &k,
        &center_line,
        &rail1,
        &[0.0, 10.0, 20.0],
        kept.pieces.clone(),
        kept.joined_ends.clone(),
        APPROX_TOL,
        ArcApproximation {
            approximate_arcs: true,
            tol_percent_radius: 0.05,
        },
    )
    .unwrap();
    // 0.05 percent of the corner ball's radius 2
    assert_eq!(approx.tol_3d, 0.01 * 0.05 * 2.0);
}

#[test]
fn a_rounds_pieces_are_found_by_its_corners_once() {
    let k = gn_kernel();
    let (dip, joined) = law_corner_pieces(&k, [4.0, 2.0, 4.0]);
    let (peak, _) = law_corner_pieces(&k, [2.0, 4.0, 2.0]);
    let mut kept = vec![peak, dip];
    assert!(has_built_law_corners(&kept));
    let joined = Surface::from(joined);
    // the joined surface turns where both laws have their corner: the first not taken
    assert_eq!(
        find_law_corner_pieces(&k, &mut kept, &joined).unwrap(),
        Some(0)
    );
    assert!(kept[0].taken && !kept[1].taken);
    assert_eq!(
        find_law_corner_pieces(&k, &mut kept, &joined).unwrap(),
        Some(1)
    );
    assert_eq!(
        find_law_corner_pieces(&k, &mut kept, &joined).unwrap(),
        None
    );
    // a surface that does not turn there has none
    let mut fresh = vec![law_corner_pieces(&k, [4.0, 2.0, 4.0]).0];
    let smooth = fresh[0].pieces[0].clone();
    assert_eq!(
        find_law_corner_pieces(&k, &mut fresh, &smooth).unwrap(),
        None
    );
    assert!(!fresh[0].taken);
}

// ---- the fork's capi cases (not built yet) -------------------------------------------------
//
// At the time of the port (solidcore main f5a031c9c), every fix on, the three cases are
// refused at the law's corner: the trace's rails run smoothly across it (no through point
// there, 6eecf04), and the fold check (`sm_BallsFold`) reads that as balls folding "where
// its radius is 2" (the dip; 2.5 for the first peak), status 1001. With the pinned
// upstream (no fix) the dip removes 35.1156 against the union of the balls' 39.5421, the
// first peak 19.1640 against 21.7999 (seven faces each), and the dips case builds an
// invalid body.

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

/// The edge from `p` to `q`, and whether it runs from `q` to `p`.
fn edge_between(k: &Kernel, b: &Brep, p: Point3, q: Point3) -> (EdgeId, bool) {
    b.edges()
        .ids()
        .find_map(|e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let s = c.evaluate_point(k, ivl.min()).unwrap();
            let t = c.evaluate_point(k, ivl.max()).unwrap();
            if s.distance(p) < 1e-9 && t.distance(q) < 1e-9 {
                Some((e, false))
            } else if s.distance(q) < 1e-9 && t.distance(p) < 1e-9 {
                Some((e, true))
            } else {
                None
            }
        })
        .expect("edge")
}

/// The fork capi's `law_along`: the B-spline law of `degree` with the full `knots` and
/// control radii `radii`, the radius as x and the parameter as y (control points at the
/// Greville abscissae), reversed when the edge runs the other way.
fn spline_law(degree: usize, knots: &[f64], radii: &[f64], reverse: bool) -> FilletLaw {
    let n = knots.len();
    let kv: Vec<f64> = (0..n)
        .map(|i| {
            if reverse {
                1.0 - knots[n - 1 - i]
            } else {
                knots[i]
            }
        })
        .collect();
    let control: Vec<Vec3> = (0..radii.len())
        .map(|i| {
            let g: f64 = (1..=degree).map(|j| kv[i + j]).sum::<f64>() / degree as f64;
            let r = radii[if reverse { radii.len() - 1 - i } else { i }];
            Vec3::new(r, g, 0.0)
        })
        .collect();
    let mut unique: Vec<f64> = Vec::new();
    let mut mults: Vec<usize> = Vec::new();
    for &t in &kv {
        if unique.last() == Some(&t) {
            *mults.last_mut().unwrap() += 1;
        } else {
            unique.push(t);
            mults.push(1);
        }
    }
    let curve = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree,
        control_points: &control,
        form: BSplineCurveForm::default(),
        knot_multiplicities: &mults,
        knots: &unique,
        knot_type: KnotType::default(),
        weights: None,
        trim: None,
    })
    .unwrap();
    FilletLaw::bspline(curve, &Extent1d::from_min_max(0.0, 1.0))
}

/// The box `size` after the law rounds its edge from `from` to `to` (the law's start at
/// `from`): the volume removed and the face count, as `smlib_fillet_variable` runs it,
/// every fix on (the fork's own behaviour).
fn law_round(
    size: [f64; 3],
    from: Point3,
    to: Point3,
    degree: usize,
    knots: &[f64],
    radii: &[f64],
) -> (f64, usize) {
    let (k, mut b, _) = law_body(size, from, to, degree, knots, radii);
    let faces = b.faces().ids().count();
    (size[0] * size[1] * size[2] - volume(&k, &mut b), faces)
}

/// The tolerances a law-cornered round is checked to: the largest distance any of its
/// session's solvers approximates to (`approx_tol_3d`), and the kernel's angle between
/// parallel vectors (`SM_ANG_TOL_DEG`, the solvers' own angle being 20 degrees).
#[derive(Debug, Clone, Copy)]
struct SolverTols {
    distance: f64,
    angle: f64,
}

/// The box `size` with its edge from `from` to `to` rounded by the law, as
/// [`law_round`] makes it, with the blend face log on, and the tolerances it is checked to.
fn law_body(
    size: [f64; 3],
    from: Point3,
    to: Point3,
    degree: usize,
    knots: &[f64],
    radii: &[f64],
) -> (Kernel, Brep, SolverTols) {
    use solidcore::blend::fillet_geom::p1::FilletSolverId;
    let mut k = Kernel::default();
    k.fixes.clone_from(&UpstreamFixes::all());
    // (the log changes no result: issue_2489_blend_faces_are_definitions)
    k.blend_face_log = Some(BlendFaceLog::default());
    let mut b = create_box(&mut k, Vec3::ZERO, size[0], size[1], size[2]).unwrap();
    let (e, reversed) = edge_between(&k, &b, from, to);
    let law = spline_law(degree, knots, radii, reversed);
    let base = radii[0].max(radii[radii.len() - 1]);
    let mut tols = SolverTols {
        distance: 0.0,
        angle: k.tolerances.ang_tol_deg.to_radians(),
    };
    {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_variable_radius_edges(&mut k, &[e], base, law, 1, 1)
            .unwrap();
        session.do_fillet(&mut k).unwrap();
        for id in 0.. {
            let Ok(s) = session.solver(FilletSolverId(id)) else {
                break;
            };
            tols.distance = tols.distance.max(s.base().approx_tol_3d);
        }
    }
    (k, b, tols)
}

/// How far `p` is from the envelope of the balls `ball(t)` (centre, radius) for `t` over
/// `[t0, t1]`: `min_t |p - c(t)| - r(t)`, by 4001 samples refined by golden sections on
/// the best sample's neighbourhood. Zero on the envelope, below zero inside a ball.
fn off_the_balls(p: Point3, t0: f64, t1: f64, ball: impl Fn(f64) -> (Point3, f64)) -> f64 {
    nearest_ball(p, t0, t1, ball).0
}

/// [`off_the_balls`], and the `t` of the ball `p` is nearest (the one it touches, on the
/// envelope).
fn nearest_ball(p: Point3, t0: f64, t1: f64, ball: impl Fn(f64) -> (Point3, f64)) -> (f64, f64) {
    let d = |t: f64| {
        let (c, r) = ball(t);
        p.distance(c) - r
    };
    let n = 4000;
    let step = (t1 - t0) / f64::from(n);
    let (mut best, mut at) = (f64::INFINITY, t0);
    for i in 0..=n {
        let t = t0 + step * f64::from(i);
        if d(t) < best {
            best = d(t);
            at = t;
        }
    }
    let (mut a, mut b) = ((at - step).max(t0), (at + step).min(t1));
    let g = (5.0_f64.sqrt() - 1.0) / 2.0;
    for _ in 0..80 {
        let (x1, x2) = (b - g * (b - a), a + g * (b - a));
        if d(x1) < d(x2) {
            b = x2;
        } else {
            a = x1;
        }
    }
    let refined = 0.5 * (a + b);
    if d(refined) < best {
        (d(refined), refined)
    } else {
        (best, at)
    }
}

/// The round's kept pieces (the solvers' `law_corner_pieces`) with the solvers'
/// tolerance, from the box `size` with its edge from `from` to `to` rounded by the law, as
/// [`law_body`] makes it.
fn law_pieces(
    size: [f64; 3],
    from: Point3,
    to: Point3,
    knots: &[f64],
    radii: &[f64],
) -> (Kernel, Vec<Surface>, f64) {
    use solidcore::blend::fillet_geom::p1::FilletSolverId;
    let mut k = Kernel::default();
    k.fixes.clone_from(&UpstreamFixes::all());
    let mut b = create_box(&mut k, Vec3::ZERO, size[0], size[1], size[2]).unwrap();
    let (e, reversed) = edge_between(&k, &b, from, to);
    let law = spline_law(1, knots, radii, reversed);
    let base = radii[0].max(radii[radii.len() - 1]);
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_variable_radius_edges(&mut k, &[e], base, law, 1, 1)
        .unwrap();
    session.do_fillet(&mut k).unwrap();
    let mut pieces = Vec::new();
    let mut tol = 0.0_f64;
    for id in 0.. {
        let Ok(s) = session.solver(FilletSolverId(id)) else {
            break;
        };
        tol = tol.max(s.base().approx_tol_3d);
        for kept in &s.base().law_corner_pieces {
            pieces.extend(kept.pieces.iter().cloned());
        }
    }
    drop(session);
    (k, pieces, tol)
}

/// The worst of [`off_the_balls`] over a 21 x 11 grid of each of `pieces`' natural
/// domains, each piece against its own side's balls: those over `[t0, corner]` for a piece
/// whose middle is before `corner` along the edge (`along`), else over `[corner, t1]`, at
/// its points over that side.
fn worst_piece_off_the_balls(
    k: &Kernel,
    pieces: &[Surface],
    t0: f64,
    corner: f64,
    t1: f64,
    along: impl Fn(Point3) -> f64,
    ball: impl Fn(f64) -> (Point3, f64),
) -> (f64, usize) {
    let mut worst = 0.0_f64;
    let mut points = 0;
    for piece in pieces {
        let d = piece.natural_uv_domain_with_nurb(k).unwrap();
        let middle = along(piece.evaluate_point(k, d.evaluate(0.5, 0.5)).unwrap());
        let (t0, t1) = if middle < corner {
            (t0, corner)
        } else {
            (corner, t1)
        };
        for i in 0..=20 {
            for j in 0..=10 {
                let uv = d.evaluate(f64::from(i) / 20.0, f64::from(j) / 10.0);
                let p = piece.evaluate_point(k, uv).unwrap();
                if !(t0..=t1).contains(&along(p)) {
                    continue;
                }
                worst = worst.max(off_the_balls(p, t0, t1, &ball).abs());
                points += 1;
            }
        }
    }
    (worst, points)
}

/// The piecewise skin of the fork's 6eecf04 (before its law corners, 7babe17): each side
/// of a law corner skinned on its own, its sections the balls' touch circles of its own
/// side, the sides joined C0 at the corner and kept as their exact pieces. Each piece is
/// the envelope of its side's balls: on the 20 cube's edge y = z = 20 the ball at x (its
/// faces y = 20 and z = 20, the material below) has centre (x, 20 - r(x), 20 - r(x)) and
/// radius r(x), the law 4 -> 2 -> 4 linear in x either side of x = 10 (the same from
/// either end); at the peak 2 -> 3 -> 2 on the 10 x 20 x 30 box's edge x = 10, z = 30
/// along y, centre (10 - r, y, 30 - r). Every point of a piece over its side of the edge
/// is on its side's envelope, never inside one of its side's balls, to the solvers'
/// tolerance (the fork's faces, which are these pieces, are within 7.6e-10). At a peak a
/// side runs on inside the other side's balls near the corner (the sides cross there,
/// where `BuildLawCorners` ends them). The joined surface is not the envelope: at a dip the sides
/// end on two different touch circles and the join takes one boundary row between them,
/// which `BuildLawCorners` mends by putting the pieces back on the split faces.
#[test]
fn a_law_cornered_rounds_pieces_are_its_balls_envelope() {
    let lin = |a: f64, m: f64, z: f64, t: f64| {
        if t <= 10.0 {
            a + (m - a) * t / 10.0
        } else {
            m + (z - m) * (t - 10.0) / 10.0
        }
    };
    let (a, b) = (Point3::new(0.0, 20.0, 20.0), Point3::new(20.0, 20.0, 20.0));
    for (from, to) in [(a, b), (b, a)] {
        let (k, pieces, tol) = law_pieces(
            [20.0; 3],
            from,
            to,
            &[0.0, 0.0, 0.5, 1.0, 1.0],
            &[4.0, 2.0, 4.0],
        );
        let ball = |x: f64| {
            let r = lin(4.0, 2.0, 4.0, x);
            (Point3::new(x, 20.0 - r, 20.0 - r), r)
        };
        let (worst, points) =
            worst_piece_off_the_balls(&k, &pieces, 0.0, 10.0, 20.0, |p| p.x, ball);
        eprintln!("dip from {from:?}: {} pieces, {points} points, worst {worst:.3e} off the balls, tol {tol:e}", pieces.len());
        assert_eq!(pieces.len(), 2);
        assert!(points > 100, "{points}");
        assert!(worst <= tol, "{worst}");
    }
    let (k, pieces, tol) = law_pieces(
        [10.0, 20.0, 30.0],
        Point3::new(10.0, 0.0, 30.0),
        Point3::new(10.0, 20.0, 30.0),
        &[0.0, 0.0, 0.5, 1.0, 1.0],
        &[2.0, 3.0, 2.0],
    );
    let ball = |y: f64| {
        let r = lin(2.0, 3.0, 2.0, y);
        (Point3::new(10.0 - r, y, 30.0 - r), r)
    };
    let (worst, points) = worst_piece_off_the_balls(&k, &pieces, 0.0, 10.0, 20.0, |p| p.y, ball);
    eprintln!(
        "peak: {} pieces, {points} points, worst {worst:.3e} off the balls, tol {tol:e}",
        pieces.len()
    );
    assert_eq!(pieces.len(), 2);
    assert!(points > 100, "{points}");
    assert!(worst <= tol, "{worst}");
}

/// The split of the fork's 6eecf04 (`NoteRadiusCornerSurfaces`,
/// `SplitFilletFacesAtRadiusCorners`): a round joined C0 at its law's corner is split there
/// into one face per side, each smooth (no C0 knot inside its surface), which SMLib's check
/// refuses inside a face; the corner is then built (7babe17), with a dip's zone between the
/// sides. On the dip from either end and on a peak: no turn inside any face, the box's six
/// faces, one per side and the dip's zone, and the body clean at check level 2.
#[test]
fn a_law_cornered_round_is_split_at_its_corner() {
    let (a, b) = (Point3::new(0.0, 20.0, 20.0), Point3::new(20.0, 20.0, 20.0));
    let cases = [
        ([20.0; 3], a, b, [4.0, 2.0, 4.0], 1usize),
        ([20.0; 3], b, a, [4.0, 2.0, 4.0], 1),
        (
            [10.0, 20.0, 30.0],
            Point3::new(10.0, 0.0, 30.0),
            Point3::new(10.0, 20.0, 30.0),
            [2.0, 3.0, 2.0],
            0,
        ),
    ];
    for (size, from, to, radii, zones) in cases {
        let (mut k, mut body, _) = law_body(size, from, to, 1, &[0.0, 0.0, 0.5, 1.0, 1.0], &radii);
        let lines = check_brep(&mut k, &mut body, CheckLevel::Level2)
            .unwrap()
            .lines();
        let mut rounds = Vec::new();
        for f in body.faces().ids() {
            let s = body.face(f).unwrap().surface.clone().unwrap();
            if !matches!(s.kind(), SurfaceKind::Plane(_)) {
                rounds.push(s);
            }
        }
        let mut turns = 0;
        for s in &rounds {
            let (u, v) = s
                .find_knots_by_max_continuity(&k, ContinuityType::C0, false)
                .unwrap();
            turns += u.len() + v.len();
        }
        let faces = body.faces().ids().count();
        eprintln!(
            "split {radii:?} from {from:?}: {faces} faces, {} round faces, {turns} turns inside; {lines:?}",
            rounds.len(),
        );
        assert!(lines.is_empty(), "{lines:?}");
        assert_eq!((faces, rounds.len(), turns), (8 + zones, 2 + zones, 0));
    }
}

/// The edge a law rounds: the 20 cube's edge y = z = 20 along x (its faces y = 20 and
/// z = 20, the material below), or the 10 x 20 x 30 box's edge x = 10, z = 30 along y.
#[derive(Debug, Clone, Copy)]
enum LawEdge {
    Cube,
    Block,
}

impl LawEdge {
    /// How far along the edge `p` is.
    fn along(self, p: Point3) -> f64 {
        match self {
            Self::Cube => p.x,
            Self::Block => p.y,
        }
    }

    /// The ball of radius `r` at `t` along the edge: its centre, the edge's two faces
    /// moved in by `r`, and `r`.
    fn ball(self, t: f64, r: f64) -> (Point3, f64) {
        match self {
            Self::Cube => (Point3::new(t, 20.0 - r, 20.0 - r), r),
            Self::Block => (Point3::new(10.0 - r, t, 30.0 - r), r),
        }
    }
}

/// The radius at `t` of the law linear between the radii `at` (each `(t, r)`, in order).
fn piecewise_linear(at: &[(f64, f64)], t: f64) -> f64 {
    let i = at
        .windows(2)
        .position(|w| t <= w[1].0)
        .unwrap_or(at.len() - 2);
    let ((t0, r0), (t1, r1)) = (at[i], at[i + 1]);
    r0 + (r1 - r0) * (t - t0) / (t1 - t0)
}

/// Points inside face `f`: a 21 x 21 grid over its trims' uv box, kept where the face
/// classifies them inside.
fn face_points(k: &Kernel, b: &mut Brep, f: FaceId) -> Vec<Point3> {
    let bx = calculate_uv_domain_from_uv_trim_curves(k, b, f, true, false, false)
        .unwrap()
        .domain;
    let s = b.face(f).unwrap().surface.clone().unwrap();
    let mut out = Vec::new();
    for i in 0..=20 {
        for j in 0..=20 {
            let uv = bx.evaluate(f64::from(i) / 20.0, f64::from(j) / 20.0);
            let mut pc = PointClassification::new(1e-6);
            point_classify(k, b, f, uv, &mut pc, PointClassifyOptions::default()).unwrap();
            if pc.point_class() == PointClassType::Face {
                out.push(s.evaluate_point(k, uv).unwrap());
            }
        }
    }
    out
}

/// The unit normal of face `f`'s surface at its point nearest `p`, either way round.
fn surface_normal_at(k: &Kernel, b: &Brep, f: FaceId, p: Point3) -> Vec3 {
    let s = b.face(f).unwrap().surface.clone().unwrap();
    let near = surface_closest_point(k, &s, p).unwrap();
    s.evaluate_normal(k, near.uvs[0], Side::Left, Side::Left)
        .unwrap()
}

/// The angle between the lines along `a` and `b`.
fn line_angle(a: Vec3, b: Vec3) -> f64 {
    (a.dot(b).abs() / (a.length() * b.length())).min(1.0).acos()
}

/// jyviko's blend-definition rule (#2243, #2489): each face the blend made is a definition
/// (`blend::definition_check::is_definition`) or of a kind the ledger lists
/// (`data/blend_definition_ledger/`, one file per kind naming its follow-up issue).
fn assert_definitions_or_listed(name: &str, k: &Kernel, b: &Brep) {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/blend_definition_ledger"
    );
    let listed: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            e.unwrap()
                .path()
                .file_stem()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    let made = k
        .blend_face_log
        .as_ref()
        .expect("the blend face log")
        .last()
        .to_vec();
    assert!(!made.is_empty(), "{name}: the blend made no face");
    for f in bare_blend_faces(b, &made).unwrap() {
        let kind = f.kind.name().replace('/', "-");
        assert!(
            listed.contains(&kind),
            "{name}: bare face {} of kind {kind} ({}) is not in the ledger",
            f.face.index(),
            f.surface
        );
    }
}

/// One law-cornered round of [`a_law_cornered_round_is_its_balls_union`]: its name, the
/// box, the edge's ends (the law's start at the first), the law's degree, knots and control
/// radii, the edge, the law as radii at points along the edge, and its corners' kinds.
struct LawCase {
    name: &'static str,
    size: [f64; 3],
    from: Point3,
    to: Point3,
    degree: usize,
    knots: &'static [f64],
    radii: &'static [f64],
    edge: LawEdge,
    law: &'static [(f64, f64)],
    corners: &'static [LawCornerKind],
}

/// The round built at its law's corners (`BuildLawCorners`, the fork's 7babe17 with
/// 62fcc82 and 58d03ef): the union of its balls. The balls are closed form from the law:
/// at `t` along the edge, the centre is the edge's two faces moved in by r(t) and the
/// radius r(t) ([`LawEdge::ball`]). On the dip 4 -> 2 -> 4 from either end and as a
/// quadratic, the peak 2 -> 3 -> 2, and the dip then peak 4, 2, 4, 2:
/// - every point inside every round face is on the boundary of the union of all the balls
///   (`min_t |p - c(t)| - r(t)` zero to the solvers' tolerance, the law's end spans carried
///   on past the edge's ends as the round is): on its side's envelope and never inside a
///   ball of the other side, so at a peak each side ends where they cross;
/// - at a dip, the corner ball's zone is a face of its own (a ball patch) on the corner
///   ball, and the faces either side meet it G1: at every point of its two circles both
///   faces' normals are the corner ball's, to the kernel's angle between parallel vectors;
/// - at a peak the sides meet G0 along their crossing, at the crease the balls give: each
///   side's normal there is its own touching ball's (along `p - c(t)`, t the side's ball
///   through `p`), to that angle, so the crease angle is the angle between those
///   two, over twice the angle between parallel vectors;
/// - the body is clean at check level 2, with one face per side and one per dip;
/// - every face the round made is a definition or listed in the ledger (#2505): each side
///   its round by its definition, the law trimmed to the side, over the stations where the
///   law stays positive (the dip then peak's last side, whose law the trace's stations past
///   the edge's end take through zero, ends at the edge's end, where the end face cuts it).
#[test]
fn a_law_cornered_round_is_its_balls_union() {
    const A: Point3 = Point3::new(0.0, 20.0, 20.0);
    const B: Point3 = Point3::new(20.0, 20.0, 20.0);
    const DIP: &[(f64, f64)] = &[(0.0, 4.0), (10.0, 2.0), (20.0, 4.0)];
    const LINEAR: &[f64] = &[0.0, 0.0, 0.5, 1.0, 1.0];
    let dip = |name, from, to| LawCase {
        name,
        size: [20.0; 3],
        from,
        to,
        degree: 1,
        knots: LINEAR,
        radii: &[4.0, 2.0, 4.0],
        edge: LawEdge::Cube,
        law: DIP,
        corners: &[LawCornerKind::Dip],
    };
    let cases = [
        dip("dip", A, B),
        dip("dip, reversed", B, A),
        LawCase {
            name: "dip, quadratic",
            degree: 2,
            knots: &[0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
            radii: &[4.0, 3.0, 2.0, 3.0, 4.0],
            ..dip("", A, B)
        },
        LawCase {
            name: "peak",
            size: [10.0, 20.0, 30.0],
            from: Point3::new(10.0, 0.0, 30.0),
            to: Point3::new(10.0, 20.0, 30.0),
            degree: 1,
            knots: LINEAR,
            radii: &[2.0, 3.0, 2.0],
            edge: LawEdge::Block,
            law: &[(0.0, 2.0), (10.0, 3.0), (20.0, 2.0)],
            corners: &[LawCornerKind::Peak],
        },
        LawCase {
            name: "dip then peak",
            knots: &[0.0, 0.0, 0.25, 0.75, 1.0, 1.0],
            radii: &[4.0, 2.0, 4.0, 2.0],
            law: &[(0.0, 4.0), (5.0, 2.0), (15.0, 4.0), (20.0, 2.0)],
            corners: &[LawCornerKind::Dip, LawCornerKind::Peak],
            // (its last side's law, 4 -> 2 over the last quarter, carried on past the edge's
            // end as the trace's stations run, 10 past it, falls through zero at 25)
            ..dip("", A, B)
        },
    ];
    for case in cases {
        let LawCase { name, law, .. } = case;
        let (mut k, mut body, tols) = law_body(
            case.size,
            case.from,
            case.to,
            case.degree,
            case.knots,
            case.radii,
        );
        let ball = |t: f64| case.edge.ball(t, piecewise_linear(law, t));
        // the balls the round's points touch: past the edge's ends too, where the round runs
        // on with its law's end spans (cut by the end faces); a touching ball's centre is
        // within its radius of the point, so its t within the largest radius of the point's
        let reach = law.iter().map(|&(_, r)| r).fold(0.0, f64::max);
        let (lo, hi) = (law[0].0 - reach, law[law.len() - 1].0 + reach);
        let lines = check_brep(&mut k, &mut body, CheckLevel::Level2)
            .unwrap()
            .lines();
        assert!(lines.is_empty(), "{name}: {lines:?}");

        // the round's faces, each with its points, and the zones among them
        let mut rounds: Vec<(FaceId, Vec<Point3>, f64)> = Vec::new();
        let mut zones: Vec<FaceId> = Vec::new();
        for f in body.faces().ids().collect::<Vec<_>>() {
            let s = body.face(f).unwrap().surface.clone().unwrap();
            match s.kind() {
                SurfaceKind::Plane(_) => continue,
                SurfaceKind::BallPatch(_) => zones.push(f),
                _ => {}
            }
            let points = face_points(&k, &mut body, f);
            assert!(
                points.len() > 20,
                "{name}: face {} has {} points",
                f.index(),
                points.len()
            );
            let mid = points.iter().map(|&p| case.edge.along(p)).sum::<f64>() / points.len() as f64;
            rounds.push((f, points, mid));
        }
        let corner_at = |kind| -> Vec<f64> {
            law.windows(3)
                .zip(case.corners)
                .filter(|(_, &k)| k == kind)
                .map(|(w, _)| w[1].0)
                .collect()
        };
        let (dips, peaks) = (
            corner_at(LawCornerKind::Dip),
            corner_at(LawCornerKind::Peak),
        );
        eprintln!(
            "{name}: {} faces, {} round faces, {} zones; {tols:?}",
            body.faces().ids().count(),
            rounds.len(),
            zones.len()
        );
        assert_eq!(rounds.len(), law.len() - 1 + dips.len(), "{name}");
        assert_eq!(zones.len(), dips.len(), "{name}");

        // every round face on the boundary of the balls' union
        let mut worst = 0.0_f64;
        for (_, points, _) in &rounds {
            for &p in points {
                worst = worst.max(off_the_balls(p, lo, hi, ball).abs());
            }
        }
        eprintln!("{name}: worst {worst:.3e} off the balls' union");
        assert!(worst <= tols.distance, "{name}: {worst}");

        // each dip's zone on its corner ball, met G1 along its two circles
        for &t in &dips {
            let (centre, r) = ball(t);
            let &(zone, ref points, _) = rounds
                .iter()
                .find(|(f, _, mid)| zones.contains(f) && (mid - t).abs() < 1.0)
                .expect("the dip's zone");
            let off = points
                .iter()
                .map(|p| (p.distance(centre) - r).abs())
                .fold(0.0, f64::max);
            let (mut kink, mut circles) = (0.0_f64, 0);
            for e in solidcore::topology::face::p4::edges(&body, zone, None, false, None).unwrap() {
                let faces = edge::faces(&body, e, None).unwrap();
                let Some(&side) = faces.iter().find(|&&f| f != zone) else {
                    continue;
                };
                circles += 1;
                let c = edge::curve(&body, e).unwrap().unwrap();
                let ivl = edge::interval(&body, e).unwrap();
                for i in 1..16 {
                    let p = c
                        .evaluate_point(&k, ivl.evaluate(f64::from(i) / 16.0))
                        .unwrap();
                    for f in [side, zone] {
                        kink = kink.max(line_angle(surface_normal_at(&k, &body, f, p), p - centre));
                    }
                }
            }
            eprintln!(
                "{name}: the dip at {t}: the zone {off:.3e} off its ball, {circles} circles, kinked {kink:.3e} rad"
            );
            assert!(off <= tols.distance, "{name}: {off}");
            assert_eq!(circles, 2, "{name}");
            assert!(kink <= tols.angle, "{name}: {kink}");
        }

        // each peak's sides crossing at the balls' crease
        for &t in &peaks {
            // the faces either side of the peak, the one before first
            let side = |before: bool| {
                rounds
                    .iter()
                    .filter(|(f, _, mid)| !zones.contains(f) && (*mid < t) == before)
                    .min_by(|a, b| (a.2 - t).abs().total_cmp(&(b.2 - t).abs()))
                    .expect("a side of the peak")
                    .0
            };
            let (before, after) = (side(true), side(false));
            let crossing: Vec<EdgeId> = solidcore::topology::brep::p1::edges(&body, None)
                .unwrap()
                .into_iter()
                .filter(|&e| {
                    let faces = edge::faces(&body, e, None).unwrap();
                    faces.contains(&before) && faces.contains(&after)
                })
                .collect();
            assert_eq!(crossing.len(), 1, "{name}: the peak at {t}");
            let c = edge::curve(&body, crossing[0]).unwrap().unwrap();
            let ivl = edge::interval(&body, crossing[0]).unwrap();
            let (mut off, mut crease_lo, mut crease_hi) = (0.0_f64, f64::INFINITY, 0.0_f64);
            for i in 1..16 {
                let p = c
                    .evaluate_point(&k, ivl.evaluate(f64::from(i) / 16.0))
                    .unwrap();
                // each side's ball through p, and the normal it gives there
                let n0 = p - ball(nearest_ball(p, lo, t, ball).1).0;
                let n1 = p - ball(nearest_ball(p, t, hi, ball).1).0;
                let crease = line_angle(n0, n1);
                crease_lo = crease_lo.min(crease);
                crease_hi = crease_hi.max(crease);
                off = off.max(line_angle(surface_normal_at(&k, &body, before, p), n0));
                off = off.max(line_angle(surface_normal_at(&k, &body, after, p), n1));
            }
            eprintln!(
                "{name}: the peak at {t}: crease {crease_lo:.6} to {crease_hi:.6} rad, the faces' normals {off:.3e} off their balls'"
            );
            assert!(off <= tols.angle, "{name}: {off}");
            // a crease, not a tangent join: the faces cannot be parallel there
            assert!(crease_lo > 2.0 * tols.angle, "{name}: {crease_lo}");
        }

        // every face the round made is in the blend face log, so none escapes the check
        let made = k.blend_face_log.as_ref().unwrap().last();
        for (f, _, _) in &rounds {
            assert!(
                made.iter().any(|m| m.face == *f),
                "{name}: face {} is not in the blend face log",
                f.index()
            );
        }
        assert_definitions_or_listed(name, &k, &body);
    }
}

// The fork's capi cases, with the volume removed its 5e8c9c1 build gives through its C API
// (a scratch driver running the capi cases, at `smlib_brep_volume`'s 1e-7).

#[test]
fn law_peak() {
    // 2 -> w -> 2 on the 10 x 20 x 30 box's top edge x = 10, z = 30, from y = 0: the
    // block's six faces and the two sides' rounds, meeting where they cross
    for (w, want, fork) in [
        (2.5, 21.799882, 21.799877917178),
        (3.0, 27.093683, 27.093677260694),
        (3.2, 29.389966, 29.389911194516),
        (4.0, 39.541034, 39.541083296321),
    ] {
        let (removed, faces) = law_round(
            [10.0, 20.0, 30.0],
            Point3::new(10.0, 0.0, 30.0),
            Point3::new(10.0, 20.0, 30.0),
            1,
            &[0.0, 0.0, 0.5, 1.0, 1.0],
            &[2.0, w, 2.0],
        );
        eprintln!("peak {w}: removed {removed}, want {want} (fork {fork}), {faces} faces");
        assert!((removed - want).abs() <= 1e-4 * want, "{removed}");
        assert_eq!(faces, 8);
    }
}

#[test]
fn law_dip() {
    // 4 -> 2 -> 4 on the 20 cube's edge y = z = 20 from either end, and the same law as a
    // quadratic with a corner at 0.5: the cube's six faces, the two sides' rounds and the
    // corner ball's zone between them (the fork: 39.542055909305 linear, 39.542055908618
    // quadratic)
    let (a, b) = (Point3::new(0.0, 20.0, 20.0), Point3::new(20.0, 20.0, 20.0));
    for (from, to, degree, knots, radii) in [
        (
            a,
            b,
            1,
            &[0.0, 0.0, 0.5, 1.0, 1.0][..],
            &[4.0, 2.0, 4.0][..],
        ),
        (
            b,
            a,
            1,
            &[0.0, 0.0, 0.5, 1.0, 1.0][..],
            &[4.0, 2.0, 4.0][..],
        ),
        (
            a,
            b,
            2,
            &[0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0][..],
            &[4.0, 3.0, 2.0, 3.0, 4.0][..],
        ),
    ] {
        let (removed, faces) = law_round([20.0; 3], from, to, degree, knots, radii);
        eprintln!("dip: removed {removed}, want 39.542073, {faces} faces");
        assert!((removed - 39.542073).abs() <= 1e-4 * 39.542073, "{removed}");
        assert_eq!(faces, 9);
    }
}

#[test]
fn law_dips() {
    // 4, 2, 4, 2 at 0, 0.25, 0.75, 1 on the 20 cube's edge y = z = 20: a dip, then a peak;
    // the cube's six faces, three rounds and the zone at the dip (the fork:
    // 38.818141402826)
    let (removed, faces) = law_round(
        [20.0; 3],
        Point3::new(0.0, 20.0, 20.0),
        Point3::new(20.0, 20.0, 20.0),
        1,
        &[0.0, 0.0, 0.25, 0.75, 1.0, 1.0],
        &[4.0, 2.0, 4.0, 2.0],
    );
    eprintln!("dips: removed {removed}, want 38.818277, {faces} faces");
    assert!((removed - 38.818277).abs() <= 1e-4 * 38.818277, "{removed}");
    assert_eq!(faces, 10);
}

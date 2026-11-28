//! The Measure and Check families on solidcore's primitives: mass properties, area and bounds
//! against their closed forms ([`crate::measure::closed`]), one solid each, edges on their faces.

use super::{Body, area, bounds, check, edge_face_distance, is_manifold_solid, make_box, make_cone, make_cylinder, make_sphere, make_torus};
use super::{counts, mass_properties, solid_count};
use crate::check::CheckLevel;
use crate::measure::MEASURE_ACCURACY;
use crate::measure::closed::{self, Closed};

/// The tolerance every quantity meets, relative.
const TOL: f64 = 1e-9;

/// The primitives, as `convert_tests` makes them, with their closed forms.
fn case(name: &str) -> (Body, Closed) {
    let (body, c) = match name {
        "box" => (make_box([1.0, -2.0, 3.0], [10.0, 20.0, 30.0]), closed::cuboid([1.0, -2.0, 3.0], [10.0, 20.0, 30.0])),
        "cylinder" => (make_cylinder([1.0, 2.0, -3.0], 5.0, 12.0), closed::cylinder([1.0, 2.0, -3.0], 5.0, 12.0)),
        "sphere" => (make_sphere([1.0, 2.0, 3.0], 7.0), closed::sphere([1.0, 2.0, 3.0], 7.0)),
        "cone" => (make_cone([0.0, 1.0, 2.0], 6.0, 0.0, 9.0), closed::cone([0.0, 1.0, 2.0], 6.0, 0.0, 9.0)),
        "frustum" => (make_cone([0.0, 1.0, 2.0], 6.0, 2.0, 9.0), closed::cone([0.0, 1.0, 2.0], 6.0, 2.0, 9.0)),
        "torus" => (make_torus([1.0, -1.0, 0.5], 10.0, 3.0), closed::torus([1.0, -1.0, 0.5], 10.0, 3.0)),
        _ => panic!("{name}"),
    };
    (body.unwrap(), c)
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs()
}

/// The body's size: twice its largest radius of gyration, the scale of centroid errors.
fn size(c: &Closed) -> f64 {
    2.0 * (c.moments.iter().fold(0.0f64, |a, &b| a.max(b)) / c.volume).sqrt()
}

fn mass_properties_match(name: &str) {
    let (b, c) = case(name);
    let m = mass_properties(&b, MEASURE_ACCURACY).unwrap();
    assert!(rel(m.area, c.area) < TOL, "{name}: area {} vs {}", m.area, c.area);
    assert!(rel(m.volume, c.volume) < TOL, "{name}: volume {} vs {}", m.volume, c.volume);
    let imax = c.moments.iter().fold(0.0f64, |a, &b| a.max(b));
    for i in 0..3 {
        assert!((m.centroid[i] - c.centroid[i]).abs() < TOL * size(&c), "{name}: centroid {:?} vs {:?}", m.centroid, c.centroid);
        assert!(rel(m.moments[i], c.moments[i]) < TOL, "{name}: moments {:?} vs {:?}", m.moments, c.moments);
        assert!(m.products[i].abs() < TOL * imax, "{name}: products {:?}", m.products);
    }
}

fn area_matches(name: &str) {
    let (b, c) = case(name);
    let a = area(&b, MEASURE_ACCURACY).unwrap();
    assert!(rel(a, c.area) < TOL, "{name}: area {a} vs {}", c.area);
}

fn bounds_match(name: &str) {
    let (b, c) = case(name);
    let (lo, hi) = bounds(&b).unwrap();
    let s = size(&c);
    for i in 0..3 {
        assert!((lo[i] - c.bounds.0[i]).abs() < TOL * s, "{name}: low {lo:?} vs {:?}", c.bounds.0);
        assert!((hi[i] - c.bounds.1[i]).abs() < TOL * s, "{name}: high {hi:?} vs {:?}", c.bounds.1);
    }
}

fn one_solid(name: &str) {
    let (b, _) = case(name);
    assert_eq!(solid_count(&b).unwrap(), 1, "{name}");
}

fn manifold_solid(name: &str) {
    let (b, _) = case(name);
    assert!(is_manifold_solid(&b).unwrap(), "{name}");
}

/// Every edge lies on the faces it bounds: its distance to the nearest face is 0.
fn edges_lie_on_their_faces(name: &str) {
    let (b, c) = case(name);
    let [faces, edges, _] = counts(&b).unwrap();
    for e in 0..edges {
        let mut d: Vec<f64> = (0..faces).filter_map(|f| edge_face_distance(&b, e, f).ok()).collect();
        d.sort_by(f64::total_cmp);
        assert!(d.first().is_some_and(|&d| d < TOL * size(&c)), "{name}: edge {e}: {d:?}");
    }
    assert!(edge_face_distance(&b, edges, 0).is_err() && edge_face_distance(&b, 0, faces).is_err());
}

fn checks_valid(name: &str) {
    let (b, _) = case(name);
    for level in [CheckLevel::Basic, CheckLevel::Geometry, CheckLevel::All] {
        let r = check(&b, level).unwrap();
        assert!(r.is_valid(), "{name}, {level:?}: {r}");
    }
}

/// Refusing is decided on the body's naming scheme alone. An Unknown body is refused rather than
/// reported invalid on every edge; a Caller one without recorded keys is checked under the arena
/// identities (`check_brep` has no error for it, so no error of its can turn into a refusal).
#[test]
fn check_refuses_only_an_unknown_naming() {
    use solidcore::topology::NamingScheme;
    let name = |naming| {
        let (b, _) = case("box");
        b.kernel().set_naming(naming);
        check(&b, CheckLevel::All)
    };
    let refused = name(NamingScheme::Unknown).unwrap_err().to_string();
    assert!(refused.contains("not supported yet") && refused.contains("NamingScheme::Unknown"), "{refused}");
    name(NamingScheme::Caller).unwrap();
}

macro_rules! primitive {
    ($m:ident, $name:literal $(, mass: $mass:meta)? $(, area: $area:meta)?) => {
        mod $m {
            use super::*;
            #[test]
            $(#[$mass])?
            fn mass_properties_match_the_closed_forms() {
                mass_properties_match($name);
            }
            #[test]
            $(#[$area])?
            fn area_matches_the_closed_form() {
                area_matches($name);
            }
            #[test]
            fn bounds_match_the_closed_form() {
                bounds_match($name);
            }
            #[test]
            fn is_one_solid() {
                one_solid($name);
            }
            #[test]
            fn is_a_manifold_solid() {
                manifold_solid($name);
            }
            #[test]
            fn edges_lie_on_their_faces() {
                super::edges_lie_on_their_faces($name);
            }
            #[test]
            fn checks_valid() {
                super::checks_valid($name);
            }
        }
    };
}

primitive!(box_, "box");
primitive!(cylinder, "cylinder");
primitive!(sphere, "sphere");
primitive!(cone, "cone");
primitive!(frustum, "frustum");
primitive!(torus, "torus");

/// A valid box passes every test at level 2 (SolidCraft's level), with an empty report.
#[test]
fn a_valid_box_passes_level_2() {
    let (b, _) = case("box");
    let r = check(&b, CheckLevel::All).unwrap();
    assert_eq!((r.failures, r.text.as_str()), (0, ""), "{r}");
}

/// A box with one corner moved 1 mm off its edges, through the arrays: the check finds it broken.
#[test]
fn a_vertex_off_its_edges_fails_the_check() {
    use crate::flat::VERTEX_POSITION;
    let (b, _) = case("box");
    let mut a = super::to_arrays(&b).unwrap();
    a.reals.get_mut(&VERTEX_POSITION).unwrap()[0] += 1.0;
    let broken = super::from_arrays(&a).unwrap();
    let r = check(&broken, CheckLevel::All).unwrap();
    assert!(!r.is_valid(), "{r}");
    assert_eq!(r.failures, r.text.lines().filter(|l| l.contains("::Test #")).count(), "{r}");
}

#[test]
fn hostile_numbers_are_refused() {
    let (b, _) = case("box");
    assert_eq!(mass_properties(&b, f64::NAN).unwrap_err(), crate::Error::Argument);
    assert_eq!(area(&b, f64::INFINITY).unwrap_err(), crate::Error::Argument);
    assert_eq!(edge_face_distance(&b, usize::MAX, 0).unwrap_err(), crate::Error::Argument);
    // Out of range is clamped, not refused.
    assert!(rel(mass_properties(&b, 1e-30).unwrap().volume, 6000.0) < TOL);
    assert!(rel(mass_properties(&b, 10.0).unwrap().volume, 6000.0) < TOL);
}

/// A 20 mm box minus a centred 10 mm box (a sealed void), as solidcore's boolean makes it and as
/// it comes back from arrays listing its solid region's shells inner first.
fn sealed_voids() -> [(&'static str, Body); 2] {
    use crate::BooleanOp;
    use crate::flat::{FACEUSE_FACE, FACEUSE_ORIENTATION, REGION_SHELL_COUNT, SHELL_FACEUSE_COUNT};
    let body = super::boolean::boolean(&make_box([-10.0; 3], [20.0; 3]).unwrap(), &make_box([-5.0; 3], [10.0; 3]).unwrap(), BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    // The bounded solid region of two shells: swap their blocks (the shell counts match, as do
    // the face-use counts, six faces each), so only the face uses move.
    let mut a = super::to_arrays(&body).unwrap();
    let regions = &a.ints[&REGION_SHELL_COUNT];
    let first = regions.iter().position(|&n| n == 2).unwrap();
    let shell: usize = regions[..first].iter().map(|&n| usize::try_from(n).unwrap()).sum();
    let counts = &a.ints[&SHELL_FACEUSE_COUNT];
    assert_eq!((counts[shell], counts[shell + 1]), (6, 6));
    let fu: usize = counts[..shell].iter().map(|&n| usize::try_from(n).unwrap()).sum();
    for id in [FACEUSE_FACE, FACEUSE_ORIENTATION] {
        let v = a.ints.get_mut(&id).unwrap();
        let (x, y) = v[fu..fu + 12].split_at_mut(6);
        x.swap_with_slice(y);
    }
    let reordered = super::from_arrays(&a).unwrap();
    [("solidcore's", body), ("inner first", reordered)]
}

/// The sealed void's mass properties: 8000 − 1000 to 1e-9, centroid at the origin, one solid,
/// in either shell order.
#[test]
fn a_sealed_void_measures_its_volume() {
    let want = 8000.0 - 1000.0;
    // Ixx of a cube of side s about its centre: s⁵ / 6.
    let moment = (20.0f64.powi(5) - 10.0f64.powi(5)) / 6.0;
    for (at, b) in sealed_voids() {
        let m = mass_properties(&b, MEASURE_ACCURACY).unwrap();
        assert!(rel(m.volume, want) < TOL, "{at}: volume {}", m.volume);
        assert!(m.centroid.iter().all(|c| c.abs() < TOL * 20.0), "{at}: centroid {:?}", m.centroid);
        assert!(m.moments.iter().all(|&i| rel(i, moment) < TOL), "{at}: moments {:?} vs {moment}", m.moments);
        assert_eq!(solid_count(&b).unwrap(), 1, "{at}");
    }
}

/// Two 10 mm squares apart in z = 0, one body of two faces with a shell each (as the top and
/// front of a box offset apart across their edge make, kernel case
/// `surfaces/cov-offset-two-faces-across-a-sharp-edge-come-apart`): `SmBrep` test 21,
/// "Suspicious Shell Count", warns of it, and the capi's check counts no warning.
#[test]
fn faces_apart_with_a_shell_each_check_valid() {
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::geom::vector3d::Vec3;
    use solidcore::primitive::primitive_creation::p1::create_rectangle;

    use super::boolean::boolean2d;
    use super::run;
    use crate::BooleanOp;

    let square = |x: f64| {
        let frame = Axis2Placement::new(Vec3::new(x, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        run("create_rectangle", |k| create_rectangle(k, 0.0, 10.0, 10.0, &frame)).map(Body::from_kernel).unwrap()
    };
    let b = boolean2d(&square(0.0), &square(20.0), BooleanOp::Union).unwrap().unwrap();
    assert_eq!(counts(&b).unwrap()[0], 2);
    for level in [CheckLevel::Basic, CheckLevel::Geometry, CheckLevel::All] {
        let r = check(&b, level).unwrap();
        assert!(r.is_valid(), "{level:?}: {r}");
    }
}

/// `smlib_brep_classify_point` (fork 34d6b2b), the capi test's cases: a 10 x 20 x 30 box at
/// `(1, -2, 3)`: inside, outside, on a face, an edge and a vertex; the cylinder of radius 5 about
/// the z axis through `(1, 2)`: off its curved wall by 1e-8 on it (within the zone tolerance), by
/// 1e-4 inside or outside, on its axis inside, on its top's centre on it.
#[test]
fn points_classify_as_the_capi() {
    use super::classify_point;
    use crate::measure::PointClass::{Inside, OnBoundary, Outside};

    let (bx, _) = case("box");
    let (cyl, _) = case("cylinder");
    let c45 = std::f64::consts::FRAC_1_SQRT_2;
    let wall = |r: f64| [1.0 + r * c45, 2.0 + r * c45, 3.0];
    let cases = [
        ("box", [6.0, 8.0, 18.0], Inside),
        ("box", [6.0, 8.0, 34.0], Outside),
        ("box", [0.0, -3.0, 2.0], Outside),
        ("box", [6.0, 8.0, 33.0], OnBoundary),
        ("box", [11.0, 8.0, 33.0], OnBoundary),
        ("box", [11.0, 18.0, 33.0], OnBoundary),
        ("box", [1.0, -2.0, 3.0], OnBoundary),
        ("cylinder", wall(5.0 + 1e-8), OnBoundary),
        ("cylinder", wall(5.0 - 1e-8), OnBoundary),
        ("cylinder", wall(5.0 + 1e-4), Outside),
        ("cylinder", wall(5.0 - 1e-4), Inside),
        ("cylinder", [1.0, 2.0, 3.0], Inside),
        ("cylinder", [1.0, 2.0, 9.0], OnBoundary),
    ];
    for (name, p, want) in cases {
        let body = if name == "box" { &bx } else { &cyl };
        assert_eq!(classify_point(body, p).unwrap(), want, "{name} {p:?}");
    }
    assert_eq!(classify_point(&bx, [f64::NAN, 0.0, 0.0]).unwrap_err(), crate::Error::Argument);
    assert_eq!(classify_point(&bx, [0.0, f64::INFINITY, 0.0]).unwrap_err(), crate::Error::Argument);
}

/// A sheet hung on a solid (fork 9767916's capi test): a 10 mm cube and a 10 x 10 sheet on from its
/// top front edge, merged into one body. Its area is every face's, the cube's 600 and the sheet's
/// 100, not the cube's boundary alone; its volume the cube's.
#[test]
fn a_sheet_hung_on_a_solid_counts_in_the_area() {
    use solidcore::boolean::api_brep::boolean_merge;
    use solidcore::geom::vector3d::Vec3;
    use solidcore::primitive::api_primitives::{create_box, create_plane};
    let both = super::run("cube and flap", |k| {
        let cube = create_box(k, Vec3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0)?;
        let flap = create_plane(k, Vec3::new(10.0, 0.0, 10.0), 10.0, 10.0)?;
        boolean_merge(k, cube, flap)
    })
    .map(Body::from_kernel)
    .unwrap();
    let m = mass_properties(&both, 1e-8).unwrap();
    assert!((m.area - 700.0).abs() < 1e-6 && (m.volume - 1000.0).abs() < 1e-6, "{m:?}");
}

/// Proved curve-to-face gap bounds on a 10 × 20 × 30 box: a spline and a quarter-circle arc on
/// the top face (z = 33) bound near 0 on that face, the spline lifted by 2.5 mm bounds at 2.5 mm,
/// a vertical quarter-circle rising 2 mm off the top face bounds at 2 mm, an out-of-range face
/// returns `None`, invalid intervals/floors/curve kinds return `Error::Argument`, and a stopped
/// token still gets answers from `curve_face_gap_bound` (spline and conic), `counts`, and `volume`.
#[test]
fn curve_face_gap_bound_bounds_splines_and_conics_and_handles_missing_face() {
    use solidcore_record::brep::{Curve, Frame};

    let (b, _) = case("box");
    assert_eq!(counts(&b).unwrap(), [6, 12, 8]);
    let on_top = Curve::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![[2.0, 0.0, 33.0], [10.0, 16.0, 33.0]] };
    let bounds_all: Vec<f64> = (0..6).map(|f| super::curve_face_gap_bound(&b, &on_top, [0.0, 1.0], f, 1e-9).unwrap().unwrap()).collect();
    let zero_faces: Vec<usize> = bounds_all.iter().enumerate().filter_map(|(f, &d)| (d < 1e-9).then_some(f)).collect();
    assert_eq!(zero_faces.len(), 1, "exactly the top face: {bounds_all:?}");
    let top = zero_faces[0];

    let lifted = Curve::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![[2.0, 0.0, 35.5], [10.0, 16.0, 35.5]] };
    let gap = super::curve_face_gap_bound(&b, &lifted, [0.0, 1.0], top, 1e-9).unwrap().unwrap();
    assert!((2.5..=2.5 + 1e-6).contains(&gap), "lifted gap {gap}");

    let arc = Curve::Circle { frame: Frame { origin: [6.0, 8.0, 33.0], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] }, radius: 2.0 };
    // Bisection stops once a piece's remainder is at most `floor`, so `floor = 1e-10` proves a
    // zero-gap curve within 1e-9 (remainder <= 1e-10 plus evaluation rounding).
    let arc_gap = super::curve_face_gap_bound(&b, &arc, [0.0, std::f64::consts::FRAC_PI_2], top, 1e-10).unwrap().unwrap();
    assert!(arc_gap < 1e-9, "quarter circle on top face: {arc_gap}");

    // Quarter circle rising off the top face to z = 35 at theta = pi/2: catches interval truncation
    // (`CLOSE_RELATIVE = 1e-3`, so the proved upper bound lies in `[2.0, 2.0 * (1 + 2e-3)]`).
    let v_arc = Curve::Circle { frame: Frame { origin: [6.0, 8.0, 33.0], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 0.0, 1.0] }, radius: 2.0 };
    let v_arc_gap = super::curve_face_gap_bound(&b, &v_arc, [0.0, std::f64::consts::FRAC_PI_2], top, 1e-9).unwrap().unwrap();
    assert!((2.0..=2.0 * (1.0 + 2.0e-3)).contains(&v_arc_gap), "vertical arc {v_arc_gap}");

    assert_eq!(super::curve_face_gap_bound(&b, &on_top, [0.0, 1.0], 6, 1e-9).unwrap(), None);
    assert_eq!(super::curve_face_gap_bound(&b, &on_top, [0.0, 1.0], usize::MAX, 1e-9).unwrap(), None);
    let line = Curve::Line { start: [2.0, 0.0, 33.0], end: [10.0, 16.0, 33.0] };
    assert_eq!(super::curve_face_gap_bound(&b, &line, [0.0, 1.0], top, 1e-9).unwrap_err(), crate::Error::Argument);
    for curve in [&on_top, &arc] {
        for bad_ivl in [[1.0, 0.0], [1.0, 1.0], [f64::NAN, 1.0], [f64::NEG_INFINITY, 1.0]] {
            assert_eq!(super::curve_face_gap_bound(&b, curve, bad_ivl, top, 1e-9).unwrap_err(), crate::Error::Argument);
        }
        for bad_floor in [-1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(super::curve_face_gap_bound(&b, curve, [0.0, 1.0], top, bad_floor).unwrap_err(), crate::Error::Argument);
        }
    }

    let stop = crate::stop::Stop::new();
    stop.stop();
    let stopped_gap = stop.within(|| super::curve_face_gap_bound(&b, &lifted, [0.0, 1.0], top, 1e-9)).unwrap().unwrap();
    assert_eq!(stopped_gap, gap);
    let stopped_conic = stop.within(|| super::curve_face_gap_bound(&b, &v_arc, [0.0, std::f64::consts::FRAC_PI_2], top, 1e-9)).unwrap().unwrap();
    assert_eq!(stopped_conic, v_arc_gap);
    assert_eq!(stop.within(|| counts(&b)).unwrap(), [6, 12, 8]);
    let stopped_vol = stop.within(|| super::volume(&b, 1e-7)).unwrap();
    assert!((stopped_vol - 6000.0).abs() < 1e-6, "stopped volume {stopped_vol}");
}

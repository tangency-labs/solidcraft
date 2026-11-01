//! The Measure and Check families on solidcore's primitives: mass properties, area and bounds
//! against their closed forms ([`crate::measure::closed`]), one solid each, edges on their faces;
//! and (with SMLib built) the same answers as the C++ kernel for the same primitive.

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
        b.0.lock().unwrap().set_naming(naming);
        check(&b, CheckLevel::All)
    };
    let refused = name(NamingScheme::Unknown).unwrap_err().to_string();
    assert!(refused.contains("not supported yet") && refused.contains("NamingScheme::Unknown"), "{refused}");
    name(NamingScheme::Caller).unwrap();
}

/// The C++ kernel's body of the same primitive, built by its C API whichever kernel is in
/// force: a body imported from solidcore's arrays is not the C++ kernel's own (its
/// `edge_face_distance` measures to each face's surface over its domain, which the import sets).
#[cfg(feature = "native")]
fn cpp(name: &str) -> crate::Brep {
    use crate::{Brep, ffi};
    crate::init().unwrap();
    match name {
        "box" => ffi::make_box(&[1.0, -2.0, 3.0], &[10.0, 20.0, 30.0]),
        "cylinder" => ffi::make_cylinder(&[1.0, 2.0, -3.0], 5.0, 12.0),
        "sphere" => ffi::make_sphere(&[1.0, 2.0, 3.0], 7.0),
        "cone" => ffi::make_cone(&[0.0, 1.0, 2.0], 6.0, 0.0, 9.0),
        "frustum" => ffi::make_cone(&[0.0, 1.0, 2.0], 6.0, 2.0, 9.0),
        "torus" => ffi::make_torus(&[1.0, -1.0, 0.5], 10.0, 3.0),
        _ => panic!("{name}"),
    }
    .map(Brep::made)
    .unwrap()
}

/// `x` within `TOL` of the C++ kernel's `y`, relative to `scale`.
#[cfg(feature = "native")]
fn same(x: f64, y: f64, scale: f64, what: &str) {
    assert!((x - y).abs() <= TOL * scale, "{what}: solidcore {x} vs C++ {y}");
}

#[cfg(feature = "native")]
fn mass_properties_parity(name: &str) {
    let (b, c) = case(name);
    let ours = mass_properties(&b, MEASURE_ACCURACY).unwrap();
    let theirs = cpp(name).mass_properties(MEASURE_ACCURACY).unwrap();
    same(ours.area, theirs.area, theirs.area, &format!("{name} area"));
    same(ours.volume, theirs.volume, theirs.volume, &format!("{name} volume"));
    let imax = theirs.moments.iter().fold(0.0f64, |a, &b| a.max(b));
    for i in 0..3 {
        same(ours.centroid[i], theirs.centroid[i], size(&c), &format!("{name} centroid[{i}]"));
        same(ours.moments[i], theirs.moments[i], theirs.moments[i], &format!("{name} moments[{i}]"));
        same(ours.products[i], theirs.products[i], imax, &format!("{name} products[{i}]"));
    }
}

#[cfg(feature = "native")]
fn area_parity(name: &str) {
    let (b, _) = case(name);
    let theirs = cpp(name).area(MEASURE_ACCURACY).unwrap();
    same(area(&b, MEASURE_ACCURACY).unwrap(), theirs, theirs, &format!("{name} area"));
}

#[cfg(feature = "native")]
fn bounds_parity(name: &str) {
    let (b, c) = case(name);
    let (lo, hi) = bounds(&b).unwrap();
    let (tlo, thi) = cpp(name).bounds().unwrap();
    for i in 0..3 {
        same(lo[i], tlo[i], size(&c), &format!("{name} low[{i}]"));
        same(hi[i], thi[i], size(&c), &format!("{name} high[{i}]"));
    }
}

#[cfg(feature = "native")]
fn solid_count_parity(name: &str) {
    let (b, _) = case(name);
    assert_eq!(solid_count(&b).unwrap(), cpp(name).solid_count().unwrap(), "{name}");
}

#[cfg(feature = "native")]
fn manifold_parity(name: &str) {
    let (b, _) = case(name);
    assert_eq!(is_manifold_solid(&b).unwrap(), cpp(name).is_manifold_solid().unwrap(), "{name}");
}

/// The whole edge-by-face distance matrix, the C++ kernel's own (both bodies list their edges
/// and faces in the same order: `convert_tests` shows their arrays are equal).
#[cfg(feature = "native")]
fn distance_parity(name: &str) {
    let (b, c) = case(name);
    let theirs = cpp(name);
    let [faces, edges, _] = counts(&b).unwrap();
    for e in 0..edges {
        for f in 0..faces {
            let (x, y) = (edge_face_distance(&b, e, f), theirs.edge_face_distance(e, f));
            match (x, y) {
                (Ok(x), Ok(y)) => same(x, y, size(&c), &format!("{name} edge {e} face {f}")),
                (x, y) => assert_eq!(x.is_ok(), y.is_ok(), "{name} edge {e} face {f}: {x:?} vs {y:?}"),
            }
        }
    }
}

#[cfg(feature = "native")]
fn check_parity(name: &str) {
    let (b, _) = case(name);
    for level in [CheckLevel::Basic, CheckLevel::Geometry, CheckLevel::All] {
        assert_eq!(check(&b, level).unwrap(), cpp(name).check(level).unwrap(), "{name}, {level:?}");
    }
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
            #[cfg(feature = "native")]
            #[test]
            $(#[$mass])?
            fn mass_properties_match_the_cpp_kernel() {
                let _cpp = crate::route::on_cpp();
                mass_properties_parity($name);
            }
            #[cfg(feature = "native")]
            #[test]
            $(#[$area])?
            fn area_matches_the_cpp_kernel() {
                let _cpp = crate::route::on_cpp();
                area_parity($name);
            }
            #[cfg(feature = "native")]
            #[test]
            fn bounds_match_the_cpp_kernel() {
                let _cpp = crate::route::on_cpp();
                bounds_parity($name);
            }
            #[cfg(feature = "native")]
            #[test]
            fn solid_count_matches_the_cpp_kernel() {
                let _cpp = crate::route::on_cpp();
                solid_count_parity($name);
            }
            #[cfg(feature = "native")]
            #[test]
            fn manifold_matches_the_cpp_kernel() {
                let _cpp = crate::route::on_cpp();
                manifold_parity($name);
            }
            #[cfg(feature = "native")]
            #[test]
            fn distances_match_the_cpp_kernel() {
                let _cpp = crate::route::on_cpp();
                distance_parity($name);
            }
            #[cfg(feature = "native")]
            #[test]
            fn check_matches_the_cpp_kernel() {
                let _cpp = crate::route::on_cpp();
                check_parity($name);
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

/// The head of each report line, `<reporting type>::Test #<n>`, or the whole line for the capi's
/// own lines (a failed status without a report, a skipped AssertValid).
#[cfg(feature = "native")]
fn tests_of(text: &str) -> Vec<&str> {
    text.lines().map(|l| l.find(" #").and_then(|i| l[i + 2..].find(' ').map(|j| &l[..i + 2 + j])).unwrap_or(l)).collect()
}

/// A valid box passes every test at level 2 (SolidCraft's level), with an empty report, on
/// solidcore and on the C++ kernel.
#[test]
fn a_valid_box_passes_level_2() {
    #[cfg(feature = "native")]
    let _cpp = crate::route::on_cpp();
    let (b, _) = case("box");
    let r = check(&b, CheckLevel::All).unwrap();
    assert_eq!((r.failures, r.text.as_str()), (0, ""), "{r}");
    #[cfg(feature = "native")]
    {
        let r = cpp("box").check(CheckLevel::All).unwrap();
        assert_eq!((r.failures, r.text.as_str()), (0, ""), "{r}");
    }
}

/// A box with one corner moved 1 mm off its edges, through the arrays: the check finds it broken,
/// and the C++ kernel fails the same tests on the same arrays.
#[test]
fn a_vertex_off_its_edges_fails_the_check() {
    #[cfg(feature = "native")]
    let _cpp = crate::route::on_cpp();
    use crate::flat::VERTEX_POSITION;
    let (b, _) = case("box");
    let mut a = super::to_arrays(&b).unwrap();
    a.reals.get_mut(&VERTEX_POSITION).unwrap()[0] += 1.0;
    let broken = super::from_arrays(&a).unwrap();
    let r = check(&broken, CheckLevel::All).unwrap();
    assert!(!r.is_valid(), "{r}");
    assert_eq!(r.failures, r.text.lines().filter(|l| l.contains("::Test #")).count(), "{r}");
    #[cfg(feature = "native")]
    {
        let cpp = crate::flat::Flat::from_arrays(&a).unwrap().import().unwrap();
        let c = cpp.check(CheckLevel::All).unwrap();
        assert_eq!(tests_of(&r.text), tests_of(&c.text), "solidcore:\n{}\nC++:\n{}", r.text, c.text);
        assert_eq!(r, c);
    }
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
/// in either shell order; with SMLib built, the C++ kernel's own.
#[test]
fn a_sealed_void_measures_its_volume() {
    #[cfg(feature = "native")]
    let _cpp = crate::route::on_cpp();
    let want = 8000.0 - 1000.0;
    // Ixx of a cube of side s about its centre: s⁵ / 6.
    let moment = (20.0f64.powi(5) - 10.0f64.powi(5)) / 6.0;
    #[cfg(feature = "native")]
    let theirs = {
        use crate::{Brep, ffi};
        crate::init().unwrap();
        let make = |o: f64, s: f64| ffi::make_box(&[o; 3], &[s; 3]).map(Brep::made).unwrap();
        let (outer, inner) = (make(-10.0, 20.0), make(-5.0, 10.0));
        let h = ffi::ops::boolean(outer.cpp().unwrap(), inner.cpp().unwrap(), ffi::ops::BOOLEAN_SUBTRACT).unwrap();
        ffi::volume(&h, 1e-9).unwrap()
    };
    for (at, b) in sealed_voids() {
        let m = mass_properties(&b, MEASURE_ACCURACY).unwrap();
        assert!(rel(m.volume, want) < TOL, "{at}: volume {}", m.volume);
        assert!(m.centroid.iter().all(|c| c.abs() < TOL * 20.0), "{at}: centroid {:?}", m.centroid);
        assert!(m.moments.iter().all(|&i| rel(i, moment) < TOL), "{at}: moments {:?} vs {moment}", m.moments);
        assert_eq!(solid_count(&b).unwrap(), 1, "{at}");
        #[cfg(feature = "native")]
        same(m.volume, theirs, theirs, &format!("{at} volume"));
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
        run("create_rectangle", |k| create_rectangle(k, 0.0, 10.0, 10.0, &frame)).map(Body::new).unwrap()
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
/// 1e-4 inside or outside, on its axis inside, on its top's centre on it. With SMLib built, the
/// C++ kernel's own primitives classify the same.
#[test]
fn points_classify_as_the_capi() {
    #[cfg(feature = "native")]
    let _cpp = crate::route::on_cpp();
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
        #[cfg(feature = "native")]
        {
            use crate::ffi::measure::{POINT_INSIDE, POINT_ON_BOUNDARY, POINT_OUTSIDE, classify_point as cpp_classify};
            let theirs = cpp_classify(cpp(name).cpp().unwrap(), p).unwrap();
            let want_cpp = match want {
                Inside => POINT_INSIDE,
                Outside => POINT_OUTSIDE,
                OnBoundary => POINT_ON_BOUNDARY,
            };
            assert_eq!(theirs, want_cpp, "C++ {name} {p:?}");
        }
    }
    assert_eq!(classify_point(&bx, [f64::NAN, 0.0, 0.0]).unwrap_err(), crate::Error::Argument);
    assert_eq!(classify_point(&bx, [0.0, f64::INFINITY, 0.0]).unwrap_err(), crate::Error::Argument);
}

/// A sheet hung on a solid (fork 9767916's capi test): a 10 mm cube and a 10 x 10 sheet on from its
/// top front edge, merged into one body. Its area is every face's, the cube's 600 and the sheet's
/// 100, not the cube's boundary alone; its volume the cube's. The same on the C++ kernel.
#[test]
fn a_sheet_hung_on_a_solid_counts_in_the_area() {
    #[cfg(feature = "native")]
    let _cpp = crate::route::on_cpp();
    use solidcore::boolean::api_brep::boolean_merge;
    use solidcore::geom::vector3d::Vec3;
    use solidcore::primitive::api_primitives::{create_box, create_plane};
    let both = super::run("cube and flap", |k| {
        let cube = create_box(k, Vec3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0)?;
        let flap = create_plane(k, Vec3::new(10.0, 0.0, 10.0), 10.0, 10.0)?;
        boolean_merge(k, cube, flap)
    })
    .map(Body::new)
    .unwrap();
    let m = mass_properties(&both, 1e-8).unwrap();
    assert!((m.area - 700.0).abs() < 1e-6 && (m.volume - 1000.0).abs() < 1e-6, "{m:?}");
    #[cfg(feature = "native")]
    {
        let cpp = crate::Brep::made_core(both);
        let c = crate::ffi::measure::mass_properties(cpp.cpp().unwrap(), 1e-8).unwrap();
        assert!((c[0] - m.area).abs() < 1e-6 && (c[1] - m.volume).abs() < 1e-6, "C++ {c:?} vs {m:?}");
    }
}

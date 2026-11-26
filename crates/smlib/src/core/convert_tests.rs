//! solidcore's primitives through the flat arrays: each exports to a valid record of the right
//! volume, and the record comes back to an equal body. A body with a sealed void crosses with each region's enclosing shell first,
//! whatever order its arrays list the shells in (the capi's `PutEnclosingShellsFirst`).

use std::f64::consts::PI;

use solidcore_record::brep as b;

use super::boolean::boolean;
use super::{Body, counts, from_arrays, make_box, make_cone, make_cylinder, make_sphere, make_torus, to_arrays, volume};
use crate::BooleanOp;
use crate::export::native;
use crate::import::{self, Arrays};

/// A primitive: solidcore's body and its analytic volume.
struct Case {
    body: Body,
    volume: f64,
}

fn case(name: &str) -> Case {
    let (body, volume) = match name {
        "box" => (make_box([1.0, -2.0, 3.0], [10.0, 20.0, 30.0]), 6000.0),
        "cylinder" => (make_cylinder([1.0, 2.0, -3.0], 5.0, 12.0), PI * 25.0 * 12.0),
        "sphere" => (make_sphere([1.0, 2.0, 3.0], 7.0), 4.0 / 3.0 * PI * 343.0),
        "cone" => (make_cone([0.0, 1.0, 2.0], 6.0, 0.0, 9.0), PI * 36.0 * 9.0 / 3.0),
        "frustum" => (make_cone([0.0, 1.0, 2.0], 6.0, 2.0, 9.0), PI * 9.0 / 3.0 * (36.0 + 12.0 + 4.0)),
        "torus" => (make_torus([1.0, -1.0, 0.5], 10.0, 3.0), 2.0 * PI * PI * 10.0 * 9.0),
        _ => panic!("{name}"),
    };
    Case { body: body.unwrap(), volume }
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs()
}

/// The record of solidcore's arrays, validated and its geometry checked.
fn record(a: &Arrays) -> b::Brep {
    let (rec, _) = native(a).unwrap();
    solidcore_ops::record_check::validate(&rec).unwrap();
    solidcore_ops::record_check::agreement(&rec).unwrap();
    rec
}

/// Every number in `a` within `tol` (relative past 1) of `b`'s, everything else equal.
fn close(a: &serde_json::Value, b: &serde_json::Value, tol: f64, at: &str) {
    use serde_json::Value as V;
    match (a, b) {
        (V::Number(x), V::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            assert!((x - y).abs() <= tol * x.abs().max(1.0), "{at}: {x} vs {y}");
        }
        (V::Array(x), V::Array(y)) => {
            assert_eq!(x.len(), y.len(), "{at}: length");
            for (i, (x, y)) in x.iter().zip(y).enumerate() {
                close(x, y, tol, &format!("{at}[{i}]"));
            }
        }
        (V::Object(x), V::Object(y)) => {
            assert_eq!(x.keys().collect::<Vec<_>>(), y.keys().collect::<Vec<_>>(), "{at}: keys");
            for (k, x) in x {
                close(x, &y[k], tol, &format!("{at}.{k}"));
            }
        }
        _ => assert_eq!(a, b, "{at}"),
    }
}

/// to_arrays gives a record that validates and whose geometry checks.
fn exports(name: &str) {
    record(&to_arrays(&case(name).body).unwrap());
}

/// solidcore's body measures its analytic volume.
fn measures(name: &str) {
    let c = case(name);
    let v = volume(&c.body, 1e-8).unwrap();
    assert!(rel(v, c.volume) < 1e-9, "{name}: solidcore volume {v} vs {}", c.volume);
}

/// record -> arrays -> from_arrays -> to_arrays -> record is the record, the volume exact.
fn round_trips(name: &str) {
    let c = case(name);
    let rec = record(&to_arrays(&c.body).unwrap());
    let back = from_arrays(&import::arrays(&rec, true).unwrap()).unwrap();
    let again = record(&to_arrays(&back).unwrap());
    close(&serde_json::to_value(&rec).unwrap(), &serde_json::to_value(&again).unwrap(), 1e-12, name);
    let v = volume(&back, 1e-8).unwrap();
    assert!(rel(v, c.volume) < 1e-9, "{name}: {v} vs {}", c.volume);
}

macro_rules! primitive {
    ($m:ident, $name:literal $(, volume: $vol:meta)? $(, round_trip: $rt:meta)?) => {
        mod $m {
            #[test]
            fn exports_a_valid_record() {
                super::exports($name);
            }
            #[test]
            $(#[$vol])?
            fn measures_its_volume() {
                super::measures($name);
            }
            #[test]
            $(#[$rt])?
            fn round_trips_through_the_record() {
                super::round_trips($name);
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

#[test]
fn hostile_arrays_are_refused() {
    let c = case("box");
    let good = to_arrays(&c.body).unwrap();
    assert!(from_arrays(&Arrays::default()).is_err());
    for id in good.ints.keys().copied().collect::<Vec<_>>() {
        let mut a = good.clone();
        if let Some(v) = a.ints.get_mut(&id) {
            v.push(7);
        }
        let _ = from_arrays(&a);
        let mut a = good.clone();
        if let Some(v) = a.ints.get_mut(&id).and_then(|v| v.first_mut()) {
            *v = i64::MAX;
        }
        let _ = from_arrays(&a);
    }
    for id in good.reals.keys().copied().collect::<Vec<_>>() {
        let mut a = good.clone();
        if let Some(v) = a.reals.get_mut(&id).and_then(|v| v.first_mut()) {
            *v = f64::NAN;
            assert!(from_arrays(&a).is_err(), "NaN in {id}");
        }
    }
}

/// `GetOrCreateGwNurbPointer` on a parabola: the one with its NURB gives it; one built over an
/// interval too small for two end tangents to cross has none and makes none, an error naming
/// `MakeNurb`.
#[test]
fn a_parabola_gives_its_nurb_or_says_it_cannot() {
    use solidcore::curve::curve::Curve;
    use solidcore::curve::parabola::Parabola;
    use solidcore::geom::extent1d::Extent1d;
    use solidcore::geom::vector3d::{Point3, Vec3};
    let (x, y) = (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
    let part = |lo: f64, hi: f64| -> crate::Result<bool> {
        let c = Curve::from(Parabola::new(Point3::new(0.0, 0.0, 0.0), x, y, Extent1d::from_min_max(lo, hi), 1.0, 3));
        let k = solidcore::kernel::Kernel::default();
        Ok(super::convert::bspline_curve_part(&k, &c)?.is_some_and(|b| b.gw_nurb().is_some()))
    };
    assert_eq!(part(-1.0, 1.0), Ok(true));
    let none = part(0.5, 0.5);
    assert!(matches!(&none, Err(e) if e.to_string().contains("MakeNurb")), "{none:?}");
}

/// Each edge's and vertex's tolerance crosses the kernels as it is. One box edge given eight times
/// the box's tolerance, in arrays whose TOLERANCE is the largest (as SMLib's export wrote them),
/// keeps it round after round, and the B-rep and every other edge and vertex keep the box's. Every
/// crossing used to give every entity the largest, so a chain of blends on a mixed route grew the
/// whole body's tolerance fourfold per blend (fillets/2341).
#[test]
fn tolerances_cross_the_arrays_per_entity() {
    use crate::flat::{EDGE_TOLERANCE, TOLERANCE, VERTEX_TOLERANCE};
    let mut a = to_arrays(&case("box").body).unwrap();
    let base = a.reals[&TOLERANCE][0];
    assert!(a.reals[&EDGE_TOLERANCE].iter().chain(&a.reals[&VERTEX_TOLERANCE]).all(|&t| t == base), "{a:?}");
    a.reals.get_mut(&EDGE_TOLERANCE).unwrap()[0] = 8.0 * base;
    a.reals.get_mut(&TOLERANCE).unwrap()[0] = 8.0 * base;
    for round in 0..3 {
        let body = from_arrays(&a).unwrap();
        assert_eq!(body.kernel().tolerance(), base / 2.0, "round {round}");
        a = to_arrays(&body).unwrap();
        assert_eq!(a.reals[&TOLERANCE], [base], "round {round}");
        let edges = &a.reals[&EDGE_TOLERANCE];
        assert_eq!(edges[0], 8.0 * base, "round {round}");
        assert!(edges[1..].iter().chain(&a.reals[&VERTEX_TOLERANCE]).all(|&t| t == base), "round {round}: {a:?}");
    }
}

/// A 20 mm box minus a centred 10 mm box: a sealed void, so its solid region has two shells.
fn sealed_void() -> Body {
    boolean(&make_box([-10.0; 3], [20.0; 3]).unwrap(), &make_box([-5.0; 3], [10.0; 3]).unwrap(), BooleanOp::Subtract).unwrap().unwrap()
}

/// 20³ − 10³.
const SEALED_VOID_VOLUME: f64 = 8000.0 - 1000.0;

/// Each bounded region's shells, as each shell's reach: the largest |coordinate| of its vertices
/// (10 for the 20 mm box, 5 for the 10 mm one).
fn reaches(rec: &b::Brep) -> Vec<Vec<f64>> {
    let reach = |s: u32| {
        let Some(b::Shell::Faces { faces, .. }) = rec.shells.get(s as usize) else { panic!("shell {s}") };
        let mut r = 0.0f64;
        for f in faces {
            for l in &rec.faces[f.face as usize].loops {
                let b::Loop::Edges { edges } = l else { continue };
                for e in edges {
                    for &v in &rec.edges[e.edge as usize].vertices {
                        r = rec.vertices[v as usize].point.iter().fold(r, |r, c| r.max(c.abs()));
                    }
                }
            }
        }
        r
    };
    rec.regions.iter().skip(1).map(|r| r.shells.iter().map(|&s| reach(s)).collect()).collect()
}

/// The sealed void's record: the solid region lists the outer box first, then the void's box;
/// the void region its box alone.
fn outer_first(a: &Arrays, at: &str) {
    let mut got = reaches(&record(a));
    got.sort_by_key(|x| std::cmp::Reverse(x.len()));
    assert_eq!(got, [vec![10.0, 5.0], vec![5.0]], "{at}");
}

/// The arrays with every region's shells listed in reverse: each shell's block (its counts and
/// its face uses) moved, nothing else touched.
fn shells_reversed(a: &Arrays) -> Arrays {
    use crate::flat::{FACEUSE_FACE, FACEUSE_ORIENTATION, REGION_SHELL_COUNT, SHELL_FACEUSE_COUNT, SHELL_POINT_TYPE, SHELL_WIRE_EDGE_COUNT};
    let mut p = a.clone();
    let blocks = [SHELL_FACEUSE_COUNT, SHELL_WIRE_EDGE_COUNT, SHELL_POINT_TYPE, FACEUSE_FACE, FACEUSE_ORIENTATION];
    for id in blocks {
        p.ints.get_mut(&id).unwrap().clear();
    }
    let (mut shell, mut fu) = (0usize, 0usize);
    for &n in &a.ints[&REGION_SHELL_COUNT] {
        let mut region = Vec::new();
        for _ in 0..n {
            let uses = usize::try_from(a.ints[&SHELL_FACEUSE_COUNT][shell]).unwrap();
            region.push((shell, fu..fu + uses));
            shell += 1;
            fu += uses;
        }
        for (s, uses) in region.into_iter().rev() {
            for id in [SHELL_FACEUSE_COUNT, SHELL_WIRE_EDGE_COUNT, SHELL_POINT_TYPE] {
                let v = a.ints[&id][s];
                p.ints.get_mut(&id).unwrap().push(v);
            }
            for id in [FACEUSE_FACE, FACEUSE_ORIENTATION] {
                p.ints.get_mut(&id).unwrap().extend_from_slice(&a.ints[&id][uses.clone()]);
            }
        }
    }
    assert_ne!(p.ints, a.ints, "nothing to reverse");
    p
}

/// solidcore's sealed void exports with its outer shell first, comes back through the arrays
/// with it first, and keeps its volume.
#[test]
fn a_sealed_void_crosses_the_arrays_outer_shell_first() {
    let body = sealed_void();
    let v = volume(&body, 1e-9).unwrap();
    assert!(rel(v, SEALED_VOID_VOLUME) < 1e-9, "{v}");
    let a = to_arrays(&body).unwrap();
    outer_first(&a, "export");
    let back = from_arrays(&import::arrays(&record(&a), true).unwrap()).unwrap();
    outer_first(&to_arrays(&back).unwrap(), "round trip");
    let v = volume(&back, 1e-9).unwrap();
    assert!(rel(v, SEALED_VOID_VOLUME) < 1e-9, "round trip: {v}");
}

#[test]
fn an_import_with_a_sealed_void_reuses_its_proven_shell_order() {
    let unordered = shells_reversed(&to_arrays(&sealed_void()).unwrap());
    let imported = crate::Brep::from_arrays(&unordered).unwrap();
    let (record, _, prepared, _) = imported.to_native_prepared().unwrap();
    let prepared = prepared.expect("the exact containment test established the outer shell");
    let rebuilt = crate::Brep::from_record(&record, true).unwrap();
    assert_eq!(prepared.to_native_ordered().unwrap().0, rebuilt.to_native_ordered().unwrap().0);
    assert!(rel(prepared.volume(1e-9).unwrap(), SEALED_VOID_VOLUME) < 1e-9);
}

/// Arrays listing the solid region's void shell before the box around it import with the box
/// first (SMLib takes a region's first shell as its enclosing one), the volume unchanged.
#[test]
fn shells_listed_inner_first_import_outer_first() {
    let a = shells_reversed(&to_arrays(&sealed_void()).unwrap());
    let back = from_arrays(&a).unwrap();
    outer_first(&to_arrays(&back).unwrap(), "inner first");
    let v = volume(&back, 1e-9).unwrap();
    assert!(rel(v, SEALED_VOID_VOLUME) < 1e-9, "inner first: {v}");
}

/// A half torus of minor radius 0.1 at coordinates of about 200 (a fillet round a hole): its weights,
/// found from the points, differ from the rebuilt ones by rounding of the coordinates' size, not of 1.
#[test]
fn nurbs_weights_far_from_the_origin_compare_within_their_rounding() {
    use solidcore::Vec3 as P;
    let cv = [
        197.06,
        33.35,
        62.217999999999996,
        197.06,
        33.25,
        62.218,
        197.06,
        33.25,
        62.117999999999995,
        191.16000000000003,
        33.35,
        62.218,
        191.16000000000005,
        33.25,
        62.217999999999996,
        191.26,
        33.25,
        62.118,
        191.16000000000003,
        33.35,
        56.318,
        191.16000000000003,
        33.25,
        56.318000000000005,
        191.26000000000002,
        33.25,
        56.318,
        191.16000000000003,
        33.35,
        50.418,
        191.16000000000003,
        33.25,
        50.418,
        191.26,
        33.25,
        50.518,
        197.06,
        33.35,
        50.418,
        197.06,
        33.25,
        50.418,
        197.06,
        33.25,
        50.518,
    ];
    let pts: Vec<_> = cv.chunks(3).map(|c| P::new(c[0], c[1], c[2])).collect();
    let mut w = [
        1.0,
        std::f64::consts::FRAC_1_SQRT_2,
        1.0,
        std::f64::consts::FRAC_1_SQRT_2,
        0.5000000000000001,
        std::f64::consts::FRAC_1_SQRT_2,
        1.0,
        std::f64::consts::FRAC_1_SQRT_2,
        1.0,
        std::f64::consts::FRAC_1_SQRT_2,
        0.5000000000000001,
        std::f64::consts::FRAC_1_SQRT_2,
        1.0,
        std::f64::consts::FRAC_1_SQRT_2,
        1.0,
    ];
    let (uk, vk) = ([2.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0], [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    let make = |w: &[f64]| super::convert::bspline_surface_from_arrays(&pts, w, &uk, &vk, 2, 2).unwrap();
    let s = make(&w);

    // weights 4e-12 apart at coordinates of 200 are the same NURBS; 1e-9 apart are not
    w[1] += 4e-12;
    assert!(super::convert::same_nurbs(s.gw_nurb().unwrap(), make(&w).gw_nurb().unwrap(), 0.01));
    w[1] += 1e-9;
    assert!(!super::convert::same_nurbs(s.gw_nurb().unwrap(), make(&w).gw_nurb().unwrap(), 0.01));
}

/// A Ø1 × 1000 cylinder 1600 below the origin drafted 0.02° in about its base: its top circle (r
/// 0.15 at z = -600) is written as a circle whose NURBS lies about 3e-10 off it, rounding of the
/// coordinates' size that the export's own check allows. The import reads it back with the same
/// allowance instead of refusing "a conic NURBS off its conic", and the frustum keeps its volume.
#[test]
fn a_drafted_circle_far_from_the_origin_comes_back_as_written() {
    let (big, h, angle) = (0.5, 1000.0, 0.02f64.to_radians());
    let small = big - h * angle.tan();
    let cyl = make_cylinder([0.0, 0.0, -1600.0], big, h).unwrap();
    let drafted = super::offset::draft(&cyl, &[[big, 0.0, -1100.0]], 1e-5, [0.0, 0.0, -1600.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], angle).unwrap();
    let back = from_arrays(&to_arrays(&drafted).unwrap()).unwrap();
    let want = PI * h * (big * big + big * small + small * small) / 3.0;
    let v = volume(&back, 1e-9).unwrap();
    assert!(rel(v, want) < 1e-7, "{v} vs {want}");
}

/// A Ø3800 × Ø800 torus with a Ø96 hole cut 200 deep into its outer equator, the hole's rim
/// 0.005 across the torus's 0° seam (#19254). The rim meets the equator (radius 1900) at
/// -3.8e-5°, so the equator's piece up to the seam is an arc 1.27e-3 long, under 1e-6 of its
/// radius. The body comes back from its arrays with the same faces, edges, vertices and volume,
/// and so does the join of an 80 × 80 × 160 block wholly inside the tube, which leaves it as it is.
/// The equator's other piece, 96 long, is still written as a circle of radius 1900.
#[test]
fn a_hole_cut_a_hair_across_a_torus_seam_comes_back() {
    use crate::flat::{CURVE_CIRCLE, EDGE_CIRCLE_RADIUS, EDGE_CURVE_TYPE};
    let torus = make_torus([0.0; 3], 1500.0, 400.0).unwrap();
    let hole = make_cylinder([1899.3937162045017, 47.9949043425656, -200.0], 48.0, 200.0).unwrap();
    let cut = boolean(&torus, &hole, BooleanOp::Subtract).unwrap().unwrap();
    let want = volume(&cut, 1e-9).unwrap();
    let arrays = to_arrays(&cut).unwrap();
    let circles = arrays.ints[&EDGE_CURVE_TYPE].iter().filter(|&&t| t == i64::from(CURVE_CIRCLE)).count();
    let radii = &arrays.reals[&EDGE_CIRCLE_RADIUS];
    assert_eq!(radii.len(), circles);
    assert!(radii.iter().any(|r| (r - 1900.0).abs() <= 1e-9 * 1900.0), "no circle of radius 1900 among {radii:?}");
    let back = from_arrays(&arrays).unwrap();
    assert_eq!(counts(&back).unwrap(), counts(&cut).unwrap());
    let v = volume(&back, 1e-9).unwrap();
    assert!(rel(v, want) < 1e-9, "round trip: {v} vs {want}");

    let block = make_box([1460.0, 80.0, -160.0], [80.0, 80.0, 160.0]).unwrap();
    let joined = boolean(&back, &block, BooleanOp::Union).unwrap().unwrap();
    let again = from_arrays(&to_arrays(&joined).unwrap()).unwrap();
    assert_eq!(counts(&again).unwrap(), counts(&cut).unwrap());
    let v = volume(&again, 1e-9).unwrap();
    assert!(rel(v, want) < 1e-9, "joined: {v} vs {want}");
}

/// The tool that ends a tooth space in a cutter's curve: a section in the plane z = 40 whose floor
/// is `arc_deg` degrees of the circle of radius 13.725 about the z axis, with two walls up to 16
/// from that axis and a top, turned a quarter turn about a line in its plane `cutter_radius` above
/// the floor. The floor sweeps a torus of which the tube shows only those degrees.
fn run_out_tool(arc_deg: f64, cutter_radius: f64) -> Body {
    use std::f64::consts::FRAC_PI_2;

    use crate::sweeps::Curve;
    let (root, out_to, z) = (13.725_f64, 16.0_f64, 40.0);
    let half = (0.5 * arc_deg).to_radians();
    let at = |a: f64| [root * a.cos(), root * a.sin(), z];
    let (after, before) = (at(FRAC_PI_2 + half), at(FRAC_PI_2 - half));
    let (high_before, high_after) = ([before[0], out_to, z], [after[0], out_to, z]);
    let line = |start, end| Curve::Line { start, end };
    let profile = [
        // from the corner after the space to the one before it: clockwise seen from +z
        Curve::Arc {
            center: [0.0, 0.0, z],
            axis: [0.0, 0.0, -1.0],
            reference: [(FRAC_PI_2 + half).cos(), (FRAC_PI_2 + half).sin(), 0.0],
            radius: root,
            start: 0.0,
            end: 2.0 * half,
        },
        line(before, high_before),
        line(high_before, high_after),
        line(high_after, after),
    ];
    super::sweep::revolve(&profile, [0.0, root + cutter_radius, z], [1.0, 0.0, 0.0], FRAC_PI_2, true).unwrap()
}

/// What [`a_torus_showing_a_short_arc_of_its_tube_comes_back_a_valid_body`] says of the floor that
/// comes back, beside the body passing its own check.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Floor {
    /// Nothing more: the arc is near where one reading gives way to another.
    Any,
    /// The torus of the arrays' own frame and radii, to rounding.
    ItsOwnTorus,
    /// No torus but its own: its exact NURBS form (the arrays that come back name no torus), or the
    /// torus of the arrays' own frame and radii where the kernel can lay that one on the form.
    ItsFormOrItsOwnTorus,
}

/// A torus of which the tube shows a short arc comes back from its arrays as a body that passes its
/// own check, alone and cut out of a shaft, however short the arc. SMLib's test works a torus back
/// out of the NURBS form, which a short arc does not fix: a 1.2 degree arc came back 1e-10 to 1e-9
/// off its form and the check refused it in some places ("Bad Analytic and NURB Evaluate"), and one
/// under about 0.3 degrees was refused outright ("a NURBS form that is not its analytic surface").
/// Where the form is off the torus found (1.2 degrees), the face is the torus of the arrays' own
/// frame and radii, to rounding. Where none is found (0.05 degrees), the face is no torus but its
/// own: it is its exact form, or the arrays' torus where the kernel can lay that one on so short a
/// form.
#[test]
fn a_torus_showing_a_short_arc_of_its_tube_comes_back_a_valid_body() {
    use crate::check::CheckLevel;
    use crate::flat::{FACE_TORUS_AXIS, FACE_TORUS_MAJOR_RADIUS, FACE_TORUS_MINOR_RADIUS, FACE_TORUS_ORIGIN, FACE_TORUS_REF_DIRECTION};
    let cases = [
        (5.0, 25.0, Floor::Any),
        (2.0, 25.0, Floor::Any),
        (1.2, 25.0, Floor::ItsOwnTorus),
        (1.2, 40.0, Floor::ItsOwnTorus),
        (0.6, 25.0, Floor::Any),
        (0.3, 25.0, Floor::Any),
        (0.1, 25.0, Floor::Any),
        (0.05, 40.0, Floor::ItsFormOrItsOwnTorus),
    ];
    for (arc_deg, cutter_radius, floor) in cases {
        let at = format!("a {arc_deg} degree arc, cutter radius {cutter_radius}");
        let tool = run_out_tool(arc_deg, cutter_radius);
        let arrays = to_arrays(&tool).unwrap();
        let made = &arrays.reals[&FACE_TORUS_MAJOR_RADIUS];
        assert_eq!(made.len(), 1, "{at}: the floor is one torus");
        assert!(rel(made[0], 13.725 + cutter_radius) < 1e-12, "{at}: major radius {}", made[0]);
        let back = from_arrays(&arrays).unwrap();
        let again = to_arrays(&back).unwrap();
        // how many tori the arrays that come back name: the floor, or none where it is its form
        let named = again.reals.get(&FACE_TORUS_MAJOR_RADIUS).map_or(0, Vec::len);
        if floor == Floor::ItsOwnTorus || (floor == Floor::ItsFormOrItsOwnTorus && named > 0) {
            for id in [FACE_TORUS_ORIGIN, FACE_TORUS_AXIS, FACE_TORUS_REF_DIRECTION, FACE_TORUS_MAJOR_RADIUS, FACE_TORUS_MINOR_RADIUS] {
                let (was, is) = (&arrays.reals[&id], &again.reals[&id]);
                assert_eq!(was.len(), is.len(), "{at}: array {id}");
                for (a, b) in was.iter().zip(is) {
                    assert!((a - b).abs() <= 8.0 * f64::EPSILON * (1.0 + a.abs()), "{at}: array {id}: {a} came back {b}");
                }
            }
        }
        assert!(super::check(&back, CheckLevel::All).unwrap().is_valid(), "{at}: the tool");
        let shaft = make_cylinder([0.0; 3], 15.75, 60.0).unwrap();
        let cut = boolean(&shaft, &back, BooleanOp::Subtract).unwrap().unwrap();
        let cut = from_arrays(&to_arrays(&cut).unwrap()).unwrap();
        assert!(super::check(&cut, CheckLevel::All).unwrap().is_valid(), "{at}: the shaft cut with it");
    }
}

/// Arrays that name a torus a thousandth off its NURBS form do not put that torus on the form: the
/// body comes back with the torus its form is (major radius 10), as before.
#[test]
fn a_torus_named_off_its_form_is_not_put_on_the_form() {
    use crate::flat::FACE_TORUS_MAJOR_RADIUS;
    let mut arrays = to_arrays(&make_torus([1.0, -1.0, 0.5], 10.0, 3.0).unwrap()).unwrap();
    for r in arrays.reals.get_mut(&FACE_TORUS_MAJOR_RADIUS).unwrap() {
        *r += 1e-3;
    }
    let again = to_arrays(&from_arrays(&arrays).unwrap()).unwrap();
    let radii = &again.reals[&FACE_TORUS_MAJOR_RADIUS];
    assert!(!radii.is_empty() && radii.iter().all(|r| (r - 10.0).abs() < 1e-9), "{radii:?}");
}

/// What the record of `body` writes for each face on an implicit offset surface, which it cannot
/// hold (#21926).
#[derive(Debug)]
struct StandIn {
    /// The face's index.
    face: usize,
    /// The tolerance the record states for the face: the face's tolerance on its import.
    stated: f64,
    /// The farthest the written spline lies from the face's surface at the same parameters, on a
    /// 61 x 61 grid over the face's parameter box (at least its distance from the face there).
    sampled: f64,
    /// The certified bound of the distance from the offset of the kernel's certified net of it
    /// within half the stated tolerance, at every parameter; `None` unless that net is the
    /// spline written, coordinate for coordinate.
    certified: Option<f64>,
}

/// Each offset face of `body` and its spline in `body`'s record, read back as the app reads a
/// record ([`import::arrays`]).
fn offset_stand_ins(body: &Body) -> Vec<StandIn> {
    let rec = record(&to_arrays(body).unwrap());
    let back = from_arrays(&import::arrays(&rec, true).unwrap()).unwrap();
    let stated: Vec<f64> = super::with_body("face tolerances", &back, |_, b| {
        let mut t = Vec::new();
        for &f in b.face_list() {
            t.push(b.face(f)?.tolerance);
        }
        Ok(Some(t))
    })
    .unwrap()
    .unwrap();
    // each offset face's index, its certified net and bound, and its points on the grid
    let offsets = super::with_body("offset faces", body, |k, b| {
        let mut out = Vec::new();
        for (i, &f) in b.face_list().iter().enumerate() {
            let face = b.face(f)?;
            let (Some(s), Some(&t)) = (face.surface.as_ref(), stated.get(i)) else { continue };
            let Some((net, bound)) = solidcore_ops::offset::certified_offset_net(k, s, t / 2.0)? else { continue };
            let d = face.uv_domain;
            let mut grid = Vec::new();
            for a in 0..=60 {
                for c in 0..=60 {
                    let at = |lo: f64, hi: f64, n: i32| lo + (hi - lo) * f64::from(n) / 60.0;
                    let uv = solidcore::Point2::new(at(d.u_min(), d.u_max(), a), at(d.v_min(), d.v_max(), c));
                    grid.push((uv, s.evaluate_point(k, uv)?));
                }
            }
            out.push((i, net, bound, grid));
        }
        Ok(Some(out))
    })
    .unwrap()
    .unwrap();
    super::with_body("stand-ins", &back, |k, b| {
        let mut out = Vec::new();
        for (i, net, bound, grid) in &offsets {
            let written = b.face(b.face_list()[*i])?.surface.clone().unwrap();
            let mut sampled: f64 = 0.0;
            for (uv, p) in grid {
                sampled = sampled.max((written.evaluate_point(k, *uv)? - *p).length());
            }
            let nurb = |s| super::convert::bspline_surface_part(k, s).unwrap().and_then(|p| p.gw_nurb().cloned()).unwrap();
            let same = super::convert::same_nurbs(&nurb(&written), &nurb(net), 0.0);
            out.push(StandIn { face: *i, stated: stated[*i], sampled, certified: same.then_some(*bound) });
        }
        Ok(Some(out))
    })
    .unwrap()
    .unwrap()
}

/// The largest edge tolerance `body`'s record writes (EDGE_TOLERANCE is twice the tolerance).
fn largest_edge_tolerance(body: &Body) -> f64 {
    to_arrays(body).unwrap().reals[&crate::flat::EDGE_TOLERANCE].iter().fold(0.0, |m: f64, &t| m.max(t / 2.0))
}

/// #21926: the fin of `examples/regressions/fins_joined_into_their_hub.json` without its trim, a
/// sheet lofted between two straight sections that twist, thickened 1.2 about itself: six faces,
/// one of them on an implicit offset surface (the sheet moved back 0.6), every tolerance 1e-5. A
/// closed shell 0.2 inward (the kernel's, not through the record) keeps that face on it, with its
/// tolerance of 1e-5, and raises edge tolerances to about 3.2e-5. The record's spline for the
/// face was fitted to half the largest tolerance and came 1.56e-5 off the face. It is now the
/// kernel's net of the offset certified within half the face's own tolerance: within the 1e-5 the
/// record states for the face at every parameter, and so at every point sampled. The thickened
/// body, whose spline was within its face's tolerance before (1.8e-6 sampled), is the control.
#[test]
fn an_offset_face_is_written_within_the_tolerance_its_record_states() {
    use crate::sweeps::Curve;
    let line = |start: [f64; 3], end: [f64; 3]| vec![Curve::Line { start, end }];
    let sheet = super::sweep::loft(
        &[
            line([-9.641814145318946, 0.0, -16.68578720333266], [9.641814145300943, 0.0, 6.29554609021679]),
            line([-12.990381056768296, 30.0, -12.5], [12.9903810567683, 30.0, 2.5]),
        ],
        1,
        false,
    )
    .unwrap();
    let fin = super::offset::thicken(&sheet, 1.2, true).unwrap();
    let shelled = Body::from_kernel(super::run("shell_brep", |k| super::offset::shell_brep_full(k, &fin.kernel(), -0.2, &[])).unwrap());
    let mut written = Vec::new();
    for (name, body, faces) in [("thickened", &fin, [6, 12, 8]), ("shelled", &shelled, [12, 24, 16])] {
        assert_eq!(counts(body).unwrap(), faces, "{name}");
        let stand_ins = offset_stand_ins(body);
        let [s] = stand_ins.as_slice() else { panic!("{name}: one offset face, not {stand_ins:?}") };
        assert_eq!((s.face, s.stated), (5, 1e-5), "{name}: {s:?}");
        assert!(s.sampled <= s.stated, "{name}: the spline is outside the face's tolerance: {s:?}");
        written.extend(stand_ins.into_iter().map(|s| (name, s)));
    }
    for (name, s) in written {
        let bound = s.certified.unwrap_or_else(|| panic!("{name}: the written spline is not the certified net: {s:?}"));
        assert!(bound <= s.stated / 2.0, "{name}: {s:?}");
    }
    // the shell raised edge tolerances past twice the face's: half the largest is past the face's
    let largest = largest_edge_tolerance(&shelled);
    assert!(largest > 2.0 * 1e-5 && largest_edge_tolerance(&fin) == 1e-5, "{largest}");
}

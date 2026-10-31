//! solidcore's primitives through the flat arrays: each exports to a valid record of the right
//! volume, the record comes back to an equal body, and (with SMLib built) the arrays are the C++
//! kernel's own. A body with a sealed void crosses with each region's enclosing shell first,
//! whatever order its arrays list the shells in (the capi's `PutEnclosingShellsFirst`).

use std::f64::consts::PI;

use solidcraft_geom::brep as b;

use super::boolean::boolean;
use super::{Body, from_arrays, make_box, make_cone, make_cylinder, make_sphere, make_torus, to_arrays, volume};
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
    rec.validate().unwrap();
    rec.check_geometry().unwrap();
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

/// The C++ kernel's arrays of the same primitive: every id, ints exact, reals within 1e-12.
#[cfg(feature = "native")]
fn matches_cpp(name: &str) {
    use crate::flat::{TOLERANCE, VERTEX_TOLERANCE, WIRE_EDGE_NURBS_FORM};
    use crate::{Brep, ffi};
    // built by the C API whatever `Op::Primitive` routes to
    crate::init().unwrap();
    let cpp = match name {
        "box" => ffi::make_box(&[1.0, -2.0, 3.0], &[10.0, 20.0, 30.0]),
        "cylinder" => ffi::make_cylinder(&[1.0, 2.0, -3.0], 5.0, 12.0),
        "sphere" => ffi::make_sphere(&[1.0, 2.0, 3.0], 7.0),
        "cone" => ffi::make_cone(&[0.0, 1.0, 2.0], 6.0, 0.0, 9.0),
        "frustum" => ffi::make_cone(&[0.0, 1.0, 2.0], 6.0, 2.0, 9.0),
        "torus" => ffi::make_torus(&[1.0, -1.0, 0.5], 10.0, 3.0),
        _ => panic!("{name}"),
    }
    .map(Brep::made)
    .unwrap();
    let flat = cpp.export().unwrap();
    let ours = to_arrays(&case(name).body).unwrap();
    for id in 1..=WIRE_EDGE_NURBS_FORM {
        assert_eq!(ours.ints.get(&id).unwrap(), &flat.get_i64(id).unwrap(), "{name}: int array {id}");
    }
    for id in TOLERANCE..=VERTEX_TOLERANCE {
        let (a, b) = (ours.reals.get(&id).unwrap(), flat.get_f64(id).unwrap());
        assert_eq!(a.len(), b.len(), "{name}: real array {id} length");
        for (i, (x, y)) in a.iter().zip(&b).enumerate() {
            assert!((x - y).abs() <= 1e-12 * x.abs().max(1.0), "{name}: real array {id}[{i}]: {x} vs {y}");
        }
    }
}

/// The record of solidcore's arrays, built in the C++ kernel, measures the analytic volume.
#[cfg(feature = "native")]
fn record_measures_in_cpp(name: &str) {
    let c = case(name);
    let rec = record(&to_arrays(&c.body).unwrap());
    let v = crate::Brep::from_record(&rec, true).unwrap().volume(1e-8).unwrap();
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
            #[cfg(feature = "native")]
            #[test]
            fn its_record_measures_in_the_cpp_kernel() {
                let _cpp = crate::route::on_cpp();
                super::record_measures_in_cpp($name);
            }
            #[cfg(feature = "native")]
            #[test]
            fn arrays_match_the_cpp_kernel() {
                let _cpp = crate::route::on_cpp();
                super::matches_cpp($name);
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

/// `Validate` against the C++ one: each array of each body grown, emptied and given hostile values.
/// What ours refuses the C++ kernel does not build, and what the C++ kernel refuses with
/// `SM_ERR_INVALID_INPUT`, its `Validate`'s answer (its build can answer it too), ours does not build.
#[cfg(feature = "native")]
#[test]
fn validate_refuses_what_the_cpp_kernel_refuses() {
    let _cpp = crate::route::on_cpp();
    /// `SM_ERR_INVALID_INPUT`.
    const INVALID_INPUT: i32 = 1007;
    use crate::flat::Flat;
    use crate::{Brep, Error, ffi};
    crate::init().unwrap();
    let plate = ffi::make_box(&[0.0; 3], &[40.0, 30.0, 5.0]).unwrap();
    let rod = ffi::make_cylinder(&[20.0, 15.0, -1.0], 6.0, 7.0).unwrap();
    let cone = ffi::make_cone(&[0.0, 1.0, 2.0], 6.0, 2.0, 9.0).unwrap();
    let bodies = [
        ("holed plate", Brep::made(ffi::ops::boolean(&plate, &rod, ffi::ops::BOOLEAN_SUBTRACT).unwrap())),
        ("frustum", Brep::made(cone)),
        ("sphere", Brep::made(ffi::make_sphere(&[1.0, 2.0, 3.0], 7.0).unwrap())),
        ("torus", Brep::made(ffi::make_torus(&[1.0, -1.0, 0.5], 10.0, 3.0).unwrap())),
    ];
    let (mut compared, mut refusals) = (0usize, 0usize);
    for (name, body) in &bodies {
        let good = body.cpp_arrays().unwrap();
        let mut hostile: Vec<(String, Arrays)> = Vec::new();
        for (id, v) in &good.ints {
            let mut grown = good.clone();
            grown.ints.get_mut(id).unwrap().push(1);
            hostile.push((format!("int {id} grown"), grown));
            let mut none = good.clone();
            none.ints.get_mut(id).unwrap().clear();
            hostile.push((format!("int {id} emptied"), none));
            for at in [0, v.len().saturating_sub(1)] {
                for x in [-1, 0, 1, 2, 7, 1_000_000, i64::MAX, i64::MIN] {
                    if v.get(at).is_some_and(|y| *y != x) {
                        let mut a = good.clone();
                        a.ints.get_mut(id).unwrap()[at] = x;
                        hostile.push((format!("int {id}[{at}] = {x}"), a));
                    }
                }
            }
        }
        for (id, v) in &good.reals {
            let mut grown = good.clone();
            grown.reals.get_mut(id).unwrap().push(1.0);
            hostile.push((format!("real {id} grown"), grown));
            let mut none = good.clone();
            none.reals.get_mut(id).unwrap().clear();
            hostile.push((format!("real {id} emptied"), none));
            for at in [0, v.len().saturating_sub(1)] {
                for x in [0.0, -1.0, 2.0, 1e10, -1e-13, f64::NAN, f64::INFINITY] {
                    if !v.is_empty() {
                        let mut a = good.clone();
                        a.reals.get_mut(id).unwrap()[at] = x;
                        hostile.push((format!("real {id}[{at}] = {x}"), a));
                    }
                }
            }
        }
        for (what, a) in hostile {
            let cpp = Flat::from_arrays(&a).unwrap().import().map(|_| ());
            let cpp_refused = cpp == Err(Error::Status(INVALID_INPUT));
            let ours = super::convert::validate_refuses(&a);
            compared += 1;
            refusals += usize::from(ours);
            assert!(!(ours && cpp.is_ok()), "{name}, {what}: refused here, built in C++");
            assert!(!(cpp_refused && !ours && from_arrays(&a).is_ok()), "{name}, {what}: refused in C++, built here");
        }
    }
    assert!(compared > 500 && refusals > 200, "{compared} compared, {refusals} refused");
}

/// The C++ kernel's own box arrays (what `Brep::core` converts) build in solidcore.
#[cfg(feature = "native")]
#[test]
fn a_cpp_box_comes_into_solidcore() {
    let _cpp = crate::route::on_cpp();
    crate::init().unwrap();
    let cpp = crate::ffi::make_box(&[1.0, -2.0, 3.0], &[10.0, 20.0, 30.0]).map(crate::Brep::made).unwrap();
    let body = from_arrays(&cpp.cpp_arrays().unwrap()).unwrap();
    let v = volume(&body, 1e-8).unwrap();
    assert!(rel(v, 6000.0) < 1e-9, "{v}");
    record(&to_arrays(&body).unwrap());
}

/// Each edge's and vertex's tolerance crosses the kernels as it is. One box edge given eight times
/// the box's tolerance, in arrays whose TOLERANCE is the largest (as the C++ kernel writes them),
/// keeps it round after round, and the B-rep and every other edge and vertex keep the box's. Every
/// crossing used to give every entity the largest, so a chain of blends on a mixed route grew the
/// whole body's tolerance fourfold per blend (fillets/2341).
#[test]
fn tolerances_cross_the_arrays_per_entity() {
    #[cfg(feature = "native")]
    let _cpp = crate::route::on_cpp();
    use crate::flat::{EDGE_TOLERANCE, TOLERANCE, VERTEX_TOLERANCE};
    let mut a = to_arrays(&case("box").body).unwrap();
    let base = a.reals[&TOLERANCE][0];
    assert!(a.reals[&EDGE_TOLERANCE].iter().chain(&a.reals[&VERTEX_TOLERANCE]).all(|&t| t == base), "{a:?}");
    a.reals.get_mut(&EDGE_TOLERANCE).unwrap()[0] = 8.0 * base;
    a.reals.get_mut(&TOLERANCE).unwrap()[0] = 8.0 * base;
    for round in 0..3 {
        let body = from_arrays(&a).unwrap();
        assert_eq!(body.0.lock().unwrap().tolerance(), base / 2.0, "round {round}");
        a = to_arrays(&body).unwrap();
        assert_eq!(a.reals[&TOLERANCE], [base], "round {round}");
        let edges = &a.reals[&EDGE_TOLERANCE];
        assert_eq!(edges[0], 8.0 * base, "round {round}");
        assert!(edges[1..].iter().chain(&a.reals[&VERTEX_TOLERANCE]).all(|&t| t == base), "round {round}: {a:?}");
        // and through the C++ kernel, which keeps them too
        #[cfg(feature = "native")]
        {
            let cpp = crate::flat::Flat::from_arrays(&a).unwrap().import().unwrap().cpp_arrays().unwrap();
            assert_eq!((&cpp.reals[&EDGE_TOLERANCE], &cpp.reals[&VERTEX_TOLERANCE]), (edges, &a.reals[&VERTEX_TOLERANCE]), "round {round}");
            a = cpp;
        }
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

/// The C++ kernel's sealed void: solidcore imports its arrays with the outer shell first, in
/// either shell order, at the C++ kernel's volume, and the C++ kernel reads solidcore's arrays
/// (either order) back as a body that passes its checks at that volume.
#[cfg(feature = "native")]
#[test]
fn a_sealed_void_matches_the_cpp_kernel() {
    let _cpp = crate::route::on_cpp();
    use crate::{Brep, ffi};
    crate::init().unwrap();
    let make = |o: f64, s: f64| ffi::make_box(&[o; 3], &[s; 3]).map(Brep::made).unwrap();
    let (outer, inner) = (make(-10.0, 20.0), make(-5.0, 10.0));
    let cpp = Brep::made(ffi::ops::boolean(outer.cpp().unwrap(), inner.cpp().unwrap(), ffi::ops::BOOLEAN_SUBTRACT).unwrap());
    let theirs = ffi::volume(cpp.cpp().unwrap(), 1e-9).unwrap();
    assert!(rel(theirs, SEALED_VOID_VOLUME) < 1e-9, "C++ {theirs}");
    let arrays = cpp.cpp_arrays().unwrap();
    outer_first(&arrays, "C++ export");
    for (at, a) in [("C++ arrays", arrays.clone()), ("C++ arrays, inner first", shells_reversed(&arrays))] {
        let body = from_arrays(&a).unwrap();
        outer_first(&to_arrays(&body).unwrap(), at);
        let v = volume(&body, 1e-9).unwrap();
        assert!(rel(v, theirs) < 1e-9, "{at}: solidcore {v} vs C++ {theirs}");
    }
    let ours = to_arrays(&sealed_void()).unwrap();
    for (at, a) in [("solidcore arrays", ours.clone()), ("solidcore arrays, inner first", shells_reversed(&ours))] {
        let h = crate::flat::Flat::from_arrays(&a).unwrap().import().unwrap();
        let v = ffi::volume(h.cpp().unwrap(), 1e-9).unwrap();
        assert!(rel(v, theirs) < 1e-9, "{at}: C++ {v} vs {theirs}");
        let mut buf = vec![0u8; 1 << 16];
        let (len, failures) = ffi::check::check(h.cpp().unwrap(), 2, &mut buf).unwrap();
        assert_eq!(failures, 0, "{at}: {}", String::from_utf8_lossy(&buf[..len]));
        outer_first(&h.cpp_arrays().unwrap(), &format!("{at}, C++ export"));
    }
}

/// C++ bodies through solidcore and back. Three crossings of the same C++ arrays are held equal,
/// ints exactly and reals to 1e-12 (the record too, where the record can say it):
/// - the C++ kernel's own: its import of them, exported again;
/// - solidcore's: `from_arrays`, then `to_arrays`;
/// - both: solidcore's arrays imported by the C++ kernel and exported again.
///
/// The first is the reference, not the arrays that went in: a crossing itself moves a few reals on
/// the C++ kernel too (UV trims made at export, about 1e-7 on a shelled box), so those are held to
/// the arrays that went in only at [`FIRST_CROSSING`]. Tolerances are excepted: they cross on
/// purpose as each entity's own (`own_tolerances`, TOLERANCE as twice the B-rep's) where the C++
/// export writes the largest over them all, so that a body does not grow its tolerance at every
/// crossing (`tolerances_cross_the_arrays_per_entity`); on a body an operation made the numbers
/// cannot agree. So is EXTENT, the bounding box padded by those tolerances.
#[cfg(feature = "native")]
mod cpp_round_trip {
    use serde_json::Value;
    use solidcraft_geom::brep as b;

    use super::{close, from_arrays, native, rel, to_arrays, volume};
    use crate::flat::{self, EDGE_TOLERANCE, EXTENT, Flat, TOLERANCE, VERTEX_TOLERANCE};
    use crate::import::Arrays;
    use crate::sweeps::{Curve, DraftCorner, SweepFrame};
    use crate::{Brep, ffi};

    const PICK_TOL: f64 = 1e-6;

    /// How far any crossing, the C++ kernel's own included, may move a real of the arrays that
    /// went in.
    const FIRST_CROSSING: f64 = 1e-6;

    fn cpp_box(o: [f64; 3], s: [f64; 3]) -> Brep {
        crate::init().unwrap();
        Brep::made(ffi::make_box(&o, &s).unwrap())
    }

    fn cpp_cylinder(o: [f64; 3], r: f64, h: f64) -> Brep {
        crate::init().unwrap();
        Brep::made(ffi::make_cylinder(&o, r, h).unwrap())
    }

    fn cut(a: &Brep, b: &Brep) -> Brep {
        Brep::made(ffi::ops::boolean(a.cpp().unwrap(), b.cpp().unwrap(), ffi::ops::BOOLEAN_SUBTRACT).unwrap())
    }

    /// `f` with the sweeps on the C++ kernel, whatever the route.
    fn on_cpp(f: impl FnOnce() -> crate::Result<Brep>) -> Brep {
        crate::init().unwrap();
        crate::sweeps::ON_CPP.with(|c| c.set(true));
        let r = f();
        crate::sweeps::ON_CPP.with(|c| c.set(false));
        r.unwrap()
    }

    /// The record as JSON, every `tolerance` (the B-rep's, an edge's, a vertex's) gone.
    fn without_tolerances(rec: &b::Brep) -> Value {
        fn strip(v: &mut Value) {
            match v {
                Value::Object(m) => {
                    m.remove("tolerance");
                    m.values_mut().for_each(strip);
                }
                Value::Array(a) => a.iter_mut().for_each(strip),
                _ => {}
            }
        }
        let mut v = serde_json::to_value(rec).unwrap();
        strip(&mut v);
        v
    }

    /// `want` and `got` agree: ints exact, reals within `tol`, tolerances and EXTENT excepted.
    fn same_arrays(at: &str, want: &Arrays, got: &Arrays, tol: f64) {
        assert_eq!(want.ints.keys().collect::<Vec<_>>(), got.ints.keys().collect::<Vec<_>>(), "{at}: int arrays");
        for (id, v) in &want.ints {
            assert_eq!(v, &got.ints[id], "{at}: int array {id}");
        }
        assert_eq!(want.reals.keys().collect::<Vec<_>>(), got.reals.keys().collect::<Vec<_>>(), "{at}: real arrays");
        for (id, v) in &want.reals {
            if [TOLERANCE, EXTENT, EDGE_TOLERANCE, VERTEX_TOLERANCE].contains(id) {
                assert_eq!(v.len(), got.reals[id].len(), "{at}: real array {id} length");
                continue;
            }
            let (x, y) = (serde_json::to_value(v).unwrap(), serde_json::to_value(&got.reals[id]).unwrap());
            close(&x, &y, tol, &format!("{at}: real array {id}"));
        }
    }

    /// [`same_arrays`], and their records the same, valid and with tolerances excepted.
    fn same_records(at: &str, want: &Arrays, got: &Arrays, tol: f64) -> b::Brep {
        same_arrays(at, want, got, tol);
        let ((w, _), (g, _)) = (native(want).unwrap(), native(got).unwrap());
        w.validate().unwrap();
        g.validate().unwrap();
        close(&without_tolerances(&w), &without_tolerances(&g), tol, at);
        w
    }

    /// The three crossings of `theirs`: the C++ kernel's own, solidcore's, and solidcore's arrays
    /// back through the C++ kernel.
    fn crossings(theirs: &Arrays) -> [Arrays; 3] {
        let own = Flat::from_arrays(theirs).unwrap().import().unwrap().cpp_arrays().unwrap();
        let ours = to_arrays(&from_arrays(theirs).unwrap()).unwrap();
        let back = Flat::from_arrays(&ours).unwrap().import().unwrap().cpp_arrays().unwrap();
        [own, ours, back]
    }

    /// Arrays the record cannot say (wire and point shells): the crossings' arrays only.
    fn crosses_as_arrays(name: &str, theirs: &Arrays) {
        let [own, ours, back] = crossings(theirs);
        same_arrays(&format!("{name}, solidcore's arrays"), &own, &ours, 1e-12);
        same_arrays(&format!("{name}, back through C++"), &own, &back, 1e-12);
    }

    /// `cpp` across: returns its record, for the case to say what it holds.
    fn crosses(name: &str, cpp: &Brep) -> b::Brep {
        let theirs = cpp.cpp_arrays().unwrap();
        let [own, ours, back] = crossings(&theirs);
        same_records(&format!("{name}, solidcore's arrays"), &own, &ours, 1e-12);
        same_records(&format!("{name}, back through C++"), &own, &back, 1e-12);
        same_records(&format!("{name}, the arrays that went in"), &theirs, &ours, FIRST_CROSSING)
    }

    /// [`crosses`], and solidcore measures the C++ kernel's volume.
    fn crosses_with_volume(name: &str, cpp: &Brep) -> b::Brep {
        let rec = crosses(name, cpp);
        let body = from_arrays(&cpp.cpp_arrays().unwrap()).unwrap();
        let (v, want) = (volume(&body, 1e-8).unwrap(), ffi::volume(cpp.cpp().unwrap(), 1e-8).unwrap());
        assert!(rel(v, want) < 1e-7, "{name}: solidcore {v} vs C++ {want}");
        rec
    }

    fn has_face(rec: &b::Brep, f: impl Fn(&b::Face) -> bool) -> bool {
        rec.faces.iter().any(f)
    }

    /// A 20 mm box with three edges at one corner rounded: rolling-ball faces and a corner patch,
    /// on an operation's tolerances.
    #[test]
    fn a_filleted_box() {
        let _cpp = crate::route::on_cpp();
        let a = cpp_box([0.0; 3], [20.0; 3]);
        let edges = [[10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]];
        let f = Brep::made(ffi::ops::pick_op(ffi::ops::Pick::Fillet, a.cpp().unwrap(), &edges, PICK_TOL, 2.0).unwrap());
        let rec = crosses_with_volume("filleted box", &f);
        assert!(rec.faces.len() > 6, "{}", rec.faces.len());
    }

    /// A plate with a round hole: inner loops, and a cylinder face looking at its axis.
    #[test]
    fn a_holed_plate() {
        let _cpp = crate::route::on_cpp();
        let p = cut(&cpp_box([0.0; 3], [40.0, 30.0, 5.0]), &cpp_cylinder([20.0, 15.0, -1.0], 6.0, 7.0));
        let rec = crosses_with_volume("holed plate", &p);
        assert!(has_face(&rec, |f| f.loops.len() > 1), "no inner loop");
    }

    /// A square swept along a cubic B-spline path: B-spline walls that are nothing analytic.
    #[test]
    fn a_swept_nurbs_face() {
        let _cpp = crate::route::on_cpp();
        let c = [[-2.0, -2.0, 0.0], [2.0, -2.0, 0.0], [2.0, 2.0, 0.0], [-2.0, 2.0, 0.0]];
        let square: Vec<Curve> = (0..4).map(|i| Curve::Line { start: c[i], end: c[(i + 1) % 4] }).collect();
        let points = [[0.0, 0.0, 0.0, 1.0], [0.0, 10.0, 20.0, 1.0], [10.0, -10.0, 40.0, 1.0], [10.0, 0.0, 60.0, 1.0]];
        let path = [Curve::Nurbs { degree: 3, points: points.to_vec(), knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0] }];
        let s = on_cpp(|| Brep::sweep(&square, &path, SweepFrame::Follow, true));
        let rec = crosses_with_volume("swept square", &s);
        assert!(has_face(&rec, |f| matches!(f.surface, b::Surface::Spline { .. } | b::Surface::RationalSpline { .. })), "no NURBS face");
    }

    /// A cylinder sliced by a plane leaning 45 degrees: the cut's edge an ellipse.
    #[test]
    fn an_ellipse_edge() {
        let _cpp = crate::route::on_cpp();
        let cyl = cpp_cylinder([0.0, 0.0, -30.0], 5.0, 60.0);
        let (s, c) = std::f64::consts::FRAC_PI_4.sin_cos();
        // About y through the origin; the box's underside is the plane z = 0 turned.
        let turn = [[c, 0.0, s, 0.0], [0.0, 1.0, 0.0, 0.0], [-s, 0.0, c, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let tool = Brep::made(ffi::transform(cpp_box([-50.0, -50.0, 0.0], [100.0; 3]).cpp().unwrap(), &turn).unwrap());
        let rec = crosses_with_volume("leaning cut", &cut(&cyl, &tool));
        assert!(rec.edges.iter().any(|e| matches!(e.curve, b::Curve::Ellipse { .. })), "no ellipse edge");
    }

    /// A 20 mm box with a 10 mm void sealed inside: two shells in the solid's region.
    #[test]
    fn a_sealed_void() {
        let _cpp = crate::route::on_cpp();
        let v = cut(&cpp_box([-10.0; 3], [20.0; 3]), &cpp_box([-5.0; 3], [10.0; 3]));
        let rec = crosses_with_volume("sealed void", &v);
        assert!(rec.regions.iter().any(|r| r.shells.len() == 2), "{:?}", rec.regions);
        assert!(rec.regions.iter().any(|r| r.void), "{:?}", rec.regions);
    }

    /// One planar face, and an open extrusion: a sheet's shells, no solid.
    #[test]
    fn sheets() {
        let _cpp = crate::route::on_cpp();
        let c = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 6.0, 0.0], [0.0, 6.0, 0.0]];
        let rect: Vec<Curve> = (0..4).map(|i| Curve::Line { start: c[i], end: c[(i + 1) % 4] }).collect();
        let rec = crosses("planar sheet", &on_cpp(|| Brep::planar_faces(&rect)));
        assert_eq!(rec.faces.len(), 1);
        let open = on_cpp(|| Brep::extrude(&rect, [0.0, 0.0, 1.0], 5.0, 0.0, DraftCorner::default(), false));
        let rec = crosses("open extrusion", &open);
        assert_eq!(rec.faces.len(), 4);
    }

    /// A C++ box's arrays with one wire edge of each kind (a line, a whole circle, a half ellipse,
    /// a cubic B-spline) and one point, each a shell of its own in the infinite region. The C API
    /// gives no body without a face (`take`), so these lie beside a box.
    fn box_with_wires_and_a_point() -> Arrays {
        let tau = std::f64::consts::TAU;
        let mut a = cpp_box([20.0; 3], [5.0; 3]).cpp_arrays().unwrap();
        let first_new = a.ints[&flat::VERTEX_POINT_TYPE].len() as i64;
        a.ints.get_mut(&flat::REGION_SHELL_COUNT).unwrap()[0] += 5;
        for (id, v) in
            [(flat::SHELL_FACEUSE_COUNT, [0, 0, 0, 0, 0]), (flat::SHELL_WIRE_EDGE_COUNT, [1, 1, 1, 1, 0]), (flat::SHELL_POINT_TYPE, [0, 0, 0, 0, 1])]
        {
            a.ints.get_mut(&id).unwrap().splice(1..1, v);
        }
        a.ints.insert(
            flat::WIRE_EDGE_CURVE_TYPE,
            [flat::CURVE_LINE, flat::CURVE_CIRCLE, flat::CURVE_ELLIPSE, flat::CURVE_NURBS].map(i64::from).to_vec(),
        );
        a.ints.insert(flat::WIRE_EDGE_VERTICES, [0, 1, 2, 2, 3, 4, 5, 6].map(|v| first_new + v).to_vec());
        a.ints.insert(flat::WIRE_EDGE_NURBS_VERTEX_COUNT, vec![4]);
        a.ints.insert(flat::WIRE_EDGE_NURBS_ORDER, vec![4]);
        let points = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [4.0, 0.0, 5.0], [3.0, 0.0, -5.0], [-3.0, 0.0, -5.0], [0.0, 9.0, 0.0], [9.0, 9.0, 3.0]];
        a.ints.get_mut(&flat::VERTEX_POINT_TYPE).unwrap().extend([1; 7]);
        a.reals.get_mut(&flat::VERTEX_POSITION).unwrap().extend(points.concat());
        let base = a.reals[&TOLERANCE][0];
        a.reals.get_mut(&VERTEX_TOLERANCE).unwrap().extend([base; 7]);
        let reals: [(i32, Vec<f64>); 17] = [
            (flat::SHELL_POINT_POSITION, vec![1.0, 2.0, 3.0]),
            (flat::WIRE_EDGE_RANGE, vec![0.0, 10.0, 0.0, tau, 0.0, std::f64::consts::PI, 0.0, 1.0]),
            (flat::WIRE_EDGE_LINE_ORIGIN, vec![0.0; 3]),
            (flat::WIRE_EDGE_LINE_DIRECTION, vec![1.0, 0.0, 0.0]),
            (flat::WIRE_EDGE_CIRCLE_CENTER, vec![0.0, 0.0, 5.0]),
            (flat::WIRE_EDGE_CIRCLE_AXIS, vec![0.0, 0.0, 1.0]),
            (flat::WIRE_EDGE_CIRCLE_REF_DIRECTION, vec![1.0, 0.0, 0.0]),
            (flat::WIRE_EDGE_CIRCLE_RADIUS, vec![4.0]),
            (flat::WIRE_EDGE_ELLIPSE_CENTER, vec![0.0, 0.0, -5.0]),
            (flat::WIRE_EDGE_ELLIPSE_AXIS, vec![0.0, 0.0, 1.0]),
            (flat::WIRE_EDGE_ELLIPSE_REF_DIRECTION, vec![1.0, 0.0, 0.0]),
            (flat::WIRE_EDGE_ELLIPSE_X_RADIUS, vec![3.0]),
            (flat::WIRE_EDGE_ELLIPSE_Y_RADIUS, vec![2.0]),
            (flat::WIRE_EDGE_NURBS_CVS, [points[5], [3.0, 12.0, 0.0], [6.0, 6.0, 2.0], points[6]].concat()),
            (flat::WIRE_EDGE_NURBS_KNOTS, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]),
            (flat::WIRE_EDGE_NURBS_WEIGHTS, Vec::new()),
            (EXTENT, vec![-3.0, 0.0, -5.0, 25.0, 25.0, 25.0]),
        ];
        a.reals.extend(reals);
        a
    }

    /// Wire edges no face uses and a point shell. The record says no such shell from SMLib, so
    /// arrays only.
    #[test]
    fn wire_edges_and_a_point_shell() {
        let _cpp = crate::route::on_cpp();
        let theirs = Flat::from_arrays(&box_with_wires_and_a_point()).unwrap().import().unwrap().cpp_arrays().unwrap();
        assert_eq!(theirs.ints[&flat::WIRE_EDGE_CURVE_TYPE].len(), 4);
        assert_eq!(theirs.reals[&flat::SHELL_POINT_POSITION], [1.0, 2.0, 3.0]);
        crosses_as_arrays("wire edges and a point", &theirs);
    }

    /// Two boxes apart, united: two lumps.
    #[test]
    fn a_multi_lump_body() {
        let _cpp = crate::route::on_cpp();
        let (a, b) = (cpp_box([0.0; 3], [10.0; 3]), cpp_box([30.0, 0.0, 0.0], [5.0; 3]));
        let u = Brep::made(ffi::ops::boolean(a.cpp().unwrap(), b.cpp().unwrap(), ffi::ops::BOOLEAN_UNION).unwrap());
        let rec = crosses_with_volume("two lumps", &u);
        assert_eq!(ffi::ops::solid_count(u.cpp().unwrap()).unwrap(), 2);
        assert_eq!(rec.faces.len(), 12);
    }

    /// A rounded box shelled: faces on the offsets of its rounds, on the operation's tolerances.
    #[test]
    fn a_shelled_rounded_box() {
        let _cpp = crate::route::on_cpp();
        let a = cpp_box([0.0; 3], [20.0; 3]);
        let r = ffi::ops::pick_op(ffi::ops::Pick::Fillet, a.cpp().unwrap(), &[[10.0, 0.0, 0.0]], PICK_TOL, 2.0).unwrap();
        let s = Brep::made(ffi::ops::pick_op(ffi::ops::Pick::Shell, &r, &[[10.0, 10.0, 20.0]], PICK_TOL, -1.0).unwrap());
        crosses_with_volume("shelled rounded box", &s);
    }
}

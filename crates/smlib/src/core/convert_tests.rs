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

/// The arc of `radius` about `axis` through `center`, from `reference`, turning by `turn`.
fn arc_by(center: [f64; 3], axis: [f64; 3], reference: [f64; 3], radius: f64, turn: f64) -> crate::sweeps::Curve {
    crate::sweeps::Curve::Arc { center, axis, reference, radius, start: 0.0, end: turn }
}

fn line(start: [f64; 3], end: [f64; 3]) -> crate::sweeps::Curve {
    crate::sweeps::Curve::Line { start, end }
}

/// A pipe of diameter 5 along lines and arcs, one of them a 1° arc of radius 50.8, and a second
/// pipe joined to its end cap (#22146; the paths and profiles as SolidCraft sweeps them for the
/// issue's design). Each arc of a pipe is a torus face carrying its NURBS form. Read back from
/// its record (as a later feature reads its operands), each pipe still passes the check, and the
/// join is one solid of the volume Pappus gives: a disc of radius 2.5 along a tangent-continuous
/// path, π 2.5² L, with L = 517.852348653780 for the first path (9.684405810658 + 76.2 × 24.7278°
/// + 107.151927790051 + 26.035 × 193.3178° + 266.7 + 50.8 × 1° + 12.7) and 51.076121171858 for
/// the second (3.928214262990 + 124.0819 × 7° + 31.988436958318). Each pipe has a wall per
/// segment and two caps; each circle where walls meet or a cap ends is one edge with one vertex,
/// and each wall has one seam. The pipes touch on the first's end cap: the two caps go and their
/// circles become one. The first pipe's last wall and the second's first wall are cylinders of
/// radius 2.5 on one axis (both run along -y through (0, -127.39, 30.06)), so they become one face
/// and the circle between them goes; its vertex stays where the two seams meet:
/// (9 + 5 - 2 - 1, 15 + 7 - 1 - 1, 8 + 4 - 1) faces, edges and vertices.
#[test]
fn a_pipe_joins_the_end_of_a_pipe_bent_through_a_one_degree_arc() {
    use crate::check::CheckLevel;
    use crate::sweeps::SweepFrame;
    let first_path = [
        line([0.0, 0.0, 0.0], [0.0, 9.525, 1.74988311195596]),
        arc_by([0.0, 23.293639577690602, -73.1958610245836], [-1.0, -0.0, -0.0], [0.0, -0.18069080810617585, 0.9835399492984194], 76.2, 0.4315810318206319),
        line([0.0, 42.13786635337631, 0.6372962777188018], [0.0, 145.96155585164072, -25.861328744953276]),
        arc_by([0.0, 152.4, -0.6349999999999618], [1.0, 0.0, 0.0], [0.0, -0.2472995639853767, -0.9689390722086929], 26.034999999999993, 3.3740316177578684),
        line([0.0, 152.8543734015946, 25.39603474339669], [0.0, -113.805006896615, 30.050591540219166]),
        arc_by([0.0, -114.6915891436288, -20.741671373725517], [1.0, 0.0, 0.0], [0.0, 0.017452406437279636, 0.9998476951563913], 50.800000000000004, 0.01745329251993666),
        line([0.0, -114.69158914362866, 30.058328626274474], [0.0, -127.39158914362864, 30.058328626274502]),
    ];
    let first_profile = arc_by([0.0, 0.0, 0.0], [0.0, 0.9835399492984193, 0.18069080810617583], [-1.0, 0.0, 0.0], 2.5, 2.0 * PI);
    let second_path = [
        line([0.0, -127.39158914362864, 30.058328626274513], [-1.5407439555097887e-30, -131.319803406619, 30.058328626274523]),
        arc_by([-124.08194968884067, -131.319803406619, 30.058328626274523], [0.0, -3.00454106039183e-15, -1.0], [1.0, 0.0, 0.0], 124.08194968884064, 0.12217304763960368),
        line([-0.9248880370297666, -146.44158914362865, 30.05832862627457], [-4.82329784569696, -178.19158914362862, 30.058328626274665]),
    ];
    let second_profile =
        arc_by([0.0, -127.39158914362864, 30.058328626274513], [-3.922250295830076e-31, -1.0, 2.713228027507837e-15], [1.0, -3.922250295830076e-31, 0.0], 2.5, 2.0 * PI);
    let area = PI * 2.5 * 2.5;
    let mut pipes = Vec::new();
    for (name, profile, path, length) in
        [("first", first_profile, &first_path[..], 517.852348653780), ("second", second_profile, &second_path[..], 51.076121171858)]
    {
        let (made, _) = super::sweep::sweep_placed(&[profile], path, SweepFrame::Follow, true).unwrap();
        let walls = path.len();
        assert_eq!(counts(&made).unwrap(), [walls + 2, 2 * walls + 1, walls + 1], "the {name} pipe's faces, edges and vertices");
        let back = from_arrays(&import::arrays(&record(&to_arrays(&made).unwrap()), true).unwrap()).unwrap();
        let check = super::check(&back, CheckLevel::All).unwrap();
        assert!(check.is_valid(), "the {name} pipe read back from its record: {check}");
        let v = volume(&back, 1e-8).unwrap();
        assert!(rel(v, area * length) < 1e-9, "the {name} pipe's volume {v} vs {}", area * length);
        pipes.push(back);
    }
    let joined = boolean(&pipes[0], &pipes[1], BooleanOp::Union).unwrap().unwrap();
    assert_eq!(counts(&joined).unwrap(), [11, 20, 11], "the join's faces, edges and vertices");
    let check = super::check(&joined, CheckLevel::All).unwrap();
    assert!(check.is_valid(), "the join: {check}");
    let want = area * (517.852348653780 + 51.076121171858);
    let v = volume(&joined, 1e-8).unwrap();
    assert!(rel(v, want) < 1e-9, "the join's volume {v} vs {want}");
}

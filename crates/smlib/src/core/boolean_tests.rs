//! Booleans on solidcore, against analytic volumes: every result exports to a record that
//! validates and counts the solids it should. With SMLib built, the same case on the C++ kernel
//! gives the same volume, counts, faces by surface kind and solids (parity only: the expected
//! values are the analytic ones).
//!
//! Curved operands are built on solidcore itself.

use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::sync::mpsc;
use std::time::Duration;

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::vector3d::Vec3;
use solidcore::mass::integrator::properties::brep_area;
use solidcore::primitive::primitive_creation::p1::create_rectangle;

use super::boolean::{boolean, boolean2d, edge_closes, imprint, union_all};
use super::{Body, counts, make_box, make_cylinder, make_sphere, run, solid_count, to_arrays, volume, with_body};
use crate::export::native;
use crate::{BooleanOp, Error};

/// A case longer than this is a hang.
const TIMEOUT: Duration = Duration::from_secs(300);

/// An operand.
#[derive(Clone, Copy, Debug)]
enum Shape {
    Box([f64; 3], [f64; 3]),
    Cylinder([f64; 3], f64, f64),
    Sphere([f64; 3], f64),
}

impl Shape {
    fn core(self) -> Body {
        match self {
            Shape::Box(o, s) => make_box(o, s),
            Shape::Cylinder(o, r, h) => make_cylinder(o, r, h),
            Shape::Sphere(c, r) => make_sphere(c, r),
        }
        .unwrap()
    }
}

/// What a case does.
#[derive(Clone, Debug)]
enum Op {
    Boolean(Shape, Shape, BooleanOp),
    UnionAll(Vec<Shape>),
    Imprint(Shape, Shape),
}

/// A case: the operation, its analytic volume (`None`: the result is empty), the faces it
/// must have where they are known analytically, and its solids (`smlib_brep_solid_count`).
struct Case {
    op: Op,
    volume: Option<f64>,
    faces: Option<usize>,
    solids: usize,
}

const A: Shape = Shape::Box([0.0; 3], [10.0; 3]);

fn case(name: &str) -> Case {
    let plain = |op, volume: Option<f64>, faces| Case { op, volume, faces, solids: 1 };
    match name {
        // |A| + |B| - |A ∩ B|: two 10 mm cubes overlapping in a 5 mm cube.
        "union_overlapping" => plain(Op::Boolean(A, Shape::Box([5.0; 3], [10.0; 3]), BooleanOp::Union), Some(1000.0 + 1000.0 - 125.0), Some(12)),
        // Two 10 mm cubes 10 mm apart: one body of two solids, 12 faces.
        "union_disjoint" => {
            Case { solids: 2, ..plain(Op::Boolean(A, Shape::Box([20.0, 0.0, 0.0], [10.0; 3]), BooleanOp::Union), Some(2000.0), Some(12)) }
        }
        "intersect_overlapping" => plain(Op::Boolean(A, Shape::Box([5.0; 3], [10.0; 3]), BooleanOp::Intersect), Some(125.0), Some(6)),
        // A slot 5 mm wide and 5 mm deep from the top, right through a 30 x 20 x 10 block.
        "subtract_through_slot" => plain(
            Op::Boolean(Shape::Box([0.0; 3], [30.0, 20.0, 10.0]), Shape::Box([10.0, -1.0, 5.0], [5.0, 22.0, 6.0]), BooleanOp::Subtract),
            Some(6000.0 - 5.0 * 20.0 * 5.0),
            Some(10),
        ),
        // A notch whose tool is flush with the block's front, back and top faces.
        "subtract_flush_notch" => plain(
            Op::Boolean(Shape::Box([0.0; 3], [30.0, 20.0, 10.0]), Shape::Box([10.0, 0.0, 5.0], [10.0, 20.0, 5.0]), BooleanOp::Subtract),
            Some(6000.0 - 10.0 * 20.0 * 5.0),
            Some(10),
        ),
        // A 4 mm cube inside a 10 mm one: a void, still one solid.
        "subtract_inside_void" => plain(Op::Boolean(A, Shape::Box([3.0; 3], [4.0; 3]), BooleanOp::Subtract), Some(1000.0 - 64.0), Some(12)),
        // abc - πr²c: a 3 mm hole through a 20 x 20 x 10 block.
        "subtract_cylinder_hole" => plain(
            Op::Boolean(Shape::Box([0.0; 3], [20.0, 20.0, 10.0]), Shape::Cylinder([10.0, 10.0, -1.0], 3.0, 12.0), BooleanOp::Subtract),
            Some(4000.0 - PI * 9.0 * 10.0),
            None,
        ),
        // Nine 10 mm cubes, each touching its neighbours face to face.
        "union_all_grid" => plain(
            Op::UnionAll((0..9).map(|i| Shape::Box([f64::from(i % 3) * 10.0, f64::from(i / 3) * 10.0, 0.0], [10.0; 3])).collect()),
            Some(9000.0),
            None,
        ),
        "intersect_disjoint" => plain(Op::Boolean(A, Shape::Box([20.0; 3], [10.0; 3]), BooleanOp::Intersect), None, None),
        // A 10 mm sphere less a cap of height h = 5: 4/3 πr³ - πh²(3r - h)/3.
        // (solidcore#1431, brep_volume of a whole sphere failing at 1e-8, does not touch the cut
        // sphere: it measures at 1e-8.)
        "sphere_minus_box" => plain(
            Op::Boolean(Shape::Sphere([0.0; 3], 10.0), Shape::Box([-20.0, -20.0, 5.0], [40.0, 40.0, 20.0]), BooleanOp::Subtract),
            Some(4.0 / 3.0 * PI * 1000.0 - PI * 25.0 * 25.0 / 3.0),
            Some(2),
        ),
        // A 4 mm square tool through the cube's top face: that face is cut in two (a square and
        // the ring round it), nothing else changes.
        "imprint_top_face" => plain(Op::Imprint(A, Shape::Box([3.0, 3.0, 8.0], [4.0; 3])), Some(1000.0), Some(7)),
        _ => panic!("{name}"),
    }
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs()
}

/// What a kernel's result measures.
#[derive(Debug, PartialEq)]
struct Measured {
    volume: f64,
    counts: [usize; 3],
    /// The record's faces by surface kind.
    kinds: BTreeMap<String, usize>,
    solids: usize,
}

fn kinds(rec: &solidcraft_geom::brep::Brep) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for f in &rec.faces {
        let v = serde_json::to_value(&f.surface).unwrap();
        *out.entry(v["kind"].as_str().unwrap_or("?").to_owned()).or_default() += 1;
    }
    out
}

/// The case on solidcore: `None` for an empty result.
fn on_core(c: &Case) -> crate::Result<Option<Body>> {
    match &c.op {
        Op::Boolean(a, b, op) => boolean(&a.core(), &b.core(), *op),
        Op::UnionAll(shapes) => {
            let bodies: Vec<Body> = shapes.iter().map(|s| s.core()).collect();
            union_all(&bodies.iter().collect::<Vec<_>>()).map(Some)
        }
        Op::Imprint(a, t) => imprint(&a.core(), &t.core()),
    }
}

/// Runs `f` on its own thread: a hang fails the test instead of stopping the run.
fn timed<T: Send + 'static>(name: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(TIMEOUT) {
        Ok(v) => v,
        // The case panicked: its own message is the failure.
        Err(mpsc::RecvTimeoutError::Disconnected) => match worker.join() {
            Err(p) => std::panic::resume_unwind(p),
            Ok(()) => panic!("{name}: no result"),
        },
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("{name}: solidcore hung (no result after {TIMEOUT:?})"),
    }
}

/// Runs the case on solidcore and checks it against the analytic answer; what it measured.
fn solidcore_case(name: &'static str) -> Option<Measured> {
    timed(name, move || {
        let c = case(name);
        let r = on_core(&c).unwrap_or_else(|e| panic!("{name}: solidcore failed: {e}"));
        let (Some(body), Some(expected)) = (&r, c.volume) else {
            assert!(r.is_none() && c.volume.is_none(), "{name}: solidcore gave {}, expected {:?}", r.is_some(), c.volume);
            return None;
        };
        let v = volume(body, 1e-8).unwrap_or_else(|e| panic!("{name}: volume: {e}"));
        assert!(rel(v, expected) < 1e-9, "{name}: solidcore volume {v} vs analytic {expected}");
        let n = counts(body).unwrap();
        if let Some(f) = c.faces {
            assert_eq!(n[0], f, "{name}: faces {n:?}");
        }
        let solids = solid_count(body).unwrap_or_else(|e| panic!("{name}: solid count: {e}"));
        assert_eq!(solids, c.solids, "{name}: solids");
        let (rec, _) = native(&to_arrays(body).unwrap()).unwrap_or_else(|e| panic!("{name}: record: {e:?}"));
        rec.validate().unwrap_or_else(|e| panic!("{name}: record: {e}"));
        Some(Measured { volume: v, counts: n, kinds: kinds(&rec), solids })
    })
}

/// The case on the C++ kernel, called directly (whatever the routing): what it measured.
#[cfg(feature = "native")]
fn cpp_case(name: &str) -> Option<Measured> {
    use crate::{Brep, ffi};
    crate::init().unwrap();
    let make = |s: Shape| -> Brep {
        match s {
            Shape::Box(o, z) => ffi::make_box(&o, &z),
            Shape::Cylinder(o, r, h) => ffi::make_cylinder(&o, r, h),
            Shape::Sphere(c, r) => ffi::make_sphere(&c, r),
        }
        .map(Brep::made)
        .unwrap()
    };
    let bool_op = |op| match op {
        BooleanOp::Union => ffi::ops::BOOLEAN_UNION,
        BooleanOp::Subtract => ffi::ops::BOOLEAN_SUBTRACT,
        BooleanOp::Intersect => ffi::ops::BOOLEAN_INTERSECT,
    };
    let c = case(name);
    let r = match &c.op {
        Op::Boolean(a, b, op) => ffi::ops::boolean(make(*a).cpp().unwrap(), make(*b).cpp().unwrap(), bool_op(*op)),
        Op::UnionAll(shapes) => {
            let bodies: Vec<Brep> = shapes.iter().map(|s| make(*s)).collect();
            let handles: Vec<&ffi::Handle> = bodies.iter().map(|b| b.cpp().unwrap()).collect();
            ffi::ops::boolean_union_all(&handles)
        }
        Op::Imprint(a, t) => ffi::ops::imprint(make(*a).cpp().unwrap(), make(*t).cpp().unwrap()),
    };
    let h = match r {
        Ok(h) => h,
        Err(ffi::ERR_EMPTY) => return None,
        Err(s) => panic!("{name}: C++ failed: {:?}", crate::error(s)),
    };
    let v = ffi::volume(&h, 1e-8).unwrap();
    let n = ffi::counts(&h).unwrap().map(|x| usize::try_from(x).unwrap());
    let solids = usize::try_from(ffi::ops::solid_count(&h).unwrap()).unwrap();
    let b = Brep::made(h);
    let (rec, _) = native(&b.cpp_arrays().unwrap()).unwrap();
    rec.validate().unwrap();
    Some(Measured { volume: v, counts: n, kinds: kinds(&rec), solids })
}

/// solidcore and C++ agree: volume within 1e-9, the same counts and faces by kind.
#[cfg(feature = "native")]
fn parity(name: &'static str) {
    let ours = solidcore_case(name);
    let cpp = cpp_case(name);
    match (ours, cpp) {
        (None, None) => {}
        (Some(a), Some(b)) => {
            assert!(rel(a.volume, b.volume) < 1e-9, "{name}: solidcore {} vs C++ {}", a.volume, b.volume);
            assert_eq!(a.counts, b.counts, "{name}: face, edge, vertex counts (solidcore vs C++)");
            assert_eq!(a.kinds, b.kinds, "{name}: faces by surface kind (solidcore vs C++)");
            assert_eq!(a.solids, b.solids, "{name}: solids (solidcore vs C++)");
        }
        (a, b) => panic!("{name}: solidcore {a:?} vs C++ {b:?}"),
    }
}

macro_rules! cases {
    ($($m:ident $(: $ign:meta)?),* $(,)?) => {$(
        mod $m {
            #[test]
            $(#[$ign])?
            fn solidcore_matches_the_analytic_volume() {
                super::solidcore_case(stringify!($m));
            }
            #[cfg(feature = "native")]
            #[test]
            $(#[$ign])?
            fn solidcore_matches_the_cpp_kernel() {
                let _cpp = crate::route::on_cpp();
                super::parity(stringify!($m));
            }
        }
    )*};
}

cases!(
    union_overlapping,
    union_disjoint,
    intersect_overlapping,
    subtract_through_slot,
    subtract_flush_notch,
    subtract_inside_void,
    subtract_cylinder_hole,
    union_all_grid,
    intersect_disjoint,
    sphere_minus_box,
    imprint_top_face,
);

/// A `w` x `h` planar sheet on z = 0 from `(x, y)`, solidcore's `CreateRectangle`.
fn rectangle(x: f64, y: f64, w: f64, h: f64) -> Body {
    let frame = Axis2Placement::new(Vec3::new(x, y, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
    run("create_rectangle", |k| create_rectangle(k, 0.0, w, h, &frame)).map(Body::new).unwrap()
}

fn area(b: &Body) -> f64 {
    with_body("brep_area", b, |k, b| brep_area(k, b, 1e-8)).unwrap()
}

/// Two 10 mm squares overlapping in a 5 mm one, as regions of their plane.
#[test]
fn boolean2d_combines_overlapping_squares() {
    for (op, expected) in [(BooleanOp::Union, 175.0), (BooleanOp::Intersect, 25.0), (BooleanOp::Subtract, 75.0)] {
        let (a, b) = (rectangle(0.0, 0.0, 10.0, 10.0), rectangle(5.0, 5.0, 10.0, 10.0));
        let r = timed("boolean2d", move || boolean2d(&a, &b, op).map(|r| r.map(|r| (area(&r), counts(&r).unwrap()))));
        let (got, n) = r.unwrap_or_else(|e| panic!("{op:?}: {e}")).unwrap_or_else(|| panic!("{op:?}: empty"));
        assert!(rel(got, expected) < 1e-9, "{op:?}: area {got} vs {expected}");
        assert_eq!(n[0], 1, "{op:?}: {n:?}");
    }
    let (a, far) = (rectangle(0.0, 0.0, 10.0, 10.0), rectangle(20.0, 0.0, 10.0, 10.0));
    assert!(boolean2d(&a, &far, BooleanOp::Intersect).unwrap().is_none(), "disjoint squares intersected");
}

/// `closed_nonmanifold`'s test of an edge, as the capi's: an edge whose wire or edgeuse query
/// fails makes the body not closed non-manifold (false), never an error that fails the boolean.
#[test]
fn closed_nonmanifold_takes_a_failed_edge_query_as_false() {
    let failed = || solidcore::SolidError::null_pointer("GetEdgeuses");
    assert!(edge_closes(Ok(false), Ok(4)));
    assert!(edge_closes(Ok(false), Ok(8)));
    assert!(!edge_closes(Ok(false), Ok(2)));
    assert!(!edge_closes(Ok(false), Ok(6)));
    assert!(!edge_closes(Ok(true), Ok(4)));
    assert!(!edge_closes(Ok(false), Err(failed())));
    assert!(!edge_closes(Err(failed()), Ok(4)));
}

/// Two 10 mm cubes touching along one edge, joined: a closed body whose two solids meet along
/// that edge (four edgeuses round it), 2000 mm³.
#[test]
fn cubes_touching_along_an_edge_join_closed_nonmanifold() {
    let r = timed("edge_touching", || {
        let a = Shape::Box([0.0; 3], [10.0; 3]).core();
        let b = Shape::Box([10.0, 10.0, 0.0], [10.0; 3]).core();
        boolean(&a, &b, BooleanOp::Union).map(|r| r.map(|r| (volume(&r, 1e-8).unwrap(), solid_count(&r).unwrap())))
    });
    let (v, solids) = r.unwrap().unwrap();
    assert!(rel(v, 2000.0) < 1e-9, "volume {v}");
    assert_eq!(solids, 2);
}

/// The capi's argument checks: the same body imprinted on itself, an empty union.
#[test]
fn hostile_operands_are_refused() {
    let a = A.core();
    assert_eq!(imprint(&a, &a).err(), Some(Error::Argument));
    assert_eq!(union_all(&[]).err(), Some(Error::Argument));
}

/// A tool that meets no face cuts none.
#[test]
fn imprint_of_a_far_tool_is_none() {
    let r = timed("imprint_far", || imprint(&A.core(), &Shape::Box([20.0; 3], [1.0; 3]).core()).map(|r| r.is_none()));
    assert!(r.unwrap());
}

/// An operand is never changed: the same body twice, and again after.
#[test]
fn operands_stay_as_they_were() {
    let r = timed("operands", || {
        let a = A.core();
        let b = Shape::Box([5.0; 3], [10.0; 3]).core();
        let u = boolean(&a, &b, BooleanOp::Union).map(|r| r.is_some());
        let same = boolean(&a, &a, BooleanOp::Union).map(|r| r.map(|r| volume(&r, 1e-8)));
        (u, same, counts(&a).unwrap(), volume(&a, 1e-8).unwrap(), counts(&b).unwrap())
    });
    let (u, same, ca, va, cb) = r;
    assert!(u.unwrap());
    assert_eq!(ca, [6, 12, 8]);
    assert_eq!(cb, [6, 12, 8]);
    assert!(rel(va, 1000.0) < 1e-12, "{va}");
    let v = same.unwrap().unwrap().unwrap();
    assert!(rel(v, 1000.0) < 1e-9, "A ∪ A: {v}");
}

/// `moved`: a closest-point query that fails, even with an error that is no SMLib status (an edge
/// without its curve here), counts as off the piece, as `SmApiEdgeClosestPoint` failing does in the
/// capi (smlib_history.cpp:246): the face is Modified, the operation stands.
#[test]
fn a_failed_closest_point_counts_as_off_the_piece() {
    use solidcore::query::api_queries::edge_closest_point;
    use solidcore::topology::edge;
    let body = A.core();
    super::with_body("moved", &body, |k, b| {
        let e = b.edge_list()[0];
        let (Some(s), Some(f)) = edge::start_end_vertices(b, e)? else { panic!("an edge with no ends") };
        let (s, f) = (b.vertex(s)?.point, b.vertex(f)?.point);
        let at = [s, f, s + (f - s) * 0.5];
        let tol = b.tolerance();
        assert!(!super::history::moved(k, b, &[(e, &at)], tol)?, "still where it lay");
        b.edge_mut(e)?.curve = None;
        assert!(edge_closest_point(k, b, e, at[2]).is_err_and(|e| e.sm_status().is_none()), "fails with no status");
        assert!(super::history::moved(k, b, &[(e, &at)], tol)?, "off the piece");
        Ok(())
    })
    .unwrap();
}

/// Face and edge tags (`Brep::tagged`) through operations routed to solidcore, as on C++: each
/// operand is tagged on the C++ kernel, its solidcore body converted from that and tagged the same
/// way there, and the two results' histories ([`Raw`], each in its own kernel's face and edge
/// order) must be the same.
#[cfg(feature = "native")]
mod history {
    use crate::history::Raw;
    use crate::{BooleanOp, Brep, ffi};

    fn tagged_box(origin: [f64; 3], size: [f64; 3], operand: usize) -> Brep {
        crate::init().unwrap();
        Brep::made(ffi::make_box(&origin, &size).unwrap()).tagged(operand).unwrap()
    }

    /// Two 10 mm cubes overlapping in a 5 mm one (the capi's `overlapping_boxes_union`): 12
    /// faces, each from one face of one box, six from each.
    fn check(h: &Raw) {
        assert_eq!(h.faces.len(), 12, "{h:?}");
        assert!(h.faces.iter().all(|f| f.origins.len() == 1), "a face without exactly one origin: {h:?}");
        for operand in [0, 1] {
            assert_eq!(h.faces.iter().filter(|f| f.origins[0].0 == operand).count(), 6, "operand {operand}: {h:?}");
        }
    }

    #[test]
    fn tags_cross_a_cpp_boolean() {
        let _cpp = crate::route::on_cpp();
        let (a, b) = (tagged_box([0.0; 3], [10.0; 3], 0), tagged_box([5.0; 3], [10.0; 3], 1));
        let r = Brep::made(ffi::ops::boolean(a.cpp().unwrap(), b.cpp().unwrap(), ffi::ops::BOOLEAN_UNION).unwrap());
        check(&r.history().unwrap());
    }

    #[test]
    fn tags_cross_a_solidcore_boolean() {
        let _cpp = crate::route::on_cpp();
        let (a, b) = (tagged_box([0.0; 3], [10.0; 3], 0), tagged_box([5.0; 3], [10.0; 3], 1));
        let r = super::boolean(a.core().unwrap(), b.core().unwrap(), BooleanOp::Union).unwrap().unwrap();
        check(&Brep::made_core(r).history().unwrap());
    }

    /// `a` op `b` on both kernels: the same history.
    fn parity(a: &Brep, b: &Brep, op: BooleanOp) {
        let code = match op {
            BooleanOp::Union => ffi::ops::BOOLEAN_UNION,
            BooleanOp::Subtract => ffi::ops::BOOLEAN_SUBTRACT,
            BooleanOp::Intersect => ffi::ops::BOOLEAN_INTERSECT,
        };
        let cpp = Brep::made(ffi::ops::boolean(a.cpp().unwrap(), b.cpp().unwrap(), code).unwrap()).history().unwrap();
        let core = Brep::made_core(super::boolean(a.core().unwrap(), b.core().unwrap(), op).unwrap().unwrap()).history().unwrap();
        assert_eq!(core.canonical(), cpp.canonical());
    }

    #[test]
    fn an_overlapping_union_has_the_cpp_history() {
        let _cpp = crate::route::on_cpp();
        parity(&tagged_box([0.0; 3], [10.0; 3], 0), &tagged_box([5.0; 3], [10.0; 3], 1), BooleanOp::Union);
    }

    /// A slot across the top of a 20 mm cube: the top is cut in two, the slot's walls and floor
    /// come from the tool.
    #[test]
    fn a_slot_has_the_cpp_history() {
        let _cpp = crate::route::on_cpp();
        let (a, b) = (tagged_box([0.0; 3], [20.0; 3], 0), tagged_box([5.0, -1.0, 10.0], [10.0, 22.0, 15.0], 1));
        parity(&a, &b, BooleanOp::Subtract);
    }

    /// Two boxes side by side, sharing the face x = 10 and their top and bottom planes: faces
    /// found lying on each other's (SMLib's coincidences and merges) stand for both.
    #[test]
    fn boxes_side_by_side_have_the_cpp_history() {
        let _cpp = crate::route::on_cpp();
        let (a, b) = (tagged_box([0.0; 3], [10.0; 3], 0), tagged_box([10.0, 0.0, 0.0], [10.0; 3], 1));
        parity(&a, &b, BooleanOp::Union);
    }

    /// A box sharing one corner region with another: the intersection keeps three faces of each.
    #[test]
    fn an_intersection_has_the_cpp_history() {
        let _cpp = crate::route::on_cpp();
        parity(&tagged_box([0.0; 3], [10.0; 3], 0), &tagged_box([5.0; 3], [10.0; 3], 1), BooleanOp::Intersect);
    }

    /// `SmApiBrepCopy` (the copy every solidcore operation starts from) keeps every tag: the
    /// copy's history is the operand's own, every face Kept, as on C++.
    #[test]
    fn tags_cross_a_copy() {
        let _cpp = crate::route::on_cpp();
        let a = tagged_box([0.0; 3], [10.0, 20.0, 30.0], 0);
        // Converted (and tagged) before the kernel's lock is taken: the conversion takes it too.
        let body = a.core().unwrap();
        let copy = super::run("copy", |k| {
            let g = body.0.lock().unwrap();
            super::super::boolean::copy_of(k, &g, 7)
        })
        .unwrap();
        let core = Brep::made_core(super::Body::new(copy)).history().unwrap();
        let cpp = Brep::made(ffi::copy(a.cpp().unwrap()).unwrap()).history().unwrap();
        assert_eq!(core.canonical(), cpp.canonical());
        assert!(core.faces.iter().all(|f| f.change == solidcraft_geom::brep::history::Change::Kept), "{core:?}");
    }

    /// A post standing on the top of a cube, imprinted on it: the top is split along the
    /// post's foot, the other faces are as they were.
    #[test]
    fn an_imprint_has_the_cpp_history() {
        let _cpp = crate::route::on_cpp();
        let (a, b) = (tagged_box([0.0; 3], [20.0; 3], 0), tagged_box([5.0, 5.0, 10.0], [10.0, 10.0, 20.0], 1));
        let cpp = Brep::made(ffi::ops::imprint(a.cpp().unwrap(), b.cpp().unwrap()).unwrap()).history().unwrap();
        let core = Brep::made_core(super::imprint(a.core().unwrap(), b.core().unwrap()).unwrap().unwrap()).history().unwrap();
        assert_eq!(core.canonical(), cpp.canonical());
    }

    /// A 10 mm wide bar laid across the top of a 40 x 30 x 20 block, overhanging its front and
    /// back, imprinted on it: the bar's sides cut the top in three, and cut the top edges of the
    /// faces y = 0 and y = 30 only by a vertex each (where they cross them at x = 10 and x = 20).
    /// Those faces are still all of themselves, their boundaries where they were: Kept, as are
    /// x = 0, x = 40 and the bottom (the fork's 1c7accd; before, y = 0 and y = 30 were Modified).
    #[test]
    fn a_face_whose_edge_is_only_cut_by_a_vertex_is_kept() {
        let _cpp = crate::route::on_cpp();
        use solidcraft_geom::brep::history::Change;
        let (a, b) = (tagged_box([0.0; 3], [40.0, 30.0, 20.0], 0), tagged_box([10.0, -5.0, 20.0], [10.0, 40.0, 10.0], 1));
        let cpp = Brep::made(ffi::ops::imprint(a.cpp().unwrap(), b.cpp().unwrap()).unwrap()).history().unwrap();
        let core = Brep::made_core(super::imprint(a.core().unwrap(), b.core().unwrap()).unwrap().unwrap()).history().unwrap();
        assert_eq!(core.canonical(), cpp.canonical());
        let of_a = |h: &Raw, c: Change| h.faces.iter().filter(|f| f.origins.iter().all(|o| o.0 == 0) && f.change == c).count();
        for h in [&core, &cpp] {
            assert_eq!(of_a(h, Change::Kept), 5, "{h:?}");
            assert_eq!(of_a(h, Change::Modified), 0, "{h:?}");
        }
    }

    /// A 10 mm post standing on a 20 mm cube's top against its x = 0 side, imprinted on it: the
    /// post's foot cuts the top's edge along x = 0 by two vertices. Each piece of that edge is a
    /// split child, standing for the edge and lying where it lay (the capi's Split gives an untagged
    /// child the split edge's `at`), so the side x = 0 is still all of itself and Kept.
    #[test]
    fn an_imprint_split_edge_lies_where_the_edge_lay() {
        let _cpp = crate::route::on_cpp();
        use solidcraft_geom::brep::history::Change;
        let (a, b) = (tagged_box([0.0; 3], [20.0; 3], 0), tagged_box([0.0, 5.0, 20.0], [10.0; 3], 1));
        let cpp = Brep::made(ffi::ops::imprint(a.cpp().unwrap(), b.cpp().unwrap()).unwrap()).history().unwrap();
        let core = Brep::made_core(super::imprint(a.core().unwrap(), b.core().unwrap()).unwrap().unwrap()).history().unwrap();
        assert_eq!(core.canonical(), cpp.canonical());
        let of_a = |h: &Raw, c: Change| h.faces.iter().filter(|f| f.origins.iter().all(|o| o.0 == 0) && f.change == c).count();
        for h in [&core, &cpp] {
            // The top's edge along x = 0 is in three pieces, each standing for it alone.
            let pieces = |e: usize| h.edges.iter().filter(|x| x.origins == [(0, e)]).count();
            assert!((0..12).any(|e| pieces(e) == 3), "{h:?}");
            assert_eq!(of_a(h, Change::Kept), 5, "{h:?}");
            assert_eq!(of_a(h, Change::Modified), 0, "{h:?}");
            assert_eq!(of_a(h, Change::Split), 2, "{h:?}");
        }
    }

    /// A 10 mm post standing on a 20 mm cube's top, subtracted from it: the post only touches the
    /// top, which the boolean copies the post's foot onto (`SmAttribute::Copy`). SMLib's Copy
    /// leaves a target that already has a tag alone, so the top stands for the cube's top only.
    #[test]
    fn a_copy_onto_a_tagged_face_keeps_its_own_tag() {
        let _cpp = crate::route::on_cpp();
        use solidcraft_geom::brep::history::Change;
        let (a, b) = (tagged_box([0.0; 3], [20.0; 3], 0), tagged_box([5.0, 5.0, 20.0], [10.0; 3], 1));
        parity(&a, &b, BooleanOp::Subtract);
        let core = Brep::made_core(super::boolean(a.core().unwrap(), b.core().unwrap(), BooleanOp::Subtract).unwrap().unwrap()).history().unwrap();
        assert_eq!(core.faces.len(), 6, "{core:?}");
        assert!(core.faces.iter().all(|f| f.change == Change::Kept && f.origins.len() == 1 && f.origins[0].0 == 0), "{core:?}");
    }

    /// Two 10 mm cubes side by side sharing the face x = 10, the second subtracted from the first:
    /// the second's edge on that face is copied onto the first's edge it lies on, which keeps its
    /// own tag (SMLib's Copy), as on C++.
    #[test]
    fn a_copy_onto_a_tagged_edge_has_the_cpp_history() {
        let _cpp = crate::route::on_cpp();
        parity(&tagged_box([0.0; 3], [10.0; 3], 0), &tagged_box([10.0, 0.0, 0.0], [10.0; 3], 1), BooleanOp::Subtract);
    }

    fn tagged_cylinder(origin: [f64; 3], radius: f64, height: f64, operand: usize) -> Brep {
        crate::init().unwrap();
        Brep::made(ffi::make_cylinder(&origin, radius, height).unwrap()).tagged(operand).unwrap()
    }

    /// A 40 x 40 x 10 plate with a round hole r 5 at (20, 20), made before tagging: its hole's rims
    /// are whole circles, edges closed on themselves.
    fn holed_plate(operand: usize) -> Brep {
        crate::init().unwrap();
        let plate = ffi::make_box(&[0.0; 3], &[40.0, 40.0, 10.0]).unwrap();
        let rod = ffi::make_cylinder(&[20.0, 20.0, -5.0], 5.0, 20.0).unwrap();
        Brep::made(ffi::ops::boolean(&plate, &rod, ffi::ops::BOOLEAN_SUBTRACT).unwrap()).tagged(operand).unwrap()
    }

    /// Faces of operand 0 with this change, in a history.
    fn of_first(h: &Raw, c: solidcraft_geom::brep::history::Change) -> usize {
        h.faces.iter().filter(|f| f.origins.iter().all(|o| o.0 == 0) && !f.origins.is_empty() && f.change == c).count()
    }

    /// A hole through a block: the tool's and the block's faces meet along whole circles.
    #[test]
    fn a_round_hole_has_the_cpp_history() {
        let _cpp = crate::route::on_cpp();
        parity(&tagged_box([0.0; 3], [40.0, 40.0, 10.0], 0), &tagged_cylinder([20.0, 20.0, -5.0], 5.0, 20.0, 1), BooleanOp::Subtract);
    }

    /// A corner cut far from the hole: the hole's wall keeps its two whole circles, each a closed
    /// edge still where it lay (`moved`'s `open.is_empty()`), so it is Kept on both kernels.
    #[test]
    fn a_face_bounded_by_closed_edges_is_kept() {
        let _cpp = crate::route::on_cpp();
        use solidcraft_geom::brep::history::Change;
        let (a, b) = (holed_plate(0), tagged_box([35.0, 35.0, -5.0], [10.0, 10.0, 20.0], 1));
        let cpp = Brep::made(ffi::ops::boolean(a.cpp().unwrap(), b.cpp().unwrap(), ffi::ops::BOOLEAN_SUBTRACT).unwrap()).history().unwrap();
        let core = Brep::made_core(super::boolean(a.core().unwrap(), b.core().unwrap(), BooleanOp::Subtract).unwrap().unwrap()).history().unwrap();
        assert_eq!(core.canonical(), cpp.canonical());
        // The hole's wall and the two sides the corner misses.
        assert_eq!(of_first(&core, Change::Kept), 3, "{core:?}");
    }

    /// A bar laid on the plate over the hole, imprinted: the bar's sides cross the hole's upper rim
    /// and cut that closed edge by vertices; the top is split, the wall's rim stays where it lay.
    #[test]
    fn an_imprint_cutting_a_closed_edge_has_the_cpp_history() {
        let _cpp = crate::route::on_cpp();
        let (a, b) = (holed_plate(0), tagged_box([-5.0, 18.0, 10.0], [50.0, 4.0, 5.0], 1));
        let cpp = Brep::made(ffi::ops::imprint(a.cpp().unwrap(), b.cpp().unwrap()).unwrap()).history().unwrap();
        let core = Brep::made_core(super::imprint(a.core().unwrap(), b.core().unwrap()).unwrap().unwrap()).history().unwrap();
        assert_eq!(core.canonical(), cpp.canonical());
    }

    /// A cylinder and a sphere cut by a slab across their seams (each kernel's angle 0, along +x):
    /// each curved face is split in two across its seam.
    #[test]
    fn a_curved_face_split_across_its_seam_has_the_cpp_history() {
        let _cpp = crate::route::on_cpp();
        use solidcraft_geom::brep::history::Change;
        let slab = || tagged_box([-20.0, -1.0, -20.0], [40.0, 2.0, 40.0], 1);
        let sphere = Brep::made(ffi::make_sphere(&[0.0, 0.0, 0.0], 7.0).unwrap()).tagged(0).unwrap();
        for (name, a) in [("cylinder", tagged_cylinder([0.0, 0.0, -5.0], 5.0, 10.0, 0)), ("sphere", sphere)] {
            parity(&a, &slab(), BooleanOp::Subtract);
            let cpp = Brep::made(ffi::ops::boolean(a.cpp().unwrap(), slab().cpp().unwrap(), ffi::ops::BOOLEAN_SUBTRACT).unwrap()).history().unwrap();
            assert!(of_first(&cpp, Change::Split) >= 2, "{name}: {cpp:?}");
        }
    }

    /// A chain of three overlapping boxes in one `union_all`.
    #[test]
    fn a_union_of_three_has_the_cpp_history() {
        let _cpp = crate::route::on_cpp();
        let (a, b, c) = (tagged_box([0.0; 3], [10.0; 3], 0), tagged_box([5.0; 3], [10.0; 3], 1), tagged_box([10.0; 3], [10.0; 3], 2));
        let cpp = Brep::made(ffi::ops::boolean_union_all(&[a.cpp().unwrap(), b.cpp().unwrap(), c.cpp().unwrap()]).unwrap()).history().unwrap();
        let core = Brep::made_core(super::union_all(&[a.core().unwrap(), b.core().unwrap(), c.core().unwrap()]).unwrap()).history().unwrap();
        assert_eq!(core.canonical(), cpp.canonical());
        assert_eq!(core.faces.iter().flat_map(|f| f.origins.iter().map(|o| o.0)).max(), Some(2), "{core:?}");
    }

    /// A row of four boxes sharing their top and bottom planes, united in pairs and the pairs
    /// united: each top merged with its neighbour's stands for two faces, and the merge of those
    /// stands for all four, both sources carrying several origins.
    #[test]
    fn a_merge_of_several_origins_on_each_side_has_the_cpp_history() {
        let _cpp = crate::route::on_cpp();
        let boxes: Vec<Brep> = (0..4u32).map(|i| tagged_box([f64::from(i) * 10.0, 0.0, 0.0], [10.0; 3], i as usize)).collect();
        let c = |x: usize, y: usize| ffi::ops::boolean(boxes[x].cpp().unwrap(), boxes[y].cpp().unwrap(), ffi::ops::BOOLEAN_UNION).unwrap();
        let (ab, cd) = (c(0, 1), c(2, 3));
        let cpp = Brep::made(ffi::ops::boolean(&ab, &cd, ffi::ops::BOOLEAN_UNION).unwrap()).history().unwrap();
        let s = |x: usize, y: usize| super::boolean(boxes[x].core().unwrap(), boxes[y].core().unwrap(), BooleanOp::Union).unwrap().unwrap();
        let (ab, cd) = (s(0, 1), s(2, 3));
        let core = Brep::made_core(super::boolean(&ab, &cd, BooleanOp::Union).unwrap().unwrap()).history().unwrap();
        assert_eq!(core.canonical(), cpp.canonical());
        assert!(core.faces.iter().any(|f| f.origins.len() == 4), "{core:?}");
    }

    /// Two overlapping squares combined as regions of one plane.
    #[test]
    #[ignore = "solidcore#1462 (PR #1977): boolean_2d carries no attributes through its merge"]
    fn a_boolean2d_has_the_cpp_history() {
        let _cpp = crate::route::on_cpp();
        use crate::sweeps::Curve;
        let square = |x: f64, y: f64, s: f64, operand: usize| {
            crate::init().unwrap();
            let c = [[x, y, 0.0], [x + s, y, 0.0], [x + s, y + s, 0.0], [x, y + s, 0.0]];
            let l: Vec<Curve> = (0..4).map(|i| Curve::Line { start: c[i], end: c[(i + 1) % 4] }).collect();
            crate::sweeps::ON_CPP.with(|f| f.set(true));
            let b = Brep::planar_faces(&l).unwrap();
            crate::sweeps::ON_CPP.with(|f| f.set(false));
            b.tagged(operand).unwrap()
        };
        let (a, b) = (square(0.0, 0.0, 10.0, 0), square(5.0, 5.0, 10.0, 1));
        for (op, code) in [
            (BooleanOp::Union, ffi::ops::BOOLEAN_UNION),
            (BooleanOp::Subtract, ffi::ops::BOOLEAN_SUBTRACT),
            (BooleanOp::Intersect, ffi::ops::BOOLEAN_INTERSECT),
        ] {
            let cpp = Brep::made(ffi::ops::boolean2d(a.cpp().unwrap(), b.cpp().unwrap(), code).unwrap()).history().unwrap();
            let core = Brep::made_core(super::boolean2d(a.core().unwrap(), b.core().unwrap(), op).unwrap().unwrap()).history().unwrap();
            assert_eq!(core.canonical(), cpp.canonical(), "{op:?}");
        }
    }
}

/// Every object of a copy's naming has a key of its own past index 1,000,000, where a key of
/// `type_code * 1_000_000 + index` made face 1,000,000 share one with loop 0 (solidcore refuses
/// such a naming), and domains stay apart at the largest kinds and indices.
#[test]
fn object_names_stay_distinct_past_a_million() {
    use solidcore::EntityRef;
    use solidcore::topology::{BrepId, EdgeId, EdgeuseId, FaceId, FaceuseId, LoopId, LoopuseId, RegionId, ShellId, VertexId, VertexuseId};

    use super::boolean::{domain, named};
    let at = |i: u32| {
        [
            EntityRef::Region(RegionId::from_index(i)),
            EntityRef::Shell(ShellId::from_index(i)),
            EntityRef::Faceuse(FaceuseId::from_index(i)),
            EntityRef::Face(FaceId::from_index(i)),
            EntityRef::Loopuse(LoopuseId::from_index(i)),
            EntityRef::Loop(LoopId::from_index(i)),
            EntityRef::Edgeuse(EdgeuseId::from_index(i)),
            EntityRef::Edge(EdgeId::from_index(i)),
            EntityRef::Vertexuse(VertexuseId::from_index(i)),
            EntityRef::Vertex(VertexId::from_index(i)),
        ]
    };
    let mut keys = std::collections::HashSet::new();
    let mut total = 0;
    for d in 0..3 {
        total += 1;
        keys.insert(named(EntityRef::Brep(BrepId::from_index(0)), domain(d)).key);
        for i in [0, 1, 1_000_000, 1_000_001, u32::MAX] {
            for e in at(i) {
                total += 1;
                keys.insert(named(e, domain(d)).key);
            }
        }
    }
    assert_eq!(keys.len(), total);
    assert_ne!(named(EntityRef::Face(FaceId::from_index(1_000_000)), 0).key, named(EntityRef::Loop(LoopId::from_index(0)), 0).key);
}

/// Rig case booleans/2928, reduced (fork c2e3d9d): a ring, r 3..5 about (0, 10), z 0..2, unioned
/// with a half annulus prism, r 4..5 about (e, 10), z 2..20, whose cap sits on the ring's top. Two
/// equal circles a little apart cross; they do not coincide over a tiny edge. The stack's volume
/// is 113 pi whatever e is.
/// This path passes with `simple_coincidence_crossing_is_point` off too, so the test holds the
/// result here and does not single out that flag (solidcore's own test does).
#[test]
fn a_ring_and_a_lip_with_walls_nearly_together() {
    use super::sweep::extrude;
    use crate::sweeps::Curve;
    use crate::sweeps::DraftCorner;
    const Z: [f64; 3] = [0.0, 0.0, 1.0];
    let up = |p: &[Curve], d| extrude(p, Z, d, 0.0, DraftCorner::default(), true).unwrap();
    let arc = |c: [f64; 3], radius, start, end| Curve::Arc { center: c, axis: Z, reference: [1.0, 0.0, 0.0], radius, start, end };
    let disc = |r| up(&[arc([0.0, 10.0, 0.0], r, 0.0, 2.0 * PI)], 2.0);
    let ring = boolean(&disc(5.0), &disc(3.0), BooleanOp::Subtract).unwrap().unwrap();
    for e in [0.0, 1e-4, 1e-3, 1e-2] {
        let c = [e, 10.0, 2.0];
        let lip = up(
            &[
                arc(c, 5.0, 0.0, PI),
                Curve::Line { start: [e - 5.0, 10.0, 2.0], end: [e - 4.0, 10.0, 2.0] },
                Curve::Arc { center: c, axis: [0.0, 0.0, -1.0], reference: [-1.0, 0.0, 0.0], radius: 4.0, start: 0.0, end: PI },
                Curve::Line { start: [e + 4.0, 10.0, 2.0], end: [e + 5.0, 10.0, 2.0] },
            ],
            18.0,
        );
        let u = boolean(&ring, &lip, BooleanOp::Union).unwrap().unwrap();
        let v = volume(&u, 1e-8).unwrap();
        let report = super::check(&u, crate::check::CheckLevel::All).unwrap();
        assert!(report.is_valid(), "e {e}: {report}");
        assert_eq!(solid_count(&u).unwrap(), 1, "e {e}");
        assert!(super::is_manifold_solid(&u).unwrap(), "e {e}");
        assert!((v - 113.0 * PI).abs() < 1e-3, "e {e}: {v}");
    }
}

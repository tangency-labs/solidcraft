//! Booleans on solidcore, against analytic volumes: every result exports to a record that validates
//! and counts the solids it should.
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
        // Fifteen 10 mm cubes in a row, each overlapping its neighbour by 5 mm (SolidCraft issue
        // #16552's join of fifteen tools): the level-by-level union or the one-by-one retry builds
        // them all into one body of 15000 - 14 * 125 mm3.
        "union_all_fifteen_chain" => plain(
            Op::UnionAll((0..15).map(|i| Shape::Box([f64::from(i) * 5.0, 0.0, 0.0], [10.0; 3])).collect()),
            Some(15.0 * 1000.0 - 14.0 * 125.0),
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

fn kinds(rec: &solidcore_record::brep::Brep) -> BTreeMap<String, usize> {
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
        solidcore_ops::record_check::validate(&rec).unwrap_or_else(|e| panic!("{name}: record: {e}"));
        Some(Measured { volume: v, counts: n, kinds: kinds(&rec), solids })
    })
}

macro_rules! cases {
    ($($m:ident $(: $ign:meta)?),* $(,)?) => {$(
        mod $m {
            #[test]
            $(#[$ign])?
            fn solidcore_matches_the_analytic_volume() {
                super::solidcore_case(stringify!($m));
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
    union_all_fifteen_chain,
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

/// Face and edge tags (`Brep::tagged`) through booleans: each operand tagged, the result's history
/// ([`Raw`]) read back.
mod history {
    use solidcore_record::brep::history::Change;

    use crate::history::Raw;
    use crate::{BooleanOp, Brep};

    fn tagged_box(origin: [f64; 3], size: [f64; 3], operand: usize) -> Brep {
        Brep::make_box(origin, size).unwrap().tagged(operand).unwrap()
    }

    fn tagged_cylinder(origin: [f64; 3], radius: f64, height: f64, operand: usize) -> Brep {
        Brep::make_cylinder(origin, radius, height).unwrap().tagged(operand).unwrap()
    }

    /// A 40 x 40 x 10 plate with a round hole r 5 at (20, 20), made before tagging: its hole's rims
    /// are whole circles, edges closed on themselves.
    fn holed_plate(operand: usize) -> Brep {
        let plate = Brep::make_box([0.0; 3], [40.0, 40.0, 10.0]).unwrap();
        let rod = Brep::make_cylinder([20.0, 20.0, -5.0], 5.0, 20.0).unwrap();
        plate.boolean(&rod, BooleanOp::Subtract).unwrap().unwrap().tagged(operand).unwrap()
    }

    fn boolean(a: &Brep, b: &Brep, op: BooleanOp) -> Raw {
        a.boolean(b, op).unwrap().unwrap().history().unwrap()
    }

    fn imprint(a: &Brep, b: &Brep) -> Raw {
        a.imprint(b).unwrap().unwrap().history().unwrap()
    }

    /// Faces of operand 0 with this change.
    fn of_first(h: &Raw, c: Change) -> usize {
        h.faces.iter().filter(|f| !f.origins.is_empty() && f.origins.iter().all(|o| o.0 == 0) && f.change == c).count()
    }

    /// Two 10 mm cubes overlapping in a 5 mm one (the capi's `overlapping_boxes_union`): 12
    /// faces, each from one face of one box, six from each.
    #[test]
    fn tags_cross_a_boolean() {
        let h = boolean(&tagged_box([0.0; 3], [10.0; 3], 0), &tagged_box([5.0; 3], [10.0; 3], 1), BooleanOp::Union);
        assert_eq!(h.faces.len(), 12, "{h:?}");
        assert!(h.faces.iter().all(|f| f.origins.len() == 1), "a face without exactly one origin: {h:?}");
        for operand in [0, 1] {
            assert_eq!(h.faces.iter().filter(|f| f.origins[0].0 == operand).count(), 6, "operand {operand}: {h:?}");
        }
    }

    /// `SmApiBrepCopy` (the copy every solidcore operation starts from) keeps every tag: the
    /// copy's history is the operand's own, every face Kept.
    #[test]
    fn tags_cross_a_copy() {
        let a = tagged_box([0.0; 3], [10.0, 20.0, 30.0], 0);
        let h = a.try_clone().unwrap().history().unwrap();
        assert_eq!(h.faces.len(), 6, "{h:?}");
        assert!(h.faces.iter().all(|f| f.change == Change::Kept), "{h:?}");
    }

    /// A 10 mm wide bar laid across the top of a 40 x 30 x 20 block, overhanging its front and
    /// back, imprinted on it: the bar's sides cut the top in three, and cut the top edges of the
    /// faces y = 0 and y = 30 only by a vertex each (where they cross them at x = 10 and x = 20).
    /// Those faces are still all of themselves, their boundaries where they were: Kept, as are
    /// x = 0, x = 40 and the bottom.
    #[test]
    fn a_face_whose_edge_is_only_cut_by_a_vertex_is_kept() {
        let h = imprint(&tagged_box([0.0; 3], [40.0, 30.0, 20.0], 0), &tagged_box([10.0, -5.0, 20.0], [10.0, 40.0, 10.0], 1));
        assert_eq!(of_first(&h, Change::Kept), 5, "{h:?}");
        assert_eq!(of_first(&h, Change::Modified), 0, "{h:?}");
    }

    /// A 10 mm post standing on a 20 mm cube's top against its x = 0 side, imprinted on it: the
    /// post's foot cuts the top's edge along x = 0 by two vertices. Each piece of that edge is a
    /// split child, standing for the edge and lying where it lay (the capi's Split gives an untagged
    /// child the split edge's `at`), so the side x = 0 is still all of itself and Kept.
    #[test]
    fn an_imprint_split_edge_lies_where_the_edge_lay() {
        let h = imprint(&tagged_box([0.0; 3], [20.0; 3], 0), &tagged_box([0.0, 5.0, 20.0], [10.0; 3], 1));
        // The top's edge along x = 0 is in three pieces, each standing for it alone.
        let pieces = |e: usize| h.edges.iter().filter(|x| x.origins == [(0, e)]).count();
        assert!((0..12).any(|e| pieces(e) == 3), "{h:?}");
        assert_eq!(of_first(&h, Change::Kept), 5, "{h:?}");
        assert_eq!(of_first(&h, Change::Modified), 0, "{h:?}");
        assert_eq!(of_first(&h, Change::Split), 2, "{h:?}");
    }

    /// A 10 mm post standing on a 20 mm cube's top, subtracted from it: the post only touches the
    /// top, which the boolean copies the post's foot onto (`SmAttribute::Copy`). SMLib's Copy
    /// leaves a target that already has a tag alone, so the top stands for the cube's top only.
    #[test]
    fn a_copy_onto_a_tagged_face_keeps_its_own_tag() {
        let h = boolean(&tagged_box([0.0; 3], [20.0; 3], 0), &tagged_box([5.0, 5.0, 20.0], [10.0; 3], 1), BooleanOp::Subtract);
        assert_eq!(h.faces.len(), 6, "{h:?}");
        assert!(h.faces.iter().all(|f| f.change == Change::Kept && f.origins.len() == 1 && f.origins[0].0 == 0), "{h:?}");
    }

    /// A corner cut far from the hole: the hole's wall keeps its two whole circles, each a closed
    /// edge still where it lay (`moved`'s `open.is_empty()`), so it is Kept, with the two sides the
    /// corner misses.
    #[test]
    fn a_face_bounded_by_closed_edges_is_kept() {
        let h = boolean(&holed_plate(0), &tagged_box([35.0, 35.0, -5.0], [10.0, 10.0, 20.0], 1), BooleanOp::Subtract);
        assert_eq!(of_first(&h, Change::Kept), 3, "{h:?}");
    }

    /// A cylinder and a sphere cut by a slab across their seams (angle 0, along +x): each curved
    /// face is split in two across its seam.
    #[test]
    fn a_curved_face_split_across_its_seam_is_split() {
        let slab = || tagged_box([-20.0, -1.0, -20.0], [40.0, 2.0, 40.0], 1);
        let sphere = Brep::make_sphere([0.0; 3], 7.0).unwrap().tagged(0).unwrap();
        for (name, a) in [("cylinder", tagged_cylinder([0.0, 0.0, -5.0], 5.0, 10.0, 0)), ("sphere", sphere)] {
            let h = boolean(&a, &slab(), BooleanOp::Subtract);
            assert!(of_first(&h, Change::Split) >= 2, "{name}: {h:?}");
        }
    }

    /// A chain of three overlapping boxes in one `union_all`: the result names all three.
    #[test]
    fn a_union_of_three_names_each() {
        let (a, b, c) = (tagged_box([0.0; 3], [10.0; 3], 0), tagged_box([5.0; 3], [10.0; 3], 1), tagged_box([10.0; 3], [10.0; 3], 2));
        let h = Brep::union_all(&[&a, &b, &c]).unwrap().history().unwrap();
        assert_eq!(h.faces.iter().flat_map(|f| f.origins.iter().map(|o| o.0)).max(), Some(2), "{h:?}");
    }

    /// A row of four boxes sharing their top and bottom planes, united in pairs and the pairs
    /// united: each top merged with its neighbour's stands for two faces, and the merge of those
    /// stands for all four.
    #[test]
    fn a_merge_of_several_origins_on_each_side_stands_for_all() {
        let boxes: Vec<Brep> = (0..4u32).map(|i| tagged_box([f64::from(i) * 10.0, 0.0, 0.0], [10.0; 3], i as usize)).collect();
        let u = |x: &Brep, y: &Brep| x.boolean(y, BooleanOp::Union).unwrap().unwrap();
        let h = u(&u(&boxes[0], &boxes[1]), &u(&boxes[2], &boxes[3])).history().unwrap();
        assert!(h.faces.iter().any(|f| f.origins.len() == 4), "{h:?}");
    }
}

/// A boolean stage fails with a status error whose cause chain holds the stage that failed. The
/// call is no answer only when that stage is a kernel status: a port failure or a cancellation
/// in the chain fails the call, as `solidcore-ops`' `session::answer` does. (Each assertion on a
/// chain fails when `answer` reads only the error on top.)
#[test]
fn a_status_error_with_a_port_failure_or_cancellation_as_cause_fails_the_call() {
    use solidcore::SolidError;
    use solidcore::error::{Failure, Reason};

    use super::answer;

    let wrap = |cause: SolidError| Failure::new("test::stage", Reason::Failed("a stage failed")).caused_by(cause).into_error();

    let status = wrap(SolidError::sm_err("test::inner"));
    assert!(status.sm_status().is_some());
    assert!(matches!(answer::<()>(Err(status)), Ok(None)));

    let unported = wrap(SolidError::unported("test::callee", "test.unit"));
    assert!(unported.sm_status().is_some());
    assert!(matches!(answer::<()>(Err(unported)), Err(e) if e.is_port_failure()));
    let deep = wrap(wrap(SolidError::unported("test::callee", "test.unit")));
    assert!(answer::<()>(Err(deep)).is_err());

    let cancelled = wrap(SolidError::cancelled("test::cancelled"));
    assert!(cancelled.sm_status().is_some() && !cancelled.is_port_failure());
    assert!(answer::<()>(Err(cancelled)).is_err());

    assert!(answer::<()>(Err(SolidError::invalid_input("test::input", "bad"))).is_err());
}

/// A Boolean failure that slice, sweep and the other users of the merge engine pass up reaches the
/// caller as the SMLib status it stands for, as the `Upstream` it replaced did, not as a message;
/// one whose cause chain holds unported code is not supported; another subsystem's failure keeps
/// its message.
#[test]
fn a_boolean_failure_from_another_operation_keeps_its_status() {
    use solidcore::SolidError;
    use solidcore::error::{Failure, Reason};

    use super::blend::status;

    let boolean = |cause: Option<SolidError>| {
        let f = Failure::new("boolean::merge::p3", Reason::Failed("a Boolean of the tree failed"));
        match cause {
            Some(c) => f.caused_by(c),
            None => f,
        }
        .into_error()
    };
    assert!(matches!(status(boolean(None)), Error::Status(1001)));
    assert!(matches!(status(boolean(Some(SolidError::sm_err("test::inner")))), Error::Status(1001)));
    assert!(matches!(status(boolean(Some(SolidError::unported("test::callee", "test.unit")))), Error::Unsupported));
    let offset = Failure::new("offset::offset_executive::p2", Reason::Failed("the offset could not be built")).into_error();
    assert!(matches!(status(offset), Error::Solidcore(_)));
    // an intersection or classification failure passes on as the status it replaced
    use solidcore::error::intersect::IntersectFailure;
    let intersect = |f| Failure::new("intersect::point_class", f).into_error();
    assert!(matches!(status(intersect(IntersectFailure::Failed("no point"))), Error::Status(1001)));
    assert!(matches!(status(intersect(IntersectFailure::Missing("the face's surface"))), Error::Status(1006)));
    // a primitive, extrusion, revolution, skin or taper failure passes on as the status it replaced
    use solidcore::error::primitive::PrimitiveFailure;
    let primitive = |f| Failure::new("primitive::primitive_creation::p3", f).into_error();
    assert!(matches!(status(primitive(PrimitiveFailure::Failed("no profiles to sweep"))), Error::Status(1001)));
    assert!(matches!(status(primitive(PrimitiveFailure::Missing("the target region"))), Error::Status(1006)));
    assert!(matches!(status(primitive(PrimitiveFailure::AxisInsideFace)), Error::Status(1025)));
    let helix = PrimitiveFailure::HelixDeviates { deviation: 0.5, tolerance: 0.25 };
    assert!(matches!(status(primitive(helix)), Error::Status(1021)));
    // a mass-property, healing or geometry-utility failure passes on as the status it replaced
    use solidcore::error::geom::GeomFailure;
    use solidcore::error::heal::HealFailure;
    use solidcore::error::mass::MassFailure;
    let mass = |f| Failure::new("mass::integrator", f).into_error();
    assert!(matches!(status(mass(MassFailure::StepControl)), Error::Status(1001)));
    assert!(matches!(status(mass(MassFailure::Missing("the edge's curve"))), Error::Status(1006)));
    let heal = |f| Failure::new("heal::stitch::p1", f).into_error();
    assert!(matches!(status(heal(HealFailure::Failed("the edges could not be glued"))), Error::Status(1001)));
    assert!(matches!(status(heal(HealFailure::NotYetHealable)), Error::Status(1040)));
    let geom = |f| Failure::new("geom::matrix", f).into_error();
    assert!(matches!(status(geom(GeomFailure::Failed("the matrix is singular"))), Error::Status(1001)));
    assert!(matches!(status(geom(GeomFailure::CoincidentPoints)), Error::Status(0)));
    assert!(matches!(status(geom(GeomFailure::ToleranceTooSmall { tolerance: 0.0 })), Error::Status(1007)));
}

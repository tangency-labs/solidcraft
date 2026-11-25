//! tangency-labs/solidcraft#17985 (WR-469): a whole body on which `MakeEdgeInFaceWithVU`'s
//! vertex step still widens a tolerance: `gen0072` of the golden unit
//! `primitive.api_trim_ops`, a box (3.64 x 3.50 x 13.23) with every edge rounded to r 0.567,
//! trimmed by an arc (r 2.66, 22.9 to 142.6 deg, at z 20.1) projected along about -Z
//! (`SmApiProjectAndTrim`, keep the side of the reference point).
//!
//! The projected arc's trim edge ends on the front face (y = 0.00847) at the fillet's
//! tangent line x = 3.0783 - 0.5668 = 2.5115, z 6.615, where the new edge's curve ends
//! 5.3683e-6 from its vertex. The vertex has the box's tolerance 1e-5, and
//! `RefreshTolerance(FALSE)` keeps it; `AssertValid` wants every edge end within half its
//! vertex's tolerance, 5e-6. Without the step (upstream, and the retired
//! `new_edge_vertex_tolerance_covers_gap` cleared) the body's level-2 check reports
//! "Edge/Vertex gap across Edgeuse start larger than tol (offset=5.3682556e-06, tolerance=
//! 5e-06)" and the vertex keeps 1e-5 (MEASURED). With it the vertex's tolerance is twice that
//! gap, 1.07365e-5, and the check is clean; no other vertex moves (MEASURED: the other 17
//! tolerances are the same bits with and without the step).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::circular_fillet;
use solidcore::curve::curve::Curve;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_arc;
use solidcore::primitive::api_primitives::create_box;
use solidcore::primitive::api_trim_ops::project_and_trim;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel};

/// The box's lowest corner and its sides, and the fillet radius (`gen0072`).
const ORIGIN: [f64; 3] = [
    -0.563_337_450_778_779_8,
    0.008_471_199_218_126_113,
    1.878_975_987_556_269_3,
];
const SIDES: [f64; 3] = [
    3.641_674_380_931_332,
    3.500_213_792_179_693_4,
    13.226_044_234_143_783,
];
const FILLET: f64 = 0.566_813_639_581_996_9;

/// The filleted box trimmed by the projected arc.
fn trimmed(k: &mut Kernel) -> Brep {
    let o = Point3::new(ORIGIN[0], ORIGIN[1], ORIGIN[2]);
    let mut b = create_box(k, o, SIDES[0], SIDES[1], SIDES[2]).unwrap();
    circular_fillet(k, &mut b, FILLET).unwrap();
    let before = check_brep(k, &mut b, CheckLevel::Level2).unwrap();
    assert!(
        before.lines().is_empty(),
        "the filleted box: {:?}",
        before.lines()
    );
    let arc = Curve::from(
        create_arc(
            Point3::new(
                0.644_892_519_944_074,
                -0.691_571_559_217_812_7,
                20.105_020_221_700_05,
            ),
            2.658_983_951_626_906_3,
            22.903_275_813_534_666,
            142.556_181_174_935_9,
        )
        .unwrap(),
    );
    project_and_trim(
        k,
        &mut b,
        &arc,
        Vec3::new(-0.032_163_053_713_179_83, -0.046_948_651_873_948_505, -1.0),
        Point3::new(
            1.257_499_739_686_886,
            0.183_481_888_827_110_8,
            15.105_020_221_700_052,
        ),
    )
    .unwrap();
    b
}

/// The trimmed body's vertices as `(x, y, z, tolerance)`, in vertex-list order (MEASURED).
/// The ninth is the step's vertex; every other tolerance is the same without the step.
const VERTICES: [(f64, f64, f64, f64); 18] = [
    (
        -0.5633374507787798,
        0.575284838800123,
        2.445789627138266,
        1e-5,
    ),
    (
        -0.5633374507787798,
        0.575284838800123,
        14.538206582118054,
        1e-5,
    ),
    (
        0.003476188803217101,
        0.008471199218125891,
        14.538206582118049,
        1.00000000053337e-5,
    ),
    (
        0.003476188803217095,
        0.008471199218126113,
        2.445789627138266,
        1e-5,
    ),
    (
        0.003476188803217095,
        0.575284838800123,
        1.8789759875562693,
        1e-5,
    ),
    (
        0.003476188803217095,
        0.575284838800123,
        15.105020221700052,
        1e-5,
    ),
    (
        2.511523290570555,
        0.575284838800123,
        15.105020221700052,
        1e-5,
    ),
    (
        2.511523290570553,
        0.008471199218126113,
        14.538206582118054,
        1.0000000002220447e-5,
    ),
    (
        2.511523290570555,
        0.008471199218126113,
        6.615215094454003,
        1.0736511129039172e-5,
    ),
    (
        2.252231146648179,
        0.008471199218126113,
        2.445789627138266,
        1e-5,
    ),
    (
        2.5115236463696955,
        0.7940497616484861,
        15.105020221699828,
        1.0466628367209873e-5,
    ),
    (
        0.003476188803217339,
        1.6888746596087147,
        15.105020221700052,
        1.043338974364815e-5,
    ),
    (
        -0.5633374507787798,
        1.4988033066218736,
        14.538206582118054,
        1e-5,
    ),
    (
        -0.5633374507787798,
        1.06010121371989,
        2.445789627138266,
        1e-5,
    ),
    (
        0.0034761765648626233,
        1.1111503340219147,
        1.8789759875562702,
        1.0589402889251424e-5,
    ),
    (
        1.660246495940927,
        0.575284838800123,
        1.8789759875562693,
        1e-5,
    ),
    (
        2.885567628187771,
        0.14941098950588833,
        14.538206582118054,
        1e-5,
    ),
    (
        2.6768920736259703,
        0.575284838800123,
        15.080360497170746,
        1e-5,
    ),
];

/// The index in [`VERTICES`] of the vertex the step widens.
const STEPPED: usize = 8;

/// Its tolerance without the step: the box's vertex tolerance, which `RefreshTolerance(FALSE)`
/// keeps (MEASURED with `new_edge_vertex_tolerance_covers_gap` cleared, before it retired).
const STEPPED_WITHOUT: f64 = 1e-5;

/// The level-2 line the body had without the step (MEASURED, as above).
const WITHOUT_LINE: &str = "SmBrep::Test #0 Bad Gap: Edge/Vertex gap across Edgeuse start \
    larger than tol (offset=5.3682556e-06, tolerance= 5e-06) [owner SmEdgeuse]";

/// The farthest any edge's curve ends from vertex `p`: per edge at `p`, the nearer of its
/// curve's two ends.
fn end_gap(k: &Kernel, b: &Brep, p: Point3) -> f64 {
    let mut worst: f64 = 0.0;
    for &e in b.edge_list() {
        let er = b.edge(e).unwrap();
        let at_p = er
            .vertices
            .iter()
            .any(|&v| b.vertex(v).unwrap().point.distance(p) == 0.0);
        if !at_p {
            continue;
        }
        let c = er.curve.as_ref().unwrap();
        let ends = [er.interval.min(), er.interval.max()]
            .map(|t| c.evaluate_point(k, t).unwrap().distance(p));
        worst = worst.max(ends[0].min(ends[1]));
    }
    worst
}

/// The trimmed body: its vertices and their tolerances (the step's vertex at twice its
/// gap, past the 1e-5 it had without the step), and a clean level-2 check where the
/// body without the step reported the gap.
#[test]
fn the_projected_arcs_end_vertex_covers_its_gap() {
    let mut k = Kernel::default();
    let mut b = trimmed(&mut k);
    assert_eq!(
        (
            b.face_list().len(),
            b.edge_list().len(),
            b.vertex_list().len()
        ),
        (14, 31, 18)
    );
    for (i, (&v, want)) in b.vertex_list().iter().zip(VERTICES).enumerate() {
        let got = b.vertex(v).unwrap();
        let p = Point3::new(want.0, want.1, want.2);
        assert!(
            got.point.distance(p) <= 1e-12,
            "vertex {i} at {:?}, want {p:?}",
            got.point
        );
        assert!(
            (got.tolerance - want.3).abs() <= 1e-12 * want.3,
            "vertex {i} tolerance {:e}, want {:e}",
            got.tolerance,
            want.3
        );
    }

    // the step's vertex: on the front face at the fillet's tangent line
    let (x, y, z, tolerance) = VERTICES[STEPPED];
    assert!((x - (ORIGIN[0] + SIDES[0] - FILLET)).abs() <= 1e-12);
    assert_eq!(y, ORIGIN[1]);
    let p = Point3::new(x, y, z);
    let gap = end_gap(&k, &b, p);
    // without the step the vertex keeps 1e-5, under twice the gap: AssertValid's half
    // tolerance (5e-6) is short of the gap
    assert!(gap > STEPPED_WITHOUT / 2.0, "gap {gap:e}");
    // with it the tolerance is twice the gap (the step's own value; it only grows one).
    // Allowance relative 1e-12: the gap is one distance, which rounding moves by a few ulp
    // (about 1e-16 relative), and doubling is exact; without the step the tolerance is
    // 1e-5, 7.4e-7 (7%) under twice the gap
    assert!(
        (tolerance - 2.0 * gap).abs() <= 1e-12 * tolerance,
        "gap {gap:e}"
    );

    let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert_eq!(check.assert_valid, Some(true));
    assert!(
        check.lines().is_empty(),
        "level 2: {:?} (without the step: {WITHOUT_LINE})",
        check.lines()
    );
}

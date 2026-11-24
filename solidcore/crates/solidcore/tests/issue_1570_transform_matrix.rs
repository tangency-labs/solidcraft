//! Issue #1570: `SmBrep::Transform(const double[4][4], bScaleTol)` (and the
//! `SmMatrix::SVDecompose` it relies on) applies an arbitrary affine map `p' = p·A + t`
//! (row-vector convention) to a B-rep: shear, non-uniform scale and reflection included.
//!
//! The issue's cases on the unit cube `[0,1]^3`: every vertex maps to `v·A + t`, the
//! volume becomes `|det A|` and the shell stays outward (positive volume) under a
//! reflection.
//!
//! #20939: a scale through the same entry point clears the edge uses' cached edge/face and
//! edge/edge gaps, which it makes wrong, so the check and the tolerance pass after it use the
//! scaled body's own gaps.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::matrix::Matrix;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_sphere};
use solidcore::topology::brep::mirror::transform_matrix;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p3::brep_copy;
use solidcore::topology::brep::validation::{
    check_brep, update_tolerances_after_boolean, CheckLevel,
};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{edge, vertex, vertexuse, EdgeuseId, VertexuseId};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

fn apply(m: &[[f64; 4]; 4], p: Point3) -> Point3 {
    Vec3::new(
        p.x * m[0][0] + p.y * m[1][0] + p.z * m[2][0] + m[3][0],
        p.x * m[0][1] + p.y * m[1][1] + p.z * m[2][1] + m[3][1],
        p.x * m[0][2] + p.y * m[1][2] + p.z * m[2][2] + m[3][2],
    )
}

/// Transforms the unit cube by `m` and checks every vertex and the volume.
fn check(m: [[f64; 4]; 4], expected_volume: f64) {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
    let vertices = p1::vertices(&b, None).unwrap();
    let before: Vec<Point3> = vertices
        .iter()
        .map(|&v| b.vertex(v).unwrap().point)
        .collect();
    transform_matrix(&mut k, &mut b, &m, true).unwrap();
    for (&v, &p) in vertices.iter().zip(&before) {
        let got = b.vertex(v).unwrap().point;
        let want = apply(&m, p);
        assert!(
            (got - want).length() < 1e-9,
            "vertex {p:?}: got {got:?}, want {want:?}"
        );
    }
    let vol = volume(&k, &mut b);
    assert!(
        (vol - expected_volume).abs() < 1e-6,
        "volume {vol}, want {expected_volume}"
    );
}

#[test]
fn issue_1570_shear_keeps_the_volume() {
    let m = [
        [1.0, 0.5, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [2.0, 3.0, 4.0, 1.0],
    ];
    check(m, 1.0);
    // the issue's spot check: (1,1,1) -> (3, 4.5, 5)
    assert_eq!(
        apply(&m, Point3::new(1.0, 1.0, 1.0)),
        Point3::new(3.0, 4.5, 5.0)
    );
}

#[test]
fn issue_1570_non_uniform_scale() {
    check(
        [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [1.0, -2.0, 0.5, 1.0],
        ],
        2.0,
    );
}

#[test]
fn issue_1570_reflection_keeps_the_shell_outward() {
    check(
        [
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        1.0,
    );
}

#[test]
fn issue_1570_sv_decompose_reconstructs_the_matrix() {
    let a = [1.0, 0.5, 0.0, 0.0, 1.0, 0.0, 0.3, -0.2, 2.0];
    let mut u = Matrix::from_row_major(3, 3, a.to_vec()).unwrap();
    let (w, v) = u.sv_decompose().unwrap();
    for r in 0..3 {
        for c in 0..3 {
            let mut x = 0.0;
            for (i, wi) in w.iter().enumerate() {
                x += u.at(r, i).unwrap() * wi * v.at(c, i).unwrap();
            }
            assert!((x - a[r * 3 + c]).abs() < 1e-12, "A[{r}][{c}] = {x}");
        }
    }
    assert!(w.iter().all(|&s| s >= 0.0));
}

// ---- #20939: the gap caches after a scale -------------------------------------------------

/// An R5 x 20 post crossed by an R3 bar off its axis by 1: the intersection edges are space
/// curves on neither cylinder, so they hold gaps of a few 1e-6 to the faces.
fn crossed_cylinders(k: &mut Kernel) -> Brep {
    let post = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 5.0, 20.0).unwrap();
    let mut bar = create_cylinder(k, Point3::new(0.0, 0.0, -15.0), 3.0, 30.0).unwrap();
    // the bar along x through (0, 1, 10)
    let turn_z_to_x = [
        [0.0, 0.0, -1.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 10.0, 1.0],
    ];
    transform_matrix(k, &mut bar, &turn_z_to_x, false).unwrap();
    boolean_union(k, post, bar).unwrap()
}

/// An R5 ball drilled by an R2 hole off its centre: space curves again.
fn ball_drilled_off_centre(k: &mut Kernel) -> Brep {
    let ball = create_sphere(k, Point3::new(0.0, 0.0, 0.0), 5.0).unwrap();
    let drill = create_cylinder(k, Point3::new(1.5, 0.7, -10.0), 2.0, 20.0).unwrap();
    boolean_difference(k, ball, drill).unwrap()
}

fn uniform_scale(s: f64) -> [[f64; 4]; 4] {
    [
        [s, 0.0, 0.0, 0.0],
        [0.0, s, 0.0, 0.0],
        [0.0, 0.0, s, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn edgeuse_ids(b: &Brep) -> Vec<EdgeuseId> {
    b.edgeuses().ids().collect()
}

fn vertexuse_ids(b: &Brep) -> Vec<VertexuseId> {
    b.vertexuses().ids().collect()
}

/// A Boolean's result as `solidcore-ops` leaves it: the tolerance pass (which caches every
/// edge use's edge/face gap), each edge/edge gap and each vertex use's vertex/edge and
/// vertex/face gap asked for once, then scaled by `s` with its tolerances (`Body::transform`). `cleared`: every gap cache of the scaled body is then
/// emptied, the twin whose reads measure everything afresh.
fn scaled_after_the_tolerance_pass(build: Build, s: f64, cleared: bool) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    let mut b = build(&mut k);
    update_tolerances_after_boolean(&mut k, &mut b).unwrap();
    for eu in edgeuse_ids(&b) {
        let _ = eu1::edge_ccw_edge_gap(&k, &mut b, eu, false);
    }
    for vu in vertexuse_ids(&b) {
        vertexuse::vertex_edge_gap(&k, &mut b, vu, false).unwrap();
        vertexuse::vertex_face_gap(&k, &mut b, vu, false).unwrap();
    }
    transform_matrix(&mut k, &mut b, &uniform_scale(s), true).unwrap();
    if cleared {
        for eu in edgeuse_ids(&b) {
            let r = b.edgeuse_mut(eu).unwrap();
            r.max_edge_face_gap.reset();
            r.edge_ccw_edge_gap.reset();
        }
        for vu in vertexuse_ids(&b) {
            let r = b.vertexuse_mut(vu).unwrap();
            r.vertex_edge_gap.reset();
            r.vertex_face_gap.reset();
        }
    }
    (k, b)
}

/// The same gap to rounding: the two ends of a mated pair measure it from either side, so the
/// cached value read through a mate may differ from its own recalculation in the last bits.
/// A gap kept from before the scale is out by the factor of the scale.
fn same_gap(a: Option<f64>, b: Option<f64>, s: f64) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() <= 1e-9 * a.abs().max(b.abs()) + 1e-12 * s,
        (a, b) => a.is_none() && b.is_none(),
    }
}

fn tolerances(b: &Brep) -> Vec<String> {
    let mut out = vec![format!("body {:?}", b.tolerance())];
    out.extend(
        b.edges()
            .iter()
            .map(|(id, e)| format!("{id:?} {:?}", e.tolerance)),
    );
    out.extend(
        b.vertices()
            .iter()
            .map(|(id, v)| format!("{id:?} {:?}", v.tolerance)),
    );
    out.extend(
        b.faces()
            .iter()
            .map(|(id, f)| format!("{id:?} {:?}", f.tolerance)),
    );
    out
}

fn check_report(k: &mut Kernel, b: &mut Brep) -> (bool, Vec<String>) {
    let c = check_brep(k, b, CheckLevel::Level2).unwrap();
    (c.is_valid(), c.lines())
}

/// `boolean::operand_tolerances`' tightening against an operand two decades finer: every
/// tolerance over `zone` set to it, then each edge and vertex grown back to its gaps as read.
fn tighten(k: &mut Kernel, b: &mut Brep, zone: f64) {
    let faces: Vec<_> = b.faces().iter().map(|(id, f)| (id, f.tolerance)).collect();
    for (f, t) in faces {
        if t > zone {
            solidcore::topology::face::p3::set_tolerance(b, f, zone, false, false).unwrap();
        }
    }
    let edges: Vec<_> = b.edges().iter().map(|(id, e)| (id, e.tolerance)).collect();
    for (e, t) in edges {
        if t > zone {
            edge::set_tolerance(b, e, zone, false, false).unwrap();
        }
    }
    let vertices: Vec<_> = b
        .vertices()
        .iter()
        .map(|(id, v)| (id, v.tolerance))
        .collect();
    for (v, t) in vertices {
        if t > zone {
            vertex::set_tolerance(b, v, zone, false, false).unwrap();
        }
    }
    if b.tolerance() > zone {
        b.set_tolerance(zone);
    }
    update_tolerances_after_boolean(k, b).unwrap();
}

/// Builds a test body.
type Build = fn(&mut Kernel) -> Brep;

const SCALED_BODIES: [(&str, Build); 2] = [
    ("crossed cylinders", crossed_cylinders),
    ("ball drilled off centre", ball_drilled_off_centre),
];

/// Every edge use's cached edge/face and edge/edge gap, and every vertex use's cached
/// vertex/edge and vertex/face gap, after the scale is the scaled body's own: before, both kept the distance measured before the scale (4.26e-6 on the crossed
/// cylinders where the scaled body has 4.26e-3 at x1000 and 4.26e-9 at x0.001).
#[test]
fn issue_20939_a_scaled_body_caches_its_own_edge_gaps() {
    for (name, build) in SCALED_BODIES {
        for s in [0.001, 1000.0] {
            let (k, mut b) = scaled_after_the_tolerance_pass(build, s, false);
            let uses = edgeuse_ids(&b);
            let face_gap = |b: &mut Brep, eu, force| {
                eu1::max_edge_face_gap(&k, b, eu, force)
                    .unwrap()
                    .map(|g| g.gap_3d)
            };
            let edge_gap = |b: &mut Brep, eu, force| {
                eu1::edge_ccw_edge_gap(&k, b, eu, force)
                    .unwrap()
                    .map(|g| g.gap_3d)
            };
            let cached: Vec<_> = uses
                .iter()
                .map(|&eu| (face_gap(&mut b, eu, false), edge_gap(&mut b, eu, false)))
                .collect();
            let mut face_gaps = 0;
            for (&eu, (face, edge)) in uses.iter().zip(cached) {
                let (forced_face, forced_edge) =
                    (face_gap(&mut b, eu, true), edge_gap(&mut b, eu, true));
                assert!(
                    same_gap(face, forced_face, s),
                    "{name} x{s}: {eu:?} caches edge/face gap {face:?}, recalculated {forced_face:?}"
                );
                assert!(
                    same_gap(edge, forced_edge, s),
                    "{name} x{s}: {eu:?} caches edge/edge gap {edge:?}, recalculated {forced_edge:?}"
                );
                face_gaps += usize::from(forced_face.is_some_and(|g| g > 1e-9 * s));
            }
            // the bodies have gaps worth scaling (else the test shows nothing)
            assert!(
                face_gaps > 0,
                "{name} x{s}: no edge/face gap over 1e-9 * scale"
            );
            // the vertex uses' gaps, reset when the scale moves each vertex
            let vus = vertexuse_ids(&b);
            let vertex_gaps = |b: &mut Brep, vu, force| {
                (
                    vertexuse::vertex_edge_gap(&k, b, vu, force)
                        .unwrap()
                        .map(|g| g.gap_3d),
                    vertexuse::vertex_face_gap(&k, b, vu, force)
                        .unwrap()
                        .map(|g| g.gap_3d),
                )
            };
            let cached: Vec<_> = vus
                .iter()
                .map(|&vu| vertex_gaps(&mut b, vu, false))
                .collect();
            for (&vu, (edge, face)) in vus.iter().zip(cached) {
                let (forced_edge, forced_face) = vertex_gaps(&mut b, vu, true);
                assert!(
                    same_gap(edge, forced_edge, s),
                    "{name} x{s}: {vu:?} caches vertex/edge gap {edge:?}, recalculated {forced_edge:?}"
                );
                assert!(
                    same_gap(face, forced_face, s),
                    "{name} x{s}: {vu:?} caches vertex/face gap {face:?}, recalculated {forced_face:?}"
                );
            }
        }
    }
}

/// The check, the tolerance pass and the tightening after a scale give what they give on the
/// same body with every gap cache emptied. Before, at x0.001 the check refused both bodies
/// with `Bad Gap` from the gap kept from before the scale, and the tolerance pass kept edge
/// and vertex tolerances raised to it; at x1000 the tightening set four edges under their
/// real gap and the check of a copy then refused them.
#[test]
fn issue_20939_checks_and_tolerances_after_a_scale_read_the_real_gaps() {
    for (name, build) in SCALED_BODIES {
        for s in [0.001, 1000.0] {
            let (mut k1, mut b1) = scaled_after_the_tolerance_pass(build, s, false);
            let (mut k2, mut b2) = scaled_after_the_tolerance_pass(build, s, true);
            let (c1, c2) = (
                check_report(&mut k1, &mut b1),
                check_report(&mut k2, &mut b2),
            );
            assert_eq!(c1, c2, "{name} x{s}: check of the scaled body");
            assert!(
                c1.0,
                "{name} x{s}: the scaled body checks valid: {:?}",
                c1.1
            );
            assert!(
                !c1.1.iter().any(|l| l.contains("Bad Gap")),
                "{name} x{s}: {:?}",
                c1.1
            );
            let u1 = format!(
                "{:?}",
                update_tolerances_after_boolean(&mut k1, &mut b1).unwrap()
            );
            let u2 = format!(
                "{:?}",
                update_tolerances_after_boolean(&mut k2, &mut b2).unwrap()
            );
            assert_eq!(u1, u2, "{name} x{s}: tolerance pass");
            assert_eq!(
                tolerances(&b1),
                tolerances(&b2),
                "{name} x{s}: tolerances after the pass"
            );
            let zone = 0.01 * b1.tolerance();
            tighten(&mut k1, &mut b1, zone);
            tighten(&mut k2, &mut b2, zone);
            assert_eq!(
                tolerances(&b1),
                tolerances(&b2),
                "{name} x{s}: tolerances after tightening"
            );
            let mut copy1 = brep_copy(&mut k1, &b1).unwrap();
            let mut copy2 = brep_copy(&mut k2, &b2).unwrap();
            assert_eq!(
                check_report(&mut k1, &mut copy1),
                check_report(&mut k2, &mut copy2),
                "{name} x{s}: check of a copy after tightening"
            );
        }
    }
}

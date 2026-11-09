//! tangency-labs/solidcore#1059 (WR-410) probe, report only. The SolidCraft fork's rig
//! case left an edge's start 1.2e-4 from its vertex: the vertex definition took the
//! position of the first crossing found, which was a grazing one, where a steeper
//! crossing fixed the point better. With `Kernel::fixes.boolean_vertex_by_best_crossing`
//! the crossing with the larger sine places it.
//!
//! Here: the box A = [0,10]^3 joined with a 4 x 4 x 20 bar B whose long edge pierces A's
//! top face at a shallow angle: B is tilted by `alpha` degrees about the y axis (its
//! edges run nearly along x) and turned by `beta` degrees about z, so the curves ending
//! at a pierce point cross B's edge at different angles. Each case runs with the flag
//! off and on; the PROBE lines report the worst gap between an edge's curve end and its
//! vertex (the fork's symptom), the counts and the volume.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> solidcore::Result<f64> {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs)
}

/// The largest gap between an edge's curve end and its vertex, and how many edge ends
/// have no live vertex. `topological` reads the ends through the edge uses
/// (`GetStartVertex`/`GetEndVertex`, `edge::start_end_vertices`); otherwise from the
/// `Edge::vertices` field, kept for comparison (the field is a co-ported shortcut that
/// the topology edits may not maintain).
fn worst_vertex_gap(k: &Kernel, b: &Brep, topological: bool) -> (f64, usize) {
    let mut worst = 0.0_f64;
    let mut dangling = 0;
    for e in p1::edges(b, None).unwrap() {
        let edge = b.edge(e).unwrap();
        let Some(curve) = edge.curve.as_ref() else {
            continue;
        };
        let ends = if topological {
            let (s, t) = solidcore::topology::edge::start_end_vertices(b, e).unwrap();
            [s, t]
        } else {
            [Some(edge.vertices[0]), Some(edge.vertices[1])]
        };
        for (v, t) in ends
            .into_iter()
            .zip([edge.interval.min(), edge.interval.max()])
        {
            let Some(Ok(vertex)) = v.map(|v| b.vertex(v)) else {
                dangling += 1;
                continue;
            };
            worst = worst.max(curve.evaluate_point(k, t).unwrap().distance(vertex.point));
        }
    }
    (worst, dangling)
}

/// B: a 20 x 4 x 4 bar along x with its middle at the origin, tilted by `alpha` about
/// y and turned by `beta` about z, then moved so its middle is at (5, 5, 8): its
/// top edges (z 10 at the middle) pierce A's top face at a shallow angle.
fn bar(k: &mut Kernel, alpha: f64, beta: f64) -> Brep {
    let mut b = create_box(k, Point3::new(-10.0, -2.0, -2.0), 20.0, 4.0, 4.0).unwrap();
    let o = Point3::new(0.0, 0.0, 0.0);
    api_rotate(
        k,
        Some(TransformObject::Brep(&mut b)),
        o,
        Vec3::new(0.0, 1.0, 0.0),
        alpha,
    )
    .unwrap();
    api_rotate(
        k,
        Some(TransformObject::Brep(&mut b)),
        o,
        Vec3::new(0.0, 0.0, 1.0),
        beta,
    )
    .unwrap();
    api_translate(
        k,
        Some(TransformObject::Brep(&mut b)),
        Vec3::new(5.0, 5.0, 8.0),
    )
    .unwrap();
    b
}

fn run(alpha: f64, beta: f64, fix: bool) {
    let mut k = Kernel::default();
    k.fixes.boolean_vertex_by_best_crossing = fix;
    let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let b = bar(&mut k, alpha, beta);
    let label = format!("alpha={alpha} beta={beta} fix={fix}");
    match boolean_union(&mut k, a, b) {
        Err(e) => eprintln!("PROBE issue_1059 {label}: Err({e:?})"),
        Ok(mut u) => {
            let faces = p1::faces(&u, None).unwrap().len();
            let edges = p1::edges(&u, None).unwrap().len();
            let vertices = p1::vertices(&u, None).unwrap().len();
            let manifold = is_manifold_solid(&u);
            let integrity = solidcore::topology::integrity::check(&u);
            let (gap, dangling) = worst_vertex_gap(&k, &u, true);
            let (field_gap, field_dangling) = worst_vertex_gap(&k, &u, false);
            let v = volume(&k, &mut u);
            eprintln!(
                "PROBE issue_1059 {label}: Ok; faces={faces} edges={edges} vertices={vertices} manifold={manifold:?} worst_vertex_gap={gap:e} dangling_vertex_refs={dangling} integrity={integrity:?} (Edge::vertices field: gap={field_gap:e} dangling={field_dangling}) volume={v:?}"
            );
        }
    }
}

#[test]
fn issue_1059_grazing_edge_crossing_probe() {
    for alpha in [-1.0, -0.1, -0.01] {
        for beta in [0.0, 30.0] {
            for fix in [false, true] {
                run(alpha, beta, fix);
            }
        }
    }
}

/// What the flag changes at beta = 30 (where the volume differs in its last digits):
/// every vertex of the flag-off union matched to the nearest of the flag-on union, the
/// vertices that moved, and the worst edge-end gap of each.
#[test]
fn issue_1059_what_the_flag_moves_probe() {
    for alpha in [-1.0, -0.1] {
        let mut out = Vec::new();
        for fix in [false, true] {
            let mut k = Kernel::default();
            k.fixes.boolean_vertex_by_best_crossing = fix;
            let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
            let b = bar(&mut k, alpha, 30.0);
            let u = boolean_union(&mut k, a, b).unwrap();
            solidcore::topology::integrity::check(&u).unwrap();
            let points: Vec<Point3> = p1::vertices(&u, None)
                .unwrap()
                .into_iter()
                .map(|v| u.vertex(v).unwrap().point)
                .collect();
            let (gap, dangling) = worst_vertex_gap(&k, &u, true);
            out.push((points, gap, dangling));
        }
        let (off, on) = (&out[0], &out[1]);
        eprintln!(
            "PROBE issue_1059 moves alpha={alpha}: vertices off={} on={}; worst gap off={:e} on={:e}; dangling off={} on={}",
            off.0.len(),
            on.0.len(),
            off.1,
            on.1,
            off.2,
            on.2
        );
        for p in &off.0 {
            let (d, q) =
                on.0.iter()
                    .map(|q| (p.distance(*q), *q))
                    .min_by(|x, y| x.0.total_cmp(&y.0))
                    .unwrap();
            if d > 0.0 {
                eprintln!("PROBE issue_1059 moves alpha={alpha}: vertex {p:?} -> {q:?} ({d:e})");
            }
        }
    }
}

/// A sweep for a geometry where the flag matters: shallower tilts and other turns, and
/// the bar also rolled about its own axis (so its two top edges pierce A's top face far
/// apart). Prints the worst edge-end gap off and on, and the largest vertex move.
#[test]
fn issue_1059_parameter_sweep_probe() {
    let roll_bar = |k: &mut Kernel, alpha: f64, beta: f64, roll: f64| {
        let mut b = create_box(k, Point3::new(-10.0, -2.0, -2.0), 20.0, 4.0, 4.0).unwrap();
        let o = Point3::new(0.0, 0.0, 0.0);
        let x = Vec3::new(1.0, 0.0, 0.0);
        let y = Vec3::new(0.0, 1.0, 0.0);
        let z = Vec3::new(0.0, 0.0, 1.0);
        api_rotate(k, Some(TransformObject::Brep(&mut b)), o, x, roll).unwrap();
        api_rotate(k, Some(TransformObject::Brep(&mut b)), o, y, alpha).unwrap();
        api_rotate(k, Some(TransformObject::Brep(&mut b)), o, z, beta).unwrap();
        api_translate(
            k,
            Some(TransformObject::Brep(&mut b)),
            Vec3::new(5.0, 5.0, 8.0),
        )
        .unwrap();
        b
    };
    for alpha in [-1.0, -0.1, -0.01, -0.001] {
        for beta in [10.0, 30.0, 60.0, 85.0] {
            for roll in [0.0, 30.0] {
                let mut res = Vec::new();
                for fix in [false, true] {
                    let mut k = Kernel::default();
                    k.fixes.boolean_vertex_by_best_crossing = fix;
                    let a =
                        create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
                    let b = roll_bar(&mut k, alpha, beta, roll);
                    res.push(boolean_union(&mut k, a, b).map(|u| {
                        let pts: Vec<Point3> = p1::vertices(&u, None)
                            .unwrap()
                            .into_iter()
                            .map(|v| u.vertex(v).unwrap().point)
                            .collect();
                        let (gap, _) = worst_vertex_gap(&k, &u, true);
                        (pts, gap)
                    }));
                }
                let label = format!("alpha={alpha} beta={beta} roll={roll}");
                match (&res[0], &res[1]) {
                    (Ok(off), Ok(on)) => {
                        let moved = off
                            .0
                            .iter()
                            .map(|p| on.0.iter().map(|q| p.distance(*q)).fold(f64::MAX, f64::min))
                            .fold(0.0_f64, f64::max);
                        eprintln!(
                            "PROBE issue_1059 sweep {label}: gap off={:e} on={:e} max_vertex_move={moved:e} vertices off={} on={}",
                            off.1,
                            on.1,
                            off.0.len(),
                            on.0.len()
                        );
                    }
                    (off, on) => eprintln!(
                        "PROBE issue_1059 sweep {label}: off={:?} on={:?}",
                        off.as_ref().map(|r| r.1),
                        on.as_ref().map(|r| r.1)
                    ),
                }
            }
        }
    }
}

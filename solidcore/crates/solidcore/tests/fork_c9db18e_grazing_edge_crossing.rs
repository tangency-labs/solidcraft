//! The fork's own regression test `booleans` `grazing_edge_crossing` (tangency-labs/smlib
//! c9db18e, WR-410, tangency-labs/solidcore#1059, rig crashes/4092), ported. A slab is
//! lofted (degree 2, capped) through three quadrilaterals whose B-spline wall leans
//! against a rod r 1.5 along x with its bottom at z 36. The slab and rod are united, then
//! a pad (x -8..8, z 36..40, 20 deep along y) whose bottom edges along y are rounded r 3
//! is joined, so its rounds and the rod touch at (+-5, 19.5, 36). Where the slab's edge
//! on the rod meets a round, the wall/round curve runs along that edge at 5 degrees and
//! the rod/round curve crosses it at 40. Placed by the first crossing, the vertex is fixed
//! along the edge only to its gap over sin 5 degrees, and the rod/round edge ends far from
//! it: SMLib's check fails test 61 ("Bad Edge"), with an edge end 9.3e-5 from its vertex
//! at a tolerance of 1e-5.
//!
//! The fork runs its imprint end fits too (WR-452/453/498, the port's
//! `imprint_trims_edge_to_proto_vertex` and `imprint_extends_edge_to_proto_vertex`).
//! With them and `boolean_vertex_by_best_crossing` the join is sound (the fork's
//! `sound()`: the level-2 check clean) and its volume lies between the ribbed slab's and
//! the sum of the two (the fork's bound). Without the best crossing, test 61 fails.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_union;
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, vertex};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// The largest distance of an edge's end from its vertex.
fn worst_vertex_gap(k: &Kernel, b: &Brep) -> f64 {
    let mut worst = 0.0_f64;
    for e in p1::edges(b, None).unwrap() {
        let Some(curve) = edge::curve(b, e).unwrap() else {
            continue;
        };
        let iv = edge::interval(b, e).unwrap();
        let ends = [
            (edge::start_vertex(b, e).unwrap(), iv.min()),
            (edge::end_vertex(b, e).unwrap(), iv.max()),
        ];
        for (v, t) in ends {
            let Some(v) = v else { continue };
            let p = curve.evaluate_point(k, t).unwrap();
            worst = worst.max(p.distance(vertex::point(b, v).unwrap()));
        }
    }
    worst
}

fn line(a: Point3, b: Point3) -> Curve {
    Curve::from(create_line_segment(a, b).unwrap())
}

/// The capi's arc: the circle r about `c` in the frame (`x0`, `y0`), from 0 to `degrees`.
fn arc(k: &Kernel, c: Point3, x0: Vec3, y0: Vec3, r: f64, degrees: f64) -> Curve {
    let frame = Axis2Placement::new(c, x0, y0);
    let interval = Extent1d::from_min_max(0.0, degrees);
    Curve::from(Ellipse::create_canonical_circle(k, &frame, r, Some(&interval), None).unwrap())
}

/// The join's level-2 check failures, its volume, the ribbed slab's and the pad's volumes,
/// and its worst edge-end gap, with `fixes`.
fn grazing_join(fixes: &UpstreamFixes) -> (usize, f64, f64, f64, f64) {
    let mut k = Kernel {
        fixes: *fixes,
        ..Default::default()
    };
    let p = Point3::new;
    let (x0, y0, z0) = (-5.068, 19.4517, 36.00078);
    let mut wall = Vec::new();
    for x in [-20.0, -5.0, 10.0] {
        // the wall about y = y0 + 0.1246 (x - x0) + 0.1351 (z - z0), bent across x
        let w = |z: f64| y0 + 0.1246 * (x - x0) + 0.1351 * (z - z0) - 0.01 * (x - x0) * (x - x0);
        wall.push(line(p(x, 0.0, 30.0), p(x, w(30.0), 30.0)));
        wall.push(line(p(x, w(30.0), 30.0), p(x, w(45.0), 45.0)));
        wall.push(line(p(x, w(45.0), 45.0), p(x, 0.0, 45.0)));
        wall.push(line(p(x, 0.0, 45.0), p(x, 0.0, 30.0)));
    }
    let refs: Vec<Option<&Curve>> = wall.iter().map(Some).collect();
    let slab = create_skin_primitive(&mut k, &[4, 4, 4], &refs, 2, true)
        .unwrap()
        .brep;
    let rod_circle = arc(&k, p(-15.0, 19.5, 37.5), Vec3::Y, Vec3::Z, 1.5, 360.0);
    let rod = create_linear_sweep(&mut k, &[Some(&rod_circle)], Vec3::X, 30.0, true).unwrap();
    let mut ribbed = boolean_union(&mut k, slab, rod).unwrap();
    let ribbed_volume = volume(&k, &mut ribbed);
    // the pad's outline in y = 10: its bottom corners rounded r 3 (arcs about -y)
    let outline = [
        line(p(-5.0, 10.0, 36.0), p(5.0, 10.0, 36.0)),
        arc(&k, p(5.0, 10.0, 39.0), -Vec3::Z, Vec3::X, 3.0, 90.0),
        line(p(8.0, 10.0, 39.0), p(8.0, 10.0, 40.0)),
        line(p(8.0, 10.0, 40.0), p(-8.0, 10.0, 40.0)),
        line(p(-8.0, 10.0, 40.0), p(-8.0, 10.0, 39.0)),
        arc(&k, p(-5.0, 10.0, 39.0), -Vec3::X, -Vec3::Z, 3.0, 90.0),
    ];
    let refs: Vec<Option<&Curve>> = outline.iter().map(Some).collect();
    let mut pad = create_linear_sweep(&mut k, &refs, Vec3::Y, 20.0, true).unwrap();
    let pad_volume = volume(&k, &mut pad);
    let mut join = boolean_union(&mut k, ribbed, pad).unwrap();
    let failures = check_brep(&mut k, &mut join, CheckLevel::Level2)
        .unwrap()
        .failure_count();
    let v = volume(&k, &mut join);
    (
        failures,
        v,
        ribbed_volume,
        pad_volume,
        worst_vertex_gap(&k, &join),
    )
}

/// The fork's imprint end fits and, as `best` says, WR-410's best crossing.
fn fixes(best: bool) -> UpstreamFixes {
    let mut f = UpstreamFixes::default();
    f.imprint_trims_edge_to_proto_vertex = true;
    f.imprint_extends_edge_to_proto_vertex = true;
    f.boolean_vertex_by_best_crossing = best;
    f
}

#[test]
fn fork_c9db18e_grazing_join_is_sound_with_the_best_crossing() {
    let (failures, v, ribbed, pad, gap) = grazing_join(&fixes(true));
    assert_eq!(failures, 0, "the check");
    assert!(
        v > ribbed && v < ribbed + pad,
        "{v} not in ({ribbed}, {})",
        ribbed + pad
    );
    assert!(gap < 1e-5, "an edge ends {gap} from its vertex");
}

/// Placed by the first crossing, an edge ends 9.3e-5 from its vertex: test 61 fails.
#[test]
fn fork_c9db18e_grazing_join_fails_the_check_by_the_first_crossing() {
    let (failures, _, _, _, gap) = grazing_join(&fixes(false));
    assert!(failures > 0);
    assert!(gap > 5e-5, "{gap}");
}

/// WR-453 (tangency-labs/smlib a8caccd, `imprint_extends_edge_to_proto_vertex`) on the same
/// join: with the best crossing and the interval trim (WR-452) but without the extension,
/// an intersection edge stops short of its vertex and the check fails; with it the join
/// is sound, every edge ending within the body tolerance of its vertex (the fork's thread
/// test's measure: no vertex needs a tolerance past the body's 1e-5 to hold a gap).
fn trims_and_best(extends: bool) -> UpstreamFixes {
    let mut f = fixes(true);
    f.imprint_extends_edge_to_proto_vertex = extends;
    f
}

#[test]
fn fork_a8caccd_grazing_join_edges_reach_their_vertices_with_the_extension() {
    let (failures, _, _, _, gap) = grazing_join(&trims_and_best(true));
    assert_eq!(failures, 0, "the check");
    assert!(gap < 1e-5, "an edge ends {gap} from its vertex");
}

#[test]
fn fork_a8caccd_grazing_join_fails_the_check_without_the_extension() {
    let (failures, _, _, _, gap) = grazing_join(&trims_and_best(false));
    assert!(failures > 0);
    assert!(gap > 5e-5, "{gap}");
}

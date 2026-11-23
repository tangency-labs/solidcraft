//! tangency-labs/solidcore#480: a union of a half-annulus lip onto a ring whose outer
//! cylinder walls are 1e-4 to 1e-3 apart (10 to 100 times `SM_ZONE_TOL_3D`). Upstream's
//! `SmApiBoolean` fails ("not a solid") at those offsets and is valid at 0 and 0.01. The
//! two r 5 cylinders cross at a grazing angle beside the coincident cap/top planes. The
//! Rust port gives a valid union at every offset (WR-362); this guards it.
//!
//! With the ring turned 90 degrees, its circles' seam vertex sits at (0, 15, 2), e / 2 from
//! where the two r 5 rims cross: for 4e-5 < e <= 2e-4 `SmProtoEdge::Resolve` splits the
//! short self-loop proto edge onto that one vertex and `SmMerge::MakeFaceuse` returns
//! `SM_ERR` (WR-214); solidcore drops the proto edge instead.
//! These are not difftest cases: the ring's own difference (two coaxial cylinders) already
//! reaches UB-175 in `~SmProtoTopologyManager`, so `boolean.api_brep` rejects the input.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::math::Vec3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_sphere};
use solidcore::topology::brep::p1::{edges, faces, vertices};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::proto_topology::p1::api_transform::{api_rotate, TransformObject};
use solidcore::{Brep, Kernel};

fn volume(k: &Kernel, b: &mut Brep) -> solidcore::Result<f64> {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs)
}

/// The base ring (r 3..5 about (0, 10), z 0..2), turned `turn_deg` about its axis, united
/// with the lip (r 4..5 about (e, 10), y >= 10, z 2..20).
fn ring_and_lip_body(k: &mut Kernel, e: f64, turn_deg: f64) -> solidcore::Result<Brep> {
    let outer = create_cylinder(k, Point3::new(0.0, 10.0, 0.0), 5.0, 2.0)?;
    let inner = create_cylinder(k, Point3::new(0.0, 10.0, -1.0), 3.0, 4.0)?;
    let mut ring = boolean_difference(k, outer, inner)?;
    if turn_deg != 0.0 {
        api_rotate(
            k,
            Some(TransformObject::Brep(&mut ring)),
            Point3::new(0.0, 10.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            turn_deg,
        )?;
    }

    let lip_outer = create_cylinder(k, Point3::new(e, 10.0, 2.0), 5.0, 18.0)?;
    let lip_inner = create_cylinder(k, Point3::new(e, 10.0, 1.0), 4.0, 20.0)?;
    let tube = boolean_difference(k, lip_outer, lip_inner)?;
    let half = create_box(k, Point3::new(e - 10.0, 0.0, 0.0), 20.0, 10.0, 30.0)?;
    let lip = boolean_difference(k, tube, half)?;

    boolean_union(k, ring, lip)
}

/// [`ring_and_lip_body`] unturned; its volume.
fn ring_and_lip(e: f64) -> solidcore::Result<f64> {
    let mut k = Kernel::default();
    let mut joined = ring_and_lip_body(&mut k, e, 0.0)?;
    volume(&k, &mut joined)
}

/// The offsets of the WR-214 window that the C++ build was checked at.
const SEAM_WINDOW: [f64; 4] = [5e-5, 7e-5, 1e-4, 1.5e-4];

/// base pi (25 - 9) 2 plus half of pi (25 - 16) 18: 355.000 (issue: 354.99997).
fn want() -> f64 {
    let pi = core::f64::consts::PI;
    pi * 16.0 * 2.0 + 0.5 * pi * 9.0 * 18.0
}

#[test]
fn issue_480_ring_lip_union_at_every_offset() {
    let mut report = Vec::new();
    for e in [0.0, 1e-4, 1e-3, 1e-2] {
        match ring_and_lip(e) {
            Ok(v) if (v - want()).abs() <= 1e-4 * want() => {}
            other => report.push(format!("e = {e}: {other:?}")),
        }
    }
    assert!(report.is_empty(), "want {}: {report:#?}", want());
}

/// WR-214: the ring turned so its seam vertex is beside the rims' crossing; every offset
/// of the window gives a manifold solid of the analytic volume (SMLib fails with `SM_ERR`).
#[test]
fn issue_480_ring_lip_union_with_the_seam_at_the_crossing() {
    let mut report = Vec::new();
    for e in SEAM_WINDOW {
        let mut k = Kernel::default();
        let got = ring_and_lip_body(&mut k, e, 90.0).and_then(|mut b| {
            let solid = is_manifold_solid(&b)?;
            Ok((solid, volume(&k, &mut b)?))
        });
        match got {
            Ok((true, v)) if (v - want()).abs() <= 1e-4 * want() => {}
            other => report.push(format!("e = {e}: {other:?}")),
        }
    }
    assert!(report.is_empty(), "want {}: {report:#?}", want());
}

/// The box of [`box_corner_inside_a_sphere_unites_validly`]: its lowest corner and its
/// sides.
const CORNER_BOX: [[f64; 3]; 2] = [
    [
        -0.000_708_347_839_923_086_1,
        -0.001_088_517_749_960_698,
        -0.000_242_246_253_575_162_7,
    ],
    [
        0.000_345_044_173_964_004_5,
        0.001_378_224_155_740_920_8,
        0.000_407_169_946_195_375_5,
    ],
];
/// The sphere of `boolean.api_brep` tess0033: its centre and radius.
const CORNER_SPHERE: ([f64; 3], f64) = (
    [
        -0.000_738_930_906_077_589_4,
        -0.001_426_578_205_177_313_5,
        -0.000_598_806_599_105_121_8,
    ],
    0.000_508_104_980_215_361_7,
);

/// The union of [`CORNER_BOX`] with [`CORNER_SPHERE`], the sphere moved along the line
/// from its centre to the box's corner so that the corner lies `depth` inside it
/// (`None`: tess0033's own centre, the corner 1.58e-5 inside): checked valid at level 2,
/// with `counts` faces, edges and vertices, and its volume within the bracket below.
///
/// The exact union is the box's volume plus the sphere's, less the corner's overlap with
/// the ball. The centre is below the corner in every coordinate, so the distance to it
/// grows along each box axis: a point of the overlap lies within the crossings'
/// distances `t` of the corner on every axis, and the overlap is inside the box
/// `tx * ty * tz`. The volume is held to `[box + sphere - tx * ty * tz, box + sphere]`,
/// which holds the exact union.
fn corner_union_report(depth: Option<f64>, counts: [usize; 3]) -> Vec<String> {
    let [origin, size] = CORNER_BOX;
    let (mut centre, radius) = CORNER_SPHERE;
    if let Some(depth) = depth {
        let d: Vec<f64> = (0..3).map(|i| origin[i] - centre[i]).collect();
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        for i in 0..3 {
            centre[i] = origin[i] - d[i] / len * (radius - depth);
        }
    }

    // closed forms: the corner's offsets from the centre, the crossings along each
    // axis, and the depth of the corner inside the sphere
    let d: Vec<f64> = (0..3).map(|i| origin[i] - centre[i]).collect();
    assert!(
        d.iter().all(|&x| x > 0.0),
        "the centre is below the corner on every axis"
    );
    let crossing = |i: usize| {
        let rest: f64 = (0..3).filter(|&j| j != i).map(|j| d[j] * d[j]).sum();
        (radius * radius - rest).sqrt() - d[i]
    };
    let t = [crossing(0), crossing(1), crossing(2)];
    assert!(t.iter().all(|&x| x > 0.0 && x < 2e-4), "{t:?}");
    let got_depth = radius - (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if let Some(depth) = depth {
        assert!((got_depth - depth).abs() < 1e-12, "depth {got_depth}");
    }

    let pi = core::f64::consts::PI;
    let sum = size[0] * size[1] * size[2] + 4.0 / 3.0 * pi * radius.powi(3);
    let overlap_bound = t[0] * t[1] * t[2];

    let mut k = Kernel::default();
    let a = create_box(
        &mut k,
        Point3::new(origin[0], origin[1], origin[2]),
        size[0],
        size[1],
        size[2],
    )
    .unwrap();
    let s = create_sphere(&mut k, Point3::new(centre[0], centre[1], centre[2]), radius).unwrap();
    let mut r = boolean_union(&mut k, a, s).unwrap();

    let mut report = Vec::new();
    let checked = check_brep(&mut k, &mut r, CheckLevel::Level2).unwrap();
    if !checked.lines().is_empty() {
        report.push(format!("depth {got_depth}: {:#?}", checked.lines()));
    }
    let got_counts = [
        faces(&r, None).unwrap().len(),
        edges(&r, None).unwrap().len(),
        vertices(&r, None).unwrap().len(),
    ];
    if got_counts != counts {
        report.push(format!(
            "depth {got_depth}: faces, edges, vertices {got_counts:?}, want {counts:?}"
        ));
    }

    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let accuracy = 1e-9;
    let v = brep_volume(&k, &mut r, accuracy, &attrs).unwrap();
    let slack = accuracy * sum;
    if !(v >= sum - overlap_bound - slack && v <= sum + slack) {
        report.push(format!(
            "depth {got_depth}: volume {v}, box + sphere {sum}, overlap at most {overlap_bound}"
        ));
    }
    report
}

/// WR-214 at a box corner (`boolean.api_brep` tess0033): a 3.45e-4 x 1.378e-3 x 4.07e-4
/// box united with a sphere of radius 5.08e-4 whose centre lies off the box's lowest
/// corner in every axis, the corner 1.58e-5 inside the sphere (less than the box's and
/// the sphere's tolerances together, 2e-5). The three crossings of the corner's edges
/// (9.9e-5, 2.3e-5 and 2.2e-5 from it) combine into the corner's proto vertex, so the
/// sphere's cut is a proto edge from that proto vertex back to itself. SMLib splits it
/// onto the corner vertex and returns a body whose loops gap by about 1e-4 (check test
/// #0); solidcore drops it, and the union is the box and the sphere meeting at the
/// corner, a valid body whose volume is the box's plus the sphere's: seven faces (the
/// box's six and the sphere), thirteen edges (the box's twelve and the seam) and ten
/// vertices (the box's eight and the sphere's poles).
///
/// With the corner 2.1e-5 inside, more than the two tolerances, the sphere cuts it off:
/// the corner's three faces each lose it to an arc of the sphere, the three arcs meeting
/// the corner's three edges at their crossings, and the sphere's seam, a meridian from
/// pole to pole, misses the cut. That is seven faces, sixteen edges (the box's twelve,
/// the three arcs and the seam) and twelve vertices (the box's seven left, the three
/// crossings and the poles); the sphere's face has two loops, the seam's and the arcs',
/// and the counts meet Euler's `V - E + F - (L - F) = 12 - 16 + 7 - 1 = 2` for one
/// shell. The arcs are split off the sphere's crossing circles away from their middles,
/// so their end weights differ, and their NURB parameters must come from the polar angle
/// exactly to pass the check's conversion test (Ellipse test 1).
#[test]
fn box_corner_inside_a_sphere_unites_validly() {
    let mut report = corner_union_report(None, [7, 13, 10]);
    report.extend(corner_union_report(Some(2.1e-5), [7, 16, 12]));
    assert!(report.is_empty(), "{report:#?}");
}

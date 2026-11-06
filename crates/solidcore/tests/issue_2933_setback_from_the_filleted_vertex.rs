//! #2933 (WR-445): a corner setback is measured from the set-back vertex on every edge, with
//! the default kernel. A box 60 x 40 x 20 with r5 rounds on the three edges at (60, 40, 20)
//! and that corner set back 8, as the goldens' `real-box-high-corner-setback-eight` builds it.
//! Each round ends on its section 8 from the vertex, so the six section ends lie on the
//! setback planes x = 52, y = 32 and z = 12, and the body is convex: its volume lies in the
//! bounds the kernel case `ladder/07-fillets/013-corner-setback` derives in closed form.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::mass::integrator::properties::brep_volume_default;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::{brep_bounding_box, brep_is_manifold_solid};
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::{edge, vertex, Brep};
use solidcore::Kernel;

const CORNER: [f64; 3] = [60.0, 40.0, 20.0];

/// The box with its far corner set back 8, built with the default kernel.
fn build() -> (Kernel, Brep) {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
    let corner = Point3::new(CORNER[0], CORNER[1], CORNER[2]);
    let v = b
        .vertices()
        .ids()
        .find(|&v| vertex::point(&b, v).unwrap() == corner)
        .unwrap();
    let edges: Vec<_> = b
        .edges()
        .ids()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(&b, e).unwrap();
            s == Some(v) || t == Some(v)
        })
        .collect();
    assert_eq!(edges.len(), 3);
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_constant_radius_edges(&mut k, &edges, 5.0, 1, 1, 1.0)
            .unwrap();
        // set back (`AddSetBackVertex`), so the corner is the N x N corner, not the Ball
        s.add_set_back_vertex(v);
        s.create_corners(&k, None).unwrap();
        let corners: Vec<_> = s
            .fillet_corners()
            .unwrap()
            .into_iter()
            .filter(|&id| s.corner(id).unwrap().filleted_vertex() == Some(v))
            .collect();
        assert_eq!(corners.len(), 1);
        s.set_corner_setback(corners[0], 8.0).unwrap();
        s.do_filleting(&mut k, None).unwrap();
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    (k, b)
}

#[test]
fn each_round_ends_eight_from_the_vertex() {
    let (_, b) = build();
    // the rounds' sections at the setback planes: x = 52 (the x edge), y = 32, z = 12
    let ends = [
        [52.0, 35.0, 20.0],
        [52.0, 40.0, 15.0],
        [55.0, 32.0, 20.0],
        [60.0, 32.0, 15.0],
        [55.0, 40.0, 12.0],
        [60.0, 35.0, 12.0],
    ];
    let points: Vec<Point3> = b
        .vertices()
        .ids()
        .map(|v| vertex::point(&b, v).unwrap())
        .collect();
    for end in ends {
        let want = Point3::new(end[0], end[1], end[2]);
        let nearest = points
            .iter()
            .map(|p| p.distance(want))
            .fold(f64::INFINITY, f64::min);
        assert!(nearest < 1e-6, "no vertex at {end:?}: nearest {nearest}");
    }
}

#[test]
fn the_set_back_box_is_convex_and_inside_its_bounds() {
    let (k, mut b) = build();
    assert!(brep_is_manifold_solid(&b).unwrap());
    let x = brep_bounding_box(&k, &b, true).unwrap();
    for (i, limit) in CORNER.iter().enumerate() {
        assert!(x.max_at(i) <= limit + 1e-6, "{i}: {}", x.max_at(i));
    }
    // 013-corner-setback's closed-form bounds: the hull of the sections in the corner cube
    // below, the three rounds' prisms above
    let volume = brep_volume_default(&k, &mut b, 1e-6).unwrap();
    assert!(
        (47_271.975_7..=47_384.893_5).contains(&volume),
        "volume {volume}"
    );
}

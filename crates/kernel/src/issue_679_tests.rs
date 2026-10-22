//! Bodies touching back to back over part of a face fuse into one solid: two L's of a 2 × 2
//! grid extruded up and down (#679), a pad half past the edge of the face it stands on (#674),
//! several contact regions, contact beside a shared edge, and random rectangles.

use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};

use crate::*;

fn prism(pts: &[[f64; 2]], z0: f64, z1: f64) -> Body {
    let outer = Loop2::polygon(&pts.iter().map(|p| Vec2::new(p[0], p[1])).collect::<Vec<_>>());
    extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], z0, z1).unwrap().pop().unwrap()
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64, z0: f64, z1: f64) -> Body {
    box_solid(Vec3::new(x0, y0, z0), Vec3::new(x1, y1, z1)).unwrap()
}

fn fused(a: &Body, b: &Body) -> Body {
    boolean(a, b, BoolOp::Union).unwrap().unwrap()
}

fn assert_one_solid(r: &Body, volume: f64, tol: f64, what: &str) {
    let m = measure(r).unwrap();
    assert!((m.volume - volume).abs() <= tol, "{what}: volume {} vs {volume}", m.volume);
    assert_eq!(r.shell_count(), 1, "{what}: one solid");
    assert!(r.validity().is_empty(), "{what}: {:?}", r.validity());
}

/// #679: XO/XX extruded up 10, XX/OX down 10, 10 mm cells. They meet back to back over two
/// whole cells, along an edge and at the centre point: 6 cells × 10 mm × 100 mm².
#[test]
fn two_ls_up_and_down_fuse_into_one_solid() {
    let up = prism(&[[-10., -10.], [10., -10.], [10., 0.], [0., 0.], [0., 10.], [-10., 10.]], 0., 10.);
    let down = prism(&[[-10., 10.], [-10., 0.], [0., 0.], [0., -10.], [10., -10.], [10., 10.]], -10., 0.);
    for (a, b) in [(&up, &down), (&down, &up)] {
        let r = fused(a, b);
        assert_one_solid(&r, 6000.0, 1e-6, "#679");
        // Two tops, two bottoms, the two cells left open at z = 0, six sides.
        assert_eq!(r.face_count(), 12);
    }
}

/// Two blocks up from z = 0 and one plate down under both: two separate contacts fuse into one
/// solid. A plate under a block, flush with its sides along shared edges.
#[test]
fn several_contact_regions_and_edge_contact() {
    let up = rect(-10., -10., -2., -2., 0., 10.);
    let up2 = rect(2., 2., 10., 10., 0., 10.);
    let ups = boolean(&up, &up2, BoolOp::Union).unwrap().unwrap();
    // Down under both up cells, and under one more.
    let down = prism(&[[-10., -10.], [0., -10.], [0., 0.], [10., 0.], [10., 10.], [-10., 10.]], -5., 0.);
    let r = fused(&ups, &down);
    assert_one_solid(&r, 2.0 * 640.0 + 3.0 * 500.0, 1e-6, "two contacts");
    // Down under one up cell and beside the other: one face contact and one edge contact.
    // A plate under one block, flush with two of its sides and running on past the third.
    let down = rect(-10., -10., -2., 5., -5., 0.);
    let r = fused(&up, &down);
    assert_one_solid(&r, 640.0 + 8.0 * 15.0 * 5.0, 1e-6, "contact and flush sides");
}

/// #674 (robustness seed 16): a short cylinder whose end lies on a block's side with half of it
/// past the block's bottom edge. The end is split along the edge, the block's side along the
/// circle, and the half disc they share goes.
#[test]
fn a_pad_half_past_the_edge_of_its_face_fuses() {
    let block = rect(0., 0., 25., 47.5, 0., 37.5);
    let pad = cylinder(Vec3::new(7.5, 0., 0.), -Vec3::Y, 5.0, 2.5).unwrap();
    // Measured as the result is (the round side's volume from its mesh).
    let want = measure(&block).unwrap().volume + measure(&pad).unwrap().volume;
    assert!((want - (25.0 * 47.5 * 37.5 + std::f64::consts::PI * 25.0 * 2.5)).abs() < 0.1);
    for (a, b) in [(&block, &pad), (&pad, &block)] {
        let r = fused(a, b);
        assert_one_solid(&r, want, 1e-6 * want, "#674");
        let bounds = r.mesh(&crate::MeshTolerance::chord(0.01)).unwrap().bounds();
        assert!((bounds.min.y + 2.5).abs() < 1e-9 && (bounds.min.z + 5.0).abs() < 0.01, "{bounds:?}");
    }
}

/// Random rectangles extruded up from z = 0 and down from it, overlapping anywhere (inside one
/// another, crossing, sharing a side): one solid of exactly the two prisms' volume.
#[test]
fn random_rectangles_either_side_of_a_plane_fuse_exactly() {
    let mut seed = 0x679_u64;
    let mut next = |lo: f64, hi: f64| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        lo + (hi - lo) * ((seed >> 11) as f64 / (1u64 << 53) as f64)
    };
    let q = |x: f64| (x * 2.0).round() / 2.0;
    let mut done = 0;
    for case in 0..30 {
        let (x0, y0) = (q(next(0., 20.)), q(next(0., 20.)));
        let (w0, h0) = (q(next(2., 20.)), q(next(2., 20.)));
        // The second overlaps the first over an area.
        let (x1, y1) = (q(next(x0 - 15., x0 + w0 - 1.)), q(next(y0 - 15., y0 + h0 - 1.)));
        let (w1, h1) = (q(next(1., 25.)).max(x0 + 1. - x1).max(1.), q(next(1., 25.)).max(y0 + 1. - y1).max(1.));
        let (t0, t1) = (q(next(1., 10.)), q(next(1., 10.)));
        let up = rect(x0, y0, x0 + w0, y0 + h0, 0., t0);
        let down = rect(x1, y1, x1 + w1, y1 + h1, -t1, 0.);
        let want = w0 * h0 * t0 + w1 * h1 * t1;
        let what = format!("case {case}: up {x0},{y0} {w0}x{h0}x{t0}, down {x1},{y1} {w1}x{h1}x{t1}");
        let r = fused(&up, &down);
        assert_one_solid(&r, want, 1e-6 * want, &what);
        done += 1;
    }
    assert_eq!(done, 30);
}

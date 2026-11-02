//! #3573: shelling a tapered box with rounded corners left the inner rounds' edges up to 0.02 mm
//! off their faces. The offset rounds are cylinders tilted by the taper; their corners stopped
//! short of the moved faces and their curved ends were fitted through unevenly spaced points
//! that slid away from the moved corners, so other STEP readers could not mesh them.

use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};
use truck_modeling::{BoundedCurve, ParametricCurve, ParametricSurface, SearchNearestParameter, Surface};

use crate::Body;
use crate::body::{from_p3, p3};

/// Per face: whether it is a rational (round) face and the largest distance from its edges to it.
fn edge_gaps(b: &Body) -> Vec<(bool, f64)> {
    b.solid
        .face_iter()
        .map(|f| {
            let s = f.surface();
            let mut worst: f64 = 0.0;
            for e in f.edge_iter() {
                let c = e.curve();
                let (t0, t1) = c.range_tuple();
                for k in 0..=20 {
                    let q = from_p3(c.subs(t0 + (t1 - t0) * k as f64 / 20.0));
                    let d = s.search_nearest_parameter(p3(q), None, 200).map_or(f64::INFINITY, |(u, v)| from_p3(s.subs(u, v)).dist(q));
                    worst = worst.max(d);
                }
            }
            (matches!(s, Surface::NurbsSurface(_)), worst)
        })
        .collect()
}

/// A 120 x 80 box, 40 high, its walls drafted out 5°, the four sloping corners rounded at 4 mm.
fn rounded_tapered_box() -> Body {
    let rect = Region2 {
        outer: Loop2::polygon(&[Vec2::new(-60.0, -40.0), Vec2::new(60.0, -40.0), Vec2::new(60.0, 40.0), Vec2::new(-60.0, 40.0)]),
        holes: vec![],
    };
    let taper = 5f64.to_radians();
    let b = crate::extrude_tapered(&Plane::XY, &rect, 40.0, 1.0, taper).unwrap();
    let grow = 20.0 * taper.tan();
    let corners: Vec<Vec3> =
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].iter().map(|(sx, sy)| Vec3::new(sx * (60.0 + grow), sy * (40.0 + grow), 20.0)).collect();
    crate::fillet(&b, &corners, 4.0).unwrap()
}

#[test]
fn offset_tilted_rounds_keep_their_edges_on_them() {
    let b = rounded_tapered_box();
    assert!(edge_gaps(&b).iter().all(|(_, g)| *g < 1e-7), "the rounded box itself is exact: {:?}", edge_gaps(&b));
    // Every face in (a closed cavity), and the top pushed out as the shell's opening does.
    let closed = crate::offset::offset_body(&b, |_, _| -2.0).unwrap();
    let opened = crate::offset::offset_body(&b, |_, n| if n.z > 0.99 { 8.0 } else { -2.0 }).unwrap();
    for (name, o) in [("closed", &closed), ("opened", &opened)] {
        let gaps = edge_gaps(o);
        let worst = gaps.iter().map(|g| g.1).fold(0.0, f64::max);
        // Was 1.6e-3 (closed) and 2.1e-2 (opened).
        assert!(worst < 1e-5, "{name}: an edge {worst:.2e} mm off its face: {gaps:?}");
    }
}

#[test]
fn shelled_tapered_box_rounds_keep_their_edges_on_them() {
    let b = rounded_tapered_box();
    let s = crate::shell(&b, &[Vec3::new(0.0, 0.0, 40.0)], 2.0).unwrap();
    let gaps = edge_gaps(&s);
    let rounds: Vec<f64> = gaps.iter().filter(|(round, _)| *round).map(|g| g.1).collect();
    assert_eq!(rounds.len(), 8, "four outer and four inner rounds: {gaps:?}");
    assert!(rounds.iter().all(|g| *g < 1e-5), "a round's edge off its face: {rounds:?}");
}

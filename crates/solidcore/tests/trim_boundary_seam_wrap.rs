//! WR-491: a cone frustum whose seam edge lies a few tens of micrometres beside the
//! seam its circles start and end at. The edge's UV trim then sits at the far side of the
//! seam from its neighbours' ends, a whole period away, and `CreateOrientedTrimBoundary`
//! fills that gap with a line across the whole domain. The loop doubles back on itself, so
//! the lateral face integrates to nothing and the body to the wrong volume. The opt-in
//! `Kernel::fixes.trim_boundary_seam_wrap` takes the gap for a seam crossing instead.
//!
//! The body is a frustum of base radius 3, top radius 2 and height 4, its seam edge
//! replaced by the same line turned about the axis by an arc `offset` long at the base.
//! The SolidCraft case behind it is a threaded insert whose thread flanks are cones
//! like this one (16 of 313 faces).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_area, brep_volume, PropertyAttributes};
use solidcore::math::{Point3, PI};
use solidcore::primitive::api_primitives::create_cone;
use solidcore::topology::trimming_tools::clear_uv_trim_curves;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

const BASE: f64 = 3.0;
const TOP: f64 = 2.0;
const HEIGHT: f64 = 4.0;

/// The frustum with its seam edge turned by `offset` (an arc length at the base radius)
/// and no UV trim curves, so they are made from the moved edge.
fn frustum(k: &mut Kernel, offset: f64) -> Brep {
    let mut b = create_cone(k, Point3::new(0.0, 0.0, 0.0), BASE, TOP, HEIGHT).unwrap();
    let edges: Vec<EdgeId> = b.edges().ids().collect();
    for e in edges {
        let ivl = edge::interval(&b, e).unwrap();
        let c = edge::curve(&b, e).unwrap().unwrap();
        let (p0, p1) = (
            c.evaluate_point(k, ivl.min()).unwrap(),
            c.evaluate_point(k, ivl.max()).unwrap(),
        );
        if (p0.z - p1.z).abs() < 1e-6 {
            continue; // a circle
        }
        let angle = offset / BASE;
        let turn = |p: Point3| {
            Point3::new(
                p.x * angle.cos() - p.y * angle.sin(),
                p.x * angle.sin() + p.y * angle.cos(),
                p.z,
            )
        };
        let line = Curve::from(Line::create_line_segment(3, turn(p0), turn(p1), None).unwrap());
        let seam = b.edge_mut(e).unwrap();
        seam.interval = line.natural_interval();
        seam.curve = Some(line);
    }
    let faces = b.face_list().to_vec();
    for f in faces {
        clear_uv_trim_curves(&mut b, f).unwrap();
    }
    b
}

/// Area and volume of the frustum with the fix on or off.
fn measure(offset: f64, fixed: bool) -> (f64, f64) {
    let mut k = Kernel::default();
    k.fixes.trim_boundary_seam_wrap = fixed;
    let mut b = frustum(&mut k, offset);
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    (
        brep_area(&k, &mut b, 1e-6).unwrap(),
        brep_volume(&k, &mut b, 1e-6, &attrs).unwrap(),
    )
}

/// The exact frustum: lateral area, two caps, volume.
fn exact() -> (f64, f64, f64) {
    let lateral = PI * (BASE + TOP) * (HEIGHT * HEIGHT + (BASE - TOP) * (BASE - TOP)).sqrt();
    let caps = PI * (BASE * BASE + TOP * TOP);
    let volume = PI * HEIGHT / 3.0 * (BASE * BASE + BASE * TOP + TOP * TOP);
    (lateral, caps, volume)
}

#[test]
fn wr491_seam_wrap_collapses_the_lateral_face_by_default() {
    let (lateral, caps, volume) = exact();
    for offset in [5e-5, -5e-5] {
        let (area, v) = measure(offset, false);
        // Upstream: the lateral face integrates to ~0, leaving the two caps.
        assert!(
            (area - caps).abs() < 1e-2,
            "{offset}: area {area} vs caps {caps}"
        );
        assert!((v - volume).abs() > 1.0, "{offset}: volume {v} vs {volume}");
        assert!(area < 0.5 * (lateral + caps));
    }
}

#[test]
fn wr491_seam_wrap_fix_gives_the_exact_frustum() {
    let (lateral, caps, volume) = exact();
    for offset in [5e-5, -5e-5, 5e-4] {
        let (area, v) = measure(offset, true);
        assert!(
            (area - lateral - caps).abs() < 1e-3 * (lateral + caps),
            "{offset}: area {area}"
        );
        assert!(
            (v - volume).abs() < 1e-3 * volume,
            "{offset}: volume {v} vs {volume}"
        );
    }
}

/// A seam edge on its seam has no gap: the fix changes nothing.
#[test]
fn wr491_seam_wrap_fix_keeps_a_sound_body() {
    let (off, on) = (measure(0.0, false), measure(0.0, true));
    assert_eq!(off.0.to_bits(), on.0.to_bits());
    assert_eq!(off.1.to_bits(), on.1.to_bits());
    let (lateral, caps, volume) = exact();
    assert!((on.0 - lateral - caps).abs() < 1e-6 * (lateral + caps));
    assert!((on.1 - volume).abs() < 1e-6 * volume);
}

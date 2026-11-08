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
use solidcore::mass::integrator::properties::{
    brep_area, brep_properties, brep_volume, MassPropertyBehavior, PropertyAttributes,
};
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

// ---- #1437: pole gaps are not seam crossings ------------------------------------------

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::primitive::api_primitives::{
    create_box, create_cylinder, create_partial_sphere, create_sphere, create_torus,
};

/// Area and volume of `make`'s body at accuracy `acc` with the fix on or off.
fn measure_body(
    make: fn(&mut Kernel) -> Brep,
    acc: f64,
    fixed: bool,
) -> (solidcore::Result<f64>, solidcore::Result<f64>) {
    let mut k = Kernel::default();
    k.fixes.trim_boundary_seam_wrap = fixed;
    let mut b = make(&mut k);
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    (
        brep_area(&k, &mut b, acc),
        brep_volume(&k, &mut b, acc, &attrs),
    )
}

/// A named body maker with its exact area and volume.
type Body = (&'static str, fn(&mut Kernel) -> Brep, f64, f64);

/// Every body of #1437's matrix, with its exact area and volume.
fn bodies() -> [Body; 5] {
    const R: f64 = 7.0;
    [
        // one face whose loop has a gap line at each pole
        (
            "sphere",
            |k| create_sphere(k, Point3::new(0.0, 0.0, 0.0), R).unwrap(),
            4.0 * PI * R * R,
            4.0 / 3.0 * PI * R * R * R,
        ),
        // the upper hemisphere: one gap line, at the north pole
        (
            "hemisphere",
            |k| {
                let s = create_sphere(k, Point3::new(0.0, 0.0, 0.0), R).unwrap();
                let bx = create_box(k, Point3::new(-10.0, -10.0, -20.0), 20.0, 20.0, 20.0).unwrap();
                boolean_difference(k, s, bx).unwrap()
            },
            3.0 * PI * R * R,
            2.0 / 3.0 * PI * R * R * R,
        ),
        // a quarter sector: the lune meets both poles, no seam inside
        (
            "sector",
            |k| create_partial_sphere(k, Point3::new(0.0, 0.0, 0.0), R, 0.0, 90.0).unwrap(),
            PI * R * R + PI * R * R,
            PI * R * R * R / 3.0,
        ),
        // closed in both directions, no singularity
        (
            "torus",
            |k| create_torus(k, Point3::new(0.0, 0.0, 0.0), 10.0, 3.0).unwrap(),
            4.0 * PI * PI * 10.0 * 3.0,
            2.0 * PI * PI * 10.0 * 9.0,
        ),
        // a seam edge, no singularity
        (
            "cylinder",
            |k| create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 5.0, 12.0).unwrap(),
            2.0 * PI * 5.0 * 12.0 + 2.0 * PI * 25.0,
            PI * 25.0 * 12.0,
        ),
    ]
}

/// #1437: with the fix on, every body measures what it does with the fix off (bit for
/// bit: none has a seam gap to wrap) and its exact area and volume. Before the fix, the
/// sphere's two pole gaps were taken for seam crossings and it measured 0.
#[test]
fn i1437_pole_gaps_are_not_seam_crossings() {
    for (name, make, area, volume) in bodies() {
        let (off, on) = (
            measure_body(make, 1e-6, false),
            measure_body(make, 1e-6, true),
        );
        let (a_off, v_off) = (off.0.unwrap(), off.1.unwrap());
        let (a_on, v_on) = (on.0.unwrap(), on.1.unwrap());
        assert_eq!(
            a_off.to_bits(),
            a_on.to_bits(),
            "{name}: area {a_off} vs {a_on}"
        );
        assert_eq!(
            v_off.to_bits(),
            v_on.to_bits(),
            "{name}: volume {v_off} vs {v_on}"
        );
        assert!(
            (a_on - area).abs() < 1e-5 * area,
            "{name}: area {a_on} vs {area}"
        );
        assert!(
            (v_on - volume).abs() < 1e-5 * volume,
            "{name}: volume {v_on} vs {volume}"
        );
    }
}

/// #1437's reproduction at 1e-8: the sphere's area, volume and inertia with the fix on
/// (the flag-off volume is #1431's `SM_ERR 1001`, so this compares with the exact values).
#[test]
fn i1437_sphere_properties_with_the_fix() {
    const R: f64 = 7.0;
    let mut k = Kernel::default();
    k.fixes.trim_boundary_seam_wrap = true;
    k.fixes.precise_properties_body_length_scale = true;
    let mut b = create_sphere(&mut k, Point3::new(1.0, -2.0, 3.0), R).unwrap();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let p = brep_properties(
        &k,
        &mut b,
        1e-8,
        Point3::new(1.0, -2.0, 3.0),
        MassPropertyBehavior::default(),
        &attrs,
    )
    .unwrap();
    let (area, volume) = (4.0 * PI * R * R, 4.0 / 3.0 * PI * R * R * R);
    assert!((p.area - area).abs() < 1e-7 * area, "area {}", p.area);
    assert!(
        (p.volume - volume).abs() < 1e-7 * volume,
        "volume {}",
        p.volume
    );
    // about the centre, the second volume moments (x², y², z²) are each V r² / 5
    let second = volume * R * R / 5.0;
    for c in [p.moments[5].x, p.moments[5].y, p.moments[5].z] {
        assert!(
            (c - second).abs() < 1e-6 * second,
            "moments {:?}",
            p.moments
        );
    }
}

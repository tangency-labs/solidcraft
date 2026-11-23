//! Tests for [`super`]: bores read from a record's faces, and the stacks among bodies' bores.
//! Every expected value is derived by hand from the fixture's geometry.

use crate::eval::geometry::{Curve, Curve2, Frame, Surface};
use crate::eval::query::{Edge, Face, Loop, Record, Use};
use crate::math::Vec3;

use super::{bores, stacks, Bore, Options, Slot};

/// A frame at `origin` with x and y the world's, so z is the world's +z.
fn frame(origin: [f64; 3]) -> Frame {
    Frame {
        origin,
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
    }
}

/// The face's use of `edge`, with a pcurve spanning `v0` to `v1` (a rim's, `v0 == v1`).
fn use_of(edge: usize, v0: f64, v1: f64) -> Use {
    Use {
        edge,
        forward: true,
        pcurve: Curve2::Spline {
            knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![[0.0, v0], [0.0, v1]],
        },
    }
}

fn line_edge(start: [f64; 3], end: [f64; 3]) -> Edge {
    Edge {
        curve: Some(Curve::Line { start, end }),
        interval: [0.0, 1.0],
        faces: vec![0],
    }
}

/// A circle of `radius` about `origin`'s z, from `interval`'s min to its max.
fn arc_edge(origin: [f64; 3], radius: f64, interval: [f64; 2]) -> Edge {
    Edge {
        curve: Some(Curve::Circle {
            frame: frame(origin),
            radius,
        }),
        interval,
        faces: vec![0],
    }
}

/// A record of one round hole: a 2 mm plate with a Ø5 hole through it, the wall one cylinder
/// face with a seam (this face's loop uses it twice) and closed rims.
fn round_hole() -> Record {
    let seam = line_edge([2.5, 0.0, 0.0], [2.5, 0.0, 2.0]);
    let bottom = arc_edge([0.0, 0.0, 0.0], 2.5, [0.0, std::f64::consts::TAU]);
    let top = arc_edge([0.0, 0.0, 2.0], 2.5, [0.0, std::f64::consts::TAU]);
    let face = Face {
        parameters: None,
        surface: Some(Surface::Cylinder {
            frame: frame([0.0, 0.0, 0.0]),
            radius: 2.5,
        }),
        domain: [[0.0, std::f64::consts::TAU * 2.5], [0.0, 2.0]],
        forward: Some(false),
        loops: vec![Loop::Edges(vec![
            use_of(1, 0.0, 0.0),
            use_of(0, 0.0, 2.0),
            use_of(2, 2.0, 2.0),
            use_of(0, 0.0, 2.0),
        ])],
    };
    Record {
        edges: vec![seam, bottom, top],
        faces: vec![face],
    }
}

/// A record of one obround slot: two half-cylinder ends, Ø6 and 3 mm thick, their centres 6 mm
/// apart along +y, so the slot is 12 mm long overall. The end at the origin bulges −y, the end
/// at (0, 6, 0) bulges +y.
fn slot() -> Record {
    let face = |origin: [f64; 3], uses: [usize; 4]| Face {
        parameters: None,
        surface: Some(Surface::Cylinder {
            frame: frame(origin),
            radius: 3.0,
        }),
        domain: [[0.0, std::f64::consts::PI * 3.0], [0.0, 3.0]],
        forward: Some(false),
        loops: vec![Loop::Edges(vec![
            use_of(uses[0], 0.0, 3.0),
            use_of(uses[1], 0.0, 3.0),
            use_of(uses[2], 0.0, 0.0),
            use_of(uses[3], 3.0, 3.0),
        ])],
    };
    Record {
        edges: vec![
            // The end at the origin, bulging −y: sides at angles 0 and π, arcs from π to 2π.
            line_edge([3.0, 0.0, 0.0], [3.0, 0.0, 3.0]),
            line_edge([-3.0, 0.0, 0.0], [-3.0, 0.0, 3.0]),
            arc_edge(
                [0.0, 0.0, 0.0],
                3.0,
                [std::f64::consts::PI, std::f64::consts::TAU],
            ),
            arc_edge(
                [0.0, 0.0, 3.0],
                3.0,
                [std::f64::consts::PI, std::f64::consts::TAU],
            ),
            // The end at (0, 6, 0), bulging +y: arcs from 0 to π.
            line_edge([3.0, 6.0, 0.0], [3.0, 6.0, 3.0]),
            line_edge([-3.0, 6.0, 0.0], [-3.0, 6.0, 3.0]),
            arc_edge([0.0, 6.0, 0.0], 3.0, [0.0, std::f64::consts::PI]),
            arc_edge([0.0, 6.0, 3.0], 3.0, [0.0, std::f64::consts::PI]),
        ],
        faces: vec![
            face([0.0, 0.0, 0.0], [0, 1, 2, 3]),
            face([0.0, 6.0, 0.0], [4, 5, 6, 7]),
        ],
    }
}

/// A record of one countersunk hole: the wall (Ø5) from z = 0 to 0.5, and above it a 90°
/// countersink from Ø5 to Ø8 at the top face z = 2, which the wall's extent runs through.
fn countersink() -> Record {
    let seam = line_edge([2.5, 0.0, 0.0], [2.5, 0.0, 0.5]);
    let bottom = arc_edge([0.0, 0.0, 0.0], 2.5, [0.0, std::f64::consts::TAU]);
    let mid = arc_edge([0.0, 0.0, 0.5], 2.5, [0.0, std::f64::consts::TAU]);
    let wall = Face {
        parameters: None,
        surface: Some(Surface::Cylinder {
            frame: frame([0.0, 0.0, 0.0]),
            radius: 2.5,
        }),
        domain: [[0.0, std::f64::consts::TAU * 2.5], [0.0, 0.5]],
        forward: Some(false),
        loops: vec![Loop::Edges(vec![
            use_of(1, 0.0, 0.0),
            use_of(0, 0.0, 0.5),
            use_of(2, 0.5, 0.5),
            use_of(0, 0.0, 0.5),
        ])],
    };
    // The cone: its frame at the top face with z the world's −z, radius 4 there, closing by 45°
    // a half turn to the wall's 2.5 at z = 0.5 (1.5 along its axis).
    let cone = Face {
        parameters: None,
        surface: Some(Surface::Cone {
            frame: Frame {
                origin: [0.0, 0.0, 2.0],
                x_axis: [1.0, 0.0, 0.0],
                y_axis: [0.0, -1.0, 0.0],
            },
            radius_at_origin: 4.0,
            half_angle: -std::f64::consts::FRAC_PI_4,
        }),
        domain: [[0.0, std::f64::consts::TAU * 4.0], [0.0, 1.5]],
        forward: Some(false),
        loops: vec![],
    };
    Record {
        edges: vec![seam, bottom, mid],
        faces: vec![wall, cone],
    }
}

/// A record whose one cylinder face has two sides that are not on one diameter (a hole another
/// feature has bitten into), and no seam: not a bore.
fn bitten() -> Record {
    let edges = vec![
        line_edge([3.0, 0.0, 0.0], [3.0, 0.0, 2.0]),
        line_edge([0.0, 3.0, 0.0], [0.0, 3.0, 2.0]),
        arc_edge([0.0, 0.0, 0.0], 3.0, [0.0, std::f64::consts::FRAC_PI_2]),
        arc_edge([0.0, 0.0, 2.0], 3.0, [0.0, std::f64::consts::FRAC_PI_2]),
    ];
    let face = Face {
        parameters: None,
        surface: Some(Surface::Cylinder {
            frame: frame([0.0, 0.0, 0.0]),
            radius: 3.0,
        }),
        domain: [[0.0, std::f64::consts::FRAC_PI_2 * 3.0], [0.0, 2.0]],
        forward: Some(false),
        loops: vec![Loop::Edges(vec![
            use_of(0, 0.0, 2.0),
            use_of(1, 0.0, 2.0),
            use_of(2, 0.0, 0.0),
            use_of(3, 2.0, 2.0),
        ])],
    };
    Record {
        edges,
        faces: vec![face],
    }
}

fn hole(point: [f64; 3], t: [f64; 2], d: f64) -> Bore {
    Bore {
        point: Vec3::from_array(point),
        axis: Vec3::new(0.0, 0.0, 1.0),
        d,
        t,
        slot: None,
    }
}

fn opts() -> Options {
    Options {
        touching: 0.05,
        axis_tolerance: 0.1,
    }
}

#[test]
fn a_round_hole_is_one_bore_with_its_extent() {
    let b = bores(&round_hole());
    assert_eq!(b.len(), 1, "{b:?}");
    assert_eq!(b[0].point, Vec3::ZERO);
    assert_eq!(b[0].axis, Vec3::new(0.0, 0.0, 1.0));
    assert_eq!(b[0].d, 5.0);
    assert_eq!(b[0].t, [0.0, 2.0]);
    assert_eq!(b[0].slot, None);
}

#[test]
fn a_boss_is_not_a_bore() {
    let mut r = round_hole();
    if let Some(f) = r.faces.first_mut() {
        f.forward = Some(true);
    }
    assert!(bores(&r).is_empty());
}

#[test]
fn two_facing_half_walls_are_one_slot() {
    let b = bores(&slot());
    assert_eq!(b.len(), 1, "{b:?}");
    let s = b[0].slot.expect("one slot");
    assert!(
        (b[0].point.x - 0.0).abs() < 1e-9
            && (b[0].point.y - 3.0).abs() < 1e-9
            && (b[0].point.z - 0.0).abs() < 1e-9,
        "{b:?}"
    );
    assert_eq!(b[0].d, 6.0);
    assert_eq!(b[0].t, [0.0, 3.0]);
    assert!((s.length - 12.0).abs() < 1e-9, "{s:?}");
    assert!((s.along.y - 1.0).abs() < 1e-9, "{s:?}");
}

#[test]
fn a_countersink_extends_the_walls_extent() {
    let b = bores(&countersink());
    assert_eq!(b.len(), 1, "{b:?}");
    assert_eq!(b[0].d, 5.0);
    assert_eq!(b[0].t, [0.0, 2.0], "{b:?}");
}

#[test]
fn a_bitten_wall_is_not_a_bore() {
    assert!(bores(&bitten()).is_empty());
}

#[test]
fn a_body_of_too_many_faces_answers_no_bores() {
    let mut r = round_hole();
    r.faces.resize_with(super::MAX_FACES + 1, || Face {
        parameters: None,
        surface: None,
        domain: [[0.0, 1.0], [0.0, 1.0]],
        forward: None,
        loops: vec![],
    });
    assert!(bores(&r).is_empty());
}

#[test]
fn three_coaxial_holes_are_one_stack() {
    let (a, b, c) = (
        hole([20.0, 15.0, 0.0], [0.0, 2.0], 5.2),
        hole([20.0, 15.0, 2.0], [0.0, 3.0], 5.2),
        hole([20.0, 15.0, 5.0], [0.0, 2.0], 5.2),
    );
    let s = stacks(&[&[a], &[b], &[c]], &opts()).unwrap();
    assert_eq!(s.stacks.len(), 1, "{s:?}");
    let st = &s.stacks[0];
    // From the top down: −z, starting on the top plate's top face.
    assert_eq!(st.axis, Vec3::new(0.0, 0.0, -1.0));
    assert!(
        (st.start.x - 20.0).abs() < 1e-9
            && (st.start.y - 15.0).abs() < 1e-9
            && (st.start.z - 7.0).abs() < 1e-9,
        "{st:?}"
    );
    assert!((st.grip - 7.0).abs() < 1e-9, "{st:?}");
    assert_eq!(
        st.layers.iter().map(|l| l.body).collect::<Vec<_>>(),
        vec![2, 1, 0]
    );
    assert_eq!(st.layers[0].t, [0.0, 2.0]);
    assert_eq!(st.layers[1].t, [2.0, 5.0]);
    assert_eq!(st.layers[2].t, [5.0, 7.0]);
    assert!(s.lone.is_empty(), "{s:?}");
}

#[test]
fn a_hole_two_tenths_off_is_no_stack() {
    let (a, b) = (
        hole([0.0, 0.0, 0.0], [0.0, 2.0], 5.0),
        hole([0.2, 0.0, 2.0], [0.0, 2.0], 5.0),
    );
    let s = stacks(
        &[std::slice::from_ref(&a), std::slice::from_ref(&b)],
        &opts(),
    )
    .unwrap();
    assert!(s.stacks.is_empty(), "{s:?}");
    assert_eq!(s.lone, vec![(0, 0), (1, 0)]);
    let loose = Options {
        touching: 0.05,
        axis_tolerance: 0.3,
    };
    let s = stacks(&[&[a], &[b]], &loose).unwrap();
    assert_eq!(s.stacks.len(), 1, "{s:?}");
}

#[test]
fn a_counterbore_and_its_hole_are_one_layer_of_their_narrowest_opening() {
    let through = hole([0.0, 0.0, 0.0], [0.0, 0.8], 5.0);
    let cbore = hole([0.0, 0.0, 0.8], [0.0, 1.2], 9.0);
    let plate = hole([0.0, 0.0, 2.0], [0.0, 2.0], 5.5);
    let s = stacks(&[&[through, cbore], &[plate]], &opts()).unwrap();
    assert_eq!(s.stacks.len(), 1, "{s:?}");
    let st = &s.stacks[0];
    assert!((st.grip - 4.0).abs() < 1e-9, "{st:?}");
    assert_eq!(st.layers.len(), 2, "{st:?}");
    assert_eq!(st.layers[0].d, 5.5);
    assert_eq!(st.layers[0].t, [0.0, 2.0]);
    assert_eq!(st.layers[1].d, 5.0);
    assert_eq!(st.layers[1].t, [2.0, 4.0]);
    assert!(s.lone.is_empty(), "{s:?}");
}

#[test]
fn plates_a_millimetre_apart_are_no_stack() {
    let (a, b) = (
        hole([0.0, 0.0, 0.0], [0.0, 2.0], 5.0),
        hole([0.0, 0.0, 3.0], [0.0, 2.0], 5.0),
    );
    let s = stacks(
        &[std::slice::from_ref(&a), std::slice::from_ref(&b)],
        &opts(),
    )
    .unwrap();
    assert!(s.stacks.is_empty(), "{s:?}");
    assert_eq!(s.lone.len(), 2);
    let wide = Options {
        touching: 1.0,
        axis_tolerance: 0.1,
    };
    let s = stacks(&[&[a], &[b]], &wide).unwrap();
    assert_eq!(s.stacks.len(), 1, "{s:?}");
}

#[test]
fn two_stacks_on_one_axis() {
    let (a, b, c, d) = (
        hole([0.0, 0.0, 0.0], [0.0, 2.0], 5.0),
        hole([0.0, 0.0, 2.0], [0.0, 2.0], 5.0),
        hole([0.0, 0.0, 10.0], [0.0, 2.0], 5.0),
        hole([0.0, 0.0, 12.0], [0.0, 2.0], 5.0),
    );
    let s = stacks(&[&[a, c], &[b, d]], &opts()).unwrap();
    assert_eq!(s.stacks.len(), 2, "{s:?}");
    assert!(
        s.stacks.iter().all(|st| (st.grip - 4.0).abs() < 1e-9),
        "{s:?}"
    );
}

#[test]
fn a_round_hole_over_a_slot_is_a_stack() {
    let sl = Bore {
        point: Vec3::new(20.0, 15.0, 0.0),
        axis: Vec3::new(0.0, 0.0, 1.0),
        d: 6.0,
        t: [0.0, 3.0],
        slot: Some(Slot {
            length: 12.0,
            along: Vec3::new(0.0, 1.0, 0.0),
        }),
    };
    // The round hole sits 2 mm off the slot's centre, over its length.
    let round = hole([20.0, 17.0, 3.0], [0.0, 2.0], 6.0);
    let s = stacks(&[&[sl], &[round]], &opts()).unwrap();
    assert_eq!(s.stacks.len(), 1, "{s:?}");
    let st = &s.stacks[0];
    assert!((st.grip - 5.0).abs() < 1e-9, "{st:?}");
    assert_eq!(st.layers.len(), 2, "{st:?}");
    assert!(st.layers[0].slot.is_none(), "{st:?}");
    assert!(st.layers[1].slot.is_some(), "{st:?}");
}

#[test]
fn slots_on_one_centre_line_stack_through_their_shared_middle() {
    let sl = |x: f64| Bore {
        point: Vec3::new(x, 10.0, 0.0),
        axis: Vec3::new(0.0, 0.0, 1.0),
        d: 6.0,
        t: [0.0, 3.0],
        slot: Some(Slot {
            length: 12.0,
            along: Vec3::new(1.0, 0.0, 0.0),
        }),
    };
    let s = stacks(&[&[sl(10.0)], &[sl(11.0)]], &opts()).unwrap();
    assert_eq!(s.stacks.len(), 1, "{s:?}");
    assert!((s.stacks[0].grip - 3.0).abs() < 1e-9, "{s:?}");
    assert_eq!(s.stacks[0].layers.len(), 2, "{s:?}");
}

#[test]
fn too_many_nearly_coaxial_holes_refuse() {
    let far: Vec<Bore> = (0..2000)
        .map(|i| hole([0.0, 0.0, f64::from(i)], [0.0, 0.1], 5.0))
        .collect();
    let near: Vec<Bore> = (0..2000)
        .map(|i| hole([0.1, 0.0, f64::from(i)], [0.0, 0.1], 5.0))
        .collect();
    assert!(stacks(&[&far, &near], &opts()).is_err());
}

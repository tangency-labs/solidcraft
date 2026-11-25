//! Picks (tangency-labs/solidcore#3138): the face at a point, by the kernel's classification,
//! none on an edge or off the body; and a pick grown across the edges where the kernel proves
//! the two faces continue one surface.
#![allow(clippy::unwrap_used)]

use solidcore_ops::split::{split_faces, Tool};
use solidcore_ops::{make_box, session, Body, Order};
use solidcore_record::brep::Surface;

fn all_fixes() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
}

/// A 10 cube's faces cut by the plane x = 5: the top is two halves on one plane definition.
fn halved_cube() -> (Body, Order) {
    let b = make_box([0.0; 3], [10.0; 3]).unwrap();
    let (_, o) = b.to_record().unwrap();
    let tool = Tool {
        surface: Surface::Plane {
            origin: [5.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
            v_axis: [0.0, 0.0, 1.0],
        },
        domain: [[-1.0, 11.0], [-1.0, 11.0]],
    };
    let all: Vec<usize> = (0..6).collect();
    let (r, _) = split_faces(&b, &o, &all, &[tool]).unwrap().unwrap();
    let (_, ro) = r.to_record().unwrap();
    (r, ro)
}

/// The face at a point inside each top half is that half; a point on the line between them
/// (an edge), on a box edge, or off the body picks no face.
#[test]
fn a_point_picks_the_face_it_lies_on_and_no_other() {
    all_fixes();
    let (b, o) = halved_cube();
    assert_eq!(o.faces.len(), 10);
    let left = b.face_at(&o, [2.5, 5.0, 10.0], 1e-6).unwrap().unwrap();
    let right = b.face_at(&o, [7.5, 5.0, 10.0], 1e-6).unwrap().unwrap();
    assert_ne!(left, right);
    let c = b.face_centroid(&o, left, 1e-8).unwrap().centroid;
    assert!(
        (c[0] - 2.5).abs() < 1e-9 && (c[2] - 10.0).abs() < 1e-9,
        "{c:?}"
    );
    assert_eq!(
        b.face_at(&o, [5.0, 5.0, 10.0], 1e-6).unwrap(),
        None,
        "on the split line"
    );
    assert_eq!(
        b.face_at(&o, [0.0, 5.0, 10.0], 1e-6).unwrap(),
        None,
        "on a box edge"
    );
    assert_eq!(
        b.face_at(&o, [5.0, 5.0, 20.0], 1e-6).unwrap(),
        None,
        "off the body"
    );
    assert_eq!(
        b.face_at(&o, [5.0, 5.0, 5.0], 1e-6).unwrap(),
        None,
        "inside the body"
    );
    assert!(b.face_at(&o, [5.0, 5.0, 10.0], 0.0).is_err());
}

/// One top half grows to the other across the split line (one plane definition, the same
/// outward side: proved) and to nothing across the box's edges (another plane); with the
/// split line kept, it stays alone.
#[test]
fn a_pick_grows_across_a_split_line_but_not_a_kept_one() {
    all_fixes();
    let (b, o) = halved_cube();
    let left = b.face_at(&o, [2.5, 5.0, 10.0], 1e-6).unwrap().unwrap();
    let right = b.face_at(&o, [7.5, 5.0, 10.0], 1e-6).unwrap().unwrap();
    assert_eq!(
        b.faces_on_one_surface(&o, &[left], &[]).unwrap(),
        vec![left, right]
    );
    let line: Vec<usize> = (0..o.edges.len())
        .filter(|&e| {
            let m = b.edge_middle(&o, e).unwrap();
            (m[0] - 5.0).abs() < 1e-9 && (m[2] - 10.0).abs() < 1e-9
        })
        .collect();
    assert_eq!(line.len(), 1, "the split line on the top");
    assert_eq!(
        b.faces_on_one_surface(&o, &[left], &line).unwrap(),
        vec![left]
    );
    assert!(b.faces_on_one_surface(&o, &[99], &[]).is_err());
}

/// Each face centre of a box picks its own face, six in all; a point on an edge is as near two
/// faces and ambiguous; a point farther than the tolerance from every face picks none.
#[test]
fn a_box_face_picks_by_point() {
    all_fixes();
    let b = make_box([0.0; 3], [1.0, 2.0, 3.0]).unwrap();
    let centres = [
        [0.0, 1.0, 1.5],
        [1.0, 1.0, 1.5],
        [0.5, 0.0, 1.5],
        [0.5, 2.0, 1.5],
        [0.5, 1.0, 0.0],
        [0.5, 1.0, 3.0],
    ];
    let picked = solidcore_ops::pick::faces(&b, &centres, 1e-6)
        .unwrap()
        .unwrap();
    let mut distinct = picked.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!((picked.len(), distinct.len()), (6, 6), "{picked:?}");
    assert!(distinct.iter().all(|&f| f < 6));
    assert_eq!(
        solidcore_ops::pick::faces(&b, &[[0.0, 0.0, 1.5]], 1e-6).unwrap(),
        Err(solidcore_ops::pick::Miss::Ambiguous(0))
    );
    assert_eq!(
        solidcore_ops::pick::faces(&b, &[[0.5, 1.0, 3.0], [9.0, 9.0, 9.0]], 1e-6).unwrap(),
        Err(solidcore_ops::pick::Miss::None(1))
    );
    assert!(solidcore_ops::pick::faces(&b, &[[f64::NAN, 0.0, 0.0]], 1e-6).is_err());
}

/// A box's faces each have one loop of four distinct corners: no outline touches itself.
#[test]
fn a_box_outline_touches_nowhere() {
    all_fixes();
    let b = make_box([0.0; 3], [1.0, 2.0, 3.0]).unwrap();
    assert_eq!(b.outline_touch().unwrap(), None);
/// A sphere inside a box cuts it into the ball and the rest: a closed tool's sheet is a closed
/// shell, split by intersection and difference.
#[test]
fn a_box_splits_by_a_sphere_into_the_ball_and_the_rest() {
    all_fixes();
    let b = make_box([0.0; 3], [10.0; 3]).unwrap();
    let tool = Tool {
        surface: Surface::Sphere {
            frame: solidcore_record::brep::Frame {
                origin: [5.0, 5.0, 5.0],
                x_axis: [1.0, 0.0, 0.0],
                y_axis: [0.0, 1.0, 0.0],
            },
            radius: 3.0,
        },
        domain: [
            [0.0, std::f64::consts::TAU],
            [-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2],
        ],
    };
    let (inside, outside) = solidcore_ops::split::split_body(&b, &tool)
        .unwrap()
        .unwrap();
    let ball = 4.0 / 3.0 * std::f64::consts::PI * 27.0;
    let rel = |a: f64, e: f64| (a - e).abs() / e;
    assert!(rel(inside.volume(1e-8).unwrap(), ball) < 1e-6);
    assert!(rel(outside.volume(1e-8).unwrap(), 1000.0 - ball) < 1e-6);
}

/// A plane strip that runs into a box's top face and ends half way across it cuts nothing off:
/// the split is refused as stopping short.
#[test]
fn a_tool_that_ends_inside_a_face_stops_short() {
    all_fixes();
    let b = make_box([0.0; 3], [10.0; 3]).unwrap();
    let (record, order) = b.to_record().unwrap();
    let top = record
        .faces
        .iter()
        .position(|f| match &f.surface {
            Surface::Plane {
                origin,
                u_axis,
                v_axis,
            } => origin[2] == 10.0 && u_axis[2] == 0.0 && v_axis[2] == 0.0,
            _ => false,
        })
        .unwrap();
    let strip = Tool {
        surface: Surface::Plane {
            origin: [5.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
            v_axis: [0.0, 0.0, 1.0],
        },
        domain: [[-1.0, 5.0], [-1.0, 11.0]],
    };
    assert_eq!(
        split_faces(&b, &order, &[top], &[strip]).err(),
        Some(solidcore_ops::Error::ToolStopsShort)
    );
}

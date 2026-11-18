//! `sweep::controlled::sweep_controlled` (#13664): a controlled sweep through the kernel's
//! stations, and which faces came from where, against closed forms.
//!
//! - a square of side 2 about the path, swept 10 along a line with end scale `k`: its sections
//!   vary linearly, which the cubic skin reproduces, so the volume is the frustum's
//!   `A L (1 + k + k^2) / 3`;
//! - its faces: one start cap in z = 0, one end cap in z = L, and a wall per profile line
//!   holding that line;
//! - twisted a quarter turn and scaled, the end cap's corners are the start's turned and scaled;
//! - one straight rail diverging from the path scales it as end scale `(d_0 + s L) / d_0`;
//! - a path with a corner and a closed path are refused, named; so are a twist and end scale
//!   past SolidCraft's limits, which live here, not in the kernel.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore_ops::sweep::controlled::{sweep_controlled, ControlledSweep, Piece, SweptFaces};
use solidcore_ops::sweep::Curve;
use solidcore_ops::{Body, Error};
use solidcore_record::brep::{Brep as Record, Loop};
use std::f64::consts::{FRAC_PI_2, PI};

const L: f64 = 10.0;

/// The square of side 2 about the origin in z = 0, counter-clockwise from (-1, -1).
fn square() -> Vec<Curve> {
    let c = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
    (0..4)
        .map(|i| Curve::Line {
            start: [c[i][0], c[i][1], 0.0],
            end: [c[(i + 1) % 4][0], c[(i + 1) % 4][1], 0.0],
        })
        .collect()
}

fn up(len: f64) -> Vec<Piece> {
    vec![Piece::Line {
        a: [0.0, 0.0, 0.0],
        b: [0.0, 0.0, len],
    }]
}

fn swept(laws: &ControlledSweep) -> (Body, SweptFaces) {
    sweep_controlled(&square(), [0.0, 0.0, 1.0], &up(L), laws).unwrap()
}

/// The points of the vertices of face `f`'s loops.
fn face_points(r: &Record, f: usize) -> Vec<[f64; 3]> {
    let mut out = Vec::new();
    for lp in &r.faces[f].loops {
        if let Loop::Edges { edges } = lp {
            for oe in edges {
                for v in r.edges[oe.edge as usize].vertices {
                    out.push(r.vertices[v as usize].point);
                }
            }
        }
    }
    out
}

fn has(points: &[[f64; 3]], p: [f64; 3]) -> bool {
    points
        .iter()
        .any(|q| (0..3).all(|i| (q[i] - p[i]).abs() < 1e-9))
}

#[test]
fn a_scaled_square_is_the_frustum() {
    for k in [1.0, 0.5, 2.0, 3.0] {
        let laws = ControlledSweep {
            end_scale: k,
            ..ControlledSweep::default()
        };
        let (b, _) = swept(&laws);
        let v = b.volume(1e-10).unwrap();
        let want = 4.0 * L * (1.0 + k + k * k) / 3.0;
        assert!((v - want).abs() <= 1e-9 * want, "k {k}: {v} vs {want}");
    }
}

#[test]
fn its_faces_are_the_caps_and_a_wall_per_profile_line() {
    let (b, faces) = swept(&ControlledSweep::default());
    let (r, _) = b.to_record().unwrap();
    assert_eq!(faces.start.len(), 1, "{faces:?}");
    assert_eq!(faces.end.len(), 1, "{faces:?}");
    assert_eq!(faces.walls.len(), 4, "{faces:?}");
    assert_eq!(r.faces.len(), 6);
    let at = |f: usize| face_points(&r, f);
    assert!(at(faces.start[0]).iter().all(|p| p[2].abs() < 1e-12));
    assert!(at(faces.end[0]).iter().all(|p| (p[2] - L).abs() < 1e-12));
    for (i, c) in square().iter().enumerate() {
        let Curve::Line { start, end } = c else {
            panic!("a line");
        };
        let wall = faces
            .walls
            .iter()
            .find(|(_, curve)| *curve == i)
            .expect("a wall for each line")
            .0;
        let points = at(wall);
        assert!(
            has(&points, *start) && has(&points, *end),
            "line {i}: {points:?}"
        );
    }
}

/// A profile given out of order, one side reversed: each wall still holds its own line (the
/// kernel's trace, not the skin's face order).
#[test]
fn a_shuffled_profile_s_walls_are_its_own_lines() {
    let mut profile = square();
    profile.swap(0, 2);
    profile.swap(1, 3);
    if let Curve::Line { start, end } = &mut profile[1] {
        std::mem::swap(start, end);
    }
    let (b, faces) = sweep_controlled(
        &profile,
        [0.0, 0.0, 1.0],
        &up(L),
        &ControlledSweep::default(),
    )
    .unwrap();
    let (r, _) = b.to_record().unwrap();
    assert_eq!(faces.walls.len(), 4, "{faces:?}");
    let mut seen: Vec<usize> = faces.walls.iter().map(|&(_, c)| c).collect();
    seen.sort_unstable();
    assert_eq!(seen, [0, 1, 2, 3], "{faces:?}");
    for &(wall, i) in &faces.walls {
        let Curve::Line { start, end } = profile[i] else {
            panic!("a line");
        };
        let points = face_points(&r, wall);
        // the wall runs from its line at the start to the line moved up L at the end
        let up_l = |p: [f64; 3]| [p[0], p[1], p[2] + L];
        assert!(
            has(&points, start) && has(&points, end) && has(&points, up_l(start)),
            "line {i}: {points:?}"
        );
    }
}

#[test]
fn a_twisted_scaled_square_ends_turned_and_scaled() {
    let (twist, k) = (FRAC_PI_2, 2.0);
    let laws = ControlledSweep {
        twist,
        end_scale: k,
        ..ControlledSweep::default()
    };
    let (b, faces) = swept(&laws);
    let (r, _) = b.to_record().unwrap();
    let end = face_points(&r, faces.end[0]);
    for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        // a quarter turn: sin 1, cos 0 (ops computes no trigonometry, docs/layers.md)
        let (s, c) = (1.0, 0.0);
        let want = [k * (x * c - y * s), k * (x * s + y * c), L];
        assert!(has(&end, want), "{want:?} in {end:?}");
    }
}

#[test]
fn a_diverging_rail_scales_as_its_distance() {
    let (d0, slope) = (4.0, 0.1);
    let rail = Curve::Line {
        start: [d0, 0.0, 0.0],
        end: [d0 + slope * L, 0.0, L],
    };
    let laws = ControlledSweep {
        rails: vec![rail],
        ..ControlledSweep::default()
    };
    let (b, _) = swept(&laws);
    let k = (d0 + slope * L) / d0;
    let want = 4.0 * L * (1.0 + k + k * k) / 3.0;
    let v = b.volume(1e-10).unwrap();
    assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
}

#[test]
fn a_cornered_or_closed_path_is_refused() {
    let corner = vec![
        Piece::Line {
            a: [0.0, 0.0, 0.0],
            b: [0.0, 0.0, L],
        },
        Piece::Line {
            a: [0.0, 0.0, L],
            b: [L, 0.0, L],
        },
    ];
    let err = sweep_controlled(
        &square(),
        [0.0, 0.0, 1.0],
        &corner,
        &ControlledSweep::default(),
    )
    .unwrap_err();
    assert!(
        matches!(err, Error::Unsupported(w) if w.contains("corners")),
        "{err:?}"
    );
    let closed = vec![Piece::Arc {
        centre: [-20.0, 0.0, 0.0],
        axis: [0.0, 1.0, 0.0],
        start: [0.0, 0.0, 0.0],
        angle: 2.0 * PI,
    }];
    let err = sweep_controlled(
        &square(),
        [0.0, 0.0, 1.0],
        &closed,
        &ControlledSweep::default(),
    )
    .unwrap_err();
    assert!(
        matches!(err, Error::Unsupported(w) if w.contains("closed")),
        "{err:?}"
    );
}

/// SolidCraft's limits are the ops entry's, passed to the kernel's laws: five turns of twist
/// and an end scale past 0.01 to 100 are refused, though the kernel's own defaults take them.
#[test]
fn solidcraft_s_limits_refuse_here() {
    use solidcore_ops::sweep::controlled::{MAX_SCALE, MAX_TWIST, MIN_SCALE};
    let refused = |laws: ControlledSweep| {
        let got = sweep_controlled(&square(), [0.0, 0.0, 1.0], &up(L), &laws);
        assert!(
            matches!(got, Err(Error::Refused { .. })),
            "{laws:?}: {got:?}"
        );
    };
    refused(ControlledSweep {
        twist: 5.0 * 2.0 * PI,
        ..ControlledSweep::default()
    });
    for end_scale in [0.5 * MIN_SCALE, 2.0 * MAX_SCALE] {
        refused(ControlledSweep {
            end_scale,
            ..ControlledSweep::default()
        });
    }
    assert_eq!(MAX_TWIST.to_bits(), (8.0 * PI).to_bits(), "four turns");
}

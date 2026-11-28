//! `sweep::controlled::sweep_controlled` (#13664): a controlled sweep through the kernel's
//! stations, and which faces came from where, against closed forms.
//!
//! - a square of side 2 about the path, swept 10 along a line with end scale `k`: its sections
//!   vary linearly, which the cubic skin reproduces, so the volume is the frustum's
//!   `A L (1 + k + k^2) / 3`;
//! - its faces: one start cap in z = 0, one end cap in z = L, and a wall per profile line
//!   holding that line;
//! - twisted a quarter turn and scaled, the end cap's corners are the start's turned and scaled;
//! - opposite axial twists give mirrored valid tools and cylinder cuts of equal volumes,
//!   including a profile whose outer arc lies on the cylinder;
//! - one straight rail diverging from the path scales it as end scale `(d_0 + s L) / d_0`;
//! - a path with a corner and a closed path are refused, named; so are a twist and end scale
//!   past SolidCraft's limits, which live here, not in the kernel.
#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::math::det;

#[path = "../../solidcore/tests/support/solidcraft_fixes.rs"]
mod solidcraft_fixes;

use solidcore_ops::boolean::boolean;
use solidcore_ops::sweep::controlled::{sweep_controlled, ControlledSweep, Piece, SweptFaces};
use solidcore_ops::sweep::Curve;
use solidcore_ops::{make_cylinder, session, Body, BooleanOp, CheckLevel, Error};
use solidcore_record::brep::{Brep as Record, Loop};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

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

/// Reflection in y = 0 changes the sign of an axial twist. Both profiles below are
/// invariant under that reflection, so both the tools and the cylinder cuts must have
/// equal volumes for opposite twists. The inner corner has a unique radius, identifying
/// the same vertex at the far end without relying on face or vertex ordering.
fn opposite_twist_cuts(profile: &[Curve], inner_radius: f64) {
    session::set_thread_fixes(solidcraft_fixes::solidcraft_fix_names()).unwrap();
    let cylinder_volume = PI * 36.0 * L;
    // A modest twist first, then the angle of the reported end mill (#20171).
    for degrees in [30.0_f64, 343.8] {
        let mut volumes = Vec::new();
        let mut end_angles = Vec::new();
        for sign in [1.0, -1.0] {
            let twist = sign * degrees.to_radians();
            let laws = ControlledSweep {
                twist,
                ..ControlledSweep::default()
            };
            let (tool, faces) = sweep_controlled(profile, [0.0, 0.0, 1.0], &up(L), &laws)
                .unwrap_or_else(|e| panic!("{degrees} degrees, sign {sign}: sweep: {e:?}"));
            assert!(tool.is_manifold_solid().unwrap());
            let report = tool.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "twist {twist}: tool: {}", report.text);

            let (record, _) = tool.to_record().unwrap();
            assert_eq!(faces.end.len(), 1, "twist {twist}: {faces:?}");
            let end = face_points(&record, faces.end[0]);
            let corners: Vec<_> = end
                .iter()
                .filter(|p| (det::hypot(p[0], p[1]) - inner_radius).abs() < 1e-8)
                .collect();
            assert!(!corners.is_empty(), "twist {twist}: end {end:?}");
            for p in corners {
                assert!((p[2] - L).abs() < 1e-9, "twist {twist}: {p:?}");
                let angle = det::atan2(p[1], p[0]);
                let error = (angle - twist).rem_euclid(TAU);
                assert!(
                    error.min(TAU - error) < 1e-9,
                    "twist {twist}: far-end vertex angle {angle}"
                );
                end_angles.push(angle);
            }

            let tool_volume = tool.volume(1e-8).unwrap();
            assert!(tool_volume > 0.0 && tool_volume < cylinder_volume);
            let cylinder = make_cylinder([0.0; 3], 6.0, L).unwrap();
            let cut = boolean(cylinder, tool, BooleanOp::Subtract)
                .unwrap_or_else(|e| panic!("{degrees} degrees, sign {sign}: cut: {e:?}"))
                .expect("the cut leaves cylinder material");
            assert!(cut.is_manifold_solid().unwrap());
            let report = cut.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "twist {twist}: cut: {}", report.text);
            let cut_volume = cut.volume(1e-8).unwrap();
            assert!(cut_volume > 0.0 && cut_volume < cylinder_volume);
            volumes.push((tool_volume, cut_volume));
        }
        // Ten times the requested integration accuracy covers the two integrations;
        // the expected equality follows from reflection, independently of the skin fit.
        let [(positive_tool, positive_cut), (negative_tool, negative_cut)] = volumes.as_slice()
        else {
            panic!("one result for each sign");
        };
        for (positive, negative) in [
            (*positive_tool, *negative_tool),
            (*positive_cut, *negative_cut),
        ] {
            assert!(
                (positive - negative).abs() <= 1e-7 * positive,
                "{degrees} degrees: positive volume {positive}, negative volume {negative}"
            );
        }
        assert!(
            end_angles.iter().any(|a| *a > 0.0) && end_angles.iter().any(|a| *a < 0.0),
            "{degrees} degrees: opposite far-end angles: {end_angles:?}"
        );
    }
}

#[test]
fn opposite_axial_twists_cut_a_cylinder_with_equal_volumes() {
    let points = [[1.0, 0.0, 0.0], [2.0, -0.5, 0.0], [2.0, 0.5, 0.0]];
    let profile: Vec<_> = (0..3)
        .map(|i| Curve::Line {
            start: points[i],
            end: points[(i + 1) % 3],
        })
        .collect();
    opposite_twist_cuts(&profile, 1.0);
}

/// The reported flute closes against an arc of the cylinder being cut. Unlike an
/// interior polygon, this exercises the contact between the skinned arc wall and the
/// cylinder. Two lines and a circular arc are enough; no splines or pattern are needed.
#[test]
fn opposite_axial_twists_cut_a_profile_touching_the_cylinder() {
    let half_angle = PI / 9.0;
    let (s, c) = det::sin_cos(half_angle);
    let inner = [4.0, 0.0, 0.0];
    let lower = [6.0 * c, -6.0 * s, 0.0];
    let upper = [6.0 * c, 6.0 * s, 0.0];
    let profile = [
        Curve::Line {
            start: inner,
            end: lower,
        },
        Curve::Arc {
            center: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
            radius: 6.0,
            start: -half_angle,
            end: half_angle,
        },
        Curve::Line {
            start: upper,
            end: inner,
        },
    ];
    opposite_twist_cuts(&profile, 4.0);
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

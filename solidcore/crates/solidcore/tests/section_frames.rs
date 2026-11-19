//! tangency-labs/solidcore#3140 step 5: a body's edge chain read as a blend's frames, against
//! closed forms, and against SolidCraft's five-point stencils; a chain of points read as frames,
//! and the turns that make open sections run one way (#15367).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::math::{Point2, Point3, Vec3};
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_plane};
use solidcore::query::api_queries::{edge_closest_point, surface_closest_point};
use solidcore::sweep::section_blend::{
    open_section_turns, point_frames, section_frames, section_frames_outward, FrameOptions,
};
use solidcore::topology::{Brep, EdgeId};
use solidcore::{Kernel, SolidError};

/// SolidCraft's rules: 0.2 of `leave` in the face's plane, even arc-length stations.
fn opts(stations: usize) -> FrameOptions {
    FrameOptions {
        stations,
        min_in_plane: 0.2,
    }
}

fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

fn near(a: Vec3, b: Vec3, tol: f64) -> bool {
    a.distance(b) <= tol
}

/// A cylinder's top rim (radius 5 at z = 10), left along the axis, down the wall: the wall's
/// generators are straight, so every frame's bend and change of bend are 0, and it leaves
/// straight down.
#[test]
fn a_cylinder_rim_left_down_its_wall_is_straight() {
    let mut k = Kernel::default();
    let b = create_cylinder(&mut k, Point3::ZERO, 5.0, 10.0).unwrap();
    let rim = pick(&k, &b, [0.0, 5.0, 10.0]);
    let frames = section_frames(&k, &b, &[rim], Vec3::new(0.0, 0.0, -1.0), &opts(9)).unwrap();
    assert_eq!(frames.len(), 9);
    for f in &frames {
        assert!((f.p.z - 10.0).abs() <= 1e-9 && (f.p.x.hypot(f.p.y) - 5.0).abs() <= 1e-9);
        assert!(
            near(f.leave, Vec3::new(0.0, 0.0, -1.0), 1e-12),
            "{:?}",
            f.leave
        );
        assert!(f.bend.length() <= 1e-9, "{:?}", f.bend);
        assert!(f.bend_change.length() <= 1e-9, "{:?}", f.bend_change);
    }
}

/// The same rim left along `(1, 0, -3)`: at each station the leave direction is its part in the
/// wall's tangent plane, and the bend's part along the wall's outward normal `e_r` is the wall's
/// normal curvature there, `-sin^2(a) / R`, `sin(a)` the leave direction's part round the
/// cylinder (`w . e_theta`): Euler's, whatever the surface's parametrization.
#[test]
fn a_cylinder_rim_left_askew_bends_by_the_wall_s_normal_curvature() {
    let mut k = Kernel::default();
    let b = create_cylinder(&mut k, Point3::ZERO, 5.0, 10.0).unwrap();
    let rim = pick(&k, &b, [0.0, 5.0, 10.0]);
    let leave = Vec3::new(1.0, 0.0, -3.0);
    let frames = section_frames(&k, &b, &[rim], leave, &opts(17)).unwrap();
    let l = leave.unitize().unwrap();
    for f in &frames {
        let e_r = Vec3::new(f.p.x, f.p.y, 0.0).unitize().unwrap();
        let e_theta = Vec3::new(-e_r.y, e_r.x, 0.0);
        let w = (l - e_r * l.dot(e_r)).unitize().unwrap();
        assert!(near(f.leave, w, 1e-9), "{:?} vs {w:?}", f.leave);
        let sin = w.dot(e_theta);
        let normal = f.bend.dot(e_r);
        assert!(
            (normal + sin * sin / 5.0).abs() <= 1e-9,
            "at {:?}: {normal} vs {}",
            f.p,
            -sin * sin / 5.0
        );
    }
}

/// A box's top front edge left into the top face: the plane bends nowhere.
#[test]
fn a_box_edge_left_into_its_top_is_flat() {
    let mut k = Kernel::default();
    let b = create_box(&mut k, Point3::ZERO, 20.0, 10.0, 5.0).unwrap();
    let e = pick(&k, &b, [10.0, 0.0, 5.0]);
    let frames = section_frames(&k, &b, &[e], Vec3::new(0.0, 1.0, 0.0), &opts(5)).unwrap();
    for f in &frames {
        assert!(
            near(f.leave, Vec3::new(0.0, 1.0, 0.0), 1e-12),
            "{:?}",
            f.leave
        );
        assert!(f.bend.length() <= 1e-12 && f.bend_change.length() <= 1e-12);
    }
}

/// SolidCraft's frames against these: at every station of the askew-left rim, SolidCraft's five
/// samples of the surface's parameter line (step a thousandth of the chain; central, or one-sided
/// into the face where the line leaves its parameters, as at the rim's seam) read the bend within
/// the stencils' truncation of the closed-form one. The measured difference is
/// printed (the PR states it).
#[test]
fn solidcraft_s_stencils_read_the_bend_within_their_truncation() {
    let mut k = Kernel::default();
    let b = create_cylinder(&mut k, Point3::ZERO, 5.0, 10.0).unwrap();
    let rim = pick(&k, &b, [0.0, 5.0, 10.0]);
    let leave = Vec3::new(1.0, 0.0, -3.0);
    let frames = section_frames(&k, &b, &[rim], leave, &opts(17)).unwrap();
    // the wall: the face of the rim whose plane keeps more of `leave` (the frames' own)
    let wall = b
        .faces()
        .ids()
        .find(|&f| {
            b.face(f)
                .unwrap()
                .surface
                .as_ref()
                .is_some_and(|s| s.is_planar(&k, 1e-9).is_ok_and(|p| !p.is_planar))
        })
        .unwrap();
    let surf = b.face(wall).unwrap().surface.as_ref().unwrap();
    let flags = EvaluateFlags {
        non_zero_tangents: false,
        ..EvaluateFlags::default()
    };
    let length = 2.0 * std::f64::consts::PI * 5.0;
    let h = (length * 1e-3).clamp(1e-6, 0.5);
    // SolidCraft's choice of samples (bridge::Support::station): central where the five fit in
    // the face's parameters, else forward, else backward, the step quartered up to eight times
    let range = b.face(wall).unwrap().uv_domain;
    let (lo, hi) = (range.min(), range.max());
    let slack = 1e-9;
    let mut worst = 0.0_f64;
    for f in &frames {
        let uv = surface_closest_point(&k, surf, f.p).unwrap().uvs[0];
        let d = surf.evaluate(&k, uv, 1, 1, flags).unwrap();
        let (su, sv) = (d[1][0], d[0][1]);
        let (a, bb, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
        let det = a * c - bb * bb;
        let du = (c * f.leave.dot(su) - bb * f.leave.dot(sv)) / det;
        let dv = (a * f.leave.dot(sv) - bb * f.leave.dot(su)) / det;
        let on = |t: f64, h: f64| {
            let (x, y) = (uv.x + t * h * du, uv.y + t * h * dv);
            x >= lo.x - slack && x <= hi.x + slack && y >= lo.y - slack && y <= hi.y + slack
        };
        let central = [-2.0, -1.0, 0.0, 1.0, 2.0];
        let forward = [0.0, 1.0, 2.0, 3.0, 4.0];
        let backward = [0.0, -1.0, -2.0, -3.0, -4.0];
        let pick_offsets = |h: f64| -> Option<[f64; 5]> {
            [central, forward, backward]
                .into_iter()
                .find(|o| o.iter().all(|t| on(*t, h)))
        };
        let mut step = h;
        let mut chosen = pick_offsets(step);
        for _ in 0..8 {
            if chosen.is_some() {
                break;
            }
            step *= 0.25;
            chosen = pick_offsets(step);
        }
        let offsets = chosen.expect("SolidCraft's samples fit at some step");
        let q: Vec<Vec3> = offsets
            .iter()
            .map(|t| {
                surf.evaluate_point(&k, Point2::new(uv.x + t * step * du, uv.y + t * step * dv))
                    .unwrap()
            })
            .collect();
        let h = step;
        let (d1, d2) = if offsets[0] == 0.0 && offsets[1] < 0.0 {
            (
                (q[0] * -25.0 + q[1] * 48.0 + q[2] * -36.0 + q[3] * 16.0 + q[4] * -3.0)
                    * (-1.0 / (12.0 * h)),
                (q[0] * 35.0 + q[1] * -104.0 + q[2] * 114.0 + q[3] * -56.0 + q[4] * 11.0)
                    * (1.0 / (12.0 * h * h)),
            )
        } else if offsets[0] == 0.0 {
            (
                (q[0] * -25.0 + q[1] * 48.0 + q[2] * -36.0 + q[3] * 16.0 + q[4] * -3.0)
                    * (1.0 / (12.0 * h)),
                (q[0] * 35.0 + q[1] * -104.0 + q[2] * 114.0 + q[3] * -56.0 + q[4] * 11.0)
                    * (1.0 / (12.0 * h * h)),
            )
        } else {
            (
                (q[0] + q[1] * -8.0 + q[3] * 8.0 + q[4] * -1.0) * (1.0 / (12.0 * h)),
                (q[0] * -1.0 + q[1] * 16.0 + q[2] * -30.0 + q[3] * 16.0 + q[4] * -1.0)
                    * (1.0 / (12.0 * h * h)),
            )
        };
        let sigma = d1.length();
        let t = d1 * (1.0 / sigma);
        let bend = (d2 - t * t.dot(d2)) * (1.0 / (sigma * sigma));
        worst = worst.max(bend.distance(f.bend));
    }
    println!(
        "RES stencil: SolidCraft's bend differs from the closed form by at most {worst:e} (h {h})"
    );
    // the one-sided second-derivative stencil's truncation, O(h^3), is about 1e-9 here
    assert!(worst <= 1e-8, "{worst}");
}

/// The refusals the query names.
#[test]
fn malformed_frame_queries_are_refused() {
    let mut k = Kernel::default();
    let b = create_box(&mut k, Point3::ZERO, 20.0, 10.0, 5.0).unwrap();
    let top_front = pick(&k, &b, [10.0, 0.0, 5.0]);
    let bottom_back = pick(&k, &b, [10.0, 10.0, 0.0]);
    let invalid = |r: solidcore::Result<Vec<_>>, why: &str| match r {
        Err(SolidError::InvalidInput { reason, .. }) => {
            assert!(reason.starts_with(why), "{reason:?} is not {why:?}");
        }
        other => panic!("{other:?}: expected {why:?}"),
    };
    invalid(
        section_frames(&k, &b, &[top_front], Vec3::Y, &opts(1)),
        "a section's frames take 2 to 257 stations",
    );
    invalid(
        section_frames(&k, &b, &[top_front, bottom_back], Vec3::Y, &opts(5)),
        "a chain whose edges do not join end to start",
    );
    invalid(
        section_frames(&k, &b, &[], Vec3::Y, &opts(5)),
        "an empty chain",
    );
    // halfway between the two faces at the edge: each plane keeps 0.71 of it, below 0.9
    let strict = FrameOptions {
        stations: 5,
        min_in_plane: 0.9,
    };
    invalid(
        section_frames(&k, &b, &[top_front], Vec3::new(0.0, -1.0, 1.0), &strict),
        "the leave direction lies in no face at the chain",
    );
}

/// A flat sheet's edges left outward: every frame leaves square to its edge, out of the sheet,
/// and flat, whichever way the blend is to go (one face bounds the edge, so `toward` chooses
/// nothing, even pointing back over the sheet).
#[test]
fn a_sheet_edge_is_left_outward_whatever_the_blend_s_direction() {
    let mut k = Kernel::default();
    // the sheet [0, 20] x [0, 10] in z = 0
    let mut b = create_plane(&mut k, Point3::ZERO, 20.0, 10.0).unwrap();
    let front = pick(&k, &b, [10.0, 0.0, 0.0]);
    let right = pick(&k, &b, [20.0, 5.0, 0.0]);
    for toward in [
        Vec3::Y,
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::Z,
        Vec3::new(1.0, 1.0, -1.0),
    ] {
        // each edge, its outward direction, and the point of the edge's line nearest a station
        for (edge, out, on_edge) in [
            (front, Vec3::new(0.0, -1.0, 0.0), Point3::new(0.0, 0.0, 0.0)),
            (right, Vec3::X, Point3::new(20.0, 0.0, 0.0)),
        ] {
            let frames = section_frames_outward(&k, &mut b, &[edge], toward, 5).unwrap();
            assert_eq!(frames.len(), 5);
            for f in &frames {
                // the station lies on the edge's line: no part along the outward direction off it
                let off = (f.p - on_edge).dot(out).abs();
                assert!(off <= 1e-12 && f.p.z.abs() <= 1e-12, "{:?}", f.p);
                assert!(near(f.leave, out, 1e-12), "{toward:?}: {:?}", f.leave);
                assert!(f.bend.length() <= 1e-12 && f.bend_change.length() <= 1e-12);
            }
        }
    }
}

/// A box's top front edge bounds two faces: `toward` breaks the tie. Leaving towards -y the top
/// face is left (out across the edge, -y); leaving upwards the front face is (out across the
/// edge, +z).
#[test]
fn between_two_faces_the_blend_s_direction_picks_the_face() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::ZERO, 20.0, 10.0, 5.0).unwrap();
    let e = pick(&k, &b, [10.0, 0.0, 5.0]);
    for (toward, out) in [
        (Vec3::new(0.0, -1.0, 0.1), Vec3::new(0.0, -1.0, 0.0)),
        (Vec3::new(0.0, 0.1, 1.0), Vec3::Z),
    ] {
        let frames = section_frames_outward(&k, &mut b, &[e], toward, 5).unwrap();
        for f in &frames {
            assert!(near(f.leave, out, 1e-12), "{toward:?}: {:?}", f.leave);
        }
    }
}

/// A cylinder's top rim left outward up its wall's continuation: straight up at every station,
/// with no bend (the wall's generators are straight), as `section_frames` reads the wall left
/// along its axis.
#[test]
fn a_cylinder_rim_left_outward_up_its_wall_is_straight() {
    let mut k = Kernel::default();
    let mut b = create_cylinder(&mut k, Point3::ZERO, 5.0, 10.0).unwrap();
    let rim = pick(&k, &b, [0.0, 5.0, 10.0]);
    let frames = section_frames_outward(&k, &mut b, &[rim], Vec3::Z, 9).unwrap();
    assert_eq!(frames.len(), 9);
    for f in &frames {
        assert!((f.p.z - 10.0).abs() <= 1e-9 && (f.p.x.hypot(f.p.y) - 5.0).abs() <= 1e-9);
        assert!(near(f.leave, Vec3::Z, 1e-9), "{:?}", f.leave);
        assert!(f.bend.length() <= 1e-9, "{:?}", f.bend);
        assert!(f.bend_change.length() <= 1e-9, "{:?}", f.bend_change);
    }
}

/// The refusals of the outward frames.
#[test]
fn malformed_outward_frame_queries_are_refused() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::ZERO, 20.0, 10.0, 5.0).unwrap();
    let e = pick(&k, &b, [10.0, 0.0, 5.0]);
    let invalid = |r: solidcore::Result<Vec<_>>, why: &str| match r {
        Err(SolidError::InvalidInput { reason, .. }) => {
            assert!(reason.starts_with(why), "{reason:?} is not {why:?}");
        }
        other => panic!("{other:?}: expected {why:?}"),
    };
    invalid(
        section_frames_outward(&k, &mut b, &[e], Vec3::Y, 1),
        "a section's frames take 2 to 257 stations",
    );
    invalid(
        section_frames_outward(&k, &mut b, &[e], Vec3::new(f64::NAN, 0.0, 0.0), 5),
        "a direction towards the blend that is not finite",
    );
    invalid(
        section_frames_outward(&k, &mut b, &[e], Vec3::new(0.0, 0.0, 0.0), 5),
        "a direction towards the blend of no length",
    );
    invalid(
        section_frames_outward(&k, &mut b, &[], Vec3::Y, 5),
        "an empty chain",
    );
}

/// A chain of three legs, 3, 4 and 12 long (19 in all), at 20 stations: one a unit of arc
/// length apart, so the corners are stations 3 and 7 and the rest lie on the legs at whole
/// lengths. Every frame leaves as asked, with no bend. Drawn the other way round, the stations
/// are the same points in the opposite order.
#[test]
fn a_chain_of_points_is_read_at_even_arc_length() {
    let pts = [
        Point3::ZERO,
        Point3::new(3.0, 0.0, 0.0),
        Point3::new(3.0, 4.0, 0.0),
        Point3::new(3.0, 4.0, 12.0),
    ];
    let leave = Vec3::new(0.0, 0.0, 2.5);
    let frames = point_frames(&pts, leave, 20).unwrap();
    assert_eq!(frames.len(), 20);
    let at = |s: f64| {
        if s <= 3.0 {
            Point3::new(s, 0.0, 0.0)
        } else if s <= 7.0 {
            Point3::new(3.0, s - 3.0, 0.0)
        } else {
            Point3::new(3.0, 4.0, s - 7.0)
        }
    };
    for (i, f) in frames.iter().enumerate() {
        assert!(near(f.p, at(i as f64), 1e-12), "station {i}: {:?}", f.p);
        assert_eq!(f.leave, leave);
        assert_eq!((f.bend, f.bend_change), (Vec3::ZERO, Vec3::ZERO));
    }
    assert_eq!((frames[0].p, frames[19].p), (pts[0], pts[3]));
    let mut back = pts;
    back.reverse();
    let turned = point_frames(&back, leave, 20).unwrap();
    for (a, b) in frames.iter().zip(turned.iter().rev()) {
        assert!(near(a.p, b.p, 1e-12), "{:?} {:?}", a.p, b.p);
    }
}

/// A point repeating the one before it adds no length, so it moves no station; two stations
/// are the chain's ends.
#[test]
fn a_repeated_point_moves_no_station() {
    let pts = [
        Point3::ZERO,
        Point3::new(3.0, 0.0, 0.0),
        Point3::new(3.0, 0.0, 0.0),
        Point3::new(3.0, 4.0, 0.0),
    ];
    let once = [pts[0], pts[1], pts[3]];
    let a = point_frames(&pts, Vec3::Z, 8).unwrap();
    let b = point_frames(&once, Vec3::Z, 8).unwrap();
    assert_eq!(a, b);
    let ends = point_frames(&pts, Vec3::Z, 2).unwrap();
    assert_eq!(
        ends.iter().map(|f| f.p).collect::<Vec<_>>(),
        vec![pts[0], pts[3]]
    );
}

/// The refusals of a chain of points.
#[test]
fn malformed_point_chains_are_refused() {
    let invalid = |r: solidcore::Result<Vec<_>>, why: &str| match r {
        Err(SolidError::InvalidInput { reason, .. }) => {
            assert!(reason.starts_with(why), "{reason:?} is not {why:?}");
        }
        other => panic!("{other:?}: expected {why:?}"),
    };
    let line = [Point3::ZERO, Point3::new(1.0, 0.0, 0.0)];
    invalid(
        point_frames(&line, Vec3::Z, 1),
        "a section's frames take 2 to 257 stations",
    );
    invalid(
        point_frames(&line, Vec3::Z, 258),
        "a section's frames take 2 to 257 stations",
    );
    invalid(
        point_frames(&line, Vec3::new(f64::NAN, 0.0, 0.0), 5),
        "a leave direction that is not finite",
    );
    invalid(
        point_frames(
            &[Point3::ZERO, Point3::new(f64::INFINITY, 0.0, 0.0)],
            Vec3::Z,
            5,
        ),
        "a point that is not finite",
    );
    invalid(point_frames(&[], Vec3::Z, 5), "a chain of no length");
    invalid(
        point_frames(&[Point3::ZERO], Vec3::Z, 5),
        "a chain of no length",
    );
    invalid(
        point_frames(&[Point3::ZERO, Point3::ZERO], Vec3::Z, 5),
        "a chain of no length",
    );
}

/// Four open sections up a ladder, the second and fourth drawn back: each of those is turned,
/// the third (drawn as the first, so as the second once turned) is not. A section square to the
/// one before, its ends as far from either end, is a tie and is kept.
#[test]
fn open_sections_are_turned_to_run_as_the_one_before() {
    let k = Kernel::default();
    let rung = |x0: f64, x1: f64, z: f64| [Point3::new(x0, 0.0, z), Point3::new(x1, 0.0, z)];
    let ladder = [
        rung(0.0, 10.0, 0.0),
        rung(10.0, 0.0, 5.0),
        rung(0.0, 10.0, 10.0),
        rung(10.0, 0.0, 15.0),
    ];
    assert_eq!(
        open_section_turns(&k, &ladder).unwrap(),
        vec![false, true, false, true]
    );
    let across = [
        rung(-5.0, 5.0, 0.0),
        [Point3::new(0.0, -5.0, 5.0), Point3::new(0.0, 5.0, 5.0)],
    ];
    assert_eq!(open_section_turns(&k, &across).unwrap(), vec![false, false]);
    assert_eq!(open_section_turns(&k, &[]).unwrap(), Vec::<bool>::new());
}

/// A closed section (its ends within the kernel's zone tolerance, 1e-5) is not supported, and a
/// point not finite is refused; ends 1e-4 apart are an open section.
#[test]
fn closed_or_malformed_sections_are_not_turned() {
    let k = Kernel::default();
    let invalid = |r: solidcore::Result<Vec<bool>>, why: &str| match r {
        Err(SolidError::InvalidInput { reason, .. } | SolidError::Unsupported { reason, .. }) => {
            assert!(reason.starts_with(why), "{reason:?} is not {why:?}");
        }
        other => panic!("{other:?}: expected {why:?}"),
    };
    assert!(matches!(
        open_section_turns(&k, &[[Point3::ZERO, Point3::ZERO]]),
        Err(SolidError::Unsupported { .. })
    ));
    let open = [Point3::ZERO, Point3::new(10.0, 0.0, 0.0)];
    let near_shut = [Point3::ZERO, Point3::new(1e-6, 0.0, 0.0)];
    invalid(
        open_section_turns(&k, &[open, near_shut]),
        "a closed section",
    );
    invalid(
        open_section_turns(&k, &[[Point3::ZERO, Point3::ZERO]]),
        "a closed section",
    );
    invalid(
        open_section_turns(&k, &[[Point3::ZERO, Point3::new(f64::NAN, 0.0, 0.0)]]),
        "a section end that is not finite",
    );
    let barely_open = [Point3::ZERO, Point3::new(1e-4, 0.0, 0.0)];
    assert_eq!(
        open_section_turns(&k, &[open, barely_open]).unwrap(),
        vec![false, false]
    );
}

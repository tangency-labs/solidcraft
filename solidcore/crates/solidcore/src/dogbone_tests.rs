use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI};

use super::*;

const R: f64 = 1.5;

fn opts(style: Style) -> Options {
    Options {
        radius: R,
        style,
        max_angle: PI,
    }
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn close3(a: Vec3, b: Vec3) -> bool {
    close(a.x, b.x, 1e-12) && close(a.y, b.y, 1e-12) && close(a.z, b.z, 1e-12)
}

/// The unit wall directions of a corner of angle `theta`: `a` along +x, `b` turned `theta` from it.
fn walls(theta: f64) -> (Vec3, Vec3) {
    (Vec3::X, Vec3::new(theta.cos(), theta.sin(), 0.0))
}

/// How far `c` lies from the line of the wall along unit `d` through the corner (the origin): a
/// relief circle of radius `R` about `c` cuts into that wall's line where this is less than `R`,
/// and touches it where it is `R`.
fn off_wall(c: Vec3, d: Vec3) -> f64 {
    c.cross(d).length()
}

fn wedge(theta: f64, style: Style) -> Wedge {
    let (a, b) = walls(theta);
    wedge_relief(a, b, 10.0, 5.0, &opts(style))
        .unwrap()
        .unwrap()
}

/// The area a relief of `options` removes beyond the walls of a corner of angle `angle`
/// (radians), in closed form: what a full-depth relief cuts away is this times its depth. `None`
/// for an angle out of (0, π) and for a T-bone sharper than 90° (its bite then crosses both
/// walls and has no short closed form here).
///
/// - Normal: two circular segments, each cut off by a wall at `d = r sin(θ/2)` from the centre,
///   `r² acos(d/r) − d √(r² − d²)` each, which sum to `r² (π − θ − sin θ)`; they meet only at
///   the corner.
/// - Minimal: the same two segments with `d = (r + gap) sin(θ/2)` (none where `d >= r`); the
///   circle misses the corner, so they don't meet.
/// - T-bone at θ ≥ 90°: the centre lies on the bitten wall's line, so half the disc lies beyond
///   it, `π r² / 2`; the other half stays inside the corner.
fn removed_area(angle: f64, options: &Options) -> Option<f64> {
    let r = options.radius;
    if !(angle.is_finite() && angle > 0.0 && angle < PI) {
        return None;
    }
    let segment = |d: f64| {
        if d >= r {
            0.0
        } else {
            r * r * (d / r).acos() - d * (r * r - d * d).sqrt()
        }
    };
    match options.style {
        Style::Normal => Some(r * r * (PI - angle - angle.sin())),
        Style::Minimal { gap } => Some(2.0 * segment((r + gap) * (angle / 2.0).sin())),
        Style::TBone { .. } if angle >= FRAC_PI_2 - ANGLE_SLACK => Some(PI * r * r / 2.0),
        Style::TBone { .. } => None,
    }
}

/// The area of the disc about `c` (radius `r`) outside the wedge of angle `theta` at the origin
/// between +x and the direction at `theta`, by a fine midpoint grid: an independent check of
/// [`removed_area`].
fn grid_area(c: Vec3, r: f64, theta: f64) -> f64 {
    let n = 1000u32;
    let h = 2.0 * r / f64::from(n);
    let mut inside_disc_outside_wedge = 0usize;
    for i in 0..n {
        for j in 0..n {
            let x = c.x - r + (f64::from(i) + 0.5) * h;
            let y = c.y - r + (f64::from(j) + 0.5) * h;
            if (x - c.x).powi(2) + (y - c.y).powi(2) > r * r {
                continue;
            }
            let ang = y.atan2(x);
            let in_wedge = ang >= 0.0 && ang <= theta;
            if !in_wedge {
                inside_disc_outside_wedge += 1;
            }
        }
    }
    inside_disc_outside_wedge as f64 * h * h
}

#[test]
fn a_normal_relief_of_a_square_corner_sits_on_the_bisector_through_the_corner() {
    let w = wedge(FRAC_PI_2, Style::Normal);
    assert!(close3(
        w.centre,
        Vec3::new(R * FRAC_1_SQRT_2, R * FRAC_1_SQRT_2, 0.0)
    ));
    // The circle passes through the corner.
    assert!(close(w.centre.length(), R, 1e-12));
    // (π/2 − 1) r² beyond the walls.
    assert!(close(
        removed_area(FRAC_PI_2, &opts(Style::Normal)).unwrap(),
        (FRAC_PI_2 - 1.0) * R * R,
        1e-14
    ));
}

#[test]
fn normal_reliefs_at_60_and_120_degrees_follow_the_bisector() {
    for theta in [PI / 3.0, 2.0 * PI / 3.0] {
        let w = wedge(theta, Style::Normal);
        let half = theta / 2.0;
        assert!(
            close3(w.centre, Vec3::new(R * half.cos(), R * half.sin(), 0.0)),
            "{theta}: {:?}",
            w.centre
        );
        // Through the corner.
        assert!(close(w.centre.length(), R, 1e-12));
    }
    // r² (π − θ − sin θ): 60° → (2π/3 − √3/2) r², 120° → (π/3 − √3/2) r².
    let s3 = 3f64.sqrt() / 2.0;
    assert!(close(
        removed_area(PI / 3.0, &opts(Style::Normal)).unwrap(),
        (2.0 * PI / 3.0 - s3) * R * R,
        1e-13
    ));
    assert!(close(
        removed_area(2.0 * PI / 3.0, &opts(Style::Normal)).unwrap(),
        (PI / 3.0 - s3) * R * R,
        1e-13
    ));
}

#[test]
fn the_closed_form_areas_match_a_grid_count() {
    for theta in [PI / 3.0, FRAC_PI_2, 2.0 * PI / 3.0, 0.8 * PI] {
        for style in [
            Style::Normal,
            Style::Minimal { gap: 0.2 },
            Style::TBone { side: Side::Longer },
        ] {
            let Some(exact) = removed_area(theta, &opts(style)) else {
                assert!(matches!(style, Style::TBone { .. }) && theta < FRAC_PI_2);
                continue;
            };
            if exact == 0.0 {
                // A minimal circle that reaches no wall at this angle: no relief.
                let (a, b) = walls(theta);
                assert_eq!(
                    wedge_relief(a, b, 10.0, 5.0, &opts(style)).unwrap(),
                    Err(Skip::Unreached)
                );
                continue;
            }
            let w = wedge(theta, style);
            let counted = grid_area(w.centre, R, theta);
            // The grid's error is about the disc's perimeter times a cell.
            assert!(
                close(counted, exact, 2.0 * PI * R * (2.0 * R / 1000.0)),
                "{theta} {style:?}: {counted} vs {exact}"
            );
        }
    }
}

#[test]
fn the_normal_relief_removes_the_least_of_any_circle_through_the_corner() {
    // r² (π − θ − (sin 2φ + sin(2θ − 2φ))/2) for a centre at angle φ from wall a: least at θ/2.
    let theta = FRAC_PI_2;
    let normal = removed_area(theta, &opts(Style::Normal)).unwrap();
    for phi in [0.1, 0.3, 0.6, 1.0, 1.3] {
        let c = Vec3::new(R * f64::cos(phi), R * f64::sin(phi), 0.0);
        assert!(grid_area(c, R, theta) > normal, "{phi}");
    }
}

#[test]
fn a_minimal_relief_stops_its_gap_short_of_the_corner() {
    let gap = 0.15;
    let w = wedge(FRAC_PI_2, Style::Minimal { gap });
    let s = R + gap;
    assert!(close3(
        w.centre,
        Vec3::new(s * FRAC_1_SQRT_2, s * FRAC_1_SQRT_2, 0.0)
    ));
    // It still cuts into both walls: its centre is s/√2 from each wall's line, less than r.
    let (a, b) = walls(FRAC_PI_2);
    for d in [a, b] {
        assert!(close(off_wall(w.centre, d), s * FRAC_1_SQRT_2, 1e-12));
    }
    assert!(s * FRAC_1_SQRT_2 < R);
    // The nearest point of the circle to the corner is `gap` from it.
    assert!(close(w.centre.length() - R, gap, 1e-12));
    // Gap 0 is the normal relief.
    assert_eq!(
        wedge(FRAC_PI_2, Style::Minimal { gap: 0.0 }).centre,
        wedge(FRAC_PI_2, Style::Normal).centre
    );
    // A gap so large the circle reaches no wall: no relief.
    assert_eq!(
        wedge_relief(a, b, 1.0, 1.0, &opts(Style::Minimal { gap: R })).unwrap(),
        Err(Skip::Unreached)
    );
}

#[test]
fn a_t_bone_bites_the_wall_it_is_told_to() {
    // Wall a is 10 long, b 5: Longer takes a, Shorter b. The centre on the bitten wall's line,
    // r from the corner: the circle cuts that wall from the corner to 2 r along it.
    let (a, b) = walls(FRAC_PI_2);
    let long = wedge(FRAC_PI_2, Style::TBone { side: Side::Longer });
    assert!(close3(long.centre, Vec3::new(R, 0.0, 0.0)));
    assert!(close(off_wall(long.centre, a), 0.0, 1e-12));
    // Tangent to the other wall at 90°: no bite there.
    assert!(close(off_wall(long.centre, b), R, 1e-12));
    let short = wedge(
        FRAC_PI_2,
        Style::TBone {
            side: Side::Shorter,
        },
    );
    assert!(close3(short.centre, Vec3::new(0.0, R, 0.0)));
    assert!(close(off_wall(short.centre, a), R, 1e-12));
    assert!(close(off_wall(short.centre, b), 0.0, 1e-12));
    // Equal walls: Longer takes the first, Shorter the second.
    let eq = |side| {
        wedge_relief(a, b, 3.0, 3.0, &opts(Style::TBone { side }))
            .unwrap()
            .unwrap()
            .centre
    };
    assert!(close3(eq(Side::Longer), Vec3::new(R, 0.0, 0.0)));
    assert!(close3(eq(Side::Shorter), Vec3::new(0.0, R, 0.0)));
    // At 60° the circle also cuts the other wall: through the corner, it crosses that wall's
    // line again at 2 (c · b) = 2 r cos θ along it.
    let (_, b) = walls(PI / 3.0);
    let sharp = wedge(PI / 3.0, Style::TBone { side: Side::Longer });
    assert!(off_wall(sharp.centre, b) < R);
    assert!(close(
        2.0 * sharp.centre.dot(b),
        2.0 * R * (PI / 3.0).cos(),
        1e-12
    ));
    assert_eq!(
        removed_area(FRAC_PI_2, &opts(Style::TBone { side: Side::Longer })),
        Some(PI * R * R / 2.0)
    );
}

#[test]
fn corners_more_open_than_the_largest_angle_are_skipped() {
    let (a, b) = walls(FRAC_PI_2 + 0.01);
    let square_only = Options {
        radius: R,
        style: Style::Normal,
        max_angle: FRAC_PI_2,
    };
    assert_eq!(
        wedge_relief(a, b, 1.0, 1.0, &square_only).unwrap(),
        Err(Skip::TooOpen)
    );
    // A square corner computed with rounding is still relieved.
    let (a, b) = walls(FRAC_PI_2 + 1e-9);
    assert!(wedge_relief(a, b, 1.0, 1.0, &square_only).unwrap().is_ok());
}

/// A face along an edge for [`edge_corner`] and [`edge_relief`], with no sides.
fn face(outward: Vec3, left: bool) -> Wall<'static> {
    Wall {
        outward,
        left,
        sides: &[],
    }
}

/// An end of an edge with no other face.
const NO_FACES: &[Option<Vec3>] = &[];

/// The edge from `start` to `end`.
fn edge(start: Vec3, end: Vec3) -> Edge {
    Edge { start, end }
}

#[test]
fn hostile_options_and_points_are_refused() {
    let (a, b) = walls(FRAC_PI_2);
    let up = edge(Vec3::ZERO, Vec3::Z);
    let pocket = [face(Vec3::Y, true), face(Vec3::X, false)];
    for radius in [0.0, -1.0, f64::NAN, f64::INFINITY, 2e6] {
        let o = Options {
            radius,
            style: Style::Normal,
            max_angle: PI,
        };
        assert!(wedge_relief(a, b, 1.0, 1.0, &o).is_err(), "{radius}");
        assert!(edge_relief(&up, &pocket, [NO_FACES, NO_FACES], &o).is_err());
    }
    for gap in [-0.1, f64::NAN, f64::INFINITY] {
        assert!(opts(Style::Minimal { gap }).checked().is_err());
    }
    for max_angle in [0.0, -1.0, 4.0, f64::NAN] {
        assert!(Options {
            radius: 1.0,
            style: Style::Normal,
            max_angle
        }
        .checked()
        .is_err());
    }
    assert!(wedge_relief(
        Vec3::new(f64::NAN, 0.0, 0.0),
        b,
        1.0,
        1.0,
        &opts(Style::Normal)
    )
    .is_err());
    assert!(wedge_relief(a, b, f64::NAN, 1.0, &opts(Style::Normal)).is_err());
    assert!(edge_corner(
        &edge(Vec3::ZERO, Vec3::ZERO),
        &[face(Vec3::X, true), face(Vec3::X, true)]
    )
    .is_err());
    assert!(edge_corner(&up, &[face(Vec3::ZERO, true), face(Vec3::ZERO, true)]).is_err());
    assert!(edge_corner(
        &edge(Vec3::ZERO, Vec3::new(0.0, 0.0, f64::NAN)),
        &[face(Vec3::X, true), face(Vec3::X, true)]
    )
    .is_err());
    let w = [Vec3::X, Vec3::Y];
    assert!(relief_cylinder(&up, w, [1.0; 2], [-1.0, 0.0], &opts(Style::Normal)).is_err());
    assert!(relief_cylinder(
        &edge(Vec3::ZERO, Vec3::ZERO),
        w,
        [1.0; 2],
        [0.0; 2],
        &opts(Style::Normal)
    )
    .is_err());
    assert!(end_is_open(Vec3::ZERO, Vec3::ZERO, Vec3::Z).is_err());
    assert!(end_is_open(Vec3::ZERO, Vec3::Z, Vec3::ZERO).is_err());
    assert!(up.lean(Vec3::ZERO).is_err());
    assert!(up.lean(Vec3::new(f64::NAN, 0.0, 1.0)).is_err());
    assert!(edge(Vec3::Z, Vec3::Z).lean(Vec3::Z).is_err());
    let in_line = Surface::Plane {
        origin: [0.0; 3],
        u_axis: [1.0, 0.0, 0.0],
        v_axis: [2.0, 0.0, 0.0],
    };
    assert_eq!(outward(&in_line, true), None);
    let sides = [edge(Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 0.0))];
    assert!(wall_length(&sides).is_err());
}

#[test]
fn a_pocket_wall_edge_is_concave_and_a_box_edge_convex() {
    // A pocket's corner on the z axis, running up (t = +z): wall y = 0 (normal +y into the
    // pocket) runs along +x, wall x = 0 (normal +x) along +y. A face is on the edge's left when
    // n × t points into it.
    let up = edge(Vec3::ZERO, Vec3::new(0.0, 0.0, 6.0));
    // n × t for the wall y = 0: (0,1,0) × (0,0,1) = (1,0,0): into that face, so it is on the left.
    let wy = face(Vec3::Y, true);
    // (1,0,0) × (0,0,1) = (0,-1,0): away from the wall x = 0, which lies along +y: on the right.
    let wx = face(Vec3::X, false);
    match edge_corner(&up, &[wy, wx]).unwrap() {
        EdgeCorner::Concave { walls } => {
            assert!(close3(walls[0], Vec3::X) && close3(walls[1], Vec3::Y));
            assert!(close(
                walls[0].angle_between(walls[1]).unwrap(),
                FRAC_PI_2,
                1e-15
            ));
        }
        c => panic!("{c:?}"),
    }
    // A box's edge on the z axis, the box in x < 0, y < 0: face y = 0 (normal +y) lies along -x
    // (on the right of +z seen from +y), face x = 0 (normal +x) along -y (on the left).
    let by = face(Vec3::Y, false);
    let bx = face(Vec3::X, true);
    assert_eq!(edge_corner(&up, &[by, bx]).unwrap(), EdgeCorner::Convex);
    // Two faces in one plane: smooth.
    let flat_a = face(Vec3::Y, true);
    let flat_b = face(Vec3::Y, false);
    assert_eq!(
        edge_corner(&up, &[flat_a, flat_b]).unwrap(),
        EdgeCorner::Smooth
    );
}

#[test]
fn a_relief_cylinder_runs_the_edge_and_its_extensions() {
    let walls = [Vec3::X, Vec3::Y];
    let c = relief_cylinder(
        &edge(Vec3::new(2.0, 3.0, 4.0), Vec3::new(2.0, 3.0, 10.0)),
        walls,
        [5.0, 5.0],
        [0.0, 1.5],
        &opts(Style::Normal),
    )
    .unwrap()
    .unwrap();
    let d = R * FRAC_1_SQRT_2;
    assert!(close3(c.base, Vec3::new(2.0 + d, 3.0 + d, 4.0)));
    assert!(close3(c.axis, Vec3::Z));
    assert_eq!((c.radius, c.height), (R, 7.5));
    // Extended below the start too, and running the other way.
    let c = relief_cylinder(
        &edge(Vec3::new(0.0, 0.0, 6.0), Vec3::ZERO),
        walls,
        [5.0, 5.0],
        [2.0, 0.0],
        &opts(Style::Normal),
    )
    .unwrap()
    .unwrap();
    assert!(close3(c.base, Vec3::new(d, d, 8.0)));
    assert!(close3(c.axis, -Vec3::Z));
    assert_eq!(c.height, 8.0);
    // Skipped like a wedge.
    let open = Options {
        radius: R,
        style: Style::Normal,
        max_angle: 1.0,
    };
    assert_eq!(
        relief_cylinder(&edge(Vec3::ZERO, Vec3::Z), walls, [1.0; 2], [0.0; 2], &open).unwrap(),
        Err(Skip::TooOpen)
    );
}

#[test]
fn ends_axes_and_planes() {
    // A floor facing up at the bottom of an edge running up: blind; the top face: open.
    let (top, floor) = (Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, 4.0));
    assert!(!end_is_open(top, floor, Vec3::Z).unwrap());
    assert!(end_is_open(floor, top, Vec3::Z).unwrap());
    assert!(end_is_open(top, Vec3::Z, Vec3::new(0.0, 0.3, -1.0)).unwrap());
    assert!(!end_is_open(floor, top, Vec3::X).unwrap());
    // An edge along the axis either way leans 0 from it; one off by 0.01 leans that much.
    assert!(
        edge(Vec3::ZERO, Vec3::new(0.0, 0.0, -2.0))
            .lean(Vec3::new(0.0, 0.0, 5.0))
            .unwrap()
            <= 1e-9
    );
    assert!(
        edge(Vec3::ZERO, Vec3::new(0.0, 0.01, 1.0))
            .lean(Vec3::Z)
            .unwrap()
            > 1e-3
    );
    assert!(
        edge(Vec3::ZERO, Vec3::new(0.0, 0.0001, 1.0))
            .lean(Vec3::Z)
            .unwrap()
            <= 1e-3
    );
    let plane = Surface::Plane {
        origin: [0.0; 3],
        u_axis: [2.0, 0.0, 0.0],
        v_axis: [0.0, 3.0, 0.0],
    };
    assert!(close3(outward(&plane, true).unwrap(), Vec3::Z));
    // Used against its natural normal by the shell: the other way.
    assert!(close3(outward(&plane, false).unwrap(), -Vec3::Z));
    let through = Surface::PlaneThrough {
        origin: [1.0, 1.0, 1.0],
        u_point: [1.0, 2.0, 1.0],
        v_point: [2.0, 1.0, 1.0],
    };
    assert!(close3(outward(&through, true).unwrap(), -Vec3::Z));
    assert_eq!(
        edge(Vec3::ZERO, Vec3::new(2.0, 4.0, 6.0)).middle(),
        Vec3::new(1.0, 2.0, 3.0)
    );
    // A wall is as long as its longest side.
    let sides = [
        edge(Vec3::ZERO, Vec3::new(3.0, 4.0, 0.0)),
        edge(Vec3::ZERO, Vec3::new(0.0, 0.0, 2.0)),
    ];
    assert_eq!(wall_length(&sides).unwrap(), 5.0);
    assert_eq!(wall_length(&[]).unwrap(), 0.0);
}

#[test]
fn an_edge_relief_reads_its_walls_and_ends() {
    // A pocket's corner edge from its floor (z = 4) up to the plate's top (z = 10), walls y = 0
    // (20 long along x) and x = 0 (14 long along y), as `a_pocket_wall_edge_is_concave_...`.
    let up = edge(Vec3::new(0.0, 0.0, 4.0), Vec3::new(0.0, 0.0, 10.0));
    let along_x = [edge(Vec3::new(0.0, 0.0, 4.0), Vec3::new(20.0, 0.0, 4.0))];
    let along_y = [edge(Vec3::new(0.0, 0.0, 4.0), Vec3::new(0.0, 14.0, 4.0))];
    let walls = [
        Wall {
            outward: Vec3::Y,
            left: true,
            sides: &along_x,
        },
        Wall {
            outward: Vec3::X,
            left: false,
            sides: &along_y,
        },
    ];
    // The floor faces up, back along the edge: blind; the top faces up, on past it: open.
    let floor: &[Option<Vec3>] = &[Some(Vec3::Z)];
    let top: &[Option<Vec3>] = &[Some(Vec3::Z)];
    let relief =
        |style, ends: [&[Option<Vec3>]; 2]| match edge_relief(&up, &walls, ends, &opts(style))
            .unwrap()
        {
            EdgeRelief::Relief(c) => c,
            r => panic!("{r:?}"),
        };
    let c = relief(Style::Normal, [floor, top]);
    let d = R * FRAC_1_SQRT_2;
    assert!(close3(c.base, Vec3::new(d, d, 4.0)));
    assert_eq!(c.height, 6.0 + R);
    // An end with a face that is not planar, or with no other face, is blind.
    let curved: &[Option<Vec3>] = &[Some(Vec3::Z), None];
    assert_eq!(relief(Style::Normal, [floor, curved]).height, 6.0);
    assert_eq!(relief(Style::Normal, [floor, NO_FACES]).height, 6.0);
    // A T-bone bites the longer wall (along x), or the shorter one.
    let long = relief(Style::TBone { side: Side::Longer }, [floor, top]);
    assert!(close3(long.base, Vec3::new(R, 0.0, 4.0)));
    let short = relief(
        Style::TBone {
            side: Side::Shorter,
        },
        [floor, top],
    );
    assert!(close3(short.base, Vec3::new(0.0, R, 4.0)));
    // The same edge on a box's outside, and between two faces of one plane.
    let outside = [face(Vec3::Y, false), face(Vec3::X, true)];
    assert_eq!(
        edge_relief(&up, &outside, [floor, top], &opts(Style::Normal)).unwrap(),
        EdgeRelief::Skipped(Skip::Convex)
    );
    let flat = [face(Vec3::Y, true), face(Vec3::Y, false)];
    assert_eq!(
        edge_relief(&up, &flat, [floor, top], &opts(Style::Normal)).unwrap(),
        EdgeRelief::Smooth
    );
    // A corner more open than the largest angle.
    let narrow = Options {
        radius: R,
        style: Style::Normal,
        max_angle: 1.0,
    };
    assert_eq!(
        edge_relief(&up, &walls, [floor, top], &narrow).unwrap(),
        EdgeRelief::Skipped(Skip::TooOpen)
    );
}

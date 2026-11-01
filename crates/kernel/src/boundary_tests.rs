//! Tests of boundary patches: the bridge this builds is measured back off the face it made — not
//! from the construction — and everything it cannot hold is an error rather than a quiet
//! approximation.

use super::*;

/// The angle between two vectors, in degrees.
fn angle_deg(a: Vec3, b: Vec3) -> f64 {
    (a.normalized().unwrap().dot(b.normalized().unwrap())).clamp(-1.0, 1.0).acos().to_degrees()
}

/// The geometric shape of a surface curve from its first three derivatives along it: the unit
/// tangent, the curvature vector and the arclength derivative of that curvature vector — the same
/// reading `bridge` states its conditions with.
fn shape_of(d1: Vec3, d2: Vec3, d3: Vec3) -> (Vec3, Vec3, Vec3) {
    let sigma = d1.len();
    let t = d1 * (1.0 / sigma);
    let tp = d2 * (1.0 / sigma) - d1 * (d1.dot(d2) / (sigma * sigma * sigma));
    let nn = d2 - t * d2.dot(t);
    let np = d3 - t * d3.dot(t) - tp * d2.dot(t) - t * d2.dot(tp);
    let kappa = np * (sigma * sigma) - nn * (2.0 * d1.dot(d2));
    (t, nn * (1.0 / (sigma * sigma)), kappa * (1.0 / (sigma.powi(5))))
}

/// Across a boundary: five samples a step `h` apart, `into` the patch from the parameter `(u, v)`,
/// read back as the position, the unit tangent, the curvature vector and the change of curvature
/// along that inward direction.
fn across(body: &Body, uv: (f64, f64), into: (f64, f64), h: f64) -> (Vec3, Vec3, Vec3, Vec3) {
    let (surf, _) = crate::bridge::the_face(body).unwrap();
    let q: Vec<Vec3> = (0..5).map(|k| surf.point([uv.0 + k as f64 * h * into.0, uv.1 + k as f64 * h * into.1]).unwrap()).collect();
    let (f0, f1, f2, f3, f4) = (q[0], q[1], q[2], q[3], q[4]);
    let d1 = (f0 * -25.0 + f1 * 48.0 + f2 * -36.0 + f3 * 16.0 + f4 * -3.0) * (1.0 / (12.0 * h));
    let d2 = (f0 * 35.0 + f1 * -104.0 + f2 * 114.0 + f3 * -56.0 + f4 * 11.0) * (1.0 / (12.0 * h * h));
    let d3 = (f0 * -5.0 + f1 * 18.0 + f2 * -24.0 + f3 * 14.0 + f4 * -3.0) * (1.0 / (2.0 * h * h * h));
    let (t, bend, change) = shape_of(d1, d2, d3);
    (f0, t, bend, change)
}

/// The stations' parameters: `n` of them, evenly spread over the patch.
fn stations(n: usize) -> Vec<f64> {
    (0..n).map(|i| i as f64 / (n - 1) as f64).collect()
}

/// The normal angle a side may be off by.
const ANGLE_TOL: f64 = 0.01;
/// How much of the supporting wall's own curvature a side must repeat, relative to it.
const CURV_TOL: f64 = 1e-3;
/// The normal part of the change of curvature a G3 side may leave over, relative to 1/r² (the
/// finite differences that read the third derivative off the face amplify round-off by 1/h³, so
/// this is a noise floor rather than a geometric tolerance).
const CHANGE_TOL: f64 = 1e-4;

/// Measure one side of a built patch at every station of `frames`, and check what its order asks
/// for: the boundary curve itself always, the tangent plane from G1 on, the curvature vector from
/// G2 on and its change along the leave direction at G3.
fn check_side(body: &Body, frames: &[Frame], order: Continuity, at: impl Fn(usize) -> ((f64, f64), (f64, f64)), r: f64) {
    for (i, f) in frames.iter().enumerate() {
        let (uv, into) = at(i);
        let (p, t, bend, change) = across(body, uv, into, 1e-3);
        let gap = p.dist(f.p);
        assert!(gap < 1e-9, "station {i} sits on its boundary: {p:?} vs {:?} ({gap:.2e} mm)", f.p);
        if order == Continuity::G0 {
            continue;
        }
        assert!(angle_deg(t, f.leave) < ANGLE_TOL, "station {i} leaves along the tangent: {t:?} vs {:?}", f.leave);
        if order == Continuity::G1 {
            continue;
        }
        if f.bend.len() > 1e-9 {
            assert!(bend.dist(f.bend) / f.bend.len() < CURV_TOL, "station {i}: curvature {bend:?} vs the wall's {:?}", f.bend);
            if order == Continuity::G3 {
                let normal = f.bend * (1.0 / f.bend.len());
                assert!(change.dot(normal).abs() < CHANGE_TOL / (r * r), "station {i}: the change of curvature has a normal part {change:?}");
            }
        } else {
            assert!(bend.len() < CURV_TOL / r, "station {i}: the wall is straight here, the patch curves by {bend:?}");
            if order == Continuity::G3 {
                assert!(change.len() < CHANGE_TOL / (r * r), "station {i}: a straight cross-section changes curvature by {change:?}");
            }
        }
    }
}

/// Two walls 20 mm apart, each leaving towards the other, both walls of the same cylinder of radius
/// `r` (the valley `bridge` is tested with): the two sides of a two-sided patch.
fn two_walls(n: usize, r: f64) -> (Vec<Frame>, Vec<Frame>) {
    let mut a = Vec::new();
    let mut b = Vec::new();
    for t in stations(n) {
        let y = 20.0 * t;
        a.push(Frame {
            p: Vec3::new(0.0, y, 0.0),
            leave: Vec3::new(0.0, 1.0, 0.0),
            bend: Vec3::new(0.0, 0.0, 1.0 / r),
            bend_change: Vec3::new(0.0, -1.0 / (r * r), 0.0),
        });
        b.push(Frame {
            p: Vec3::new(0.0, y + 20.0, 0.0),
            leave: Vec3::new(0.0, -1.0, 0.0),
            bend: Vec3::new(0.0, 0.0, 1.0 / r),
            bend_change: Vec3::new(0.0, 1.0 / (r * r), 0.0),
        });
    }
    (a, b)
}

#[test]
fn a_two_sided_patch_is_a_bridge_that_holds_both_ends() {
    let n = 9;
    let r = 10.0;
    let (a, b) = two_walls(n, r);
    let boundaries = vec![Boundary { frames: a.clone(), order: Continuity::G2 }, Boundary { frames: b.clone(), order: Continuity::G2 }];
    let body = boundary_patch(&boundaries, 0.0, n).unwrap();
    let s = stations(n);
    check_side(&body, &a, Continuity::G2, |i| ((s[i], 0.0), (0.0, 1.0)), r);
    check_side(&body, &b, Continuity::G2, |i| ((s[i], 1.0), (0.0, -1.0)), r);
}

/// Four chained sides round a 20 × 20 square in the XY plane, each leaving into the square: the
/// loop a patch through three or four boundaries would have to close.
fn square_loop(n: usize) -> Vec<Boundary> {
    let corners = [(0.0_f64, 0.0_f64), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)];
    let leaves = [Vec3::new(0.0, 1.0, 0.0), Vec3::new(-1.0, 0.0, 0.0), Vec3::new(0.0, -1.0, 0.0), Vec3::new(1.0, 0.0, 0.0)];
    (0..4)
        .map(|side| {
            let (a, b) = (corners[side], corners[(side + 1) % 4]);
            let frames = stations(n)
                .into_iter()
                .map(|t| Frame {
                    p: Vec3::new(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, 0.0),
                    leave: leaves[side],
                    bend: Vec3::ZERO,
                    bend_change: Vec3::ZERO,
                })
                .collect();
            Boundary { frames, order: Continuity::G2 }
        })
        .collect()
}

#[test]
fn a_two_sided_patch_meets_its_walls_up_to_the_curvature() {
    let n = 11;
    let r = 10.0;
    let (a, b) = two_walls(n, r);
    // G0, G1 and G2 are what this patch states: position, tangent plane and curvature across each
    // boundary. (A G3 bridge's third-order rows are `bridge`'s own subject, measured in its tests.)
    for order in [Continuity::G0, Continuity::G1, Continuity::G2] {
        let boundaries = vec![Boundary { frames: a.clone(), order }, Boundary { frames: b.clone(), order }];
        let body = boundary_patch(&boundaries, 0.0, n).unwrap();
        let s = stations(n);
        if order == Continuity::G0 {
            for (i, f) in a.iter().enumerate() {
                let (p, _, _, _) = across(&body, (s[i], 0.0), (0.0, 1.0), 1e-3);
                assert!(p.dist(f.p) < 1e-9, "station {i}: {p:?} vs {:?}", f.p);
            }
            continue;
        }
        check_side(&body, &a, order, |i| ((s[i], 0.0), (0.0, 1.0)), r);
        check_side(&body, &b, order, |i| ((s[i], 1.0), (0.0, -1.0)), r);
    }
}

#[test]
fn a_crown_shapes_the_middle_without_touching_the_boundary_rows() {
    let n = 9;
    let r = 10.0;
    let (a, b) = two_walls(n, r);
    let boundaries = vec![Boundary { frames: a.clone(), order: Continuity::G2 }, Boundary { frames: b.clone(), order: Continuity::G2 }];
    let s = stations(n);
    let mut middles = Vec::new();
    for crown in [-0.5, 0.0, 1.0, 2.0] {
        let body = boundary_patch(&boundaries, crown, n).unwrap();
        let grid = crate::bridge::sample_grid(&body, 9, 9).unwrap();
        middles.push(grid[4][4].z);
        // The interior is the cross-section between the two walls: its middle sits in the gap, not
        // outside it, and it stays finite everywhere on the grid.
        for row in &grid {
            for p in row {
                assert!(p.is_finite() && (-2.0..=42.0).contains(&p.y), "the interior stays between the walls: {p:?}");
            }
        }
        assert!((18.0..=22.0).contains(&grid[4][4].y), "the middle of the patch sits midway in the 20 mm band: {:?}", grid[4][4]);
        // The conditions are untouched: the same boundary points, tangent and curvature.
        check_side(&body, &a, Continuity::G2, |i| ((s[i], 0.0), (0.0, 1.0)), r);
        check_side(&body, &b, Continuity::G2, |i| ((s[i], 1.0), (0.0, -1.0)), r);
    }
    let steps: Vec<f64> = middles.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(steps.iter().all(|d| d.abs() > 1e-9), "a fuller crown moves the middle: {middles:?}");
}

#[test]
fn the_patch_refuses_what_it_cannot_hold() {
    let n = 9;
    let (a, b) = two_walls(n, 10.0);
    let two = vec![Boundary { frames: a.clone(), order: Continuity::G2 }, Boundary { frames: b.clone(), order: Continuity::G2 }];
    // Not 2 to 4 boundaries.
    assert!(boundary_patch(&[], 0.0, n).is_err());
    assert!(boundary_patch(&two[..1], 0.0, n).is_err());
    let five = vec![two[0].clone(), two[1].clone(), two[0].clone(), two[1].clone(), two[0].clone()];
    assert!(boundary_patch(&five, 0.0, n).is_err());
    // Three or four sides are refused: a tensor-product patch cannot hold two sides' data at a
    // corner, which needs a Gregory-type corner (issue #763). The message says so.
    let square = square_loop(n);
    let e = boundary_patch(&square, 0.0, n).unwrap_err().to_string();
    assert!(e.contains("not supported yet") && e.contains("#763"), "4 sides: {e}");
    // Three sides that do close: a triangle, whose last side runs from the second one's end back to
    // the first one's start.
    let triangle: Vec<Boundary> = vec![
        square[0].clone(),
        square[1].clone(),
        Boundary {
            frames: stations(n)
                .into_iter()
                .map(|t| Frame {
                    p: Vec3::new(20.0 * (1.0 - t), 20.0 * (1.0 - t), 0.0),
                    leave: Vec3::new(-std::f64::consts::FRAC_1_SQRT_2, -std::f64::consts::FRAC_1_SQRT_2, 0.0),
                    bend: Vec3::ZERO,
                    bend_change: Vec3::ZERO,
                })
                .collect(),
            order: Continuity::G2,
        },
    ];
    let e = boundary_patch(&triangle, 0.0, n).unwrap_err().to_string();
    assert!(e.contains("not supported yet") && e.contains("#763"), "3 sides: {e}");
    // A loop whose sides do not even meet is refused earlier and for its own reason.
    let broken = square_loop(n);
    let mut moved = broken[1].frames.clone();
    moved[0] = Frame { p: Vec3::new(30.0, 0.0, 0.0), ..moved[0] };
    let broken = vec![broken[0].clone(), Boundary { frames: moved, order: Continuity::G2 }, broken[2].clone()];
    let e = boundary_patch(&broken, 0.0, n).unwrap_err().to_string();
    assert!(e.contains("run on into one another"), "{e}");
    // The stations and the crown.
    let short = vec![Boundary { frames: a[..n - 1].to_vec(), order: Continuity::G2 }, two[1].clone()];
    assert!(boundary_patch(&short, 0.0, n).is_err(), "a boundary with the wrong number of stations");
    assert!(boundary_patch(&two, 0.0, n - 1).is_err());
    assert!(boundary_patch(&two, 0.0, 1).is_err());
    for crown in [-1.5, 3.5, f64::NAN, f64::INFINITY] {
        assert!(boundary_patch(&two, crown, n).is_err(), "crown {crown}");
    }
    // A station that is not finite, and a station with no leave direction.
    for field in 0..4 {
        let mut bad = two.clone();
        let mut frames = bad[0].frames.clone();
        frames[3] = match field {
            0 => Frame { p: Vec3::new(f64::NAN, 0.0, 0.0), ..frames[3] },
            1 => Frame { leave: Vec3::new(f64::NAN, 0.0, 0.0), ..frames[3] },
            2 => Frame { bend: Vec3::new(f64::INFINITY, 0.0, 0.0), ..frames[3] },
            _ => Frame { bend_change: Vec3::new(f64::NAN, 0.0, 0.0), ..frames[3] },
        };
        if let Some(b) = bad.first_mut() {
            b.frames = frames;
        }
        assert!(boundary_patch(&bad, 0.0, n).is_err(), "a station that is not finite: field {field}");
    }
    let mut flat = two.clone();
    if let Some(b) = flat.first_mut() {
        b.frames = vec![Frame { p: Vec3::new(0.0, 1.0, 0.0), leave: Vec3::ZERO, bend: Vec3::ZERO, bend_change: Vec3::ZERO }; n];
    }
    assert!(boundary_patch(&flat, 0.0, n).is_err(), "a station with no leave direction");
    let point = vec![
        Boundary {
            frames: vec![Frame { p: Vec3::new(1.0, 1.0, 1.0), leave: Vec3::X, bend: Vec3::ZERO, bend_change: Vec3::ZERO }; n],
            order: Continuity::G1,
        },
        Boundary {
            frames: vec![Frame { p: Vec3::new(1.0, 1.0, 1.0), leave: Vec3::X, bend: Vec3::ZERO, bend_change: Vec3::ZERO }; n],
            order: Continuity::G1,
        },
    ];
    assert!(boundary_patch(&point, 0.0, n).is_err(), "a boundary with no length");
}

use super::*;
use crate::eval::uv::{degenerate, degenerate_within};

const MID: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// The spline of `degree` whose net is `rows[i][j]` (i along u), Euclidean points with weights,
/// on clamped knots with one span along each direction.
fn spline(degree: [usize; 2], rows: &[Vec<([f64; 3], f64)>]) -> EvalSurface {
    let (nu, nv) = (rows.len(), rows[0].len());
    let clamped = |p: usize, n: usize| {
        let mut k = vec![0.0; p + 1];
        let inner = n - p - 1;
        k.extend((1..=inner).map(|i| i as f64 / (inner + 1) as f64));
        k.extend(vec![1.0; p + 1]);
        k
    };
    let mut ctrl = Vec::new();
    for j in 0..nv {
        for row in rows {
            let ([x, y, z], w) = row[j];
            ctrl.push([w * x, w * y, w * z, w]);
        }
    }
    EvalSurface::nurbs(
        degree,
        [clamped(degree[0], nu), clamped(degree[1], nv)],
        [nu, nv],
        ctrl,
    )
}

/// The quarter turn about z of the profile `profile` ((radius, height) control points with
/// weights, degree one less than their count), scaled by `scale` and moved by `offset`, as the
/// record holds a revolution: u the profile, v the quarter arc's rational parameter.
fn turned(profile: &[([f64; 2], f64)], scale: f64, offset: [f64; 3]) -> EvalSurface {
    let arc = [([1.0, 0.0], 1.0), ([1.0, 1.0], MID), ([0.0, 1.0], 1.0)];
    let rows: Vec<Vec<([f64; 3], f64)>> = profile
        .iter()
        .map(|([r, z], wp)| {
            arc.iter()
                .map(|([ax, ay], wa)| {
                    let p = [
                        offset[0] + scale * r * ax,
                        offset[1] + scale * r * ay,
                        offset[2] + scale * z,
                    ];
                    (p, wp * wa)
                })
                .collect()
        })
        .collect();
    spline([profile.len() - 1, 2], &rows)
}

/// The cone of the comparison's face 3: the straight profile (s², s²) of a quadratic Bézier
/// with its first two control points at the apex, so `S_u = S_v = 0` there.
fn cone(scale: f64, offset: [f64; 3]) -> EvalSurface {
    turned(
        &[([0.0, 0.0], 1.0), ([0.0, 0.0], 1.0), ([1.0, 1.0], 1.0)],
        scale,
        offset,
    )
}

/// The apex of a cone whose first derivatives both vanish there is collapsed along its row (v,
/// the arc) and not along u, wherever the cone lies and at any scale, at every v, exactly and
/// within a reach; off the apex nothing is collapsed. The speeds alone read both directions as
/// collapsed there, and an apex off the origin (its derivatives then rounding, not zero) fell
/// back to that.
#[test]
fn an_apex_of_zero_speed_is_collapsed_along_its_row_anywhere_and_at_any_scale() {
    let cases = [
        (10.0, [0.0, 0.0, 0.0]),
        (10.0, [10.0, 0.0, 0.0]),
        (10.0, [1000.0, 1000.0, 1000.0]),
        (1e-2, [0.01, 0.0, 0.0]),
        (1e4, [1e4, -2e4, 3e4]),
    ];
    for (scale, offset) in cases {
        let s = cone(scale, offset);
        let reach = 1e-7 * scale;
        for v in [0.0, 0.3, 0.77, 1.0] {
            let (_, su, sv) = s.derivs(0.0, v).unwrap();
            let size = scale + offset.iter().map(|x| x.abs()).fold(0.0, f64::max);
            assert!(
                su.len() <= 1e-12 * size && sv.len() <= 1e-12 * size,
                "{scale} {offset:?} {v}"
            );
            for r in [0.0, reach] {
                assert_eq!(
                    collapse(&s, [0.0, v], r).unwrap(),
                    Collapse::V,
                    "{scale} {offset:?} v {v} reach {r}"
                );
            }
            assert_eq!(collapse(&s, [0.5, v], 0.0).unwrap(), Collapse::Neither);
            assert_eq!(collapse(&s, [0.5, v], reach).unwrap(), Collapse::Neither);
            // Beside the apex, within the reach: its circle (radius scale · u²) is within it,
            // the meridian is not. A speed × span test read both as collapsed here.
            assert_eq!(collapse(&s, [1e-9, v], reach).unwrap(), Collapse::V);
            assert!(degenerate(&s, [0.0, v], 1).unwrap() && !degenerate(&s, [0.0, v], 0).unwrap());
            assert!(degenerate_within(&s, [0.0, v], 1, reach).unwrap());
            assert!(!degenerate_within(&s, [0.0, v], 0, reach).unwrap());
        }
    }
    // A rational profile (weights 1, 0.5, 1) off the origin.
    let s = turned(
        &[([0.0, 0.0], 1.0), ([0.0, 0.0], 0.5), ([1.0, 1.0], 1.0)],
        10.0,
        [10.0, 0.0, 0.0],
    );
    for v in [0.0, 0.3, 1.0] {
        assert_eq!(collapse(&s, [0.0, v], 1e-6).unwrap(), Collapse::V);
    }
}

/// An apex of degree 5 (S ~ s⁵, every derivative to the fourth zero along both directions) is
/// collapsed along its row; a row of degree 5 whose first five points coincide but whose sixth
/// does not is not collapsed, though its derivatives to the fourth vanish at v = 0.
#[test]
fn degree_five_apexes_and_rows_are_decided_on_the_net() {
    let mut profile = vec![([0.0, 0.0], 1.0); 5];
    profile.push(([1.0, 1.0], 1.0));
    let s = turned(&profile, 10.0, [10.0, 20.0, 30.0]);
    assert_eq!(collapse(&s, [0.0, 0.3], 0.0).unwrap(), Collapse::V);
    assert_eq!(collapse(&s, [0.5, 0.3], 0.0).unwrap(), Collapse::Neither);

    let o = [0.0, 0.0, 0.0];
    let generic = |i: usize, j: usize| [i as f64, j as f64, (i * j) as f64 + 1.0];
    let rows: Vec<Vec<([f64; 3], f64)>> = (0..3)
        .map(|i| {
            (0..6)
                .map(|j| {
                    let p = match (i, j) {
                        (0, 5) => [0.0, 0.0, 1.0],
                        (0, _) | (1, 0) => o,
                        _ => generic(i, j),
                    };
                    (p, 1.0)
                })
                .collect()
        })
        .collect();
    let s = spline([2, 5], &rows);
    let p = s.partials(0.0, 0.0).unwrap();
    assert!(
        (1..=4).all(|k| p[0][k].len() == 0.0),
        "the row's derivatives to the fourth vanish"
    );
    assert_ne!(
        s.derivs(0.0, 1.0).unwrap().0,
        s.derivs(0.0, 0.0).unwrap().0,
        "the row moves"
    );
    for v in [0.0, 0.5] {
        assert_eq!(
            collapse(&s, [0.0, v], 0.0).unwrap(),
            Collapse::Neither,
            "{v}"
        );
    }
}

/// An isolated singular point (P00 = P10 = P01: `S_u = S_v = 0` at the corner, `S_uu`, `S_vv` not)
/// is no pole; a doubly degenerate corner (rows u = 0 and v = 0 both at one point) is collapsed
/// along both directions there and along each side's own direction elsewhere on it.
#[test]
fn a_singular_point_is_no_pole_and_a_doubly_degenerate_corner_is_both() {
    let o = [5.0, 6.0, 7.0];
    let generic = |i: usize, j: usize| [5.0 + i as f64, 6.0 + j as f64, 7.0 + (i * i + j) as f64];
    let net = |at_o: &dyn Fn(usize, usize) -> bool| -> EvalSurface {
        let rows: Vec<Vec<([f64; 3], f64)>> = (0..3)
            .map(|i| {
                (0..3)
                    .map(|j| (if at_o(i, j) { o } else { generic(i, j) }, 1.0))
                    .collect()
            })
            .collect();
        spline([2, 2], &rows)
    };
    let singular = net(&|i, j| (i, j) == (0, 0) || (i, j) == (1, 0) || (i, j) == (0, 1));
    let (_, su, sv) = singular.derivs(0.0, 0.0).unwrap();
    assert!(su.len() == 0.0 && sv.len() == 0.0);
    assert_eq!(
        collapse(&singular, [0.0, 0.0], 0.0).unwrap(),
        Collapse::Neither
    );

    let corner = net(&|i, j| i == 0 || j == 0);
    assert_eq!(collapse(&corner, [0.0, 0.0], 0.0).unwrap(), Collapse::Both);
    assert_eq!(collapse(&corner, [0.0, 0.5], 0.0).unwrap(), Collapse::V);
    assert_eq!(collapse(&corner, [0.5, 0.0], 0.0).unwrap(), Collapse::U);
    assert_eq!(
        collapse(&corner, [0.5, 0.5], 0.0).unwrap(),
        Collapse::Neither
    );
}

/// A sphere as a revolution (two rational quarter arcs from pole to pole), off the origin and at
/// scales 1e-3 and 1e3: collapsed along the angle at both poles, nowhere else.
#[test]
fn a_sphere_of_revolution_is_collapsed_at_its_poles_only() {
    for (r, o) in [
        (2.0, [0.0, 0.0, 0.0]),
        (2e-3, [1.0, -1.0, 2.0]),
        (2e3, [3e3, 0.0, -4e3]),
    ] {
        let pts = [[0.0, -r], [r, -r], [r, 0.0], [r, r], [0.0, r]];
        let ws = [1.0, MID, 1.0, MID, 1.0];
        let ctrl = pts
            .iter()
            .zip(ws)
            .map(|([x, z], w)| [w * (o[0] + x), w * o[1], w * (o[2] + z), w])
            .collect();
        let s = EvalSurface::Revolved {
            profile: EvalCurve::new(2, vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0], ctrl),
            origin: Vec3::new(o[0], o[1], o[2]),
            axis: Vec3::Z,
            transform: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            swap: false,
        };
        for a in [0.0, 1.0, 4.0] {
            assert_eq!(collapse(&s, [0.0, a], 0.0).unwrap(), Collapse::V, "{r}");
            assert_eq!(collapse(&s, [1.0, a], 0.0).unwrap(), Collapse::V, "{r}");
            assert_eq!(
                collapse(&s, [0.25, a], 0.0).unwrap(),
                Collapse::Neither,
                "{r}"
            );
            assert_eq!(
                collapse(&s, [0.25, a], 1e-3 * r).unwrap(),
                Collapse::Neither,
                "{r}"
            );
        }
    }
}

/// What cannot be decided is refused, never guessed: parameters or a reach that are not finite,
/// a net with a point that is not finite, a parameter off the spline, a direction past v.
#[test]
fn what_cannot_be_decided_is_refused() {
    let s = cone(10.0, [1.0, 2.0, 3.0]);
    assert!(collapse(&s, [f64::NAN, 0.3], 0.0).is_err());
    assert!(collapse(&s, [0.0, f64::INFINITY], 0.0).is_err());
    assert!(collapse(&s, [0.0, 0.3], f64::NAN).is_err());
    assert!(collapse(&s, [0.0, 0.3], -1.0).is_err());
    assert!(collapse(&s, [3.0, 0.3], 0.0).is_err(), "off the spline");
    assert!(degenerate(&s, [0.0, 0.3], 2).is_err());
    let mut rows = vec![vec![([0.0, 0.0, 0.0], 1.0); 3]; 3];
    rows[2][2] = ([f64::NAN, 0.0, 0.0], 1.0);
    let bad = spline([2, 2], &rows);
    assert!(collapse(&bad, [0.0, 0.0], 0.0).is_err());
}

// ---- Round 2 ----

/// The tess test surfaces with no point somewhere (a weight with a root there): an exact
/// cylinder of radius 3 about z whose rational profile's weight 3 (1 − 2u)² has its double
/// root at u = 1/2, its homogeneous weights 3, −1, −1, 3 of both signs.
fn cylinder_without_its_middle() -> EvalSurface {
    let ctrl = vec![
        [9.0, 0.0, 0.0, 3.0],
        [-3.0, 0.0, 40.0, -1.0],
        [-3.0, 0.0, -80.0, -1.0],
        [9.0, 0.0, 120.0, 3.0],
    ];
    EvalSurface::Revolved {
        profile: EvalCurve::new(3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], ctrl),
        origin: Vec3::ZERO,
        axis: Vec3::Z,
        transform: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        swap: false,
    }
}

/// The plane z = 0, x = u, y = v over [0, 1]² as a rational bicubic whose weight
/// (u − ½)² + (v − ½)² has its root at (½, ½), of both signs in its net.
fn plane_without_its_middle() -> EvalSurface {
    let a36 = [9.0, -3.0, -3.0, 9.0];
    let ta36 = [0.0, 3.0, -6.0, 9.0];
    let t3 = [0.0, 1.0, 2.0, 3.0];
    let a12 = [3.0, -1.0, -1.0, 3.0];
    let mut ctrl = Vec::new();
    for j in 0..4 {
        for i in 0..4 {
            let x = ta36[i] + t3[i] * a12[j];
            let y = a12[i] * t3[j] + ta36[j];
            ctrl.push([x, y, 0.0, a36[i] + a36[j]]);
        }
    }
    let knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    EvalSurface::nurbs([3, 3], [knots.clone(), knots], [4, 4], ctrl)
}

/// The parabolic cylinder z = u² over [0, 1]² as a rational quartic-by-cubic with no point at
/// (½, ½), of both signs in its net.
fn parabola_without_its_middle() -> EvalSurface {
    let ctrl: Vec<[f64; 4]> = [
        [0, 0, 0, 36],
        [9, 0, 0, 18],
        [6, 0, 6, 12],
        [9, 0, 0, 18],
        [36, 0, 36, 36],
        [0, 12, 0, 12],
        [3, 6, 0, -6],
        [-6, 4, 2, -12],
        [-9, 6, -12, -6],
        [12, 12, 12, 12],
        [0, 0, 0, 12],
        [3, -12, 0, -6],
        [-6, -16, 2, -12],
        [-9, -12, -12, -6],
        [12, 0, 12, 12],
        [0, 36, 0, 36],
        [9, 18, 0, 18],
        [6, 12, 6, 12],
        [9, 18, 0, 18],
        [36, 36, 36, 36],
    ]
    .iter()
    .map(|c| c.map(f64::from))
    .collect();
    EvalSurface::nurbs(
        [4, 3],
        [
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        ],
        [5, 4],
        ctrl,
    )
}

/// R2-1: the weights asked about are the iso-line's own. A point away from the bad weight of
/// the tess no-value surfaces classifies (no pole), exactly and within a reach: the net-wide
/// "every weight positive" requirement refused them all. A net whose weights are all negative
/// is as good as one whose weights are all positive, and a negative weight away from a pole row
/// leaves the row a pole. Only a weight on the iso-line not certified non-zero is refused.
#[test]
fn only_the_iso_lines_own_weights_matter() {
    let cyl = cylinder_without_its_middle();
    for q in [[0.1, 0.3], [0.9, 2.0], [0.0, 0.0]] {
        for reach in [0.0, 1e-6] {
            assert_eq!(
                collapse(&cyl, q, reach).unwrap(),
                Collapse::Neither,
                "{q:?} {reach}"
            );
        }
    }
    // On the circle with no point the profile point's weight is zero: refused.
    assert!(collapse(&cyl, [0.5, 0.3], 0.0).is_err());
    for s in [plane_without_its_middle(), parabola_without_its_middle()] {
        for q in [[0.3, 0.7], [0.5, 0.25], [0.9, 0.5], [0.0, 1.0]] {
            for reach in [0.0, 1e-6] {
                assert_eq!(
                    collapse(&s, q, reach).unwrap(),
                    Collapse::Neither,
                    "{q:?} {reach}"
                );
            }
        }
    }
    // All weights negative: the cone's apex is the same pole.
    let EvalSurface::Nurbs(n) = cone(10.0, [1.0, 2.0, 3.0]) else {
        unreachable!()
    };
    let negated: Vec<[f64; 4]> = n.ctrl().iter().map(|h| h.map(|x| -x)).collect();
    let neg = EvalSurface::nurbs(n.degree(), n.knots().clone(), n.count(), negated);
    for reach in [0.0, 1e-6] {
        assert_eq!(collapse(&neg, [0.0, 0.3], reach).unwrap(), Collapse::V);
    }
    // One weight of the rim made negative (far from the apex row): the apex is still a pole.
    let mut ctrl = n.ctrl().to_vec();
    ctrl[2 + 3] = ctrl[2 + 3].map(|x| -x); // u index 2 (the rim), v index 1 (the arc's middle)
    let far = EvalSurface::nurbs(n.degree(), n.knots().clone(), n.count(), ctrl);
    assert_eq!(collapse(&far, [0.0, 0.3], 0.0).unwrap(), Collapse::V);
}

/// R2-3: the bounds are the control points' bounding box (not twice the spread from the first)
/// and the map's spectral bound (1 for the identity, not √3). A circle of diameter 0.8 is
/// collapsed within 1.0; doubled by the map, within 1.7 and not 1.0. A row whose points lie at
/// x = −e, e, −e (e = 1e-7) moves by at most 2e-7: collapsed within 2.1e-7, not 1.9e-7.
#[test]
fn reach_bounds_are_tight() {
    let profile = EvalCurve::new(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![[0.4, 0.0, 0.0, 1.0], [0.4, 0.0, 1.0, 1.0]],
    );
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let doubled = [
        [0.0, -2.0, 0.0, 5.0],
        [2.0, 0.0, 0.0, -1.0],
        [0.0, 0.0, 2.0, 0.0],
    ];
    let tube = |transform| EvalSurface::Revolved {
        profile: profile.clone(),
        origin: Vec3::ZERO,
        axis: Vec3::Z,
        transform,
        swap: false,
    };
    assert_eq!(
        collapse(&tube(identity), [0.5, 1.0], 1.0).unwrap(),
        Collapse::V
    );
    assert_eq!(
        collapse(&tube(identity), [0.5, 1.0], 0.79).unwrap(),
        Collapse::Neither
    );
    assert_eq!(
        collapse(&tube(doubled), [0.5, 1.0], 1.0).unwrap(),
        Collapse::Neither
    );
    assert_eq!(
        collapse(&tube(doubled), [0.5, 1.0], 1.7).unwrap(),
        Collapse::V
    );

    let e = 1e-7;
    let rows: Vec<Vec<([f64; 3], f64)>> = (0..3)
        .map(|i| {
            (0..3)
                .map(|j| {
                    let p = if i == 0 {
                        [if j == 1 { e } else { -e }, 0.0, 0.0]
                    } else {
                        [f64::from(i), f64::from(j), f64::from(i * j)]
                    };
                    (p, 1.0)
                })
                .collect()
        })
        .collect();
    let s = spline([2, 2], &rows);
    assert!(degenerate_within(&s, [0.0, 0.4], 1, 2.1e-7).unwrap());
    assert!(!degenerate_within(&s, [0.0, 0.4], 1, 1.9e-7).unwrap());
    assert!(!degenerate(&s, [0.0, 0.4], 1).unwrap());
}

/// R2-5: a row collapsed over only part of its range (v in [0, ½] of a two-span quadratic: its
/// first three points coincide, the fourth does not) is a pole of a face trimmed to that part,
/// and not of the whole spline or of a face reaching past it.
#[test]
fn a_row_collapsed_over_part_of_its_range_is_a_pole_there() {
    let c = [1.0, 2.0, 3.0];
    let rows: Vec<Vec<([f64; 3], f64)>> = (0..3)
        .map(|i| {
            (0..4)
                .map(|j| {
                    let p = match (i, j) {
                        (0, 3) => [1.0, 2.0, 4.0],
                        (0, _) => c,
                        _ => [
                            1.0 + f64::from(i),
                            2.0 + f64::from(j),
                            3.0 + f64::from(i * j),
                        ],
                    };
                    (p, 1.0)
                })
                .collect()
        })
        .collect();
    let s = spline([2, 2], &rows); // v knots 0 0 0 ½ 1 1 1
    let at = [0.0, 0.2];
    assert!(collapsed_on(&s, at, 1, 0.0, Some((0.0, 0.5))).unwrap());
    assert!(collapsed_on(&s, at, 1, 0.0, Some((0.1, 0.4))).unwrap());
    assert!(!collapsed_on(&s, at, 1, 0.0, Some((0.25, 0.75))).unwrap());
    assert!(!collapsed(&s, at, 1, 0.0).unwrap());
}

/// R2-6: knots with an empty span where one is needed are malformed (Invalid), not a point off
/// the surface; a revolution's axis must be of unit length.
#[test]
fn malformed_knots_and_axes_are_named() {
    let rows = vec![vec![([0.0, 0.0, 0.0], 1.0); 3]; 3];
    let ctrl: Vec<[f64; 4]> = rows
        .iter()
        .flatten()
        .map(|(p, w)| [p[0], p[1], p[2], *w])
        .collect();
    let flat = EvalSurface::nurbs(
        [2, 2],
        [vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![1.0; 6]],
        [3, 3],
        ctrl,
    );
    assert!(matches!(
        collapse(&flat, [0.5, 1.0], 0.0),
        Err(EvalError::Invalid(_))
    ));
    let long = EvalSurface::Revolved {
        profile: EvalCurve::new(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 1.0, 1.0]],
        ),
        origin: Vec3::ZERO,
        axis: Vec3::new(0.0, 0.0, 2.0),
        transform: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        swap: false,
    };
    assert!(matches!(
        collapse(&long, [0.0, 0.0], 0.0),
        Err(EvalError::Invalid(_))
    ));
}

/// The cone's apex as a revolution with its parameters swapped (u the angle) and a rigid map off
/// the origin: collapsed along u there. A row that coincides only to an importer's tolerance
/// (spread by 1e-10 of its size) is no pole to rounding, and one within a reach covering it.
#[test]
fn a_swapped_mapped_revolution_and_a_producer_tolerance_row() {
    let profile = EvalCurve::new(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0, 1.0],
            [10.0, 0.0, 10.0, 1.0],
        ],
    );
    let (s, c) = 0.6f64.sin_cos();
    let s = EvalSurface::Revolved {
        profile,
        origin: Vec3::ZERO,
        axis: Vec3::Z,
        transform: [
            [c, -s, 0.0, 100.0],
            [s, c, 0.0, -50.0],
            [0.0, 0.0, 1.0, 7.0],
        ],
        swap: true,
    };
    for a in [0.0, 1.3, 5.0] {
        assert_eq!(collapse(&s, [a, 0.0], 0.0).unwrap(), Collapse::U);
        assert_eq!(collapse(&s, [a, 0.5], 1e-6).unwrap(), Collapse::Neither);
    }

    let EvalSurface::Nurbs(n) = cone(10.0, [1.0, 2.0, 3.0]) else {
        unreachable!()
    };
    let mut ctrl = n.ctrl().to_vec();
    // The apex row (u index 0) spread by ±1e-9 mm (1e-10 of the cone's size).
    for (j, h) in ctrl.iter_mut().enumerate().filter(|(k, _)| k % 3 == 0) {
        h[0] += if j % 2 == 0 { 1e-9 } else { -1e-9 } * h[3];
    }
    let s = EvalSurface::nurbs(n.degree(), n.knots().clone(), n.count(), ctrl);
    assert_eq!(collapse(&s, [0.0, 0.3], 0.0).unwrap(), Collapse::Neither);
    assert_eq!(collapse(&s, [0.0, 0.3], 1e-6).unwrap(), Collapse::V);
}

// ---- Off the knot range (`collapse_near`, `SurfaceEval::pole_within`) ----

/// The degree (1, 1) polynomial patch on knots `[lo, lo, hi, hi]` along u and v whose corners
/// are `p00, p10, p01, p11` (first index u).
fn bilinear(u: (f64, f64), v: (f64, f64), corners: [[f64; 3]; 4]) -> EvalSurface {
    let h = |p: [f64; 3]| [p[0], p[1], p[2], 1.0];
    EvalSurface::nurbs(
        [1, 1],
        [vec![u.0, u.0, u.1, u.1], vec![v.0, v.0, v.1, v.1]],
        [2, 2],
        corners.iter().map(|p| h(*p)).collect(),
    )
}

/// The patch `scale × (u, v, 0)` over `u` × `v`: speed `scale` along both, no pole.
fn flat(u: (f64, f64), v: (f64, f64), scale: f64) -> EvalSurface {
    let p = |a: f64, b: f64| [scale * a, scale * b, 0.0];
    bilinear(u, v, [p(u.0, v.0), p(u.1, v.0), p(u.0, v.1), p(u.1, v.1)])
}

fn off_range(r: Result<Collapse, EvalError>) -> bool {
    matches!(r, Err(EvalError::OffRange(m)) if m.starts_with("OffRangeBeyondReach"))
}

#[test]
fn a_point_past_the_range_is_asked_at_the_range_with_the_reach_less_its_shift() {
    // Speed 10 along u: 1e-5 past u = 1 shifts the surface by 1e-4, so δ = 2e-4.
    let s = flat((0.0, 1.0), (0.0, 1.0), 10.0);
    let uv = [1.0 + 1e-5, 0.5];
    let (near, delta) = onto_range(&s, uv).unwrap().unwrap();
    assert_eq!(near, [1.0, 0.5]);
    let want = 2.0 * (uv[0] - 1.0) * 10.0;
    assert!(
        (want..=want * (1.0 + 1e-9)).contains(&delta),
        "{delta} {want}"
    );
    assert!(
        collapse(&s, uv, 1e-3).is_err(),
        "collapse itself still refuses it"
    );
    // Under the reach: answered, as at the range's end.
    assert_eq!(collapse_near(&s, uv, 1e-3).unwrap(), Collapse::Neither);
    // At or over it, and at no reach: refused by name, never answered.
    assert!(off_range(collapse_near(&s, uv, 2e-4)));
    assert!(off_range(collapse_near(&s, uv, 1e-4)));
    assert!(off_range(collapse_near(&s, uv, 0.0)));
    // On the range, and past it by rounding only: collapse's own answer.
    for q in [[0.5, 0.5], [1.0 + 1e-12, 0.5]] {
        assert_eq!(onto_range(&s, q).unwrap(), None);
        assert_eq!(collapse_near(&s, q, 1e-3), collapse(&s, q, 1e-3));
    }
    // Below the range too, and off both ways at once (each partial's bound grown by the
    // other direction's continuation).
    let (near, delta) = onto_range(&s, [-1e-5, 0.5]).unwrap().unwrap();
    assert_eq!(near, [0.0, 0.5]);
    assert!((2e-4..=2e-4 * (1.0 + 1e-9)).contains(&delta), "{delta}");
    let (near, delta) = onto_range(&s, [1.0 + 1e-6, 1.0 + 1e-6]).unwrap().unwrap();
    assert_eq!(near, [1.0, 1.0]);
    let o = (1.0 + 1e-6) - 1.0;
    let want = 2.0 * 2.0 * o * 10.0 * (1.0 + 2.0 * o);
    assert!(
        (want..=want * (1.0 + 1e-9)).contains(&delta),
        "{delta} {want}"
    );
}

#[test]
fn a_pole_past_the_range_is_a_pole_within_the_reach() {
    // The side u = 1 is one point (10, 5, 0): collapsed along v there. The u partial's
    // coefficients are (10, 5, 0) and (10, −5, 0), of length √125.
    let s = bilinear(
        (0.0, 1.0),
        (0.0, 1.0),
        [
            [0.0, 0.0, 0.0],
            [10.0, 5.0, 0.0],
            [0.0, 10.0, 0.0],
            [10.0, 5.0, 0.0],
        ],
    );
    let uv = [1.0 + 1e-6, 0.3];
    let (_, delta) = onto_range(&s, uv).unwrap().unwrap();
    let want = 2.0 * (uv[0] - 1.0) * 125f64.sqrt();
    assert!(
        (want..=want * (1.0 + 1e-9)).contains(&delta),
        "{delta} {want}"
    );
    assert_eq!(collapse_near(&s, uv, 1e-3).unwrap(), Collapse::V);
    assert!(off_range(collapse_near(&s, uv, 2e-6)));
}

#[test]
fn a_rational_net_past_the_range_is_refused() {
    let mut rows = vec![
        vec![([0.0, 0.0, 0.0], 1.0), ([0.0, 1.0, 0.0], 1.0)],
        vec![([1.0, 0.0, 0.0], 1.0), ([1.0, 1.0, 0.0], 2.0)],
    ];
    let s = spline([1, 1], &rows);
    assert!(matches!(
        collapse_near(&s, [1.1, 0.5], 1.0),
        Err(EvalError::OffRange(_))
    ));
    rows[1][1].1 = 1.0;
    let s = spline([1, 1], &rows);
    assert_eq!(
        collapse_near(&s, [1.0 + 1e-3, 0.5], 1.0).unwrap(),
        Collapse::Neither
    );
}

#[test]
fn the_step_faces_whose_pcurves_end_past_their_patch_are_answered() {
    // 278_moon_buggy faces #6105 and #6165: speed 25.4, v 1.8087e-5 past its range, at a
    // vertex reach of 2.54e-3 (δ ≈ 9.2e-4).
    let moon = flat(
        (0.076579574233648, 14.393741175797247),
        (2.314966172279155, 7.627145784997975),
        25.4,
    );
    for uv in [
        [2.080136380739515, 7.62716387189905],
        [2.0801363807395163, 7.627163871899048],
    ] {
        let (_, delta) = onto_range(&moon, uv).unwrap().unwrap();
        assert!(delta < 1e-3, "{delta}");
        assert_eq!(
            collapse_near(&moon, uv, 2.54e-3).unwrap(),
            Collapse::Neither
        );
    }
    // 291_tork faces #22815, #24450, #24132 and #25564: speed 10, u 9.765e-8 past ±0.3556 on
    // either side, at a reach of 1e-3.
    let r = 0.355599999999977;
    let tork = flat((-r, r), (-r, r), 10.0);
    for uv in [
        [0.35560009764949707, 2.8421709430403982e-14],
        [0.35560009764920997, -3.552713678800497e-14],
        [-0.35560009764949985, -2.0605739337042883e-14],
        [-0.3556000976491943, 3.339550858072468e-14],
    ] {
        assert!(collapse(&tork, uv, 1e-3).is_err());
        assert_eq!(collapse_near(&tork, uv, 1e-3).unwrap(), Collapse::Neither);
    }
}

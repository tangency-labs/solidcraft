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

/// The plane `S(u, v) = (u, v, 0)` on [0, 1]² as a bilinear spline.
fn unit_square() -> EvalSurface {
    let at = |u: f64, v: f64| ([u, v, 0.0], 1.0);
    spline(
        [1, 1],
        &[
            vec![at(0.0, 0.0), at(0.0, 1.0)],
            vec![at(1.0, 0.0), at(1.0, 1.0)],
        ],
    )
}

/// A pcurve past both knot ranges at a corner is put on both at once, with one bound on the
/// surface's move over both coordinates: from (−1e-6, −1e-6) the plane moves by at most
/// 1e-6 along each.
#[test]
fn a_pcurve_past_both_knot_ranges_is_put_on_both() {
    let s = unit_square();
    let pc = Curve2::Spline {
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![[-1e-6, -1e-6], [0.5, 0.5]],
    };
    let onto = pcurve_onto_knots(&s, &pc, 1e-3).unwrap().unwrap();
    assert_eq!(
        onto.pcurve,
        Curve2::Spline {
            knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![[0.0, 0.0], [0.5, 0.5]],
        }
    );
    assert!(
        (2e-6..2.0001e-6).contains(&onto.displacement),
        "{}",
        onto.displacement
    );
    // A tolerance under the move refuses it; a pcurve on the range is not moved.
    assert!(matches!(
        pcurve_onto_knots(&s, &pc, 1e-6),
        Err(EvalError::Unevaluable(_))
    ));
    assert_eq!(pcurve_onto_knots(&s, &onto.pcurve, 1e-3), Ok(None));
}

/// A net with one weight so small that its Euclidean points overflow (1 / 1e-310 is infinite)
/// has no partial bound: refused, not a bound of zero.
#[test]
fn a_partial_bound_of_a_net_whose_points_overflow_is_refused() {
    let at = |u: f64, v: f64| ([u + 1.0, v + 1.0, 1.0], 1e-310);
    let s = spline(
        [1, 1],
        &[
            vec![at(0.0, 0.0), at(0.0, 1.0)],
            vec![at(1.0, 0.0), at(1.0, 1.0)],
        ],
    );
    for across in [[0.0, 1.0], [0.0, 0.0]] {
        assert!(
            partial_bound(&s, 0, across, 0.0, 0.0).is_err(),
            "{across:?}"
        );
    }
    assert!(matches!(
        partial_bound(&s, 0, [0.0, 1.0], 0.0, 0.0),
        Err(EvalError::Unevaluable(m)) if m.contains("not finite")
    ));
}

// ---- A pcurve past a rational net (`pcurve_onto_knots`) ----

/// A rounded corner of a STEP part (#15999): a rational patch of degree 3 along u (knots
/// `u[0]` ×4, `u[1]`, `u[2]` ×4) and 2 along v (one span on [0, 1], a quadratic arc), its
/// five rows `rows[i]` in metres as the file writes them, here in mm, `weights[i]` along v.
struct Corner {
    u: [f64; 3],
    rows: [[[f64; 3]; 3]; 5],
    weights: [[f64; 3]; 5],
}

impl Corner {
    fn surface(&self) -> EvalSurface {
        let [a, b, c] = self.u;
        let mut ctrl = Vec::new();
        for j in 0..3 {
            for (row, w) in self.rows.iter().zip(&self.weights) {
                let p = row[j].map(|x| 1000.0 * x);
                ctrl.push([w[j] * p[0], w[j] * p[1], w[j] * p[2], w[j]]);
            }
        }
        EvalSurface::nurbs(
            [3, 2],
            [
                vec![a, a, a, a, b, c, c, c, c],
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            ],
            [5, 3],
            ctrl,
        )
    }

    /// The point at `(u, v)`, `u` on the range and `v` anywhere: the patch's one span along v
    /// continued past it (its Bernstein form), by de Boor along u on each column.
    fn continued(&self, u: f64, v: f64) -> Vec3 {
        let [a, b, c] = self.u;
        let knots = [a, a, a, a, b, c, c, c, c];
        let k = if u < b { 3 } else { 4 };
        let mut h = [0.0f64; 4];
        let bern = [(1.0 - v) * (1.0 - v), 2.0 * v * (1.0 - v), v * v];
        for (j, bj) in bern.iter().enumerate() {
            let mut d: Vec<[f64; 4]> = (k - 3..=k)
                .map(|i| {
                    let (p, w) = (self.rows[i][j].map(|x| 1000.0 * x), self.weights[i][j]);
                    [w * p[0], w * p[1], w * p[2], w]
                })
                .collect();
            for r in 1..=3 {
                for jj in (r..=3).rev() {
                    let i = k - 3 + jj;
                    let t = (u - knots[i]) / (knots[i + 4 - r] - knots[i]);
                    let prev = d[jj - 1];
                    for (x, y) in d[jj].iter_mut().zip(prev) {
                        *x = (1.0 - t) * y + t * *x;
                    }
                }
            }
            for (x, y) in h.iter_mut().zip(d[3]) {
                *x += bj * y;
            }
        }
        Vec3::new(h[0], h[1], h[2]) * (1.0 / h[3])
    }
}

/// Faces #1492 and #1527 of the part in #15999 (`Part Studio 1 - U-Basis rechts.step`).
fn corners() -> [Corner; 2] {
    [
        Corner {
            u: [
                4.11996825544492e-18,
                0.00251056122308457,
                0.00502112244616914,
            ],
            rows: [
                [
                    [0.0095184968375232, -0.00986179878491862, 0.0134614064833007],
                    [0.00984547792923133, -0.0105042605347972, 0.0134871606368189],
                    [0.00993553687231624, -0.0105705161938981, 0.0127745340970221],
                ],
                [
                    [0.00897867388977568, -0.0102463642058592, 0.0134188881743844],
                    [0.00936996294472618, -0.0107379916504237, 0.0133681247951212],
                    [0.00927460906300522, -0.0109053022311392, 0.0127460470296204],
                ],
                [
                    [0.0080222599860908, -0.0112868486692588, 0.0137696671002265],
                    [0.00825309201170418, -0.0114502503599108, 0.0134946237875554],
                    [0.00809036813975832, -0.0117513433342993, 0.0132894503045143],
                ],
                [
                    [0.00721215723437218, -0.0125213092646158, 0.0144252957625912],
                    [0.00721049190656136, -0.0125934604247244, 0.0143424012975383],
                    [0.00720882656029216, -0.0126656116150791, 0.0142595068556948],
                ],
                [
                    [0.00683262629400762, -0.0131389917691748, 0.0147602487171921],
                    [0.00683262629400761, -0.0131389917691748, 0.0147602487171921],
                    [0.00683262629400761, -0.0131389917691748, 0.0147602487171921],
                ],
            ],
            weights: [
                [1.0, 0.742673477355744, 1.0],
                [1.0, 0.792863107677938, 1.0],
                [1.0, 0.940123873307597, 1.0],
                [1.0, 0.999999999980117, 1.0],
                [1.0, 1.0, 1.0],
            ],
        },
        Corner {
            u: [0.0260020748241999, 0.0289898331189168, 0.0319775914136337],
            rows: [
                [
                    [0.00964600585684235, 0.0141999999999938, 0.0134707936396788],
                    [0.00934382669159065, 0.0133434884416167, 0.01345010055111],
                    [0.00925313175592443, 0.0132615384615327, 0.0143503289281726],
                ],
                [
                    [0.0105235219186991, 0.0138640752829399, 0.0135308855311016],
                    [0.0101604417047262, 0.0130699498274284, 0.0135269754561354],
                    [0.0101348711266899, 0.0129514541077582, 0.0143912141505575],
                ],
                [
                    [0.0122359098322075, 0.0129963238459051, 0.0134577370854064],
                    [0.011783139391092, 0.0123590672770896, 0.0135169243785278],
                    [0.0118605844965322, 0.0121504527806829, 0.0142672032412564],
                ],
                [
                    [0.0137210539020434, 0.0118849769714294, 0.013100487137082],
                    [0.0133258875385779, 0.0113457831363959, 0.0132066760081018],
                    [0.0133651304708384, 0.0111245941273494, 0.013845191154838],
                ],
                [
                    [0.0144232612793709, 0.0112683970776079, 0.0128683621841344],
                    [0.0140589970531455, 0.0107713989527169, 0.0129887750694204],
                    [0.014078199200318, 0.0105554434562535, 0.0135780028069607],
                ],
            ],
            weights: [
                [1.0, 0.740161731980124, 1.0],
                [1.0, 0.754618828405926, 1.0],
                [1.0, 0.790554928957286, 1.0],
                [1.0, 0.828478792294795, 1.0],
                [1.0, 0.846911961456441, 1.0],
            ],
        },
    ]
}

/// The trims SMLib makes for these corners start 0.0079 and 0.0066 past v = 0 (about 9 µm
/// on the part, inside the file's 0.02 mm). Each is put on the patch with a certified bound
/// on the surface's move: no smaller than the move measured along the trim, and within twice
/// it (the bound adds what moving along v does to the rows' weights across).
#[test]
fn a_pcurve_past_a_rational_net_is_put_on_it_within_the_tolerance() {
    for (corner, past) in corners()
        .iter()
        .zip([0.007903823091784556, 0.006605362236920265])
    {
        let s = corner.surface();
        let [a, _, c] = corner.u;
        let pc = Curve2::Spline {
            knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![[a, -past], [c, 0.0]],
        };
        let onto = pcurve_onto_knots(&s, &pc, 0.02).unwrap().unwrap();
        assert_eq!(
            onto.pcurve,
            Curve2::Spline {
                knots: vec![0.0, 0.0, 1.0, 1.0],
                points: vec![[a, 0.0], [c, 0.0]],
            }
        );
        let moved = (0..=200)
            .map(|k| {
                let t = f64::from(k) / 200.0;
                let (u, v) = (a + t * (c - a), -past * (1.0 - t));
                (corner.continued(u, v) - corner.continued(u, 0.0)).len()
            })
            .fold(0.0, f64::max);
        assert!(moved > 0.008, "{moved}");
        assert!(
            moved <= onto.displacement && onto.displacement <= 2.0 * moved,
            "measured {moved}, bound {}",
            onto.displacement
        );
        assert!(onto.displacement <= 0.02);
    }
}

/// A trim along a corner's rounded edge (u at its start) from v = 1 to past v = 0 moves points
/// all along v: the bound over the whole range is too wide in one piece (above 0.02 mm), and
/// tight enough once the piece is cut into parts.
#[test]
fn a_pcurve_along_a_rational_net_past_its_range_is_bounded_in_parts() {
    for (corner, past) in corners()
        .iter()
        .zip([0.007903823091784556, 0.006605362236920265])
    {
        let s = corner.surface();
        let a = corner.u[0];
        let (one, _) = rational_partial_bound(&s, 1, [-past, 1.0], [a, a], f64::INFINITY).unwrap();
        let (parts, _) = rational_partial_bound(&s, 1, [-past, 1.0], [a, a], 0.0).unwrap();
        assert!(past * one > 0.02 && parts < one, "{one} {parts}");
        let pc = Curve2::Spline {
            knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![[a, 1.0], [a, -past]],
        };
        let onto = pcurve_onto_knots(&s, &pc, 0.02).unwrap().unwrap();
        let moved = (0..=400)
            .map(|k| {
                let t = f64::from(k) / 400.0;
                let (v, w) = (1.0 - t * (1.0 + past), 1.0 - t);
                (corner.continued(a, v) - corner.continued(a, w)).len()
            })
            .fold(0.0, f64::max);
        assert!(
            moved <= onto.displacement && onto.displacement <= 0.02,
            "measured {moved}, bound {}",
            onto.displacement
        );
    }
}

/// A pcurve past both of a rational net's ranges is refused: the move across is not bounded
/// there.
#[test]
fn a_pcurve_past_both_ranges_of_a_rational_net_is_refused() {
    let corner = &corners()[0];
    let s = corner.surface();
    let [a, _, c] = corner.u;
    let pc = Curve2::Spline {
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![[a - 1e-6, -1e-3], [c, 0.5]],
    };
    assert!(matches!(
        pcurve_onto_knots(&s, &pc, 0.02),
        Err(EvalError::Unevaluable(_))
    ));
}

/// The rational spline of degree `deg` on `knots` whose net is `rows[i][j]` (i along u,
/// Euclidean points with weights), at `uv`, each direction's end spans continued past its
/// range: de Boor on the span the parameter is in, or the end span.
fn continued_at(
    deg: [usize; 2],
    knots: &[Vec<f64>; 2],
    rows: &[Vec<([f64; 3], f64)>],
    uv: [f64; 2],
) -> Vec3 {
    let curve = |p: usize, k: &[f64], pts: &[[f64; 4]], t: f64| -> [f64; 4] {
        let n = pts.len();
        let mut s = p;
        while s + 1 < n && k[s + 1] <= t {
            s += 1;
        }
        let mut d = pts[s - p..=s].to_vec();
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i = s - p + j;
                let a = (t - k[i]) / (k[i + p + 1 - r] - k[i]);
                let prev = d[j - 1];
                for (x, y) in d[j].iter_mut().zip(prev) {
                    *x = (1.0 - a) * y + a * *x;
                }
            }
        }
        d[p]
    };
    let columns: Vec<[f64; 4]> = (0..rows[0].len())
        .map(|j| {
            let pts: Vec<[f64; 4]> = rows
                .iter()
                .map(|r| {
                    let ([x, y, z], w) = r[j];
                    [w * x, w * y, w * z, w]
                })
                .collect();
            curve(deg[0], &knots[0], &pts, uv[0])
        })
        .collect();
    let h = curve(deg[1], &knots[1], &columns, uv[1]);
    Vec3::new(h[0], h[1], h[2]) * (1.0 / h[3])
}

/// The spline of `deg` on `knots` whose net is `rows[i][j]` (i along u).
fn net(deg: [usize; 2], knots: &[Vec<f64>; 2], rows: &[Vec<([f64; 3], f64)>]) -> EvalSurface {
    let (nu, nv) = (rows.len(), rows[0].len());
    let mut ctrl = Vec::new();
    for j in 0..nv {
        for row in rows {
            let ([x, y, z], w) = row[j];
            ctrl.push([w * x, w * y, w * z, w]);
        }
    }
    EvalSurface::nurbs(deg, knots.clone(), [nu, nv], ctrl)
}

/// The bound against `|∂S/∂x_d|` sampled on a grid over the whole strip, along u and along v,
/// on a net of two spans along `d` with weights from 0.2 to 4.5 that the strip runs past at
/// both ends: at the first level and at the finest, never below what is sampled.
#[test]
fn a_rational_partial_bound_holds_over_its_strip() {
    let pts = [[0.0, 0.0], [1.0, 2.0], [3.0, 2.5], [4.0, 0.0]];
    let weights = [1.0, 0.2, 3.0, 1.0];
    let rows: Vec<Vec<([f64; 3], f64)>> = (0..4)
        .map(|i| {
            let [x, y] = pts[i];
            (0..2)
                .map(|j| {
                    let j = f64::from(j);
                    (
                        [x, y + 0.3 * j, 2.0 * j + 0.5 * x],
                        weights[i] * (1.0 + 0.5 * j),
                    )
                })
                .collect()
        })
        .collect();
    let two = vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0];
    let one = vec![0.0, 0.0, 1.0, 1.0];
    let along = [-0.04, 1.04];
    let across = [0.1, 0.9];
    let transposed: Vec<Vec<([f64; 3], f64)>> = (0..2)
        .map(|j| rows.iter().map(|r| r[j]).collect())
        .collect();
    for d in 0..2 {
        let (deg, knots, net_rows) = if d == 0 {
            ([2, 1], [two.clone(), one.clone()], &rows)
        } else {
            ([1, 2], [one.clone(), two.clone()], &transposed)
        };
        let s = net(deg, &knots, net_rows);
        let at = |x: f64, y: f64| {
            let uv = if d == 0 { [x, y] } else { [y, x] };
            continued_at(deg, &knots, net_rows, uv)
        };
        let h = 1e-6;
        let mut sampled = 0.0f64;
        for a in 0..=400 {
            let x = along[0] + (along[1] - along[0]) * f64::from(a) / 400.0;
            for b in 0..=40 {
                let y = across[0] + (across[1] - across[0]) * f64::from(b) / 40.0;
                sampled = sampled.max((at(x + h, y) - at(x - h, y)).len() / (2.0 * h));
            }
        }
        for target in [f64::INFINITY, 0.0] {
            let (bound, eps) = rational_partial_bound(&s, d, along, across, target).unwrap();
            assert!(
                bound >= sampled * (1.0 - 1e-6),
                "d {d}: {bound} < {sampled}"
            );
            assert!(eps < 1e-12, "{eps}");
        }
    }
}

/// A rational pcurve past a rational patch is put on it as a polynomial one is, its weights
/// kept: the moved trim's points within the bound of where they were.
#[test]
fn a_rational_pcurve_past_a_rational_net_is_put_on_it() {
    let corner = &corners()[0];
    let s = corner.surface();
    let [a, _, c] = corner.u;
    let past = 0.007903823091784556;
    let (m, w) = (0.5 * (a + c), 0.5);
    let before = vec![[a, -past, 1.0], [m * w, 0.0, w], [c, 0.0, 1.0]];
    let pc = Curve2::RationalSpline {
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        homogeneous: before.clone(),
    };
    let onto = pcurve_onto_knots(&s, &pc, 0.02).unwrap().unwrap();
    let Curve2::RationalSpline { homogeneous, .. } = &onto.pcurve else {
        panic!("{:?}", onto.pcurve);
    };
    assert_eq!(
        homogeneous,
        &vec![[a, 0.0, 1.0], [m * w, 0.0, w], [c, 0.0, 1.0]]
    );
    let point = |h: &[[f64; 3]], t: f64| {
        let b = [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
        let mut q = [0.0; 3];
        for (bk, hk) in b.iter().zip(h) {
            for (x, y) in q.iter_mut().zip(hk) {
                *x += bk * y;
            }
        }
        [q[0] / q[2], q[1] / q[2]]
    };
    let moved = (0..=200)
        .map(|k| {
            let t = f64::from(k) / 200.0;
            let ([u0, v0], [u1, v1]) = (point(&before, t), point(homogeneous, t));
            (corner.continued(u0, v0) - corner.continued(u1, v1)).len()
        })
        .fold(0.0, f64::max);
    assert!(
        moved > 0.008 && moved <= onto.displacement && onto.displacement <= 0.02,
        "{moved} {}",
        onto.displacement
    );
}

/// The corner moved 1e5 mm from the origin is put on its patch as at home: the bound's rounding
/// follows the net's size, not its distance from the origin.
#[test]
fn a_pcurve_past_a_rational_net_far_from_the_origin_is_put_on_it() {
    let [mut corner, _] = corners();
    for row in &mut corner.rows {
        for p in row.iter_mut() {
            p[0] += 100.0;
        }
    }
    let [a, _, c] = corner.u;
    let pc = Curve2::Spline {
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![[a, -0.007903823091784556], [c, 0.0]],
    };
    let far = pcurve_onto_knots(&corner.surface(), &pc, 0.02)
        .unwrap()
        .unwrap();
    let home = pcurve_onto_knots(&corners()[0].surface(), &pc, 0.02)
        .unwrap()
        .unwrap();
    assert!(far.displacement <= 0.02);
    assert!(
        (far.displacement - home.displacement).abs() < 1e-6 * home.displacement,
        "{} {}",
        far.displacement,
        home.displacement
    );
}

/// The pcurve from u = −0.1 to 0.9 at v = 0.5, moved 0.1 along u onto [0, 1].
fn past_u() -> Curve2 {
    Curve2::Spline {
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![[-0.1, 0.5], [0.9, 0.5]],
    }
}

/// A degree-(1, 1) rational patch is bounded; the same surface with every homogeneous
/// coordinate times 1e-200, 1e-160 or 1e200 is refused, its weight products underflowing to
/// zero, below the normal range (where their rounding is no longer relative) or overflowing to
/// infinity, never accepted with a bound that leaves them out.
#[test]
fn a_rational_net_whose_weight_products_underflow_or_overflow_is_refused() {
    let linear = vec![0.0, 0.0, 1.0, 1.0];
    let patch = |scale: f64| {
        let row = |u: f64, w: f64| vec![([u, 0.0, u], w * scale), ([u, 1.0, 2.0 * u], w * scale)];
        net(
            [1, 1],
            &[linear.clone(), linear.clone()],
            &[row(0.0, 1.0), row(1.0, 2.0)],
        )
    };
    let home = pcurve_onto_knots(&patch(1.0), &past_u(), 1.0)
        .unwrap()
        .unwrap();
    assert!(home.displacement > 0.1, "{}", home.displacement);
    for (scale, why) in [
        (1e-200, "products underflow"),
        (1e-160, "products underflow"),
        (1e200, "not finite"),
    ] {
        let scaled = pcurve_onto_knots(&patch(scale), &past_u(), 1.0);
        assert!(
            matches!(&scaled, Err(EvalError::Unevaluable(m)) if m.contains(why)),
            "{scale:e}: {scaled:?}"
        );
    }
}

/// A patch whose points do not change along u, so it moves along u only through its weights:
/// at v = 0.5, x = 1e-20 / (2 + u), and the move from u = −0.1 to 0 is 2.6e-22. With weights
/// near 2e-154 the weights' part of the bound is below the normal range once it is times the
/// patch's extent; it is never dropped, so the bound still covers the move.
#[test]
fn a_rational_net_moved_only_by_its_tiny_weights_is_bounded_over_its_move() {
    let linear = vec![0.0, 0.0, 1.0, 1.0];
    let patch = |scale: f64| {
        let row = |w: f64| vec![([0.0, 0.0, 0.0], w * scale), ([1e-20, 0.0, 0.0], scale)];
        net(
            [1, 1],
            &[linear.clone(), linear.clone()],
            &[row(1.0), row(2.0)],
        )
    };
    let moved = 1e-20 * (1.0 / 1.9 - 1.0 / 2.0);
    for scale in [1.0, 2e-154] {
        let onto = pcurve_onto_knots(&patch(scale), &past_u(), 1e-18);
        assert!(
            matches!(&onto, Ok(Some(o)) if o.displacement >= moved),
            "{scale:e}: {onto:?}"
        );
    }
}

/// The #15999 patch along knots 0 to 1e300 with weights 1e10 and 2e10: the piece's length
/// times its least weight overflows, and the bound is never taken as zero for it.
#[test]
fn a_rational_net_whose_span_times_its_weight_overflows_is_not_bounded_by_zero() {
    let (l, w) = (1e300, 1e10);
    let row = |u: f64, k: f64| vec![([u, 0.0, u], k * w), ([u, 1.0, 2.0 * u], k * w)];
    let s = net(
        [1, 1],
        &[vec![0.0, 0.0, l, l], vec![0.0, 0.0, 1.0, 1.0]],
        &[row(0.0, 1.0), row(1.0, 2.0)],
    );
    let pc = Curve2::Spline {
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![[-0.1 * l, 0.5], [0.9 * l, 0.5]],
    };
    let onto = pcurve_onto_knots(&s, &pc, 1.0);
    assert!(
        !matches!(&onto, Ok(Some(o)) if o.displacement < 0.1),
        "{onto:?}"
    );
}

/// Knots 0, 0, 0.5, 0.5, 1, 1 along u at degree 1: x is 0 below u = 0.5 and 100 above it, each
/// piece flat along u, and the move from u = 0.48 to 0.522 at t = 0.58 crosses the jump. The
/// knot's multiplicity is above the degree, so the knots do not fit and the move is refused.
#[test]
fn a_rational_net_that_jumps_at_a_knot_is_refused() {
    let jump = |x: f64, w: f64| vec![([x, 0.0, 0.0], w), ([x, 1.0, 0.0], w)];
    let s = net(
        [1, 1],
        &[vec![0.0, 0.0, 0.5, 0.5, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]],
        &[
            jump(0.0, 1.0),
            jump(0.0, 2.0),
            jump(100.0, 1.0),
            jump(100.0, 2.0),
        ],
    );
    let onto = pcurve_onto_knots(&s, &past_u(), 1.0);
    assert!(
        matches!(&onto, Err(EvalError::Invalid(m)) if m.contains("multiplicity")),
        "{onto:?}"
    );
}

/// What a rational net cannot be bounded on is refused: a weight that turns negative past the
/// range, a move past the tolerance, and a pcurve whose knots do not fit its points.
#[test]
fn what_a_rational_net_cannot_bound_is_refused() {
    // Weights 1, 10, 1 along v: continued 0.1 past v = 0 the weight is about −0.98.
    let at = |u: f64, v: f64, w: f64| ([u, v, u * v], w);
    let s = net(
        [1, 2],
        &[vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]],
        &[
            vec![at(0.0, 0.0, 1.0), at(0.0, 0.5, 10.0), at(0.0, 1.0, 1.0)],
            vec![at(1.0, 0.0, 1.0), at(1.0, 0.5, 10.0), at(1.0, 1.0, 1.0)],
        ],
    );
    let pc = Curve2::Spline {
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![[0.5, -0.1], [0.5, 0.5]],
    };
    assert!(matches!(
        pcurve_onto_knots(&s, &pc, 1.0),
        Err(EvalError::Unevaluable(m)) if m.contains("certified positive")
    ));
    let corner = &corners()[0];
    let s = corner.surface();
    let [a, _, c] = corner.u;
    let pc = Curve2::Spline {
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: vec![[a, -0.007903823091784556], [c, 0.0]],
    };
    assert!(matches!(
        pcurve_onto_knots(&s, &pc, 1e-4),
        Err(EvalError::Unevaluable(m)) if m.contains("past the tolerance")
    ));
    let pc = Curve2::Spline {
        knots: vec![0.0, 1.0],
        points: vec![[a, -0.007903823091784556], [c, 0.0]],
    };
    assert!(matches!(
        pcurve_onto_knots(&s, &pc, 0.02),
        Err(EvalError::Invalid(_))
    ));
}

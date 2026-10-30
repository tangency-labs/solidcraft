use super::*;
use crate::brep::eval::SurfaceEval;
use crate::brep::{Curve, Frame, Surface};
use crate::linkage::surface_jet;

const R: f64 = 10.0;
/// The window's half-widths in longitude and latitude.
const ALPHA: f64 = 0.5;
const BETA: f64 = 0.35;

fn spec() -> IsoSpec<'static> {
    IsoSpec { tol: 1e-6, angle_tol: 0.05f64.to_radians(), crown: 0.0, points: &[] }
}

/// The arc of the circle about `frame`'s z of radius `r` from angle `a0` to `a1`, on its NURBS
/// form (rational quadratic quarter arcs, as SMLib holds circles); run backwards when `reverse`.
fn arc(frame: Frame, r: f64, [a0, a1]: [f64; 2], reverse: bool) -> IsoEdge {
    let (form, [t0, t1]) = crate::brep::form::curve(&Curve::Circle { frame, radius: r }, [a0, a1]).unwrap();
    let Curve::RationalSpline { knots, homogeneous } = form else { panic!("a circle's form is rational") };
    let h: Vec<HPoint> = homogeneous.iter().map(|h| HPoint { x: h[0], y: h[1], z: h[2], w: h[3] }).collect();
    let p = knots.len() - h.len() - 1;
    let (mut knots, mut h) = segment(p, &knots, &h, t0, t1).unwrap();
    if reverse {
        knots = knots.iter().rev().map(|k| t0 + t1 - k).collect();
        h.reverse();
    }
    IsoEdge { degree: p, knots, points: h.iter().map(|q| q.project().unwrap()).collect(), weights: Some(h.iter().map(|q| q.w).collect()) }
}

fn frame(origin: Vec3, x: Vec3, y: Vec3) -> Frame {
    Frame { origin: [origin.x, origin.y, origin.z], x_axis: [x.x, x.y, x.z], y_axis: [y.x, y.y, y.z] }
}

/// A window in a sphere of radius `R` about the origin: longitudes `±ALPHA`, latitudes `±BETA`;
/// the bottom latitude east, the east meridian up, the top latitude west, the west meridian down.
fn window() -> Vec<IsoEdge> {
    let radial = |u: f64| Vec3::new(u.cos(), u.sin(), 0.0);
    let latitude = |v: f64| frame(Vec3::Z * (R * v.sin()), Vec3::X, Vec3::Y);
    let meridian = |u: f64| frame(Vec3::ZERO, radial(u), Vec3::Z);
    vec![
        arc(latitude(-BETA), R * BETA.cos(), [-ALPHA, ALPHA], false),
        arc(meridian(ALPHA), R, [-BETA, BETA], false),
        arc(latitude(BETA), R * BETA.cos(), [-ALPHA, ALPHA], true),
        arc(meridian(-ALPHA), R, [-BETA, BETA], true),
    ]
}

/// The sphere round the window, met with continuity `order` (1 … 4): its outward normal, the way
/// it leaves into the window, and its jet.
fn sphere_beside(edges: &[IsoEdge], order: usize) -> impl FnMut(usize, &[f64]) -> Result<Vec<Option<Across>>, String> + '_ {
    let sphere = SurfaceEval::new(&Surface::Sphere { frame: frame(Vec3::ZERO, Vec3::X, Vec3::Y), radius: R }).unwrap();
    move |k, ts| {
        let e = &edges[k];
        ts.iter()
            .map(|t| {
                let p = eval_edge_h(e, *t).unwrap().project().unwrap();
                let uv = [p.y.atan2(p.x), (p.z / R).clamp(-1.0, 1.0).asin()];
                let (_, su, sv) = sphere.derivs(uv[0], uv[1]).unwrap();
                let normal = su.cross(sv).normalized().unwrap();
                let leave = match k {
                    0 => sv.normalized().unwrap(),
                    1 => su.normalized().unwrap() * -1.0,
                    2 => sv.normalized().unwrap() * -1.0,
                    _ => su.normalized().unwrap(),
                };
                let jet = (order >= 2).then(|| surface_jet(&sphere, uv, normal, leave, order).unwrap());
                Ok(Some(Across { leave, normal, bend: None, jet }))
            })
            .collect()
    }
}

/// How far the fill strays from the sphere, over a grid of each patch.
fn off_sphere(fill: &IsoFill) -> f64 {
    let mut worst = 0.0f64;
    for p in &fill.patches {
        for i in 0..=16 {
            for j in 0..=16 {
                let q = p.surface.eval(i as f64 / 16.0, j as f64 / 16.0).unwrap();
                worst = worst.max((q.len() - R).abs());
            }
        }
    }
    worst
}

/// The fill's one patch as a surface record, to be evaluated independently of the fill.
fn record(fill: &IsoFill) -> SurfaceEval {
    let s = &fill.patches[0].surface;
    let [nu, nv] = s.count();
    let homogeneous = (0..nu).map(|i| (0..nv).map(|j| s.point(i, j)).map(|h| [h.x, h.y, h.z, h.w]).collect()).collect();
    let [u_knots, v_knots] = s.knots().clone();
    SurfaceEval::new(&Surface::RationalSpline { u_knots, v_knots, homogeneous }).unwrap()
}

/// The largest jump of order `k` (2 … 4) across the window's edges, clear of the corners: the
/// fill's linkage curves (its exact partials across each edge) against the sphere's jet of order
/// `k`, per unit length of their foot, as `crate::linkage` defines it.
fn jump(fill: &IsoFill, edges: &[IsoEdge], k: usize) -> f64 {
    let surface = record(fill);
    let mut beside = sphere_beside(edges, k);
    let mut worst = 0.0f64;
    for (side, edge) in edges.iter().enumerate() {
        let [a, b] = edge.domain();
        for i in 1..40 {
            let f = i as f64 / 40.0;
            if !(CORNER_SHARE..=1.0 - CORNER_SHARE).contains(&f) {
                continue;
            }
            let across = beside(side, &[a + (b - a) * f]).unwrap()[0].unwrap();
            let (uv, (du, dv, sign)) = station_on(side, f);
            let d = surface.partials(uv[0], uv[1]).unwrap();
            let c: Vec<Vec3> = (1..=k).map(|m| d[m * du][m * dv] * sign.powi(m as i32)).collect();
            let (n, t, w) = (across.normal, across.normal.cross(across.leave), across.leave);
            let x: Vec<[f64; 2]> = c.iter().map(|d| [d.dot(t), d.dot(w)]).collect();
            let want = across.jet.unwrap().targets(&x).unwrap()[k - 2];
            let foot = (c[0] - n * c[0].dot(n)).len();
            worst = worst.max(((n.dot(c[k - 1]) - want) / foot.powi(k as i32)).abs());
        }
    }
    worst
}

/// A window in a sphere, bounded by four circular arcs (rational edges), filled G1 … G4. Each
/// fill of order n holds its jumps of orders 2 … n, by its own report and by an independent
/// reading of its record; the jump of order n + 1 it leaves to its energy is many times larger.
/// (The sphere itself is not in the fill's space, whose weights are a Coons blend and the
/// sphere's a product, so the orders a fill is not asked for do leave jumps.)
#[test]
fn a_sphere_window_on_rational_arcs_holds_each_order_asked_and_not_the_next() {
    let edges = window();
    for order in 1..=4 {
        let fill = iso_fill(&edges, &mut sphere_beside(&edges, order), &spec()).unwrap();
        let a = fill.achieved;
        let measured: Vec<f64> = (2..=4).map(|k| jump(&fill, &edges, k)).collect();
        eprintln!(
            "G{order}: degree {}, {} control points a side, normals within {:.2e}°, reported jumps {:?}, read off the record (G2, G3, G4) {measured:?}, off the sphere {:.2e}",
            fill.patches[0].surface.degree()[0],
            fill.patches[0].surface.count()[0],
            a.normal_deviation.to_degrees(),
            a.jumps,
            off_sphere(&fill)
        );
        assert!(a.normal_deviation <= spec().angle_tol);
        // Each order held within the angle tolerance over the window's size, by the fill's own
        // report and by the independent reading of its record (at other stations, so within
        // twice that).
        let size = {
            let mut all = edges.iter().flat_map(|e| e.points.iter().copied());
            let first = all.next().unwrap();
            let (lo, hi) = all.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
            lo.dist(hi)
        };
        for k in 2..=order {
            let (reported, read) = (turn(a.jumps[k], k, size), turn(measured[k - 2], k, size));
            assert!(reported <= spec().angle_tol, "G{order}: order {k} reported {reported:e}");
            assert!(read <= 2.0 * spec().angle_tol, "G{order}: order {k} read {read:e}");
        }
        if (2..4).contains(&order) {
            let (held, next) = (measured[order - 2], measured[order - 1]);
            assert!(next > 10.0 * held.max(1e-12), "G{order} leaves order {} to the energy: {next:e} vs {held:e}", order + 1);
        }
        // Not the sphere (nothing asks for it), but a cap over the window, not a dent.
        assert!(off_sphere(&fill) < 0.05 * R, "G{order} near the sphere: {}", off_sphere(&fill));
    }
}

#[test]
fn hostile_edges_are_refused() {
    let edges = window();
    let ok = |e: &[IsoEdge]| iso_fill(e, &mut sphere_beside(e, 2), &spec());
    let mut bad = edges.clone();
    bad[1].weights = Some(vec![-1.0; bad[1].points.len()]);
    assert!(ok(&bad).is_err(), "a negative weight");
    let mut bad = edges.clone();
    bad[1].weights = Some(vec![1.0]);
    assert!(ok(&bad).is_err(), "a weight count that does not match");
    let mut bad = edges.clone();
    if let Some(w) = bad[0].weights.as_mut() {
        w[0] = f64::NAN;
    }
    assert!(ok(&bad).is_err(), "a weight that is not a number");
    // One edge's start weight doubled alone (its curve changes, and its corner disagrees).
    let mut bad = edges.clone();
    if let Some(w) = bad[0].weights.as_mut() {
        w[0] *= 2.0;
    }
    assert!(matches!(ok(&bad), Err(SplineError::Unsupported(_))), "corner weights that cannot agree");
    // A jet above G4.
    let mut beside = sphere_beside(&edges, 4);
    let mut too_high = |k: usize, ts: &[f64]| -> Result<Vec<Option<Across>>, String> {
        let mut got = beside(k, ts)?;
        for a in got.iter_mut().flatten() {
            if let Some(j) = a.jet.as_mut() {
                j.order = 5;
            }
        }
        Ok(got)
    };
    assert!(iso_fill(&edges, &mut too_high, &spec()).is_err());
}

/// The rational basis's derivatives (the Leibniz rule on `w N N = R W`) against central
/// differences of the derivative one order lower, on the sphere window's patch.
#[test]
fn a_rational_patch_s_derivatives_are_its_differences() {
    let edges = window();
    let q = Quilt::new(&edges, &[Vec::new(), Vec::new()], 3).unwrap();
    let p = &q.patches[0];
    assert!(p.rational);
    let (u, v, h) = (0.3, 0.6, 1e-5);
    for (du, dv) in [(1, 0), (0, 1), (2, 0), (0, 2), (1, 1), (3, 0), (0, 3)] {
        let exact = p.eval(u, v, du, dv).unwrap();
        let (lu, lv) = if du > 0 { (du - 1, dv) } else { (du, dv - 1) };
        let (a, b) = if du > 0 { ((u + h, v), (u - h, v)) } else { ((u, v + h), (u, v - h)) };
        let differences = (p.eval(a.0, a.1, lu, lv).unwrap() - p.eval(b.0, b.1, lu, lv).unwrap()) * (0.5 / h);
        assert!(exact.dist(differences) < 1e-5 * exact.len().max(1.0), "∂({du}, {dv}): {exact:?} vs {differences:?}");
    }
}

/// The four quarter arcs of the circle of radius `r` about z at height `z`, on their NURBS form.
fn circle_quarters(z: f64, r: f64) -> Vec<IsoEdge> {
    let f = frame(Vec3::Z * z, Vec3::X, Vec3::Y);
    let q = std::f64::consts::FRAC_PI_2;
    (0..4).map(|k| arc(f.clone(), r, [k as f64 * q, (k + 1) as f64 * q], false)).collect()
}

/// Neighbours given by a normal and a leave direction at each point of the loop.
fn beside_by<'a>(
    edges: &'a [IsoEdge],
    frame_at: impl Fn(Vec3) -> (Vec3, Vec3) + 'a,
) -> impl FnMut(usize, &[f64]) -> Result<Vec<Option<Across>>, String> + 'a {
    move |k, ts| {
        ts.iter()
            .map(|t| {
                let p = eval_edge_h(&edges[k], *t).map_err(|e| e.to_string())?.project().ok_or("a point at infinity")?;
                let (normal, leave) = frame_at(p);
                Ok(Some(Across { leave, normal, bend: None, jet: None }))
            })
            .collect()
    }
}

/// Whether any patch of the fill folds (its normal reverses between neighbouring samples).
fn any_fold(fill: &IsoFill) -> bool {
    fill.patches.iter().any(|p| {
        let s = SurfaceEval::new(&Surface::RationalSpline {
            u_knots: p.surface.knots()[0].clone(),
            v_knots: p.surface.knots()[1].clone(),
            homogeneous: (0..p.surface.count()[0])
                .map(|i| (0..p.surface.count()[1]).map(|j| p.surface.point(i, j)).map(|h| [h.x, h.y, h.z, h.w]).collect())
                .collect(),
        })
        .unwrap();
        let n = 48;
        let normal = |i: usize, j: usize| {
            let (_, su, sv) = s.derivs(i as f64 / n as f64, j as f64 / n as f64).unwrap();
            su.cross(sv).normalized()
        };
        (0..=n).any(|i| {
            (0..=n).any(|j| {
                let Some(a) = normal(i, j) else { return false };
                [(i + 1, j), (i, j + 1)].iter().any(|&(k, l)| k <= n && l <= n && normal(k, l).is_some_and(|b| a.dot(b) <= 0.0))
            })
        })
    })
}

/// A tube's end: the circle in four quarter arcs, G1 to the tube's wall (its normal radial, the
/// wall going on upwards). Every corner of the loop is a smooth vertex, where the cross derivative
/// of one side is the next side's tangent, along the edge itself.
#[test]
fn a_tube_end_capped_g1_holds_the_wall_and_does_not_fold() {
    let edges = circle_quarters(30.0, 10.0);
    let radial = |p: Vec3| (Vec3::new(p.x, p.y, 0.0).normalized().unwrap(), Vec3::Z);
    let s = IsoSpec { tol: 2e-5, ..spec() };
    let fill = iso_fill(&edges, &mut beside_by(&edges, radial), &s).unwrap();
    eprintln!("tube end g1: {} patches, {:?}", fill.patches.len(), fill.achieved);
    assert!(fill.achieved.normal_excess <= s.angle_tol);
    assert!(!any_fold(&fill), "the cap folds");
}

//! Certificates for drop answers (tests only): an enclosure of the distance from a point to a
//! surface, around the answer a drop gave. The lower end is the kernel's certified lower bound
//! ([`surface_distance_lower_bound`]: the convex hull of the NURBS form's Bezier patches,
//! subdivided, rounding carried); the upper end is the distance to the drop's surface point,
//! plus the rounding of evaluating that point and of the distance. The evaluation's rounding
//! is bounded from the NURBS form's net (ARGUED in [`evaluation_slack`]); an analytic surface
//! is evaluated in its own form, which its NURBS form represents exactly up to rounding. Every point of the surface is at least `lo`
//! from the point, and the drop's foot is at most `hi`: the foot is the nearest point to
//! within `hi - lo`, whatever the drop did.
//!
//! The goldens WR-56 moved (#17467) are certified with it: each moved drop answer is enclosed,
//! and an old answer outside the enclosure was wrong.

pub(crate) mod tube;

use solidcore::blend::fillet_section::interval::round_up;
use solidcore::blend::fillet_section::station::as_b_spline;
use solidcore::geom::extent2d::Extent2d;
use solidcore::kernel::Kernel;
use solidcore::math::{Point2, Point3};
use solidcore::nurbs::math::{gamma, NOW};
use solidcore::query::distance_bound::surface_distance_lower_bound;
use solidcore::surface::surface::Surface;

/// A certified enclosure of the distance from a point to a surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Enclosure {
    /// No point of the surface is nearer.
    pub(crate) lo: f64,
    /// The drop's foot is no farther.
    pub(crate) hi: f64,
}

impl Enclosure {
    /// `hi - lo`.
    pub(crate) fn width(&self) -> f64 {
        self.hi - self.lo
    }

    /// Whether `d` lies in the enclosure.
    pub(crate) fn holds(&self, d: f64) -> bool {
        self.lo <= d && d <= self.hi
    }
}

/// The surface a drop onto `surface` measures distance to: a curve-bounded surface's
/// basis (its points are the basis's, at the same parameters, and its drops run on the
/// basis), else the surface itself.
pub(crate) fn measured(surface: &Surface) -> &Surface {
    use solidcore::surface::surface::p2::SurfaceKind;
    match surface.kind() {
        SurfaceKind::CurveBounded(c) => measured(c.basis()),
        _ => surface,
    }
}

/// The largest Euclidean coordinate of `surface`'s NURBS net and its degrees: what the
/// rounding of evaluating a point of it is bounded by.
///
/// # Panics
///
/// For a surface without a NURBS form or with a weight that is not positive.
pub(crate) fn net_size(k: &Kernel, surface: &Surface) -> (f64, usize, usize) {
    let surface = measured(surface);
    let form = as_b_spline(k, surface)
        .expect("the NURBS form")
        .expect("a NURBS form");
    let nurb = form.gw_nurb().expect("a NURB");
    let mut size = 0.0_f64;
    for row in nurb.rows() {
        for c in row {
            let w = if c.w == NOW { 1.0 } else { c.w };
            assert!(w > 0.0, "a weight that is not positive");
            size = size
                .max(c.x.abs() / w)
                .max(c.y.abs() / w)
                .max(c.z.abs() / w);
        }
    }
    (size, nurb.p(), nurb.q())
}

/// The rounding of a surface point evaluated at a parameter: de Boor's algorithm takes each
/// coordinate through `p` convex steps in `u` and `q` in `v` (three roundings each, the
/// weights' own included) on coefficients at most the net's size `m`, with the knot ratios'
/// roughly as many again; `gamma(8 (p + q + 2)) m` per coordinate, times `sqrt 3` for the
/// vector, bounds it with margin (Higham, *Accuracy and Stability of Numerical Algorithms*,
/// 2nd ed., section 3.1).
pub(crate) fn evaluation_slack(k: &Kernel, surface: &Surface) -> f64 {
    let (m, p, q) = net_size(k, surface);
    gamma(8 * (p + q + 2)) * m * 3.0_f64.sqrt()
}

/// [`evaluation_slack`] at the parameters `uv`, which may lie past the net's knots (the
/// evaluation extrapolates the end spans' polynomials): inside the knots the slack itself;
/// past them each direction's de Boor steps take affine rather than convex combinations,
/// with factors at most `1 + 2 d / span` for `d` the distance past the end knot and `span`
/// the end span's length, so the rounding grows by at most that factor to the degree per
/// direction (the growth factor `solidcore`'s own `point_rounding` charges; that bound
/// also has a rational weight term, which this slack does not need: inside the knots
/// `sum N_i w_i |x_i| <= m w(uv)` for `m` the largest Euclidean coordinate, so the
/// slack above bounds a rational net's point too). `None` for a rational net
/// past its knots, whose weight sum is not bounded away from zero there, and for
/// parameters that are not finite: no enclosure is claimed.
pub(crate) fn evaluation_slack_at(k: &Kernel, surface: &Surface, uv: Point2) -> Option<f64> {
    let measured = measured(surface);
    let form = as_b_spline(k, measured).ok()??;
    let nurb = form.gw_nurb()?;
    let grow = extrapolation(nurb.u_knots(), nurb.p(), uv.x)?
        * extrapolation(nurb.v_knots(), nurb.q(), uv.y)?;
    let rational = nurb.rows().flatten().any(|c| c.w != NOW);
    if rational && grow > 1.0 {
        return None;
    }
    let slack = evaluation_slack(k, surface) * grow;
    slack.is_finite().then_some(slack)
}

/// The growth of a degree-`p` B-spline evaluation's rounding at `t` past `knots`' ends
/// ([`evaluation_slack_at`]): 1 inside, `((1 + 2 d / span)(1 + u_4))^p (1 + gamma(p + 1))`
/// past an end. `None` for `t` not finite or an end span that is empty.
fn extrapolation(knots: &[f64], p: usize, t: f64) -> Option<f64> {
    let lo = *knots.get(p)?;
    let hi = *knots.get(knots.len().checked_sub(p + 1)?)?;
    if !t.is_finite() {
        return None;
    }
    if t >= lo && t <= hi {
        return Some(1.0);
    }
    let (d, span) = if t < lo {
        (lo - t, knots.iter().copied().find(|&x| x > lo)? - lo)
    } else {
        (t - hi, hi - knots.iter().rev().copied().find(|&x| x < hi)?)
    };
    if span <= 0.0 || span.is_nan() {
        return None;
    }
    let ratio = (1.0 + 2.0 * d / span) * (1.0 + gamma(4));
    let grow = ratio.powi(i32::try_from(p).ok()?) * (1.0 + gamma(p + 1));
    grow.is_finite().then_some(grow)
}

/// The enclosure of the distance from `p` to `surface` over `uv_box` (its NURB parameters;
/// `None` for the whole surface) around the drop answer `foot`.
///
/// # Panics
///
/// When the foot cannot be evaluated or the surface cannot be bounded.
pub(crate) fn nearest(
    k: &Kernel,
    surface: &Surface,
    uv_box: Option<&Extent2d>,
    p: Point3,
    foot: Point2,
) -> Enclosure {
    nearest_with_rounding(k, surface, uv_box, p, foot).0
}

/// [`nearest`], with the rounding its upper end adds to the computed distance from `p` to
/// the foot's surface point: the evaluation's slack at the foot and `γ_8` of the distance's
/// coordinates, the two terms counted below (infinite where the evaluation is not bounded).
///
/// # Panics
///
/// As [`nearest`].
pub(crate) fn nearest_with_rounding(
    k: &Kernel,
    surface: &Surface,
    uv_box: Option<&Extent2d>,
    p: Point3,
    foot: Point2,
) -> (Enclosure, f64) {
    let surface = measured(surface);
    let q = surface.evaluate_point(k, foot).expect("the foot");
    // the evaluation's rounding, and the distance's own (a difference per coordinate, three
    // squares, two sums and a root, on coordinates at most the larger point's size)
    // (past a rational net's knots the evaluation is not bounded: no upper end)
    let slack = evaluation_slack_at(k, surface, foot).unwrap_or(f64::INFINITY);
    let distance = gamma(8) * p.max_dimension().max(q.max_dimension()) * 3.0_f64.sqrt();
    let hi = p.distance(q) + slack + distance;
    // ask for the foot's distance: the subdivision stops once the nearest piece is that far
    let lo = surface_distance_lower_bound(k, surface, uv_box, p, p.distance(q))
        .expect("the lower bound");
    (Enclosure { lo, hi }, round_up(slack + distance))
}

/// The enclosure of a sampled maximum gap from a curve to `surface`: the largest of the true
/// distances from the samples' curve points to the surface. Each sample is enclosed around
/// its own foot ([`nearest`]); the maximum of the true distances lies between the largest
/// lower end and the largest upper end, and so does the maximum of the samples' own
/// distances (each at least its true distance, at most its upper end). Each sample's
/// surface point is checked to be the surface's point at its parameters.
///
/// # Panics
///
/// When a sample's surface point is not the surface's point at its parameters, or a sample
/// cannot be enclosed.
pub(crate) fn sampled_max(
    k: &Kernel,
    surface: &Surface,
    uv_box: Option<&Extent2d>,
    samples: &[solidcore::heal::gap::GapSample],
) -> Enclosure {
    let slack = evaluation_slack(k, surface);
    let mut max = Enclosure {
        lo: f64::NEG_INFINITY,
        hi: f64::NEG_INFINITY,
    };
    for (i, s) in samples.iter().enumerate() {
        let uv = s.other_param();
        let at = surface.evaluate_point(k, uv).expect("the foot");
        assert!(
            at.distance(s.other_pos()) <= slack,
            "sample {i}: surface point {:?} is not the surface's {at:?}",
            s.other_pos()
        );
        let e = nearest(k, surface, uv_box, s.this_pos(), uv);
        assert!(e.lo <= e.hi, "sample {i}: {e:?}");
        max.lo = max.lo.max(e.lo);
        max.hi = max.hi.max(e.hi);
    }
    assert!(max.lo.is_finite() && max.hi.is_finite(), "no samples");
    max
}

/// The alignment `A = w / |w| . dir` of the derivative `w` across the singular direction
/// (the `u` derivative where `singular_u`, else the `v` one) at `uv` with the direction,
/// and a bound on its rounding (WR-57, #17856): `w` and its error `e` come from the
/// certified partials of the surface's NURBS form
/// ([`solidcore::eval::collapse::certified_partials`]); the unit vector moves by at most
/// `2 e / |w|` (`|a / |a| - b / |b|| <= 2 |a - b| / |a|`), and its own computation and the
/// dot product round by at most `2 gamma(16)`. `None` where `2 e >= |w|` (the tangent is
/// not enclosed away from zero), or the surface has no NURBS form or its partials no bound.
pub(crate) fn alignment(
    k: &Kernel,
    surface: &Surface,
    uv: Point2,
    dir: solidcore::math::Vec3,
    singular_u: bool,
) -> Option<(f64, f64)> {
    use solidcore::eval::collapse::certified_partials;
    use solidcore::eval::spline::EvalSurface;
    use solidcore::nurbs::math::NOZ;
    let net = as_b_spline(k, measured(surface)).ok()??;
    let nurb = net.gw_nurb()?;
    let cp = nurb.cpts();
    let (nu, nv) = (cp.len(), cp.first()?.len());
    let mut ctrl = Vec::new();
    for j in 0..nv {
        for row in &cp {
            let c = *row.get(j)?;
            let w = if c.w == NOW { 1.0 } else { c.w };
            let z = if c.z == NOZ { 0.0 } else { c.z };
            ctrl.push([c.x, c.y, z, w]);
        }
    }
    let eval = EvalSurface::nurbs_closed(
        [nurb.p(), nurb.q()],
        [nurb.u_knots().to_vec(), nurb.v_knots().to_vec()],
        [nu, nv],
        ctrl,
        [false; 2],
    );
    let (d, e) = certified_partials(&eval, [uv.x, uv.y]).ok()?;
    let (w, ew) = if singular_u {
        (d[0][1], e[0][1])
    } else {
        (d[1][0], e[1][0])
    };
    let len = w.length();
    if 2.0 * ew >= len || !len.is_finite() {
        return None;
    }
    let a = (w / len).dot(dir);
    Some((a, 2.0 * ew / len + 2.0 * gamma(16)))
}

/// The certified distance from `p` to `surface`'s point at `uv`: `[lo, hi]`, the
/// evaluation's rounding ([`evaluation_slack_at`], grown past the knots) and the distance's
/// own ([`solidcore::geom::certified::distance`]) charged; `None` where the evaluation fails
/// or is not bounded (a rational net past its knots).
pub(crate) fn distance(k: &Kernel, surface: &Surface, p: Point3, uv: Point2) -> Option<(f64, f64)> {
    let q = surface.evaluate_point(k, uv).ok()?;
    let d = solidcore::geom::certified::distance(q, p);
    let slack = evaluation_slack_at(k, surface, uv)?;
    let lo = d.value - d.bound - slack;
    let hi = d.value + d.bound + slack;
    (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
}

/// A lower bound on the distance from `p` to `surface` over its natural domain, as the
/// least of [`surface_distance_lower_bound`] over a grid of boxes covering it, each
/// subdivided on its own (asked for `want`): 4, 16 and then 64 boxes a side, stopping once
/// the bound is within `1e-6` relative of `want`. `None` where the domain or a box's bound
/// is not available.
pub(crate) fn split_lower_bound(
    k: &Kernel,
    surface: &Surface,
    p: Point3,
    want: f64,
) -> Option<f64> {
    let nat = surface.natural_uv_domain_with_nurb(k).ok()?;
    let (u0, v0, u1, v1) = (nat.min().x, nat.min().y, nat.max().x, nat.max().y);
    let mut lo = 0.0;
    for n in [4_u32, 16, 64] {
        let at = |a: f64, b: f64, i: u32| {
            if i == n {
                b
            } else {
                a + (b - a) * f64::from(i) / f64::from(n)
            }
        };
        lo = f64::INFINITY;
        for iu in 0..n {
            for iv in 0..n {
                let b = Extent2d::from_points(
                    Point2::new(at(u0, u1, iu), at(v0, v1, iv)),
                    Point2::new(at(u0, u1, iu + 1), at(v0, v1, iv + 1)),
                );
                lo = lo.min(surface_distance_lower_bound(k, surface, Some(&b), p, want).ok()?);
            }
        }
        if want - lo <= 1e-6 * want {
            break;
        }
    }
    Some(lo)
}

thread_local! {
    static KEPT: std::cell::RefCell<Option<solidcore::topology::Brep>> =
        const { std::cell::RefCell::new(None) };
}

/// Keeps a copy of `b` (a runner's result B-rep) for [`take_kept`].
///
/// # Panics
///
/// When the B-rep cannot be copied.
pub(crate) fn keep(b: &solidcore::topology::Brep) {
    let copy = b.clone_standing_for_self().expect("a copy of the B-rep");
    KEPT.with(|k| *k.borrow_mut() = Some(copy));
}

/// The B-rep [`keep`] last kept on this thread, and forgets it.
pub(crate) fn take_kept() -> Option<solidcore::topology::Brep> {
    KEPT.with(|k| k.borrow_mut().take())
}

/// One edge's tolerance and what it must cover: the largest certified upper end of its
/// uses' sampled edge-face gaps ([`sampled_max`] over `CalcMaxEdgeFaceGap`'s 20-sample gap
/// function) and their largest lower end.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EdgeCover {
    /// The edge.
    pub(crate) edge: solidcore::topology::EdgeId,
    /// Its tolerance.
    pub(crate) tolerance: f64,
    /// The largest lower end over its uses: a tolerance under it cannot cover the edge.
    pub(crate) needed_lo: f64,
    /// The largest upper end over its uses: a tolerance at or over it covers the edge's
    /// sampled gaps.
    pub(crate) needed_hi: f64,
}

/// One vertex's tolerance and the largest distance from its point to the ends of its
/// edges' curves (each computed in floating point: the slack is the rounding of a
/// difference and a norm of coordinates at most the larger point's size).
#[derive(Debug, Clone, Copy)]
pub(crate) struct VertexCover {
    /// The vertex.
    pub(crate) vertex: solidcore::topology::VertexId,
    /// Its tolerance.
    pub(crate) tolerance: f64,
    /// The largest end distance.
    pub(crate) distance: f64,
    /// The rounding of that distance.
    pub(crate) slack: f64,
}

/// Every edge of `b` with a curve and its uses' face surfaces ([`EdgeCover`]), and every
/// vertex of an edge ([`VertexCover`]).
///
/// # Panics
///
/// When the B-rep's entities cannot be read or a gap cannot be enclosed.
pub(crate) fn tolerance_covers(
    k: &Kernel,
    b: &solidcore::topology::Brep,
) -> (Vec<EdgeCover>, Vec<VertexCover>) {
    use solidcore::geom::tol::{context_xsect_tol_3d, SmallTopoType};
    use solidcore::heal::gap::GapFunction;
    use solidcore::topology::edgeuse::p1 as eu1;
    let tol = context_xsect_tol_3d(k, None, SmallTopoType::NormalSize);
    let mut edges = Vec::new();
    let mut ends: Vec<(solidcore::topology::VertexId, Point3)> = Vec::new();
    for e in solidcore::topology::brep::p1::edges(b, None).expect("edges") {
        let rec = b.edge(e).expect("edge");
        let Some(curve) = rec.curve.as_ref() else {
            continue;
        };
        let (v0, v1) = solidcore::topology::edge::start_end_vertices(b, e).expect("ends");
        for (v, t) in [
            (v0, rec.interval.evaluate(0.0)),
            (v1, rec.interval.evaluate(1.0)),
        ] {
            if let Some(v) = v {
                ends.push((v, curve.evaluate_point(k, t).expect("end point")));
            }
        }
        let mut cover = EdgeCover {
            edge: e,
            tolerance: rec.tolerance,
            needed_lo: 0.0,
            needed_hi: 0.0,
        };
        for &eu in &rec.edgeuses {
            let Some(face) = eu1::face(b, eu).expect("face") else {
                continue;
            };
            let Some(surface) = b.face(face).expect("face").surface.as_ref() else {
                continue;
            };
            let mut gf = GapFunction::crv_srf(tol, curve, &rec.interval, surface, 20);
            gf.max_gap_sample(k, false).expect("samples");
            let enc = sampled_max(k, surface, None, gf.samples());
            cover.needed_lo = cover.needed_lo.max(enc.lo);
            cover.needed_hi = cover.needed_hi.max(enc.hi);
        }
        edges.push(cover);
    }
    let mut vertices: Vec<VertexCover> = Vec::new();
    for (v, at) in ends {
        let rec = b.vertex(v).expect("vertex");
        let d = rec.point.distance(at);
        let size = rec.point.max_dimension().max(at.max_dimension());
        // a difference per coordinate, three squares, two sums and a root
        let slack = gamma(8) * size * 3.0_f64.sqrt();
        match vertices.iter_mut().find(|c| c.vertex == v) {
            Some(c) => {
                if d > c.distance {
                    c.distance = d;
                    c.slack = slack;
                }
            }
            None => vertices.push(VertexCover {
                vertex: v,
                tolerance: rec.tolerance,
                distance: d,
                slack,
            }),
        }
    }
    (edges, vertices)
}

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
    let surface = measured(surface);
    let q = surface.evaluate_point(k, foot).expect("the foot");
    // the evaluation's rounding, and the distance's own (a difference per coordinate, three
    // squares, two sums and a root, on coordinates at most the larger point's size)
    let hi = p.distance(q)
        + evaluation_slack(k, surface)
        + gamma(8) * p.max_dimension().max(q.max_dimension()) * 3.0_f64.sqrt();
    // ask for the foot's distance: the subdivision stops once the nearest piece is that far
    let lo = surface_distance_lower_bound(k, surface, uv_box, p, p.distance(q))
        .expect("the lower bound");
    Enclosure { lo, hi }
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

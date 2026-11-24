//! Not upstream: fitting an intersection edge's ends to its proto vertices before
//! `SmEdgeDefinition::Imprint` makes the edge (the SolidCraft fork's changes).
//!
//! A cylinder minus a screw-swept thread groove crosses the shaft's rim at a grazing angle
//! (3 degrees for an M33x3.5 external or an M10x1.5 internal thread), so where the groove
//! flank's curve meets the rim is only known along the rim to the tolerance over the
//! angle's sine. The proto vertex there comes from another intersection and lies a few
//! tolerances off the flank/end-face curve's end: inside the curve (the edge then runs
//! past its vertex along the rim, "Face has partially Coincident Edges", `SmFace` test #34,
//! tangency-labs/solidcore#435) or past it (the edge stops short, and the gap goes into
//! the vertex tolerance, #410).
//!
//! | Rust | Fork |
//! |------|------|
//! | [`ProtoTopologyManager::fit_edge_def_ends_to_proto_vertices`] | the block at the top of `SmEdgeDefinition::Imprint` (tangency-labs/smlib b43876f, a8caccd, ffaf4e8) |
//! | `trim_end_to_proto_vertex` | b43876f: `Imprint` moves an interval end to the curve's point nearest its proto vertex |
//! | `extend_end_to_proto_vertex` | a8caccd: `SmEdgeDefinition::ExtendEndToProtoVertex` |
//! | `extended_curve` | a8caccd: `sm_CreateExactlyExtendedCurve` |
//! | `analytic_of_continued_nurb` | bec8a0b: `sm_AnalyticOfContinuedNurb` |
//! | `ieee_remainder` (private) | `std::remainder` |
//!
//! Deviations: the fork's `GetOwner() != NULL` checks have no counterpart (curves live in
//! the owner's store behind handles, none of them owned by an edge yet); replaced curves
//! stay in that store, as the curves `ProtoTopologyManager::proto_edge_split` replaces do.
//! WR-452 (trim) and WR-453 (extend) are always on (upstream has neither; the WR-453
//! flag `imprint_extends_edge_to_proto_vertex` was retired by #14246); both carry the fork's
//! correction ffaf4e8 (WR-498, #1480, #470): an end goes to its proto vertex's position
//! only when the proto vertex is one vertex in each B-rep.
//! The fork's `sm_CreateExactlyExtendedCurve` (a8caccd) continues any `SmBSplineCurve`
//! kind and makes the analytic ones plain B-splines; `extended_curve` continues a plain
//! B-spline only. Its correction bec8a0b (WR-601) continues a line, circle or ellipse as
//! that kind and no other analytic kind, so the port is the fork at bec8a0b.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // the fork's NaN-safe tests

use super::{curve_of, dangling, ignore_soft, EdgeDefId, ProtoBrep, ProtoTopologyManager};
use super::{ProtoVertexId, Result};
use crate::curve::curve::p2::{Curve, CurveKind};
use crate::error::{Failure, Reason, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::{SS_UMAX, SS_UMIN, SS_VMAX, SS_VMIN};
use crate::geom::global_solver::SolverOperation;
use crate::geom::tol::are_same;
use crate::intersect::point_class::{ClassObject, ClassObjectKind, PointClassType};
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, EFF_ZERO};
use crate::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::surface::surface::p4::NoObject;

const OP: &str = "topology::proto_topology::p2::imprint_ends";

/// How far ahead of a curve's end, as a fraction of the interval, a proto vertex may lie
/// for the curve to be continued to it.
const MAX_EXTENSION_FRACTION: f64 = 0.05;

/// Not upstream (#3062): a quadric's equation about a point `o`, `f(x) = y.M.y + 2 m.y + c`
/// with `y = x - o`, and how a bound of `|f|` bounds the distance from the surface
/// ([`QuadricKind`]).
struct Quadric {
    o: Vec3,
    m: [[f64; 3]; 3],
    lin: Vec3,
    c: f64,
    kind: QuadricKind,
}

/// A quadric's kind, for its distance bound: a plane's `|f|` (its normal unit); a sphere's
/// and a cylinder's or cone's from `f = (d - R)(d + R)`, `d` the distance from the centre or
/// the axis and `R` the radius (a cone's least along the span), by [`radial_off`], a cone's
/// then over `sqrt(1 + tan^2)`, as its surface is that far from a point `d - R` off it
/// radially.
enum QuadricKind {
    Plane,
    Sphere {
        radius: f64,
    },
    Cone {
        axis: Vec3,
        radius: f64,
        tan_angle: f64,
    },
}

/// The quadric of a plane, cylinder, cone or sphere; `None` for any other surface.
fn quadric_of(surface: &crate::surface::surface::Surface) -> Option<Quadric> {
    use crate::surface::implicit::{AnalyticSupport, Implicit};
    Some(match AnalyticSupport::of(surface)?.implicit() {
        Implicit::Plane { origin, normal } => Quadric {
            o: origin,
            m: [[0.0; 3]; 3],
            lin: normal * 0.5,
            c: 0.0,
            kind: QuadricKind::Plane,
        },
        Implicit::Sphere { centre, radius } => Quadric {
            o: centre,
            m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            lin: Vec3::ZERO,
            c: -radius * radius,
            kind: QuadricKind::Sphere { radius },
        },
        // |y|^2 - (y.a)^2 - (r + t y.a)^2
        Implicit::Cone {
            origin,
            axis,
            radius,
            tan_angle,
        } => {
            let a = [axis.x, axis.y, axis.z];
            let s = 1.0 + tan_angle * tan_angle;
            let mut m = [[0.0; 3]; 3];
            for (row, ai) in m.iter_mut().zip(a) {
                for (x, aj) in row.iter_mut().zip(a) {
                    *x = -s * ai * aj;
                }
            }
            for (i, row) in m.iter_mut().enumerate() {
                if let Some(x) = row.get_mut(i) {
                    *x += 1.0;
                }
            }
            Quadric {
                o: origin,
                m,
                lin: axis * (-radius * tan_angle),
                c: -radius * radius,
                kind: QuadricKind::Cone {
                    axis,
                    radius,
                    tan_angle,
                },
            }
        }
        Implicit::Torus { .. } => return None,
    })
}

/// `C(n, k)`.
fn binomial(n: usize, k: usize) -> f64 {
    let mut r = 1.0_f64;
    for i in 0..k {
        #[allow(clippy::cast_precision_loss)]
        let f = (n - i) as f64 / (i + 1) as f64;
        r *= f;
    }
    r
}

/// The product of two polynomials in Bernstein form over `[0, 1]`, of degrees
/// `a.len() - 1` and `b.len() - 1`.
fn bernstein_product(a: &[f64], b: &[f64]) -> Vec<f64> {
    let (p, q) = (a.len().saturating_sub(1), b.len().saturating_sub(1));
    let mut c = vec![0.0; p + q + 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            if let Some(slot) = c.get_mut(i + j) {
                *slot += binomial(p, i) * binomial(q, j) / binomial(p + q, i + j) * x * y;
            }
        }
    }
    c
}

/// De Casteljau's split of homogeneous control points at `u`: the part over `[u, 1]` when
/// `upper`, else over `[0, u]`.
fn de_casteljau(pts: &[[f64; 4]], u: f64, upper: bool) -> Vec<[f64; 4]> {
    let mut row = pts.to_vec();
    let mut lower = Vec::new();
    let mut higher = Vec::new();
    while let (Some(first), Some(last)) = (row.first().copied(), row.last().copied()) {
        lower.push(first);
        higher.push(last);
        if row.len() == 1 {
            break;
        }
        row = row
            .windows(2)
            .map(|w| {
                let p = w.first().copied().unwrap_or_default();
                let q = w.get(1).copied().unwrap_or_default();
                [
                    p[0] + u * (q[0] - p[0]),
                    p[1] + u * (q[1] - p[1]),
                    p[2] + u * (q[2] - p[2]),
                    p[3] + u * (q[3] - p[3]),
                ]
            })
            .collect();
    }
    if upper {
        higher.reverse();
        higher
    } else {
        lower
    }
}

/// Not upstream (#20691): a bound of `|d - r|` from `|f| <= f_over` where
/// `f = (d - r)(d + r)`, `d >= 0`, `r >= r_min > 0`: `|d - r| = |f| / (d + r)` and
/// `d + r >= max(r_min, 2 r_min - |d - r|)`, so `|d - r| (2 r_min - |d - r|) <= f_over`.
/// Where `f_over < r_min^2` that is `|d - r| <= f_over / (r_min + sqrt(r_min^2 - f_over))`
/// (the other root exceeds `r_min`, where `|f| >= |d - r| r_min > r_min^2`), about
/// `f_over / 2 r_min`; else `f_over / r_min`. Returns the bound and whether
/// `|d - r| < r_min`.
///
/// Rounding (#20854). The computed `disc = fl(fl(r_min^2) - f_over)` is within
/// `gamma(2) (r_min^2 + |disc|)` of `r_min^2 - f_over`, an error `sqrt` would amplify where
/// `disc` is near zero; `below`, `disc` less `gamma(4)` of that (enough to cover the
/// rounding of `below`'s own two operations and its subtraction), is at most
/// `r_min^2 - f_over`, so its root, and the bound with it, can only err upward. The root,
/// the sum, the quotient and the product each round by at most `u`, which the raise by
/// `gamma(6)` (itself rounded) covers. Where `below` is not positive, `f_over / r_min`.
fn radial_off(f_over: f64, r_min: f64) -> (f64, bool) {
    use crate::nurbs::math::gamma;
    let r2 = r_min * r_min;
    let disc = r2 - f_over;
    let below = disc - gamma(4) * (r2 + disc.abs());
    let raise = 1.0 + gamma(6);
    if below > 0.0 {
        (f_over / (r_min + below.sqrt()) * raise, true)
    } else {
        (f_over / r_min * raise, false)
    }
}

/// Not upstream (#3062): a certified bound of how far `curve` over `[from, to]` (inside one
/// of its spans) lies from each of `surfaces` (planes, cylinders, cones, spheres), with no
/// sampling. The span's Bézier form, its homogeneous control points `(X, W)` cut to the
/// interval by de Casteljau, composes with each quadric to the polynomial
/// `F = Y.M.Y + 2 m.Y W + c W^2`, `Y = X - o W`, in Bernstein form. `|f| = |F| / W^2` is at
/// most the largest coefficient's size, with their rounding (`gamma`), over the least weight
/// coefficient squared, and the distance follows ([`QuadricKind`]). `None` where a surface
/// is of another kind, the curve is no B-spline, a weight coefficient is not positive, or a
/// cone's radius along the span is not bounded away from zero.
fn span_off_surfaces(
    curve: &Curve,
    from: f64,
    to: f64,
    surfaces: [&crate::surface::surface::Surface; 2],
) -> Option<f64> {
    let CurveKind::BSpline(bs) = curve.kind() else {
        return None;
    };
    let (lo, hi) = (from.min(to), from.max(to));
    let segments = bs.gw_nurb()?.decompose_bez().ok()?;
    let segment = segments.iter().find(|seg| {
        let kn = seg.knots();
        kn.first()
            .is_some_and(|a| *a <= lo + EFF_ZERO * (1.0 + lo.abs()))
            && kn
                .last()
                .is_some_and(|b| *b >= hi - EFF_ZERO * (1.0 + hi.abs()))
    })?;
    let (a, b) = (*segment.knots().first()?, *segment.knots().last()?);
    if !(b > a) {
        return None;
    }
    let mut pts: Vec<[f64; 4]> = segment
        .cpts()
        .iter()
        .map(|c| {
            if c.w == crate::nurbs::math::NOW {
                [c.x, c.y, c.z, 1.0]
            } else {
                [c.x, c.y, c.z, c.w]
            }
        })
        .collect();
    let (u0, u1) = ((lo - a) / (b - a), (hi - a) / (b - a));
    pts = de_casteljau(&pts, u0.clamp(0.0, 1.0), true);
    let u = if u0 < 1.0 {
        ((u1 - u0) / (1.0 - u0)).clamp(0.0, 1.0)
    } else {
        1.0
    };
    pts = de_casteljau(&pts, u, false);
    let w: Vec<f64> = pts.iter().map(|p| p[3]).collect();
    let w_min = w.iter().copied().fold(f64::INFINITY, f64::min);
    if !(w_min > 0.0) {
        return None;
    }
    let n = pts.len().saturating_sub(1);
    let mut worst = 0.0_f64;
    for surface in surfaces {
        let q = quadric_of(surface)?;
        let y: Vec<[f64; 3]> = pts
            .iter()
            .map(|p| {
                [
                    p[0] - q.o.x * p[3],
                    p[1] - q.o.y * p[3],
                    p[2] - q.o.z * p[3],
                ]
            })
            .collect();
        let comp =
            |i: usize| -> Vec<f64> { y.iter().map(|v| v.get(i).copied().unwrap_or(0.0)).collect() };
        let mut f = vec![0.0; 2 * n + 1];
        let mut f_abs = vec![0.0; 2 * n + 1];
        let mut add = |terms: Vec<f64>, factor: f64| {
            for ((slot, abs), t) in f.iter_mut().zip(f_abs.iter_mut()).zip(terms) {
                *slot += factor * t;
                *abs += (factor * t).abs();
            }
        };
        for (i, row) in q.m.iter().enumerate() {
            for (j, mij) in row.iter().enumerate() {
                if *mij != 0.0 {
                    add(bernstein_product(&comp(i), &comp(j)), *mij);
                }
            }
        }
        for (i, li) in [q.lin.x, q.lin.y, q.lin.z].iter().enumerate() {
            if *li != 0.0 {
                add(bernstein_product(&comp(i), &w), 2.0 * li);
            }
        }
        if q.c != 0.0 {
            add(bernstein_product(&w, &w), q.c);
        }
        let rounding =
            crate::nurbs::math::gamma(8 * (n + 1) + 16) * f_abs.iter().copied().fold(0.0, f64::max);
        let f_bound = f.iter().fold(0.0_f64, |m, x| m.max(x.abs())) + rounding;
        let f_over = f_bound / (w_min * w_min);
        let off = match q.kind {
            QuadricKind::Plane => f_over,
            QuadricKind::Sphere { radius } => radial_off(f_over, radius).0,
            QuadricKind::Cone {
                axis,
                radius,
                tan_angle,
            } => {
                // the radius along the span, `r + t (x - o).a`, at least its least over the
                // Euclidean control points (the span is their convex combination)
                let along =
                    |v: &[f64; 3], wi: f64| (v[0] * axis.x + v[1] * axis.y + v[2] * axis.z) / wi;
                let mut h_lo = f64::INFINITY;
                let mut h_hi = f64::NEG_INFINITY;
                for (v, wi) in y.iter().zip(&w) {
                    let h = along(v, *wi);
                    h_lo = h_lo.min(h);
                    h_hi = h_hi.max(h);
                }
                let r_min = (radius + tan_angle * h_lo).min(radius + tan_angle * h_hi);
                if !(r_min > 0.0) {
                    return None;
                }
                // Within `r_min` of the axis's distance, the point's foot on its generator
                // line in its half plane through the axis lies on the cone, not past its apex.
                match radial_off(f_over, r_min) {
                    (radial, true) => radial / tan_angle.mul_add(tan_angle, 1.0).sqrt(),
                    (radial, false) => radial,
                }
            }
        };
        worst = worst.max(off);
    }
    Some(worst)
}

/// `ANALYTIC_TOL_SCALE` (`SmConfig.h`).
const ANALYTIC_TOL_SCALE: f64 = 100.0;

impl<O: ClassObject> ProtoTopologyManager<O> {
    /// The block at the top of the fork's `SmEdgeDefinition::Imprint`: for each end of the
    /// definition's interval whose curve point is off its proto vertex, continue the
    /// curves to the vertex (WR-453) or else move the end
    /// to the curve's point nearest the vertex (WR-452).
    /// Nothing changes for a closed proto edge or where no check passes.
    ///
    /// The point an end goes to (ffaf4e8, WR-498) is the proto vertex's position when
    /// every vertex definition in it was imprinted onto `final_vertices` (the end's
    /// final vertex in B-rep 1 and in B-rep 2); otherwise it is B-rep 1's final vertex
    /// when the two final vertices lie within the definition's tolerance, and the end is
    /// left alone if not. A proto vertex that `Resolve` made several vertices of sits at
    /// their mean, which is not the vertex an end belongs to.
    ///
    /// # Errors
    ///
    /// [`crate::error::SolidError::InvalidInput`] for deleted objects or unknown handles;
    /// [`crate::error::SolidError::Unported`] from the curve evaluations.
    pub(super) fn fit_edge_def_ends_to_proto_vertices(
        &mut self,
        k: &Kernel,
        brep: &mut impl ProtoBrep<O>,
        ed: EdgeDefId,
        start_pv: ProtoVertexId,
        end_pv: ProtoVertexId,
        final_vertices: [(O, O); 2],
    ) -> Result<()> {
        if start_pv == end_pv {
            return Ok(());
        }
        for (at_end, (vtx1, vtx2)) in [false, true].into_iter().zip(final_vertices) {
            let pv = if at_end { end_pv } else { start_pv };
            let proto = self
                .proto_vertex(pv)
                .ok_or(dangling("proto vertex is deleted"))?;
            let mut one_vertex = true;
            for &vd in proto.vertex_definitions() {
                if !one_vertex {
                    break;
                }
                // A deleted definition is no vertex (the fork reads a live pointer).
                one_vertex = self.vertex_def(vd).is_some_and(|v| {
                    v.final_vertex(1) == Some(vtx1) && v.final_vertex(2) == Some(vtx2)
                });
            }
            let pv_tol = proto.raw_tolerance();
            let pv_pos = if one_vertex {
                proto.position()
            } else {
                let p1 = brep.vertex_point(vtx1)?;
                let p2 = brep.vertex_point(vtx2)?;
                if !(p1.distance(p2) <= self.ed_ref(ed)?.raw_tolerance()) {
                    continue;
                }
                p1
            };
            let d = self.ed_ref(ed)?;
            let interval = d.xsect_interval();
            let end_param = if at_end {
                interval.max()
            } else {
                interval.min()
            };
            let Some(end_pt) =
                ignore_soft(curve_of(&*brep, d.xsect_curve_3d())?.evaluate_point(k, end_param))?
            else {
                continue;
            };
            let gap = end_pt.distance(pv_pos);
            if !(gap > EFF_ZERO * (1.0 + end_pt.max_dimension())) {
                continue;
            }
            if one_vertex && self.move_end_to_pole(k, brep, ed, at_end, pv_pos, gap, pv_tol)? {
                continue;
            }
            if self.extend_end_to_proto_vertex(k, brep, ed, at_end, pv_pos, end_pt, gap)? {
                continue;
            }
            self.trim_end_to_proto_vertex(k, brep, ed, at_end, pv_pos, gap)?;
        }
        Ok(())
    }

    /// Not upstream (#16637): moves an end that stops short of its proto vertex onto it
    /// where that vertex is a pole of one of the definition's faces, by replacing the end
    /// control point of the 3d curve with the vertex's position and that of each UV curve
    /// with the vertex's parameters: on the face whose surface collapses there, the end's
    /// parameter along the degenerate side with the side's own across it (the end on the
    /// pole, inside the domain); on the other face, the vertex's foot (a local point solve
    /// from the end's parameters, within the vertex's tolerance of it).
    ///
    /// It applies when the gap (the distance from the evaluated end to the vertex,
    /// computed here) is within the proto vertex's tolerance. For a vertex that combined
    /// definitions away, that tolerance reaches the computed position and object points of
    /// each of them, and the curve ends attached when it combined them, from where the
    /// vertex now is (`ProtoVertex::set_position`); it is not a bound on where any other
    /// end lies, which is why the gap is measured. Each curve is
    /// clamped, so its end is its end control point, and moving that point by `Δ` moves
    /// every point of the curve by at most `|Δ|` (the basis functions, rational ones with
    /// positive weights included, are a partition of unity, each at most 1): the 3d curve
    /// stays within the gap of where it
    /// was, ARGUED (not asserted in code). A continuation past the end instead
    /// (`extend_end_to_proto_vertex`) follows the end span's polynomial, which near a pole
    /// runs out of the face's UV domain. Returns whether the curves were replaced.
    #[allow(clippy::too_many_arguments)]
    fn move_end_to_pole(
        &mut self,
        k: &Kernel,
        brep: &mut impl ProtoBrep<O>,
        ed: EdgeDefId,
        at_end: bool,
        pv_pos: Point3,
        gap: f64,
        pv_tol: f64,
    ) -> Result<bool> {
        // (written to accept: a NaN gap or tolerance moves nothing)
        if !(gap <= pv_tol) {
            return Ok(false);
        }
        let d = self.ed_ref(ed)?;
        let interval = d.xsect_interval();
        let end_param = if at_end {
            interval.max()
        } else {
            interval.min()
        };
        let curve = curve_of(&*brep, d.xsect_curve_3d())?;
        let natural = curve.natural_interval();
        if !are_same(
            end_param,
            if at_end { natural.max() } else { natural.min() },
        ) {
            return Ok(false);
        }
        let zero = EFF_ZERO * (1.0 + pv_pos.max_dimension());
        let mut new_uv: [Option<Curve>; 2] = [None, None];
        let mut on_pole = false;
        for (slot, which) in new_uv.iter_mut().zip([1, 2]) {
            let Some(h) = d.xsect_uv_curve(which) else {
                continue;
            };
            let uv_curve = curve_of(&*brep, Some(h))?;
            // The UV curve's end control point is its point at `end_param` only where its
            // natural interval ends there too.
            let uv_natural = uv_curve.natural_interval();
            if !are_same(
                end_param,
                if at_end {
                    uv_natural.max()
                } else {
                    uv_natural.min()
                },
            ) {
                return Ok(false);
            }
            let face = d
                .src_topo(which)
                .filter(|_| d.src_topo_type(which) == PointClassType::Face)
                .filter(|o| o.class_kind() == ClassObjectKind::Face);
            let Some(surface) = face
                .and_then(|f| brep.face_surface(f))
                .flatten()
                .and_then(|s| brep.surface(s))
            else {
                return Ok(false);
            };
            let Some(e) = ignore_soft(uv_curve.evaluate_point(k, end_param))? else {
                return Ok(false);
            };
            let Some(dom) = ignore_soft(surface.natural_uv_domain_with_nurb(k))? else {
                return Ok(false);
            };
            let sides =
                ignore_soft(surface.get_singularities(k, false, false))?.map_or(0, |r| r.singular);
            // the end on each pole side of this surface
            let mut target = None;
            for (bit, uv) in [
                (SS_UMIN, Point2::new(dom.min().x, e.y)),
                (SS_UMAX, Point2::new(dom.max().x, e.y)),
                (SS_VMIN, Point2::new(e.x, dom.min().y)),
                (SS_VMAX, Point2::new(e.x, dom.max().y)),
            ] {
                if sides & bit == 0 || target.is_some() {
                    continue;
                }
                let Some(p) = ignore_soft(surface.evaluate_point(k, uv))? else {
                    continue;
                };
                if p.distance(pv_pos) <= zero {
                    target = Some(uv);
                }
            }
            let uv = if let Some(uv) = target {
                on_pole = true;
                uv
            } else {
                let solved = ignore_soft(surface.local_point_solve::<NoObject>(
                    k,
                    &dom,
                    SolverOperation::Minimize,
                    pv_pos,
                    Point2::new(e.x, e.y),
                ))?;
                let Some(r) = solved.filter(|r| r.found) else {
                    return Ok(false);
                };
                let (Ok(u), Ok(v)) = (r.solution.start.param(0), r.solution.start.param(1)) else {
                    return Ok(false);
                };
                let uv = Point2::new(u, v);
                let Some(p) = ignore_soft(surface.evaluate_point(k, uv))? else {
                    return Ok(false);
                };
                if !(p.distance(pv_pos) <= pv_tol) {
                    return Ok(false);
                }
                uv
            };
            let Some(c) = curve_with_end_point(uv_curve, at_end, Vec3::new(uv.x, uv.y, 0.0)) else {
                return Ok(false);
            };
            *slot = Some(c);
        }
        if !on_pole {
            return Ok(false);
        }
        let Some(new_3d) = curve_with_end_point(curve, at_end, pv_pos) else {
            return Ok(false);
        };
        self.install_end_fit(brep, ed, Some(new_3d), new_uv, interval)?;
        Ok(true)
    }

    /// WR-452 (b43876f): moves the interval end to the curve's point nearest `pv_pos`
    /// when that point is inside the interval, nearer than the end and within the
    /// definition's tolerance; the UV curves are trimmed with it, on copies, so a failure
    /// changes nothing. The 3d curve keeps its extent.
    fn trim_end_to_proto_vertex(
        &mut self,
        k: &Kernel,
        brep: &mut impl ProtoBrep<O>,
        ed: EdgeDefId,
        at_end: bool,
        pv_pos: Point3,
        gap: f64,
    ) -> Result<()> {
        let d = self.ed_ref(ed)?;
        let interval = d.xsect_interval();
        let tol = d.raw_tolerance();
        let end_param = if at_end {
            interval.max()
        } else {
            interval.min()
        };
        let curve = curve_of(&*brep, d.xsect_curve_3d())?;
        let Some(dropped) = ignore_soft(curve.drop_point(
            k,
            &interval,
            pv_pos,
            None,
            tol,
            Some(end_param),
            SolverOperation::Minimize,
        ))?
        else {
            return Ok(());
        };
        if !dropped.success
            || !(dropped.distance < gap)
            || !(dropped.distance <= tol)
            || !interval.contains_value(dropped.parameter, -EFF_ZERO)
        {
            return Ok(());
        }
        let param = dropped.parameter;
        let mut trimmed = interval;
        let moved = if at_end {
            trimmed.set_max(param)
        } else {
            trimmed.set_min(param)
        };
        if moved.is_err() && k.fixes.ssi_proto_topology_statuses_pass_on {
            // fork bcc6f01: `SER( sTrimmed.SetMin/SetMax(dParam) )`
            return moved;
        }
        if moved.is_err() || !(trimmed.length() > EFF_ZERO * (1.0 + param.abs())) {
            return Ok(());
        }
        // The UV curves share the 3d curve's parameter and span the edge's interval.
        let mut new_uv: [Option<Curve>; 2] = [None, None];
        for (slot, which) in new_uv.iter_mut().zip([1, 2]) {
            let Some(handle) = d.xsect_uv_curve(which) else {
                continue;
            };
            let Ok(mut copy) = curve_of(&*brep, Some(handle))?.copy(k) else {
                return Ok(());
            };
            let wanted = trimmed;
            if copy.trim(k, &mut trimmed, TrimMode::Notify).is_err()
                || !(are_same(wanted.min(), trimmed.min()) && are_same(wanted.max(), trimmed.max()))
            {
                return Ok(());
            }
            *slot = Some(copy);
        }
        if !(trimmed.length() > EFF_ZERO * (1.0 + param.abs())) {
            return Ok(());
        }
        self.install_end_fit(brep, ed, None, new_uv, trimmed)
    }

    /// WR-453 (a8caccd, `SmEdgeDefinition::ExtendEndToProtoVertex`): continues the 3d
    /// and both UV curves past an end where they stop short of `pv_pos`, each by the
    /// polynomial of its own end span, to the parameter where the continued 3d curve
    /// passes nearest it. Applies where the interval ends where its curves do and the
    /// vertex lies ahead along the tangent within a twentieth of the interval; the new
    /// 3d end must be within the tolerance of the vertex and nearer than the old end,
    /// and each UV curve's new end must lie on its face within the tolerance. Returns
    /// whether the curves and interval were replaced.
    #[allow(clippy::too_many_arguments)] // the fork's signature plus the kernel and B-rep
    fn extend_end_to_proto_vertex(
        &mut self,
        k: &Kernel,
        brep: &mut impl ProtoBrep<O>,
        ed: EdgeDefId,
        at_end: bool,
        pv_pos: Point3,
        end_pt: Point3,
        gap: f64,
    ) -> Result<bool> {
        let d = self.ed_ref(ed)?;
        let interval = d.xsect_interval();
        let tol = d.raw_tolerance();
        let end_param = if at_end {
            interval.max()
        } else {
            interval.min()
        };
        let curve = curve_of(&*brep, d.xsect_curve_3d())?;
        let natural_end = |c: &Curve| {
            let n = c.natural_interval();
            are_same(end_param, if at_end { n.max() } else { n.min() })
        };
        if !natural_end(curve) {
            return Ok(false);
        }
        let uv_handles = [d.xsect_uv_curve(1), d.xsect_uv_curve(2)];
        for h in uv_handles.into_iter().flatten() {
            if !natural_end(curve_of(&*brep, Some(h))?) {
                return Ok(false);
            }
        }

        // The vertex must be ahead of the end along the tangent.
        let Some(tangent) = ignore_soft(curve.evaluate(k, end_param, 1, at_end, false))?
            .and_then(|v| v.get(1).copied())
        else {
            return Ok(false);
        };
        let speed2 = tangent.length_squared();
        if !(speed2 > 0.0) {
            return Ok(false);
        }
        let mut ahead = (pv_pos - end_pt).dot(tangent) / speed2;
        if !at_end {
            ahead = -ahead;
        }
        if !(ahead > EFF_ZERO * (1.0 + end_param.abs()))
            || !(ahead <= MAX_EXTENSION_FRACTION * interval.length())
        {
            return Ok(false);
        }

        // The 3d curve continued twice as far, and the parameter on it nearest the vertex.
        let far = if at_end {
            end_param + 2.0 * ahead
        } else {
            end_param - 2.0 * ahead
        };
        let Some(far_curve) = extended_curve(k, curve, at_end, far) else {
            return Ok(false);
        };
        let mut far_interval = Extent1d::new();
        if far_interval
            .set_min_max(end_param.min(far), end_param.max(far))
            .is_err()
        {
            return Ok(false);
        }
        let guess = if at_end {
            end_param + ahead
        } else {
            end_param - ahead
        };
        let Some(dropped) = ignore_soft(far_curve.drop_point(
            k,
            &far_interval,
            pv_pos,
            None,
            tol,
            Some(guess),
            SolverOperation::Minimize,
        ))?
        else {
            return Ok(false);
        };
        let new_param = dropped.parameter;
        if !dropped.success
            || !(dropped.distance < gap)
            || !(dropped.distance <= tol)
            || !(if at_end {
                new_param > end_param
            } else {
                new_param < end_param
            })
        {
            return Ok(false);
        }

        // All three curves continued to the new end.
        let Some(new_3d) = extended_curve(k, curve, at_end, new_param) else {
            return Ok(false);
        };
        let mut new_uv: [Option<Curve>; 2] = [None, None];
        for (slot, h) in new_uv.iter_mut().zip(uv_handles) {
            let Some(h) = h else { continue };
            match extended_curve(k, curve_of(&*brep, Some(h))?, at_end, new_param) {
                Some(c) => *slot = Some(c),
                None => return Ok(false),
            }
        }

        // Each UV curve's new end must lie on its face's surface at the new 3d end.
        let Some(new_end_pt) = ignore_soft(new_3d.evaluate_point(k, new_param))? else {
            return Ok(false);
        };
        for (uv, which) in new_uv.iter().zip([1, 2]) {
            let Some(uv) = uv else { continue };
            let face = d
                .src_topo(which)
                .filter(|_| d.src_topo_type(which) == PointClassType::Face)
                .filter(|o| o.class_kind() == ClassObjectKind::Face);
            let Some(surface) = face
                .and_then(|f| brep.face_surface(f))
                .flatten()
                .and_then(|s| brep.surface(s))
            else {
                return Ok(false);
            };
            let Some(p) = ignore_soft(uv.evaluate_point(k, new_param))? else {
                return Ok(false);
            };
            let Some(on_face) = ignore_soft(surface.evaluate_point(k, Point2::new(p.x, p.y)))?
            else {
                return Ok(false);
            };
            if !(on_face.distance(new_end_pt) <= tol) {
                return Ok(false);
            }
            // Not upstream (#16637): a continued UV curve whose end leaves its surface's
            // domain across a side the surface collapses on (a pole) is no trim of the face:
            // the end span's polynomial runs past the pole, where no parameter of the face
            // continues it. Not installed: the end is left as it was, as for every other
            // continuation this function declines. A continuation past any other side
            // (across a periodic seam, or a hair past an open side) is left to the checks
            // that follow, as before.
            let Some(dom) = ignore_soft(surface.natural_uv_domain_with_nurb(k))? else {
                return Ok(false);
            };
            let uv_zero = EFF_ZERO * (1.0 + dom.max_dimension());
            let sides =
                ignore_soft(surface.get_singularities(k, false, false))?.map_or(0, |r| r.singular);
            let past = [
                (SS_UMIN, p.x < dom.min().x - uv_zero),
                (SS_UMAX, p.x > dom.max().x + uv_zero),
                (SS_VMIN, p.y < dom.min().y - uv_zero),
                (SS_VMAX, p.y > dom.max().y + uv_zero),
            ];
            if past.iter().any(|&(bit, out)| out && sides & bit != 0) {
                return Ok(false);
            }
        }

        // Not upstream (#3062): where both faces are planes, cylinders, cones or spheres the
        // continued span is certified on both surfaces over its whole length, or the edge is
        // refused, naming how far
        let mut surfaces = Vec::new();
        for which in [1, 2] {
            let surface = d
                .src_topo(which)
                .filter(|_| d.src_topo_type(which) == PointClassType::Face)
                .filter(|o| o.class_kind() == ClassObjectKind::Face)
                .and_then(|f| brep.face_surface(f))
                .flatten()
                .and_then(|s| brep.surface(s));
            surfaces.extend(surface);
        }
        if let [s1, s2] = surfaces[..] {
            if let Some(bound) = span_off_surfaces(&new_3d, end_param, new_param, [s1, s2]) {
                if bound.is_nan() || bound > tol {
                    return Err(SolidError::Failed(Box::new(Failure::new(
                        OP,
                        Reason::EdgeOffItsFaces {
                            gap,
                            bound,
                            tolerance: tol,
                        },
                    ))));
                }
            }
        }

        let mut ivl = interval;
        let set = if at_end {
            ivl.set_max(new_param)
        } else {
            ivl.set_min(new_param)
        };
        if set.is_err() {
            return Ok(false);
        }
        self.install_end_fit(brep, ed, Some(new_3d), new_uv, ivl)?;
        Ok(true)
    }

    /// Replaces the definition's 3d curve (when given), its UV curves (those given) and
    /// its interval.
    fn install_end_fit(
        &mut self,
        brep: &mut impl ProtoBrep<O>,
        ed: EdgeDefId,
        new_3d: Option<Curve>,
        new_uv: [Option<Curve>; 2],
        interval: Extent1d,
    ) -> Result<()> {
        let null = || dangling("curve store is full");
        let h3 = new_3d
            .map(|c| brep.insert_curve(c).ok_or_else(null))
            .transpose()?;
        let mut huv = [None, None];
        for (slot, c) in huv.iter_mut().zip(new_uv) {
            *slot = c
                .map(|c| brep.insert_curve(c).ok_or_else(null))
                .transpose()?;
        }
        let d = self.ed_mut(ed)?;
        if let Some(h) = h3 {
            d.set_xsect_curve_3d(Some(h));
        }
        for (which, h) in [1, 2].into_iter().zip(huv) {
            if let Some(h) = h {
                d.set_xsect_curve_uv(which, Some(h));
            }
        }
        d.set_xsect_interval(interval);
        Ok(())
    }
}

/// Not upstream (#16637): a copy of the clamped B-spline curve `curve` whose end (or
/// start) control point, its end point, is `point` (a rational curve keeps that point's
/// weight). `None` for another curve kind or an unclamped curve.
fn curve_with_end_point(curve: &Curve, at_end: bool, point: Vec3) -> Option<Curve> {
    let CurveKind::BSpline(src) = curve.kind() else {
        return None;
    };
    let degree = src.degree().ok()?;
    let (knots, mults) = src.knots(None).ok()?;
    if degree < 1
        || knots.len() < 2
        || mults.len() != knots.len()
        || mults.first() != Some(&(degree + 1))
        || mults.last() != Some(&(degree + 1))
    {
        return None;
    }
    let (mut pts, wts) = src.control_polygon().ok()?;
    let rational = !wts.is_empty();
    let slot = if at_end { pts.len().checked_sub(1)? } else { 0 };
    *pts.get_mut(slot)? = point;
    let moved = BSplineCurve::create_canonical(&CanonicalData {
        dim: src.dim(),
        degree,
        control_points: &pts,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &mults,
        knots: &knots,
        knot_type: KnotType::Unspecified,
        weights: rational.then_some(wts.as_slice()),
        trim: None,
    })
    .ok()?;
    Some(Curve::from(moved))
}

/// `sm_CreateExactlyExtendedCurve` (a8caccd): a copy of a clamped B-spline curve whose end
/// (or start) span is continued exactly, its polynomial unchanged, to `new_end`, outside
/// the natural interval. The span is made one Bezier segment (its inner knot raised to the
/// degree, which leaves the curve as it is) and that segment's polynomial is taken over
/// the longer interval by de Casteljau's subdivision at a parameter outside `[0, 1]`, in
/// homogeneous coordinates so a rational curve keeps its shape. No control points are
/// added. `None` for another curve kind, an unclamped curve, or a `new_end` that is not
/// outside the natural interval.
///
/// A line, circle or ellipse is continued too (bec8a0b, WR-601), over its NURB, and comes
/// back as that kind ([`analytic_of_continued_nurb`]), or `None` when that fails (as for a
/// parabola or a hyperbola).
fn extended_curve(k: &Kernel, curve: &Curve, at_end: bool, new_end: f64) -> Option<Curve> {
    let src: std::borrow::Cow<'_, BSplineCurve> = match curve.kind() {
        CurveKind::BSpline(src) => std::borrow::Cow::Borrowed(src),
        CurveKind::Line(line) => line.base_with_nurb().ok()?,
        CurveKind::Ellipse(ellipse) => std::borrow::Cow::Borrowed(ellipse.base()),
        _ => return None,
    };
    let natural = src.natural_interval();
    if if at_end {
        !(new_end > natural.max())
    } else {
        !(new_end < natural.min())
    } {
        return None;
    }
    let mut copy = src.copy();
    let bs = &mut copy;
    let degree = bs.degree().ok()?;
    let (mut knots, mut mults) = bs.knots(None).ok()?;
    let mut num = knots.len();
    if degree < 1
        || num < 2
        || mults.len() != num
        || mults.first() != Some(&(degree + 1))
        || mults.last() != Some(&(degree + 1))
        || !are_same(*knots.first()?, natural.min())
        || !are_same(*knots.last()?, natural.max())
    {
        return None;
    }

    // The end span's inner knot, up to the degree: the span is then one Bezier segment.
    let inner = if at_end { num - 2 } else { 1 };
    if inner != 0 && inner != num - 1 && *mults.get(inner)? < degree {
        let insert = vec![*knots.get(inner)?; degree - *mults.get(inner)?];
        bs.insert_knots(&insert).ok()?;
        (knots, mults) = bs.knots(None).ok()?;
        num = knots.len();
        if mults.len() != num || num < 2 {
            return None;
        }
    }

    let (mut pts, wts) = bs.control_polygon().ok()?;
    let rational = !wts.is_empty();
    if pts.len() < degree + 1 || (rational && wts.len() != pts.len()) {
        return None;
    }
    // The segment over [a, b] in Bezier form, homogeneous; s = the new end on its [0, 1].
    let (a, b) = if at_end {
        (*knots.get(num - 2)?, *knots.get(num - 1)?)
    } else {
        (*knots.first()?, *knots.get(1)?)
    };
    if !(b - a > 0.0) {
        return None;
    }
    let s = (new_end - a) / (b - a);
    let first = if at_end { pts.len() - 1 - degree } else { 0 };
    let mut h: Vec<Vec3> = Vec::new();
    let mut hw: Vec<f64> = Vec::new();
    for i in 0..=degree {
        let w = if rational { *wts.get(first + i)? } else { 1.0 };
        h.push(*pts.get(first + i)? * w);
        hw.push(w);
    }
    // de Casteljau at s: the end piece's points are the first of each level (from a to the
    // new end), the start piece's the last of each level (from the new start to b).
    let mut new_pts = vec![Vec3::new(0.0, 0.0, 0.0); degree + 1];
    let mut new_wts = vec![0.0; degree + 1];
    for level in 0..=degree {
        let slot = if at_end { level } else { degree - level };
        let src_i = if at_end { 0 } else { degree - level };
        *new_pts.get_mut(slot)? = *h.get(src_i)?;
        *new_wts.get_mut(slot)? = *hw.get(src_i)?;
        for i in 0..degree - level {
            let (h0, h1) = (*h.get(i)?, *h.get(i + 1)?);
            let (w0, w1) = (*hw.get(i)?, *hw.get(i + 1)?);
            *h.get_mut(i)? = h0 * (1.0 - s) + h1 * s;
            *hw.get_mut(i)? = w0 * (1.0 - s) + w1 * s;
        }
    }
    let mut weights = wts;
    for i in 0..=degree {
        let w = *new_wts.get(i)?;
        if !(w > 0.0) {
            return None;
        }
        *pts.get_mut(first + i)? = *new_pts.get(i)? / w;
        if rational {
            *weights.get_mut(first + i)? = w;
        }
    }
    if at_end {
        *knots.last_mut()? = new_end;
    } else {
        *knots.first_mut()? = new_end;
    }

    let extended = BSplineCurve::create_canonical(&CanonicalData {
        dim: bs.dim(),
        degree,
        control_points: &pts,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &mults,
        knots: &knots,
        knot_type: KnotType::Unspecified,
        weights: rational.then_some(weights.as_slice()),
        trim: None,
    })
    .ok()?;
    // An analytic curve (a line, circle or ellipse) continued is that curve still: the
    // same kind over the continued NURB, not a plain B-spline in its place (bec8a0b).
    if !matches!(curve.kind(), CurveKind::BSpline(_)) {
        return analytic_of_continued_nurb(k, curve, at_end, &extended);
    }
    Some(Curve::from(extended))
}

/// `sm_AnalyticOfContinuedNurb` (bec8a0b, WR-601): the continued NURB `nurb` of the
/// analytic `curve` made that kind again. A circle or ellipse keeps its frame, radii and
/// inside-out sense, its angular domain carried on to the new end (`at_end`: its end was
/// continued, else its start); a line runs between the continued ends over the same
/// interval. The new curve takes `nurb` as its NURB, so its parameter is the continued
/// one; it must evaluate as `nurb` does at nine points from end to end, or this is
/// `None`, as for any other kind and an inside-out line.
fn analytic_of_continued_nurb(
    k: &Kernel,
    curve: &Curve,
    at_end: bool,
    nurb: &BSplineCurve,
) -> Option<Curve> {
    use crate::curve::ellipse::EllipseOptions;
    let nat = nurb.natural_interval();
    let start = nurb.evaluate_point(nat.min()).ok()?;
    let end = nurb.evaluate_point(nat.max()).ok()?;

    let kind = match curve.kind() {
        CurveKind::Ellipse(e) => {
            let pos = e.position();
            let inside_out = e.inside_out();
            let dom = e.step_interval();
            // The angular end the continued NURB end is: the max when the NURB runs the
            // angle's way.
            let at_max = at_end != inside_out;
            let d = (if at_end { end } else { start }) - pos.origin();
            let ang = crate::math::rad_to_deg(
                (d.dot(pos.y_axis()) / e.y_radius()).atan2(d.dot(pos.x_axis()) / e.x_radius()),
            );
            let old = if at_max { dom.max() } else { dom.min() };
            let delta = ieee_remainder(ang - old, 360.0);
            if if at_max {
                !(delta > 0.0)
            } else {
                !(delta < 0.0)
            } {
                return None;
            }
            let mut new_dom = dom;
            if at_max {
                new_dom.set_max(old + delta).ok()?;
            } else {
                new_dom.set_min(old + delta).ok()?;
            }
            let max_dom = e.max_analytic_domain();
            if !(new_dom.length() <= 360.0)
                || !(new_dom.min() >= max_dom.min())
                || !(new_dom.max() <= max_dom.max())
            {
                return None;
            }
            let opts = EllipseOptions {
                dim: curve.dim(),
                nurb: Some(nurb),
                inside_out,
            };
            let made = if e.is_circle_class() {
                crate::curve::ellipse::Ellipse::new_circle(
                    k,
                    pos.origin(),
                    pos.x_axis(),
                    pos.y_axis(),
                    new_dom,
                    e.x_radius(),
                    &opts,
                )
            } else {
                crate::curve::ellipse::Ellipse::new(
                    k,
                    pos.origin(),
                    pos.x_axis(),
                    pos.y_axis(),
                    new_dom,
                    e.x_radius(),
                    e.y_radius(),
                    &opts,
                )
            };
            Curve::from(made.ok()?)
        }
        CurveKind::Line(line) if !line.inside_out() => Curve::from(
            crate::curve::line::Line::create_line_segment(curve.dim(), start, end, Some(&nat))
                .ok()?,
        ),
        _ => return None,
    };

    // The same curve over the same parameter, to round-off.
    let kind_nat = kind.natural_interval();
    if !are_same(kind_nat.min(), nat.min()) || !are_same(kind_nat.max(), nat.max()) {
        return None;
    }
    let zero = ANALYTIC_TOL_SCALE
        * EFF_ZERO
        * (1.0 + crate::math::max2(start.max_dimension(), end.max_dimension()));
    for ii in 0..=8_u32 {
        let t = nat.evaluate(f64::from(ii) / 8.0);
        let on_nurb = nurb.evaluate_point(t).ok()?;
        let on_kind = kind.evaluate_point(k, t).ok()?;
        if !(on_nurb.distance(on_kind) <= zero) {
            return None;
        }
    }
    Some(kind)
}

/// C's `remainder(x, y)` (IEEE 754): `x - n y` for the integer `n` nearest `x / y`, ties
/// to even. The result is exact; `n` is corrected when the rounded quotient misses it.
fn ieee_remainder(x: f64, y: f64) -> f64 {
    if !x.is_finite() || !(y.abs() > 0.0) || y.is_nan() {
        return f64::NAN;
    }
    if y.is_infinite() {
        return x;
    }
    let y = y.abs();
    let mut n = (x / y).round_ties_even();
    let mut r = x - n * y;
    let half = 0.5 * y;
    if r > half || (r == half && n % 2.0 != 0.0) {
        n += 1.0;
        r = x - n * y;
    } else if r < -half || (r == -half && n % 2.0 != 0.0) {
        n -= 1.0;
        r = x - n * y;
    }
    if r == 0.0 {
        return 0.0_f64.copysign(x);
    }
    r
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test helpers
    use super::*;

    /// #3062: the certified bound of a span off two planes is its distance from them, to
    /// rounding, for a line; for a quarter circle at least that, and at most that over cos² 45°.
    #[test]
    fn a_span_is_bounded_off_its_surfaces() {
        use crate::surface::plane::p1::Plane;
        use crate::surface::surface::Surface;
        let plane =
            |z: f64, n: Vec3| Surface::from(Plane::from_normal(Point3::new(0.0, 0.0, z), n));
        let (z, y) = (Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 1.0, 0.0));
        let end = Point3::new(4.0, 0.0, 0.0);
        let line = Curve::from(BSplineCurve::create_line_segment(3, Point3::ZERO, end).unwrap());
        let ivl = line.natural_interval();
        let (a, b) = (ivl.evaluate(0.25), ivl.evaluate(0.75));
        let on = span_off_surfaces(&line, a, b, [&plane(0.0, z), &plane(0.0, y)]).unwrap();
        assert!(on <= 1e-14, "{on:e}");
        let off = span_off_surfaces(&line, a, b, [&plane(1e-3, z), &plane(0.0, y)]).unwrap();
        assert!((off - 1e-3).abs() <= 1e-15, "{off:e}");
        let arc = crate::primitive::api_curves::create_arc(Point3::ZERO, 2.0, 0.0, 90.0).unwrap();
        let arc = Curve::from(arc);
        let ivl = arc.natural_interval();
        let (a, b) = (ivl.evaluate(0.1), ivl.evaluate(0.6));
        let on = span_off_surfaces(&arc, a, b, [&plane(0.0, z), &plane(0.0, z)]).unwrap();
        assert!(on <= 1e-14, "{on:e}");
        let off = span_off_surfaces(&arc, a, b, [&plane(-2e-4, z), &plane(0.0, z)]).unwrap();
        assert!((2e-4..=4e-4 * (1.0 + 1e-12)).contains(&off), "{off:e}");
    }

    /// #20691: a circle `d` off a cylinder, a sphere or a 45° cone of radius 2 radially is
    /// bounded off it by its distance, `d` (`d / sqrt 2` for the cone), not twice that: the
    /// quadric's `f` is `(r + d)^2 - r^2`, about `2 r d`, which the bound took over `r`.
    /// The groove of an M1 thread on a chamfered boss meets the chamfer's cone in a circle
    /// on its root cylinder, continued 2.9e-6 to its vertex, and the doubled bound, 5.9e-6,
    /// refused it against its tolerance 5e-6.
    #[test]
    fn a_span_off_a_quadric_is_bounded_by_its_distance() {
        use crate::geom::axis2_placement::Axis2Placement;
        use crate::surface::cone::p1::Cone;
        use crate::surface::sphere::Sphere;
        use crate::surface::surface::Surface;
        let k = Kernel::default();
        let frame = Axis2Placement::new(Point3::ZERO, Vec3::X, Vec3::Y);
        let (r, d) = (2.0, 1e-4);
        let arc = crate::primitive::api_curves::create_arc(Point3::ZERO, r + d, 0.0, 90.0).unwrap();
        let arc = Curve::from(arc);
        let ivl = arc.natural_interval();
        let (a, b) = (ivl.evaluate(0.1), ivl.evaluate(0.12));
        let cylinder = Surface::from(Cone::create_canonical_cylinder(&k, &frame, r).unwrap());
        let sphere = Surface::from(Sphere::create_canonical(&k, &frame, r).unwrap());
        let cone = Surface::from(
            Cone::create_canonical_with_semi_angle(&k, &frame, r, 45.0, Some(1.0), false, false)
                .unwrap(),
        );
        for (surface, off) in [(&cylinder, d), (&sphere, d), (&cone, d / 2.0_f64.sqrt())] {
            let bound = span_off_surfaces(&arc, a, b, [surface, surface]).unwrap();
            // at least the distance; above it only by the hull of the span's Bernstein
            // coefficients (2% here), well under the doubled bound
            assert!((off..=off * 1.05).contains(&bound), "{bound:e} for {off:e}");
        }
    }

    /// #20854: where `r^2 - f` is near zero the bound still encloses every `d` with
    /// `|d^2 - r^2| <= f`: at `r = 1.2`, `f = 1.4399999999999997` that includes
    /// `d = 1.3e-8` (`r^2 - d^2 <= f` exactly, on these doubles), `|d - r| = 1.199999987`,
    /// which the discriminant rounded to `2.2e-16` (exactly `1.69e-16`) put over the bound,
    /// `1.1999999851`. Far from that limit, at the M1 groove's root circle (`r = 0.36468`,
    /// 2.94e-6 off), it is the closed form `r - sqrt(r^2 - f)` to rounding.
    #[test]
    fn the_radial_bound_encloses_its_distance_near_its_limit() {
        let (r, f, d) = (1.2_f64, 1.439_999_999_999_999_7_f64, 1.3e-8_f64);
        assert!((d * d - r * r).abs() <= f);
        let (bound, _) = radial_off(f, r);
        assert!(bound >= r - d, "{bound} under {}", r - d);
        let (r, off) = (0.364_683_530_658_681_5_f64, 2.94e-6_f64);
        let f = (r + off) * (r + off) - r * r;
        let (bound, near) = radial_off(f, r);
        let exact = r - (r * r - f).sqrt();
        assert!(near && bound >= off, "{bound:e}");
        assert!(
            (bound - exact).abs() <= 1e-9 * exact,
            "{bound:e} for {exact:e}"
        );
    }

    /// A clamped cubic with three spans (inner knots 1 and 2) over `[0, 3]`.
    fn cubic() -> Curve {
        let pts = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(2.5, 1.0, 1.0),
            Point3::new(3.0, -1.0, 0.5),
            Point3::new(4.5, 0.5, -1.0),
            Point3::new(5.0, 2.0, 0.0),
        ];
        let c = BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 3,
            control_points: &pts,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &[4, 1, 1, 4],
            knots: &[0.0, 1.0, 2.0, 3.0],
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        })
        .unwrap();
        Curve::from(c)
    }

    /// The continued curve is the same over the old interval and carries the end span's
    /// polynomial beyond it: its value there is the Taylor polynomial of the old end
    /// (exact for a cubic), at either end (WR-453).
    #[test]
    fn extended_curve_continues_the_end_span_polynomial() {
        let k = Kernel::default();
        let old = cubic();
        let h = 0.04;
        for at_end in [true, false] {
            let (end, new_end) = if at_end { (3.0, 3.0 + h) } else { (0.0, -h) };
            let new = extended_curve(&k, &old, at_end, new_end).unwrap();
            assert_eq!(
                new.natural_interval().max(),
                if at_end { new_end } else { 3.0 }
            );
            assert_eq!(
                new.natural_interval().min(),
                if at_end { 0.0 } else { new_end }
            );
            for t in [0.0, 0.4, 1.0, 1.7, 2.5, 3.0] {
                let (a, b) = (
                    old.evaluate_point(&k, t).unwrap(),
                    new.evaluate_point(&k, t).unwrap(),
                );
                assert!(a.distance(b) < 1e-12, "t = {t}: {a:?} vs {b:?}");
            }
            let d = old.evaluate(&k, end, 3, at_end, false).unwrap();
            let dt = new_end - end;
            let taylor = d[0] + d[1] * dt + d[2] * (dt * dt / 2.0) + d[3] * (dt * dt * dt / 6.0);
            let got = new.evaluate_point(&k, new_end).unwrap();
            assert!(got.distance(taylor) < 1e-12, "{got:?} vs {taylor:?}");
        }
        // an end that is not outside the natural interval, and a curve of another kind
        assert!(extended_curve(&k, &old, true, 3.0).is_none());
        assert!(extended_curve(&k, &old, false, 0.5).is_none());
    }

    /// The quarter circle of radius 2 about (1, 1, 0) in the XY plane, from `from` to
    /// `from + 90` degrees (`inside_out`: its NURB runs the other way).
    fn quarter_circle(k: &Kernel, from: f64, inside_out: bool) -> Curve {
        let e = crate::curve::ellipse::Ellipse::new_circle(
            k,
            Point3::new(1.0, 1.0, 0.0),
            Vec3::X,
            Vec3::Y,
            Extent1d::from_min_max(from, from + 90.0),
            2.0,
            &crate::curve::ellipse::EllipseOptions {
                inside_out,
                ..Default::default()
            },
        )
        .unwrap();
        Curve::from(e)
    }

    /// WR-601 (bec8a0b): a circle continued past either end of its NURB stays a circle
    /// of the same frame and radius, its angular domain carried on to the new end, over
    /// the continued NURB's parameter.
    #[test]
    fn wr601_continued_circle_stays_a_circle() {
        for inside_out in [false, true] {
            for at_end in [true, false] {
                let k = Kernel::default();
                let old = quarter_circle(&k, 10.0, inside_out);
                let nat = old.natural_interval();
                let h = 0.01 * nat.length();
                let new_end = if at_end { nat.max() + h } else { nat.min() - h };
                let new = extended_curve(&k, &old, at_end, new_end)
                    .unwrap_or_else(|| panic!("inside_out {inside_out} at_end {at_end}"));
                let CurveKind::Ellipse(e) = new.kind() else {
                    panic!("not a circle: {}", new.class_string());
                };
                assert!(e.is_circle_class());
                assert_eq!(e.x_radius(), 2.0);
                assert_eq!(e.inside_out(), inside_out);
                let dom = e.step_interval();
                // The NURB end continued is the angular max unless the circle runs
                // inside out.
                if at_end == inside_out {
                    assert!(dom.min() < 10.0, "{dom:?}");
                    assert_eq!(dom.max(), 100.0);
                } else {
                    assert_eq!(dom.min(), 10.0);
                    assert!(dom.max() > 100.0, "{dom:?}");
                }
                let new_nat = new.natural_interval();
                assert!(are_same(
                    if at_end { new_nat.max() } else { new_nat.min() },
                    new_end
                ));
                // On the circle, and the old curve over the old interval.
                for i in 0..=10 {
                    let t = new_nat.evaluate(f64::from(i) / 10.0);
                    let p = new.evaluate_point(&k, t).unwrap();
                    let r = p.distance(Point3::new(1.0, 1.0, 0.0));
                    assert!((r - 2.0).abs() < 1e-12, "t = {t}: r = {r}");
                }
                for i in 0..=10 {
                    let t = nat.evaluate(f64::from(i) / 10.0);
                    let (a, b) = (
                        old.evaluate_point(&k, t).unwrap(),
                        new.evaluate_point(&k, t).unwrap(),
                    );
                    assert!(a.distance(b) < 1e-12, "t = {t}: {a:?} vs {b:?}");
                }
            }
        }
    }

    /// WR-601: a circle from 0 degrees continued back past 0 would leave its maximum
    /// analytic domain `[0, 360]`, so it is not continued, as in the fork.
    #[test]
    fn wr601_circle_not_continued_out_of_its_analytic_domain() {
        let k = Kernel::default();
        let old = quarter_circle(&k, 0.0, false);
        let nat = old.natural_interval();
        assert!(extended_curve(&k, &old, false, nat.min() - 0.01 * nat.length()).is_none());
        assert!(extended_curve(&k, &old, true, nat.max() + 0.01 * nat.length()).is_some());
    }

    /// WR-601: a continued ellipse stays an ellipse of the same radii.
    #[test]
    fn wr601_continued_ellipse_stays_an_ellipse() {
        let k = Kernel::default();
        let old = Curve::from(
            crate::curve::ellipse::Ellipse::new(
                &k,
                Point3::new(0.0, 0.0, 1.0),
                Vec3::X,
                Vec3::Z,
                Extent1d::from_min_max(-30.0, 60.0),
                3.0,
                1.5,
                &crate::curve::ellipse::EllipseOptions::default(),
            )
            .unwrap(),
        );
        let nat = old.natural_interval();
        let new_end = nat.max() + 0.02 * nat.length();
        let new = extended_curve(&k, &old, true, new_end).unwrap();
        let CurveKind::Ellipse(e) = new.kind() else {
            panic!("not an ellipse: {}", new.class_string());
        };
        assert!(!e.is_circle_class());
        assert_eq!((e.x_radius(), e.y_radius()), (3.0, 1.5));
        assert_eq!(e.step_interval().min(), -30.0);
        assert!(e.step_interval().max() > 60.0);
        let p = new.evaluate_point(&k, new_end).unwrap();
        let (x, z) = (p.x / 3.0, (p.z - 1.0) / 1.5);
        assert!((x * x + z * z - 1.0).abs() < 1e-12, "{p:?}");
    }

    /// WR-601: a continued line stays a line over the continued interval, in 3D and as
    /// a 2D UV curve.
    #[test]
    fn wr601_continued_line_stays_a_line() {
        for dim in [3, 2] {
            let k = Kernel::default();
            let (a, b) = (Point3::new(1.0, 2.0, 0.0), Point3::new(4.0, -2.0, 0.0));
            let old = Curve::from(crate::curve::line::Line::from_points(a, b, dim));
            let nat = old.natural_interval();
            let new_end = nat.min() - 0.03 * nat.length();
            let new = extended_curve(&k, &old, false, new_end).unwrap();
            let CurveKind::Line(line) = new.kind() else {
                panic!("not a line: {}", new.class_string());
            };
            assert_eq!(line.dim(), dim);
            let new_nat = new.natural_interval();
            assert!(are_same(new_nat.min(), new_end) && are_same(new_nat.max(), nat.max()));
            let start = new.evaluate_point(&k, new_end).unwrap();
            let want = a + (b - a) * ((new_end - nat.min()) / nat.length());
            assert!(start.distance(want) < 1e-12, "{start:?} vs {want:?}");
        }
    }

    #[test]
    fn ieee_remainder_matches_c() {
        assert_eq!(ieee_remainder(190.0, 360.0), -170.0);
        assert_eq!(ieee_remainder(-190.0, 360.0), 170.0);
        assert_eq!(ieee_remainder(180.0, 360.0), 180.0);
        assert_eq!(ieee_remainder(540.0, 360.0), -180.0);
        assert_eq!(ieee_remainder(-180.0, 360.0), -180.0);
        assert_eq!(ieee_remainder(5.0, 360.0), 5.0);
        assert_eq!(ieee_remainder(-0.0, 360.0).to_bits(), (-0.0_f64).to_bits());
    }
}

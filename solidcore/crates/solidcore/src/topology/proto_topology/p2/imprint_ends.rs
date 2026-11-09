//! Not upstream: fitting an intersection edge's ends to its proto vertices before
//! `SmEdgeDefinition::Imprint` makes the edge (the SolidCraft fork's changes, opt-in).
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
//! WR-452 (trim, `Kernel::fixes.imprint_trims_edge_to_proto_vertex`), WR-453 (extend,
//! `Kernel::fixes.imprint_extends_edge_to_proto_vertex`); both flags carry the fork's
//! correction ffaf4e8 (WR-498, #1480, #470): an end goes to its proto vertex's position
//! only when the proto vertex is one vertex in each B-rep.
//! The fork's `sm_CreateExactlyExtendedCurve` (a8caccd) continues any `SmBSplineCurve`
//! kind and makes the analytic ones plain B-splines; `extended_curve` continues a plain
//! B-spline only. Its correction bec8a0b (WR-601,
//! `Kernel::fixes.imprint_extension_keeps_analytic_curve`) continues a line, circle or
//! ellipse as that kind and no other analytic kind, so with both flags on the port is
//! the fork at bec8a0b.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // the fork's NaN-safe tests

use super::{curve_of, dangling, ignore_soft, EdgeDefId, ProtoBrep, ProtoTopologyManager};
use super::{ProtoVertexId, Result};
use crate::curve::curve::p2::{Curve, CurveKind};
use crate::error::{Failure, Reason, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::tol::are_same;
use crate::intersect::point_class::{ClassObject, ClassObjectKind, PointClassType};
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, EFF_ZERO};
use crate::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;

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

/// A quadric's kind, for its distance bound: a plane's `|f|` (its normal unit), a sphere's
/// `|f| / r`, a cylinder's or cone's `|f| / R`, `R` its least radius along the span.
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
            QuadricKind::Sphere { radius } => f_over / radius,
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
                f_over / r_min
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
    /// curves to the vertex (`imprint_extends_edge_to_proto_vertex`) or else move the end
    /// to the curve's point nearest the vertex (`imprint_trims_edge_to_proto_vertex`).
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
            if k.fixes.imprint_extends_edge_to_proto_vertex
                && self.extend_end_to_proto_vertex(k, brep, ed, at_end, pv_pos, end_pt, gap)?
            {
                continue;
            }
            if k.fixes.imprint_trims_edge_to_proto_vertex {
                self.trim_end_to_proto_vertex(k, brep, ed, at_end, pv_pos, gap)?;
            }
        }
        Ok(())
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

/// `sm_CreateExactlyExtendedCurve` (a8caccd): a copy of a clamped B-spline curve whose end
/// (or start) span is continued exactly, its polynomial unchanged, to `new_end`, outside
/// the natural interval. The span is made one Bezier segment (its inner knot raised to the
/// degree, which leaves the curve as it is) and that segment's polynomial is taken over
/// the longer interval by de Casteljau's subdivision at a parameter outside `[0, 1]`, in
/// homogeneous coordinates so a rational curve keeps its shape. No control points are
/// added. `None` for another curve kind, an unclamped curve, or a `new_end` that is not
/// outside the natural interval.
///
/// With `Kernel::fixes.imprint_extension_keeps_analytic_curve` (bec8a0b, WR-601) a line,
/// circle or ellipse is continued too, over its NURB, and comes back as that kind
/// ([`analytic_of_continued_nurb`]), or `None` when that fails (as for a parabola or a
/// hyperbola). Without it those kinds are `None`.
fn extended_curve(k: &Kernel, curve: &Curve, at_end: bool, new_end: f64) -> Option<Curve> {
    let analytic = k.fixes.imprint_extension_keeps_analytic_curve;
    let src: std::borrow::Cow<'_, BSplineCurve> = match curve.kind() {
        CurveKind::BSpline(src) => std::borrow::Cow::Borrowed(src),
        CurveKind::Line(line) if analytic => line.base_with_nurb().ok()?,
        CurveKind::Ellipse(ellipse) if analytic => std::borrow::Cow::Borrowed(ellipse.base()),
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
    /// the continued NURB's parameter; without `imprint_extension_keeps_analytic_curve`
    /// it is not continued.
    #[test]
    fn wr601_continued_circle_stays_a_circle() {
        for inside_out in [false, true] {
            for at_end in [true, false] {
                let mut k = Kernel::default();
                let old = quarter_circle(&k, 10.0, inside_out);
                let nat = old.natural_interval();
                let h = 0.01 * nat.length();
                let new_end = if at_end { nat.max() + h } else { nat.min() - h };
                assert!(extended_curve(&k, &old, at_end, new_end).is_none());

                k.fixes.imprint_extension_keeps_analytic_curve = true;
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
        let mut k = Kernel::default();
        k.fixes.imprint_extension_keeps_analytic_curve = true;
        let old = quarter_circle(&k, 0.0, false);
        let nat = old.natural_interval();
        assert!(extended_curve(&k, &old, false, nat.min() - 0.01 * nat.length()).is_none());
        assert!(extended_curve(&k, &old, true, nat.max() + 0.01 * nat.length()).is_some());
    }

    /// WR-601: a continued ellipse stays an ellipse of the same radii.
    #[test]
    fn wr601_continued_ellipse_stays_an_ellipse() {
        let mut k = Kernel::default();
        k.fixes.imprint_extension_keeps_analytic_curve = true;
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
            let mut k = Kernel::default();
            let (a, b) = (Point3::new(1.0, 2.0, 0.0), Point3::new(4.0, -2.0, 0.0));
            let old = Curve::from(crate::curve::line::Line::from_points(a, b, dim));
            let nat = old.natural_interval();
            let new_end = nat.min() - 0.03 * nat.length();
            assert!(extended_curve(&k, &old, false, new_end).is_none());
            k.fixes.imprint_extension_keeps_analytic_curve = true;
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

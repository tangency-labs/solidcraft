//! Pcurves for truck faces, which keep none: each edge use's curve in its face's surface
//! parameters, on the parameter of the edge's own curve (as the adapter writes it), running in
//! that curve's direction. Exact where the edge lies on its surface in closed form, otherwise a
//! 2D B-spline fitted within the tolerance.
use super::Result;
use crate::KernelError;
use crate::body::from_p3;
use crate::tess::uv;
use mt::BoundedCurve;
use solidcraft_geom::brep as b;
use solidcraft_geom::tess::EvalSurface;
use truck_modeling as mt;

/// For each of `face`'s boundary wires, in order, the pcurve of each of its edges, in order, in
/// the (u, v) of the face's surface as truck evaluates it (`ParametricSurface::subs`), within
/// `tolerance` mm when lifted onto the surface. Wires and edges run as `face.boundaries()` lists
/// them (the order the adapter writes); each pcurve runs in its curve's own direction.
pub(super) fn pcurves(face: &mt::Face, tolerance: f64) -> Result<Vec<Vec<b::Curve2>>> {
    if !(tolerance.is_finite() && tolerance > 0.0) {
        return Err(KernelError::Invalid(format!("pcurve tolerance {tolerance}")));
    }
    crate::guard("pcurves", || {
        let surface = face.surface();
        let mut out = Vec::new();
        for (w, wire) in face.boundaries().iter().enumerate() {
            let mut uses = Vec::new();
            for (i, edge) in wire.edge_iter().enumerate() {
                let curve = edge.curve();
                let pc = pcurve(&surface, &curve).map_err(|e| KernelError::Failed(format!("pcurve of wire {w} edge {i}: {e}")))?;
                uses.push(pc);
            }
            out.push(uses);
        }
        Ok(out)
    })
}

fn pcurve(surface: &mt::Surface, curve: &mt::Curve) -> Result<b::Curve2> {
    match surface {
        mt::Surface::Plane(_) => on_plane(surface, curve),
        _ => Err(KernelError::Failed("not supported yet: pcurves on curved surfaces".into())),
    }
}

/// A line or (rational) B-spline on a plane: the plane's parameters are affine in space, so the
/// pcurve is the curve's own knots and weights with its control points mapped.
fn on_plane(surface: &mt::Surface, curve: &mt::Curve) -> Result<b::Curve2> {
    let Some(plane @ EvalSurface::Plane { .. }) = crate::tess::surface::eval_surface(surface) else {
        return Err(KernelError::Failed("plane parameters".into()));
    };
    let dom = uv::domain(&plane);
    let map = |p: mt::Point3| uv::project(&plane, &dom, from_p3(p), None, 0.0).ok_or_else(|| KernelError::Failed("degenerate plane".into()));
    Ok(match curve {
        mt::Curve::Line(l) => {
            let (t0, t1) = curve.range_tuple();
            b::Curve2::Spline { knots: vec![t0, t0, t1, t1], points: vec![map(l.0)?, map(l.1)?] }
        }
        mt::Curve::BSplineCurve(c) => b::Curve2::Spline {
            knots: c.knot_vec().iter().copied().collect(),
            points: c.control_points().iter().map(|p| map(*p)).collect::<Result<_>>()?,
        },
        mt::Curve::NurbsCurve(c) => b::Curve2::RationalSpline {
            knots: c.knot_vec().iter().copied().collect(),
            homogeneous: c
                .control_points()
                .iter()
                .map(|h| {
                    let [u, v] = map(mt::Point3::new(h.x / h.w, h.y / h.w, h.z / h.w))?;
                    Ok([u * h.w, v * h.w, h.w])
                })
                .collect::<Result<_>>()?,
        },
        mt::Curve::IntersectionCurve(_) => return Err(KernelError::Failed("not supported yet: intersection pcurves".into())),
    })
}

/// The point of `c` at `t` (`None` outside its interval or for malformed records).
pub(super) fn eval(c: &b::Curve2, t: f64) -> Option<[f64; 2]> {
    let (knots, h): (&[f64], Vec<[f64; 3]>) = match c {
        b::Curve2::Spline { knots, points } => (knots, points.iter().map(|&[u, v]| [u, v, 1.0]).collect()),
        b::Curve2::RationalSpline { knots, homogeneous } => (knots, homogeneous.clone()),
    };
    let p = knots.len().checked_sub(h.len() + 1)?;
    let span = solidcraft_geom::bspline::find_span(knots, p, h.len(), t)?;
    let n = basis(knots, p, span, t)?;
    let mut s = [0.0; 3];
    for (j, nj) in n.iter().enumerate() {
        let q = h.get(span + j - p)?;
        for (a, x) in s.iter_mut().zip(q) {
            *a += nj * x;
        }
    }
    (s[2].abs() > 0.0).then(|| [s[0] / s[2], s[1] / s[2]])
}

/// The `p + 1` basis functions non-zero on knot span `span` at `t` (NURBS Book A2.2).
fn basis(knots: &[f64], p: usize, span: usize, t: f64) -> Option<Vec<f64>> {
    if span < p || span + p >= knots.len() {
        return None;
    }
    let k = |i: usize| knots.get(i).copied().unwrap_or(0.0);
    let mut n = vec![0.0; p + 1];
    let (mut left, mut right) = (vec![0.0; p + 1], vec![0.0; p + 1]);
    *n.first_mut()? = 1.0;
    for j in 1..=p {
        *left.get_mut(j)? = t - k(span + 1 - j);
        *right.get_mut(j)? = k(span + j) - t;
        let mut saved = 0.0;
        for r in 0..j {
            let (rr, lj) = (*right.get(r + 1)?, *left.get(j - r)?);
            let den = rr + lj;
            let tmp = if den != 0.0 { n.get(r)? / den } else { 0.0 };
            *n.get_mut(r)? = saved + rr * tmp;
            saved = lj * tmp;
        }
        *n.get_mut(j)? = saved;
    }
    Some(n)
}

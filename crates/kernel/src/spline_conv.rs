//! geom's B-spline curves and surfaces as schema 4 records, weights kept (homogeneous control
//! points carried over as they are), and a whole tensor surface as a one-face sheet.

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::{Curve, Curve2, Surface};
use solidcraft_geom::bspline::{HPoint, NurbsSurface};

use crate::assemble::{AFace, AUse, Assembly};
use crate::body::Body;
use crate::{KernelError, Result};

fn fail(m: impl std::fmt::Display) -> KernelError {
    KernelError::Failed(format!("spline geometry: {m}"))
}

/// A homogeneous control point as the record's `[x*w, y*w, z*w, w]`.
pub(crate) fn h4(h: HPoint) -> [f64; 4] {
    [h.x, h.y, h.z, h.w]
}

/// A NURBS curve of knots and homogeneous control points.
pub(crate) fn nurbs_curve(knots: &[f64], ctrl: &[HPoint]) -> Curve {
    Curve::RationalSpline { knots: knots.to_vec(), homogeneous: ctrl.iter().copied().map(h4).collect() }
}

/// A NURBS surface of knots and homogeneous control points (`rows[i][j]`: i along u).
pub(crate) fn nurbs_surface(u_knots: &[f64], v_knots: &[f64], rows: Vec<Vec<[f64; 4]>>) -> Surface {
    Surface::RationalSpline { u_knots: u_knots.to_vec(), v_knots: v_knots.to_vec(), homogeneous: rows }
}

/// geom's tensor surface as a record surface.
pub(crate) fn surface(s: &NurbsSurface) -> Surface {
    let [nu, nv] = s.count();
    let [ku, kv] = s.knots();
    nurbs_surface(ku, kv, (0..nu).map(|i| (0..nv).map(|j| h4(s.point(i, j))).collect()).collect())
}

/// The active interval `[knots[degree], knots[count]]` of a direction with `count` points.
fn active(knots: &[f64], count: usize) -> Result<[f64; 2]> {
    let degree = knots.len().checked_sub(count + 1).ok_or_else(|| fail("knots and points disagree"))?;
    match (knots.get(degree), knots.get(count)) {
        (Some(a), Some(b)) if a < b => Ok([*a, *b]),
        _ => Err(fail("an empty knot interval")),
    }
}

/// One face of a whole tensor surface as a sheet: its four boundary curves (the net's first and
/// last rows and columns) as edges sharing the corners, each with its exact pcurve (an edge's
/// parameter is the surface's along it). This is what a patch built as a single control net
/// (`bridge`, `boundary`, the T-spline sheet) hands to the B-rep.
pub(crate) fn tensor_face(s: &NurbsSurface) -> Result<Body> {
    let [nu, nv] = s.count();
    if nu < 2 || nv < 2 {
        return Err(fail("a face needs at least 2 x 2 control points"));
    }
    let [ku, kv] = s.knots();
    let ([u0, u1], [v0, v1]) = (active(ku, nu)?, active(kv, nv)?);
    let corners = [s.point(0, 0), s.point(nu - 1, 0), s.point(nu - 1, nv - 1), s.point(0, nv - 1)];
    let corners: Vec<Vec3> = corners.iter().map(|c| c.project().ok_or_else(|| fail("a corner has no position"))).collect::<Result<_>>()?;
    let diag = corners.iter().flat_map(|a| corners.iter().map(move |b| a.dist(*b))).fold(0.0, f64::max);
    let mut asm = Assembly::sheet((diag * 1e-7).max(1e-9));
    let v: Vec<u32> = corners.iter().map(|p| asm.vertex(*p)).collect::<Result<_>>()?;
    let (Some(&a), Some(&b), Some(&c), Some(&d)) = (v.first(), v.get(1), v.get(2), v.get(3)) else { return Err(fail("corners")) };
    let row = |j: usize| -> Vec<HPoint> { (0..nu).map(|i| s.point(i, j)).collect() };
    let column = |i: usize| -> Vec<HPoint> { (0..nv).map(|j| s.point(i, j)).collect() };
    let line = |t: [f64; 2], p: [f64; 2], q: [f64; 2]| Curve2::Spline { knots: vec![t[0], t[0], t[1], t[1]], points: vec![p, q] };
    // Bottom (v0) and top (v1) run along u, right (u1) and left (u0) along v, each from its
    // lower corner; the loop runs counter-clockwise in (u, v).
    let bottom = asm.edge(nurbs_curve(ku, &row(0)), [u0, u1], a, b)?;
    let right = asm.edge(nurbs_curve(kv, &column(nu - 1)), [v0, v1], b, c)?;
    let top = asm.edge(nurbs_curve(ku, &row(nv - 1)), [u0, u1], d, c)?;
    let left = asm.edge(nurbs_curve(kv, &column(0)), [v0, v1], a, d)?;
    let loop_ = vec![
        AUse { edge: bottom, forward: true, given: Some(line([u0, u1], [u0, v0], [u1, v0])) },
        AUse { edge: right, forward: true, given: Some(line([v0, v1], [u1, v0], [u1, v1])) },
        AUse { edge: top, forward: false, given: Some(line([u0, u1], [u0, v1], [u1, v1])) },
        AUse { edge: left, forward: false, given: Some(line([v0, v1], [u0, v0], [u0, v1])) },
    ];
    asm.face(AFace { surface: surface(s), natural_out: true, loops: vec![loop_], shell: 0, domain: Some([[u0, u1], [v0, v1]]) });
    Body::from_record(asm.finish()?.record)
}

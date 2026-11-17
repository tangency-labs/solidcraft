//! Sketch Fillet at a corner where a conic or an open spline meets a line, an arc, or another
//! such curve (#9178). The round's centre comes from Newton's method on the two sides' offsets:
//! each pass meets the tangent lines at the centre's projections, offset by the radius. Both
//! sides are then cut back to their tangent points exactly and in place: a conic stays the conic
//! it was (the rest of the same rational quadratic, its apex moved), a spline becomes the control
//! spline of its kept Bézier spans, and each keeps its id and the constraints on it.

use serde_json::Value;
use solidcore_record::{Seg2, Vec2};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{ConstraintKind, CurveKind, Nurbs, Sketch};

use super::sketch::add_c;
use super::sketch_freeform::{locate, point_at, span};
use crate::Result;
use crate::params::{bad, str_};

/// Within this of the corner or a side's far end, a tangent point leaves nothing to round or
/// nothing of the side, as `round_corner` has it.
const NEAR: f64 = 1e-6;

/// The most passes the centre's iteration takes: it converges in a handful.
const MAX_PASSES: usize = 60;

fn ends(sk: &Sketch, c: usize) -> Option<(usize, usize)> {
    sk.curves.get(c).and_then(|c| c.kind.ends())
}

fn id_of(sk: &Sketch, c: usize) -> String {
    sk.curves.get(c).map(|c| c.id.clone()).unwrap_or_default()
}

/// The corner `p` names, of any two curves with ends: the sides and the shared point, found as
/// `corner` and `round_corner` find them (never a linked curve or an ellipse's axis, and the
/// profile sides where construction curves end there too). None when it names no such corner.
fn sides(sk: &Sketch, p: &Value) -> Option<(usize, usize, usize)> {
    if let Some(rf) = str_(p, "point") {
        let q = sk.resolve_point(rf)?;
        let at: Vec<usize> = (0..sk.curves.len())
            .filter(|i| ends(sk, *i).is_some_and(|(a, b)| a == q || b == q) && !sk.is_linked_curve(*i) && !sk.is_ellipse_axis(*i))
            .collect();
        let at = if at.len() == 2 { at } else { at.into_iter().filter(|i| sk.curves.get(*i).is_some_and(|c| !c.construction)).collect() };
        match at[..] {
            [a, b] => Some((a, b, q)),
            _ => None,
        }
    } else {
        let a = str_(p, "a").and_then(|i| sk.curve_index(i))?;
        let b = str_(p, "b").and_then(|i| sk.curve_index(i))?;
        if a == b || sk.is_ellipse_axis(a) || sk.is_ellipse_axis(b) {
            return None;
        }
        let ((a0, a1), (b0, b1)) = (ends(sk, a)?, ends(sk, b)?);
        let q = [a0, a1].into_iter().find(|x| *x == b0 || *x == b1)?;
        Some((a, b, q))
    }
}

/// Round a corner with a conic or an open spline as a side: the arc of radius `r` tangent to
/// both sides nearest the corner, and the tangent constraints. None when the corner is not of
/// that kind (the caller reports why the corner was refused).
pub(super) fn round_freeform_corner(sk: &mut Sketch, p: &Value, r: f64, cmd: &str) -> Result<Option<(usize, Vec<String>)>> {
    let Some((ca, cb, q)) = sides(sk, p) else { return Ok(None) };
    if ![ca, cb].into_iter().any(|c| sk.curves.get(c).is_some_and(|c| c.kind.is_freeform())) {
        return Ok(None);
    }
    for c in [ca, cb] {
        refuse_side(sk, c, cmd)?;
    }
    refuse_kept_corner(sk, q, [ca, cb], cmd)?;
    let Some(pc) = sk.point(q) else { return Ok(None) };
    let side = |c: usize| Side::new(sk, c, q).ok_or_else(|| bad(cmd, format!("`{}` has no shape", id_of(sk, c))));
    let (sa, sb) = (side(ca)?, side(cb)?);
    let (cen, ua, ub) = centre(&sa, &sb, pc, r, cmd)?;
    let (ta, tb) = (sa.at(ua), sb.at(ub));
    let (na, nb) = (sk.add_point(ta, None)?, sk.add_point(tb, None)?);
    let mut gone = vec![q];
    gone.extend(cut_back(sk, ca, &sa, ua, q, na, cmd)?);
    gone.extend(cut_back(sk, cb, &sb, ub, q, nb, cmd)?);
    let ccw = (ta - cen).cross(tb - cen) > 0.0;
    let arc =
        if ccw { sk.add_arc(cen, ta, tb, [None, Some(na), Some(nb)], None)? } else { sk.add_arc(cen, tb, ta, [None, Some(nb), Some(na)], None)? };
    let cons = vec![add_c(sk, ConstraintKind::Tangent { a: ca, b: arc })?, add_c(sk, ConstraintKind::Tangent { a: cb, b: arc })?];
    let arc_id = id_of(sk, arc);
    // The corner and the points a cut-back side no longer uses go, unless another curve uses
    // them (never the origin). Whatever measured to such a point goes with it.
    gone.retain(|p| *p != 0 && !sk.curves.iter().any(|c| c.kind.uses(*p)));
    gone.sort_unstable();
    gone.dedup();
    sk.remove_points(&gone);
    let arc = sk.curve_index(&arc_id).ok_or_else(|| bad(cmd, "the fillet arc is gone"))?;
    Ok(Some((arc, cons)))
}

/// What a side of such a corner must be: a conic whose apex point nothing else holds (the apex
/// moves with the cut), or an open spline without tangent handles, with exact Bézier spans of its
/// own degree and no weights.
fn refuse_side(sk: &Sketch, c: usize, cmd: &str) -> Result<()> {
    let id = id_of(sk, c);
    let Some(curve) = sk.curves.get(c) else { return Ok(()) };
    match &curve.kind {
        CurveKind::Conic { apex, .. } => {
            let apex = *apex;
            let held = held_by(sk, apex, &[c]);
            if let Some(why) = held {
                return Err(bad(cmd, format!("`{id}`'s apex point moves when the corner is rounded, and {why}")));
            }
        }
        CurveKind::Spline { handles, nurbs, .. } => {
            if !handles.is_empty() || sk.curves.iter().any(|x| x.spline_handle_of.as_deref() == Some(id.as_str())) {
                return Err(bad(cmd, format!("`{id}` has tangent handles: reset them first (sketch.spline.reset_handles)")));
            }
            if nurbs.as_ref().is_some_and(|n| n.weights.iter().zip(n.weights.iter().skip(1)).any(|(u, v)| u != v)) {
                return Err(bad(cmd, format!("not supported yet: a fillet on a corner of `{id}`, a rational spline")));
            }
            if sk.exact_segs(c).is_none() {
                return Err(bad(cmd, format!("not supported yet: a fillet on a corner of `{id}`, a spline of degree above 9")));
            }
        }
        _ => {}
    }
    Ok(())
}

/// What holds point `p` besides the curves `own`: a constraint or dimension on it, a fixed or
/// expression-driven position, or another curve using it. The cause, or None.
fn held_by(sk: &Sketch, p: usize, own: &[usize]) -> Option<String> {
    if let Some(k) = sk.constraints.iter().find(|k| k.kind.refers_to_point(p)) {
        return Some(format!("`{}` holds or measures to it", k.id));
    }
    if let Some((_, other)) = sk.curves.iter().enumerate().find(|(i, c)| !own.contains(i) && c.kind.uses(p)) {
        return Some(format!("`{}` uses it too", other.id));
    }
    let point = sk.points.get(p)?;
    if point.fixed {
        return Some("it is fixed".to_string());
    }
    if point.exprs.is_some() {
        return Some("an expression places it".to_string());
    }
    None
}

/// A corner that `keep_corner` would keep as a virtual sharp corner is refused here: a point
/// held on a conic or spline is held on the curve as it is, with no extension past its end, so
/// the corner could not stay on both sides' extensions.
fn refuse_kept_corner(sk: &Sketch, q: usize, own: [usize; 2], cmd: &str) -> Result<()> {
    let why = if q == 0 {
        Some("it is the origin".to_string())
    } else {
        held_by(sk, q, &own).or_else(|| {
            sk.constraints.iter().find(|k| matches!(k.kind, ConstraintKind::Length { l, .. } if own.contains(&l))).map(|k| {
                let line = match k.kind {
                    ConstraintKind::Length { l, .. } => id_of(sk, l),
                    _ => String::new(),
                };
                format!("`{}` holds the length of `{line}`, measured to it", k.id)
            })
        })
    };
    match why {
        Some(why) => {
            Err(bad(cmd, format!("a corner of a conic or spline cannot stay as a virtual sharp corner, and this one would have to: {why}")))
        }
        None => Ok(()),
    }
}

/// One side of the corner: its exact pieces in its direction of travel, and which end the
/// corner is.
struct Side {
    segs: Vec<Seg2>,
    /// The corner is where the pieces start (else where they end).
    at_start: bool,
    far: Vec2,
}

impl Side {
    fn new(sk: &Sketch, c: usize, q: usize) -> Option<Side> {
        let segs = sk.exact_pieces(c);
        let (a, b) = ends(sk, c)?;
        let at_start = a == q;
        let far = sk.point(if at_start { b } else { a })?;
        (!segs.is_empty() && segs.iter().all(|s| s.start().is_finite() && s.end().is_finite())).then_some(Side { segs, at_start, far })
    }
    fn n(&self) -> f64 {
        self.segs.len() as f64
    }
    /// The corner's position along the pieces.
    fn corner_u(&self) -> f64 {
        if self.at_start { 0.0 } else { self.n() }
    }
    fn at(&self, u: f64) -> Vec2 {
        point_at(&self.segs, u)
    }
    /// The unit direction of travel at position `u`, None where the curve has none.
    fn dir(&self, u: f64) -> Option<Vec2> {
        let i = (u.floor().max(0.0) as usize).min(self.segs.len().saturating_sub(1));
        let t = (u - i as f64).clamp(0.0, 1.0);
        let d = self.segs.get(i)?.derivative(t);
        d.is_finite().then(|| d.normalized()).flatten()
    }
    /// The unit direction away from the corner, at the corner.
    fn away(&self) -> Option<Vec2> {
        let d = self.dir(self.corner_u())?;
        Some(if self.at_start { d } else { -d })
    }
}

/// Where the line through `p` along `d` meets the one through `q` along `e`.
fn meet(p: Vec2, d: Vec2, q: Vec2, e: Vec2) -> Option<Vec2> {
    let den = d.cross(e);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = (q - p).cross(e) / den;
    let x = p + d * t;
    x.is_finite().then_some(x)
}

/// The centre of the round of radius `r` nearest the corner `pc`, and the positions of its
/// tangent points along each side. Newton's method on the sides' offsets: from the tangent lines
/// at the corner, each pass meets the two lines offset by `r`, projects that point onto both
/// sides and takes the tangent lines there, until the point stays put (a line's tangent line is
/// itself). The offsets can lie four ways; of the ones whose tangent points sit inside both
/// sides, the nearest the corner wins.
fn centre(sa: &Side, sb: &Side, pc: Vec2, r: f64, cmd: &str) -> Result<(Vec2, f64, f64)> {
    let (Some(wa), Some(wb)) = (sa.away(), sb.away()) else { return Err(bad(cmd, "degenerate curve")) };
    if wa.cross(wb).abs() < 1e-7 {
        return Err(bad(
            cmd,
            if wa.dot(wb) < 0.0 {
                "the curves meet smoothly there: there is no corner to round"
            } else {
                "the curves fold back on each other there: there is no corner to round"
            },
        ));
    }
    let scale = 1.0 + pc.len() + r;
    let (mut smooth, mut short) = (false, false);
    let mut best: Option<(f64, Vec2, f64, f64)> = None;
    for sign_a in [1.0, -1.0] {
        for sign_b in [1.0, -1.0] {
            // Each side's tangent line (a point of it and its unit direction) and the position
            // of that point along the side.
            let (mut la, mut lb) = ((pc, wa, sa.corner_u()), (pc, wb, sb.corner_u()));
            let mut cen: Option<Vec2> = None;
            let mut settled = false;
            for _ in 0..MAX_PASSES {
                let Some(c) = meet(la.0 + la.1.perp() * (r * sign_a), la.1, lb.0 + lb.1.perp() * (r * sign_b), lb.1) else { break };
                let still = cen.is_some_and(|o| o.dist(c) <= 1e-12 * scale);
                cen = Some(c);
                let (Some(ta), Some(tb)) = (tangent_line(sa, c), tangent_line(sb, c)) else {
                    cen = None;
                    break;
                };
                (la, lb) = (ta, tb);
                if still {
                    settled = true;
                    break;
                }
            }
            let (Some(c), true) = (cen, settled) else { continue };
            let (ua, ub) = (la.2, lb.2);
            let (ta, tb) = (sa.at(ua), sb.at(ub));
            // A projection that stopped at a side's end is not at the radius from the centre.
            if (ta.dist(c) - r).abs() > 1e-7 * scale || (tb.dist(c) - r).abs() > 1e-7 * scale {
                continue;
            }
            if ta.dist(pc) < NEAR || tb.dist(pc) < NEAR {
                smooth = true;
                continue;
            }
            if ta.dist(sa.far) < NEAR || tb.dist(sb.far) < NEAR {
                short = true;
                continue;
            }
            let score = ta.dist(pc) + tb.dist(pc);
            if best.is_none_or(|b| score < b.0) {
                best = Some((score, c, ua, ub));
            }
        }
    }
    best.map(|(_, c, ua, ub)| (c, ua, ub)).ok_or_else(|| {
        bad(cmd, if smooth && !short { "the curves meet smoothly there: there is no corner to round" } else { "that radius does not fit the corner" })
    })
}

/// The side's tangent line at the point of it nearest `c`: that point, its unit direction and
/// its position along the side.
fn tangent_line(side: &Side, c: Vec2) -> Option<(Vec2, Vec2, f64)> {
    let (u, _) = locate(&side.segs, c);
    if !u.is_finite() {
        return None;
    }
    Some((side.at(u), side.dir(u)?, u))
}

/// Cut side `c` back from the corner `q` to its tangent point `t` at position `u` along it, in
/// place: the side keeps its id and ends at the new point `n` there. The points it no longer
/// uses are returned for removal.
fn cut_back(sk: &mut Sketch, c: usize, side: &Side, u: f64, q: usize, n: usize, cmd: &str) -> Result<Vec<usize>> {
    let id = id_of(sk, c);
    let Some(kind) = sk.curves.get(c).map(|x| x.kind.clone()) else { return Ok(Vec::new()) };
    match kind {
        CurveKind::Line { .. } | CurveKind::Arc { .. } => {
            if let Some(cu) = sk.curves.get_mut(c) {
                cu.kind.map_points(&|i| if i == q { n } else { i });
            }
            Ok(Vec::new())
        }
        CurveKind::Conic { a, b, apex, .. } => {
            let Some(whole) = side.segs.first() else { return Err(bad(cmd, format!("`{id}` has no shape"))) };
            let t = u.clamp(0.0, 1.0);
            let (first, second) = whole.split_at(t);
            let kept = if side.at_start { second } else { first };
            let Seg2::Conic { apex: new_apex, w, .. } = kept else { return Err(bad(cmd, format!("`{id}` is not a conic"))) };
            let rho = w / (1.0 + w);
            if !(new_apex.is_finite() && rho.is_finite() && rho > 1e-6 && rho < 1.0 - 1e-6) {
                return Err(bad(cmd, format!("that radius leaves too little of `{id}` to stay a conic")));
            }
            let Some(point) = sk.points.get_mut(apex) else { return Err(bad(cmd, format!("`{id}` has no apex point"))) };
            point.pos = new_apex;
            if let Some(cu) = sk.curves.get_mut(c) {
                cu.kind = if side.at_start { CurveKind::Conic { a: n, b, apex, rho } } else { CurveKind::Conic { a, b: n, apex, rho } };
            }
            Ok(Vec::new())
        }
        CurveKind::Spline { pts, .. } => {
            // The kept spans as Bézier pieces: a fit spline's are cubic, a control spline's of
            // its degree (or fewer, with fewer points than that). The spline becomes the control
            // spline of exactly those pieces: their control points, and knots that repeat at
            // each join as often as the degree, where there is more than one piece.
            let kept = if side.at_start { span(&side.segs, u, side.n()) } else { span(&side.segs, 0.0, u) };
            let controls: Vec<Vec<Vec2>> = kept.iter().map(bezier_controls).collect::<Option<_>>().ok_or_else(|| {
                bad(cmd, format!("not supported yet: a fillet on a corner of `{id}`, whose spans are not Béziers"))
            })?;
            let d = controls.first().map(|c| c.len().saturating_sub(1)).unwrap_or(0);
            if d == 0 || d > solidcore_record::MAX_BEZIER_DEGREE || controls.iter().any(|c| c.len() != d + 1) {
                return Err(bad(cmd, format!("not supported yet: a fillet on a corner of `{id}`, whose spans differ in degree")));
            }
            let count = controls.len() * d + 1;
            if count > solidcraft_sketch::MAX_SPLINE_POINTS {
                return Err(bad(
                    cmd,
                    format!("not supported yet: a fillet on a corner of `{id}`, whose kept part needs {count} control points (at most {})", solidcraft_sketch::MAX_SPLINE_POINTS),
                ));
            }
            let far = if side.at_start { *pts.last().unwrap_or(&q) } else { *pts.first().unwrap_or(&q) };
            // The control points: the new end, the far end, and new points between them.
            let mut new_pts = vec![if side.at_start { n } else { far }];
            for (k, piece) in controls.iter().enumerate() {
                let last = k + 1 == controls.len();
                for (j, p) in piece.iter().enumerate().skip(1) {
                    if j == d && last {
                        new_pts.push(if side.at_start { far } else { n });
                    } else {
                        new_pts.push(sk.add_point(*p, None)?);
                    }
                }
            }
            let nurbs = if controls.len() > 1 {
                let m = controls.len();
                let mut knots = vec![0.0; d + 1];
                for k in 1..m {
                    knots.extend(std::iter::repeat_n(k as f64, d));
                }
                knots.extend(std::iter::repeat_n(m as f64, d + 1));
                let nurbs = Nurbs { knots, weights: Vec::new() };
                nurbs.check(new_pts.len(), d as u8).map_err(|e| bad(cmd, format!("`{id}`: {e}")))?;
                Some(nurbs)
            } else {
                None
            };
            if let Some(cu) = sk.curves.get_mut(c) {
                cu.kind = CurveKind::Spline { pts: new_pts.clone(), control: true, degree: d as u8, periodic: false, nurbs, handles: Vec::new() };
            }
            Ok(pts.into_iter().filter(|p| !new_pts.contains(p)).collect())
        }
        _ => Err(bad(cmd, format!("`{id}` is not a side of a corner"))),
    }
}

/// The control points of a polynomial Bézier piece: none for an arc or a weighted conic.
fn bezier_controls(s: &Seg2) -> Option<Vec<Vec2>> {
    match *s {
        Seg2::Line { a, b } => Some(vec![a, b]),
        Seg2::Conic { a, apex, b, w } if (w - 1.0).abs() < 1e-12 => Some(vec![a, apex, b]),
        Seg2::Cubic { p0, p1, p2, p3 } => Some(vec![p0, p1, p2, p3]),
        Seg2::Bezier { n, ref p } => Some(p.iter().take(usize::from(n) + 1).copied().collect()),
        Seg2::Conic { .. } | Seg2::Arc { .. } => None,
    }
}

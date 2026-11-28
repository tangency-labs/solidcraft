//! Sketch Fillet at a corner where a conic or a control point spline meets a line, an arc, or
//! another such curve (#9178). The round's centre comes from a fixed-point iteration on the two
//! sides' offsets: each pass meets the tangent lines at the centre's projections, offset by the
//! radius. Both sides are then cut back to their tangent points exactly and in place. A conic
//! stays the conic it was (the rest of the same rational quadratic, its apex moved). A control
//! point spline keeps its degree, its knots and its weights past the cut: the cut parameter is
//! inserted as a knot until the spline splits there, and the cut-away part goes. Each side keeps
//! its id.

use serde_json::Value;
use solidcore_record::{Seg2, Vec2, Vec3};
use solidcraft_geom::bspline::{self, HPoint};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{ConstraintKind, CurveKind, Nurbs, Sketch};

use super::sketch::add_c;
use crate::Result;
use crate::params::{bad, str_};

/// Within this of the corner or a side's far end, a tangent point leaves nothing to round or
/// nothing of the side, as `round_corner` has it.
const NEAR: f64 = 1e-6;

/// The most passes the centre's iteration takes: it converges in a handful.
const MAX_PASSES: usize = 60;

/// Position along the pieces (piece index + local parameter) closest to `p`, and the distance.
fn locate(segs: &[Seg2], p: Vec2) -> (f64, f64) {
    segs.iter()
        .enumerate()
        .map(|(i, s)| {
            let t = s.closest_param(p);
            (i as f64 + t, s.point_at(t).dist(p))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((0.0, f64::INFINITY))
}

/// The point at position `u` along the pieces (piece index + local parameter).
fn point_at(segs: &[Seg2], u: f64) -> Vec2 {
    let n = segs.len();
    let i = (u.floor() as usize).min(n.saturating_sub(1));
    segs.get(i).map(|s| s.point_at((u - i as f64).clamp(0.0, 1.0))).unwrap_or_default()
}

fn ends(sk: &Sketch, c: usize) -> Option<(usize, usize)> {
    sk.curves.get(c).and_then(|c| c.kind.ends())
}

fn id_of(sk: &Sketch, c: usize) -> String {
    sk.curves.get(c).map(|c| c.id.clone()).unwrap_or_default()
}

/// The corner `p` names, of any two curves with ends: the sides and the shared point, found as
/// `corner` finds them (never a linked curve or an ellipse's axis, and the profile sides where
/// construction curves end there too). None when it names no such corner.
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

/// Round a corner with a conic or a control point spline as a side: the arc of radius `r`
/// tangent to both sides nearest the corner, and the tangent constraints. None when the corner
/// is not of that kind (the caller reports why the corner was refused).
pub(super) fn round_freeform_corner(sk: &mut Sketch, p: &Value, r: f64, cmd: &str) -> Result<Option<(usize, Vec<String>)>> {
    let Some((ca, cb, q)) = sides(sk, p) else { return Ok(None) };
    let freeform = |sk: &Sketch, c: usize| sk.curves.get(c).is_some_and(|c| c.kind.is_freeform());
    if !freeform(sk, ca) && !freeform(sk, cb) {
        return Ok(None);
    }
    for c in [ca, cb] {
        if freeform(sk, c) {
            refuse_side(sk, c, q, cmd)?;
            refuse_curve_holds(sk, c, q, cmd)?;
        }
    }
    refuse_kept_corner(sk, q, [ca, cb], cmd)?;
    refuse_line_relations(sk, [ca, cb], cmd)?;
    let Some(pc) = sk.point(q) else { return Ok(None) };
    let side = |c: usize| Side::new(sk, c, q).ok_or_else(|| bad(cmd, format!("`{}` has no shape", id_of(sk, c))));
    let (sa, sb) = (side(ca)?, side(cb)?);
    let (cen, ua, ub) = centre(&sa, &sb, pc, r, cmd)?;
    let (na, nb) = (sk.add_point(sa.at(ua), None)?, sk.add_point(sb.at(ub), None)?);
    let mut gone = vec![q];
    gone.extend(cut_back(sk, ca, &sa, ua, q, na, cmd)?);
    gone.extend(cut_back(sk, cb, &sb, ub, q, nb, cmd)?);
    // A cut side moved its new end onto its exact split point.
    let (ta, tb) = (sk.point(na).unwrap_or_default(), sk.point(nb).unwrap_or_default());
    let ccw = (ta - cen).cross(tb - cen) > 0.0;
    let arc =
        if ccw { sk.add_arc(cen, ta, tb, [None, Some(na), Some(nb)], None)? } else { sk.add_arc(cen, tb, ta, [None, Some(nb), Some(na)], None)? };
    let cons = vec![add_c(sk, ConstraintKind::Tangent { a: ca, b: arc })?, add_c(sk, ConstraintKind::Tangent { a: cb, b: arc })?];
    let arc_id = id_of(sk, arc);
    // The corner and the points a cut-back side no longer uses go, unless another curve uses
    // them (never the origin).
    gone.retain(|p| *p != 0 && !sk.curves.iter().any(|c| c.kind.uses(*p)));
    gone.sort_unstable();
    gone.dedup();
    sk.remove_points(&gone);
    let arc = sk.curve_index(&arc_id).ok_or_else(|| bad(cmd, "the fillet arc is gone"))?;
    Ok(Some((arc, cons)))
}

/// What a free-form side of such a corner must be: a conic whose apex point nothing else holds
/// (the apex moves with the cut), or a control point spline with exact pieces. Neither may start
/// and end at the corner.
fn refuse_side(sk: &Sketch, c: usize, q: usize, cmd: &str) -> Result<()> {
    let id = id_of(sk, c);
    let Some(curve) = sk.curves.get(c) else { return Ok(()) };
    if ends(sk, c).is_some_and(|(a, b)| a == q && b == q) {
        return Err(bad(cmd, format!("not supported yet: a fillet where `{id}` starts and ends at the corner")));
    }
    match &curve.kind {
        CurveKind::Conic { apex, .. } => {
            if let Some(why) = held_by(sk, *apex, &[c]) {
                return Err(bad(cmd, format!("`{id}`'s apex point moves when the corner is rounded, and {why}")));
            }
        }
        CurveKind::Spline { control: false, .. } => {
            return Err(bad(cmd, format!("not supported yet: a fillet on `{id}`, a fit point spline")));
        }
        CurveKind::Spline { degree, nurbs, .. } if sk.exact_segs(c).is_none() => {
            let rational = nurbs.as_ref().is_some_and(|n| n.weights.iter().zip(n.weights.iter().skip(1)).any(|(u, v)| u != v));
            let why = if usize::from(*degree) > solidcore_record::MAX_BEZIER_DEGREE {
                format!("a spline of degree {degree}, above {}", solidcore_record::MAX_BEZIER_DEGREE)
            } else if rational && *degree != 2 {
                format!("a rational spline of degree {degree}")
            } else {
                "a spline whose points or knots give it no exact shape".to_string()
            };
            return Err(bad(cmd, format!("not supported yet: a fillet on `{id}`, {why}")));
        }
        // A rational span's exact conic runs on a parameter of its own once its end weights
        // differ, so a cut placed by that parameter would miss the tangent point on the knots.
        CurveKind::Spline { nurbs: Some(n), .. } if n.weights.windows(2).any(|w| matches!(w, [u, v] if u != v)) => {
            return Err(bad(cmd, format!("not supported yet: a fillet on `{id}`, a rational spline whose weights differ")));
        }
        _ => {}
    }
    Ok(())
}

/// A constraint on free-form side `c` itself would change its meaning once the side is cut
/// short: a point held on it or along it, a tangency that can touch anywhere along it, a length.
/// Only a tangency or smoothness at the side's far end, which stays, is kept.
fn refuse_curve_holds(sk: &Sketch, c: usize, q: usize, cmd: &str) -> Result<()> {
    let far = ends(sk, c).map(|(a, b)| if a == q { b } else { a });
    for k in &sk.constraints {
        if !k.kind.indices().1.contains(&c) {
            continue;
        }
        if let ConstraintKind::Tangent { a, b } | ConstraintKind::Smooth { a, b } = k.kind {
            let other = if a == c { b } else { a };
            if far.is_some_and(|f| ends(sk, other).is_some_and(|(x, y)| x == f || y == f)) {
                continue;
            }
        }
        let id = id_of(sk, c);
        return Err(bad(cmd, format!("`{}` holds something on `{id}`, and the fillet cuts `{id}` back, which would change what it holds", k.id)));
    }
    Ok(())
}

/// A line side is cut short too, so an equal length or a midpoint on it would measure the
/// shorter line. A line fillet keeps the original side as construction geometry through the
/// sharp corner for these, which a corner with a conic or spline side cannot keep.
fn refuse_line_relations(sk: &Sketch, sides: [usize; 2], cmd: &str) -> Result<()> {
    for c in sides {
        if !matches!(sk.curves.get(c).map(|x| &x.kind), Some(CurveKind::Line { .. })) {
            continue;
        }
        let measures = |k: &ConstraintKind| {
            matches!(*k, ConstraintKind::Equal { a, b } if a == c || b == c) || matches!(*k, ConstraintKind::Midpoint { l, .. } if l == c)
        };
        if let Some(k) = sk.constraints.iter().find(|k| measures(&k.kind)) {
            let id = id_of(sk, c);
            return Err(bad(
                cmd,
                format!("`{}` measures the length or midpoint of `{id}`, and the fillet shortens `{id}`, which would change what it measures", k.id),
            ));
        }
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
        Some(why) => Err(bad(cmd, format!("a corner of a conic or spline cannot stay as a virtual sharp corner, and this one would have to: {why}"))),
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
        let segs = if sk.curves.get(c)?.kind.is_freeform() { sk.exact_segs(c)? } else { sk.exact_pieces(c) };
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
    /// The piece holding position `u` and the parameter along it.
    fn piece(&self, u: f64) -> (usize, f64) {
        let i = (u.floor().max(0.0) as usize).min(self.segs.len().saturating_sub(1));
        (i, (u - i as f64).clamp(0.0, 1.0))
    }
    /// The unit direction of travel at position `u`, None where the curve has none.
    fn dir(&self, u: f64) -> Option<Vec2> {
        let (i, t) = self.piece(u);
        let d = self.segs.get(i)?.derivative(t);
        d.is_finite().then(|| d.normalized()).flatten()
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
/// tangent points along each side. A fixed-point iteration on the sides' offsets: from the
/// tangent lines at the corner, each pass meets the two lines offset by `r`, projects that point
/// onto both sides and takes the tangent lines there, until the point stays put (a line's tangent
/// line is itself). Each tangent line keeps the side's direction of travel, so an offset stays on
/// one side of it from pass to pass. The offsets can lie four ways. Of the results whose tangent
/// points sit inside both sides, the nearest the corner wins.
fn centre(sa: &Side, sb: &Side, pc: Vec2, r: f64, cmd: &str) -> Result<(Vec2, f64, f64)> {
    let (Some(da), Some(db)) = (sa.dir(sa.corner_u()), sb.dir(sb.corner_u())) else { return Err(bad(cmd, "degenerate curve")) };
    // The directions away from the corner along each side.
    let (wa, wb) = (if sa.at_start { da } else { -da }, if sb.at_start { db } else { -db });
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
            // Each side's tangent line (a point of it and its unit direction of travel) and the
            // position of that point along the side.
            let (mut la, mut lb) = ((pc, da, sa.corner_u()), (pc, db, sb.corner_u()));
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

/// Cut side `c` back from the corner `q` to its tangent point at position `u` along it, in
/// place: the side keeps its id and ends at the new point `n`, which moves onto the exact split
/// point. The points it no longer uses are returned for removal.
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
            // `split_at` keeps the parameter off the ends by this much.
            let t = u.clamp(1e-9, 1.0 - 1e-9);
            let (first, second) = whole.split_at(t);
            let kept = if side.at_start { second } else { first };
            let Seg2::Conic { a: ka, apex: new_apex, b: kb, w } = kept else { return Err(bad(cmd, format!("`{id}` is not a conic"))) };
            let rho = w / (1.0 + w);
            let cut = if side.at_start { ka } else { kb };
            if !(new_apex.is_finite() && cut.is_finite() && rho.is_finite() && rho > 1e-6 && rho < 1.0 - 1e-6) {
                return Err(bad(cmd, format!("that radius leaves too little of `{id}` to stay a conic")));
            }
            for (point, at) in [(apex, new_apex), (n, cut)] {
                let Some(pt) = sk.points.get_mut(point) else { return Err(bad(cmd, format!("`{id}` has lost a point"))) };
                pt.pos = at;
            }
            if let Some(cu) = sk.curves.get_mut(c) {
                cu.kind = if side.at_start { CurveKind::Conic { a: n, b, apex, rho } } else { CurveKind::Conic { a, b: n, apex, rho } };
            }
            Ok(Vec::new())
        }
        CurveKind::Spline { pts, control: true, periodic: false, degree, nurbs, .. } => {
            cut_spline(sk, c, side, u, q, n, (&pts, degree, nurbs.as_ref()), cmd)
        }
        _ => Err(bad(cmd, format!("`{id}` is not a side of a corner"))),
    }
}

/// Cut control point spline `c` (its points, degree and own knots) at position `u` along its
/// pieces, keeping the part away from the corner `q`. The cut parameter is inserted as a knot
/// until it repeats as often as the degree, which splits the spline there without changing its
/// shape (Boehm's insertion), and the cut-away part goes. The kept points that insertion left
/// where they were stay as they are. The ones it made reuse the spline's points that went, then
/// new points. Refused when a point that moves or goes is held by anything but the spline.
#[allow(clippy::too_many_arguments)]
fn cut_spline(
    sk: &mut Sketch,
    c: usize,
    side: &Side,
    u: f64,
    q: usize,
    n: usize,
    spline: (&[usize], u8, Option<&Nurbs>),
    cmd: &str,
) -> Result<Vec<usize>> {
    let (pts, degree, nurbs) = spline;
    let id = id_of(sk, c);
    let failed = || bad(cmd, format!("`{id}` could not be cut at the tangent point"));
    let count = pts.len();
    let d = usize::from(degree).min(count.saturating_sub(1)).max(1);
    // The spline's knots: its own, or the clamped uniform ones a spline without them has.
    let (knots, weights): (Vec<f64>, Vec<f64>) = match nurbs {
        Some(nb) => (nb.knots.clone(), nb.weights.clone()),
        None => {
            let spans = count - d;
            let mut k = vec![0.0; d + 1];
            k.extend((1..spans).map(|i| i as f64 / spans as f64));
            k.extend(std::iter::repeat_n(1.0, d + 1));
            (k, Vec::new())
        }
    };
    let weight = |i: usize| weights.get(i).copied().unwrap_or(1.0);
    let mut ctrl: Vec<HPoint> = pts
        .iter()
        .enumerate()
        .map(|(i, p)| sk.point(*p).map(|v| HPoint::weighted(Vec3::new(v.x, v.y, 0.0), weight(i))))
        .collect::<Option<_>>()
        .ok_or_else(failed)?;
    // Each exact piece is one non-empty knot span, in order, over which its parameter runs
    // linearly.
    let spans: Vec<(f64, f64)> = knots
        .get(d..=count)
        .ok_or_else(failed)?
        .windows(2)
        .filter_map(|w| match *w {
            [lo, hi] if lo < hi => Some((lo, hi)),
            _ => None,
        })
        .collect();
    if spans.len() != side.segs.len() {
        return Err(failed());
    }
    let (i, t) = side.piece(u);
    let (lo, hi) = spans.get(i).copied().ok_or_else(failed)?;
    let s = lo + t * (hi - lo);
    // Which original point each control point is, while insertion leaves it in place.
    let mut ids: Vec<Option<usize>> = pts.iter().copied().map(Some).collect();
    let mut k = knots;
    for _ in 0..d {
        if k.iter().filter(|x| **x == s).count() >= d {
            break;
        }
        let span = bspline::find_span(&k, d, ctrl.len(), s).ok_or_else(failed)?;
        let kn = |j: usize| k.get(j).copied().unwrap_or(0.0);
        // As `bspline::insert_knot` places them.
        let moved: Vec<Option<usize>> = (0..=ids.len())
            .map(|j| {
                if j + d <= span {
                    ids.get(j).copied().flatten()
                } else if j > span {
                    ids.get(j - 1).copied().flatten()
                } else if kn(j + d) > kn(j) {
                    None
                } else {
                    ids.get(j).copied().flatten()
                }
            })
            .collect();
        (k, ctrl) = bspline::insert_knot(d, &k, &ctrl, s).map_err(|e| bad(cmd, format!("`{id}`: {e}")))?;
        ids = moved;
    }
    // The spline splits at control point r - 1, where the knot s starts.
    let r = k.iter().position(|x| *x == s).filter(|r| *r >= 1).ok_or_else(failed)?;
    let ends_s = std::iter::repeat_n(s, d + 1);
    let (kept, kept_ids, kept_knots): (Vec<HPoint>, Vec<Option<usize>>, Vec<f64>) = if side.at_start {
        let tail = k.get(r + d..).ok_or_else(failed)?;
        (ctrl.get(r - 1..).ok_or_else(failed)?.to_vec(), ids.get(r - 1..).ok_or_else(failed)?.to_vec(), ends_s.chain(tail.iter().copied()).collect())
    } else {
        let head = k.get(..r).ok_or_else(failed)?;
        (ctrl.get(..r).ok_or_else(failed)?.to_vec(), ids.get(..r).ok_or_else(failed)?.to_vec(), head.iter().copied().chain(ends_s).collect())
    };
    if kept.len() > solidcraft_sketch::MAX_SPLINE_POINTS {
        return Err(bad(
            cmd,
            format!(
                "not supported yet: a fillet on `{id}`, whose kept part needs {} control points (at most {})",
                kept.len(),
                solidcraft_sketch::MAX_SPLINE_POINTS
            ),
        ));
    }
    let cut_slot = if side.at_start { 0 } else { kept.len().saturating_sub(1) };
    let stay: Vec<usize> = kept_ids.iter().enumerate().filter(|(j, _)| *j != cut_slot).filter_map(|(_, i)| *i).collect();
    // The spline's points that move or go: nothing but the spline may hold them.
    let pool: Vec<usize> = pts.iter().copied().filter(|p| *p != q && !stay.contains(p)).collect();
    for p in &pool {
        if let Some(why) = held_by(sk, *p, &[c]) {
            let name = sk.points.get(*p).map(|x| x.id.clone()).unwrap_or_default();
            return Err(bad(cmd, format!("`{id}`'s control point `{name}` moves or goes when the corner is rounded, and {why}")));
        }
    }
    let mut reuse = pool.into_iter();
    let mut new_pts = Vec::with_capacity(kept.len());
    for (j, (h, orig)) in kept.iter().zip(&kept_ids).enumerate() {
        let at = h.project().map(|v| Vec2::new(v.x, v.y)).filter(|v| v.is_finite()).ok_or_else(failed)?;
        let index = match orig {
            _ if j == cut_slot => n,
            Some(i) => *i,
            None => match reuse.next() {
                Some(p) => p,
                None => sk.add_point(at, None)?,
            },
        };
        if orig.is_none() || j == cut_slot {
            let Some(pt) = sk.points.get_mut(index) else { return Err(failed()) };
            pt.pos = at;
        }
        new_pts.push(index);
    }
    let rational = !weights.is_empty() || kept.iter().any(|h| h.w != 1.0);
    let nurbs = Nurbs { knots: kept_knots, weights: if rational { kept.iter().map(|h| h.w).collect() } else { Vec::new() } };
    nurbs.check(new_pts.len(), d as u8).map_err(|e| bad(cmd, format!("`{id}`: {e}")))?;
    if let Some(cu) = sk.curves.get_mut(c) {
        cu.kind = CurveKind::Spline { pts: new_pts, control: true, degree: d as u8, periodic: false, nurbs: Some(nurbs), handles: Vec::new() };
    }
    Ok(reuse.collect())
}

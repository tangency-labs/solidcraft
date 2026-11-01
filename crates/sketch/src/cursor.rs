//! Feasible cursor motion: optimize only within the design Jacobian's null space, then
//! retract to the constraint manifold. Mouse error never competes with design residuals.
use super::{
    ACCEPT, Hold, Layout, State, TOL, build_blocks, columns, initial_x, layout, lm_capped_with_budget, local_system, max_abs, points_on_curves,
    residual_work, system, within_budget,
};
use crate::{
    Budget, CurveKind, Sketch,
    linalg::{Mat, rref},
    union_find::{find, union},
};
use solidcraft_geom::Vec2;

const MAX_VARIABLES: usize = 400;
const MAX_STEPS: usize = 96;
const CORRECTION_ITERS: usize = 16;
const TOTAL_CORRECTION_ITERS: usize = 256;
/// Pivot tolerance of the null space, on the Jacobian equilibrated to unit rows and columns.
/// Redundant constraints (a parallelogram held both parallel and equal on each pair of sides)
/// are dependent only at an exact solution; a pose corrected to `TOL` leaves their rows
/// independent by about that much, and a tighter tolerance counts that as rank and finds no
/// freedom (#4585). The solver's rank analysis, which reports the sketch's degrees of freedom,
/// uses 1e-7 too.
const NULL_SPACE_TOL: f64 = 1e-7;

/// Move a point toward `target` using freedom left by the design constraints. `max_step`
/// bounds the full variable displacement before constraint correction; `valid` can reject
/// a corrected candidate (for example, an arc crossing its own start). Only feasible,
/// cursor-improving candidates are committed. False leaves the sketch unchanged when the
/// input is unsolved, invalid or outside the dense-work limit. Work is bounded; true may
/// retain the best feasible pose when the iteration or optional shared frame-work budget is
/// exhausted. The work budget includes Jacobian, null-space and LM arithmetic, not wall time.
pub fn move_point_towards(
    sk: &mut Sketch,
    point: usize,
    target: Vec2,
    max_step: f64,
    budget: Option<&mut Budget>,
    valid: impl Fn(&Sketch, &Sketch) -> bool,
) -> bool {
    move_cursor_towards(sk, &[(point, Vec2::X)], target, max_step, budget, valid)
}

/// Move a grabbed position attached to a line toward the cursor. `grab.x` is its fraction
/// along the line (0 at its start, 1 at its end); `grab.y` is the signed perpendicular offset,
/// also divided by the line's length. The offset rotates and scales with the line.
/// The line may rotate: its endpoints are not independently pinned to a translation.
/// Feasibility, work limits and the return value have the same meaning as [`move_point_towards`].
pub fn move_line_towards(
    sk: &mut Sketch,
    line: usize,
    grab: Vec2,
    target: Vec2,
    max_step: f64,
    budget: Option<&mut Budget>,
    valid: impl Fn(&Sketch, &Sketch) -> bool,
) -> bool {
    if !grab.is_finite() {
        return false;
    }
    let Some(CurveKind::Line { a, b }) = sk.curves.get(line).map(|c| &c.kind) else { return false };
    let (Some(pa), Some(pb)) = (sk.point(*a), sk.point(*b)) else { return false };
    let d = pb - pa;
    if !(d.dot(d) > 1e-18 && crate::model::in_range(pa + weighted(d, grab))) {
        return false;
    }
    move_cursor_towards(sk, &[(*a, Vec2::new(1.0 - grab.x, -grab.y)), (*b, grab)], target, max_step, budget, valid)
}

// Both callers' endpoint weights sum to (1, 0): translating the anchors translates the grab.
fn move_cursor_towards(
    sk: &mut Sketch,
    points: &[(usize, Vec2)],
    target: Vec2,
    max_step: f64,
    mut budget: Option<&mut Budget>,
    valid: impl Fn(&Sketch, &Sketch) -> bool,
) -> bool {
    let setup_work = sk.points.len().saturating_add(sk.curves.len()).saturating_add(sk.constraints.len()).max(1);
    if !within_budget(&mut budget, setup_work) {
        return false;
    }
    if !crate::model::in_range(target) || !max_step.is_finite() || max_step <= 0.0 || !in_range(sk) {
        return false;
    }
    let problem = sk.clone();
    let lay = layout(&problem, &Hold::default());
    let active: Vec<_> =
        points.iter().filter(|(_, w)| w.x != 0.0 || w.y != 0.0).filter_map(|(p, w)| Some((lay.pvar.get(*p).copied().flatten()?, *w))).collect();
    let Some(&(px, _)) = active.first() else { return false };
    let (blocks, _) = build_blocks(&problem, &lay, &points_on_curves(&problem));
    let mut parent: Vec<usize> = (0..lay.n).collect();
    for &(v, _) in &active {
        union(&mut parent, px, v);
        union(&mut parent, px, v + 1);
    }
    for (_, vars) in &blocks {
        if let Some(&first) = vars.first() {
            for &v in vars.iter().skip(1) {
                union(&mut parent, first, v);
            }
        }
    }
    let root = find(&mut parent, px);
    let vars: Vec<usize> = (0..lay.n).filter(|v| find(&mut parent, *v) == root).collect();
    if vars.len() > MAX_VARIABLES {
        return false;
    }
    let sub: Vec<_> =
        blocks.iter().filter(|(_, v)| v.iter().any(|i| vars.binary_search(i).is_ok())).map(|(b, v)| (super::copy_block(b), v.clone())).collect();
    // Reserve both initial and final all-block validation before starting cursor work.
    // An exhausted search can then keep its best feasible pose, including unrelated blocks.
    if !within_budget(&mut budget, residual_work(&lay, &blocks).saturating_mul(2)) {
        return false;
    }
    let mut x = initial_x(&problem, &lay);
    let initial_residual = max_abs(&system(&problem, &lay, &blocks, &mut x, false).0);
    if initial_residual >= ACCEPT {
        return false;
    }
    let mut col = vec![usize::MAX; lay.n];
    columns(&mut col, &vars, &vars);
    let axes: Vec<_> = active.iter().filter_map(|(v, w)| Some((vars.binary_search(v).ok()?, *w))).collect();
    let position = |x: &[f64]| {
        let state = State { sk: &problem, lay: &lay, x };
        // Relative coordinates avoid cancellation when a small line is far from the origin.
        let base = points.first().map_or(Vec2::ZERO, |(p, _)| state.p(*p));
        points.iter().fold(base, |at, (p, w)| at + weighted(state.p(*p) - base, *w))
    };
    let mut best = problem.clone();
    let mut spent = 0;
    // Held feasibility solves accept up to ACCEPT. A tiny, nearly feasible hard pin can
    // precede a failed step; refine it before constructing a tangent, rather than losing
    // the cursor objective just because the previous pose is between TOL and ACCEPT.
    // An ill-conditioned sketch (a near-zero-length construction line makes its angle row
    // unsolvable to TOL, #7635) stalls that refinement between TOL and ACCEPT. The solver
    // itself calls such a pose solved, so the cursor keeps working from it and accepts its
    // corrected trials at the same tolerance the final validation uses.
    let mut trial_tol = TOL;
    if initial_residual >= TOL {
        let Some((its, residual)) = lm_capped_with_budget(&problem, &lay, &sub, &vars, &mut x, &col, CORRECTION_ITERS, budget.as_deref_mut()) else {
            return false;
        };
        spent += its.max(1);
        if residual >= ACCEPT {
            return false;
        }
        if residual >= TOL {
            trial_tol = ACCEPT;
        }
        apply(&mut best, &lay, &x);
        if !in_range(&best) || !valid(&problem, &best) {
            return false;
        }
    }
    let mut gain = 1.0_f64;
    for _ in 0..MAX_STEPS {
        let at = position(&x);
        let error = target - at;
        if error.len() < 1e-7 || spent >= TOTAL_CORRECTION_ITERS {
            break;
        }
        let n = vars.len();
        let jacobian_work = residual_work(&lay, &sub).saturating_mul(n.saturating_mul(2).saturating_add(1));
        if !within_budget(&mut budget, jacobian_work) {
            break;
        }
        let (_, j, _) = local_system(&problem, &lay, &sub, &mut x, &col, n);
        let null_work = n.saturating_mul(n).saturating_mul(j.rows.saturating_add(n));
        if !within_budget(&mut budget, null_work) {
            break;
        }
        let z = null_space(j);
        if z.is_empty() {
            break;
        }
        let mut direction = target_step(&z, &axes, error);
        let norm = length(&direction);
        // Adapt to curvature of the reachable locus. Inside a circle the Gauss-Newton
        // cursor step can be arbitrarily small although useful freedom remains.
        let scale = if norm > 0.0 { gain.min(max_step / norm) } else { gain };
        for v in &mut direction {
            *v *= scale;
        }
        let mut directions = vec![(direction, false)];
        // A cursor across a circular locus has (nearly) zero first derivative at the
        // farthest point. If line search stalls, probe both tangent directions. Testing
        // only an exactly zero gradient would strand reversed drags after rounding.
        let influence = |v: &&Vec<f64>| cursor_direction(v, &axes).len();
        let mut probes: Vec<_> = z.iter().collect();
        probes.sort_by(|a, b| influence(b).total_cmp(&influence(a)));
        for v in probes.into_iter().take(2) {
            directions.push((v.iter().map(|x| x * max_step).collect(), true));
            directions.push((v.iter().map(|x| -x * max_step).collect(), true));
        }
        let old_cost = error.dot(error);
        let mut accepted = None;
        'search: for (direction, probe) in directions {
            if length(&direction) < 1e-12 {
                continue;
            }
            for backtrack in 0..if probe { 1 } else { 8 } {
                if spent >= TOTAL_CORRECTION_ITERS {
                    break 'search;
                }
                let alpha = 0.5_f64.powi(backtrack);
                let mut trial = x.clone();
                for (&v, d) in vars.iter().zip(&direction) {
                    if let Some(t) = trial.get_mut(v) {
                        *t += alpha * d;
                    }
                }
                let Some((its, residual)) = lm_capped_with_budget(
                    &problem,
                    &lay,
                    &sub,
                    &vars,
                    &mut trial,
                    &col,
                    CORRECTION_ITERS.min(TOTAL_CORRECTION_ITERS - spent),
                    budget.as_deref_mut(),
                ) else {
                    break 'search;
                };
                spent += its.max(1);
                if !(residual < trial_tol) {
                    if budget.as_ref().is_some_and(|b| b.exhausted()) {
                        break 'search;
                    }
                    continue;
                }
                let next = position(&trial);
                let cost = (target - next).dot(target - next);
                // The cursor itself must move. Retracting onto the constraints moves it by
                // rounding even along freedom that can't reach it (the far end of a line held only
                // by its direction): taking such a step for the rounding moved that end a whole
                // step, and step after step it walked away or collapsed its line to a point (#8944).
                if !cost.is_finite() || cost >= old_cost - 1e-14 * old_cost.max(1.0) || next.dist(at) < trial_tol {
                    continue;
                }
                if !within_budget(&mut budget, setup_work.saturating_mul(4)) {
                    break 'search;
                }
                let mut candidate = best.clone();
                apply(&mut candidate, &lay, &trial);
                if !in_range(&candidate) || !valid(&best, &candidate) {
                    continue;
                }
                accepted = Some((trial, candidate, ((if probe { 1.0 } else { scale }) * alpha * 1.5).clamp(1e-8, 1e8)));
                break 'search;
            }
        }
        let Some((next, candidate, next_gain)) = accepted else { break };
        x = next;
        best = candidate;
        gain = next_gain;
    }
    // Full constraint validation includes constant blocks and disconnected components.
    if max_abs(&system(&best, &lay, &blocks, &mut x, false).0) >= ACCEPT {
        return false;
    }
    super::refresh_driven_dimensions(&mut best);
    *sk = best;
    true
}

fn length(v: &[f64]) -> f64 {
    v.iter().map(|x| x * x).sum::<f64>().sqrt()
}

fn in_range(sk: &Sketch) -> bool {
    sk.points.iter().all(|p| crate::model::in_range(p.pos))
        && sk
            .curves
            .iter()
            .all(|c| !matches!(c.kind, CurveKind::Circle { r, .. } | CurveKind::Ellipse { r, .. } if !crate::model::radius_in_range(r)))
}

fn null_space(j: Mat) -> Vec<Vec<f64>> {
    // Equilibrate before the rank analysis. A near-zero-length construction line makes its
    // angle row's entries on that line ~1e7 while the same row's entries on the line it
    // measures are ~1: per-row scaling alone hides those (physically meaningful) small
    // entries below the pivot tolerance, and the null space then lets the measured line
    // rotate freely (#7635). Scaling each column by its largest entry first brings the
    // row's entries onto one scale; the null vectors are mapped back afterwards.
    let mut col = vec![0.0_f64; j.cols];
    for (c, m) in col.iter_mut().enumerate() {
        for r in 0..j.rows {
            *m = m.max(j.get(r, c).abs());
        }
    }
    let inv: Vec<f64> = col.iter().map(|m| 1.0 / m.max(1.0)).collect();
    let mut a = Mat::zeros(j.rows, j.cols);
    for r in 0..j.rows {
        let scale = (0..j.cols).map(|c| j.get(r, c).abs() * inv[c]).fold(0.0_f64, f64::max);
        if scale > 0.0 {
            for (c, &ic) in inv.iter().enumerate() {
                a.set(r, c, j.get(r, c) * ic / scale);
            }
        }
    }
    let pivots = rref(&mut a, NULL_SPACE_TOL);
    let mut z: Vec<Vec<f64>> = Vec::new();
    for free in (0..a.cols).filter(|c| !pivots.contains(c)) {
        let mut v = vec![0.0; a.cols];
        if let Some(x) = v.get_mut(free) {
            *x = 1.0;
        }
        for (row, &pivot) in pivots.iter().enumerate() {
            if let Some(x) = v.get_mut(pivot) {
                *x = -a.get(row, free);
            }
        }
        for (x, &ic) in v.iter_mut().zip(&inv) {
            *x *= ic;
        }
        for _ in 0..2 {
            for q in &z {
                let d: f64 = v.iter().zip(q).map(|(a, b)| a * b).sum();
                for (a, b) in v.iter_mut().zip(q) {
                    *a -= d * b;
                }
            }
        }
        let norm = length(&v);
        if norm.is_finite() && norm > 1e-10 {
            for x in &mut v {
                *x /= norm;
            }
            z.push(v);
        }
    }
    z
}

/// The second weight component multiplies a 90-degree rotation of the endpoint.
fn weighted(v: Vec2, w: Vec2) -> Vec2 {
    if w.y == 0.0 { v * w.x } else { Vec2::new(v.x * w.x - v.y * w.y, v.y * w.x + v.x * w.y) }
}

fn cursor_direction(v: &[f64], axes: &[(usize, Vec2)]) -> Vec2 {
    axes.iter().fold(Vec2::ZERO, |at, (i, w)| at + weighted(Vec2::new(v.get(*i).copied().unwrap_or(0.0), v.get(*i + 1).copied().unwrap_or(0.0)), *w))
}

fn target_step(z: &[Vec<f64>], axes: &[(usize, Vec2)], error: Vec2) -> Vec<f64> {
    let mut a = 0.0;
    let mut b = 0.0;
    let mut c = 0.0;
    for q in z {
        let Vec2 { x, y } = cursor_direction(q, axes);
        a += x * x;
        b += x * y;
        c += y * y;
    }
    let high = 0.5 * (a + c + (a - c).hypot(2.0 * b));
    let low = if high > 0.0 { (a * c - b * b).max(0.0) / high } else { 0.0 };
    let axis = if a >= c { Vec2::new(high - c, b) } else { Vec2::new(b, high - a) }.normalized().unwrap_or(if a >= c { Vec2::X } else { Vec2::Y });
    let other = Vec2::new(-axis.y, axis.x);
    let mut u = if high > 1e-12 { axis * (axis.dot(error) / high) } else { Vec2::ZERO };
    if low > high * 1e-8 {
        u += other * (other.dot(error) / low);
    }
    let mut step = vec![0.0; z.first().map_or(0, Vec::len)];
    for q in z {
        let d = cursor_direction(q, axes).dot(u);
        for (x, v) in step.iter_mut().zip(q) {
            *x += d * v;
        }
    }
    step
}

fn apply(sk: &mut Sketch, lay: &Layout, x: &[f64]) {
    for (i, p) in sk.points.iter_mut().enumerate() {
        if let Some(v) = lay.pvar.get(i).copied().flatten()
            && let (Some(a), Some(b)) = (x.get(v), x.get(v + 1))
        {
            p.pos = Vec2::new(*a, *b);
        }
    }
    for (i, c) in sk.curves.iter_mut().enumerate() {
        if let Some(v) = lay.rvar.get(i).copied().flatten()
            && let Some(value) = x.get(v)
            && let CurveKind::Circle { r, .. } | CurveKind::Ellipse { r, .. } = &mut c.kind
        {
            *r = *value;
        }
    }
}

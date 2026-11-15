//! Inverse kinematics (`joint.reach`): move the values of the joints on an occurrence's chain so a
//! point of it reaches a target (and an axis of it points a given way), every value within its
//! limits, motion links followed, and the closed loops those values are in kept closed.

use solidcraft_geom::prelude::*;
use std::cell::Cell;
use std::collections::BTreeSet;
use std::f64::consts::{PI, TAU};

use solidcore_record::Vec3;
use solidcraft_sketch::linalg;

use super::{
    Assembly, Held, JointKind, MAX_LOOP_VALUES, MAX_LOOP_WORK, MAX_LOOPS, Solution, Tree, Values, apply_links, build, close_within, jacobian,
    joint_values, largest, limit_bounds, link_chains, loop_groups, loop_scale, loose_gap, unknowns,
};
use crate::{Document, ModelState, apply_point, apply_vector, mat_mul};

/// How close counts as reached: mm for the point, degrees for the axis.
pub const REACH_TOL: f64 = 1e-4;

/// What a reach asks.
#[derive(Clone, Debug)]
pub struct Reach {
    /// The occurrence that reaches.
    pub occurrence: u64,
    /// Its point that goes to `to`, in its own (component) coordinates.
    pub point: Vec3,
    /// Where the point goes (world).
    pub to: Vec3,
    /// A unit axis of it (its own coordinates) and the unit world direction it turns to.
    pub axis: Option<(Vec3, Vec3)>,
    /// The joints whose values move toward the target (`None`: every joint on its chain). The
    /// other joints of a closed loop they are in follow.
    pub joints: Option<BTreeSet<u64>>,
}

/// How a reach ended.
#[derive(Clone, Debug, Default)]
pub struct Reached {
    /// The new stored values of the joints that moved (joint id → values).
    pub values: Values,
    /// Short of the target: the values it ended on a limit of, with the target pulling past it
    /// (joint, value index, the upper limit).
    pub limits: Vec<(u64, usize, bool)>,
    /// Short of the target: the closing joint of a loop that stopped it (the chain alone, the
    /// loop left open, gets closer).
    pub stopped_by_loop: Option<u64>,
}

/// How much a loop's gap (mm) outweighs a miss of the target (mm) in a step: enough to keep the
/// step along the mechanism, so putting it back on the loops moves it little.
const LOOP_WEIGHT: f64 = 1e3;
/// Steps a reach takes at most, each moving no value more than 10° or a tenth of the chain's
/// size (so it follows the mechanism's branch).
const MAX_REACH_STEPS: usize = 200;
const STEP_ANGLE: f64 = 10.0;
const STEP_SHARE: f64 = 0.1;

/// Move the values of the joints on `r.occurrence`'s chain (those in `r.joints`, when given) so
/// its point gets to `r.to`, or as close as the limits and closed loops let it: the nearest pose,
/// found from where it is (it stays on the mechanism's branch). Values a motion link sets move
/// through the start of their link chain; the other values of the loops they are in follow. An
/// error says why nothing can move.
pub fn reach(doc: &Document, st: &ModelState, r: &Reach) -> Result<Reached, String> {
    let asm = &doc.assembly;
    let vals = joint_values(asm);
    let built = build(doc, st, &vals, &mut Solution::default());
    let tree = &built.tree;
    if tree.fixed.contains_key(&r.occurrence) {
        return Err("it is fixed in place (grounded, or the part its joints are placed from)".into());
    }
    if !tree.parent.contains_key(&r.occurrence) {
        return Err("no joint places it: it is not connected to anything grounded (ground a part, or join it to a grounded part)".into());
    }
    let linked: BTreeSet<(u64, usize)> = asm.links.iter().map(|l| (l.b, l.ib)).collect();
    let allowed = |id: &u64| r.joints.as_ref().is_none_or(|js| js.contains(id));
    let path: BTreeSet<u64> = tree.path(r.occurrence).1.iter().map(|(k, _)| *k).collect();
    // The values that move it: those of the joints on its chain (the ones given), a linked one
    // through the start of its link chain (given or not), and the starts given of the chain's
    // other linked values.
    let chosen: BTreeSet<u64> = path.iter().copied().filter(allowed).collect();
    let mut drive: Vec<(u64, usize)> = Vec::new();
    for j in chosen.iter().filter_map(|id| tree.joints.get(id)) {
        drive.extend((0..j.kind.dofs().len()).map(|i| (j.id, i)).filter(|v| !linked.contains(v)));
    }
    let starts = link_chains(asm, tree, &path, &linked).1.into_iter().filter(|v| allowed(&v.0));
    for v in link_chains(asm, tree, &chosen, &linked).1.into_iter().chain(starts) {
        if !drive.contains(&v) {
            drive.push(v);
        }
    }
    if drive.is_empty() {
        return Err(if r.joints.is_some() {
            "none of the joints given moves it"
        } else {
            "no joint on its chain moves (rigid, or set by motion links)"
        }
        .into());
    }
    // The loops those values are in, and the values of theirs that follow.
    if built.closing.len() > MAX_LOOPS {
        return Err(format!("the design has more than {MAX_LOOPS} closed loops"));
    }
    let groups = loop_groups(tree, &built.closing, asm, &linked);
    let (mut vars, mut closers, mut around) = (drive.clone(), Vec::new(), path.clone());
    let mut taken = BTreeSet::new();
    while vars.len() <= MAX_LOOP_VALUES {
        let more: Vec<usize> =
            groups.iter().enumerate().filter(|(g, (_, _, t))| !taken.contains(g) && t.iter().any(|v| vars.contains(v))).map(|(g, _)| g).collect();
        if more.is_empty() {
            break;
        }
        for (c, a, _) in more.iter().filter_map(|g| groups.get(*g)) {
            closers.extend(c.iter().copied());
            around.extend(a.iter().copied());
            for v in unknowns(tree, a, asm, &linked, &Held::new()) {
                if !vars.contains(&v) {
                    vars.push(v);
                }
            }
        }
        taken.extend(more);
    }
    if vars.len() > MAX_LOOP_VALUES {
        return Err(format!("its chain and loops have more than {MAX_LOOP_VALUES} values"));
    }
    let scale = loop_scale(tree, &around, &vals);
    let raw_bounds = limit_bounds(tree, &vars);
    let mut bounds = raw_bounds.clone();
    let start: Vec<f64> = vars.iter().map(|(id, i)| vals.get(id).and_then(|q| q.get(*i)).copied().unwrap_or(0.0)).collect();
    // A ball joint moves by a turn from where it starts, not by its Euler angles (`BallTurn`).
    let balls = ball_turns(tree, &vars, &start, &raw_bounds);
    let mut x0 = start.clone();
    for k in balls.iter().flat_map(|b| b.at..b.at + 3) {
        if let (Some(x), Some(b)) = (x0.get_mut(k), bounds.get_mut(k)) {
            (*x, *b) = (0.0, (f64::NEG_INFINITY, f64::INFINITY));
        }
    }
    // The joints' values for the solve's `x`.
    let values = |x: &[f64]| -> Vec<f64> {
        let mut v = x.to_vec();
        for b in &balls {
            if let (Some(q), Some(slot)) = (b.values(x), v.get_mut(b.at..b.at + 3)) {
                slot.copy_from_slice(&q);
            }
        }
        v
    };
    // A distance steps by a share of the chain's size, of the way to the target or of the point's
    // reach from its joints, whichever is longest (one slider on its own has no size, and a part
    // turning about a joint far from its point moves that far).
    let point = tree.world(r.occurrence, &vals).map(|t| apply_point(&t, r.point));
    let lever = path
        .iter()
        .filter_map(|id| {
            let (j, frames) = (tree.joints.get(id)?, tree.frames.get(id)?);
            Some(apply_point(&mat_mul(&tree.world(j.a.occurrence, &vals)?, &frames.a), Vec3::ZERO).dist(point?))
        })
        .fold(0.0, f64::max);
    let span = point.map_or(scale, |p| scale.max(p.dist(r.to)).max(lever));
    let cap: Vec<f64> = vars
        .iter()
        .map(|(id, i)| if tree.joints.get(id).is_some_and(|j| j.kind.is_angle(*i)) { STEP_ANGLE.to_radians() } else { STEP_SHARE * span })
        .collect();
    // Every value around the chain and its loops, links included, moves at most a step at a time:
    // a value a link sets can turn far more than the one that drives it, and a loop must stay
    // closed on the way, as joint.drive steps its loops.
    let mut paced: Vec<((u64, usize), f64)> = Vec::new();
    // A ball joint is paced by its turn (`cap`) instead: near pitch 0 a small turn changes its yaw
    // and roll a lot.
    for j in around.iter().filter_map(|id| tree.joints.get(id)).filter(|j| !balls.iter().any(|b| b.id == j.id)) {
        for i in 0..j.kind.dofs().len() {
            paced.push(((j.id, i), if j.kind.is_angle(i) { STEP_ANGLE.to_radians() } else { STEP_SHARE * span }));
        }
    }
    let tol = (1e-10 * scale).min(1e-7);
    let mt = if r.axis.is_some() { 6 } else { 3 };
    let spent = Cell::new(0usize);
    let mut work = vals.clone();
    let mut workspace = super::PlacementWorkspace::new(tree);
    let mut f = |x: &[f64]| misses(&mut workspace, asm, r, scale, &vars, &closers, &values(x), &mut work, &spent);
    let mut x = x0.clone();
    let miss = |res: &[f64]| res.iter().take(mt).map(|v| v * v).sum::<f64>().sqrt();
    let before = f(&x0).map_or(f64::INFINITY, |r| miss(&r));
    let res = approach(&mut f, &|a, b| stride(asm, &vals, &vars, &paced, &values(a), &values(b)), mt, &mut x, &bounds, &cap, tol, loose_gap(scale))
        .ok_or("a closed loop on its chain is open and can't close (joint.list shows its conflict)")?;
    let mut out = Reached::default();
    let point_miss = res.iter().take(3).map(|v| v * v).sum::<f64>().sqrt();
    let turn_miss = (res.iter().skip(3).take(mt - 3).map(|v| v * v).sum::<f64>().sqrt() / scale).to_degrees();
    if point_miss > REACH_TOL || turn_miss > REACH_TOL {
        // The limits it rests on: the target pulls the value past them (−Jᵀr over the target's
        // rows, ignoring pulls at right angles to the miss: under a millionth of the value's own
        // reach times the miss).
        let n = x.len();
        if let Some(jac) = jacobian(&mut f, &x, res.len()) {
            for (k, (((id, i), v), (lo, hi))) in vars.iter().zip(&x).zip(&bounds).enumerate() {
                let column = |row: usize| jac.get(row * n + k).copied().unwrap_or(0.0);
                let pull: f64 = -(0..mt).map(|row| column(row) * res.get(row).copied().unwrap_or(0.0)).sum::<f64>();
                let eps = 1e-6 * (0..mt).map(|row| column(row).powi(2)).sum::<f64>().sqrt() * miss(&res);
                let at = |b: f64| b.is_finite() && (v - b).abs() <= 1e-9 * b.abs().max(1.0);
                if at(*hi) && pull > eps {
                    out.limits.push((*id, *i, true));
                } else if at(*lo) && pull < -eps {
                    out.limits.push((*id, *i, false));
                }
            }
        }
        // A ball joint's value held at its limit (its turn isn't bounded; its values are).
        for b in &balls {
            for (i, (v, (lo, hi))) in b.values(&x).unwrap_or(b.start).iter().zip(b.bounds).enumerate() {
                let at = |l: f64| l.is_finite() && (v - l).abs() <= 1e-9 * l.abs().max(1.0);
                if at(hi) {
                    out.limits.push((b.id, i, true));
                } else if at(lo) {
                    out.limits.push((b.id, i, false));
                }
            }
        }
        // A value a motion link sets, held at its own limit (the link clamps it there): the
        // target's pull on it as if it were free.
        if f(&x).is_some() {
            for (id, i) in linked.iter().filter(|(id, _)| around.contains(id)) {
                let (Some(v), Some((lo, hi))) = (work.get(id).and_then(|q| q.get(*i)).copied(), limit_bounds(tree, &[(*id, *i)]).first().copied())
                else {
                    continue;
                };
                let at = |b: f64| b.is_finite() && (v - b).abs() <= 1e-9 * b.abs().max(1.0);
                if !at(lo) && !at(hi) {
                    continue;
                }
                let nudged = |dv: f64| {
                    let mut w = work.clone();
                    if let Some(slot) = w.get_mut(id).and_then(|q| q.get_mut(*i)) {
                        *slot = v + dv;
                    }
                    target(tree, r, scale, &w)
                };
                let (Some(up), Some(down)) = (nudged(1e-6), nudged(-1e-6)) else { continue };
                let pull: f64 = -up.iter().zip(&down).zip(&res).map(|((a, b), r)| (a - b) / 2e-6 * r).sum::<f64>();
                let eps = 1e-6 * up.iter().zip(&down).map(|(a, b)| ((a - b) / 2e-6).powi(2)).sum::<f64>().sqrt() * miss(&res);
                if at(hi) && pull > eps {
                    out.limits.push((*id, *i, true));
                } else if at(lo) && pull < -eps {
                    out.limits.push((*id, *i, false));
                }
            }
        }
        // A loop stopped it when the chain alone gets closer.
        // ponytail: names the first loop of several; trying each one open would name the one that binds.
        if let Some(first) = closers.first() {
            let k = drive.len();
            let mut alone = vals.clone();
            let mut alone_workspace = super::PlacementWorkspace::new(tree);
            let mut g = |x: &[f64]| misses(&mut alone_workspace, asm, r, scale, &drive, &[], x, &mut alone, &spent);
            let (mut xo, bo, co) =
                (start.get(..k).unwrap_or_default().to_vec(), raw_bounds.get(..k).unwrap_or_default(), cap.get(..k).unwrap_or_default());
            // The loops left open, nothing to pace.
            if let Some(ro) = approach(&mut g, &|_, _| 0.0, mt, &mut xo, bo, co, tol, loose_gap(scale))
                && miss(&ro) < miss(&res) - REACH_TOL.max(1e-9 * miss(&res))
            {
                out.stopped_by_loop = Some(*first);
            }
        }
    }
    // The stored values that changed, when that got it closer (by more than the solver's noise:
    // a held part doesn't creep).
    let gained = before - miss(&res) > 1e-7;
    for ((id, i), v) in vars.iter().zip(&values(&x)).filter(|_| gained) {
        let Some(j) = tree.joints.get(id) else { continue };
        if j.values.get(*i) == Some(v) {
            continue;
        }
        let q = out.values.entry(*id).or_insert_with(|| {
            let mut q = j.values.clone();
            q.resize(j.kind.dofs().len(), 0.0);
            q
        });
        if let Some(slot) = q.get_mut(*i) {
            *slot = *v;
        }
    }
    Ok(out)
}

/// A unit quaternion (w, x, y, z): a turn that composes without the lock of Euler angles.
#[derive(Clone, Copy, Debug)]
struct Quat([f64; 4]);

impl Quat {
    fn mul(self, o: Quat) -> Quat {
        let ([a1, b1, c1, d1], [a2, b2, c2, d2]) = (self.0, o.0);
        Quat([
            a1 * a2 - b1 * b2 - c1 * c2 - d1 * d2,
            a1 * b2 + b1 * a2 + c1 * d2 - d1 * c2,
            a1 * c2 - b1 * d2 + c1 * a2 + d1 * b2,
            a1 * d2 + b1 * c2 - c1 * b2 + d1 * a2,
        ])
    }

    /// The opposite turn.
    fn conj(self) -> Quat {
        let [w, x, y, z] = self.0;
        Quat([w, -x, -y, -z])
    }

    /// The turn by the rotation vector `w`: |w| radians about its direction.
    fn exp([x, y, z]: [f64; 3]) -> Quat {
        let t = (x * x + y * y + z * z).sqrt();
        let (s, c) = (t / 2.0).sin_cos();
        let k = if t < 1e-12 { 0.5 } else { s / t };
        Quat([c, x * k, y * k, z * k])
    }

    /// A ball joint's turn for its (yaw, pitch, roll): about z, then y, then z (`motion`).
    fn zyz([a, b, c]: [f64; 3]) -> Quat {
        let (sa, ca) = (a / 2.0).sin_cos();
        let (sb, cb) = (b / 2.0).sin_cos();
        let (sc, cc) = (c / 2.0).sin_cos();
        Quat([ca, 0.0, 0.0, sa]).mul(Quat([cb, 0.0, sb, 0.0])).mul(Quat([cc, 0.0, 0.0, sc]))
    }

    /// Its (yaw, pitch, roll): of the two sets (at pitch 0 or 180°, the many), the one nearest
    /// `near`, each angle a whole number of turns from where it would wrap.
    fn to_zyz(self, near: [f64; 3]) -> [f64; 3] {
        let [a0, b0, c0] = near;
        let n = self.0.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-300);
        let [w, x, y, z] = self.0.map(|v| v / n);
        let r22 = 1.0 - 2.0 * (x * x + y * y);
        let (r02, r12) = (2.0 * (x * z + w * y), 2.0 * (y * z - w * x));
        let (r20, r21) = (2.0 * (x * z - w * y), 2.0 * (y * z + w * x));
        let (r00, r10) = (1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + w * z));
        // Not acos(r22): that rounds a turn under about 1e-8 to none, and a finite difference
        // there sees nothing tilt.
        let sin_b = r02.hypot(r12);
        let b = sin_b.atan2(r22);
        if sin_b > 1e-12 {
            let (a, c) = (r12.atan2(r02), r21.atan2(-r20));
            let one = [wrap(a, a0), wrap(b, b0), wrap(c, c0)];
            let two = [wrap(a + PI, a0), wrap(-b, b0), wrap(c + PI, c0)];
            let off = |v: &[f64; 3]| (v[0] - a0).abs() + (v[1] - b0).abs() + (v[2] - c0).abs();
            if off(&two) < off(&one) { two } else { one }
        } else if r22 > 0.0 {
            // Pitch 0: only yaw + roll is known; yaw stays.
            [a0, wrap(0.0, b0), wrap(r10.atan2(r00) - a0, c0)]
        } else {
            // Pitch 180°: only roll − yaw is known; yaw stays.
            [a0, wrap(PI, b0), wrap(a0 + r10.atan2(-r00), c0)]
        }
    }
}

/// How far (radians) a ball joint turns from the (yaw, pitch, roll) `a` to `b`: near pitch 0 a
/// small turn changes its yaw and roll a lot.
pub(super) fn ball_turn_between(a: [f64; 3], b: [f64; 3]) -> f64 {
    let [w, x, y, z] = Quat::zyz(a).conj().mul(Quat::zyz(b)).0;
    2.0 * (x * x + y * y + z * z).sqrt().atan2(w.abs())
}

/// `t` moved by whole turns to lie nearest `near`.
fn wrap(t: f64, near: f64) -> f64 {
    t + TAU * ((near - t) / TAU).round()
}

/// A ball joint whose three values all move in a reach or a loop solve. It is solved over a small
/// turn from where it starts (a rotation vector in its own frame, radians) instead of its Euler
/// angles, which lock at pitch 0: there yaw and roll turn about the same axis, and nothing tilts it
/// the other way (#12873, #14852).
pub(super) struct BallTurn {
    id: u64,
    /// Where its yaw is in the solve's values; pitch and roll follow.
    pub(super) at: usize,
    from: Quat,
    start: [f64; 3],
    bounds: [(f64, f64); 3],
}

impl BallTurn {
    /// Its (yaw, pitch, roll) for the turn in `x`, each held within its limits (pitch is the
    /// angle between the joint's z axes, so its limits hold it in a cone, its yaw still free).
    pub(super) fn values(&self, x: &[f64]) -> Option<[f64; 3]> {
        let w = x.get(self.at..self.at + 3)?;
        let v = self.from.mul(Quat::exp([*w.first()?, *w.get(1)?, *w.get(2)?])).to_zyz(self.start);
        let mut out = [0.0; 3];
        for ((o, v), (lo, hi)) in out.iter_mut().zip(v).zip(self.bounds) {
            *o = v.max(lo).min(hi);
        }
        Some(out)
    }
}

/// The ball joints among `vars` with all three values there, in order.
pub(super) fn ball_turns(tree: &Tree, vars: &[(u64, usize)], start: &[f64], bounds: &[(f64, f64)]) -> Vec<BallTurn> {
    vars.windows(3)
        .enumerate()
        .filter_map(|(at, w)| {
            let [(id, 0), (i1, 1), (i2, 2)] = w else { return None };
            (id == i1 && id == i2 && tree.joints.get(id).is_some_and(|j| j.kind == JointKind::Ball)).then_some(())?;
            let s = [*start.get(at)?, *start.get(at + 1)?, *start.get(at + 2)?];
            Some(BallTurn { id: *id, at, from: Quat::zyz(s), start: s, bounds: [*bounds.get(at)?, *bounds.get(at + 1)?, *bounds.get(at + 2)?] })
        })
        .collect()
}

/// The target's misses with the values `vars` at `x`: the point's (mm), then the axis's (as mm
/// across the chain's `scale`), then the gaps of the loops `closers`, weighted.
fn misses(
    workspace: &mut super::PlacementWorkspace,
    asm: &Assembly,
    r: &Reach,
    scale: f64,
    vars: &[(u64, usize)],
    closers: &[u64],
    x: &[f64],
    work: &mut Values,
    spent: &Cell<usize>,
) -> Option<Vec<f64>> {
    spent.set(spent.get() + 1);
    if spent.get() > MAX_LOOP_WORK {
        return None;
    }
    for ((id, i), v) in vars.iter().zip(x) {
        if let Some(slot) = work.get_mut(id).and_then(|q| q.get_mut(*i)) {
            *slot = *v;
        }
    }
    apply_links(asm, work);
    let tree = workspace.tree;
    // An open chain has only one target placement to read. Loops share their ancestors with
    // that target, but a single placement needs no residual-local map.
    let mut out = if closers.is_empty() {
        target(tree, r, scale, work)?
    } else {
        let mut placed = workspace.at(work);
        let mut out = target_at(&placed.world(r.occurrence)?, r, scale);
        placed.closure_gaps(closers, scale, &mut out)?;
        out
    };
    let n = if r.axis.is_some() { 6 } else { 3 };
    for v in out.iter_mut().skip(n) {
        *v *= LOOP_WEIGHT;
    }
    out.iter().all(|v| v.is_finite()).then_some(out)
}

/// The target's misses for these values: the point's (mm), then the axis's (as mm across `scale`).
fn target(tree: &Tree, r: &Reach, scale: f64, vals: &Values) -> Option<Vec<f64>> {
    Some(target_at(&tree.world(r.occurrence, vals)?, r, scale))
}

fn target_at(t: &crate::Mat, r: &Reach, scale: f64) -> Vec<f64> {
    let d = apply_point(t, r.point) - r.to;
    let mut out = vec![d.x, d.y, d.z];
    if let Some((a, z)) = r.axis {
        let w = (apply_vector(t, a) - z) * scale;
        out.extend([w.x, w.y, w.z]);
    }
    out
}

/// How many steps apart the values `vars` at `a` and at `b` put the `paced` values (each with its
/// step), motion links followed.
fn stride(asm: &Assembly, vals: &Values, vars: &[(u64, usize)], paced: &[((u64, usize), f64)], a: &[f64], b: &[f64]) -> f64 {
    let at = |x: &[f64]| {
        let mut w = vals.clone();
        for ((id, i), v) in vars.iter().zip(x) {
            if let Some(slot) = w.get_mut(id).and_then(|q| q.get_mut(*i)) {
                *slot = *v;
            }
        }
        apply_links(asm, &mut w);
        w
    };
    let (wa, wb) = (at(a), at(b));
    let get = |w: &Values, id: &u64, i: usize| w.get(id).and_then(|q| q.get(i)).copied().unwrap_or(0.0);
    paced.iter().map(|((id, i), step)| (get(&wb, id, *i) - get(&wa, id, *i)).abs() / step).fold(0.0, f64::max)
}

/// Levenberg-Marquardt that stays on the loops: move `x` (within `bounds`, at most `cap` per value
/// and a step of every value `stride` measures, links included, a step) to bring the first `mt` values of `f` (the target's misses) toward zero, the rest (the
/// loops' weighted gaps) at zero. Each step is solved over all the rows, values on a limit the
/// step pushes past held, then put back on the loops (`close_within` on their gaps) and taken
/// when it gets closer. It ends at the target, or as close as the limits and loops let it get.
/// Returns `f` there, or `None` when the loops don't close at the start (within `loose`).
fn approach(
    f: &mut dyn FnMut(&[f64]) -> Option<Vec<f64>>,
    stride: &dyn Fn(&[f64], &[f64]) -> f64,
    mt: usize,
    x: &mut Vec<f64>,
    bounds: &[(f64, f64)],
    cap: &[f64],
    tol: f64,
    loose: f64,
) -> Option<Vec<f64>> {
    let n = x.len();
    let miss = |r: &[f64]| r.iter().take(mt).map(|v| v * v).sum::<f64>();
    // The loops close first, to `tol` or as near as they come within `loose` (`close_within`),
    // and stay that closed, or within `loose` where their gap grows as they move (#6211).
    let loops = f(x)?.len() > mt;
    let mut loop_tol = tol * LOOP_WEIGHT;
    if loops {
        loop_tol = close_within(&mut |y| f(y).map(|r| r.get(mt..).unwrap_or_default().to_vec()), x, bounds, loop_tol, loose * LOOP_WEIGHT)?;
    }
    let close = |f: &mut dyn FnMut(&[f64]) -> Option<Vec<f64>>, x: &mut Vec<f64>| {
        close_within(&mut |y| f(y).map(|r| r.get(mt..).unwrap_or_default().to_vec()), x, bounds, loop_tol, loose * LOOP_WEIGHT).is_some()
    };
    let mut r = f(x)?;
    let (mut lambda, mut kicked) = (1e-3, false);
    for _ in 0..MAX_REACH_STEPS {
        if largest(r.get(..mt).unwrap_or_default()) <= tol {
            break;
        }
        let m = r.len();
        let Some(jac) = jacobian(f, x, m) else { break };
        // The normal equations JᵀJ δ = −Jᵀr, and the target rows' share of the diagonal (the
        // damping scales with it, so the loops' weight doesn't slow the step along them).
        let (mut jtj, mut g, mut diag) = (linalg::Mat::zeros(n, n), vec![0.0; n], vec![0.0; n]);
        for (row, ri) in r.iter().enumerate() {
            let a = jac.get(row * n..(row + 1) * n).unwrap_or_default();
            for (p, ap) in a.iter().enumerate().filter(|(_, v)| **v != 0.0) {
                if let Some(gp) = g.get_mut(p) {
                    *gp -= ap * ri;
                }
                if row < mt
                    && let Some(dp) = diag.get_mut(p)
                {
                    *dp += ap * ap;
                }
                for (q, aq) in a.iter().enumerate().skip(p) {
                    jtj.add(p, q, ap * aq);
                }
            }
        }
        for p in 0..n {
            for q in 0..p {
                jtj.set(p, q, jtj.get(q, p));
            }
        }
        let mut stepped = false;
        while lambda < 1e12 && !stepped {
            let mut held = vec![false; n];
            // Solved, then again with the values held that it pushes past their limits.
            for _ in 0..=n {
                let mut a = jtj.clone();
                let mut b = g.clone();
                for p in 0..n {
                    a.add(p, p, lambda * (1.0 + diag.get(p).copied().unwrap_or(0.0)));
                    if held.get(p).copied().unwrap_or(false) {
                        for q in 0..n {
                            a.set(p, q, if p == q { 1.0 } else { 0.0 });
                            a.set(q, p, if p == q { 1.0 } else { 0.0 });
                        }
                        if let Some(bp) = b.get_mut(p) {
                            *bp = 0.0;
                        }
                    }
                }
                let Some(d) = linalg::solve(a, b) else { break };
                let past: Vec<bool> =
                    x.iter().zip(&d).zip(bounds).map(|((v, dv), (lo, hi))| (*dv > 0.0 && *v >= *hi) || (*dv < 0.0 && *v <= *lo)).collect();
                if past.iter().zip(&held).any(|(p, h)| *p && !*h) {
                    held = held.iter().zip(&past).map(|(h, p)| *h || *p).collect();
                    continue;
                }
                let full: Vec<f64> = x.iter().zip(&d).map(|(v, dv)| v + dv).collect();
                let over = d.iter().zip(cap).map(|(d, c)| d.abs() / c).fold(1.0f64, f64::max).max(stride(x, &full));
                let mut next: Vec<f64> = x.iter().zip(&d).zip(bounds).map(|((v, dv), (lo, hi))| (v + dv / over).max(*lo).min(*hi)).collect();
                // Checked again once taken: a link held at a limit or the loops closed again may
                // move it further than foreseen (joint.drive halves such steps too).
                if (!loops || close(f, &mut next))
                    && stride(x, &next) <= 2.0
                    && let Some(rn) = f(&next)
                    && miss(&rn) < miss(&r)
                {
                    (*x, r) = (next, rn);
                    // A step cut down to the cap (as at a stretched, singular pose) damps the next one
                    // more instead of less, so it doesn't crawl along its longest value.
                    lambda = if over > 1.0 { (lambda * over).min(1e6) } else { (lambda / 3.0).max(1e-9) };
                    stepped = true;
                }
                break;
            }
            if !stepped {
                lambda *= 4.0;
            }
        }
        if !stepped {
            // Where nothing changes the misses to first order but they aren't least (the target
            // straight across a pivot from the point), a small kick off it gets closer, once.
            if kicked {
                break;
            }
            kicked = true;
            // Both ways: one may run into a limit.
            for sign in [1.0, -1.0] {
                let mut next: Vec<f64> = x
                    .iter()
                    .zip(cap.iter().zip(bounds))
                    .enumerate()
                    .map(|(k, (v, (c, (lo, hi))))| (v + sign * 1e-3 * c * if k % 2 == 0 { 1.0 } else { -1.0 }).max(*lo).min(*hi))
                    .collect();
                if (!loops || close(f, &mut next))
                    && stride(x, &next) <= 2.0
                    && let Some(rn) = f(&next)
                    && miss(&rn) < miss(&r)
                {
                    (*x, r, lambda, stepped) = (next, rn, 1e-3, true);
                    break;
                }
            }
            if !stepped {
                break;
            }
        }
    }
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{PLACEMENT_INSERTS, PLACEMENT_STEPS, chain, placement_tree};
    use super::*;

    /// A ball joint's quaternion is the turn `motion` places it by, and its angles come back
    /// from it: the nearer of the two sets, and at pitch 0 the yaw it had.
    #[test]
    fn a_ball_turn_matches_the_joint_and_comes_back() {
        let rot = |q: Quat, v: Vec3| {
            let [w, x, y, z] = q.0;
            let u = Vec3::new(x, y, z);
            v + u.cross(v) * (2.0 * w) + u.cross(u.cross(v)) * 2.0
        };
        for zyz in [[0.3, 0.4, -1.2], [-2.0, -0.5, 0.7], [1.0, 3.0, 0.2], [0.5, 0.0, 0.25]] {
            let (q, m) = (Quat::zyz(zyz), super::super::motion(JointKind::Ball, &zyz));
            for v in [Vec3::X, Vec3::Y, Vec3::Z] {
                assert!(rot(q, v).dist(apply_vector(&m, v)) < 1e-12, "{zyz:?} {v:?}");
            }
            let back = q.to_zyz(zyz);
            assert!(back.iter().zip(zyz).all(|(a, b)| (a - b).abs() < 1e-9), "{zyz:?} -> {back:?}");
        }
        // Pitch negative, asked near it: the same turn as pitch positive, yaw and roll a half
        // turn round, but given as it was.
        let zyz = [0.2, -0.4, 0.1];
        let back = Quat::zyz(zyz).to_zyz(zyz);
        assert!(back.iter().zip(zyz).all(|(a, b)| (a - b).abs() < 1e-9), "{back:?}");
        // A small turn off pitch 0 tilts it whichever way it points.
        let tilted = Quat::zyz([0.0; 3]).mul(Quat::exp([0.05, 0.0, 0.0])).to_zyz([0.0; 3]);
        assert!((tilted[1].abs() - 0.05).abs() < 1e-9, "{tilted:?}");
        // That tilt is a turn of 0.05, though its yaw and roll moved a quarter turn each; and a
        // turn is as far either way.
        assert!((ball_turn_between([0.0; 3], tilted) - 0.05).abs() < 1e-9, "{tilted:?}");
        assert!((ball_turn_between([0.3, 0.4, -1.2], [-2.0, -0.5, 0.7]) - ball_turn_between([-2.0, -0.5, 0.7], [0.3, 0.4, -1.2])).abs() < 1e-12);
    }

    /// Exercise the IK callback itself: removing sharing at this call site must fail even if
    /// the reusable placement helper still passes its own tests.
    #[test]
    fn ik_residual_reuses_target_and_loop_ancestors_at_the_call_site() {
        let mut asm = chain(6, 0.2);
        asm.joints[5].a.occurrence = 3;
        asm.joints[5].b.occurrence = 4;
        let tree = placement_tree(&asm);
        let base = joint_values(&asm);
        let mut workspace = super::super::PlacementWorkspace::new(&tree);
        let mut cold = true;
        for axis in [None, Some((Vec3::Y, Vec3::Z))] {
            let r = Reach { occurrence: 3, point: Vec3::new(1.0, 2.0, 3.0), to: Vec3::new(7.0, 11.0, 13.0), axis, joints: None };
            for x in [0.2, 0.2 + 1e-6, 0.2 - 1e-6, 1.3] {
                let mut reference = base.clone();
                reference.get_mut(&1).unwrap()[0] = x;
                apply_links(&asm, &mut reference);
                let mut expected = target(&tree, &r, 37.0, &reference).unwrap();
                let n = expected.len();
                super::super::closure_gaps(&mut super::super::PlacementWorkspace::new(&tree), &[6], &reference, 37.0, &mut expected).unwrap();
                for v in expected.iter_mut().skip(n) {
                    *v *= LOOP_WEIGHT;
                }
                let mut work = base.clone();
                let spent = Cell::new(0);
                PLACEMENT_STEPS.with(|steps| steps.set(0));
                PLACEMENT_INSERTS.with(|inserts| inserts.set(0));
                let actual = misses(&mut workspace, &asm, &r, 37.0, &[(1, 0)], &[6], &[x], &mut work, &spent);
                assert_eq!(actual, Some(expected));
                assert_eq!(
                    PLACEMENT_INSERTS.with(|inserts| inserts.get()),
                    if cold { 4 } else { 0 },
                    "IK must retain cache storage across evaluations"
                );
                cold = false;
                assert_eq!(spent.get(), 1, "reuse must not skip a finite-difference evaluation or bypass its work cap");
                assert_eq!(PLACEMENT_STEPS.with(|steps| steps.get()), 5, "four tree placements shared by the target and loop, plus the closing step");
            }
            let mut work = base.clone();
            let spent = Cell::new(MAX_LOOP_WORK);
            PLACEMENT_STEPS.with(|steps| steps.set(0));
            assert!(misses(&mut workspace, &asm, &r, 37.0, &[(1, 0)], &[6], &[0.2], &mut work, &spent).is_none());
            assert_eq!(PLACEMENT_STEPS.with(|steps| steps.get()), 0);
        }
    }
}

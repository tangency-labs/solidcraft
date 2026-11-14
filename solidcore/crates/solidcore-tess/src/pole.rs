//! Display normals at a pole of higher order: a boundary node where the kernel's evaluator has
//! no normal ([`EvalError::Degenerate`]: `S_u × S_v` and its first-order limit vanish there).
//!
//! The planner's layout is kept as it is. The plan names each such node ([`PoleNode`]) with the
//! normal it gets:
//!
//! - [`PoleRule::Limit`]: the node is emitted once per incident triangle, and each copy gets the
//!   kernel's exact limit normal ([`EvalSurface::normal_along`]) at the triangle's own point of
//!   the pole, along the triangle's direction into it ([`foot`]). The mesh is then watertight
//!   by position there, not by index.
//! - [`PoleRule::Averaged`], where the kernel refuses that limit for some triangle (a pole of
//!   order four or more, or any other refusal): the node stays one vertex, and its normal is the
//!   area-weighted mean of the normals of the face's triangles at the pole point. Each such
//!   node is named in [`crate::PlanStats::averaged_poles`] with the kernel's reason.
//!
//! Either is a display normal and decides nothing a model depends on.

use solidcore::eval::{EvalError, EvalSurface};
use solidcore_record::Vec3;

use crate::FacePlan;
use crate::vec::VecExt as _;

/// A boundary node of a face where its surface has no normal, with the normal it gets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoleNode {
    /// The node's index in [`FacePlan::uv`].
    pub node: u32,
    /// What its triangles get there.
    pub rule: PoleRule,
}

/// The normal a [`PoleNode`] gets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PoleRule {
    /// One copy of the node per incident triangle, with the surface's limit normal along that
    /// triangle's direction into the pole.
    Limit {
        /// The parameter the pole is collapsed along (0: u, 1: v): along it every value is the
        /// pole point.
        along: usize,
        /// How near a pole a point is taken as one: the face's, as the planner classified it.
        reach: f64,
    },
    /// One vertex, with the area-weighted mean of the normals of the face's triangles at the
    /// pole point: the kernel refused the limit ([`AveragedPole`]).
    Averaged,
}

/// A pole whose normal is the mean of its triangles' normals, because the kernel refused its
/// limit normal (see [`crate::PlanStats::averaged_poles`]).
#[derive(Clone, Debug, PartialEq)]
pub struct AveragedPole {
    /// The face's index in the record.
    pub face: usize,
    /// The parameters of the face's nodes at the pole point (the planner puts a pole at each
    /// end of its collapsed side).
    pub uv: Vec<[f64; 2]>,
    /// The pole point.
    pub point: Vec3,
    /// Why the kernel has no limit normal there.
    pub reason: String,
}

/// A triangle's own point of the pole at `q` and its direction into the triangle: along the
/// pole's free parameter `d` every value is the same point, and the plan puts the pole node at
/// one of them (an end of the collapsed side). The triangle meets the pole where its other
/// corners `others` lie along `d`: the foot is `q` with its parameter `d` their mean, and the
/// direction runs from the foot to their mean (across the collapsed side, into the face).
/// Where the surface closes on itself along `d` (`period`), the plan holds every parameter in
/// its active interval, so corners either side of the interval's end lie a period apart: each
/// is first moved by whole periods next to the first, so the mean is between them, not on the
/// far side of the pole. `None` when no corner is off the pole.
fn foot(
    q: [f64; 2],
    d: usize,
    period: Option<f64>,
    others: &[[f64; 2]],
) -> Option<([f64; 2], [f64; 2])> {
    let first = others.first()?.get(d).copied()?;
    let n = others.len() as f64;
    let along = |p: &[f64; 2], k: usize| {
        let x = p.get(k).copied().unwrap_or_default();
        if k == d {
            solidcore::eval::uv::unwrap(x, first, period)
        } else {
            x
        }
    };
    let mean = [0, 1].map(|k| others.iter().map(|p| along(p, k)).sum::<f64>() / n);
    let mut at = q;
    if let (Some(x), Some(m)) = (at.get_mut(d), mean.get(d)) {
        *x = *m;
    }
    Some((at, [mean[0] - at[0], mean[1] - at[1]]))
}

/// The limit normal of `s` (unflipped) at pole node `k` of face `f` for its triangle `t`,
/// collapsed along `along`; `poles` says which nodes are on the pole; `reach` is the face's, as
/// the planner gives it to the pole classifier. The kernel's refusal as its message.
pub(crate) fn limit_normal(
    s: &EvalSurface,
    f: &FacePlan,
    k: u32,
    t: &[u32; 3],
    along: usize,
    poles: &[u32],
    reach: f64,
) -> Result<Vec3, String> {
    let q = *f
        .uv
        .get(k as usize)
        .ok_or_else(|| format!("pole node {k} is not a node of the face"))?;
    let others: Vec<[f64; 2]> = t
        .iter()
        .filter(|j| !poles.contains(j))
        .filter_map(|&j| f.uv.get(j as usize).copied())
        .collect();
    let period = solidcore::eval::uv::domain(s)
        .period
        .get(along)
        .copied()
        .flatten();
    let (at, dir) = foot(q, along, period, &others)
        .ok_or_else(|| "a triangle with no corner off the pole".to_string())?;
    s.normal_along(at[0], at[1], dir, reach)
        .map(solidcore_convert::record_vec3)
        .map_err(crate::eval_message)
}

/// The pole nodes of face `fi`'s plan `f` on `s`, with `reach` the face's: its boundary nodes
/// where the evaluator has no normal, each with the limit rule where the kernel gives every
/// incident triangle its limit normal, else the averaged one, named with the kernel's reason.
/// A boundary node the evaluator refuses otherwise is left to [`crate::TessPlan::evaluate`],
/// which refuses the face there.
/// The first direction (u, then v) `s` is collapsed along at `q` to within `reach`, as the
/// kernel's classifier says; its refusal as its message.
fn pole_dir(s: &EvalSurface, q: [f64; 2], reach: f64) -> Result<Option<usize>, String> {
    for d in 0..2 {
        if solidcore::eval::uv::degenerate_within(s, q, d, reach).map_err(crate::eval_message)? {
            return Ok(Some(d));
        }
    }
    Ok(None)
}

pub(crate) fn plan_poles(
    fi: usize,
    s: &EvalSurface,
    f: &FacePlan,
    reach: f64,
) -> (Vec<PoleNode>, Vec<AveragedPole>) {
    let mut found: Vec<u32> = Vec::new();
    for (k, q) in f.uv.iter().enumerate().take(f.fixed.len()) {
        if let (Err(EvalError::Degenerate(_)), Ok(k)) = (s.eval(q[0], q[1]), u32::try_from(k)) {
            found.push(k);
        }
    }
    let mut poles = Vec::new();
    let mut averaged = Vec::new();
    for &k in &found {
        let (q, p) = (
            f.uv.get(k as usize).copied().unwrap_or_default(),
            f.fixed.get(k as usize).copied().unwrap_or_default(),
        );
        let limit = pole_dir(s, q, reach)
            .and_then(|d| d.ok_or_else(|| format!("no direction is collapsed at {q:?}")))
            .and_then(|along| {
                for t in f.triangles.iter().filter(|t| t.contains(&k)) {
                    limit_normal(s, f, k, t, along, &found, reach)?;
                }
                Ok(along)
            });
        let rule = match limit {
            Ok(along) => PoleRule::Limit { along, reach },
            Err(reason) => {
                // One entry per pole point, with every node of the face there.
                match averaged
                    .iter_mut()
                    .find(|a: &&mut AveragedPole| a.point == p)
                {
                    Some(a) => a.uv.push(q),
                    None => averaged.push(AveragedPole {
                        face: fi,
                        uv: vec![q],
                        point: p,
                        reason,
                    }),
                }
                PoleRule::Averaged
            }
        };
        poles.push(PoleNode { node: k, rule });
    }
    (poles, averaged)
}

/// The area-weighted mean of the normals of `triangles` (oriented) with a corner at the point
/// `at`, from the vertex `positions`; `None` where they have no area.
pub(crate) fn averaged_normal(
    triangles: &[[u32; 3]],
    positions: &[Vec3],
    at: Vec3,
) -> Option<Vec3> {
    let mut sum = Vec3::new(0.0, 0.0, 0.0);
    for t in triangles {
        let p = [
            *positions.get(t[0] as usize)?,
            *positions.get(t[1] as usize)?,
            *positions.get(t[2] as usize)?,
        ];
        if p.contains(&at) {
            sum += (p[1] - p[0]).cross(p[2] - p[0]);
        }
    }
    let l = sum.len();
    (l > 0.0 && l.is_finite()).then(|| sum * (1.0 / l))
}

/// A face's vertex positions, their normals, and its triangles on them.
pub(crate) type FaceVertices = (Vec<Vec3>, Vec<Vec3>, Vec<[u32; 3]>);

/// Face `f`'s vertices at its pole nodes `at_poles` (evaluated to no normal, each named in the
/// plan) given their normals, from the face's evaluated `positions` and `normals` (oriented):
/// the vertices and the face's triangles on them. A [`PoleRule::Limit`] node is replaced by one
/// copy per incident triangle with that triangle's limit normal; a [`PoleRule::Averaged`] one
/// gets the mean of the normals of the face's triangles at its point. A normal the rule cannot
/// give refuses the face, with why.
pub(crate) fn apply(
    s: &EvalSurface,
    f: &FacePlan,
    at_poles: &[usize],
    mut positions: Vec<Vec3>,
    mut normals: Vec<Vec3>,
) -> Result<FaceVertices, String> {
    let sign = if f.flip { -1.0 } else { 1.0 };
    let names: Vec<u32> = f.poles.iter().map(|p| p.node).collect();
    // The averaged normals first: they are taken on the plan's triangles as they are.
    for &k in at_poles {
        if f.pole(k) == Some(PoleRule::Averaged) {
            let p = positions.get(k).copied().unwrap_or_default();
            let n = averaged_normal(&f.triangles, &positions, p).ok_or_else(|| {
                format!("pole vertex {k}: its triangles have no area to average a normal from")
            })?;
            if let Some(slot) = normals.get_mut(k) {
                *slot = n;
            }
        }
    }
    // The limit copies, appended after the nodes; the nodes they replace then taken out.
    let n_nodes = positions.len();
    let mut triangles = f.triangles.clone();
    // Every limit node is replaced, its copies the only vertices there (one on no triangle
    // has none).
    let gone: Vec<bool> = (0..n_nodes)
        .map(|k| at_poles.contains(&k) && matches!(f.pole(k), Some(PoleRule::Limit { .. })))
        .collect();
    for t in &mut triangles {
        let corners = *t;
        for (c, &k) in t.iter_mut().zip(&corners) {
            let Some(PoleRule::Limit { along, reach }) = f.pole(k as usize) else {
                continue;
            };
            if !at_poles.contains(&(k as usize)) {
                continue;
            }
            let n = limit_normal(s, f, k, &corners, along, &names, reach)
                .map_err(|why| format!("pole vertex {k}: {why}"))?;
            let copy = u32::try_from(positions.len())
                .map_err(|_| format!("its vertices lie past the mesh's {} index", u32::MAX))?;
            positions.push(positions.get(k as usize).copied().unwrap_or_default());
            normals.push(n * sign);
            *c = copy;
        }
    }
    // Renumber without the replaced nodes.
    let mut index: Vec<u32> = Vec::new();
    let (mut kept_p, mut kept_n) = (Vec::new(), Vec::new());
    for (k, (p, n)) in positions.into_iter().zip(normals).enumerate() {
        let i = u32::try_from(kept_p.len())
            .map_err(|_| format!("its vertices lie past the mesh's {} index", u32::MAX))?;
        index.push(i);
        if !gone.get(k).copied().unwrap_or(false) {
            kept_p.push(p);
            kept_n.push(n);
        }
    }
    for t in &mut triangles {
        for c in t.iter_mut() {
            *c = index.get(*c as usize).copied().unwrap_or(u32::MAX);
        }
    }
    Ok((kept_p, kept_n, triangles))
}

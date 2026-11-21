//! The outline of a body seen along a direction, on its edges: the stretches of each edge between
//! a face that faces the viewer and one that does not, and of an open sheet's border where its
//! face faces the viewer. A sketch's projection of a whole body draws these (with the silhouettes
//! of its curved faces) as the body's outline.
//!
//! A face faces the viewer where its normal out of the material leans toward the viewer, by more
//! than [`FACING`]; edge-on is away. Where a face is edge-on at its edge (two faces meeting
//! tangentially where the view grazes them both: the end of a round seen along the round) it faces
//! as it does a hair inside, [`STEP`] of the body's size from the edge. An edge is looked at in
//! [`SAMPLES`] even steps of its parameter, and where its state changes between two the change is
//! bisected on the edge itself; a stretch that starts or stops within [`HAIR`] of the edge's
//! interval from a vertex does so at it, where the faces meet a third and which way they face is
//! settled only to rounding.
//!
//! The samples make it an approximation: a stretch that starts and stops between two neighbouring
//! samples (a face that turns toward the viewer and away again within a 32nd of an edge) is not
//! seen. Every stretch it reports is on the outline, ends bisected to rounding.
//!
//! Not upstream: SolidCraft's whole-body projection (`crates/doc/src/project.rs`, MIT OR
//! Apache-2.0, tangency-labs/solidcraft#19386), moved into the kernel so that the application
//! computes no geometry (docs/layers.md §1). The arithmetic is the same, step for step.
//!
//! Deviations: none (no upstream counterpart).

// A comparison that is not true takes the refusing branch, so a NaN is refused too.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

pub mod pieces;

use super::query::Query;
use super::vec::VecExt;
use crate::error::{Result, SolidError};
use crate::math::Vec3;

const OP: &str = "eval::outline::stretches";

/// Even steps along an edge at which its faces are asked which way they face.
pub const SAMPLES: u32 = 32;
/// Bisections of a change of state between two samples.
pub const BISECTIONS: u32 = 60;
/// Least lean toward the viewer (the cosine of the normal's angle with the view) that faces it.
pub const FACING: f64 = 1e-9;
/// Below this lean a face is taken for edge-on at an edge, and asked how it leans a hair inside.
pub const EDGE_ON: f64 = 1e-6;
/// The step inside a face, as a fraction of the body's size.
pub const STEP: f64 = 1e-5;
/// Within this fraction of an edge's interval from its end, a stretch's end is the end.
pub const HAIR: f64 = 1e-5;

/// One stretch of an edge on the outline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stretch {
    /// Into the record's edges.
    pub edge: usize,
    /// The stretch's interval on the edge's parameter, within the edge's own.
    pub range: [f64; 2],
    /// Its points at the ends of `range`.
    pub ends: [Vec3; 2],
    /// It is the whole edge.
    pub whole: bool,
}

/// The stretches of `q`'s edges on the body's outline seen along `view`, edge by edge in the
/// record's order and along each edge in the order of its parameter (an approximation: see the
/// module docs). `view` is a unit vector pointing toward the viewer: a face faces the viewer where
/// its normal out of the material has a component along `view` above [`FACING`]. Between two
/// faces the outline is the same whichever way `view` points; an open sheet's border is outline
/// only where its face faces the viewer. `size` is the body's size (mm), `tolerance` each edge's
/// tolerance (mm, by edge index; a missing one is taken as 0). A wire (an edge on no face) and a
/// seam (one face on both its sides) have none.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a `view` that is not a finite unit vector or a `size` that is
/// not finite and positive, for an edge or face that cannot be read (its curve or surface, or a
/// point's parameters on a face) and for an edge that bounds more than two faces: the outline
/// cannot be told exactly.
pub fn stretches(q: &Query, view: Vec3, size: f64, tolerance: &[f64]) -> Result<Vec<Stretch>> {
    if !(view.is_finite() && (view.len() - 1.0).abs() <= 1e-9) {
        return Err(SolidError::invalid_input(
            OP,
            "the view is not a finite unit vector",
        ));
    }
    if !(size.is_finite() && size > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "the size is not finite and positive",
        ));
    }
    let unreadable = || SolidError::invalid_input(OP, "an edge or face cannot be read");
    let n = view;
    let step = size * STEP;
    // Which way face `f` faces at `p`, a point of one of its edges running along `along`.
    let front = |f: usize, p: Vec3, along: Vec3, accept: f64| -> Option<bool> {
        let normal = q.face_normal(f, q.face_uv(f, p, accept)?)?;
        let s = normal.dot(n);
        if s.abs() > EDGE_ON {
            return Some(s > FACING);
        }
        let Some(across) = normal.cross(along).normalized() else {
            return Some(false);
        };
        // A step to the side off the face finds its edge again (its parameters held to the
        // face's), where it is edge-on; the step onto it finds how it leans. The side that leans
        // more is the face's.
        let near = accept + 0.1 * step;
        let lean = |side: f64| {
            q.face_uv(f, p + across * (step * side), near)
                .and_then(|uv| q.face_normal(f, uv))
                .map(|v| v.dot(n))
        };
        let s = [lean(1.0), lean(-1.0)]
            .into_iter()
            .flatten()
            .fold(s, |a, b| if b.abs() > a.abs() { b } else { a });
        Some(s > FACING)
    };
    let mut out = Vec::new();
    for e in 0..q.record().edges.len() {
        let uses = q.edge_faces(e);
        let mut faces = uses.clone();
        faces.sort_unstable();
        faces.dedup();
        if faces.is_empty() || (faces.len() == 1 && uses.len() > 1) {
            continue;
        }
        let accept = 4.0 * tolerance.get(e).copied().unwrap_or(0.0).max(1e-7) + size * 1e-7;
        if faces.len() > 2 {
            return Err(SolidError::invalid_input(
                OP,
                "an edge bounds more than two faces",
            ));
        }
        let outline = |(p, along): (Vec3, Vec3)| -> Option<bool> {
            match faces.as_slice() {
                [f] => front(*f, p, along, accept),
                [f, g] => Some(front(*f, p, along, accept)? != front(*g, p, along, accept)?),
                _ => None,
            }
        };
        let [t0, t1] = q.edge_interval(e).ok_or_else(unreadable)?;
        let at = |t: f64| q.edge_at(e, t).map(|(p, _)| p);
        let state = |t: f64| q.edge_at(e, t).and_then(outline);
        let ts: Vec<f64> = (0..=SAMPLES)
            .map(|k| t0 + (t1 - t0) * f64::from(k) / f64::from(SAMPLES))
            .collect();
        let states: Vec<bool> = ts
            .iter()
            .map(|t| state(*t))
            .collect::<Option<_>>()
            .ok_or_else(unreadable)?;
        if !states.iter().any(|s| *s) {
            continue;
        }
        let cross = |lo: f64, hi: f64, lo_state: bool| -> Option<f64> {
            let (mut lo, mut hi) = (lo, hi);
            for _ in 0..BISECTIONS {
                let mid = 0.5 * (lo + hi);
                // Neighbouring numbers: every further step finds `lo` or `hi` again, whose
                // states are known, and changes nothing.
                if mid == lo || mid == hi {
                    break;
                }
                if state(mid)? == lo_state {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            Some(0.5 * (lo + hi))
        };
        let mut ranges: Vec<[f64; 2]> = Vec::new();
        let mut start = states.first().copied().unwrap_or(false).then_some(t0);
        for (k, w) in states.windows(2).enumerate() {
            let (Some(&a), Some(&b), Some(&ta), Some(&tb)) =
                (w.first(), w.get(1), ts.get(k), ts.get(k + 1))
            else {
                continue;
            };
            if a == b {
                continue;
            }
            let t = cross(ta, tb, a).ok_or_else(unreadable)?;
            match start.take() {
                Some(s) => ranges.push([s, t]),
                None => start = Some(t),
            }
        }
        if let Some(s) = start {
            ranges.push([s, t1]);
        }
        let hair = (t1 - t0) * HAIR;
        for r in &mut ranges {
            if r[0] - t0 <= hair {
                r[0] = t0;
            }
            if t1 - r[1] <= hair {
                r[1] = t1;
            }
        }
        for [a, b] in ranges {
            let whole = a == t0 && b == t1;
            let (pa, pb) = (at(a).ok_or_else(unreadable)?, at(b).ok_or_else(unreadable)?);
            // A stretch that is a point of the edge, not a piece of it.
            if !whole && pa.dist(pb) <= size * 1e-9 && (b - a) < (t1 - t0) * 1e-6 {
                continue;
            }
            out.push(Stretch {
                edge: e,
                range: [a, b],
                ends: [pa, pb],
                whole,
            });
        }
    }
    Ok(out)
}

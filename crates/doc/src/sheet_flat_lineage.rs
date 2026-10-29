//! What each edge of a skin's joined flat is in the skin (#6748, #7990).
//!
//! Until the kernel's planar booleans report history (#6748): delete this module. The flat is
//! joined from its pieces by `planar_regions_boolean`, which returns regions without saying which
//! input edge each output edge came from; the prism booleans already tag their section segments
//! (`crates/kernel/src/prism_lineage.rs`), and the planar ones would do the same. Until then this
//! recovers it after the join: an output edge came from every input edge whose line or circle it
//! lies on and overlaps. Lines and arcs match exactly; a free-form edge that the join cut comes
//! back as tangent arcs within `PLANAR_FIT_TOL`, and matches within that.

use std::f64::consts::TAU;

use solidcraft_develop::Development;
use solidcraft_geom::{Region2, Seg2};
use solidcraft_kernel::PLANAR_FIT_TOL;

/// Within this (mm), an edge lies on a line or circle.
const ON_TOL: f64 = 1e-6;

/// What a flat edge is in the skin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// Segment `seg` of loop `lp` (0 the outer one) of the panel's own region.
    Panel { panel: usize, lp: usize, seg: usize },
    /// A bend zone's edge on its `a` panel's side.
    ZoneNear(usize),
    /// A bend zone's edge on its `b` panel's side.
    ZoneFar(usize),
    /// A bend zone's end (0 at the anchors' origin, 1 at the other).
    ZoneEnd(usize, usize),
    /// Where a joint's trim leaves a panel's material: its tangent line at a bend, its cut edge
    /// at a rip.
    Cut { joint: usize, panel: usize },
}

/// How the joined flat's edges resolve to the skin, in #7990's classes: an edge on exactly one
/// source (`exact`), on several (`merged`, such as a zone's end running straight on into a
/// panel's side), or on none (`geometry_only`, which only a fault leaves); and sources that the
/// join split into several edges (`pieces`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Resolution {
    pub exact: usize,
    pub merged: usize,
    pub pieces: usize,
    pub geometry_only: usize,
}

/// The edges of the pieces a flat is joined from, each with what it is in the skin.
pub(crate) fn sources(dev: &Development) -> Vec<(Source, Seg2)> {
    let mut out = Vec::new();
    for (panel, flat) in dev.panels.iter().enumerate() {
        let Some(flat) = flat else { continue };
        for (lp, (l, from)) in std::iter::once(&flat.region.outer).chain(&flat.region.holes).zip(&flat.origins).enumerate() {
            out.extend(l.segs.iter().zip(from).map(|(s, &seg)| (Source::Panel { panel, lp, seg }, *s)));
        }
    }
    for z in &dev.zones {
        let line = |a, b| Seg2::Line { a, b };
        out.push((Source::ZoneNear(z.joint), line(z.near[0], z.near[1])));
        out.push((Source::ZoneFar(z.joint), line(z.far[0], z.far[1])));
        out.push((Source::ZoneEnd(z.joint, 0), line(z.near[0], z.far[0])));
        out.push((Source::ZoneEnd(z.joint, 1), line(z.near[1], z.far[1])));
    }
    out.extend(dev.trims.iter().map(|t| (Source::Cut { joint: t.joint, panel: t.panel }, Seg2::Line { a: t.line[0], b: t.line[1] })));
    out
}

/// Resolves every edge of the joined flat to the sources it lies on.
pub(crate) fn resolve(flat: &[Region2], sources: &[(Source, Seg2)]) -> Resolution {
    let mut hits = vec![0usize; sources.len()];
    let mut out = Resolution::default();
    for s in flat.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).flat_map(|l| &l.segs) {
        let mut on = 0;
        for ((_, c), hit) in sources.iter().zip(hits.iter_mut()) {
            if lies_on(s, c) {
                on += 1;
                *hit += 1;
            }
        }
        match on {
            0 => out.geometry_only += 1,
            1 => out.exact += 1,
            _ => out.merged += 1,
        }
    }
    out.pieces = hits.iter().filter(|&&h| h > 1).count();
    out
}

/// Whether the edge `s` lies on the source edge `c`'s line or circle and overlaps it.
fn lies_on(s: &Seg2, c: &Seg2) -> bool {
    match (*s, *c) {
        (Seg2::Line { a, b }, Seg2::Line { a: p, b: q }) => {
            let len = p.dist(q);
            let Some(n) = (q - p).normalized() else { return false };
            let off = |x: solidcraft_geom::Vec2| (x - p).cross(n).abs() <= ON_TOL;
            let mid = ((a - p).dot(n) + (b - p).dot(n)) / 2.0;
            off(a) && off(b) && mid >= -ON_TOL && mid <= len + ON_TOL
        }
        (Seg2::Arc { center, radius, start, sweep }, Seg2::Arc { center: c2, radius: r2, start: s2, sweep: w2 }) => {
            let mid = start + sweep / 2.0;
            let into = ((mid - s2) * w2.signum()).rem_euclid(TAU);
            center.dist(c2) <= ON_TOL && (radius - r2).abs() <= ON_TOL && into <= w2.abs() + ON_TOL / r2.max(ON_TOL)
        }
        (Seg2::Line { .. }, Seg2::Arc { .. }) | (Seg2::Arc { .. }, Seg2::Line { .. }) => false,
        // A free-form source, or a free-form edge the join made of arcs: within the join's fit.
        _ => [0.0, 0.5, 1.0].iter().all(|&t| {
            let p = s.point_at(t);
            c.point_at(c.closest_param(p)).dist(p) <= ON_TOL + PLANAR_FIT_TOL
        }),
    }
}

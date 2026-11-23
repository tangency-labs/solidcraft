//! A 2D mesh checker: members are polygonised regions with a rigid motion, and a pair is probed
//! over driver angles for overlap (area and penetration depth) and for the gap between the
//! flanks. Conjugate teeth overlap nowhere and stay in (near) contact throughout.
//!
//! [`check_mesh`] runs it at reduced cost over a plan's meshing pairs ([`MeshPair`], which the
//! plan lists in `Plan::meshes`) for a live verdict in the window; the tests use the finer
//! [`Member::new`] and [`run`] directly.

use std::f64::consts::TAU;

use crate::geom::{Outline, Region, Seg, V2, natural_spline};
use crate::plan::{Kind, Plan, PreviewGear, Spec};

/// A rigid motion against the driver angle t: turned `speed`·t about a centre that goes round
/// the origin `orbit`·t and slides `slide`·t along +x.
#[derive(Clone, Copy, Debug)]
pub struct Motion {
    pub center: V2,
    pub speed: f64,
    pub slide: f64,
    pub orbit: f64,
}

impl Motion {
    pub fn turning(center: V2, speed: f64) -> Motion {
        Motion { center, speed, slide: 0.0, orbit: 0.0 }
    }
    /// The preview's own motion (a sliding member is not wrapped by its period here). A sliding
    /// member with a period starts half a period back, as [`PreviewGear::place`] draws it (a
    /// face gear's rack of an even count is built with a space at the middle, and that half
    /// pitch puts a tooth under the pinion's space): without it the check met the rack half a
    /// pitch out of phase and read the default face gear as interfering by 0.5 mm.
    pub fn of(g: &PreviewGear) -> Motion {
        let back = if g.slide != 0.0 && g.period > 0.0 && g.name != "worm" { V2::new(-0.5 * g.period, 0.0) } else { V2::ZERO };
        Motion { center: g.center + back, speed: g.speed, slide: g.slide, orbit: g.orbit }
    }
    /// (angle, translation): a local point q is at q.rot(angle) + translation.
    fn pose(&self, t: f64) -> (f64, V2) {
        (self.speed * t, self.center.rot(self.orbit * t) + V2::new(self.slide * t, 0.0))
    }
}

struct Edge {
    a: V2,
    b: V2,
    hole: bool,
}

/// A region as closed polylines with the material on their left, and a grid over its edges for
/// fast inside and distance queries in its own frame.
pub struct Member {
    loops: Vec<Vec<V2>>,
    edges: Vec<Edge>,
    lo: V2,
    c: f64,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
    solid: Vec<bool>,
    rows: Vec<Vec<u32>>,
    /// Every edge lies within `rmax` of the local origin, and none within `hollow` of it when
    /// the origin is not in the material (0 otherwise): a point further than a few cells out of
    /// that band is plainly outside and far from the boundary ([`Member::far`]).
    rmax: f64,
    hollow: f64,
    pub motion: Motion,
}

/// The outline as a closed polyline within `tol` of its curves: splines as the sketch runs them
/// ([`natural_spline`]), sampled finely and thinned to chords that stray no more than `tol`
/// (`Outline::polyline` gives a spline a fixed few chords a span, whose sag would show up here
/// as flanks crossing).
pub fn polygon(o: &Outline, tol: f64) -> Vec<V2> {
    polygon_n(o, tol, 32)
}

/// As [`polygon`], with `per_span` samples of each spline span to thin from.
fn polygon_n(o: &Outline, tol: f64, per_span: usize) -> Vec<V2> {
    let mut out: Vec<V2> = Vec::new();
    for s in &o.segs {
        let mut pts = Vec::new();
        match s {
            Seg::Spline(p) => {
                let mut fine = Vec::new();
                natural_spline(p, per_span, &mut fine);
                let mut i = 0;
                pts.push(fine[0]);
                while i + 1 < fine.len() {
                    let mut j = i + 1;
                    while j + 1 < fine.len() && j + 1 - i <= 32 && (i + 1..=j).all(|k| seg_dist(fine[k], fine[i], fine[j + 1]) <= tol) {
                        j += 1;
                    }
                    pts.push(fine[j]);
                    i = j;
                }
            }
            other => other.sample(tol, &mut pts),
        }
        if !out.is_empty() && !pts.is_empty() {
            pts.remove(0);
        }
        out.extend(pts);
    }
    out
}

fn signed_area(p: &[V2]) -> f64 {
    p.windows(2).map(|w| w[0].cross(w[1])).sum::<f64>() * 0.5
}

fn seg_dist(p: V2, a: V2, b: V2) -> f64 {
    let d = b - a;
    let l2 = d.dot(d);
    let t = if l2 > 0.0 { ((p - a).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(a + d * t)
}

impl Member {
    /// `cell`: the grid's cell size; distances are exact up to it, and edges are no longer than
    /// half of it. `tol`: chord tolerance of arcs.
    pub fn new(region: &Region, motion: Motion, cell: f64, tol: f64) -> Member {
        Member::build(region, motion, cell, tol, 32)
    }

    /// As [`Member::new`], with `per_span` samples of each spline span to thin from (fewer is
    /// faster to build and good to a few 1e-4 of a module).
    pub fn build(region: &Region, motion: Motion, cell: f64, tol: f64, per_span: usize) -> Member {
        let mut loops = Vec::new();
        let mut edges = Vec::new();
        for (outlines, hole) in [(&region.outer, false), (&region.holes, true)] {
            for o in outlines {
                let mut p = polygon_n(o, tol, per_span);
                if p.len() < 3 {
                    continue;
                }
                // Closed exactly: a gap of rounding size lets a ray through.
                let last = p.len() - 1;
                if p[0].dist(p[last]) > 1e-9 {
                    p.push(p[0]);
                } else {
                    p[last] = p[0];
                }
                // Material on the left: outer loops counter-clockwise, holes clockwise.
                if (signed_area(&p) < 0.0) != hole {
                    p.reverse();
                }
                // No edge longer than half a cell.
                let mut q = vec![p[0]];
                for w in p.windows(2) {
                    let n = (w[0].dist(w[1]) / (0.5 * cell)).ceil().max(1.0) as usize;
                    for i in 1..=n {
                        q.push(w[0].lerp(w[1], i as f64 / n as f64));
                    }
                }
                for w in q.windows(2) {
                    if w[0].dist(w[1]) > 0.0 {
                        edges.push(Edge { a: w[0], b: w[1], hole });
                    }
                }
                loops.push(q);
            }
        }
        let (mut lo, mut hi) = (V2::new(f64::MAX, f64::MAX), V2::new(f64::MIN, f64::MIN));
        for q in loops.iter().flatten() {
            lo = V2::new(lo.x.min(q.x), lo.y.min(q.y));
            hi = V2::new(hi.x.max(q.x), hi.y.max(q.y));
        }
        let c = cell.max((hi.x - lo.x).max(hi.y - lo.y) / 400.0);
        // Off the round numbers, so that no ray of the inside test runs along a symmetry axis
        // through the vertices on it.
        lo = lo - V2::new(2.137 * c, 2.291 * c);
        let nx = ((hi.x - lo.x) / c).ceil() as usize + 3;
        let ny = ((hi.y - lo.y) / c).ceil() as usize + 3;
        let mut m = Member {
            loops,
            edges,
            lo,
            c,
            nx,
            ny,
            cells: vec![Vec::new(); nx * ny],
            solid: vec![false; nx * ny],
            rows: vec![Vec::new(); ny],
            rmax: 0.0,
            hollow: 0.0,
            motion,
        };
        for (k, e) in m.edges.iter().enumerate() {
            // The grid spans every edge, so both corners are in it.
            let (Some((i0, j0)), Some((i1, j1))) =
                (m.cell(V2::new(e.a.x.min(e.b.x), e.a.y.min(e.b.y))), m.cell(V2::new(e.a.x.max(e.b.x), e.a.y.max(e.b.y))))
            else {
                continue;
            };
            for j in j0..=j1 {
                m.rows[j].push(k as u32);
                for i in i0..=i1 {
                    m.cells[j * nx + i].push(k as u32);
                }
            }
        }
        // The empty cells' material, a row at a time: the crossings of the row's middle line
        // (as `raycast` finds them, the same test and the same arithmetic) counted from the
        // right, instead of a ray cast from every cell.
        let mut xs: Vec<(f64, bool)> = Vec::new();
        for j in 0..ny {
            let y = m.lo.y + (j as f64 + 0.5) * c;
            xs.clear();
            for &k in &m.rows[j] {
                let e = &m.edges[k as usize];
                if (e.a.y > y) != (e.b.y > y) {
                    xs.push((e.a.x + (y - e.a.y) / (e.b.y - e.a.y) * (e.b.x - e.a.x), e.hole));
                }
            }
            xs.sort_by(|a, b| b.0.total_cmp(&a.0));
            let (mut next, mut outer, mut hole) = (0, false, false);
            for i in (0..nx).rev() {
                let x = m.lo.x + (i as f64 + 0.5) * c;
                // Crossings strictly right of the cell's middle.
                while next < xs.len() && xs[next].0 > x {
                    if xs[next].1 {
                        hole = !hole;
                    } else {
                        outer = !outer;
                    }
                    next += 1;
                }
                if m.cells[j * nx + i].is_empty() {
                    m.solid[j * nx + i] = outer && !hole;
                }
            }
        }
        m.rmax = m.edges.iter().map(|e| e.a.len().max(e.b.len())).fold(0.0, f64::max);
        if !m.inside(V2::ZERO) {
            m.hollow = m.edges.iter().map(|e| seg_dist(V2::ZERO, e.a, e.b)).fold(f64::INFINITY, f64::min);
        }
        m
    }

    /// Is the local point so far out of the band the edges lie in that it is outside the
    /// material and [`Member::dist`] finds no edge near it? (Then `inside` is false and `dist`
    /// infinite for it, exactly: `dist` looks at edges listed in the neighbouring cells, which
    /// are under four cells away.)
    fn far(&self, p: V2) -> bool {
        let r = p.len();
        let reach = 4.0 * self.c;
        r > self.rmax + reach || r < self.hollow - reach
    }

    pub fn of(g: &PreviewGear, cell: f64, tol: f64) -> Member {
        Member::new(&g.region, Motion::of(g), cell, tol)
    }

    fn cell(&self, p: V2) -> Option<(usize, usize)> {
        let (x, y) = ((p.x - self.lo.x) / self.c, (p.y - self.lo.y) / self.c);
        if x < 0.0 || y < 0.0 || x >= self.nx as f64 || y >= self.ny as f64 {
            return None;
        }
        Some((x as usize, y as usize))
    }

    fn raycast(&self, p: V2) -> bool {
        let Some((_, j)) = self.cell(p) else { return false };
        let (mut outer, mut hole) = (false, false);
        for &k in &self.rows[j] {
            let e = &self.edges[k as usize];
            if (e.a.y > p.y) != (e.b.y > p.y) {
                let x = e.a.x + (p.y - e.a.y) / (e.b.y - e.a.y) * (e.b.x - e.a.x);
                if x > p.x {
                    if e.hole {
                        hole = !hole;
                    } else {
                        outer = !outer;
                    }
                }
            }
        }
        outer && !hole
    }

    /// Is the local point in the material?
    pub fn inside(&self, p: V2) -> bool {
        match self.cell(p) {
            None => false,
            Some((i, j)) => {
                if self.cells[j * self.nx + i].is_empty() {
                    self.solid[j * self.nx + i]
                } else {
                    self.raycast(p)
                }
            }
        }
    }

    /// The distance from the local point to the boundary when it is under a cell away (else
    /// infinity or some larger distance).
    pub fn dist(&self, p: V2) -> f64 {
        let Some((i, j)) = self.cell(p) else { return f64::INFINITY };
        let mut d = f64::INFINITY;
        for jj in j.saturating_sub(1)..=(j + 1).min(self.ny - 1) {
            for ii in i.saturating_sub(1)..=(i + 1).min(self.nx - 1) {
                for &k in &self.cells[jj * self.nx + ii] {
                    let e = &self.edges[k as usize];
                    d = d.min(seg_dist(p, e.a, e.b));
                }
            }
        }
        d
    }
}

/// What a pair does over a range of driver angles.
#[derive(Clone, Copy, Debug)]
pub struct Stat {
    /// Largest overlap area (mm²) and the driver angle it happens at.
    pub area: f64,
    pub area_at: f64,
    /// Deepest penetration of one boundary into the other's material (mm).
    pub depth: f64,
    /// Smallest and largest (over the angles) of the least distance between the boundaries
    /// where they don't overlap (mm).
    pub gap_min: f64,
    pub gap_max: f64,
}

impl Stat {
    pub fn merge(&mut self, o: &Stat) {
        if o.area > self.area {
            self.area = o.area;
            self.area_at = o.area_at;
        }
        self.depth = self.depth.max(o.depth);
        self.gap_min = self.gap_min.min(o.gap_min);
        self.gap_max = self.gap_max.max(o.gap_max);
    }
}

/// `a`'s boundary against `b`'s material at driver angle t: (area term, depth, gap). The area
/// term is the part of Green's integral over the pieces of `a`'s boundary inside `b`, about `o`.
fn pass(a: &Member, b: &Member, t: f64, o: V2) -> (f64, f64, f64) {
    let (aa, ta) = a.motion.pose(t);
    let (ab, tb) = b.motion.pose(t);
    let (s, c) = aa.sin_cos();
    let (sr, cr) = (aa - ab).sin_cos();
    let d = (ta - tb).rot(-ab);
    let world = |q: V2| V2::new(q.x * c - q.y * s, q.x * s + q.y * c) + ta;
    let local = |q: V2| V2::new(q.x * cr - q.y * sr, q.x * sr + q.y * cr) + d;
    let (mut area, mut depth, mut gap) = (0.0, 0.0f64, f64::INFINITY);
    for lp in &a.loops {
        // The loop's last point repeats its first.
        let n = lp.len() - 1;
        // Most of `a`'s boundary is nowhere near `b`: those points are outside and far, and cost
        // only their distance from `b`'s middle.
        let pts: Vec<(V2, bool)> = lp[..n]
            .iter()
            .map(|q| {
                let l = local(*q);
                if b.far(l) {
                    return (*q, false);
                }
                let inside = b.inside(l);
                let dist = b.dist(l);
                if inside {
                    depth = depth.max(dist.min(b.c));
                } else {
                    gap = gap.min(dist);
                }
                (*q, inside)
            })
            .collect();
        let Some(start) = pts.iter().position(|(_, inside)| !inside) else {
            // Wholly inside: the loop's own area.
            area += (0..n).map(|k| 0.5 * (world(pts[k].0) - o).cross(world(pts[(k + 1) % n].0) - o)).sum::<f64>();
            continue;
        };
        // Each run of the boundary inside `b`, from where it enters to where it leaves, closed by
        // the chord back: its area does not depend on `o`, so runs that the other boundary
        // answers only in part (a corner through one edge) still add up.
        let mut entry = V2::ZERO;
        for k in 0..n {
            let ((q0, in0), (q1, in1)) = (pts[(start + k) % n], pts[(start + k + 1) % n]);
            if !in0 && !in1 {
                continue;
            }
            let (w0, w1) = (world(q0) - o, world(q1) - o);
            if in0 && in1 {
                area += 0.5 * w0.cross(w1);
                continue;
            }
            // The crossing, by bisection on the edge.
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..40 {
                let mid = 0.5 * (lo + hi);
                if b.inside(local(q0.lerp(q1, mid))) == in0 { lo = mid } else { hi = mid }
            }
            let x = w0.lerp(w1, 0.5 * (lo + hi));
            if in1 {
                entry = x;
                area += 0.5 * x.cross(w1);
            } else {
                area += 0.5 * w0.cross(x) + 0.5 * x.cross(entry);
            }
        }
    }
    (area, depth, gap)
}

/// The pair at one driver angle.
pub fn probe(a: &Member, b: &Member, t: f64) -> Stat {
    let o = (a.motion.pose(t).1 + b.motion.pose(t).1) * 0.5;
    let (a1, d1, g1) = pass(a, b, t, o);
    let (a2, d2, g2) = pass(b, a, t, o);
    let gap = g1.min(g2);
    Stat { area: (a1 + a2).abs(), area_at: t, depth: d1.max(d2), gap_min: gap, gap_max: gap }
}

/// The pair over `steps` + 1 driver angles from `t0` to `t1`.
pub fn run(a: &Member, b: &Member, t0: f64, t1: f64, steps: usize) -> Stat {
    let mut s = probe(a, b, t0);
    for i in 1..=steps {
        s.merge(&probe(a, b, t0 + (t1 - t0) * i as f64 / steps as f64));
    }
    s
}

// ---------------------------------------------------------------------------------------------
// The plan's meshes, checked for the window.

/// Two members of a plan's preview that mesh (indices into `Plan::preview`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshPair {
    pub a: usize,
    pub b: usize,
    /// `b` surrounds `a` (a ring gear round a planet or pinion, a pin ring round its disc).
    pub internal: bool,
    /// The driver angle after which the mesh repeats (one tooth pitch of the driver; radians).
    pub period: f64,
    /// The flank gap the set is meant to have (its backlash; mm).
    pub slack: f64,
    /// The length tolerances scale with (the module; the pin radius of a cycloidal drive; mm).
    pub scale: f64,
}

/// Which preview members mesh, for a parallel-axis set (none for the others: their preview is
/// not the plane the teeth mesh in).
pub fn topology(s: &Spec, preview: &[PreviewGear]) -> Vec<MeshPair> {
    if let Some(t) = &s.train {
        return crate::plan::train::mesh_pairs(s, t, preview);
    }
    let n = preview.len();
    let pair = |a: usize, b: usize, internal: bool, teeth: u32| MeshPair {
        a,
        b,
        internal,
        period: TAU / f64::from(teeth.max(1)),
        slack: s.backlash,
        scale: s.module,
    };
    match s.kind {
        Kind::Pair | Kind::Cycloidal if n == 2 => vec![pair(0, 1, false, s.teeth)],
        // Sun first, ring last, the planets between.
        Kind::Planetary if n >= 3 => (1..n - 1).flat_map(|k| [pair(0, k, false, s.teeth), pair(k, n - 1, true, s.teeth)]).collect(),
        Kind::CycloidalDrive if n == 2 => vec![MeshPair {
            a: 0,
            b: 1,
            internal: true,
            period: TAU / f64::from(s.pins.max(1)),
            slack: s.backlash / 2.0,
            scale: s.pin_diameter / 2.0,
        }],
        Kind::Bevel | Kind::SpiralBevel | Kind::Hypoid | Kind::CrossedHelical | Kind::Worm | Kind::BevelPlanetary | Kind::Differential => Vec::new(),
        // A rack with a pinion on it, should a set come to have both: the first sliding member
        // against the first turning one.
        _ => {
            let rack = preview.iter().position(|g| g.slide != 0.0 && g.period > 0.0);
            let pinion = preview.iter().position(|g| g.slide == 0.0 && g.speed != 0.0 && g.pitch_radius > 0.0);
            match (pinion, rack) {
                (Some(a), Some(b)) => {
                    vec![MeshPair { a, b, internal: false, period: preview[b].period / preview[a].pitch_radius, slack: s.backlash, scale: s.module }]
                }
                _ => Vec::new(),
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// No overlap, and the flanks touch (or stay the backlash apart) all the way round.
    Ok,
    /// The members cut into each other.
    Interferes,
    /// The flanks come apart at some angle (or never meet).
    NoContact,
}

/// What [`check_mesh`] found for one [`MeshPair`].
#[derive(Clone, Debug, PartialEq)]
pub struct MeshResult {
    pub a: usize,
    pub b: usize,
    /// Deepest the members cut into each other over a tooth pitch (mm; 0 when they never do).
    pub penetration_mm: f64,
    /// Smallest and largest, over the pitch, of the least gap between the members (mm).
    pub gap_min_mm: f64,
    pub gap_max_mm: f64,
    pub verdict: Verdict,
    /// One line to show: who, the verdict and the number behind it.
    pub message: String,
}

/// Driver angles probed over one tooth pitch by [`check_mesh`].
pub const CHECK_STEPS: usize = 12;
/// Overlap deeper than this is interference (× the pair's scale): above what the coarse
/// polygons and the fitted flanks can cross by on their own.
pub const CHECK_DEPTH: f64 = 3e-3;
/// Flanks further apart than the backlash plus this have lost contact (× the pair's scale).
pub const CHECK_GAP: f64 = 6e-3;

/// Checks every meshing pair of a plan in its preview plane over one tooth pitch: overlap and
/// loss of contact. Coarser than the tests' sweep (about 1e-3 of a module) and fast enough to
/// call on every edit (debounce it all the same).
///
/// Sets whose axes are not parallel (their preview is a side-by-side picture, not the plane the
/// teeth mesh in) are checked in 3D instead, between their bodies ([`crate::mesh3d::check`]);
/// `a` and `b` are still preview members (gear 1 and gear 2; the wheel and the worm).
pub fn check_mesh(plan: &Plan) -> Vec<MeshResult> {
    let mut out = check_mesh_2d(plan);
    out.extend(plan.meshes3d.iter().map(|m| crate::mesh3d::check(plan, m)));
    out
}

fn check_mesh_2d(plan: &Plan) -> Vec<MeshResult> {
    let n = plan.preview.len();
    let pairs: Vec<&MeshPair> = plan.meshes.iter().filter(|m| m.a < n && m.b < n && m.a != m.b && m.scale > 0.0 && m.period > 0.0).collect();
    // Each member once, at the scale of the first pair it is in; all at once.
    let mut wanted: Vec<(usize, f64)> = Vec::new();
    for m in &pairs {
        for i in [m.a, m.b] {
            if !wanted.iter().any(|w| w.0 == i) {
                wanted.push((i, m.scale));
            }
        }
    }
    let built = crate::par::map(wanted.len(), |k| {
        let (i, scale) = wanted[k];
        let g = &plan.preview[i];
        // Bores and lightening holes play no part, and cost time: only a member that
        // surrounds another keeps its holes (a ring's teeth are one).
        let surrounds = pairs.iter().any(|q| q.internal && q.b == i);
        let region = if surrounds { g.region.clone() } else { Region { outer: g.region.outer.clone(), holes: Vec::new() } };
        Member::build(&region, Motion::of(g), 0.25 * scale, 1e-4 * scale, 8)
    });
    let mut members: Vec<Option<Member>> = (0..n).map(|_| None).collect();
    for ((i, _), m) in wanted.iter().zip(built) {
        members[*i] = Some(m);
    }
    // Each pair with its two members (every pair's members were built above).
    let pairs: Vec<(&MeshPair, &Member, &Member)> = pairs.iter().filter_map(|m| Some((*m, members[m.a].as_ref()?, members[m.b].as_ref()?))).collect();
    // Every pair's probes at once, merged afterwards in the order one thread would.
    let probes = crate::par::map(pairs.len() * CHECK_STEPS, |k| {
        let ((m, a, b), i) = (pairs[k / CHECK_STEPS], k % CHECK_STEPS);
        // Either side of the drawn position: a coupon's few teeth are centred on it.
        let t = if i == 0 { -0.5 * m.period } else { m.period * (i as f64 / CHECK_STEPS as f64 - 0.5) };
        probe(a, b, t)
    });
    pairs
        .iter()
        .enumerate()
        .map(|(k, &(m, _, _))| {
            let mine = &probes[k * CHECK_STEPS..(k + 1) * CHECK_STEPS];
            let mut s = mine[0];
            for p in &mine[1..] {
                s.merge(p);
            }
            let (na, nb) = (&plan.preview[m.a].name, &plan.preview[m.b].name);
            let depth = if s.depth.is_finite() { s.depth } else { 0.0 };
            let gap_min = if s.gap_min.is_finite() { s.gap_min } else { 0.0 };
            let gap_max = if s.gap_max.is_finite() { s.gap_max } else { gap_min };
            let (verdict, message) = if depth > CHECK_DEPTH * m.scale {
                (Verdict::Interferes, format!("{na} and {nb} interfere: they overlap by {depth:.3} mm"))
            } else if gap_max > m.slack + CHECK_GAP * m.scale {
                (Verdict::NoContact, format!("{na} and {nb} lose contact: up to {gap_max:.3} mm apart"))
            } else if m.slack > 0.0 {
                (Verdict::Ok, format!("{na} and {nb} mesh: no overlap, flanks {gap_min:.3} to {gap_max:.3} mm apart"))
            } else {
                (Verdict::Ok, format!("{na} and {nb} mesh: no overlap, flanks in contact"))
            };
            MeshResult { a: m.a, b: m.b, penetration_mm: depth, gap_min_mm: gap_min, gap_max_mm: gap_max, verdict, message }
        })
        .collect()
}

/// The worst verdict of a plan's meshes (interference before lost contact), or none when the
/// plan has no mesh to check.
pub fn worst(results: &[MeshResult]) -> Option<Verdict> {
    let has = |v: Verdict| results.iter().any(|r| r.verdict == v);
    if results.is_empty() {
        None
    } else if has(Verdict::Interferes) {
        Some(Verdict::Interferes)
    } else if has(Verdict::NoContact) {
        Some(Verdict::NoContact)
    } else {
        Some(Verdict::Ok)
    }
}

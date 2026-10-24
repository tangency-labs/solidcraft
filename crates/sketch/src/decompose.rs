//! Cluster decomposition: places points that are rigid relative to known geometry but that no
//! point-by-point construction reaches.
//!
//! Owen (1991, §3) splits a well-constrained constraint graph at articulation pairs into
//! triangles whose sides are original dimensions or *virtual bonds*, solves the triangles with
//! square roots and reassembles the pieces (§3.3.2). Fudos and Hoffmann (1997) run the same
//! reduction bottom-up, and that is the form implemented here (Hoffmann 2005 gives the
//! summary):
//!
//! - a **cluster** is a set of points whose relative positions are known, stored in the
//!   cluster's own frame. Every dimension between two points starts as a two-point cluster;
//!   the known geometry is the *ground* cluster, in world coordinates;
//! - three clusters that pairwise share one point know the three distances between the
//!   shared points (Owen's virtual bonds). The triangle is constructed in one cluster's frame
//!   (circle ∩ circle) and the other two are moved onto it rigidly: the three become one;
//! - a rigid cluster sharing two points with another is moved onto it;
//! - the ground absorbs everything it reaches, so placing the root cluster is the rigid
//!   transform onto the shared known points.
//!
//! Horizontal/vertical relations and DistanceX/Y are not invariant under rotation. Two of them
//! between the same pair of points (or one with a distance) fix the pair's offset vector: an
//! *oriented* cluster, known up to a translation. Two oriented clusters (the ground counts as
//! one) sharing a point merge by translation. A rigid cluster holding both ends of a single
//! horizontal/vertical relation turns until it holds and becomes oriented; one pinned to an
//! oriented cluster at a single point turns about the pin until a relation between the two
//! holds (circle ∩ axis, the hinge step).
//!
//! Branches follow Owen's rule (§2.4.3, §5): every constructed triangle keeps the orientation
//! (clockwise or counter-clockwise) its three points have in `reference`, the geometry before
//! the solve (or before a drag). A cluster is never reflected when it is moved.
//!
//! The relations come from [`crate::loci`]'s reading, as for propagation: those between two
//! points build the starting clusters, and a point that is not in a cluster but whose
//! relations and ties to the cluster's points (a point on a line through two of them, a
//! midpoint, a mirrored point, a tangent circle's centre, a direction derived from a line of
//! the cluster…) cross at one place joins it (Owen's single-point merge), placed by the same
//! [`crate::loci::place`] as propagation, in the cluster's frame. Relations fixed in the
//! sketch's frame (horizontal, vertical, line angles) count only in a ground or oriented
//! cluster.
//!
//! A rigid cluster that never reaches the ground (no known point, or a single one it can still
//! turn about) has no pose the constraints decide. Once nothing more is placed,
//! [`Decomposition::floats`] places such a cluster where its reference pose puts it, if that
//! moves it at all (its pin moved, or one of its relations misses at the reference): free, the
//! rotation and translation that fit its points to their reference positions best in the
//! least-squares sense; pinned, its reference pose about the pin moved onto the pin (a large
//! move of the pin carries it along unturned); an oriented cluster by translation only. It is
//! never reflected. The pose is a guess at free motion: the caller seeds the solver with it but
//! builds nothing on it, and the solver keeps every degree of freedom.
//!
//! What remains unplaced is left to the numerical solver: triconnected remainders, which no
//! sequence of square roots solves (Owen §3.3.1 step 4), and anything held by constraints this
//! module does not read. The output only seeds the solver; it verifies every constraint and
//! its rank analysis decides the degrees of freedom. Every intersection goes through [`crate::loci`]: a near-tangent one pins nothing
//! down well and a branch the reference does not decide is unknown, so neither is merged
//! (the solver starts those points where they are).

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use solidcraft_geom::Vec2;

use crate::loci::{Locus, Reading, Rel, Span, View, crossing, mag, place, side};
use crate::union_find::{find, union};

/// More starting clusters than this (one per dimensioned point pair) and the sketch is left
/// to the numerical solver unchanged.
const MAX_CLUSTERS: usize = 20_000;
/// Bound on the membership scans of the merge search, so a hostile sketch can't stall it
/// (over the life of one graph, whatever is added to it): about 0.3 s.
pub(crate) const MAX_WORK: usize = 20_000_000;
/// The bound for all the solves of one live drag frame together: about 16 ms. What a solve's
/// decomposition leaves unplaced once it is spent starts where it is, for LM, so that solve keeps
/// its branch only by continuation: a drag that runs out takes short steps for the rest of the
/// frame (see [`Budget::exhausted`]).
pub(crate) const FRAME_WORK: usize = 1_000_000;

/// Work shared by the held solves and feasible cursor correction of one drag frame. Cluster
/// decomposition charges membership scans; cursor correction reserves estimates of dense
/// arithmetic before starting it. These deterministic units approximate time, not a deadline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    left: usize,
    spent: usize,
    cut: bool,
    /// A live drag frame's allowance for the numerical solves (LM), which are counted but never
    /// stopped by it: once it is spent, the drag takes no more steps this frame (`None`: not
    /// counted).
    numeric_left: Option<usize>,
}

impl Budget {
    /// A budget of `work` units: membership scans or estimated dense arithmetic operations.
    pub fn new(work: usize) -> Budget {
        Budget { left: work, spent: 0, cut: false, numeric_left: None }
    }
    /// The budget of one solve: about 0.3 s, so a hostile sketch can't stall it.
    pub fn full() -> Budget {
        Budget::new(MAX_WORK)
    }
    /// The budget of one live drag frame: about 16 ms, and as much again for its numerical
    /// solves ([`Budget::numeric_exhausted`]).
    pub fn frame() -> Budget {
        Budget { numeric_left: Some(FRAME_WORK), ..Budget::new(FRAME_WORK) }
    }
    /// The frame's numerical solves have done their share of work: a drag takes no more steps
    /// this frame and goes on in the next.
    pub fn numeric_exhausted(&self) -> bool {
        self.numeric_left == Some(0)
    }
    /// This budget with `work` units for its numerical solves (a frame that runs out sooner).
    pub fn with_numeric(self, work: usize) -> Budget {
        Budget { numeric_left: Some(work), ..self }
    }
    /// Whether the numerical solves' work is counted ([`Budget::frame`]).
    pub(crate) fn counts_numeric(&self) -> bool {
        self.numeric_left.is_some()
    }
    /// Count a numerical solve's work against the frame's allowance.
    pub(crate) fn charge_numeric(&mut self, work: usize) {
        if let Some(left) = &mut self.numeric_left {
            *left = left.saturating_sub(work);
        }
    }
    /// Some decomposition or cursor work stopped because the budget ran out.
    pub fn exhausted(&self) -> bool {
        self.cut
    }
    /// The work charged so far (it may pass the budget by one scan).
    pub fn spent(&self) -> usize {
        self.spent
    }
    pub(crate) fn left(&self) -> usize {
        self.left
    }
    /// Reserve estimated dense arithmetic before starting it. Unlike a decomposition scan,
    /// a dense operation can be large, so it must not overshoot the remaining work.
    pub(crate) fn try_charge(&mut self, work: usize) -> bool {
        if work > self.left {
            self.cut = true;
            return false;
        }
        self.charge(work, false);
        true
    }

    /// Charge a decomposition's `work`; `cut` when it stopped at the budget.
    pub(crate) fn charge(&mut self, work: usize, cut: bool) {
        self.left = self.left.saturating_sub(work);
        self.spent = self.spent.saturating_add(work);
        self.cut |= cut;
    }
}
/// Shorter than this (mm), a pair of points gives no direction to align on.
const DEGENERATE: f64 = 1e-9;

/// The constraint graph of a sketch reduced by cluster merging, relative to the known points.
/// More points can become known later (placed by propagation); the graph is extended and
/// reduced further rather than rebuilt.
pub(crate) struct Decomposition<'a> {
    /// `None` when nothing can be placed or the sketch is too large.
    graph: Option<Graph<'a>>,
}

impl<'a> Decomposition<'a> {
    /// Reduce the graph of the reading's relations relative to the known points (there may be
    /// none), unless it can place nothing (see [`may_place`]): then no graph is built.
    ///
    /// `known[i]` is the position of point `i` when it is already fixed (fixed, held or placed
    /// by propagation; a missing or non-finite entry is unknown). The reading's reference
    /// layout picks the branch of every construction (where it has no position for a point,
    /// every branch it would decide is left to the solver). `edits`: the solve applies edits
    /// (no drag step): a floating part whose relations miss at the reference may float.
    ///
    /// `limit` bounds the merge search's work over the decomposition's life (what is left of
    /// the solve's [`Budget`]).
    pub(crate) fn new(reading: &'a Reading, known: &[Option<Vec2>], edits: bool, limit: usize) -> Self {
        let known_at = |i: usize| known.get(i).copied().flatten().filter(|p| p.is_finite());
        if !may_place(reading, &known_at, edits) {
            return Decomposition { graph: None };
        }
        let mut graph = Graph::build(reading, &known_at, limit);
        if let Some(g) = &mut graph {
            g.edits = edits;
            g.reduce();
        }
        Decomposition { graph }
    }

    /// A graph was built: the decomposition may place points.
    pub(crate) fn built(&self) -> bool {
        self.graph.is_some()
    }

    /// The points are now known at their positions: merge what that lets merge.
    pub(crate) fn know(&mut self, points: &[(usize, Vec2)]) {
        let Some(g) = &mut self.graph else { return };
        for (i, at) in points.iter().filter(|(_, at)| at.is_finite()) {
            g.know(*i, *at);
        }
        g.reduce();
    }

    /// Where the ground cluster has point `i` (known points included, at their known
    /// positions); None when the decomposition did not reach it or built no graph.
    pub(crate) fn pos(&mut self, i: usize) -> Option<Vec2> {
        let g = self.graph.as_mut()?;
        let node = g.node(i);
        g.cluster(g.ground?)?.pts.get(&node).copied().filter(|p| p.is_finite())
    }

    /// The work the merge search did, and whether it stopped at its limit.
    pub(crate) fn work(&self) -> (usize, bool) {
        self.graph.as_ref().map_or((0, false), |g| (g.work, g.work > g.limit))
    }

    /// The merge search's budget is spent: nothing more will merge.
    pub(crate) fn exhausted(&self) -> bool {
        self.graph.as_ref().is_none_or(|g| g.work > g.limit)
    }

    /// The rigid clusters that are free or pinned at a single anchored point, each placed by its
    /// reference pose (see the module doc) as its points but the pin with their positions;
    /// pinned ones first, larger ones first. Only those the pose moves: pinned at a point that
    /// moved from its reference, or with a relation among its points that misses at the
    /// reference. A cluster holding a point of a pair whose readings disagree is left to the
    /// solver. `anchor(i)`: where the seed placed point `i`, if it builds on it (pins come only
    /// from there).
    pub(crate) fn floats(&mut self, anchor: &dyn Fn(usize) -> Option<Vec2>) -> Vec<Vec<(usize, Vec2)>> {
        self.graph.as_mut().map_or_else(Vec::new, |g| g.floats(anchor))
    }
}

/// Can a decomposition place anything? A point it places is rigid relative to known points or
/// to the other points of a cluster of three or more (or of an oriented pair), so it has two
/// relations (a coincidence counts twice, a tie relates each of its points to each other one)
/// to known points and to points that are themselves rigid. Peeling unknown points with fewer,
/// until none is left, keeps every point a decomposition could place; it places something
/// only if one is kept next to a known point, or (`edits`) a kept point misses a relation or
/// tie at the reference, so a floating cluster may float. A drag step moves only held points:
/// a floating part it does not reach is left as it is. Linear in the relations, so it is cheap
/// next to building the graph; this is where most drag frames of an under-constrained sketch
/// stop (chains of dimensioned links peel away entirely).
fn may_place(reading: &Reading, known: &dyn Fn(usize) -> Option<Vec2>, edits: bool) -> bool {
    let n = reading.points();
    let weight = |rel: Rel| if rel.joins() { 2 } else { 1 };
    let mut adj: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    let mut degree = vec![0usize; n];
    let ties = reading.tie_point_lists().flat_map(|pts| pts.iter().flat_map(move |p| pts.iter().filter(move |q| *q > p).map(move |q| (*p, *q, 1))));
    for (p, q, w) in reading.rels.iter().map(|&(p, q, rel)| (p, q, weight(rel))).chain(ties) {
        for (a, b) in [(p, q), (q, p)] {
            if let Some(list) = adj.get_mut(a) {
                list.push((b, w));
            }
            if let Some(d) = degree.get_mut(a) {
                *d += w;
            }
        }
    }
    let mut alive: Vec<bool> = (0..n).map(|i| known(i).is_none()).collect();
    let mut peel: Vec<usize> = (0..n).filter(|i| alive.get(*i).copied().unwrap_or(false) && degree.get(*i).is_some_and(|d| *d < 2)).collect();
    while let Some(i) = peel.pop() {
        if let Some(a) = alive.get_mut(i) {
            *a = false;
        }
        for &(o, w) in adj.get(i).into_iter().flatten() {
            if alive.get(o).copied().unwrap_or(false)
                && let Some(d) = degree.get_mut(o)
            {
                let was = *d;
                *d = d.saturating_sub(w);
                if was >= 2 && *d < 2 {
                    peel.push(o);
                }
            }
        }
    }
    let kept = |i: usize| alive.get(i).copied().unwrap_or(false);
    let next_to_known = |i: usize| adj.get(i).into_iter().flatten().any(|(o, _)| known(*o).is_some());
    let before = |i: usize| reading.before(i);
    let view = View { at: &before, oriented: true };
    let misses = |i: usize| before(i).is_none_or(|at| !reading.holds(i, at, &view));
    (0..n).any(|i| kept(i) && (next_to_known(i) || (edits && misses(i))))
}

#[cfg(test)]
thread_local! {
    static BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static LAST_WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Graphs built on this thread so far (tests count rebuilds with it).
#[cfg(test)]
pub(crate) fn builds() -> usize {
    BUILDS.with(std::cell::Cell::get)
}

/// The work charged to the last decomposition dropped on this thread (0 when it built no
/// graph).
#[cfg(test)]
pub(crate) fn last_work() -> usize {
    LAST_WORK.with(std::cell::Cell::get)
}

#[cfg(test)]
impl Drop for Decomposition<'_> {
    fn drop(&mut self) {
        let work = self.graph.as_ref().map_or(0, |g| g.work);
        LAST_WORK.with(|w| w.set(work));
    }
}

/// How much of a cluster's placement is still free, least free first: a merge keeps the
/// frame of its least free cluster.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Frame {
    /// World coordinates.
    Ground,
    /// World coordinates up to a translation.
    Oriented,
    /// World coordinates up to a rotation and a translation.
    Rigid,
}

#[derive(Clone, Debug)]
struct Cluster {
    frame: Frame,
    /// Node → position in the cluster's frame.
    pts: BTreeMap<usize, Vec2>,
    /// Node → the point the cluster's constraints are written on there. Coincident points
    /// may be apart in the reference; branches are picked from this point's side.
    via: BTreeMap<usize, usize>,
    /// Nodes whose neighbours [`Graph::grow`] has not tried yet.
    fresh: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    X,
    Y,
}

impl Axis {
    /// (the coordinate along the axis, the other one).
    fn split(self, v: Vec2) -> (f64, f64) {
        match self {
            Axis::X => (v.x, v.y),
            Axis::Y => (v.y, v.x),
        }
    }
    fn join(self, along: f64, other: f64) -> Vec2 {
        match self {
            Axis::X => Vec2::new(along, other),
            Axis::Y => Vec2::new(other, along),
        }
    }
}

/// One horizontal/vertical relation alone between two nodes: `axis(hi) − axis(lo) = delta`.
/// `ends` are the points the constraint is written on (on the `lo` and `hi` side), whose
/// reference positions pick branches: coincident points may be apart in the reference.
#[derive(Clone, Copy, Debug)]
struct AxisBond {
    lo: usize,
    hi: usize,
    ends: (usize, usize),
    axis: Axis,
    delta: f64,
}

/// The constraints this module reads between one pair of nodes, signed from `lo` to `hi`;
/// `ends` as in [`AxisBond`] (the lowest pair of points written on, so that nothing depends
/// on the order of the constraints).
#[derive(Clone, Copy, Debug)]
struct Bundle {
    ends: (usize, usize),
    dist: Option<Span>,
    dx: Option<Span>,
    dy: Option<Span>,
}

impl Bundle {
    /// The agreed (distance, dx, dy); None when readings of one kind disagree, or a distance
    /// disagrees with both offsets.
    fn agreed(&self) -> Option<(Option<f64>, Option<f64>, Option<f64>)> {
        let one = |s: Option<Span>| s.map_or(Some(None), |s| s.agreed().map(Some));
        let (dist, dx, dy) = (one(self.dist)?, one(self.dx)?, one(self.dy)?);
        if let (Some(len), Some(x), Some(y)) = (dist, dx, dy)
            && !Rel::Dist(len).holds(Vec2::ZERO, Vec2::new(x, y), 0.0)
        {
            return None;
        }
        Some((dist, dx, dy))
    }
}

/// Rotation then translation taking `from` to `to`; never a reflection.
#[derive(Clone, Copy, Debug)]
struct Motion {
    /// (cos, sin) of the rotation.
    rot: Vec2,
    from: Vec2,
    to: Vec2,
}

impl Motion {
    const IDENTITY: Motion = Motion { rot: Vec2::X, from: Vec2::ZERO, to: Vec2::ZERO };

    fn translate(from: Vec2, to: Vec2) -> Self {
        Motion { rot: Vec2::X, from, to }
    }

    /// The motion taking `ua` to `wa` and the direction `ua→ub` onto `wa→wb`.
    fn onto(ua: Vec2, ub: Vec2, wa: Vec2, wb: Vec2) -> Option<Self> {
        let (u, w) = (ub - ua, wb - wa);
        if !(u.len() > DEGENERATE && w.len() > DEGENERATE) {
            return None;
        }
        let rot = Vec2::new(u.dot(w), u.cross(w)).normalized()?;
        Some(Motion { rot, from: ua, to: wa })
    }

    fn apply(&self, p: Vec2) -> Vec2 {
        let d = p - self.from;
        self.to + Vec2::new(self.rot.x * d.x - self.rot.y * d.y, self.rot.y * d.x + self.rot.x * d.y)
    }
}

/// (cos, sin) of the rotation turning the vectors `u` onto their `w` best in the least-squares
/// sense (never a reflection); None when it is undecided (they are all zero, or the fit is as
/// good at every angle).
fn best_turn(pairs: impl Iterator<Item = (Vec2, Vec2)>) -> Option<Vec2> {
    let (mut sum, mut scale) = (Vec2::ZERO, 0.0);
    for (u, w) in pairs {
        sum += Vec2::new(u.dot(w), u.cross(w));
        scale += u.len() * w.len();
    }
    if !(sum.len() > 1e-9 * scale) {
        return None;
    }
    sum.normalized()
}

/// A merge: the clusters taking part, each with the motion into the merged frame (the first
/// one's positions win where they overlap), and the merged frame.
struct Merge {
    frame: Frame,
    parts: Vec<(usize, Motion)>,
}

/// The point at distance `r` from `centre` whose `axis` coordinate is `along` past
/// `centre`'s (circle ∩ axis line), on the side of the axis through `ref_centre` that
/// `ref_point` is on.
fn on_axis(axis: Axis, centre: Vec2, r: f64, along: f64, ref_centre: Option<Vec2>, ref_point: Option<Vec2>) -> Option<Vec2> {
    let loci = |c: Vec2| (Locus::Circle { c, r }, Locus::Line { o: c + axis.join(along, 0.0), d: axis.join(0.0, 1.0) });
    crossing(loci(centre), ref_centre.map(loci), ref_point)
}

/// The constraint graph reduced by cluster merging.
struct Graph<'a> {
    /// What the constraints say, and the reference layout (per point, not per node).
    reading: &'a Reading,
    /// Point → representative node (coincident points share one; final once built).
    node_of: Vec<usize>,
    /// Node → its points.
    points_of: Vec<Vec<usize>>,
    /// `None` once merged into another cluster; a merge keeps its base cluster's slot.
    clusters: Vec<Option<Cluster>>,
    /// Per cluster: waiting in `queue`.
    queued: Vec<bool>,
    /// Per node: in a pair whose readings disagree (left to the solver, never floated).
    conflicted: Vec<bool>,
    /// The solve applies edits (see [`Decomposition::new`]): a cluster whose relations miss at
    /// the reference floats; otherwise only one whose pin moved.
    edits: bool,
    /// Node → clusters containing it. Merged-away clusters are dropped lazily (they stay
    /// `None` for good), so a merge touches only the nodes that change cluster.
    member: Vec<Vec<usize>>,
    bonds: Vec<AxisBond>,
    /// Node → axis bonds touching it.
    bonds_of: Vec<Vec<usize>>,
    queue: VecDeque<usize>,
    /// Every scan of the merge search is charged here; past `limit` the search stops.
    work: usize,
    /// The work budget.
    limit: usize,
    /// The ground cluster's slot (a merge never moves it: the ground is always the base);
    /// always set once built, possibly empty.
    ground: Option<usize>,
}

impl<'a> Graph<'a> {
    fn build(reading: &'a Reading, known: &dyn Fn(usize) -> Option<Vec2>, limit: usize) -> Option<Self> {
        #[cfg(test)]
        BUILDS.with(|b| b.set(b.get() + 1));
        let n = reading.points();
        let rels = &reading.rels;
        // Coincident points, and points at distance zero, are one node: the lowest point of
        // its group.
        let mut parent: Vec<usize> = (0..n).collect();
        for &(p, q, rel) in rels {
            if rel.joins() {
                let (a, b) = (find(&mut parent, p), find(&mut parent, q));
                union(&mut parent, a.max(b), a.min(b));
            }
        }
        let node_of: Vec<usize> = (0..n).map(|i| find(&mut parent, i)).collect();
        let mut points_of = vec![Vec::new(); n];
        for (i, node) in node_of.iter().enumerate() {
            if let Some(list) = points_of.get_mut(*node) {
                list.push(i);
            }
        }
        let mut g = Graph {
            reading,
            node_of,
            points_of,
            clusters: Vec::new(),
            queued: Vec::new(),
            conflicted: vec![false; n],
            edits: false,
            member: vec![Vec::new(); n],
            bonds: Vec::new(),
            bonds_of: vec![Vec::new(); n],
            queue: VecDeque::new(),
            work: 0,
            limit,
            ground: None,
        };
        let mut ground = Cluster { frame: Frame::Ground, pts: BTreeMap::new(), via: BTreeMap::new(), fresh: Vec::new() };
        for i in 0..n {
            if let Some(p) = known(i) {
                let node = g.node(i);
                if let Entry::Vacant(slot) = ground.pts.entry(node) {
                    slot.insert(p);
                    ground.via.insert(node, i);
                }
            }
        }

        let mut bundles: BTreeMap<(usize, usize), Bundle> = BTreeMap::new();
        for &(p, q, rel) in rels {
            let (axis, value) = match rel {
                Rel::Same => continue,
                Rel::Dist(v) => (None, v),
                Rel::SameX => (Some(Axis::X), 0.0),
                Rel::SameY => (Some(Axis::Y), 0.0),
                Rel::OffsetX(v) => (Some(Axis::X), v),
                Rel::OffsetY(v) => (Some(Axis::Y), v),
            };
            let (a, b) = (g.node(p), g.node(q));
            if a == b {
                continue;
            }
            let ((lo, lp), (hi, hp)) = if a < b { ((a, p), (b, q)) } else { ((b, q), (a, p)) };
            // Signed offsets take the side the constrained points are on before the solve; a
            // nonzero offset whose side is unknown is left to the solver.
            let signed = |axis: Axis| {
                if value == 0.0 {
                    return Some(0.0);
                }
                let (from, to) = (g.at(lp)?, g.at(hp)?);
                let (c, _) = axis.split(to - from);
                side(c, value, mag(from).max(mag(to))).map(|s| s * value)
            };
            let Some(v) = axis.map_or(Some(value), signed) else { continue };
            let bundle = bundles.entry((lo, hi)).or_insert(Bundle { ends: (lp, hp), dist: None, dx: None, dy: None });
            bundle.ends = bundle.ends.min((lp, hp));
            Span::add(
                match axis {
                    None => &mut bundle.dist,
                    Some(Axis::X) => &mut bundle.dx,
                    Some(Axis::Y) => &mut bundle.dy,
                },
                v,
            );
        }

        let mut pairs = Vec::new();
        for ((lo, hi), bundle) in bundles {
            // Readings that disagree are left to the solver, which reports the conflict.
            let Some((dist, dx, dy)) = bundle.agreed() else {
                for node in [lo, hi] {
                    if let Some(c) = g.conflicted.get_mut(node) {
                        *c = true;
                    }
                }
                continue;
            };
            let ends = bundle.ends;
            if ground.pts.contains_key(&lo) && ground.pts.contains_key(&hi) {
                continue;
            }
            let dir = g.at(ends.1).zip(g.at(ends.0)).and_then(|(b, a)| (b - a).normalized());
            let offset = match (dx, dy, dist) {
                (Some(x), Some(y), _) => Some((Vec2::new(x, y), Frame::Oriented)),
                (Some(x), None, Some(len)) => on_axis(Axis::X, Vec2::ZERO, len, x, g.at(ends.0), g.at(ends.1)).map(|v| (v, Frame::Oriented)),
                (None, Some(y), Some(len)) => on_axis(Axis::Y, Vec2::ZERO, len, y, g.at(ends.0), g.at(ends.1)).map(|v| (v, Frame::Oriented)),
                (None, None, Some(len)) => Some((dir.unwrap_or(Vec2::X) * len, Frame::Rigid)),
                (Some(delta), None, None) => {
                    g.bond(AxisBond { lo, hi, ends, axis: Axis::X, delta });
                    None
                }
                (None, Some(delta), None) => {
                    g.bond(AxisBond { lo, hi, ends, axis: Axis::Y, delta });
                    None
                }
                (None, None, None) => None,
            };
            if let Some((v, frame)) = offset {
                let base = g.at(lo).unwrap_or_default();
                let pts = BTreeMap::from([(lo, base), (hi, base + v)]);
                pairs.push(Some(Cluster { frame, pts, via: BTreeMap::from([(lo, ends.0), (hi, ends.1)]), fresh: vec![lo, hi] }));
            }
            if pairs.len() > MAX_CLUSTERS {
                return None;
            }
        }
        // Kept even when nothing is known yet: floating clusters join it once placed.
        g.ground = Some(g.clusters.len());
        g.clusters.push(Some(ground));
        g.clusters.extend(pairs);
        for (id, c) in g.clusters.iter().enumerate() {
            for node in c.iter().flat_map(|c| c.pts.keys()) {
                if let Some(list) = g.member.get_mut(*node) {
                    list.push(id);
                }
            }
        }
        g.queued = vec![true; g.clusters.len()];
        g.queue = (0..g.clusters.len()).collect();
        Some(g)
    }

    /// Point `i` is known at `at`: add it to the ground and queue the other clusters holding it
    /// (they find the ground through it).
    fn know(&mut self, i: usize, at: Vec2) {
        let node = self.node(i);
        let Some(id) = self.ground else { return };
        let Some(Some(ground)) = self.clusters.get_mut(id) else { return };
        let Entry::Vacant(slot) = ground.pts.entry(node) else { return };
        slot.insert(at);
        ground.via.insert(node, i);
        self.join(node, id);
    }

    /// `node` joined cluster `id`: queue the other clusters holding it (they meet `id` through
    /// it).
    fn join(&mut self, node: usize, id: usize) {
        let Graph { member, queued, queue, .. } = self;
        if let Some(list) = member.get_mut(node) {
            for c in list.iter() {
                enqueue(queued, queue, *c);
            }
            list.push(id);
        }
    }

    /// Record an axis bond (it makes no cluster of its own).
    fn bond(&mut self, b: AxisBond) {
        let id = self.bonds.len();
        self.bonds.push(b);
        for e in [b.lo, b.hi] {
            if let Some(list) = self.bonds_of.get_mut(e) {
                list.push(id);
            }
        }
    }

    /// Representative node of point `i`.
    fn node(&self, i: usize) -> usize {
        self.node_of.get(i).copied().unwrap_or(i)
    }

    /// Reference position of a point (`None`: unknown).
    fn at(&self, point: usize) -> Option<Vec2> {
        self.reading.before(point)
    }

    /// Reference position of `node` as cluster `id` constrains it.
    fn at_node(&self, id: usize, node: usize) -> Option<Vec2> {
        self.at(self.cluster(id).and_then(|c| c.via.get(&node)).copied().unwrap_or(node))
    }

    fn cluster(&self, id: usize) -> Option<&Cluster> {
        self.clusters.get(id).and_then(Option::as_ref)
    }

    fn pos(&self, id: usize, node: usize) -> Option<Vec2> {
        self.cluster(id)?.pts.get(&node).copied()
    }

    fn frame(&self, id: usize) -> Frame {
        self.cluster(id).map_or(Frame::Rigid, |c| c.frame)
    }

    /// Nodes `x` and `y` share.
    fn common(&self, x: usize, y: usize) -> Vec<usize> {
        let (Some(a), Some(b)) = (self.cluster(x), self.cluster(y)) else { return Vec::new() };
        let (small, big) = if a.pts.len() <= b.pts.len() { (a, b) } else { (b, a) };
        small.pts.keys().copied().filter(|n| big.pts.contains_key(n)).collect()
    }

    /// The cost of scanning a rigid cluster's nodes and their axis bonds (only rigid
    /// clusters turn on bonds).
    fn bond_load(&self, id: usize) -> usize {
        self.cluster(id).filter(|c| c.frame == Frame::Rigid).map_or(0, |c| c.pts.keys().map(|n| 1 + self.bonds_of.get(*n).map_or(0, Vec::len)).sum())
    }

    /// Merge and grow clusters until none of the steps applies. A cluster is queued at most
    /// once at a time, and only again after a step changed it or a cluster it shares a node
    /// with; every pop and scan is charged to the work budget, which bounds the loop.
    fn reduce(&mut self) {
        while let Some(id) = self.queue.pop_front() {
            if let Some(q) = self.queued.get_mut(id) {
                *q = false;
            }
            self.work += 1;
            if self.work > self.limit {
                return;
            }
            if let Some(m) = self.find_merge(id) {
                self.absorb(m);
            } else if self.grow(id) {
                enqueue(&mut self.queued, &mut self.queue, id);
            }
        }
    }

    /// The first step `id` takes part in.
    fn find_merge(&mut self, id: usize) -> Option<Merge> {
        self.work += self.bond_load(id);
        if let Some(m) = self.orient(id) {
            return Some(m);
        }
        let nodes: Vec<usize> = self.cluster(id)?.pts.keys().copied().collect();
        // Pairs first, stopping at the first merge: a busy node's member list is read only
        // up to the first partner.
        let (mut tried, mut work) = (BTreeSet::new(), 0);
        let found = 'pairs: {
            for other in nodes.iter().flat_map(|n| self.member.get(*n).into_iter().flatten()) {
                work += 1;
                if self.work + work > self.limit {
                    break 'pairs None;
                }
                let Some(size) = self.cluster(*other).map(|c| c.pts.len()) else { continue };
                if *other == id || !tried.insert(*other) {
                    continue;
                }
                work += size.min(nodes.len());
                if let Some(m) = self.pair(id, *other, &self.common(id, *other)) {
                    break 'pairs Some(m);
                }
            }
            None
        };
        self.work += work;
        if found.is_some() || self.work > self.limit {
            return found;
        }
        // Neighbours with the nodes they share (dropping merged-away clusters on the way).
        let mut shared: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for node in &nodes {
            let clusters = &self.clusters;
            let Some(list) = self.member.get_mut(*node) else { continue };
            list.retain(|c| clusters.get(*c).is_some_and(Option::is_some));
            self.work += list.len();
            for other in list.iter().filter(|c| **c != id) {
                shared.entry(*other).or_default().push(*node);
            }
        }
        // Triangles through the node index: y shares `a` with x, z holds a node `b` of y
        // and shares another node `c` with x.
        let mut work = 0;
        let found = 'search: {
            let xc = self.cluster(id)?;
            for (y, ys) in &shared {
                let (Some(&a), Some(yc)) = (ys.first(), self.cluster(*y)) else { continue };
                for b in yc.pts.keys().filter(|b| !xc.pts.contains_key(b)) {
                    work += 1;
                    for z in self.member.get(*b).into_iter().flatten() {
                        work += 1;
                        if self.work + work > self.limit {
                            break 'search None;
                        }
                        let Some(c) = shared.get(z).filter(|_| z != y).and_then(|zs| zs.iter().find(|c| **c != a)) else { continue };
                        if let Some(m) = self.triangle([id, *y, *z], [a, *b, *c]) {
                            break 'search Some(m);
                        }
                    }
                }
            }
            None
        };
        self.work += work;
        if found.is_some() || self.work > self.limit {
            return found;
        }
        for (other, nodes) in &shared {
            if let [pivot] = nodes.as_slice() {
                self.work += self.bond_load(id) + self.bond_load(*other);
                if self.work > self.limit {
                    return None;
                }
                if let Some(m) = self.hinge(id, *other, *pivot) {
                    return Some(m);
                }
            }
        }
        None
    }

    /// A rigid cluster holding both ends of an axis bond turns until the bond holds and is
    /// then oriented. Of the two turns, the one keeping the bond's other component on its
    /// reference side. Bonds whose ends coincide in the cluster can't turn it; the next is
    /// tried.
    fn orient(&self, id: usize) -> Option<Merge> {
        let c = self.cluster(id).filter(|c| c.frame == Frame::Rigid)?;
        c.pts.keys().flat_map(|n| self.bonds_of.get(*n).into_iter().flatten()).filter_map(|b| self.bonds.get(*b)).find_map(|bond| {
            let (lo, hi) = (*c.pts.get(&bond.lo)?, *c.pts.get(&bond.hi)?);
            let target = on_axis(bond.axis, lo, lo.dist(hi), bond.delta, self.at(bond.ends.0), self.at(bond.ends.1))?;
            let m = Motion::onto(lo, hi, lo, target)?;
            Some(Merge { frame: Frame::Oriented, parts: vec![(id, m)] })
        })
    }

    /// Two clusters sharing points: two non-rigid ones by translation, a rigid one onto two
    /// shared points.
    fn pair(&self, x: usize, y: usize, shared: &[usize]) -> Option<Merge> {
        // The least free frame is kept; between equals the larger cluster stays put.
        let key = |c: usize| (self.frame(c), std::cmp::Reverse(self.cluster(c).map_or(0, |c| c.pts.len())));
        let (base, moved) = if key(y) < key(x) { (y, x) } else { (x, y) };
        let a = *shared.first()?;
        let (wa, ua) = (self.pos(base, a)?, self.pos(moved, a)?);
        let m = if self.frame(moved) != Frame::Rigid {
            Motion::translate(ua, wa)
        } else {
            // The other shared point farthest from `a` gives the best-conditioned direction.
            let far = |n: &usize| self.pos(base, *n).map_or(0.0, |w| w.dist(wa));
            let b = shared.iter().skip(1).max_by(|p, q| far(p).total_cmp(&far(q)))?;
            Motion::onto(ua, self.pos(moved, *b)?, wa, self.pos(base, *b)?)?
        };
        Some(Merge { frame: self.frame(base), parts: vec![(base, Motion::IDENTITY), (moved, m)] })
    }

    /// Owen's triangle step: clusters `[x, y, z]` hold the distinct nodes `[a, b, c]` as
    /// x ∋ c, a; y ∋ a, b; z ∋ b, c, so the distances between them are known. Construct the
    /// triangle in the frame of the least free cluster, keeping its reference orientation,
    /// and move the other two (rigid) onto it.
    fn triangle(&self, [x, y, z]: [usize; 3], [a, b, c]: [usize; 3]) -> Option<Merge> {
        // Each cluster with the two shared nodes it holds, in cyclic order (previous, next).
        let held = [(x, c, a), (y, a, b), (z, b, c)];
        let base = (0..3).min_by_key(|i| held.get(*i).map(|h| self.frame(h.0)))?;
        let (kp, p, r) = *held.get(base)?;
        let (kr, _, q) = *held.get((base + 1) % 3)?;
        let (kq, ..) = *held.get((base + 2) % 3)?;
        if self.frame(kr) != Frame::Rigid || self.frame(kq) != Frame::Rigid {
            return None;
        }
        let (wp, wr) = (self.pos(kp, p)?, self.pos(kp, r)?);
        let (rr, rq) = (self.pos(kr, r)?, self.pos(kr, q)?);
        let (qq, qp) = (self.pos(kq, q)?, self.pos(kq, p)?);
        let (to_p, to_r) = (qp.dist(qq), rr.dist(rq));
        let circles = |p: Vec2, r: Vec2| (Locus::Circle { c: p, r: to_p }, Locus::Circle { c: r, r: to_r });
        let then = self.at_node(kp, p).zip(self.at_node(kp, r)).map(|(p, r)| circles(p, r));
        // q keeps the turn p→r→q it has in the reference.
        let wq = crossing(circles(wp, wr), then, self.at_node(kr, q))?;
        let (mr, mq) = (Motion::onto(rr, rq, wr, wq)?, Motion::onto(qq, qp, wq, wp)?);
        Some(Merge { frame: self.frame(kp), parts: vec![(kp, Motion::IDENTITY), (kr, mr), (kq, mq)] })
    }

    /// A rigid cluster pinned at one point of a non-rigid one turns about the pin until an
    /// axis bond between them holds (circle ∩ axis line). The branch keeps the side of the
    /// pin the bonded point is on in the reference.
    fn hinge(&self, x: usize, y: usize, pivot: usize) -> Option<Merge> {
        let (base, moved) = if self.frame(y) < self.frame(x) { (y, x) } else { (x, y) };
        if self.frame(base) == Frame::Rigid || self.frame(moved) != Frame::Rigid {
            return None;
        }
        let (bc, mc) = (self.cluster(base)?, self.cluster(moved)?);
        let (centre, local_pivot) = (*bc.pts.get(&pivot)?, *mc.pts.get(&pivot)?);
        let pin = self.at_node(moved, pivot);
        mc.pts.iter().filter(|(n, _)| **n != pivot).find_map(|(u, local_u)| {
            let radius = local_u.dist(local_pivot);
            self.bonds_of.get(*u).into_iter().flatten().filter_map(|b| self.bonds.get(*b)).find_map(|bond| {
                let (other, delta, end) = if bond.hi == *u { (bond.lo, bond.delta, bond.ends.1) } else { (bond.hi, -bond.delta, bond.ends.0) };
                let (along, _) = bond.axis.split(*bc.pts.get(&other)? - centre);
                let target = on_axis(bond.axis, centre, radius, along + delta, pin, self.at(end))?;
                let m = Motion::onto(local_pivot, *local_u, centre, target)?;
                Some(Merge { frame: self.frame(base), parts: vec![(base, Motion::IDENTITY), (moved, m)] })
            })
        })
    }

    /// Owen's single-point merge: every point outside cluster `id` that its relations and
    /// ties to the cluster's points place at one crossing joins it (in the cluster's frame),
    /// and the clusters holding it are queued to meet the grown one. Only the neighbours of
    /// nodes not tried before are tried (a point's placement changes only when a point it is
    /// related to joins). The ground grows by propagation instead. True when a point joined.
    fn grow(&mut self, id: usize) -> bool {
        if self.ground == Some(id) {
            return false;
        }
        let Some(c) = self.clusters.get_mut(id).and_then(Option::as_mut) else { return false };
        let oriented = c.frame != Frame::Rigid;
        let mut scan = std::mem::take(&mut c.fresh);
        let mut grown = false;
        // Out of budget: the nodes not scanned stay fresh (nothing more merges anyway).
        let stop = |g: &mut Self, node: usize, scan: Vec<usize>| {
            if let Some(Some(c)) = g.clusters.get_mut(id) {
                c.fresh.extend(scan);
                c.fresh.push(node);
            }
        };
        while let Some(node) = scan.pop() {
            // Points outside the cluster related to a point of `node`.
            let mut candidates = BTreeSet::new();
            for p in self.points_of.get(node).into_iter().flatten() {
                let woken = self.reading.woken(*p);
                self.work += woken.len();
                for o in self.reading.related(*p).chain(woken) {
                    self.work += 1;
                    if self.work > self.limit {
                        stop(self, node, scan);
                        return grown;
                    }
                    if self.cluster(id).is_some_and(|c| !c.pts.contains_key(&self.node(o))) {
                        candidates.insert(o);
                    }
                }
            }
            for t in candidates {
                if self.work > self.limit {
                    stop(self, node, scan);
                    return grown;
                }
                let joining = self.node(t);
                let Some(c) = self.cluster(id).filter(|c| !c.pts.contains_key(&joining)) else { continue };
                let at = |i: usize| c.pts.get(&self.node(i)).copied();
                let items = self.reading.items(t, &View { at: &at, oriented });
                let fix = place(&items, self.at(t)).filter(|f| f.transversal);
                self.work += 1 + self.reading.adj(t).len() + self.reading.tie_count_of(t) + items.len().saturating_mul(items.len());
                let Some(fix) = fix else { continue };
                if let Some(Some(c)) = self.clusters.get_mut(id) {
                    c.pts.insert(joining, fix.at);
                    c.via.insert(joining, t);
                }
                self.join(joining, id);
                scan.push(joining);
                grown = true;
            }
        }
        grown
    }

    /// See [`Decomposition::floats`]. Charged a scan of the clusters and their relations.
    fn floats(&mut self, anchor: &dyn Fn(usize) -> Option<Vec2>) -> Vec<Vec<(usize, Vec2)>> {
        let node_at = |n: usize| self.points_of.get(n).into_iter().flatten().find_map(|i| anchor(*i));
        let mut work = 0;
        // (pinned first, larger first, lower slot first), slot, pin with its position.
        let mut picks = Vec::new();
        for (id, c) in self.clusters.iter().enumerate() {
            let Some(c) = c.as_ref().filter(|_| self.ground != Some(id)) else { continue };
            work += c.pts.len();
            if self.work + work > self.limit {
                self.work += work;
                return Vec::new();
            }
            if c.pts.keys().any(|n| self.conflicted.get(*n).copied().unwrap_or(false)) {
                continue;
            }
            let mut pins = c.pts.keys().filter_map(|n| Some((*n, node_at(*n)?)));
            let (pin, more) = (pins.next(), pins.next().is_some());
            let eligible = match c.frame {
                Frame::Rigid => c.pts.len() >= 3 && !more,
                Frame::Oriented => c.pts.len() >= 2 && pin.is_none(),
                Frame::Ground => false,
            };
            if eligible && (pin.is_some_and(|(n, at)| self.at_node(id, n) != Some(at)) || (self.edits && self.misses(id, &mut work))) {
                picks.push(((pin.is_none(), std::cmp::Reverse(c.pts.len()), id), pin));
            }
        }
        self.work += work;
        picks.sort_by_key(|(key, _)| *key);
        picks.into_iter().filter_map(|((.., id), pin)| self.pose(id, pin)).collect()
    }

    /// A relation or tie among cluster `id`'s points misses at the reference (or a point has
    /// no reference position). Charges `work`.
    fn misses(&self, id: usize, work: &mut usize) -> bool {
        let Some(c) = self.cluster(id) else { return false };
        let inside = |i: usize| c.pts.contains_key(&self.node(i)).then(|| self.at(i)).flatten();
        let view = View { at: &inside, oriented: true };
        c.pts.keys().flat_map(|n| self.points_of.get(*n).into_iter().flatten()).any(|i| {
            *work += 1 + self.reading.adj(*i).len() + self.reading.tie_count_of(*i);
            self.at(*i).is_none_or(|at| !self.reading.holds(*i, at, &view))
        })
    }

    /// Cluster `id` where its reference pose puts it (see the module doc), pinned at a node
    /// placed at a position if any: each of its points but the pin's, with its position. None
    /// when the reference decides no pose.
    fn pose(&self, id: usize, pin: Option<(usize, Vec2)>) -> Option<Vec<(usize, Vec2)>> {
        let c = self.cluster(id)?;
        let moved = || c.pts.iter().filter(move |(n, _)| pin.is_none_or(|(p, _)| **n != p));
        // (position in the cluster's frame, reference position) of each node but the pin.
        let pairs: Vec<(Vec2, Vec2)> = moved().filter_map(|(n, p)| Some((*p, self.at_node(id, *n)?))).collect();
        let centroids = || {
            let (u, w) = pairs.iter().fold((Vec2::ZERO, Vec2::ZERO), |(su, sw), (u, w)| (su + *u, sw + *w));
            (u / pairs.len() as f64, w / pairs.len() as f64)
        };
        // The point the cluster turns about, in its frame and at its reference, where that
        // point goes, and whether it turns.
        let (from, about, to, turns) = match (c.frame, pin) {
            (Frame::Oriented, None) if !pairs.is_empty() => {
                let (from, to) = centroids();
                (from, to, to, false)
            }
            (Frame::Rigid, Some((pin, at))) => (*c.pts.get(&pin)?, self.at_node(id, pin).unwrap_or(at), at, true),
            (Frame::Rigid, None) if pairs.len() >= 2 => {
                let (from, to) = centroids();
                (from, to, to, true)
            }
            _ => return None,
        };
        let rot = if turns { best_turn(pairs.iter().map(|(u, w)| (*u - from, *w - about)))? } else { Vec2::X };
        let motion = Motion { rot, from, to };
        let out: Vec<(usize, Vec2)> =
            moved().flat_map(|(n, p)| self.points_of.get(*n).into_iter().flatten().map(move |i| (*i, motion.apply(*p)))).collect();
        out.iter().all(|(_, at)| at.is_finite()).then_some(out)
    }

    /// Replace the merged clusters by their union, kept in the first one's slot (so only the
    /// nodes joining it change membership), and queue it.
    fn absorb(&mut self, m: Merge) {
        let Some(&(base, base_motion)) = m.parts.first() else { return };
        let Some(mut merged) = self.clusters.get_mut(base).and_then(Option::take) else { return };
        merged.frame = m.frame;
        // Only `orient` moves the base (a merge's base stays put, and may be the ground); an
        // oriented cluster reads relations along the axes, so every node is tried again.
        if m.parts.len() == 1 {
            self.work += merged.pts.len();
            for p in merged.pts.values_mut() {
                *p = base_motion.apply(*p);
            }
            merged.fresh = merged.pts.keys().copied().collect();
        }
        for (id, motion) in m.parts.iter().skip(1) {
            let Some(c) = self.clusters.get_mut(*id).and_then(Option::take) else { continue };
            self.work += c.pts.len();
            for (node, p) in c.pts {
                if let Entry::Vacant(slot) = merged.pts.entry(node) {
                    slot.insert(motion.apply(p));
                    merged.fresh.push(node);
                    if let Some(point) = c.via.get(&node) {
                        merged.via.entry(node).or_insert(*point);
                    }
                    if let Some(list) = self.member.get_mut(node) {
                        list.push(base);
                    }
                }
            }
        }
        // The ground grows by propagation: it keeps no nodes to try.
        if merged.frame == Frame::Ground {
            merged.fresh.clear();
        }
        if let Some(slot) = self.clusters.get_mut(base) {
            *slot = Some(merged);
        }
        enqueue(&mut self.queued, &mut self.queue, base);
    }
}

/// Push `i` onto a work queue unless `queued[i]` says it is waiting there already.
pub(crate) fn enqueue(queued: &mut [bool], queue: &mut VecDeque<usize>, i: usize) {
    if let Some(q) = queued.get_mut(i)
        && !*q
    {
        *q = true;
        queue.push_back(i);
    }
}

#[cfg(test)]
#[path = "decompose_tests.rs"]
mod tests;

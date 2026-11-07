//! Arrange: lay flat parts out on a sheet (cutting stock, a build plate), apart from each other by
//! a spacing and from the sheet's edge by a margin.
//!
//! Parts and the sheet are polygons with holes, in any orientation. Each part is tried at each of
//! its allowed rotations and at a set of start positions (along the sheet's top, beside and above
//! the parts already placed, inside their holes, at the sheet's corners); from each start it falls
//! down and slides left until it touches something, and the position that leaves it lowest, then
//! leftmost, wins. Parts go largest first, so small ones can still fill the gaps and holes the
//! large ones leave; an optional search then tries other orders and keeps the best layout (more
//! area placed first, then a lower, then a narrower layout).
//!
//! Every test is exact on the polygons given, up to a tolerance of 1e-9 of the sheet's size:
//! - Two regions overlap when an edge of one properly crosses an edge of the other, or when a
//!   piece of one's boundary (its edges cut wherever the other's boundary meets them) lies inside
//!   the other, or runs along the other's boundary with both materials on the same side. This
//!   holds at zero spacing too, where parts may touch.
//! - A part keeps a clearance when every corner of each is at least that far from every edge of
//!   the other (the closest points of two polygons are always a corner of one and an edge of the
//!   other).
//! - A part moves until one of its corners reaches the band of the clearance's width around an
//!   edge of something else, or a corner of something else reaches the band around one of its own
//!   edges; at zero clearance, a corner on an edge or a corner stops only when it would move into
//!   the material there.
//!
//! A caller that flattens curves into polygons widens the spacing by its chord error, and the
//! layout keeps the true curves apart too.
//!
//! The same input always gives the same layout on one platform: the order search draws from a
//! seeded generator and stops after a set number of tries, never on the clock. Only a caller's
//! time budget ([`arrange_hurried`]) can change a layout, and only once it says to hurry. (Turns that are not
//! multiples of 90° use the platform's sine and cosine, which may differ in the last bit between
//! platforms.)

use std::sync::atomic::{AtomicBool, Ordering};

use solidcore_record::Vec2;

use crate::Aabb2;

/// Most copies of all parts together [`arrange`] places.
pub const MAX_COPIES: usize = 10_000;
/// Most corners a part (all its rings together) or the sheet may have.
pub const MAX_CORNERS: usize = 4_096;
/// Most rotations a part may list.
pub const MAX_ROTATIONS: usize = 360;
/// Most corners over every rotation of every part together.
pub const MAX_POSE_CORNERS: usize = 1_000_000;
/// Most order-search tries.
pub const MAX_IMPROVE: usize = 10_000;
/// Largest coordinate, spacing or margin accepted (in the caller's units).
pub const MAX_COORD: f64 = 1e9;

/// A flat region: an outer ring and the rings of its holes, each at least three corners, in either
/// orientation, closed implicitly (the last corner joins the first; a repeated first corner at the
/// end is dropped).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape2 {
    pub outer: Vec<Vec2>,
    pub holes: Vec<Vec<Vec2>>,
}

impl Shape2 {
    /// The rectangle from (0, 0) to (`w`, `h`).
    pub fn rectangle(w: f64, h: f64) -> Shape2 {
        Shape2 { outer: vec![Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h)], holes: Vec::new() }
    }
}

/// One kind of part: its shape, how many copies, and the angles (degrees, counter-clockwise) it
/// may be turned by. No angles means it keeps its orientation.
#[derive(Clone, Debug, PartialEq)]
pub struct ArrangeItem {
    pub shape: Shape2,
    pub quantity: usize,
    pub rotations_deg: Vec<f64>,
}

/// How parts are fitted together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArrangeMethod {
    /// Each part's own outline, so parts tuck into each other's corners and holes.
    #[default]
    TrueShape,
    /// Each part's bounding rectangle at its rotation: neat rows, never one part inside another.
    Rectangular,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArrangeOptions {
    pub method: ArrangeMethod,
    /// Least distance between two parts.
    pub spacing: f64,
    /// Least distance between a part and the sheet's edge (and the sheet's holes).
    pub margin: f64,
    /// Whether a part may go inside a hole of another part.
    pub part_in_part: bool,
    /// How many other part orders to try after the first layout (0: largest first only).
    pub improve: usize,
    /// Seed of the order search.
    pub seed: u64,
}

impl Default for ArrangeOptions {
    fn default() -> Self {
        ArrangeOptions { method: ArrangeMethod::TrueShape, spacing: 0.0, margin: 0.0, part_in_part: true, improve: 0, seed: 1 }
    }
}

/// Where one copy went: a point `p` of the item's shape lands at `rotate(p, angle) + shift`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement2 {
    pub item: usize,
    pub copy: usize,
    /// Radians, counter-clockwise.
    pub angle: f64,
    pub shift: Vec2,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArrangeResult {
    pub placed: Vec<Placement2>,
    /// For each item, how many copies did not fit.
    pub unplaced: Vec<usize>,
    /// The box around the placed parts.
    pub used: Aabb2,
    /// Placed parts' area over the sheet's area.
    pub fill: f64,
    /// The most the caller's budget made the layout hurry ([`Hurry::No`] when it never did).
    pub hurried: Hurry,
}

/// How far behind its time budget a layout is, as the caller of [`arrange_hurried`] judges it:
/// what is given up to keep up. Each step also does what the ones before it do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Hurry {
    /// The full search.
    #[default]
    No,
    /// No starts from corners meeting placed parts' corners, and the first spot that fits wins;
    /// no other orders are tried.
    Quick,
    /// Each part still to place goes by its box at its turn.
    Boxes,
    /// No more parts are placed: those left count as unplaced.
    Stop,
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ArrangeError {
    #[error("{what} has a corner that is not a finite number below {MAX_COORD}")]
    NotFinite { what: String },
    #[error("{what} has a ring with fewer than 3 corners")]
    TooFewCorners { what: String },
    #[error("{what} has more than {MAX_CORNERS} corners")]
    TooManyCorners { what: String },
    #[error("{what} has a ring with no area")]
    NoArea { what: String },
    #[error("{what} has a ring that crosses or touches itself or another of its rings")]
    SelfCrossing { what: String },
    #[error("{what} has a hole that is not inside its outer ring")]
    HoleOutside { what: String },
    #[error("{what} has a hole inside another hole")]
    HolesNested { what: String },
    #[error("{what} must be a finite number from 0 to {MAX_COORD}, got {value}")]
    BadDistance { what: &'static str, value: f64 },
    #[error("part {item} has a rotation that is not a finite number")]
    BadRotation { item: usize },
    #[error("part {item} lists more than {MAX_ROTATIONS} rotations")]
    TooManyRotations { item: usize },
    #[error("more than {MAX_COPIES} copies in all")]
    TooManyCopies,
    #[error("the parts at all their rotations have more than {MAX_POSE_CORNERS} corners")]
    TooMuchWork,
    #[error("more than {MAX_IMPROVE} order-search tries")]
    TooManyTries,
    #[error("arrange was cancelled")]
    Cancelled,
}

/// Lays out `items` on `sheet`. Copies that do not fit are counted in
/// [`ArrangeResult::unplaced`]; nothing placed ever overlaps another part, comes closer than the
/// spacing to another part, leaves the sheet, covers one of its holes or comes closer than the
/// margin to its edges. `cancel`, when set, stops the work within one part's search.
pub fn arrange(items: &[ArrangeItem], sheet: &Shape2, options: &ArrangeOptions, cancel: Option<&AtomicBool>) -> Result<ArrangeResult, ArrangeError> {
    arrange_around(items, sheet, &[], options, &|| cancel.is_some_and(|c| c.load(Ordering::Relaxed)))
}

/// Most fixed obstacles [`arrange_around`] takes.
pub const MAX_OBSTACLES: usize = 2_000;

/// [`arrange`], with `obstacles` already standing where they are (sheet coordinates, anywhere):
/// nothing placed comes closer than the spacing to one. They count for neither the fill nor
/// the used box. The work stops soon after `cancel` says so.
pub fn arrange_around(
    items: &[ArrangeItem],
    sheet: &Shape2,
    obstacles: &[Shape2],
    options: &ArrangeOptions,
    cancel: &dyn Fn() -> bool,
) -> Result<ArrangeResult, ArrangeError> {
    arrange_hurried(items, sheet, obstacles, options, cancel, &|| Hurry::No)
}

/// [`arrange_around`], asking `hurry` before each part how far behind a time budget it is and
/// giving up layout quality to match ([`Hurry`]): a caller's clock bounds the work, which on its
/// own grows with the parts, their corners and their turns.
pub fn arrange_hurried(
    items: &[ArrangeItem],
    sheet: &Shape2,
    obstacles: &[Shape2],
    options: &ArrangeOptions,
    cancel: &dyn Fn() -> bool,
    hurry: &dyn Fn() -> Hurry,
) -> Result<ArrangeResult, ArrangeError> {
    if obstacles.len() > MAX_OBSTACLES {
        return Err(ArrangeError::TooMuchWork);
    }
    for (what, v) in [("spacing", options.spacing), ("margin", options.margin)] {
        if !(0.0..=MAX_COORD).contains(&v) {
            return Err(ArrangeError::BadDistance { what, value: v });
        }
    }
    if options.improve > MAX_IMPROVE {
        return Err(ArrangeError::TooManyTries);
    }
    // Everything is worked out with the sheet's box starting at (0, 0), so coordinates far from
    // the origin lose no precision.
    let sheet = Region::new(sheet, "the sheet")?;
    let mut copies = 0usize;
    let mut corners = 0usize;
    let mut kinds = Vec::with_capacity(items.len());
    let mut boxes = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        if item.rotations_deg.len() > MAX_ROTATIONS {
            return Err(ArrangeError::TooManyRotations { item: i });
        }
        if item.rotations_deg.iter().any(|a| !a.is_finite()) {
            return Err(ArrangeError::BadRotation { item: i });
        }
        let region = Region::new(&item.shape, &format!("part {i}"))?;
        copies = copies.saturating_add(item.quantity);
        if copies > MAX_COPIES {
            return Err(ArrangeError::TooManyCopies);
        }
        let kind =
            if item.quantity == 0 { Kind { poses: Vec::new(), area: region.area } } else { Kind::new(&region, &item.rotations_deg, options.method) };
        corners = corners.saturating_add(kind.poses.iter().map(|p| p.shape.corners()).sum());
        if corners > MAX_POSE_CORNERS {
            return Err(ArrangeError::TooMuchWork);
        }
        kinds.push(kind);
        // The same turns by their boxes, for when time runs short.
        boxes.push(if item.quantity == 0 {
            Kind { poses: Vec::new(), area: region.area }
        } else {
            Kind::new(&region, &item.rotations_deg, ArrangeMethod::Rectangular)
        });
    }
    let size = sheet.shape.bbox.diag();
    // Obstacles off the sheet's box (grown by the spacing) can never be in the way.
    let reach = sheet.shape.bbox.grow(options.spacing + options.margin);
    let mut fixed = Vec::new();
    for (i, o) in obstacles.iter().enumerate() {
        let region = Region::new(o, &format!("obstacle {i}"))?;
        corners = corners.saturating_add(region.shape.corners());
        if corners > MAX_POSE_CORNERS {
            return Err(ArrangeError::TooMuchWork);
        }
        let shape = region.shape.moved(region.min.sub(sheet.min));
        if shape.bbox.hits(&reach) {
            let poles = poles(&shape);
            fixed.push(Laid { kind: usize::MAX, pose: 0, at: P::new(0.0, 0.0), shape, poles });
        }
    }
    let ctx = Ctx {
        fixed: &fixed,
        sheet: &sheet.shape,
        kinds: &kinds,
        spacing: options.spacing,
        margin: options.margin,
        part_in_part: options.part_in_part && options.method == ArrangeMethod::TrueShape,
        tol: 1e-9 * size,
        cancel,
        boxes: &boxes,
        hurry,
        hurried: std::cell::Cell::new(Hurry::No),
    };

    // Largest first; ties keep the caller's order. Each part may take any of its turns.
    let mut order: Vec<Gene> = Vec::with_capacity(copies);
    for (i, item) in items.iter().enumerate() {
        order.extend(std::iter::repeat_n(Gene { kind: i, pose: None }, item.quantity));
    }
    order.sort_by(|a, b| kinds_area(&kinds, b.kind).total_cmp(&kinds_area(&kinds, a.kind)));

    let better = Better { area: 1e-9 * sheet.area, length: ctx.tol * 1e3 };
    let mut best = ctx.lay(&order, None)?;
    let mut rng = Rng(options.seed ^ 0x9E37_79B9_7F4A_7C15);
    for _ in 0..options.improve {
        if hurry() > Hurry::No {
            break;
        }
        let poses = |k: usize| kinds.get(k).map_or(0, |kind| kind.poses.len());
        let Some((next, from)) = rng.reorder(&order, poses) else { break };
        let layout = ctx.lay(&next, Some((&best, from)))?;
        // A layout as good as the best is taken too, so the search walks across plateaus
        // (a turn held that changes nothing yet) instead of stopping on them.
        if !better.is(&best, &layout) {
            best = layout;
            order = next;
        }
    }

    let sheet_area = sheet.area.max(f64::MIN_POSITIVE);
    let mut placed = Vec::with_capacity(best.parts.len());
    let mut done = vec![0usize; items.len()];
    let mut used = Aabb2::EMPTY;
    let mut area = 0.0;
    for p in &best.parts {
        let (Some(kind), Some(n)) = (kinds.get(p.kind), done.get_mut(p.kind)) else { continue };
        let Some(pose) = kind.poses.get(p.pose) else { continue };
        let shift = p.at.add(pose.base).add(sheet.min);
        placed.push(Placement2 { item: p.kind, copy: *n, angle: pose.angle, shift: Vec2::new(shift.x, shift.y) });
        *n += 1;
        let b = p.shape.bbox.shift(sheet.min);
        used = used.union(&Aabb2 { min: Vec2::new(b.min.x, b.min.y), max: Vec2::new(b.max.x, b.max.y) });
        area += kind.area;
    }
    let unplaced = items.iter().zip(&done).map(|(item, n)| item.quantity.saturating_sub(*n)).collect();
    Ok(ArrangeResult { placed, unplaced, used, fill: area / sheet_area, hurried: ctx.hurried.get() })
}

fn kinds_area(kinds: &[Kind], i: usize) -> f64 {
    kinds.get(i).map_or(0.0, |k| k.area)
}

// ---------- points, boxes, rings ----------

#[derive(Clone, Copy, Debug, PartialEq)]
struct P {
    x: f64,
    y: f64,
}

impl P {
    const fn new(x: f64, y: f64) -> P {
        P { x, y }
    }
    fn add(self, o: P) -> P {
        P::new(self.x + o.x, self.y + o.y)
    }
    fn sub(self, o: P) -> P {
        P::new(self.x - o.x, self.y - o.y)
    }
    fn scale(self, k: f64) -> P {
        P::new(self.x * k, self.y * k)
    }
    fn dot(self, o: P) -> f64 {
        self.x * o.x + self.y * o.y
    }
    fn cross(self, o: P) -> f64 {
        self.x * o.y - self.y * o.x
    }
    fn len(self) -> f64 {
        self.dot(self).sqrt()
    }
    /// Turned a quarter counter-clockwise.
    fn left(self) -> P {
        P::new(-self.y, self.x)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Box2 {
    min: P,
    max: P,
}

impl Box2 {
    const EMPTY: Box2 = Box2 { min: P::new(f64::INFINITY, f64::INFINITY), max: P::new(f64::NEG_INFINITY, f64::NEG_INFINITY) };
    const ALL: Box2 = Box2 { min: P::new(f64::NEG_INFINITY, f64::NEG_INFINITY), max: P::new(f64::INFINITY, f64::INFINITY) };
    fn at(p: P) -> Box2 {
        Box2 { min: p, max: p }
    }
    fn of<'a>(pts: impl IntoIterator<Item = &'a P>) -> Box2 {
        pts.into_iter().fold(Box2::EMPTY, |b, p| b.with(*p))
    }
    fn seg(a: P, b: P) -> Box2 {
        Box2::EMPTY.with(a).with(b)
    }
    fn with(self, p: P) -> Box2 {
        Box2 { min: P::new(self.min.x.min(p.x), self.min.y.min(p.y)), max: P::new(self.max.x.max(p.x), self.max.y.max(p.y)) }
    }
    fn union(self, o: Box2) -> Box2 {
        Box2 { min: P::new(self.min.x.min(o.min.x), self.min.y.min(o.min.y)), max: P::new(self.max.x.max(o.max.x), self.max.y.max(o.max.y)) }
    }
    fn grow(self, d: f64) -> Box2 {
        Box2 { min: P::new(self.min.x - d, self.min.y - d), max: P::new(self.max.x + d, self.max.y + d) }
    }
    fn shift(self, by: P) -> Box2 {
        Box2 { min: self.min.add(by), max: self.max.add(by) }
    }
    fn hits(&self, o: &Box2) -> bool {
        self.min.x <= o.max.x && o.min.x <= self.max.x && self.min.y <= o.max.y && o.min.y <= self.max.y
    }
    fn diag(&self) -> f64 {
        self.size().len()
    }
    fn size(&self) -> P {
        self.max.sub(self.min)
    }
}

/// The edges of a closed ring.
fn edges(ring: &[P]) -> impl Iterator<Item = (P, P)> + '_ {
    ring.iter().copied().zip(ring.iter().copied().cycle().skip(1))
}

/// Signed area, counter-clockwise positive; taken about the first corner so it stays accurate
/// away from the origin.
fn ring_area(ring: &[P]) -> f64 {
    let Some(&o) = ring.first() else { return 0.0 };
    edges(ring).map(|(a, b)| a.sub(o).cross(b.sub(o))).sum::<f64>() / 2.0
}

fn seg_dist(p: P, a: P, b: P) -> f64 {
    let ab = b.sub(a);
    let l = ab.dot(ab);
    let t = if l > 0.0 { (p.sub(a).dot(ab) / l).clamp(0.0, 1.0) } else { 0.0 };
    p.sub(a.add(ab.scale(t))).len()
}

/// Whether segments a-b and c-d cross at a point inside both, each passing more than `tol` to
/// either side of the other (not merely touching).
fn crosses(a: P, b: P, c: P, d: P, tol: f64) -> bool {
    let side = |o: P, p: P, q: P| {
        let v = p.sub(o);
        let len = v.len();
        if len == 0.0 { 0.0 } else { v.cross(q.sub(o)) / len }
    };
    let (d1, d2) = (side(c, d, a), side(c, d, b));
    let (d3, d4) = (side(a, b, c), side(a, b, d));
    ((d1 > tol && d2 < -tol) || (d1 < -tol && d2 > tol)) && ((d3 > tol && d4 < -tol) || (d3 < -tol && d4 > tol))
}

fn seg_seg_dist(a: P, b: P, c: P, d: P) -> f64 {
    if crosses(a, b, c, d, 0.0) {
        return 0.0;
    }
    seg_dist(a, c, d).min(seg_dist(b, c, d)).min(seg_dist(c, a, b)).min(seg_dist(d, a, b))
}

const TAU: f64 = std::f64::consts::TAU;
const PI: f64 = std::f64::consts::PI;
const ANGLE_TOL: f64 = 1e-9;

fn angle(p: P) -> f64 {
    p.y.atan2(p.x).rem_euclid(TAU)
}

/// The material at corner `w` (between the edges to `prev` and `next`, the material on side
/// `side` of the ring: +1 left, -1 right), as the open arc of directions from `start` turning
/// counter-clockwise by `width`.
#[derive(Clone, Copy, Debug)]
struct Wedge {
    start: f64,
    width: f64,
}

impl Wedge {
    fn at(prev: P, w: P, next: P, side: f64) -> Wedge {
        let (to_prev, to_next) = (prev.sub(w), next.sub(w));
        let (s, e) = if side > 0.0 { (to_next, to_prev) } else { (to_prev, to_next) };
        let start = angle(s);
        Wedge { start, width: (angle(e) - start).rem_euclid(TAU) }
    }

    fn turned_half(self) -> Wedge {
        Wedge { start: (self.start + PI).rem_euclid(TAU), width: self.width }
    }

    /// Convex pieces (each under half a turn).
    fn halves(self) -> [Wedge; 2] {
        let h = self.width / 2.0;
        [Wedge { start: self.start, width: h }, Wedge { start: (self.start + h).rem_euclid(TAU), width: h }]
    }

    /// Where the arc starts, measured counter-clockwise from direction angle `d`.
    fn from(self, d: f64) -> f64 {
        (self.start - d).rem_euclid(TAU)
    }

    /// Whether two arcs share directions.
    fn meets(self, o: Wedge) -> bool {
        o.from(self.start) < self.width - ANGLE_TOL || self.from(o.start) < o.width - ANGLE_TOL
    }

    fn holds(self, d: f64) -> bool {
        self.from(d) + self.width > TAU + ANGLE_TOL
    }

    /// The least turn counter-clockwise from `d` into the arc within half a turn, if any.
    fn above(self, d: f64) -> f64 {
        let s = self.from(d);
        if self.holds(d) {
            0.0
        } else if s < PI {
            s
        } else {
            f64::INFINITY
        }
    }

    /// The least turn clockwise from `d` into the arc within half a turn, if any.
    fn below(self, d: f64) -> f64 {
        Wedge { start: (2.0 * d - self.start - self.width).rem_euclid(TAU), width: self.width }.above(d)
    }
}

/// Whether moving a part by a little along `dir` makes its material at a corner (`part`) meet the
/// material of an obstacle at the same point (`obstacle`): whether `dir` is in the sum of the
/// obstacle's wedge and the part's wedge turned half a turn.
fn corners_meet(dir: P, obstacle: Wedge, part: Wedge) -> bool {
    let d = angle(dir);
    let back = part.turned_half();
    obstacle.halves().into_iter().any(|o| {
        back.halves()
            .into_iter()
            .any(|p| o.holds(d) || p.holds(d) || o.above(d) + p.below(d) < PI - ANGLE_TOL || p.above(d) + o.below(d) < PI - ANGLE_TOL)
    })
}

/// Edges per run: a ring keeps the box of each run of this many edges, so a test near one place
/// skips the runs that are far from it.
const RUN: usize = 16;

/// A closed ring and the side its material is on.
#[derive(Clone, Debug)]
struct Ring {
    pts: Vec<P>,
    /// +1 when the material is left of each edge, -1 when right.
    side: f64,
    bbox: Box2,
    /// The box of each run of edges (run k: the edges from corners k·RUN up to (k+1)·RUN).
    runs: Vec<Box2>,
}

impl Ring {
    fn new(pts: Vec<P>, side: f64) -> Ring {
        let n = pts.len();
        let runs = (0..n.div_ceil(RUN))
            .map(|k| (k * RUN..=((k + 1) * RUN).min(n)).fold(Box2::EMPTY, |b, i| pts.get(i % n.max(1)).map_or(b, |p| b.with(*p))))
            .collect();
        Ring { bbox: Box2::of(&pts), runs, pts, side }
    }

    fn pt(&self, i: usize) -> P {
        self.pts.get(i % self.pts.len().max(1)).copied().unwrap_or(P::new(0.0, 0.0))
    }

    /// The edge from corner `i` to the next.
    fn edge(&self, i: usize) -> (P, P) {
        (self.pt(i), self.pt(i + 1))
    }

    /// Corner `i` with the corners before and after it.
    fn corner(&self, i: usize) -> (P, P, P) {
        (self.pt(i + self.pts.len() - 1), self.pt(i), self.pt(i + 1))
    }

    fn run(&self, k: usize) -> std::ops::Range<usize> {
        k * RUN..((k + 1) * RUN).min(self.pts.len())
    }

    /// The corners (and the edges from them) of the runs whose box meets `b`.
    fn near<'a>(&'a self, b: &'a Box2) -> impl Iterator<Item = usize> + 'a {
        self.runs.iter().enumerate().filter(|(_, r)| r.hits(b)).flat_map(|(k, _)| self.run(k))
    }

    fn edges_near<'a>(&'a self, b: &'a Box2) -> impl Iterator<Item = (P, P)> + 'a {
        self.near(b).map(|i| self.edge(i))
    }

    fn moved(&self, by: P) -> Ring {
        Ring {
            pts: self.pts.iter().map(|p| p.add(by)).collect(),
            side: self.side,
            bbox: self.bbox.shift(by),
            runs: self.runs.iter().map(|r| r.shift(by)).collect(),
        }
    }
}

/// Pairs of runs, one of `x` and one of `y`, whose boxes meet once `x`'s are widened by `widen`.
fn run_pairs(x: &Ring, widen: impl Fn(&Box2) -> Box2, y: &Ring) -> Vec<(std::ops::Range<usize>, std::ops::Range<usize>)> {
    let mut out = Vec::new();
    for (kx, bx) in x.runs.iter().enumerate() {
        let gx = widen(bx);
        if !gx.hits(&y.bbox) {
            continue;
        }
        for (ky, by) in y.runs.iter().enumerate() {
            if gx.hits(by) {
                out.push((x.run(kx), y.run(ky)));
            }
        }
    }
    out
}

/// A region: the outer ring first, then holes.
#[derive(Clone, Debug)]
struct Shape {
    rings: Vec<Ring>,
    bbox: Box2,
}

impl Shape {
    fn new(rings: Vec<Vec<P>>) -> Shape {
        let rings: Vec<Ring> = rings
            .into_iter()
            .enumerate()
            .map(|(k, pts)| {
                let ccw = ring_area(&pts) > 0.0;
                Ring::new(pts, if (k == 0) == ccw { 1.0 } else { -1.0 })
            })
            .collect();
        let bbox = rings.iter().fold(Box2::EMPTY, |b, r| b.union(r.bbox));
        Shape { rings, bbox }
    }

    fn moved(&self, by: P) -> Shape {
        Shape { rings: self.rings.iter().map(|r| r.moved(by)).collect(), bbox: self.bbox.shift(by) }
    }

    fn outer(&self) -> &[Ring] {
        self.rings.get(..1).unwrap_or(&[])
    }

    fn corners(&self) -> usize {
        self.rings.iter().map(|r| r.pts.len()).sum()
    }
}

/// Even-odd test: whether `q` is in the material of `rings` (holes included).
fn in_material(q: P, rings: &[Ring]) -> bool {
    let mut c = false;
    let ray = Box2 { min: q, max: P::new(f64::INFINITY, q.y) };
    for r in rings {
        if !r.bbox.hits(&ray) {
            continue;
        }
        for (a, b) in r.edges_near(&ray) {
            if (a.y > q.y) != (b.y > q.y) && q.x < (b.x - a.x) * (q.y - a.y) / (b.y - a.y) + a.x {
                c = !c;
            }
        }
    }
    c
}

/// A validated part or sheet, moved so its box starts at (0, 0).
struct Region {
    shape: Shape,
    /// Where the box started.
    min: P,
    area: f64,
}

impl Region {
    fn new(shape: &Shape2, what: &str) -> Result<Region, ArrangeError> {
        let mut raw: Vec<Vec<P>> = Vec::with_capacity(1 + shape.holes.len());
        let mut count = 0usize;
        for ring in std::iter::once(&shape.outer).chain(&shape.holes) {
            let mut r: Vec<P> = Vec::with_capacity(ring.len());
            for v in ring {
                if !(v.x.abs() <= MAX_COORD && v.y.abs() <= MAX_COORD) {
                    return Err(ArrangeError::NotFinite { what: what.into() });
                }
                let p = P::new(v.x, v.y);
                if r.last() != Some(&p) {
                    r.push(p);
                }
            }
            if r.len() > 1 && r.first() == r.last() {
                r.pop();
            }
            count += r.len();
            if count > MAX_CORNERS {
                return Err(ArrangeError::TooManyCorners { what: what.into() });
            }
            if r.len() < 3 {
                return Err(ArrangeError::TooFewCorners { what: what.into() });
            }
            raw.push(r);
        }
        let bbox = Box2::of(raw.iter().flatten());
        let rings: Vec<Vec<P>> = raw.iter().map(|r| r.iter().map(|p| p.sub(bbox.min)).collect()).collect();
        let size = bbox.diag();
        let tol = 1e-9 * size;
        if rings.iter().any(|r| ring_area(r).abs() <= tol * size) {
            return Err(ArrangeError::NoArea { what: what.into() });
        }
        if any_contact(&rings, tol) {
            return Err(ArrangeError::SelfCrossing { what: what.into() });
        }
        // With no contacts between rings, one corner tells which side of another ring a whole ring
        // is on.
        let shape = Shape::new(rings);
        if let Some((outer, holes)) = shape.rings.split_first() {
            for (i, h) in holes.iter().enumerate() {
                let Some(&q) = h.pts.first() else { continue };
                if !in_material(q, std::slice::from_ref(outer)) {
                    return Err(ArrangeError::HoleOutside { what: what.into() });
                }
                if holes.iter().enumerate().any(|(j, o)| j != i && in_material(q, std::slice::from_ref(o))) {
                    return Err(ArrangeError::HolesNested { what: what.into() });
                }
            }
        }
        let mut areas = shape.rings.iter().map(|r| ring_area(&r.pts).abs());
        let area = areas.next().unwrap_or(0.0) - areas.sum::<f64>();
        Ok(Region { shape, min: bbox.min, area })
    }
}

/// Whether `a` and `b`, where they stand, keep at least `clear` apart: the material of neither
/// meets the other's, and no corner of one comes nearer than `clear` to an edge of the other. A
/// shape [`arrange`] would not take is never apart.
pub fn apart(a: &Shape2, b: &Shape2, clear: f64) -> bool {
    let (Ok(ra), Ok(rb)) = (Region::new(a, "a shape"), Region::new(b, "a shape")) else { return false };
    let (sa, sb) = (ra.shape.moved(ra.min), rb.shape.moved(rb.min));
    let tol = 1e-9 * sa.bbox.union(sb.bbox).diag();
    !conflict(&sa.rings, &sb.rings, false, tol) && !conflict(&sb.rings, &sa.rings, false, tol) && clear_of(&sa, &sb, clear.max(0.0), tol)
}

/// Whether `ring` is a region outline [`arrange`] takes: at least three corners, an area, and no
/// edge meeting another but at the corners neighbours share.
pub(crate) fn is_simple(ring: &[Vec2]) -> bool {
    Region::new(&Shape2 { outer: ring.to_vec(), holes: Vec::new() }, "ring").is_ok()
}

/// Whether any two edges of `rings` meet other than at the corner two neighbouring edges share,
/// or two neighbours fold back along each other.
fn any_contact(rings: &[Vec<P>], tol: f64) -> bool {
    struct E {
        ring: usize,
        at: usize,
        n: usize,
        a: P,
        b: P,
        bbox: Box2,
    }
    let mut all: Vec<E> = rings
        .iter()
        .enumerate()
        .flat_map(|(ring, r)| edges(r).enumerate().map(move |(at, (a, b))| E { ring, at, n: r.len(), a, b, bbox: Box2::seg(a, b).grow(tol) }))
        .collect();
    all.sort_by(|p, q| p.bbox.min.x.total_cmp(&q.bbox.min.x));
    for (i, e) in all.iter().enumerate() {
        for f in all.iter().skip(i + 1) {
            if f.bbox.min.x > e.bbox.max.x {
                break;
            }
            if !e.bbox.hits(&f.bbox) {
                continue;
            }
            let next = |x: &E, y: &E| x.ring == y.ring && (x.at + 1) % x.n == y.at;
            let shared = if next(e, f) {
                Some((e.b, e.a, f.b))
            } else if next(f, e) {
                Some((f.b, f.a, e.b))
            } else {
                None
            };
            match shared {
                Some((s, x, y)) => {
                    let (u, v) = (x.sub(s), y.sub(s));
                    if u.dot(v) > 0.0 && u.cross(v).abs() <= tol * u.len().max(v.len()) {
                        return true;
                    }
                }
                None => {
                    if seg_seg_dist(e.a, e.b, f.a, f.b) <= tol {
                        return true;
                    }
                }
            }
        }
    }
    false
}

// ---------- overlap and clearance ----------

/// Whether the inside of `a` meets the material of `b` (or, with `b_out`, the outside of `b`:
/// then `a` is not within `b`).
fn conflict(a: &[Ring], b: &[Ring], b_out: bool, tol: f64) -> bool {
    let ba = a.iter().fold(Box2::EMPTY, |x, r| x.union(r.bbox)).grow(tol);
    let bb = b.iter().fold(Box2::EMPTY, |x, r| x.union(r.bbox)).grow(tol);
    if !b_out && !ba.hits(&bb) {
        return false;
    }
    let flip = if b_out { -1.0 } else { 1.0 };
    let near_b = if b_out { Box2::ALL } else { bb };
    for ra in a {
        for (p, q) in ra.edges_near(&near_b) {
            if (b_out || Box2::seg(p, q).hits(&bb)) && boundary_meets(p, q, Some(ra.side), b, flip, tol) {
                return true;
            }
        }
    }
    for rb in b {
        for (p, q) in rb.edges_near(&ba) {
            if Box2::seg(p, q).hits(&ba) && boundary_meets(p, q, None, a, 1.0, tol) {
                return true;
            }
        }
    }
    false
}

/// Cuts edge p-q wherever the boundary of `other` meets it and tests each piece: a piece inside
/// the material of `other` (taken inverted when `flip` is -1) means the regions overlap; so does a
/// piece along `other`'s boundary with the material of both on the same side, when this edge's
/// material side `side` is given. A proper crossing always means overlap.
fn boundary_meets(p: P, q: P, side: Option<f64>, other: &[Ring], flip: f64, tol: f64) -> bool {
    let e = q.sub(p);
    let len = e.len();
    if len <= 4.0 * tol {
        return false;
    }
    let u = e.scale(1.0 / len);
    let eb = Box2::seg(p, q).grow(tol);
    let mut cuts = vec![0.0, len];
    for r in other {
        if !r.bbox.grow(tol).hits(&eb) {
            continue;
        }
        for (c, d) in r.edges_near(&eb) {
            if !Box2::seg(c, d).hits(&eb) {
                continue;
            }
            if crosses(p, q, c, d, tol) {
                return true;
            }
            for w in [c, d] {
                if seg_dist(w, p, q) <= tol {
                    cuts.push(w.sub(p).dot(u).clamp(0.0, len));
                }
            }
        }
    }
    cuts.sort_by(f64::total_cmp);
    for (t0, t1) in cuts.iter().zip(cuts.iter().skip(1)) {
        if t1 - t0 <= 4.0 * tol {
            continue;
        }
        let m = p.add(u.scale((t0 + t1) / 2.0));
        match on_boundary(m, other, tol) {
            Some(n) => {
                if let Some(s) = side
                    && u.left().scale(s).dot(n.scale(flip)) > 0.0
                {
                    return true;
                }
            }
            None => {
                if in_material(m, other) == (flip > 0.0) {
                    return true;
                }
            }
        }
    }
    false
}

/// The normal towards the material of the edge of `rings` that `m` lies on, if any.
fn on_boundary(m: P, rings: &[Ring], tol: f64) -> Option<P> {
    for r in rings {
        let at = Box2::at(m).grow(tol);
        if !r.bbox.hits(&at) {
            continue;
        }
        for (c, d) in r.edges_near(&at) {
            if seg_dist(m, c, d) <= tol {
                let e = d.sub(c);
                let l = e.len();
                if l > 0.0 {
                    return Some(e.scale(r.side / l).left());
                }
            }
        }
    }
    None
}

/// Whether every corner of each of `a` and `b` is at least `clear` from every edge of the other.
fn clear_of(a: &Shape, b: &Shape, clear: f64, tol: f64) -> bool {
    if clear <= tol || !a.bbox.grow(clear).hits(&b.bbox) {
        return true;
    }
    let need = clear - 1e-3 * tol;
    let close = |v: P, (c, d): (P, P)| seg_dist(v, c, d) < need;
    for ra in &a.rings {
        for rb in &b.rings {
            for (xa, xb) in run_pairs(ra, |r| r.grow(clear), rb) {
                for i in xa.clone() {
                    for j in xb.clone() {
                        if close(ra.pt(i), rb.edge(j)) || close(rb.pt(j), ra.edge(i)) {
                            return false;
                        }
                    }
                }
            }
        }
    }
    true
}

// ---------- moving until contact ----------

/// How far `part` can move along unit `dir`, up to `reach`, before a corner of it enters the band
/// of width `clear` around an edge of `other`, or a corner of `other` enters the band around an
/// edge of `part` (infinity when nothing is met within `reach`). With `out`, `other` is the sheet
/// and the part must stay in its material; otherwise out of it.
fn sweep(part: &Shape, other: &Shape, dir: P, clear: f64, tol: f64, reach: f64, out: bool) -> f64 {
    let back = dir.scale(-1.0);
    let flip = if out { -1.0 } else { 1.0 };
    let path = dir.scale(reach);
    let widen = |b: &Box2| b.union(b.shift(path)).grow(clear + tol);
    let mut t = f64::INFINITY;
    for ro in &other.rings {
        let bad = ro.side * flip;
        for rp in &part.rings {
            for (xp, xo) in run_pairs(rp, widen, ro) {
                for i in xo.clone() {
                    let (c, d) = ro.edge(i);
                    let (prev, w, next) = ro.corner(i);
                    let obstacle = Wedge::at(prev, w, next, bad);
                    for j in xp.clone() {
                        t = t.min(enter_capsule(rp.pt(j), dir, c, d, clear, tol, bad));
                        let (a, b) = rp.edge(j);
                        t = t.min(enter_capsule(w, back, a, b, clear, tol, rp.side));
                        // With no band, a corner that meets a corner (now or on the way) stops
                        // there when the materials at the two would overlap past it.
                        if clear <= tol {
                            let (pp, v, pn) = rp.corner(j);
                            let rel = w.sub(v);
                            let at = rel.dot(dir);
                            if at >= -tol && at < t && dir.cross(rel).abs() <= tol && corners_meet(dir, obstacle, Wedge::at(pp, v, pn, rp.side)) {
                                t = at.max(0.0);
                            }
                        }
                    }
                }
            }
        }
    }
    t
}

/// The first `t >= 0` at which `o + t·d` (d of unit length) enters the set of points within `r`
/// of the segment a-b; infinity if it never does. A ray that starts on the band's edge and moves
/// along it or away from it is not stopped. With no band (`r` within `tol` of 0), a ray that starts
/// inside the segment stops when it moves to the side `bad` (+1 left of a-b, -1 right), and one
/// that crosses the segment stops where it does; meeting an end of the segment is left to the
/// corner test.
fn enter_capsule(o: P, d: P, a: P, b: P, r: f64, tol: f64, bad: f64) -> f64 {
    let ab = b.sub(a);
    let len = ab.len();
    if len == 0.0 {
        return f64::INFINITY;
    }
    let u_dir = ab.scale(1.0 / len);
    let w_dir = u_dir.left();
    let rel = o.sub(a);
    let (ou, ow, du, dw) = (rel.dot(u_dir), rel.dot(w_dir), d.dot(u_dir), d.dot(w_dir));
    if r <= tol {
        if ow.abs() <= tol {
            return if ou > tol && ou < len - tol && dw * bad > 1e-12 { 0.0 } else { f64::INFINITY };
        }
        if dw.abs() < 1e-12 {
            return f64::INFINITY;
        }
        let t = -ow / dw;
        let u = ou + du * t;
        return if t > 0.0 && u > tol && u < len - tol { t } else { f64::INFINITY };
    }
    enter_disk(o, d, a, r, tol).min(enter_disk(o, d, b, r, tol)).min(enter_slab(ou, du, 0.0, len, ow, dw, r, tol))
}

fn enter_disk(o: P, d: P, c: P, r: f64, tol: f64) -> f64 {
    let oc = o.sub(c);
    let b = d.dot(oc);
    let disc = b * b - (oc.dot(oc) - r * r);
    if disc < 0.0 {
        return f64::INFINITY;
    }
    let sq = disc.sqrt();
    let (t1, t2) = (-b - sq, -b + sq);
    if t2 <= tol {
        f64::INFINITY
    } else if t1 >= -tol {
        t1.max(0.0)
    } else if b < -1e-9 * r {
        // Starts inside the disk and moves towards its centre.
        0.0
    } else {
        f64::INFINITY
    }
}

/// Entry into the rectangle u in [u0, u1], |w| <= r, along (du, dw) from (ou, ow).
#[allow(clippy::too_many_arguments)]
fn enter_slab(ou: f64, du: f64, u0: f64, u1: f64, ow: f64, dw: f64, r: f64, tol: f64) -> f64 {
    let span = |o: f64, d: f64, lo: f64, hi: f64| -> Option<(f64, f64)> {
        if d.abs() < 1e-12 {
            // Moving along this axis' edges: inside only when strictly between them.
            if o > lo + tol && o < hi - tol { Some((f64::NEG_INFINITY, f64::INFINITY)) } else { None }
        } else {
            let (a, b) = ((lo - o) / d, (hi - o) / d);
            Some((a.min(b), a.max(b)))
        }
    };
    let (Some((ua, ub)), Some((wa, wb))) = (span(ou, du, u0, u1), span(ow, dw, -r, r)) else { return f64::INFINITY };
    let (enter, exit) = (ua.max(wa), ub.min(wb));
    if exit <= tol || enter > exit {
        f64::INFINITY
    } else if enter >= -tol {
        enter.max(0.0)
    } else if dw * ow < 0.0 {
        // Starts inside the band and moves towards the segment.
        0.0
    } else {
        f64::INFINITY
    }
}

// ---------- parts at their rotations ----------

/// One rotation of a part, moved so its box starts at (0, 0).
struct Pose {
    angle: f64,
    /// A part whose box starts at `at` has `shift = at + base` (before the sheet's own move).
    base: P,
    size: P,
    shape: Shape,
    /// Discs inside the outline, where its box starts at (0, 0) (see [`poles`]).
    poles: Vec<Disc>,
}

/// A disc inside a part's material: centre and radius.
#[derive(Clone, Copy, Debug)]
struct Disc {
    c: P,
    r: f64,
}

/// Most discs a part gets for [`Ctx::fits`]'s quick rejection.
const POLES: usize = 8;

/// Most spots tried while looking for one disc: they only speed up a check, so a rough one does.
const MAX_POLE_TRIES: usize = 256;

/// Discs well inside `shape`'s material, largest first, each the largest that fits clear of the
/// edges and of the discs before it (to within a hundredth of the shape's size, or a twentieth of
/// its width when it is thin), until one would
/// be under a sixth of the first. Two parts whose discs come closer than their radii plus the
/// spacing surely come too close, so most spots that don't fit are told apart without looking at
/// a single edge.
fn poles(shape: &Shape) -> Vec<Disc> {
    let b = shape.bbox;
    let s = b.size();
    let size = s.x.max(s.y);
    if !(size.is_finite() && size > 0.0) {
        return Vec::new();
    }
    // Fine enough for a thin part to get its discs too.
    let prec = (size * 1e-2).min(s.x.min(s.y) * 0.05);
    let mut out: Vec<Disc> = Vec::new();
    // How far `q` is from leaving the material or entering a disc found already (negative
    // outside).
    let room = |q: P, out: &[Disc]| -> f64 {
        let mut d = f64::INFINITY;
        for r in &shape.rings {
            for (a, e) in edges(&r.pts) {
                d = d.min(seg_dist(q, a, e));
            }
        }
        for o in out {
            d = d.min(q.sub(o.c).len() - o.r);
        }
        if in_material(q, &shape.rings) { d } else { -d }
    };
    // A cell: its centre's room and how much more any point in it could have.
    #[derive(PartialEq)]
    struct Cell(f64, f64, P, f64);
    impl Eq for Cell {}
    impl PartialOrd for Cell {
        fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(o))
        }
    }
    impl Ord for Cell {
        fn cmp(&self, o: &Self) -> std::cmp::Ordering {
            (self.0 + self.1).total_cmp(&(o.0 + o.1))
        }
    }
    for _ in 0..POLES {
        // Cells no wider than the thin side (at most 64 along the long one), centred on the box so
        // a thin part's middle line is among the first centres looked at.
        let h = (s.x.min(s.y) / 2.0).max(size / 128.0);
        let (nx, ny) = ((s.x / (2.0 * h)).ceil().max(1.0) as usize, (s.y / (2.0 * h)).ceil().max(1.0) as usize);
        let mid = b.min.add(s.scale(0.5));
        let mut heap = std::collections::BinaryHeap::new();
        let mut best = Disc { c: mid, r: f64::NEG_INFINITY };
        for i in 0..nx {
            for j in 0..ny {
                let c = P::new(mid.x + (2.0 * i as f64 - (nx - 1) as f64) * h, mid.y + (2.0 * j as f64 - (ny - 1) as f64) * h);
                heap.push(Cell(room(c, &out), h * std::f64::consts::SQRT_2, c, h));
            }
        }
        let mut evals = 0usize;
        while let Some(Cell(d, up, c, h)) = heap.pop() {
            if d > best.r {
                best = Disc { c, r: d };
            }
            if d + up <= best.r + prec || evals > MAX_POLE_TRIES {
                break;
            }
            let h = h / 2.0;
            for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let q = c.add(P::new(dx * h, dy * h));
                heap.push(Cell(room(q, &out), h * std::f64::consts::SQRT_2, q, h));
                evals += 1;
            }
        }
        // A hair inside, so rounding never puts a disc's edge outside the material.
        let r = best.r - prec;
        if !(r.is_finite() && r > 0.0) || out.first().is_some_and(|f| r < f.r / 6.0) {
            break;
        }
        out.push(Disc { c: best.c, r });
    }
    out
}

struct Kind {
    poses: Vec<Pose>,
    area: f64,
}

/// Sine and cosine, exact at quarter turns.
fn sin_cos_deg(deg: f64) -> (f64, f64) {
    match deg {
        0.0 => (0.0, 1.0),
        90.0 => (1.0, 0.0),
        180.0 => (0.0, -1.0),
        270.0 => (-1.0, 0.0),
        _ => deg.to_radians().sin_cos(),
    }
}

impl Kind {
    fn new(region: &Region, rotations_deg: &[f64], method: ArrangeMethod) -> Kind {
        let mut angles: Vec<f64> = rotations_deg.iter().map(|a| a.rem_euclid(360.0)).map(|a| if a > 360.0 - 1e-9 { 0.0 } else { a }).collect();
        if angles.is_empty() {
            angles.push(0.0);
        }
        angles.sort_by(f64::total_cmp);
        angles.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        let discs = poles(&region.shape);
        let poses = angles
            .into_iter()
            .map(|deg| {
                let (s, c) = sin_cos_deg(deg);
                let rot = |p: P| P::new(p.x * c - p.y * s, p.x * s + p.y * c);
                let turned: Vec<Vec<P>> = region.shape.rings.iter().map(|r| r.pts.iter().map(|&p| rot(p)).collect()).collect();
                let b = Box2::of(turned.iter().flatten());
                let rings = match method {
                    ArrangeMethod::TrueShape => turned.iter().map(|r| r.iter().map(|p| p.sub(b.min)).collect()).collect(),
                    ArrangeMethod::Rectangular => {
                        let s = b.size();
                        vec![vec![P::new(0.0, 0.0), P::new(s.x, 0.0), P::new(s.x, s.y), P::new(0.0, s.y)]]
                    }
                };
                // A point p of the caller's shape is rot(p - min) - b.min + at.
                let poles = discs.iter().map(|d| Disc { c: rot(d.c).sub(b.min), r: d.r }).collect();
                Pose { angle: deg.to_radians(), base: rot(region.min).add(b.min).scale(-1.0), size: b.size(), shape: Shape::new(rings), poles }
            })
            .collect();
        Kind { poses, area: region.area }
    }
}

// ---------- a grid of placed parts ----------

/// Which placed parts' boxes reach each cell of a grid over the sheet.
struct Grid {
    min: P,
    cell: P,
    cells: Vec<Vec<usize>>,
}

const GRID: usize = 48;

/// How many fitting starts [`Ctx::best_spot`] settles before taking the best.
const SETTLE_TRIES: usize = 8;

/// Most start positions one part's search keeps: the best by where they would end. Detailed
/// outlines at fine turns would otherwise make millions (and as many bytes times forty).
const MAX_STARTS: usize = 1 << 17;

impl Grid {
    fn new(over: Box2) -> Grid {
        let s = over.size();
        let cell = P::new((s.x / GRID as f64).max(f64::MIN_POSITIVE), (s.y / GRID as f64).max(f64::MIN_POSITIVE));
        Grid { min: over.min, cell, cells: vec![Vec::new(); GRID * GRID] }
    }

    fn span(&self, b: &Box2) -> (usize, usize, usize, usize) {
        let at = |v: f64, lo: f64, c: f64| ((v - lo) / c).floor().clamp(0.0, (GRID - 1) as f64) as usize;
        (
            at(b.min.x, self.min.x, self.cell.x),
            at(b.max.x, self.min.x, self.cell.x),
            at(b.min.y, self.min.y, self.cell.y),
            at(b.max.y, self.min.y, self.cell.y),
        )
    }

    fn insert(&mut self, i: usize, b: &Box2) {
        let (x0, x1, y0, y1) = self.span(b);
        for y in y0..=y1 {
            for x in x0..=x1 {
                if let Some(c) = self.cells.get_mut(y * GRID + x) {
                    c.push(i);
                }
            }
        }
    }

    fn query(&self, b: &Box2) -> Vec<usize> {
        let (x0, x1, y0, y1) = self.span(b);
        let mut out = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                if let Some(c) = self.cells.get(y * GRID + x) {
                    out.extend_from_slice(c);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

// ---------- laying parts out ----------

struct Ctx<'a> {
    /// Parts standing on the sheet before any is laid; first in every layout.
    fixed: &'a [Laid],
    sheet: &'a Shape,
    kinds: &'a [Kind],
    spacing: f64,
    margin: f64,
    part_in_part: bool,
    tol: f64,
    cancel: &'a dyn Fn() -> bool,
    /// Each kind's turns by their boxes, taken when [`Hurry::Boxes`].
    boxes: &'a [Kind],
    hurry: &'a dyn Fn() -> Hurry,
    /// The most `hurry` has said so far.
    hurried: std::cell::Cell<Hurry>,
}

/// A part on the sheet.
#[derive(Clone, Debug)]
struct Laid {
    kind: usize,
    pose: usize,
    at: P,
    shape: Shape,
    /// Its discs where it stands.
    poles: Vec<Disc>,
}

struct Layout {
    parts: Vec<Laid>,
    /// For each entry of the order: how many parts were laid after it, and whether it found no
    /// place.
    after: Vec<usize>,
    failed: Vec<bool>,
    area: f64,
    top: f64,
    right: f64,
}

/// How much better a layout must be to count.
struct Better {
    area: f64,
    length: f64,
}

impl Better {
    /// More area placed, then lower, then narrower.
    fn is(&self, a: &Layout, b: &Layout) -> bool {
        if (a.area - b.area).abs() > self.area {
            return a.area > b.area;
        }
        if (a.top - b.top).abs() > self.length {
            return a.top < b.top;
        }
        a.right < b.right - self.length
    }
}

impl Ctx<'_> {
    fn cancelled(&self) -> Result<(), ArrangeError> {
        if (self.cancel)() { Err(ArrangeError::Cancelled) } else { Ok(()) }
    }

    /// Lays out the parts in `order`; with `base`, reuses its layout of the first `from` entries
    /// (the same as in `order`).
    fn lay(&self, order: &[Gene], base: Option<(&Layout, usize)>) -> Result<Layout, ArrangeError> {
        let from = base.map_or(0, |(_, f)| f);
        let (mut laid, mut after, mut failed) = match base {
            Some((b, f)) => {
                let keep = f.checked_sub(1).and_then(|i| b.after.get(i)).copied().unwrap_or(self.fixed.len());
                (
                    b.parts.get(..keep).map(<[Laid]>::to_vec).unwrap_or_default(),
                    b.after.get(..f).map(<[usize]>::to_vec).unwrap_or_default(),
                    b.failed.get(..f).map(<[bool]>::to_vec).unwrap_or_default(),
                )
            }
            None => (self.fixed.to_vec(), Vec::new(), Vec::new()),
        };
        // A kind that found no place at any turn is not tried again: the sheet only fills up.
        // (New parts do bring new starts beside them, so this can miss a late fit; it keeps a
        // full sheet fast.)
        let mut no_room = vec![false; self.kinds.len()];
        for (g, f) in order.iter().zip(&failed) {
            if *f
                && g.pose.is_none()
                && let Some(n) = no_room.get_mut(g.kind)
            {
                *n = true;
            }
        }
        let mut grid = Grid::new(self.sheet.bbox);
        let mut area = 0.0;
        for (i, l) in laid.iter().enumerate() {
            grid.insert(i, &l.shape.bbox);
            area += kinds_area(self.kinds, l.kind);
        }
        for &Gene { kind: k, pose: lock } in order.iter().skip(from) {
            self.cancelled()?;
            let hurry = (self.hurry)();
            self.hurried.set(self.hurried.get().max(hurry));
            if hurry >= Hurry::Stop {
                after.push(laid.len());
                failed.push(true);
                continue;
            }
            let mut fail = false;
            let kinds = if hurry >= Hurry::Boxes { self.boxes } else { self.kinds };
            if let (Some(kind), Some(full)) = (kinds.get(k), no_room.get_mut(k))
                && !*full
            {
                match self.best_spot(k, kind, lock, &laid, &grid, hurry >= Hurry::Quick)? {
                    Some(l) => {
                        area += kind.area;
                        grid.insert(laid.len(), &l.shape.bbox);
                        laid.push(l);
                    }
                    None => {
                        *full |= lock.is_none();
                        fail = true;
                    }
                }
            }
            after.push(laid.len());
            failed.push(fail);
        }
        let own = laid.get(self.fixed.len()..).unwrap_or_default();
        let used = own.iter().fold(Box2::EMPTY, |b, l| b.union(l.shape.bbox));
        let (top, right) = if own.is_empty() { (0.0, 0.0) } else { (used.max.y, used.max.x) };
        Ok(Layout { parts: laid, after, failed, area, top, right })
    }

    /// The best spot for a part of kind `k`, at any of its turns or only at `lock`.
    /// `quick`: no corner-contact starts beside placed parts, and the first spot that settles wins.
    fn best_spot(
        &self,
        k: usize,
        kind: &Kind,
        lock: Option<usize>,
        laid: &[Laid],
        grid: &Grid,
        mut quick: bool,
    ) -> Result<Option<Laid>, ArrangeError> {
        // The budget is asked as the search goes too, so one detailed part can't overrun it.
        let behind = || {
            let h = (self.hurry)();
            self.hurried.set(self.hurried.get().max(h));
            h
        };
        // Every start of every rotation, best first by where its box would end (lowest top, then
        // leftmost). Settling only moves a part down or left, so the first few that fit hold the
        // best spots; later ones only get looked at when those fail.
        let mut cands: Vec<(f64, f64, usize, P)> = Vec::new();
        let order = |a: &(f64, f64, usize, P), b: &(f64, f64, usize, P)| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2));
        for (pi, pose) in kind.poses.iter().enumerate().filter(|(pi, _)| lock.is_none_or(|l| l == *pi)) {
            self.cancelled()?;
            if behind() >= Hurry::Stop {
                return Ok(None);
            }
            cands.extend(self.starts(pose, laid, !quick).into_iter().map(|at| (at.y + pose.size.y, at.x + pose.size.x, pi, at)));
            if cands.len() > 2 * MAX_STARTS {
                cands.select_nth_unstable_by(MAX_STARTS, order);
                cands.truncate(MAX_STARTS);
            }
        }
        cands.sort_by(order);
        let mut best: Option<(P, usize, Box2)> = None;
        let q = self.tol * 1e3;
        let mut found = 0;
        for (ci, &(top, right, pi, start)) in cands.iter().enumerate() {
            if ci % 64 == 0 {
                self.cancelled()?;
                match behind() {
                    Hurry::Stop => break,
                    Hurry::No => {}
                    _ => quick = true,
                }
            }
            if found >= if quick { 1 } else { SETTLE_TRIES } {
                break;
            }
            // Nothing further down the list can beat a settled spot below its start.
            if let Some((_, _, o)) = &best
                && (top > o.max.y + q || (top > o.max.y - q && right >= o.max.x - q))
                && found > 0
            {
                break;
            }
            let Some(pose) = kind.poses.get(pi) else { continue };
            if !self.fits(pose, start, laid, grid) {
                continue;
            }
            let at = self.settle(pose, start, laid, grid);
            // Settling only moves through free space, but a check after it keeps a numerical
            // slip from ever letting an overlap through.
            if at != start && !self.fits(pose, at, laid, grid) {
                continue;
            }
            found += 1;
            let b = Box2 { min: at, max: at.add(pose.size) };
            let better = match &best {
                None => true,
                Some((_, _, o)) => {
                    if (b.max.y - o.max.y).abs() > q {
                        b.max.y < o.max.y
                    } else {
                        b.max.x < o.max.x - q
                    }
                }
            };
            if better {
                best = Some((at, pi, b));
            }
        }
        Ok(best.and_then(|(at, pi, _)| {
            let pose = kind.poses.get(pi)?;
            Some(Laid {
                kind: k,
                pose: pi,
                at,
                shape: pose.shape.moved(at),
                poles: pose.poles.iter().map(|d| Disc { c: d.c.add(at), r: d.r }).collect(),
            })
        }))
    }

    /// Positions where a corner of `pose`'s outline sits on a corner of `ring` (pushed out from it
    /// by `gap` along the corner's outward bisector): the tight spots between parts, which sliding
    /// down and left alone would miss.
    fn contacts(&self, pose: &Pose, ring: &Ring, side: f64, gap: f64, out: &mut Vec<P>) {
        let Some(outline) = pose.shape.rings.first() else { return };
        let own: Vec<(P, Wedge)> = (0..outline.pts.len())
            .map(|j| {
                let (pp, v, pn) = outline.corner(j);
                (v, Wedge::at(pp, v, pn, outline.side))
            })
            .collect();
        // Pushed a hair past the gap so rounding never leaves it short.
        let push = if gap > self.tol { gap + self.tol } else { 0.0 };
        for i in 0..ring.pts.len() {
            if out.len() >= 2 * MAX_STARTS {
                return;
            }
            let (prev, w, next) = ring.corner(i);
            let wedge = Wedge::at(prev, w, next, side);
            let mid = wedge.start + wedge.width / 2.0;
            let at = w.add(P::new(-mid.cos(), -mid.sin()).scale(push));
            for (v, mine) in &own {
                // Corners whose materials would overlap where they meet cannot touch there.
                if !wedge.meets(*mine) {
                    out.push(at.sub(*v));
                }
            }
        }
    }

    /// Start positions for the box corner of `pose`: along the sheet's top and its corners; above,
    /// beside and on top of placed parts near the top of the pile; beside every placed part, in
    /// their holes; and a coarse grid for sheets that are not upright rectangles.
    /// Without `contacts`, no starts where its corners meet placed parts' corners: they are the
    /// most numerous.
    fn starts(&self, pose: &Pose, laid: &[Laid], contacts: bool) -> Vec<P> {
        let sb = self.sheet.bbox;
        let (m, s, w, h) = (self.margin, self.spacing, pose.size.x, pose.size.y);
        let (lo, hi) = (P::new(sb.min.x + m, sb.min.y + m), P::new(sb.max.x - m - w, sb.max.y - m - h));
        if lo.x > hi.x + self.tol || lo.y > hi.y + self.tol {
            return Vec::new();
        }
        let top = hi.y;
        let mut out = Vec::new();
        let n = 16;
        for i in 0..=n {
            out.push(P::new(lo.x + (hi.x - lo.x) * i as f64 / n as f64, top));
        }
        for r in self.sheet.outer() {
            for c in &r.pts {
                out.extend([P::new(c.x, c.y), P::new(c.x - w, c.y), P::new(c.x, c.y - h), P::new(c.x - w, c.y - h)]);
            }
        }
        for r in &self.sheet.rings {
            self.contacts(pose, r, -r.side, m, &mut out);
        }
        let pile_top = laid.iter().fold(f64::NEG_INFINITY, |t, l| t.max(l.shape.bbox.max.y));
        for l in laid {
            let b = l.shape.bbox;
            // Dropping from the top and corner contacts only help beside the parts that are not
            // buried (and in holes).
            if b.max.y >= pile_top - 2.0 * h - s {
                out.extend([P::new(b.max.x + s, top), P::new(b.min.x, top), P::new(b.min.x - s - w, top)]);
                for r in l.shape.outer().iter().filter(|_| contacts) {
                    self.contacts(pose, r, r.side, s, &mut out);
                }
            }
            if self.part_in_part && contacts {
                for r in l.shape.rings.iter().skip(1) {
                    if r.bbox.size().x >= w && r.bbox.size().y >= h {
                        self.contacts(pose, r, r.side, s, &mut out);
                    }
                }
            }
            out.extend([
                P::new(b.max.x + s, b.min.y),
                P::new(b.max.x + s, (b.min.y + b.max.y - h) / 2.0),
                P::new(b.max.x + s, b.max.y - h),
                P::new(b.min.x, b.max.y + s),
                P::new(b.max.x - w, b.max.y + s),
            ]);
            if self.part_in_part {
                for hole in l.shape.rings.iter().skip(1) {
                    let hb = hole.bbox;
                    let free = hb.size().sub(pose.size).sub(P::new(2.0 * s, 2.0 * s));
                    if free.x < -self.tol || free.y < -self.tol {
                        continue;
                    }
                    for (fx, fy) in [(0.0, 0.0), (0.5, 0.0), (1.0, 0.0), (0.0, 0.5), (0.5, 0.5), (1.0, 0.5), (0.0, 1.0), (0.5, 1.0), (1.0, 1.0)] {
                        out.push(P::new(hb.min.x + s + fx * free.x.max(0.0), hb.min.y + s + fy * free.y.max(0.0)));
                    }
                }
            }
        }
        let upright = self.sheet.rings.len() == 1
            && self
                .sheet
                .outer()
                .iter()
                .all(|r| r.pts.len() == 4 && r.pts.iter().all(|p| (p.x == sb.min.x || p.x == sb.max.x) && (p.y == sb.min.y || p.y == sb.max.y)));
        if !upright {
            let n = 10;
            for i in 0..=n {
                for j in 0..=n {
                    out.push(P::new(lo.x + (hi.x - lo.x) * i as f64 / n as f64, lo.y + (hi.y - lo.y) * j as f64 / n as f64));
                }
            }
        }
        let q = self.tol * 1e3;
        out.retain(|p| p.x >= lo.x - self.tol && p.x <= hi.x + self.tol && p.y >= lo.y - self.tol && p.y <= hi.y + self.tol);
        out.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
        out.dedup_by(|a, b| (a.x - b.x).abs() < q && (a.y - b.y).abs() < q);
        out
    }

    /// Whether `pose` at `at` stays in the sheet's material by the margin and apart from every
    /// placed part by the spacing.
    fn fits(&self, pose: &Pose, at: P, laid: &[Laid], grid: &Grid) -> bool {
        let (tol, m, sb) = (self.tol, self.margin, self.sheet.bbox);
        let b = Box2 { min: at, max: at.add(pose.size) };
        if b.min.x < sb.min.x + m - tol || b.min.y < sb.min.y + m - tol || b.max.x > sb.max.x - m + tol || b.max.y > sb.max.y - m + tol {
            return false;
        }
        let near: Vec<&Laid> = grid
            .query(&b.grow(self.spacing + tol))
            .into_iter()
            .filter_map(|i| laid.get(i))
            .filter(|l| b.grow(self.spacing + tol).hits(&l.shape.bbox))
            .collect();
        // Discs inside both coming too close: surely too close, with no edge looked at.
        for l in &near {
            for d in &pose.poles {
                let c = d.c.add(at);
                if l.poles.iter().any(|e| c.sub(e.c).len() + tol < d.r + e.r + self.spacing) {
                    return false;
                }
            }
        }
        let part = pose.shape.moved(at);
        // On a plain rectangle the box check above is the whole check.
        let plain = matches!(self.sheet.rings.as_slice(), [r] if r.pts.len() == 4 && r.pts.iter().all(|p| (p.x == sb.min.x || p.x == sb.max.x) && (p.y == sb.min.y || p.y == sb.max.y)));
        if !plain && (conflict(&part.rings, &self.sheet.rings, true, tol) || !clear_of(&part, self.sheet, m, tol)) {
            return false;
        }
        for l in near {
            if conflict(&part.rings, &l.shape.rings, false, tol) || !clear_of(&part, &l.shape, self.spacing, tol) {
                return false;
            }
            // Without part-in-part, neither may sit in the other's hole.
            if !self.part_in_part && conflict(part.outer(), l.shape.outer(), false, tol) {
                return false;
            }
        }
        true
    }

    /// Falls down and slides left from `start` (a fitting position) until it can move neither way.
    fn settle(&self, pose: &Pose, start: P, laid: &[Laid], grid: &Grid) -> P {
        let mut at = start;
        for _ in 0..64 {
            let mut moved = 0.0;
            for dir in [P::new(0.0, -1.0), P::new(-1.0, 0.0)] {
                // Short of a band by a hair, so rounding never leaves a gap below the spacing.
                let t = self.travel(pose, at, dir, laid, grid) - if self.spacing > self.tol || self.margin > self.tol { self.tol } else { 0.0 };
                if t.is_finite() && t > self.tol {
                    at = at.add(dir.scale(t));
                    moved += t;
                }
            }
            if moved <= self.tol * 10.0 {
                break;
            }
        }
        at
    }

    /// How far `pose` at `at` can move along `dir` (straight down or left) before it comes within
    /// the spacing of a placed part or the margin of the sheet's edge.
    fn travel(&self, pose: &Pose, at: P, dir: P, laid: &[Laid], grid: &Grid) -> f64 {
        let part = pose.shape.moved(at);
        let pb = part.bbox;
        let sb = self.sheet.bbox;
        let reach = if dir.y < 0.0 { pb.min.y - sb.min.y } else { pb.min.x - sb.min.x }.max(0.0);
        let swept = pb.union(pb.shift(dir.scale(reach)));
        let mut t = sweep(&part, self.sheet, dir, self.margin, self.tol, reach, true);
        // Nearest obstacles first, so the search stops once the next one is farther than `t`.
        let ahead = |o: &Box2| if dir.y < 0.0 { pb.min.y - o.max.y } else { pb.min.x - o.max.x };
        let mut near: Vec<(f64, usize)> = grid
            .query(&swept.grow(self.spacing + self.tol))
            .into_iter()
            .filter_map(|i| Some((ahead(&laid.get(i)?.shape.bbox) - self.spacing, i)))
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for (gap, i) in near {
            if gap > t + self.tol {
                break;
            }
            if let Some(l) = laid.get(i) {
                t = t.min(sweep(&part, &l.shape, dir, self.spacing, self.tol, t.min(reach) + self.tol, false));
            }
        }
        t
    }
}

/// One entry of the order the parts are laid in: the part's kind and, when the search holds it to
/// one, the turn it must take.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Gene {
    kind: usize,
    pose: Option<usize>,
}

/// A small deterministic generator for the order search.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }

    /// A different order, by swapping two entries, moving one, or holding one part to one of its
    /// `poses(kind)` turns (or freeing it again), and the first index where it differs; none
    /// when nothing can change. Holding a turn reaches layouts the greedy choice of the lowest
    /// spot never makes: a part stood on end in a row with its likes rather than laid flat.
    fn reorder(&mut self, order: &[Gene], poses: impl Fn(usize) -> usize) -> Option<(Vec<Gene>, usize)> {
        if order.is_empty() {
            return None;
        }
        for _ in 0..32 {
            let mut next = order.to_vec();
            let (a, b) = (self.below(next.len()), self.below(next.len()));
            match self.below(3) {
                0 => next.swap(a, b),
                1 => {
                    let k = next.remove(a);
                    next.insert(b.min(next.len()), k);
                }
                _ => {
                    if let Some(g) = next.get_mut(a) {
                        let n = poses(g.kind);
                        if n > 1 {
                            let pick = self.below(n + 1);
                            g.pose = if pick == n { None } else { Some(pick) };
                        }
                    }
                }
            }
            if let Some(from) = next.iter().zip(order).position(|(x, y)| x != y) {
                return Some((next, from));
            }
        }
        None
    }
}

#[path = "arrange_coarsen.rs"]
mod coarsen;
pub use coarsen::{COARSEN_BUDGET, coarsen};

#[cfg(test)]
#[path = "arrange_tests.rs"]
mod tests;

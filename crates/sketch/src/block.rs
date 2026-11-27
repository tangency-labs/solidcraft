//! Sketch blocks: sets of sketch curves that keep their shape relative to each other while the
//! set as a whole moves, turns and, with [`BlockScale::Free`], scales.
//!
//! A block's pose lives in the two points of its own construction line, the axis: its start is
//! the block origin O, its end X. Every member point P is held at its captured place in the
//! frame, `P = O + u·d + v·perp(d)` with `d = X − O`, and every member circle or ellipse radius
//! at its captured share of the axis length. The constraints wholly inside the block are kept
//! but not solved while it exists ([`SketchBlock::held`]).
//!
//! A block can be opened to edit its contents in place ([`Sketch::open_block`]): while it is open
//! its frame rows are left out of the solve, its held constraints drive, and everything outside
//! its curves, its axis included, is held where it is. Closing it ([`Sketch::close_block`])
//! captures the new shape into the same block, with the curves drawn while it was open as members.

use serde::{Deserialize, Serialize};
use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;
use std::collections::{HashMap, HashSet};

use crate::model::{Constraint, ConstraintKind, CurveKind, Result, Sketch, SketchError, in_range};

/// Whether a block keeps its size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockScale {
    /// The block keeps its size: its axis keeps the length it had when the shape was captured.
    /// A saved value this version doesn't know reads as this.
    #[default]
    Fixed,
    /// The block keeps its proportions; one length dimension sets its size.
    Free,
}

impl<'de> Deserialize<'de> for BlockScale {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Saved {
            Word(String),
            Other(serde::de::IgnoredAny),
        }
        Ok(match Saved::deserialize(d)? {
            Saved::Word(w) if w == "free" => BlockScale::Free,
            Saved::Word(_) | Saved::Other(_) => BlockScale::Fixed,
        })
    }
}

/// A set of sketch curves that keeps its shape (see the module documentation).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SketchBlock {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The member curves, by id.
    pub curves: Vec<String>,
    /// The id of the block's own construction line, from the block origin along its +x.
    pub axis: String,
    #[serde(default)]
    pub scale: BlockScale,
    /// The axis length when the shape was captured (L0, mm).
    pub length: f64,
    /// Each member point: its id and its place in the block's frame, (u, v) in units of L0.
    pub points: Vec<(String, [f64; 2])>,
    /// Each member circle or ellipse: its id and its radius at capture, in units of L0.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub radii: Vec<(String, f64)>,
    /// The constraints wholly inside the block, kept but not solved while it exists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub held: Vec<String>,
    /// The points that are members on their own, on none of its curves (drawn while it was
    /// open), by id; they are among [`SketchBlock::points`] too.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loose: Vec<String>,
    /// Whether the constraints and dimensions it holds are drawn while it is closed; hidden by
    /// default, leaving its curves, axis and glyph.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub show_held: bool,
    /// Set while the block is open for editing its contents ([`Sketch::open_block`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<BlockOpen>,
}

/// What a block open for editing its contents keeps until it closes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BlockOpen {
    /// The constraints wholly inside the block that drove when it was opened (a free block's
    /// size): they keep driving when it closes, and every other constraint then wholly inside
    /// it is held.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub driving: Vec<String>,
    /// The curves outside the block when it was opened, its axis among them: they stay where
    /// they are while it is open, and every other curve is a member when it closes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outside: Vec<String>,
    /// The points that were not the block's when it was opened, by id: they stay where they are
    /// while it is open, and every other point on no curve is a member when it closes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<String>,
    /// The block's own points when it was opened, by id: a point on no curve that is neither
    /// these nor among [`BlockOpen::points`] was made while it was open.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub own: Vec<String>,
}

impl SketchBlock {
    /// The block's name, or its id when it has none.
    pub fn label(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    /// Is the block open for editing its contents?
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

/// What [`Sketch::close_block`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockClosed {
    /// The block as it was captured again; `None` when it had no curves left, and went with its
    /// axis.
    pub made: Option<BlockMade>,
    /// The curves drawn while it was open that became members.
    pub added: Vec<String>,
    /// The curves that could not be members (text, projected curves, offset outputs, pattern
    /// geometry, curves of another block): they stay in the sketch, outside the block.
    pub left_out: Vec<String>,
}

/// A block's rows as the solver reads them: indices into the sketch.
pub(crate) struct FrameRows {
    pub o: usize,
    pub x: usize,
    pub scale: BlockScale,
    pub length: f64,
    pub points: Vec<(usize, [f64; 2])>,
    pub radii: Vec<(usize, f64)>,
}

/// What [`Sketch::make_block`] made.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockMade {
    pub block: String,
    pub axis: String,
    pub origin: String,
    pub held: Vec<String>,
}

/// Where a new block's origin goes.
#[derive(Clone, Copy)]
enum Anchor {
    /// The middle of its curves' bounding box.
    Middle,
    /// On this point (by index).
    Point(usize),
    /// At this sketch position.
    At(Vec2),
}

/// What releasing a block's held constraints did: the ones that hold where the geometry is now
/// drive again, the others are removed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Released {
    pub restored: Vec<String>,
    pub dropped: Vec<String>,
}

/// Each block's member points and curves (indices), looked up once for many questions.
pub struct BlockMembers(Vec<(HashSet<usize>, HashSet<usize>)>);

impl BlockMembers {
    /// Is every point and curve the constraint `k` names in one block?
    pub fn hold(&self, k: &ConstraintKind) -> bool {
        let (ps, cs) = k.indices();
        !(ps.is_empty() && cs.is_empty())
            && self.0.iter().any(|(points, curves)| ps.iter().all(|p| points.contains(p)) && cs.iter().all(|c| curves.contains(c)))
    }
}

/// Most curves one block takes.
pub const MAX_BLOCK_CURVES: usize = crate::CEILING_CURVES;
/// Most characters in a block's name.
pub const MAX_BLOCK_NAME: usize = 200;

/// A block's name as given: trimmed, none when empty, refused when too long.
fn block_name(name: Option<String>) -> Result<Option<String>> {
    let name = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    if name.as_ref().is_some_and(|n| n.chars().count() > MAX_BLOCK_NAME || n.chars().any(char::is_control)) {
        return Err(SketchError::Invalid(format!("a block's name must be at most {MAX_BLOCK_NAME} characters, without control characters")));
    }
    Ok(name)
}

/// The checks [`Sketch::validate_blocks`] makes of one block, apart from its id.
fn check_block(b: &SketchBlock) -> Result<()> {
    if b.curves.len() > crate::CEILING_CURVES || b.points.len() > crate::CEILING_POINTS || b.radii.len() > crate::CEILING_CURVES {
        return Err(SketchError::TooLarge);
    }
    if b.held.len() > crate::CEILING_CONSTRAINTS {
        return Err(SketchError::TooLarge);
    }
    if b.loose.len() > crate::CEILING_POINTS
        || b.open.as_ref().is_some_and(|o| {
            o.driving.len() > crate::CEILING_CONSTRAINTS
                || o.outside.len() > crate::CEILING_CURVES
                || o.points.len() > crate::CEILING_POINTS
                || o.own.len() > crate::CEILING_POINTS
        })
    {
        return Err(SketchError::TooLarge);
    }
    if b.name.as_ref().is_some_and(|n| n.chars().count() > MAX_BLOCK_NAME) {
        return Err(SketchError::Invalid(format!("block `{}` has too long a name", b.id)));
    }
    let bounded = |v: f64| v.is_finite() && v.abs() < crate::MAX_COORD;
    check_length(&b.id, b.length)?;
    if b.points.iter().any(|(_, uv)| !uv.iter().all(|v| bounded(*v))) || b.radii.iter().any(|(_, k)| !bounded(*k)) {
        return Err(SketchError::Invalid(format!("block `{}` lies too far from its axis for its size", b.id)));
    }
    Ok(())
}

/// Block `id`'s captured axis length `l` is finite, positive and in range.
fn check_length(id: &str, l: f64) -> Result<()> {
    if l.is_finite() && l > 0.0 && l < crate::MAX_COORD { Ok(()) } else { Err(SketchError::Invalid(format!("block `{id}` has an invalid length"))) }
}

/// A tidy length near `l`: 1, 2 or 5 times a power of ten.
pub(crate) fn tidy(l: f64) -> f64 {
    let p = 10f64.powf(l.log10().floor());
    [1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * p).min_by(|a, b| (a - l).abs().total_cmp(&(b - l).abs())).unwrap_or(l)
}

/// Constraint kinds that tie a block to something outside it whatever they name.
fn ties_outside(k: &ConstraintKind) -> bool {
    matches!(
        k,
        ConstraintKind::Fix { .. }
            | ConstraintKind::Pierce { .. }
            | ConstraintKind::OffsetDistance { .. }
            | ConstraintKind::PatternPoint { .. }
            | ConstraintKind::PatternRadius { .. }
    )
}

/// Curve `i` of `sk` as an edit can change it: the curve with its point indices taken out, and
/// the ids of its points.
fn curve_shape(sk: &Sketch, i: usize) -> Option<(crate::model::Curve, Vec<&str>)> {
    let mut c = sk.curves.get(i)?.clone();
    let ids = c.kind.point_ids().into_iter().filter_map(|p| sk.points.get(p)).map(|p| p.id.as_str()).collect();
    c.kind.map_points(&|_| 0);
    Some((c, ids))
}

/// Constraint `c` of `sk` as an edit can change it: the constraint with its indices taken out,
/// and the ids of the points and curves it names.
fn constraint_shape<'a>(sk: &'a Sketch, c: &Constraint) -> (Constraint, Vec<&'a str>, Vec<&'a str>) {
    let (ps, cs) = c.kind.indices();
    let mut c = c.clone();
    c.kind.remap(&|_| Some(0), &|_| Some(0));
    (
        c,
        ps.iter().filter_map(|p| sk.points.get(*p)).map(|p| p.id.as_str()).collect(),
        cs.iter().filter_map(|k| sk.curves.get(*k)).map(|k| k.id.as_str()).collect(),
    )
}

/// What of the sketch a block's constraints may name: its member curves, its member points (a
/// member curve's, or one that is a member on its own), and its axis line and that line's points.
struct Reach {
    curves: HashSet<usize>,
    points: HashSet<usize>,
    axis: Option<usize>,
    axis_points: Vec<usize>,
}

impl Reach {
    /// Does `k` name only what the block reaches, and at least one member?
    fn holds(&self, k: &ConstraintKind) -> bool {
        let (ps, cs) = k.indices();
        let member = ps.iter().any(|p| self.points.contains(p)) || cs.iter().any(|c| self.curves.contains(c));
        member
            && ps.iter().all(|p| self.points.contains(p) || self.axis_points.contains(p))
            && cs.iter().all(|c| self.curves.contains(c) || Some(*c) == self.axis)
    }
}

/// Whether curves can be block members, with the sketch's curves and block memberships looked up
/// once, so a block of a whole inserted drawing takes time in proportion to it.
struct MemberCheck<'a> {
    index: HashMap<&'a str, usize>,
    owner: HashMap<&'a str, &'a SketchBlock>,
    except: Option<&'a str>,
}

impl<'a> MemberCheck<'a> {
    fn new(sk: &'a Sketch, except: Option<&'a str>) -> Self {
        let mut index: HashMap<&str, usize> = HashMap::with_capacity(sk.curves.len());
        for (i, c) in sk.curves.iter().enumerate() {
            index.entry(c.id.as_str()).or_insert(i);
        }
        let mut owner: HashMap<&str, &SketchBlock> = HashMap::new();
        for b in &sk.blocks {
            for c in [&b.axis].into_iter().chain(&b.curves) {
                owner.entry(c.as_str()).or_insert(b);
            }
        }
        MemberCheck { index, owner, except }
    }

    /// Curve `id` (its index) when it can be a member, or why not.
    fn member(&self, sk: &Sketch, id: &str) -> Result<usize> {
        let c = self.index.get(id).copied().ok_or_else(|| SketchError::Unknown(id.into()))?;
        if let Some(b) = self.owner.get(id).filter(|b| Some(b.id.as_str()) != self.except) {
            return Err(SketchError::Invalid(if b.axis == id {
                format!("`{id}` is the axis of block {}", b.label())
            } else {
                format!("`{id}` is already part of block {}", b.label())
            }));
        }
        if self.owner.get(id).is_some_and(|b| b.axis == id) {
            return Err(SketchError::Invalid(format!("`{id}` is the block's own axis")));
        }
        if sk.is_text_curve(c) {
            return Err(SketchError::Invalid(format!("not supported yet: blocks of text (`{id}` is text)")));
        }
        if sk.is_linked_curve(c) {
            return Err(SketchError::Invalid(format!("`{id}` is projected from the model: break the link before making a block of it")));
        }
        if sk.offset_distance(id).is_some() {
            return Err(SketchError::Invalid(format!("`{id}` is the output of an offset: break the offset association before making a block of it")));
        }
        if let Some(p) = sk.patterns.iter().find(|p| p.contains(id)) {
            return Err(SketchError::Invalid(format!("not supported yet: blocks of sketch pattern geometry (`{id}` is part of pattern {})", p.id)));
        }
        Ok(c)
    }
}

impl Sketch {
    /// The block that has curve `id` as a member, or as its axis.
    pub fn block_of(&self, id: &str) -> Option<&SketchBlock> {
        self.blocks.iter().find(|b| b.axis == id || b.curves.iter().any(|c| c == id))
    }

    /// The block that has curve `id` as a member (not as its axis).
    pub fn block_with_member(&self, id: &str) -> Option<&SketchBlock> {
        self.blocks.iter().find(|b| b.curves.iter().any(|c| c == id))
    }

    /// Why curve `id` cannot be changed on its own: it is a member of a block that is not open
    /// for editing its contents.
    pub fn block_refusal(&self, id: &str) -> Option<String> {
        let block = self.block_with_member(id).filter(|b| !b.is_open())?;
        Some(format!("`{id}` is part of block {}, which keeps its shape: open it with Edit Contents to change it", block.label()))
    }

    /// The constraints block `b` holds, or for an open block those it will hold when it closes
    /// as it is now.
    pub fn would_hold(&self, b: &SketchBlock) -> Vec<String> {
        if !b.is_open() {
            return b.held.clone();
        }
        // Closing takes in what was drawn while it was open, so ask a copy that closes it.
        let mut copy = self.clone();
        copy.close_block(&b.id).ok().and_then(|closed| closed.made).map(|made| made.held).unwrap_or_default()
    }

    /// The block open for editing its contents, if one is.
    pub fn open_block_now(&self) -> Option<&SketchBlock> {
        self.blocks.iter().find(|b| b.is_open())
    }

    /// The closed block that has the curve or point `id` (a member point, or a line's
    /// `mid:` point) as a member.
    pub fn closed_block_with(&self, id: &str) -> Option<&SketchBlock> {
        if self.blocks.iter().all(SketchBlock::is_open) {
            return None;
        }
        let curve = id.strip_prefix("mid:").unwrap_or(id);
        if let Some(b) = self.block_with_member(curve) {
            return (!b.is_open()).then_some(b);
        }
        let p = self.resolve_point(id)?;
        let pid = self.points.get(p)?.id.as_str();
        self.blocks.iter().filter(|b| !b.is_open()).find(|b| b.points.iter().any(|(q, _)| q == pid))
    }

    /// While a block is open: per point, whether it may move (one of the block's points or a point
    /// made since it opened, that no curve outside uses), and per curve, whether it is one of the
    /// block's curves or of those drawn since it opened. `None` while no block is open.
    pub fn open_contents(&self) -> Option<(Vec<bool>, Vec<bool>)> {
        let (axis, open) = self.blocks.iter().find_map(|b| b.open.as_ref().map(|o| (b.axis.as_str(), o)))?;
        let outside: HashSet<&str> = open.outside.iter().map(String::as_str).chain(std::iter::once(axis)).collect();
        let inside: Vec<bool> = self.curves.iter().map(|c| !outside.contains(c.id.as_str())).collect();
        let others: HashSet<&str> = open.points.iter().map(String::as_str).collect();
        let mut free: Vec<bool> = self.points.iter().map(|p| !others.contains(p.id.as_str())).collect();
        for (_, curve) in self.curves.iter().enumerate().filter(|(c, _)| !inside.get(*c).copied().unwrap_or(false)) {
            for p in curve.kind.point_ids() {
                if let Some(f) = free.get_mut(p) {
                    *f = false;
                }
            }
        }
        Some((free, inside))
    }

    /// Per constraint: held by a block (kept, not solved). `None` when no constraint is held.
    pub(crate) fn held_mask(&self) -> Option<Vec<bool>> {
        if self.blocks.iter().all(|b| b.held.is_empty()) {
            return None;
        }
        let held: HashSet<&str> = self.blocks.iter().flat_map(|b| b.held.iter().map(String::as_str)).collect();
        Some(self.constraints.iter().map(|c| held.contains(c.id.as_str())).collect())
    }

    /// Is the constraint with this id held by a block?
    pub fn is_held(&self, id: &str) -> bool {
        self.blocks.iter().any(|b| b.held.iter().any(|h| h == id))
    }

    /// Every block's rows, by index, leaving out what no longer resolves.
    pub(crate) fn frame_rows(&self) -> Vec<FrameRows> {
        if self.blocks.is_empty() {
            return Vec::new();
        }
        let points: HashMap<&str, usize> = self.points.iter().enumerate().map(|(i, p)| (p.id.as_str(), i)).collect();
        let curves: HashMap<&str, usize> = self.curves.iter().enumerate().map(|(i, c)| (c.id.as_str(), i)).collect();
        let mut out = Vec::with_capacity(self.blocks.len());
        for b in self.blocks.iter().filter(|b| !b.is_open()) {
            let Some(CurveKind::Line { a, b: x }) = curves.get(b.axis.as_str()).and_then(|i| self.curves.get(*i)).map(|c| &c.kind) else {
                continue;
            };
            if !(b.length.is_finite() && b.length > 0.0) || a == x {
                continue;
            }
            let finite = |v: &[f64]| v.iter().all(|x| x.is_finite());
            out.push(FrameRows {
                o: *a,
                x: *x,
                scale: b.scale,
                length: b.length,
                points: b
                    .points
                    .iter()
                    .filter(|(_, uv)| finite(uv))
                    .filter_map(|(id, uv)| points.get(id.as_str()).map(|i| (*i, *uv)))
                    .filter(|(i, _)| i != a && i != x)
                    .collect(),
                radii: b
                    .radii
                    .iter()
                    .filter(|(_, k)| k.is_finite())
                    .filter_map(|(id, k)| curves.get(id.as_str()).map(|i| (*i, *k)))
                    .filter(|(i, _)| self.curves.get(*i).is_some_and(|c| matches!(c.kind, CurveKind::Circle { .. } | CurveKind::Ellipse { .. })))
                    .collect(),
            });
        }
        out
    }

    /// Make a block of the curves `ids` where they are now (nothing moves): a new construction
    /// line, the axis, starts at `origin` (a point index; default the centre of the curves'
    /// bounding box) and runs along sketch +X for half the box diagonal, rounded to a tidy length.
    /// The constraints wholly inside the curves are held. Refuses an empty or degenerate set, an
    /// unknown curve, and curves that cannot be members: in another block, projected, text,
    /// offset outputs and pattern geometry.
    pub fn make_block(&mut self, ids: &[String], scale: BlockScale, origin: Option<usize>, name: Option<String>) -> Result<BlockMade> {
        self.make_posed_block(ids, scale, origin.map_or(Anchor::Middle, Anchor::Point), 0.0, name)
    }

    /// [`Sketch::make_block`] with the block origin at the sketch position `at` and its axis
    /// turned `angle` (radians, counter-clockwise) from sketch +X, as Insert as block places a
    /// drawing's block: at the drawing's placement anchor, along its insert angle. The axis is
    /// half the curves' bounding-box diagonal long (rounded to a tidy length), however far `at`
    /// lies from them.
    pub fn make_block_at(&mut self, ids: &[String], scale: BlockScale, at: Vec2, angle: f64, name: Option<String>) -> Result<BlockMade> {
        if !(angle.is_finite() && in_range(at)) {
            return Err(SketchError::Invalid("the block's axis would lie out of range".into()));
        }
        self.make_posed_block(ids, scale, Anchor::At(at), angle, name)
    }

    /// [`Sketch::make_block`] with its origin at `anchor` and its axis `angle` radians from +X.
    fn make_posed_block(&mut self, ids: &[String], scale: BlockScale, anchor: Anchor, angle: f64, name: Option<String>) -> Result<BlockMade> {
        self.all_or_nothing(|sk| sk.make_posed_block_now(ids, scale, anchor, angle, name))
    }

    /// Run `f`, leaving the sketch as it was when `f` fails part way.
    fn all_or_nothing<T>(&mut self, f: impl FnOnce(&mut Sketch) -> Result<T>) -> Result<T> {
        let before = self.clone();
        let out = f(self);
        if out.is_err() {
            *self = before;
        }
        out
    }

    fn make_posed_block_now(&mut self, ids: &[String], scale: BlockScale, anchor: Anchor, angle: f64, name: Option<String>) -> Result<BlockMade> {
        let members = self.block_members(ids, None)?;
        let points = self.member_points(&members);
        let (lo, hi) = self.members_box(&members).ok_or_else(|| SketchError::Invalid("the curves have no size to make a block of".into()))?;
        let diagonal = lo.dist(hi);
        if !(diagonal.is_finite() && diagonal > 1e-6) {
            return Err(SketchError::Invalid("the curves have no size to make a block of".into()));
        }
        let name = block_name(name)?;
        let (origin, at) = match anchor {
            Anchor::Point(p) => (Some(p), self.point(p).ok_or_else(|| SketchError::Unknown(format!("point {p}")))?),
            Anchor::Middle => (None, lo.lerp(hi, 0.5)),
            Anchor::At(at) => (None, at),
        };
        // A position away from the curves (a file origin far from its drawing) keeps the axis
        // the curves' own size; a point or the middle reaches to the farthest member.
        let length = match anchor {
            Anchor::At(_) => tidy(diagonal / 2.0),
            Anchor::Point(_) | Anchor::Middle => {
                let reach = points.iter().filter_map(|p| self.point(*p)).map(|p| p.dist(at)).fold(0.0_f64, f64::max);
                tidy(reach.max(diagonal / 2.0))
            }
        };
        let end = at + Vec2::from_angle(angle) * length;
        if !(length.is_finite() && length > 0.0) || !in_range(at) || !in_range(end) {
            return Err(SketchError::Invalid("the block's axis would lie out of range".into()));
        }
        let o = self.add_point(at, None)?;
        let x = self.add_point(end, None)?;
        let axis_index = self.add_curve(CurveKind::Line { a: o, b: x }, None)?;
        if let Some(axis) = self.curves.get_mut(axis_index) {
            axis.construction = true;
        }
        let axis = self.curves.get(axis_index).map(|c| c.id.clone()).unwrap_or_default();
        let origin_id = self.points.get(o).map(|p| p.id.clone()).unwrap_or_default();
        if let Some(p) = origin.filter(|p| !points.contains(p)) {
            self.add_constraint(ConstraintKind::Coincident { p: o, q: p }, None)?;
        }
        let id = self.fresh("blk")?;
        let mut block = SketchBlock {
            id: id.clone(),
            name,
            curves: Vec::new(),
            axis: axis.clone(),
            scale,
            length,
            points: Vec::new(),
            radii: Vec::new(),
            held: Vec::new(),
            loose: Vec::new(),
            show_held: false,
            open: None,
        };
        self.capture(&mut block, &members, &[])?;
        check_block(&block)?;
        let held = block.held.clone();
        self.blocks.push(block);
        Ok(BlockMade { block: id, axis, origin: origin_id, held })
    }

    /// [`Sketch::make_block`] on an existing line as the axis (its start the block origin), as
    /// a paste of a whole block with its axis does.
    pub fn make_block_on(&mut self, ids: &[String], axis: &str, scale: BlockScale, name: Option<String>) -> Result<BlockMade> {
        self.all_or_nothing(|sk| sk.make_block_on_now(ids, axis, scale, name))
    }

    fn make_block_on_now(&mut self, ids: &[String], axis: &str, scale: BlockScale, name: Option<String>) -> Result<BlockMade> {
        let members = self.block_members(ids, None)?;
        let a = self.curve_index(axis).ok_or_else(|| SketchError::Unknown(axis.into()))?;
        if members.contains(&a) || self.block_of(axis).is_some() {
            return Err(SketchError::Invalid(format!("`{axis}` cannot be a block's axis and a member")));
        }
        let Some((o, _)) = self.curves.get(a).filter(|c| matches!(c.kind, CurveKind::Line { .. })).and_then(|c| c.kind.ends()) else {
            return Err(SketchError::WrongKind(axis.into(), "a block's axis must be a line".into()));
        };
        let origin = self.points.get(o).map(|p| p.id.clone()).unwrap_or_default();
        let name = block_name(name)?;
        let id = self.fresh("blk")?;
        let mut block = SketchBlock {
            id: id.clone(),
            name,
            curves: Vec::new(),
            axis: axis.into(),
            scale,
            length: 0.0,
            points: Vec::new(),
            radii: Vec::new(),
            held: Vec::new(),
            loose: Vec::new(),
            show_held: false,
            open: None,
        };
        self.capture(&mut block, &members, &[])?;
        check_block(&block)?;
        let held = block.held.clone();
        self.blocks.push(block);
        Ok(BlockMade { block: id, axis: axis.into(), origin, held })
    }

    /// Is every point and curve the constraint `k` names a member of one closed block?
    pub fn inside_a_block(&self, k: &ConstraintKind) -> bool {
        !self.blocks.is_empty() && self.block_members_now(false).hold(k)
    }

    /// Among the constraints `among`, the first that lies wholly inside a closed block (its
    /// members, and its axis with at least one member) that does not hold it, with the block's
    /// label: what would lock or fight the block's shape. A driven
    /// dimension, a constraint that ties the block to something outside (Fix) and a free
    /// block's length dimension (it sets the block's size) are passed over.
    pub fn loose_inside_closed<'a>(&self, among: impl IntoIterator<Item = &'a Constraint>) -> Option<(String, String)> {
        let among: Vec<&Constraint> = among.into_iter().filter(|c| !c.driven && !ties_outside(&c.kind)).collect();
        if among.is_empty() {
            return None;
        }
        let closed: Vec<(&SketchBlock, Reach)> = self.blocks.iter().filter(|b| !b.is_open()).map(|b| (b, self.block_reach(b))).collect();
        for c in among {
            for (b, reach) in &closed {
                let sizes = b.scale == BlockScale::Free && c.kind.is_dimension() && !c.kind.is_angle();
                if reach.holds(&c.kind) && !sizes && !b.held.contains(&c.id) {
                    return Some((c.id.clone(), b.label().to_string()));
                }
            }
        }
        None
    }

    /// Would a new driving constraint `k` lie wholly inside a closed block that does not hold it?
    pub fn locks_a_closed_block(&self, k: &ConstraintKind) -> bool {
        let c = Constraint { id: String::new(), kind: k.clone(), param: None, driven: false, text: None };
        self.loose_inside_closed([&c]).is_some()
    }

    /// Each closed block's member points and curves; with `axes`, its axis line and points too.
    pub fn block_members_now(&self, axes: bool) -> BlockMembers {
        let curves: HashMap<&str, usize> = self.curves.iter().enumerate().map(|(i, c)| (c.id.as_str(), i)).collect();
        BlockMembers(
            self.blocks
                .iter()
                .filter(|b| !b.is_open())
                .map(|b| {
                    let mut members: Vec<usize> = b.curves.iter().filter_map(|c| curves.get(c.as_str()).copied()).collect();
                    if axes {
                        members.extend(curves.get(b.axis.as_str()));
                    }
                    let mut points: HashSet<usize> = self.member_points(&members).into_iter().collect();
                    points.extend(b.loose.iter().filter_map(|p| self.point_index(p)));
                    (points, members.into_iter().collect())
                })
                .collect(),
        )
    }

    /// The blocks with a member among the curves `curves` or the points `points`, or whose axis
    /// is among them (ids).
    pub fn blocks_touching(&self, curves: &[usize], points: &[usize]) -> Vec<String> {
        let curve_ids: HashSet<&str> = curves.iter().filter_map(|c| self.curves.get(*c)).map(|c| c.id.as_str()).collect();
        let point_ids: HashSet<&str> = points.iter().filter_map(|p| self.points.get(*p)).map(|p| p.id.as_str()).collect();
        let axis_points = |b: &SketchBlock| {
            self.curve_index(&b.axis).and_then(|a| self.curves.get(a)).and_then(|c| c.kind.ends()).map(|(o, x)| [o, x]).unwrap_or_default()
        };
        self.blocks
            .iter()
            .filter(|b| {
                curve_ids.contains(b.axis.as_str())
                    || b.curves.iter().any(|c| curve_ids.contains(c.as_str()))
                    || b.points.iter().any(|(p, _)| point_ids.contains(p.as_str()))
                    || axis_points(b).iter().any(|p| points.contains(p))
            })
            .map(|b| b.id.clone())
            .collect()
    }

    /// The curves of the blocks `ids` (indices): their members, and their axes with `axes`.
    pub fn block_curves(&self, ids: &[String], axes: bool) -> Vec<usize> {
        self.blocks
            .iter()
            .filter(|b| ids.contains(&b.id))
            .flat_map(|b| b.curves.iter().chain(axes.then_some(&b.axis)))
            .filter_map(|c| self.curve_index(c))
            .collect()
    }

    /// Hold, in each of the blocks `ids`, the constraints wholly inside it whose ids are not
    /// among `before`: the relations a turn of the whole block puts in place of held ones.
    pub fn hold_added(&mut self, ids: &[String], before: &[String]) {
        let before: HashSet<&str> = before.iter().map(String::as_str).collect();
        for i in 0..self.blocks.len() {
            let Some(b) = self.blocks.get(i).filter(|b| ids.contains(&b.id)) else { continue };
            let added: Vec<String> = self.inside(&self.block_reach(b)).into_iter().filter(|c| !before.contains(c.as_str())).collect();
            if let Some(b) = self.blocks.get_mut(i) {
                b.held.extend(added);
            }
        }
    }

    /// Scale the length of each of the blocks `ids` by `factor`: after a scale of a whole block
    /// that scaled the dimensions inside it too.
    pub fn scale_block_lengths(&mut self, ids: &[String], factor: f64) -> Result<()> {
        if factor == 1.0 {
            return Ok(());
        }
        // Every new length is checked before any block changes.
        for b in self.blocks.iter().filter(|b| ids.contains(&b.id)) {
            check_length(&b.id, b.length * factor)?;
        }
        for b in self.blocks.iter_mut().filter(|b| ids.contains(&b.id)) {
            b.length *= factor;
        }
        Ok(())
    }

    /// The length of block `b`'s axis now.
    fn axis_length(&self, b: &SketchBlock) -> Option<f64> {
        let (o, x) = self.curve_index(&b.axis).and_then(|a| self.curves.get(a)).and_then(|c| c.kind.ends())?;
        Some(self.point(o)?.dist(self.point(x)?))
    }

    /// How much a free block has grown since its shape was captured (1 for a fixed one).
    fn growth(&self, b: &SketchBlock) -> f64 {
        match (b.scale, self.axis_length(b)) {
            (BlockScale::Free, Some(l)) if l.is_finite() && l > 0.0 && b.length > 0.0 => l / b.length,
            _ => 1.0,
        }
    }

    /// The held constraint `id` as it would drive once released: a free block's dimensions that
    /// are plain numbers (no parameter) scaled by how much the block has grown. `None` when no
    /// block holds it.
    pub fn held_kind_now(&self, id: &str) -> Option<ConstraintKind> {
        let b = self.blocks.iter().find(|b| b.held.iter().any(|h| h == id))?;
        let c = self.constraints.iter().find(|c| c.id == id)?;
        let mut kind = c.kind.clone();
        let s = self.growth(b);
        if c.param.is_none()
            && (s - 1.0).abs() > 1e-12
            && kind.is_dimension()
            && !kind.is_angle()
            && let Some(v) = kind.value()
        {
            kind.set_value(v * s);
        }
        Some(kind)
    }

    /// Scale block `index`'s held plain dimensions to its size now (see [`Sketch::held_kind_now`])
    /// and make that size its own.
    fn rescale_held(&mut self, index: usize) {
        let Some(b) = self.blocks.get(index) else { return };
        let kinds: Vec<(String, ConstraintKind)> = b.held.iter().filter_map(|h| Some((h.clone(), self.held_kind_now(h)?))).collect();
        let length = self.axis_length(b);
        for (id, kind) in kinds {
            if let Some(c) = self.constraints.iter_mut().find(|c| c.id == id) {
                c.kind = kind;
            }
        }
        if let (Some(l), Some(b)) = (length.filter(|l| l.is_finite() && *l > 0.0), self.blocks.get_mut(index)) {
            b.length = l;
        }
    }

    /// Let the constraints `ids` drive again where they hold at the geometry's place now, and
    /// remove the others (a turned block's horizontals, a resized block's dimensions driven by
    /// parameters).
    fn release(&mut self, ids: &[String]) -> Released {
        let mut out = Released::default();
        for id in ids {
            let Some(ci) = self.constraints.iter().position(|c| c.id == *id) else { continue };
            if crate::solver::constraint_residual(self, ci).is_some_and(|r| r <= crate::solver::CONSISTENT) {
                out.restored.push(id.clone());
            } else {
                self.constraints.remove(ci);
                out.dropped.push(id.clone());
            }
        }
        out
    }

    /// Hold exactly the constraints `held` of block `id` that are wholly inside it, as a pasted
    /// copy of a whole block held them.
    pub fn set_block_held(&mut self, id: &str, held: &[String]) {
        let Some(i) = self.blocks.iter().position(|b| b.id == id) else { return };
        let inside: HashSet<String> = self.blocks.get(i).map(|b| self.inside(&self.block_reach(b)).into_iter().collect()).unwrap_or_default();
        if let Some(b) = self.blocks.get_mut(i) {
            b.held = held.iter().filter(|h| inside.contains(*h)).cloned().collect();
        }
    }

    /// Capture `members` and the points `loose` (members on their own) into `block` from where
    /// they and the block's axis are now: each point's place in the frame, each radius's share of
    /// the axis length, and the constraints held.
    fn capture(&self, block: &mut SketchBlock, members: &[usize], loose: &[usize]) -> Result<()> {
        let axis = self.curve_index(&block.axis).ok_or_else(|| SketchError::Unknown(block.axis.clone()))?;
        let Some(CurveKind::Line { a, b }) = self.curves.get(axis).map(|c| &c.kind) else {
            return Err(SketchError::WrongKind(block.axis.clone(), "a block's axis must be a line".into()));
        };
        let (o, x) = (self.point(*a).unwrap_or_default(), self.point(*b).unwrap_or_default());
        let d = x - o;
        let l2 = d.len2();
        if !(l2.is_finite() && l2 > 1e-18) {
            return Err(SketchError::Invalid("the block's axis has no length".into()));
        }
        block.length = l2.sqrt();
        let mut points = self.member_points(members);
        let on_members: HashSet<usize> = points.iter().copied().collect();
        let loose: Vec<usize> = loose.iter().copied().filter(|p| !on_members.contains(p) && *p != *a && *p != *b).collect();
        points.extend(&loose);
        block.loose = loose.iter().filter_map(|p| self.points.get(*p)).map(|p| p.id.clone()).collect();
        block.curves = members.iter().filter_map(|c| self.curves.get(*c).map(|c| c.id.clone())).collect();
        block.points = points
            .iter()
            .filter(|p| **p != *a && **p != *b)
            .filter_map(|p| {
                let q = self.points.get(*p)?;
                let r = q.pos - o;
                Some((q.id.clone(), [r.dot(d) / l2, r.dot(d.perp()) / l2]))
            })
            .collect();
        let l = l2.sqrt();
        block.radii = members
            .iter()
            .filter_map(|c| {
                let curve = self.curves.get(*c)?;
                match curve.kind {
                    CurveKind::Circle { r, .. } | CurveKind::Ellipse { r, .. } => Some((curve.id.clone(), r / l)),
                    _ => None,
                }
            })
            .collect();
        let inside = self.inside(&self.reach(members, &loose, Some(axis)));
        // A constraint that was inside the block and drove while it existed keeps driving.
        let driving: HashSet<&str> = match self.blocks.iter().find(|b| b.id == block.id) {
            Some(SketchBlock { open: Some(open), .. }) => open.driving.iter().map(String::as_str).collect(),
            Some(old) => self
                .inside(&self.block_reach(old))
                .into_iter()
                .filter(|c| !old.held.contains(c))
                .filter_map(|c| self.constraints.iter().find(|k| k.id == c))
                .map(|c| c.id.as_str())
                .collect(),
            None => HashSet::new(),
        };
        block.held = inside.into_iter().filter(|c| !driving.contains(c.as_str())).collect();
        Ok(())
    }

    /// The ids of the driving constraints wholly inside `reach`: every point and curve they name
    /// is a member, a member's point or the axis, and one of them a member.
    fn inside(&self, reach: &Reach) -> Vec<String> {
        self.constraints.iter().filter(|c| !c.driven && !ties_outside(&c.kind) && reach.holds(&c.kind)).map(|c| c.id.clone()).collect()
    }

    /// What constraints of the curves `members`, the points `loose` and the axis line `axis` reach.
    fn reach(&self, members: &[usize], loose: &[usize], axis: Option<usize>) -> Reach {
        let mut points: HashSet<usize> = self.member_points(members).into_iter().collect();
        points.extend(loose);
        let axis_points = axis.and_then(|a| self.curves.get(a)).and_then(|c| c.kind.ends()).map(|(o, x)| vec![o, x]).unwrap_or_default();
        Reach { curves: members.iter().copied().collect(), points, axis, axis_points }
    }

    /// What block `b`'s constraints reach as the sketch is now.
    fn block_reach(&self, b: &SketchBlock) -> Reach {
        let ids: HashSet<&str> = b.curves.iter().map(String::as_str).collect();
        let members: Vec<usize> = self.curves.iter().enumerate().filter(|(_, c)| ids.contains(c.id.as_str())).map(|(i, _)| i).collect();
        let axis = self.curves.iter().position(|c| c.id == b.axis);
        self.reach(&members, &self.loose_points(b), axis)
    }

    /// Block `b`'s points that are members on their own (indices).
    fn loose_points(&self, b: &SketchBlock) -> Vec<usize> {
        if b.loose.is_empty() {
            return Vec::new();
        }
        let ids: HashSet<&str> = b.loose.iter().map(String::as_str).collect();
        self.points.iter().enumerate().filter(|(_, p)| ids.contains(p.id.as_str())).map(|(i, _)| i).collect()
    }

    /// Change a block: its scale, its name, and its members (`add`, `remove` by curve id), and
    /// capture its shape again from where its curves are now. Returns what [`Sketch::make_block`]
    /// returns, and what became of the held constraints no longer inside it (as
    /// [`Sketch::explode_block`] releases them).
    pub fn edit_block(
        &mut self,
        id: &str,
        scale: Option<BlockScale>,
        name: Option<Option<String>>,
        add: &[String],
        remove: &[String],
    ) -> Result<(BlockMade, Released)> {
        let index = self.blocks.iter().position(|b| b.id == id).ok_or_else(|| SketchError::Unknown(id.into()))?;
        let block = self.blocks.get(index).cloned().ok_or_else(|| SketchError::Unknown(id.into()))?;
        if block.is_open() {
            return Err(SketchError::Invalid(format!("block {} is open for editing its contents: close it first", block.label())));
        }
        for r in remove {
            if !block.curves.contains(r) {
                return Err(SketchError::Invalid(format!("`{r}` is not a member of block {}", block.label())));
            }
        }
        let mut ids: Vec<String> = block.curves.iter().filter(|c| !remove.contains(c)).cloned().collect();
        ids.extend(add.iter().cloned());
        // The held constraints count again while the members are worked out: they were held
        // only because the block existed.
        let members = self.block_members(&ids, Some(id))?;
        let name = name.map(block_name).transpose()?;
        self.rescale_held(index);
        let mut block = self.blocks.get(index).cloned().ok_or_else(|| SketchError::Unknown(id.into()))?;
        let old_held = block.held.clone();
        if let Some(s) = scale {
            block.scale = s;
        }
        if let Some(n) = name {
            block.name = n;
        }
        let loose = self.loose_points(&block);
        self.capture(&mut block, &members, &loose)?;
        check_block(&block)?;
        let gone: Vec<String> = old_held.into_iter().filter(|h| !block.held.contains(h)).collect();
        if let Some(slot) = self.blocks.get_mut(index) {
            *slot = block.clone();
        }
        let released = self.release(&gone);
        let origin = self.block_origin(&block);
        Ok((BlockMade { block: block.id, axis: block.axis, origin, held: block.held }, released))
    }

    /// Show or hide, while block `id` is closed, the constraints and dimensions it holds. Nothing
    /// else changes, and an open block takes it too.
    pub fn set_block_show_held(&mut self, id: &str, show: bool) -> Result<BlockMade> {
        let b = self.blocks.iter_mut().find(|b| b.id == id).ok_or_else(|| SketchError::Unknown(id.into()))?;
        b.show_held = show;
        let b = b.clone();
        Ok(BlockMade { origin: self.block_origin(&b), block: b.id, axis: b.axis, held: b.held })
    }

    /// Turn a block back into plain geometry, its axis staying as a plain construction line. Its
    /// held constraints that hold where the geometry is now drive again (a free block's plain
    /// dimensions first scaled to its size now); the others are removed. Returns the former
    /// members and what became of the held constraints.
    pub fn explode_block(&mut self, id: &str) -> Result<(Vec<String>, Released)> {
        let index = self.blocks.iter().position(|b| b.id == id).ok_or_else(|| SketchError::Unknown(id.into()))?;
        self.rescale_held(index);
        let block = self.blocks.remove(index);
        let released = self.release(&block.held);
        Ok((block.curves, released))
    }

    /// Open block `id` for editing its contents in place: its frame rows leave the solve, its
    /// held constraints drive again where they hold where the geometry is now (a free block's
    /// plain dimensions first scaled to its size now), and everything outside its curves, its
    /// axis included, is held where it is until it closes. A held constraint that sets a member
    /// line's direction in the sketch (Horizontal, Vertical, a line angle with a plain value) and
    /// no longer holds because the block turned becomes the same direction relative to the
    /// block's axis: Parallel or Perpendicular to it, or an angle to it, as the line lies now.
    /// The other held constraints that no longer hold are removed, as Explode Block removes them.
    /// Another open block is closed first (its id is returned). Opening the open block again
    /// changes nothing.
    pub fn open_block(&mut self, id: &str) -> Result<(Released, Option<String>)> {
        let b = self.blocks.iter().find(|b| b.id == id).ok_or_else(|| SketchError::Unknown(id.into()))?;
        if b.is_open() {
            return Ok((Released::default(), None));
        }
        let other = self.open_block_now().map(|b| b.id.clone());
        if let Some(other) = &other {
            self.close_block(other)?;
        }
        let index = self.blocks.iter().position(|b| b.id == id).ok_or_else(|| SketchError::Unknown(id.into()))?;
        self.rescale_held(index);
        let b = self.blocks.get(index).cloned().ok_or_else(|| SketchError::Unknown(id.into()))?;
        let held: HashSet<&str> = b.held.iter().map(String::as_str).collect();
        let driving: Vec<String> = self.inside(&self.block_reach(&b)).into_iter().filter(|c| !held.contains(c.as_str())).collect();
        let member_ids: HashSet<&str> = b.curves.iter().map(String::as_str).collect();
        let outside: Vec<String> = self.curves.iter().filter(|c| !member_ids.contains(c.id.as_str())).map(|c| c.id.clone()).collect();
        let own: HashSet<&str> = b.points.iter().map(|(p, _)| p.as_str()).collect();
        let points: Vec<String> = self.points.iter().filter(|p| !own.contains(p.id.as_str())).map(|p| p.id.clone()).collect();
        let own: Vec<String> = b.points.iter().map(|(p, _)| p.clone()).collect();
        let axis = self.curve_index(&b.axis);
        let released = self.release_onto(&b.held, axis);
        if let Some(slot) = self.blocks.get_mut(index) {
            slot.held.clear();
            slot.open = Some(BlockOpen { driving, outside, points, own });
            check_block(slot)?;
        }
        Ok((released, other))
    }

    /// [`Sketch::release`], putting a direction in the sketch that no longer holds onto the axis
    /// line `axis` (see [`Sketch::open_block`]).
    fn release_onto(&mut self, ids: &[String], axis: Option<usize>) -> Released {
        let mut out = Released::default();
        for id in ids {
            let Some(ci) = self.constraints.iter().position(|c| c.id == *id) else { continue };
            if crate::solver::constraint_residual(self, ci).is_some_and(|r| r <= crate::solver::CONSISTENT) {
                out.restored.push(id.clone());
            } else if let Some(kind) = axis.and_then(|a| self.direction_on_axis(ci, a)) {
                if let Some(c) = self.constraints.get_mut(ci) {
                    c.kind = kind;
                }
                out.restored.push(id.clone());
            } else {
                self.constraints.remove(ci);
                out.dropped.push(id.clone());
            }
        }
        out
    }

    /// Constraint `ci`, a direction of a line in the sketch (Horizontal, Vertical, a line angle
    /// with a plain value), as the same line's direction relative to the line `axis` where both
    /// lie now: Parallel, Perpendicular or an angle, whichever holds there.
    fn direction_on_axis(&self, ci: usize, axis: usize) -> Option<ConstraintKind> {
        let c = self.constraints.get(ci)?;
        let l = match c.kind {
            ConstraintKind::Horizontal { l } | ConstraintKind::Vertical { l } => l,
            ConstraintKind::LineAngle { l, .. } if c.param.is_none() => l,
            _ => return None,
        };
        let ends = |line: usize| match self.curves.get(line).map(|c| &c.kind) {
            Some(CurveKind::Line { a, b }) => Some(self.point(*b)? - self.point(*a)?),
            _ => None,
        };
        let (u, w) = (ends(l)?, ends(axis)?);
        if l == axis || u.len2() <= 1e-24 || w.len2() <= 1e-24 {
            return None;
        }
        let theta = (u.x * w.y - u.y * w.x).abs().atan2(u.dot(w));
        let mut candidates = vec![ConstraintKind::Parallel { a: l, b: axis }, ConstraintKind::Perpendicular { a: l, b: axis }];
        for value in [theta, std::f64::consts::PI - theta] {
            for flip in [false, true] {
                candidates.push(ConstraintKind::Angle { a: l, b: axis, value, flip });
            }
        }
        candidates.into_iter().find(|k| crate::kind_residual(self, k) <= crate::solver::CONSISTENT)
    }

    /// Close the open block `id`: capture its shape again into the same block from where its
    /// curves are now, relative to its axis, which did not move. The curves and the points on no
    /// curve made while it was open join it (curves that cannot be members are left out, see
    /// [`BlockClosed::left_out`]); the constraints wholly inside it are held again, except those
    /// that drove when it was opened. A block left with no curve goes with its axis.
    pub fn close_block(&mut self, id: &str) -> Result<BlockClosed> {
        self.close_block_taking(id, true)
    }

    /// [`Sketch::close_block`]; without `adopt`, with its members as they are listed, taking
    /// nothing made while it was open.
    fn close_block_taking(&mut self, id: &str, adopt: bool) -> Result<BlockClosed> {
        let index = self.blocks.iter().position(|b| b.id == id).ok_or_else(|| SketchError::Unknown(id.into()))?;
        let b = self.blocks.get(index).cloned().ok_or_else(|| SketchError::Unknown(id.into()))?;
        let Some(open) = &b.open else {
            return Err(SketchError::Invalid(format!("block {} is not open", b.label())));
        };
        let outside: HashSet<&str> = open.outside.iter().map(String::as_str).chain(std::iter::once(b.axis.as_str())).collect();
        let was: HashSet<&str> = b.curves.iter().map(String::as_str).collect();
        let check = MemberCheck::new(self, Some(id));
        let (mut kept, mut added, mut left_out) = (Vec::new(), Vec::new(), Vec::new());
        // The members still there, then the curves drawn since it opened (their splines bring
        // their guide lines), in sketch order; another block's curves stay that block's.
        let drawn = self
            .curves
            .iter()
            .filter(|c| adopt && !outside.contains(c.id.as_str()) && !was.contains(c.id.as_str()) && c.spline_guide_owner().is_none())
            .map(|c| c.id.as_str())
            .filter(|c| check.owner.get(c).is_none_or(|o| o.id == id));
        for c in b.curves.iter().map(String::as_str).filter(|c| check.index.contains_key(c)).chain(drawn) {
            match check.member(self, c) {
                Ok(i) => {
                    kept.push(i);
                    if !was.contains(c) {
                        added.push(c.to_string());
                    }
                }
                Err(_) => left_out.push(c.to_string()),
            }
        }
        let others: HashSet<String> = check.owner.iter().filter(|(_, o)| o.id != id).map(|(c, _)| c.to_string()).collect();
        if kept.is_empty() {
            self.blocks.remove(index);
            self.remove_block_axes(vec![b.axis.clone()]);
            return Ok(BlockClosed { made: None, added, left_out });
        }
        let mut members = self.with_guides(kept);
        // Another block's guide line stays that block's.
        members.retain(|c| self.curves.get(*c).is_some_and(|c| !others.contains(c.id.as_str())));
        if members.len() > MAX_BLOCK_CURVES {
            return Err(SketchError::TooLarge);
        }
        // The points on no curve: those that were members on their own, and those made since.
        let mut loose = self.loose_points(&b);
        if adopt {
            let theirs: HashSet<&str> = open.points.iter().chain(&open.own).map(String::as_str).collect();
            let mut on_curve = vec![false; self.points.len()];
            for p in self.curves.iter().flat_map(|c| c.kind.point_ids()) {
                if let Some(o) = on_curve.get_mut(p) {
                    *o = true;
                }
            }
            loose.extend(
                self.points
                    .iter()
                    .enumerate()
                    .filter(|(i, p)| !on_curve.get(*i).copied().unwrap_or(true) && !theirs.contains(p.id.as_str()))
                    .map(|(i, _)| i),
            );
        }
        let mut block = b.clone();
        self.capture(&mut block, &members, &loose)?;
        block.open = None;
        check_block(&block)?;
        let made = BlockMade { block: block.id.clone(), axis: block.axis.clone(), origin: self.block_origin(&block), held: block.held.clone() };
        if let Some(slot) = self.blocks.get_mut(index) {
            *slot = block;
        }
        Ok(BlockClosed { made: Some(made), added, left_out })
    }

    /// Close every open block, taking what was made while it was open (a sketch left). A block
    /// that cannot be captured again is exploded: its curves stay, with every constraint driving.
    pub fn close_open_blocks(&mut self) {
        self.close_every_open(true);
    }

    /// Close every open block with its members as they are listed, taking nothing else in (a
    /// design opened: what its file says was drawn while it was open is not trusted).
    pub fn close_open_blocks_as_saved(&mut self) {
        self.close_every_open(false);
    }

    fn close_every_open(&mut self, adopt: bool) {
        let open: Vec<String> = self.blocks.iter().filter(|b| b.is_open()).map(|b| b.id.clone()).collect();
        for id in open {
            if self.close_block_taking(&id, adopt).is_err() {
                self.blocks.retain(|b| b.id != id);
            }
        }
    }

    /// What an edit changed outside the block open in `before` that is still open here: the id
    /// of the first curve, point or constraint outside its contents the edit changed, moved,
    /// removed or put in a block, or of a constraint it added that names nothing inside the
    /// block or names its axis. `None` when no block was open, or the edit closed it.
    pub fn outside_change(&self, before: &Sketch) -> Option<String> {
        let open = before.open_block_now()?;
        if !self.open_block_now().is_some_and(|b| b.id == open.id) {
            return None;
        }
        let (free, inside) = before.open_contents()?;
        let (free_now, inside_now) = self.open_contents()?;
        let curves_now: HashMap<&str, usize> = self.curves.iter().enumerate().map(|(i, c)| (c.id.as_str(), i)).collect();
        let points_now: HashMap<&str, usize> = self.points.iter().enumerate().map(|(i, p)| (p.id.as_str(), i)).collect();
        let owners = |sk: &'_ Sketch| -> HashMap<String, String> {
            sk.blocks.iter().flat_map(|b| b.curves.iter().chain(std::iter::once(&b.axis)).map(|c| (c.clone(), b.id.clone()))).collect()
        };
        let (owned, owned_now) = (owners(before), owners(self));
        for (i, c) in before.curves.iter().enumerate().filter(|(i, _)| !inside.get(*i).copied().unwrap_or(false)) {
            let Some(&j) = curves_now.get(c.id.as_str()) else { return Some(c.id.clone()) };
            if curve_shape(before, i) != curve_shape(self, j) || owned.get(&c.id) != owned_now.get(&c.id) {
                return Some(c.id.clone());
            }
        }
        for (_, p) in before.points.iter().enumerate().filter(|(i, _)| !free.get(*i).copied().unwrap_or(false)) {
            if points_now.get(p.id.as_str()).and_then(|j| self.points.get(*j)) != Some(p) {
                return Some(p.id.clone());
            }
        }
        let fixed = |sk: &Sketch, c: &Constraint, free: &[bool], inside: &[bool]| {
            let axis = sk.curve_index(&open.axis);
            let axis_points = axis.and_then(|a| sk.curves.get(a)).and_then(|c| c.kind.ends()).map(|(o, x)| [o, x]);
            let (ps, cs) = c.kind.indices();
            let on_axis = cs.iter().any(|k| Some(*k) == axis) || ps.iter().any(|p| axis_points.is_some_and(|e| e.contains(p)));
            let names_inside =
                ps.iter().any(|p| free.get(*p).copied().unwrap_or(false)) || cs.iter().any(|k| inside.get(*k).copied().unwrap_or(false));
            on_axis || !names_inside
        };
        let constraints_now: HashMap<&str, &Constraint> = self.constraints.iter().map(|c| (c.id.as_str(), c)).collect();
        for c in before.constraints.iter().filter(|c| fixed(before, c, &free, &inside)) {
            if constraints_now.get(c.id.as_str()).is_none_or(|now| constraint_shape(before, c) != constraint_shape(self, now)) {
                return Some(c.id.clone());
            }
        }
        let had: HashSet<&str> = before.constraints.iter().map(|c| c.id.as_str()).collect();
        self.constraints.iter().find(|c| !had.contains(c.id.as_str()) && fixed(self, c, &free_now, &inside_now)).map(|c| c.id.clone())
    }

    /// The id of block `b`'s origin, the start of its axis.
    fn block_origin(&self, b: &SketchBlock) -> String {
        self.curve_index(&b.axis)
            .and_then(|c| self.curves.get(c))
            .and_then(|c| c.kind.ends())
            .and_then(|(o, _)| self.points.get(o))
            .map(|p| p.id.clone())
            .unwrap_or_default()
    }

    /// The curves `ids` as members of one block: each known, once, and able to be a member (see
    /// [`Sketch::make_block`]); with the guide lines of member splines. `except`: the block
    /// being edited, whose own members and axis may be named.
    fn block_members(&self, ids: &[String], except: Option<&str>) -> Result<Vec<usize>> {
        if ids.is_empty() {
            return Err(SketchError::Invalid("name the curves to make a block of".into()));
        }
        if ids.len() > MAX_BLOCK_CURVES {
            return Err(SketchError::TooLarge);
        }
        let check = MemberCheck::new(self, except);
        let mut members = Vec::new();
        let mut seen = HashSet::new();
        for id in ids {
            if seen.insert(id.as_str()) {
                members.push(check.member(self, id)?);
            }
        }
        let members = self.with_guides(members);
        // A guide line comes with its spline, and only when no other block has it.
        for c in members.iter().filter_map(|c| self.curves.get(*c)).filter(|c| !seen.contains(c.id.as_str())) {
            check.member(self, &c.id)?;
        }
        Ok(members)
    }

    /// `members` with the guide lines of the splines among them.
    fn with_guides(&self, mut members: Vec<usize>) -> Vec<usize> {
        let splines: HashSet<&str> = members.iter().filter_map(|c| self.curves.get(*c)).map(|c| c.id.as_str()).collect();
        let named: HashSet<usize> = members.iter().copied().collect();
        let guides: Vec<usize> = self
            .curves
            .iter()
            .enumerate()
            .filter(|(i, c)| c.spline_guide_owner().is_some_and(|o| splines.contains(o)) && !named.contains(i))
            .map(|(i, _)| i)
            .collect();
        members.extend(guides);
        members
    }

    /// The points of the curves `members`, each once, in order.
    fn member_points(&self, members: &[usize]) -> Vec<usize> {
        let mut seen = HashSet::new();
        members.iter().filter_map(|c| self.curves.get(*c)).flat_map(|c| c.kind.point_ids()).filter(|p| seen.insert(*p)).collect()
    }

    /// The box around the curves `members`: their points, and the extents of circles and ellipses.
    fn members_box(&self, members: &[usize]) -> Option<(Vec2, Vec2)> {
        let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
        let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut grow = |p: Vec2, r: f64| {
            lo = Vec2::new(lo.x.min(p.x - r), lo.y.min(p.y - r));
            hi = Vec2::new(hi.x.max(p.x + r), hi.y.max(p.y + r));
        };
        for c in members.iter().filter_map(|c| self.curves.get(*c)) {
            for p in c.kind.point_ids() {
                grow(self.point(p)?, 0.0);
            }
            match c.kind {
                CurveKind::Circle { c, r } => grow(self.point(c)?, r.abs()),
                CurveKind::Ellipse { c, m, r } => {
                    let (c, m) = (self.point(c)?, self.point(m)?);
                    grow(c, c.dist(m).max(r.abs()));
                }
                _ => {}
            }
        }
        (lo.x.is_finite() && hi.x.is_finite() && lo.y.is_finite() && hi.y.is_finite()).then_some((lo, hi))
    }

    /// Drop from blocks what the sketch no longer has: members, points, radii and held
    /// constraints. A block whose axis is gone is exploded. Returns the axes of the blocks that
    /// lost every member, which are dropped too; the caller removes those lines.
    pub fn prune_blocks(&mut self) -> Vec<String> {
        if self.blocks.is_empty() {
            return Vec::new();
        }
        let curves: HashMap<&str, &crate::model::Curve> = self.curves.iter().map(|c| (c.id.as_str(), c)).collect();
        let constraints: HashSet<&str> = self.constraints.iter().map(|c| c.id.as_str()).collect();
        let existing: HashSet<&str> = self.points.iter().map(|p| p.id.as_str()).collect();
        let axis_ok = |axis: &str| curves.get(axis).is_some_and(|c| matches!(c.kind, CurveKind::Line { a, b } if a != b));
        let mut empty = Vec::new();
        let mut kept = Vec::with_capacity(self.blocks.len());
        let mut shrunk = Vec::with_capacity(self.blocks.len());
        let mut loose = Vec::new();
        for mut b in std::mem::take(&mut self.blocks) {
            if !axis_ok(&b.axis) {
                loose.extend(b.held);
                continue;
            }
            let before = (b.curves.len(), b.points.len());
            b.curves.retain(|c| curves.contains_key(c.as_str()));
            // The points the members that are left still use.
            let used: HashSet<&str> = b
                .curves
                .iter()
                .filter_map(|c| curves.get(c.as_str()))
                .flat_map(|c| c.kind.point_ids())
                .filter_map(|p| self.points.get(p))
                .map(|p| p.id.as_str())
                .collect();
            b.loose.retain(|p| existing.contains(p.as_str()));
            let loose_now: HashSet<&str> = b.loose.iter().map(String::as_str).collect();
            b.points.retain(|(p, _)| used.contains(p.as_str()) || loose_now.contains(p.as_str()));
            let members: HashSet<&str> = b.curves.iter().map(String::as_str).collect();
            b.radii.retain(|(c, _)| members.contains(c.as_str()));
            b.held.retain(|h| constraints.contains(h.as_str()));
            if let Some(open) = &mut b.open {
                open.driving.retain(|h| constraints.contains(h.as_str()));
                open.outside.retain(|c| curves.contains_key(c.as_str()));
                open.points.retain(|p| existing.contains(p.as_str()));
                open.own.retain(|p| existing.contains(p.as_str()));
            }
            let shrank = (b.curves.len(), b.points.len()) != before;
            // An open block stays while its contents are edited, even with no member left.
            if b.curves.is_empty() && !b.is_open() {
                empty.push(b.axis);
            } else {
                kept.push(b);
                shrunk.push(shrank);
            }
        }
        self.blocks = kept;
        // A closed block that lost members holds only what is still wholly inside it.
        let kinds: HashMap<&str, &ConstraintKind> = self.constraints.iter().map(|c| (c.id.as_str(), &c.kind)).collect();
        let mut leaving: Vec<(usize, Vec<String>)> = Vec::new();
        for (i, b) in self.blocks.iter().enumerate() {
            if !shrunk.get(i).copied().unwrap_or(false) || b.is_open() || b.held.is_empty() {
                continue;
            }
            let reach = self.block_reach(b);
            let out: Vec<String> = b.held.iter().filter(|h| kinds.get(h.as_str()).is_some_and(|k| !reach.holds(k))).cloned().collect();
            if !out.is_empty() {
                leaving.push((i, out));
            }
        }
        for (i, out) in leaving {
            if let Some(b) = self.blocks.get_mut(i) {
                b.held.retain(|h| !out.contains(h));
            }
            loose.extend(out);
        }
        // A block whose axis went is exploded: what it held drives again where it holds.
        self.release(&loose);
        empty
    }

    /// [`Sketch::prune_blocks`], removing the axes of the blocks it dropped for having no member.
    pub fn prune_blocks_and_axes(&mut self) {
        let axes = self.prune_blocks();
        self.remove_block_axes(axes);
    }

    /// Remove the axis lines `axes` that [`Sketch::prune_blocks`] left without a block.
    pub(crate) fn remove_block_axes(&mut self, axes: Vec<String>) {
        let which: Vec<usize> = axes.iter().filter_map(|a| self.curve_index(a)).collect();
        if !which.is_empty() {
            self.remove_curves(&which);
        }
    }

    /// A design's blocks check out: within the size limits, unique ids, positive finite lengths
    /// and finite places.
    pub fn validate_blocks(&self) -> Result<()> {
        if self.blocks.len() > crate::CEILING_CURVES {
            return Err(SketchError::TooLarge);
        }
        let mut ids = HashSet::new();
        for b in &self.blocks {
            if !ids.insert(b.id.as_str()) {
                return Err(SketchError::Duplicate(b.id.clone()));
            }
            check_block(b)?;
        }
        // A closed block holds only constraints wholly inside it.
        let kinds: HashMap<&str, &ConstraintKind> = self.constraints.iter().map(|c| (c.id.as_str(), &c.kind)).collect();
        for b in self.blocks.iter().filter(|b| !b.is_open() && !b.held.is_empty()) {
            let reach = self.block_reach(b);
            if let Some(h) = b.held.iter().find(|h| kinds.get(h.as_str()).is_some_and(|k| !reach.holds(k))) {
                return Err(SketchError::Invalid(format!("block {} holds `{h}`, which is not wholly inside it", b.label())));
            }
        }
        Ok(())
    }
}

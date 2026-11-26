//! Sketch blocks: sets of sketch curves that keep their shape relative to each other while the
//! set as a whole moves, turns and, with [`BlockScale::Free`], scales.
//!
//! A block's pose lives in the two points of its own construction line, the axis: its start is
//! the block origin O, its end X. Every member point P is held at its captured place in the
//! frame, `P = O + u·d + v·perp(d)` with `d = X − O`, and every member circle or ellipse radius
//! at its captured share of the axis length. The constraints wholly inside the block are kept
//! but not solved while it exists ([`SketchBlock::held`]).

use serde::{Deserialize, Serialize};
use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;
use std::collections::{HashMap, HashSet};

use crate::model::{ConstraintKind, CurveKind, Result, Sketch, SketchError, in_range};

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
}

impl SketchBlock {
    /// The block's name, or its id when it has none.
    pub fn label(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
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
    if b.name.as_ref().is_some_and(|n| n.chars().count() > MAX_BLOCK_NAME) {
        return Err(SketchError::Invalid(format!("block `{}` has too long a name", b.id)));
    }
    let bounded = |v: f64| v.is_finite() && v.abs() < crate::MAX_COORD;
    if !(bounded(b.length) && b.length > 0.0) {
        return Err(SketchError::Invalid(format!("block `{}` has an invalid length", b.id)));
    }
    if b.points.iter().any(|(_, uv)| !uv.iter().all(|v| bounded(*v))) || b.radii.iter().any(|(_, k)| !bounded(*k)) {
        return Err(SketchError::Invalid(format!("block `{}` lies too far from its axis for its size", b.id)));
    }
    Ok(())
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

impl Sketch {
    /// The block that has curve `id` as a member, or as its axis.
    pub fn block_of(&self, id: &str) -> Option<&SketchBlock> {
        self.blocks.iter().find(|b| b.axis == id || b.curves.iter().any(|c| c == id))
    }

    /// The block that has curve `id` as a member (not as its axis).
    pub fn block_with_member(&self, id: &str) -> Option<&SketchBlock> {
        self.blocks.iter().find(|b| b.curves.iter().any(|c| c == id))
    }

    /// Why curve `id` cannot be changed on its own: it is a member of a block.
    pub fn block_refusal(&self, id: &str) -> Option<String> {
        let block = self.block_with_member(id)?;
        Some(format!("`{id}` is part of block {}: explode it, edit, then make the block again", block.label()))
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
        for b in &self.blocks {
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
        let members = self.block_members(ids, None)?;
        let points = self.member_points(&members);
        let (lo, hi) = self.members_box(&members).ok_or_else(|| SketchError::Invalid("the curves have no size to make a block of".into()))?;
        let diagonal = lo.dist(hi);
        if !(diagonal.is_finite() && diagonal > 1e-6) {
            return Err(SketchError::Invalid("the curves have no size to make a block of".into()));
        }
        let name = block_name(name)?;
        let at = match origin {
            Some(p) => self.point(p).ok_or_else(|| SketchError::Unknown(format!("point {p}")))?,
            None => lo.lerp(hi, 0.5),
        };
        let reach = points.iter().filter_map(|p| self.point(*p)).map(|p| p.dist(at)).fold(0.0_f64, f64::max);
        let length = tidy(reach.max(diagonal / 2.0));
        if !(length.is_finite() && length > 0.0) || !in_range(at) || !in_range(at + Vec2::new(length, 0.0)) {
            return Err(SketchError::Invalid("the block's axis would lie out of range".into()));
        }
        let o = self.add_point(at, None)?;
        let x = self.add_point(at + Vec2::new(length, 0.0), None)?;
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
        };
        self.capture(&mut block, &members)?;
        check_block(&block)?;
        let held = block.held.clone();
        self.blocks.push(block);
        Ok(BlockMade { block: id, axis, origin: origin_id, held })
    }

    /// [`Sketch::make_block`] on an existing line as the axis (its start the block origin), as
    /// a paste of a whole block with its axis does.
    pub fn make_block_on(&mut self, ids: &[String], axis: &str, scale: BlockScale, name: Option<String>) -> Result<BlockMade> {
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
        };
        self.capture(&mut block, &members)?;
        check_block(&block)?;
        let held = block.held.clone();
        self.blocks.push(block);
        Ok(BlockMade { block: id, axis: axis.into(), origin, held })
    }

    /// Is every point and curve the constraint `k` names a member of one block?
    pub fn inside_a_block(&self, k: &ConstraintKind) -> bool {
        !self.blocks.is_empty() && self.block_members_now(false).hold(k)
    }

    /// Each block's member points and curves; with `axes`, its axis line and points too.
    pub fn block_members_now(&self, axes: bool) -> BlockMembers {
        let curves: HashMap<&str, usize> = self.curves.iter().enumerate().map(|(i, c)| (c.id.as_str(), i)).collect();
        BlockMembers(
            self.blocks
                .iter()
                .map(|b| {
                    let mut members: Vec<usize> = b.curves.iter().filter_map(|c| curves.get(c.as_str()).copied()).collect();
                    if axes {
                        members.extend(curves.get(b.axis.as_str()));
                    }
                    (self.member_points(&members).into_iter().collect(), members.into_iter().collect())
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
            let members: Vec<usize> = b.curves.iter().filter_map(|c| self.curve_index(c)).collect();
            let added: Vec<String> = self.inside(&members).into_iter().filter(|c| !before.contains(c.as_str())).collect();
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
        for b in self.blocks.iter_mut().filter(|b| ids.contains(&b.id)) {
            b.length *= factor;
            check_block(b)?;
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
        let members: Vec<usize> = self.blocks.get(i).map(|b| b.curves.iter().filter_map(|c| self.curve_index(c)).collect()).unwrap_or_default();
        let inside: HashSet<String> = self.inside(&members).into_iter().collect();
        if let Some(b) = self.blocks.get_mut(i) {
            b.held = held.iter().filter(|h| inside.contains(*h)).cloned().collect();
        }
    }

    /// Capture `members` into `block` from where they and the block's axis are now: each point's
    /// place in the frame, each radius's share of the axis length, and the constraints held.
    fn capture(&self, block: &mut SketchBlock, members: &[usize]) -> Result<()> {
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
        let points = self.member_points(members);
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
        let inside = self.inside(members);
        // A constraint that was inside the block and drove while it existed keeps driving.
        let driving: HashSet<&str> = match self.blocks.iter().find(|b| b.id == block.id) {
            Some(old) => {
                let was: Vec<usize> = old.curves.iter().filter_map(|c| self.curve_index(c)).collect();
                self.inside(&was)
                    .into_iter()
                    .filter(|c| !old.held.contains(c))
                    .filter_map(|c| self.constraints.iter().find(|k| k.id == c))
                    .map(|c| c.id.as_str())
                    .collect()
            }
            None => HashSet::new(),
        };
        block.held = inside.into_iter().filter(|c| !driving.contains(c.as_str())).collect();
        Ok(())
    }

    /// The ids of the driving constraints wholly inside the curves `members`: every point and
    /// curve they name is a member or a member's point.
    fn inside(&self, members: &[usize]) -> Vec<String> {
        let member_points: HashSet<usize> = self.member_points(members).into_iter().collect();
        let member_curves: HashSet<usize> = members.iter().copied().collect();
        self.constraints
            .iter()
            .filter(|c| !c.driven && !ties_outside(&c.kind))
            .filter(|c| {
                let (ps, cs) = c.kind.indices();
                !(ps.is_empty() && cs.is_empty()) && ps.iter().all(|p| member_points.contains(p)) && cs.iter().all(|c| member_curves.contains(c))
            })
            .map(|c| c.id.clone())
            .collect()
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
        self.capture(&mut block, &members)?;
        check_block(&block)?;
        let gone: Vec<String> = old_held.into_iter().filter(|h| !block.held.contains(h)).collect();
        if let Some(slot) = self.blocks.get_mut(index) {
            *slot = block.clone();
        }
        let released = self.release(&gone);
        let origin = self
            .curve_index(&block.axis)
            .and_then(|c| self.curves.get(c))
            .and_then(|c| c.kind.ends())
            .and_then(|(o, _)| self.points.get(o))
            .map(|p| p.id.clone())
            .unwrap_or_default();
        Ok((BlockMade { block: block.id, axis: block.axis, origin, held: block.held }, released))
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
        let mut members = Vec::new();
        let mut seen = HashSet::new();
        for id in ids {
            if !seen.insert(id.as_str()) {
                continue;
            }
            let c = self.curve_index(id).ok_or_else(|| SketchError::Unknown(id.clone()))?;
            if let Some(b) = self.block_of(id).filter(|b| Some(b.id.as_str()) != except) {
                return Err(SketchError::Invalid(if b.axis == *id {
                    format!("`{id}` is the axis of block {}", b.label())
                } else {
                    format!("`{id}` is already part of block {}", b.label())
                }));
            }
            if self.blocks.iter().any(|b| b.axis == *id) {
                return Err(SketchError::Invalid(format!("`{id}` is the block's own axis")));
            }
            if self.is_text_curve(c) {
                return Err(SketchError::Invalid(format!("not supported yet: blocks of text (`{id}` is text)")));
            }
            if self.is_linked_curve(c) {
                return Err(SketchError::Invalid(format!("`{id}` is projected from the model: break the link before making a block of it")));
            }
            if self.offset_distance(id).is_some() {
                return Err(SketchError::Invalid(format!(
                    "`{id}` is the output of an offset: break the offset association before making a block of it"
                )));
            }
            if let Some(p) = self.patterns.iter().find(|p| p.contains(id)) {
                return Err(SketchError::Invalid(format!(
                    "not supported yet: blocks of sketch pattern geometry (`{id}` is part of pattern {})",
                    p.id
                )));
            }
            members.push(c);
        }
        let splines: HashSet<&str> = members.iter().filter_map(|c| self.curves.get(*c)).map(|c| c.id.as_str()).collect();
        let guides: Vec<usize> = self
            .curves
            .iter()
            .enumerate()
            .filter(|(i, c)| !members.contains(i) && c.spline_guide_owner().is_some_and(|o| splines.contains(o)))
            .map(|(i, _)| i)
            .collect();
        members.extend(guides);
        Ok(members)
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
        let axis_ok = |axis: &str| curves.get(axis).is_some_and(|c| matches!(c.kind, CurveKind::Line { a, b } if a != b));
        let mut empty = Vec::new();
        let mut kept = Vec::with_capacity(self.blocks.len());
        let mut loose = Vec::new();
        for mut b in std::mem::take(&mut self.blocks) {
            if !axis_ok(&b.axis) {
                loose.extend(b.held);
                continue;
            }
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
            b.points.retain(|(p, _)| used.contains(p.as_str()));
            b.radii.retain(|(c, _)| b.curves.contains(c));
            b.held.retain(|h| constraints.contains(h.as_str()));
            if b.curves.is_empty() {
                empty.push(b.axis);
            } else {
                kept.push(b);
            }
        }
        self.blocks = kept;
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
        Ok(())
    }
}

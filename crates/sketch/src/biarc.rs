//! Biarc spline parameters kept beside their generated sketch lines and circular arcs.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};
use solidcore_ops::prism;
use solidcore_record::{Seg2, Vec2};
use solidcraft_geom::prelude::*;

use crate::model::{Result, Sketch, SketchError, in_range, radius_in_range};
use crate::{Curve, CurveKind};

/// Maximum number of interpolation points in one biarc spline.
pub const MAX_BIARC_NODES: usize = crate::MAX_SPLINE_POINTS;

/// A point the spline interpolates and its tangent in the direction of travel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BiarcNode {
    pub point: String,
    pub tangent: Vec2,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<BiarcHandle>,
}

/// The point and construction line that give an interpolation point its tangent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BiarcHandle {
    pub point: String,
    pub curve: String,
}

/// Which end of a span, if either, must use a straight piece.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BiarcSpanMode {
    /// Fit both pieces using the span ratio.
    #[default]
    Auto,
    /// Start at the first node along a straight line.
    StraightStart,
    /// Reach the second node along a straight line.
    StraightEnd,
}

impl BiarcSpanMode {
    fn is_auto(&self) -> bool {
        *self == Self::Auto
    }
}

/// The pair of pieces between consecutive nodes, in spline order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BiarcSpan {
    pub ratio: f64,
    #[serde(default, skip_serializing_if = "BiarcSpanMode::is_auto")]
    pub mode: BiarcSpanMode,
    pub curves: [String; 2],
    /// Stable centre IDs, also when a piece is temporarily straight.
    #[serde(default, skip_serializing_if = "no_reserved_centers")]
    pub centers: [Option<String>; 2],
}

fn no_reserved_centers(centers: &[Option<String>; 2]) -> bool {
    centers.iter().all(Option::is_none)
}

/// A chain of biarcs. Point and curve identities can resolve after unrelated sketch entities are
/// removed and their indices change.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BiarcSpline {
    pub id: String,
    pub nodes: Vec<BiarcNode>,
    pub spans: Vec<BiarcSpan>,
}

impl BiarcSpline {
    /// The generated curves in spline order.
    pub fn curves(&self) -> impl Iterator<Item = &str> {
        self.spans.iter().flat_map(|span| span.curves.iter().map(String::as_str))
    }

    /// The construction lines that set the tangent directions.
    pub fn handle_curves(&self) -> impl Iterator<Item = &str> {
        self.nodes.iter().filter_map(|node| node.handle.as_ref()).map(|handle| handle.curve.as_str())
    }

    /// The member pieces and the construction lines that belong to this spline.
    pub fn all_curves(&self) -> impl Iterator<Item = &str> {
        self.curves().chain(self.handle_curves())
    }

    /// The centres reserved for the pieces, including when the corresponding piece is straight.
    pub fn reserved_centers(&self) -> impl Iterator<Item = &str> {
        self.spans.iter().flat_map(|span| span.centers.iter().flatten().map(String::as_str))
    }
}

fn invalid(message: impl Into<String>) -> SketchError {
    SketchError::Invalid(message.into())
}

struct SpanGeometry {
    joint: Vec2,
    pieces: [Seg2; 2],
}

/// The two nodes and forward tangents for one span, resolved once for repeated previews.
#[derive(Clone, Copy, Debug)]
pub struct BiarcSpanPreview {
    start: (Vec2, Vec2),
    end: (Vec2, Vec2),
    span: usize,
}

impl BiarcSpanPreview {
    /// Preview the join and its two pieces in spline order.
    pub fn geometry(self, ratio: f64, mode: BiarcSpanMode) -> Result<(Vec2, [Seg2; 2])> {
        let geometry = SpanGeometry::compute(self.start, self.end, ratio, mode, self.span)?;
        Ok((geometry.joint, geometry.pieces))
    }
}

/// One interval's inputs and placements, resolved once before a solve. The joint and centres
/// are derived from the interpolation points, tangent handles, and span parameters on each trial.
#[derive(Clone, Copy)]
pub(crate) struct BiarcSpanPlan {
    pub start: usize,
    pub end: usize,
    pub handles: [Option<usize>; 2],
    ratio: f64,
    mode: BiarcSpanMode,
    pub joint: usize,
    pub curves: [usize; 2],
    pub centers: [Option<usize>; 2],
    pub path: usize,
    span: usize,
    legacy: bool,
    tangents: [Vec2; 2],
}

impl BiarcSpanPlan {
    pub(crate) fn inputs(&self) -> impl Iterator<Item = usize> {
        [self.start, self.end].into_iter().chain(self.handles.into_iter().flatten())
    }

    pub(crate) fn geometry(&self, point: impl Fn(usize) -> Vec2) -> Result<(Vec2, [Seg2; 2])> {
        let a = point(self.start);
        let b = point(self.end);
        let [from, to] = self.handles;
        let tangent_a = from.map_or(self.tangents[0], |i| prism::chord(a, point(i)).0);
        let tangent_b = to.map_or(self.tangents[1], |i| prism::chord(b, point(i)).0);
        let geometry = SpanGeometry::compute((a, tangent_a), (b, tangent_b), self.ratio, self.mode, self.span)?;
        Ok((geometry.joint, geometry.pieces))
    }

    pub(crate) fn movable(&self) -> bool {
        !self.legacy
    }
}

impl SpanGeometry {
    fn compute((a, da): (Vec2, Vec2), (b, db): (Vec2, Vec2), ratio: f64, mode: BiarcSpanMode, index: usize) -> Result<Self> {
        let label = || format!("biarc span {}", index + 1);
        let (joint, pieces) = match mode {
            BiarcSpanMode::Auto => solidcore_ops::curve::biarc(a, da, b, db, ratio),
            BiarcSpanMode::StraightStart | BiarcSpanMode::StraightEnd => {
                solidcore_ops::curve::biarc_with_straight_piece(a, da, b, db, ratio, mode == BiarcSpanMode::StraightStart)
            }
        }
        .map_err(|error| invalid(format!("{}: {error}", label())))?;
        Sketch::check_pos(joint)?;
        for (piece, (start, end)) in pieces.iter().zip([(a, joint), (joint, b)]) {
            piece.check(crate::MAX_COORD).map_err(|error| invalid(format!("{}: {error}", label())))?;
            if let Seg2::Arc { center, radius, sweep, .. } = *piece
                && (!in_range(center)
                    || !radius_in_range(radius)
                    || sweep.abs() <= 1e-12
                    || sweep.abs() >= TAU - 1e-12
                    || !arc_keeps_biarc_shape(*piece, prism::stored_arc(center, start, end, sweep < 0.0)))
            {
                return Err(invalid(format!("{} cannot be stored as a sketch arc", label())));
            }
        }
        Ok(Self { joint, pieces })
    }
}

fn arc_keeps_biarc_shape(expected: Seg2, stored: Seg2) -> bool {
    matches!(stored, Seg2::Arc { radius, .. } if radius_in_range(radius))
        && prism::arc_contacts_match(&expected, &stored, crate::profiles::TANGENT_ANGLE_TOL)
}

fn same_position(actual: Vec2, expected: Vec2) -> bool {
    // A saved sketch can be opened on a platform with slightly different trigonometric rounding.
    in_range(actual) && prism::points_agree(actual, expected, 1e-12)
}

fn fixed_point(sk: &Sketch, point: usize) -> Result<Vec2> {
    sk.points
        .get(point)
        .filter(|point| point.fixed && point.link.is_none() && point.exprs.is_none() && in_range(point.pos))
        .map(|point| point.pos)
        .ok_or_else(|| invalid("missing or changed biarc point"))
}

fn input_point(sk: &Sketch, point: usize) -> Result<Vec2> {
    sk.points
        .get(point)
        .filter(|point| point.link.is_none() && in_range(point.pos))
        .map(|point| point.pos)
        .ok_or_else(|| invalid("missing or changed biarc input point"))
}

fn drawn_ends(curve: &Curve) -> Option<(usize, usize)> {
    match curve.kind {
        CurveKind::Line { a, b } if !curve.reversed => Some((a, b)),
        CurveKind::Arc { a, b, .. } if curve.reversed => Some((b, a)),
        CurveKind::Arc { a, b, .. } => Some((a, b)),
        _ => None,
    }
}

fn add_fixed_point(sk: &mut Sketch, at: Vec2, max_points: usize) -> Result<usize> {
    let point = sk.add_point_limited(at, max_points)?;
    sk.points.get_mut(point).ok_or_else(|| invalid("missing new biarc point"))?.fixed = true;
    Ok(point)
}

fn point_id(sk: &Sketch, point: usize) -> Result<&str> {
    sk.points.get(point).map(|point| point.id.as_str()).ok_or_else(|| invalid("missing biarc point"))
}

fn add_handle(sk: &mut Sketch, node: usize, at: Vec2, tangent: Vec2, max_points: usize) -> Result<(BiarcHandle, Vec2)> {
    if !tangent.is_finite() || tangent == Vec2::ZERO {
        return Err(invalid("invalid biarc tangent"));
    }
    // Tangent magnitude is immaterial to the interpolation. Use it as the drawn handle
    // length when it can be positioned at model precision.
    let tip = prism::tangent_guide_tip(at, tangent, 1e-6, &[1.0, 0.1, 0.01, 0.001, 0.0001, 0.00001], crate::MAX_COORD)
        .ok_or_else(|| invalid("cannot place a biarc tangent handle within sketch limits"))?;
    let point = sk.add_point_limited(tip, max_points)?;
    let id = sk.fresh("h")?;
    let curve = sk.add_line_pts(node, point, Some(&id))?;
    sk.curves.get_mut(curve).ok_or_else(|| invalid("missing new biarc guide"))?.construction = true;
    Ok((BiarcHandle { point: point_id(sk, point)?.to_string(), curve: id }, prism::chord(at, tip).0))
}

fn piece_kind(piece: Seg2, a: usize, b: usize, center: Option<usize>) -> Result<(CurveKind, bool)> {
    match piece {
        Seg2::Line { .. } => Ok((CurveKind::Line { a, b }, false)),
        Seg2::Arc { sweep, .. } => {
            let c = center.ok_or_else(|| invalid("missing reserved biarc center"))?;
            let reversed = sweep < 0.0;
            let (a, b) = if reversed { (b, a) } else { (a, b) };
            Ok((CurveKind::Arc { c, a, b }, reversed))
        }
        _ => Err(invalid("a biarc piece must be a line or circular arc")),
    }
}

fn piece_center(piece: Seg2, joint: Vec2) -> Vec2 {
    match piece {
        Seg2::Arc { center, .. } => center,
        _ => joint,
    }
}

impl Sketch {
    /// The spline given its id or the id of one of its generated pieces.
    pub fn biarc_spline(&self, id_or_member: &str) -> Option<&BiarcSpline> {
        self.biarc_splines.iter().find(|spline| spline.id == id_or_member || spline.curves().any(|id| id == id_or_member))
    }

    /// Find the spline that owns a construction line used to edit its tangent.
    pub fn biarc_guide_spline(&self, id: &str) -> Option<&BiarcSpline> {
        self.biarc_splines.iter().find(|spline| spline.handle_curves().any(|guide| guide == id))
    }

    fn biarc_edit_target(&self, id_or_member: &str) -> Result<&BiarcSpline> {
        self.biarc_spline(id_or_member).or_else(|| self.biarc_guide_spline(id_or_member)).ok_or_else(|| SketchError::Unknown(id_or_member.into()))
    }

    /// Resolve a span's inputs for repeated ratio or mode previews. Span indices start at zero.
    pub fn biarc_span_preview(&self, id_or_member: &str, span: usize) -> Result<BiarcSpanPreview> {
        let spline = self.biarc_edit_target(id_or_member)?;
        let [left, right] = spline
            .nodes
            .get(span..)
            .and_then(|nodes| nodes.first_chunk::<2>())
            .filter(|_| span < spline.spans.len())
            .ok_or_else(|| invalid("biarc span index is out of range"))?;
        let resolve = |node: &BiarcNode| -> Result<(Vec2, Vec2)> {
            let index = self.point_index(&node.point).ok_or_else(|| invalid("missing biarc node"))?;
            if let Some(handle) = &node.handle {
                let handle = self.point_index(&handle.point).ok_or_else(|| invalid("missing biarc handle point"))?;
                let position = input_point(self, index)?;
                Ok((position, prism::chord(position, input_point(self, handle)?).0))
            } else {
                Ok((fixed_point(self, index)?, node.tangent))
            }
        };
        Ok(BiarcSpanPreview { start: resolve(left)?, end: resolve(right)?, span })
    }

    /// Preview one span without editing the sketch or running the solver.
    pub fn preview_biarc_span(&self, id_or_member: &str, span: usize, ratio: f64, mode: BiarcSpanMode) -> Result<(Vec2, [Seg2; 2])> {
        self.biarc_span_preview(id_or_member, span)?.geometry(ratio, mode)
    }

    /// Add a biarc spline through positions and forward tangents. Every span defaults to an equal
    /// split of its tangent distances. The interpolation nodes and tangent handles can be moved or
    /// constrained in the sketch.
    pub fn add_biarc_spline(&mut self, nodes: &[(Vec2, Vec2)], ratios: Option<&[f64]>, construction: bool) -> Result<BiarcSpline> {
        if !(2..=MAX_BIARC_NODES).contains(&nodes.len()) {
            return Err(invalid(format!("a biarc spline needs 2 to {MAX_BIARC_NODES} points")));
        }
        let span_count = nodes.len() - 1;
        if ratios.is_some_and(|r| r.len() != span_count) {
            return Err(invalid("a biarc spline needs one ratio per span"));
        }
        if self.curves.len().saturating_add(2 * span_count + nodes.len()) > crate::limits::max_curves() {
            return Err(SketchError::TooLarge);
        }
        let max_points = crate::limits::max_points();
        let mut candidate = self.clone();
        let id = candidate.fresh("biarc")?;
        let mut inputs = Vec::with_capacity(nodes.len());
        let mut records = Vec::with_capacity(nodes.len());
        for &(at, tangent) in nodes {
            let index = candidate.add_point_limited(at, max_points)?;
            let point = point_id(&candidate, index)?.to_string();
            let (handle, current) = add_handle(&mut candidate, index, at, tangent, max_points)?;
            inputs.push((index, (at, current)));
            records.push(BiarcNode { point, tangent, handle: Some(handle) });
        }
        let mut spans = Vec::with_capacity(span_count);
        for (i, (&(a, left), &(b, right))) in inputs.iter().zip(inputs.iter().skip(1)).enumerate() {
            let ratio = ratios.and_then(|ratios| ratios.get(i)).copied().unwrap_or(0.5);
            let mode = BiarcSpanMode::Auto;
            let geometry = SpanGeometry::compute(left, right, ratio, mode, i)?;
            let joint = add_fixed_point(&mut candidate, geometry.joint, max_points)?;
            let add = |sk: &mut Sketch, piece, a, b| -> Result<(String, Option<String>)> {
                let center = add_fixed_point(sk, piece_center(piece, geometry.joint), max_points)?;
                let (kind, reversed) = piece_kind(piece, a, b, Some(center))?;
                let index = sk.add_curve(kind, None)?;
                let curve = sk.curves.get_mut(index).ok_or_else(|| invalid("missing new biarc curve"))?;
                curve.reversed = reversed;
                curve.construction = construction;
                let curve = curve.id.clone();
                Ok((curve, Some(point_id(sk, center)?.to_string())))
            };
            let [left, right] = geometry.pieces;
            let (left, center_left) = add(&mut candidate, left, a, joint)?;
            let (right, center_right) = add(&mut candidate, right, joint, b)?;
            spans.push(BiarcSpan { ratio, mode, curves: [left, right], centers: [center_left, center_right] });
        }
        let spline = BiarcSpline { id, nodes: records, spans };
        candidate.biarc_splines.push(spline.clone());
        *self = candidate;
        Ok(spline)
    }

    /// Set one span's ratio. Span indices start at zero.
    pub fn set_biarc_ratio(&mut self, id_or_member: &str, span: usize, ratio: f64) -> Result<BiarcSpline> {
        self.set_biarc_span(id_or_member, span, Some(ratio), None)
    }

    /// Change one span's ratio or mode, preserving the generated point and curve identities.
    /// The nodes and tangents determine the join for a line with an arc. The stored ratio
    /// continues to apply to automatic fits and pairs of collinear lines.
    pub fn set_biarc_span(&mut self, id_or_member: &str, span: usize, ratio: Option<f64>, mode: Option<BiarcSpanMode>) -> Result<BiarcSpline> {
        if ratio.is_none() && mode.is_none() {
            return Err(invalid("a biarc span edit needs a ratio or a mode"));
        }
        self.edit_biarc_spans(id_or_member, |settings| {
            let (previous_ratio, previous_mode) = settings.get_mut(span).ok_or_else(|| invalid("biarc span index is out of range"))?;
            *previous_ratio = ratio.unwrap_or(*previous_ratio);
            *previous_mode = mode.unwrap_or(*previous_mode);
            Ok(())
        })
    }

    /// Set all of the spline's ratios in one edit. Each piece keeps its curve identity and its
    /// start and end identities, so adjacent sketch geometry can continue to share those points.
    pub fn set_biarc_ratios(&mut self, id_or_member: &str, ratios: &[f64]) -> Result<BiarcSpline> {
        self.edit_biarc_spans(id_or_member, |settings| {
            if ratios.len() != settings.len() {
                return Err(invalid("a biarc spline needs one ratio per span"));
            }
            for ((previous, _), &ratio) in settings.iter_mut().zip(ratios) {
                *previous = ratio;
            }
            Ok(())
        })
    }

    fn edit_biarc_spans(&mut self, id_or_member: &str, update: impl FnOnce(&mut [(f64, BiarcSpanMode)]) -> Result<()>) -> Result<BiarcSpline> {
        self.validate_biarc_splines()?;
        let original = self.biarc_edit_target(id_or_member)?;
        let parameters = |span: &BiarcSpan| (span.ratio, span.mode);
        let mut settings = original.spans.iter().map(parameters).collect::<Vec<_>>();
        update(&mut settings)?;
        if original.spans.iter().map(parameters).eq(settings.iter().copied()) {
            return Ok(original.clone());
        }
        let mut candidate = self.clone();
        candidate.upgrade_legacy_biarc_splines()?;
        let record = candidate.biarc_splines.iter_mut().find(|record| record.id == original.id).ok_or_else(|| invalid("missing biarc spline"))?;
        for (span, (ratio, mode)) in record.spans.iter_mut().zip(settings) {
            span.ratio = ratio;
            span.mode = mode;
        }
        let spline = record.clone();
        candidate.refresh_biarc_splines()?;
        *self = candidate;
        Ok(spline)
    }

    fn check_biarc_edit(&self, retired: &HashSet<usize>, changed: &HashMap<usize, CurveKind>) -> Result<()> {
        if changed.is_empty() {
            return Ok(());
        }
        if changed.keys().any(|&i| self.curves.get(i).is_some_and(|curve| self.patterns.iter().any(|pattern| pattern.entities.contains(&curve.id)))) {
            return Err(invalid("a patterned biarc piece cannot change between an arc and a line"));
        }
        if !retired.is_empty()
            && self.curves.iter().enumerate().any(|(i, curve)| changed.get(&i).unwrap_or(&curve.kind).point_ids().iter().any(|p| retired.contains(p)))
        {
            return Err(invalid("a biarc arc cannot become a line while its center is shared with other curves"));
        }
        for constraint in &self.constraints {
            let (points, curves) = constraint.kind.indices();
            if points.iter().any(|p| retired.contains(p)) {
                return Err(invalid("a biarc arc cannot become a line while its center is constrained"));
            }
            if curves.iter().any(|c| changed.contains_key(c)) && !self.biarc_transition_preserves(&constraint.kind, changed) {
                return Err(invalid("a constrained biarc piece cannot change between an arc and a line"));
            }
        }
        Ok(())
    }

    /// Test only kind requirements that an arc-to-line transition can change. The solver
    /// checks the values after materialization, including constraints shared with other curves.
    fn biarc_transition_preserves(&self, kind: &crate::ConstraintKind, changed: &HashMap<usize, CurveKind>) -> bool {
        use crate::ConstraintKind::*;
        let shape = |index: usize| changed.get(&index).or_else(|| self.curves.get(index).map(|curve| &curve.kind));
        let is_line = |index: usize| matches!(shape(index), Some(CurveKind::Line { .. }));
        let is_arc = |index: usize| matches!(shape(index), Some(CurveKind::Arc { .. }));
        let is_round = |index: usize| matches!(shape(index), Some(CurveKind::Circle { .. } | CurveKind::Arc { .. }));
        match *kind {
            Horizontal { l }
            | Vertical { l }
            | Length { l, .. }
            | LineAngle { l, .. }
            | Midpoint { l, .. }
            | Symmetric { l, .. }
            | PointLineDistance { l, .. }
            | LinearDiameter { l, .. } => is_line(l),
            ArcLength { c, .. } | ArcSweep { c, .. } => is_arc(c),
            Radius { c, .. } => is_round(c) || matches!(shape(c), Some(CurveKind::Ellipse { .. })),
            Diameter { c, .. } | PointCircleDistance { c, .. } => is_round(c),
            Parallel { a, b } | Perpendicular { a, b } | Collinear { a, b } | Angle { a, b, .. } => is_line(a) && is_line(b),
            Concentric { a, b } | CircleCircleDistance { a, b, .. } => is_round(a) && is_round(b),
            SlotOverallLength { a, b, .. } => is_arc(a) && is_arc(b),
            LineCircleDistance { l, c, .. } => is_line(l) && is_round(c),
            PatternPoint { directions, .. } => directions.iter().flatten().all(|(line, _)| is_line(*line)),
            PatternRadius { .. } => false,
            Equal { a, b } => is_line(a) == is_line(b),
            Tangent { a, b } => [a, b].into_iter().any(|i| is_round(i) || self.curves.get(i).is_some_and(|curve| curve.kind.is_freeform())),
            PointAlong { p, c, .. } => shape(c).is_some_and(|curve| !curve.uses(p)),
            _ => true,
        }
    }

    /// Resolve the indexed inputs and outputs for this sketch's biarc splines. This checks the
    /// stored topology without requiring the dependent positions to match the current inputs.
    pub(crate) fn biarc_span_plans(&self) -> Result<Vec<BiarcSpanPlan>> {
        if self.biarc_splines.is_empty() {
            return Ok(Vec::new());
        }
        if self.biarc_splines.len() > self.curves.len() / 2 {
            return Err(invalid("invalid biarc spline count"));
        }
        let mut point_index = HashMap::new();
        let mut curve_index = HashMap::new();
        for (index, point) in self.points.iter().enumerate() {
            point_index.entry(point.id.as_str()).and_modify(|slot| *slot = usize::MAX).or_insert(index);
        }
        for (index, curve) in self.curves.iter().enumerate() {
            curve_index.entry(curve.id.as_str()).and_modify(|slot| *slot = usize::MAX).or_insert(index);
        }
        let group_ids = self.biarc_splines.iter().map(|spline| spline.id.as_str()).collect::<HashSet<_>>();
        if group_ids.len() != self.biarc_splines.len() {
            return Err(invalid("duplicate biarc spline id"));
        }
        let mut seen_groups = HashSet::new();
        for id in self.all_ids().filter(|id| group_ids.contains(id)) {
            if !seen_groups.insert(id) {
                return Err(SketchError::Duplicate(id.into()));
            }
        }
        let mut owned_points = HashSet::new();
        let mut dependent = HashSet::new();
        let mut owned_curves = HashSet::new();
        let mut plans = Vec::new();
        for (path, spline) in self.biarc_splines.iter().enumerate() {
            if spline.id.is_empty() || !(2..=MAX_BIARC_NODES).contains(&spline.nodes.len()) || spline.spans.len() != spline.nodes.len() - 1 {
                return Err(invalid("invalid biarc spline record"));
            }
            let legacy = spline.nodes.iter().all(|node| node.handle.is_none());
            let points = spline
                .nodes
                .iter()
                .map(|node| {
                    let a = *point_index.get(node.point.as_str()).ok_or_else(|| invalid("missing biarc node"))?;
                    if a == 0 || !owned_points.insert(a) {
                        return Err(invalid("biarc interpolation points must be distinct"));
                    }
                    let handle = if legacy {
                        fixed_point(self, a)?;
                        None
                    } else {
                        input_point(self, a)?;
                        let handle = node.handle.as_ref().ok_or_else(|| invalid("missing biarc tangent handle"))?;
                        let b = *point_index.get(handle.point.as_str()).ok_or_else(|| invalid("missing biarc handle point"))?;
                        if b == 0 || !owned_points.insert(b) {
                            return Err(invalid("biarc handle points must be distinct"));
                        }
                        input_point(self, b)?;
                        let ci = *curve_index.get(handle.curve.as_str()).ok_or_else(|| invalid("missing biarc guide"))?;
                        let curve = self.curves.get(ci).ok_or_else(|| invalid("missing or duplicate biarc guide"))?;
                        if curve.kind != (CurveKind::Line { a, b })
                            || !curve.construction
                            || curve.link.is_some()
                            || curve.infinite
                            || curve.centerline
                            || curve.reversed
                            || curve.spline_handle_of.is_some()
                            || !owned_curves.insert(ci)
                        {
                            return Err(invalid("invalid biarc guide"));
                        }
                        Some(b)
                    };
                    Ok((a, handle, node.tangent))
                })
                .collect::<Result<Vec<_>>>()?;
            let mut construction = None;
            for (i, ((&(a, left, da), &(b, right, db)), span)) in points.iter().zip(points.iter().skip(1)).zip(&spline.spans).enumerate() {
                let [id0, id1] = &span.curves;
                let index0 = *curve_index.get(id0.as_str()).ok_or_else(|| invalid("missing biarc curve"))?;
                let index1 = *curve_index.get(id1.as_str()).ok_or_else(|| invalid("missing biarc curve"))?;
                let c0 = self.curves.get(index0).ok_or_else(|| invalid("missing or duplicate biarc curve"))?;
                let c1 = self.curves.get(index1).ok_or_else(|| invalid("missing or duplicate biarc curve"))?;
                if !owned_curves.insert(index0) || !owned_curves.insert(index1) {
                    return Err(invalid("biarc curves must be distinct"));
                }
                let (start, joint) = drawn_ends(c0).ok_or_else(|| invalid("changed biarc curve"))?;
                let (other_joint, end) = drawn_ends(c1).ok_or_else(|| invalid("changed biarc curve"))?;
                if start != a || end != b || joint != other_joint || joint == 0 || !owned_points.insert(joint) {
                    return Err(invalid("invalid biarc spline junction"));
                }
                fixed_point(self, joint)?;
                dependent.insert(joint);
                let resolve_center = |curve: &Curve, center_id: &Option<String>| {
                    if curve.construction != *construction.get_or_insert(curve.construction) {
                        return Err(invalid("the pieces of a biarc spline must share one line type"));
                    }
                    if curve.fixed != legacy || curve.link.is_some() || curve.infinite || curve.centerline || curve.spline_handle_of.is_some() {
                        return Err(invalid("changed biarc curve flags"));
                    }
                    let center = match curve.kind {
                        CurveKind::Arc { c, .. } => Some(c),
                        _ => None,
                    };
                    let reserved = match (legacy, center_id) {
                        (true, None) => center,
                        (false, Some(id)) => {
                            let index = *point_index.get(id.as_str()).ok_or_else(|| invalid("missing biarc arc center"))?;
                            if center.is_some_and(|c| c != index) {
                                return Err(invalid("a biarc arc must use its reserved center"));
                            }
                            Some(index)
                        }
                        _ => return Err(invalid("invalid biarc arc center reservation")),
                    };
                    if let Some(reserved) = reserved {
                        if reserved == 0 || !owned_points.insert(reserved) {
                            return Err(invalid("invalid biarc arc center"));
                        }
                        fixed_point(self, reserved)?;
                        dependent.insert(reserved);
                    }
                    Ok(reserved)
                };
                let [left_center, right_center] = &span.centers;
                let mut resolve_center = resolve_center;
                let centers = [resolve_center(c0, left_center)?, resolve_center(c1, right_center)?];
                plans.push(BiarcSpanPlan {
                    start: a,
                    end: b,
                    handles: [left, right],
                    ratio: span.ratio,
                    mode: span.mode,
                    joint,
                    curves: [index0, index1],
                    centers,
                    path,
                    span: i,
                    legacy,
                    tangents: [da, db],
                });
            }
        }
        if self.constraints.iter().any(|constraint| {
            matches!(constraint.kind, crate::ConstraintKind::Fix { p } | crate::ConstraintKind::Pierce { p, .. } if dependent.contains(&p))
        }) {
            return Err(invalid("a derived biarc point cannot be fixed or pierced"));
        }
        Ok(plans)
    }

    /// Check that the stored joint, curves and centers agree with their exact biarc geometry.
    pub fn validate_biarc_splines(&self) -> Result<()> {
        for plan in self.biarc_span_plans()? {
            let (joint, pieces) = plan.geometry(|point| self.point(point).unwrap_or_default())?;
            if !same_position(fixed_point(self, plan.joint)?, joint) {
                return Err(invalid("biarc spline junction disagrees with its inputs"));
            }
            for ((index, center), piece) in plan.curves.into_iter().zip(plan.centers).zip(pieces) {
                let curve = self.curves.get(index).ok_or_else(|| invalid("missing biarc curve"))?;
                match (&curve.kind, piece) {
                    (CurveKind::Line { .. }, Seg2::Line { .. }) => {}
                    (CurveKind::Arc { .. }, Seg2::Arc { sweep, .. }) if curve.reversed == (sweep < 0.0) => {
                        let stored = self.segs(index).into_iter().next().ok_or_else(|| invalid("missing biarc arc"))?;
                        let stored = if curve.reversed { stored.reversed() } else { stored };
                        if !arc_keeps_biarc_shape(piece, stored) {
                            return Err(invalid("stored biarc arc loses its tangent direction"));
                        }
                    }
                    _ => return Err(invalid("biarc spline piece disagrees with its inputs")),
                }
                if let Some(center) = center
                    && !same_position(fixed_point(self, center)?, piece_center(piece, joint))
                {
                    return Err(invalid("biarc spline center disagrees with its inputs"));
                }
            }
        }
        Ok(())
    }

    /// Keep the stored joins and pieces at their regenerated positions. Resolve every changed
    /// span before writing so one invalid span leaves the entire materialization untouched.
    pub fn refresh_biarc_splines(&mut self) -> Result<()> {
        let plans = self.biarc_span_plans()?;
        self.place_biarc_plan(&plans)
    }

    pub(crate) fn place_biarc_plan(&mut self, plans: &[BiarcSpanPlan]) -> Result<()> {
        let mut points = Vec::with_capacity(plans.len().saturating_mul(3));
        let mut curves = Vec::with_capacity(plans.len().saturating_mul(2));
        let mut retired = HashSet::new();
        let mut changed = HashMap::new();
        for plan in plans {
            let (joint, pieces) = plan.geometry(|point| self.point(point).unwrap_or_default())?;
            if self.points.get(plan.joint).is_none() {
                return Err(invalid("missing biarc joint"));
            }
            points.push((plan.joint, joint));
            for (((index, center), piece), (a, b)) in
                plan.curves.into_iter().zip(plan.centers).zip(pieces).zip([(plan.start, plan.joint), (plan.joint, plan.end)])
            {
                let curve = self.curves.get(index).ok_or_else(|| invalid("missing biarc curve"))?;
                let (kind, reversed) = piece_kind(piece, a, b, center)?;
                let is_arc = matches!(kind, CurveKind::Arc { .. });
                let was_arc = matches!(curve.kind, CurveKind::Arc { .. });
                if was_arc != is_arc {
                    if plan.legacy {
                        return Err(invalid("the fixed biarc has no reserved arc center"));
                    }
                    changed.insert(index, kind.clone());
                    if let (true, Some(c)) = (was_arc, center) {
                        retired.insert(c);
                    }
                }
                if let Some(center) = center {
                    if self.points.get(center).is_none() {
                        return Err(invalid("missing biarc center"));
                    }
                    points.push((center, piece_center(piece, joint)));
                }
                curves.push((index, kind, reversed));
            }
        }
        self.check_biarc_edit(&retired, &changed)?;
        // Each index was checked above, and no point or curve is added or removed here.
        for (index, position) in points {
            if let Some(point) = self.points.get_mut(index) {
                point.pos = position;
            }
        }
        for (index, kind, reversed) in curves {
            if let Some(curve) = self.curves.get_mut(index) {
                curve.kind = kind;
                curve.reversed = reversed;
            }
        }
        Ok(())
    }

    /// Convert a valid older spline with fixed interpolation points to editable points and
    /// tangent handles, retaining its existing curve IDs, joint IDs and arc center IDs.
    pub fn upgrade_legacy_biarc_splines(&mut self) -> Result<()> {
        if !self.biarc_splines.iter().any(|spline| spline.nodes.iter().any(|node| node.handle.is_none())) {
            return Ok(());
        }
        self.validate_biarc_splines()?;
        let plans = self.biarc_span_plans()?;
        let mut candidate = self.clone();
        for (path, mut spline) in
            self.biarc_splines.iter().cloned().enumerate().filter(|(_, spline)| spline.nodes.iter().all(|node| node.handle.is_none()))
        {
            if candidate.curves.len().saturating_add(spline.nodes.len()) > crate::CEILING_CURVES {
                return Err(SketchError::TooLarge);
            }
            let nodes = spline
                .nodes
                .iter()
                .map(|node| {
                    let i = candidate.point_index(&node.point).ok_or_else(|| invalid("missing biarc node"))?;
                    Ok((i, fixed_point(&candidate, i)?))
                })
                .collect::<Result<Vec<_>>>()?;
            for (i, (node, &(index, at))) in spline.nodes.iter_mut().zip(&nodes).enumerate() {
                // The saved magnitude had no effect on the old spline. Scale short handles
                // with the nearer span so their tips start clear of the fit point.
                let span = [i.checked_sub(1), Some(i + 1)]
                    .into_iter()
                    .flatten()
                    .filter_map(|i| nodes.get(i).map(|(_, next)| prism::chord(at, *next).1))
                    .fold(f64::INFINITY, f64::min);
                let guide = prism::lengthen_direction(node.tangent, 0.2 * span);
                let point = candidate.points.get_mut(index).ok_or_else(|| invalid("missing biarc node"))?;
                point.fixed = false;
                let (handle, _) = add_handle(&mut candidate, index, at, guide, crate::CEILING_POINTS)?;
                node.handle = Some(handle);
            }
            for plan in plans.iter().filter(|plan| plan.path == path) {
                let span = spline.spans.get_mut(plan.span).ok_or_else(|| invalid("missing biarc span"))?;
                let joint = candidate.point(plan.joint).ok_or_else(|| invalid("missing biarc joint"))?;
                for ((index, stored), slot) in plan.curves.into_iter().zip(plan.centers).zip(&mut span.centers) {
                    let center = match stored {
                        Some(index) => index,
                        None => add_fixed_point(&mut candidate, joint, crate::CEILING_POINTS)?,
                    };
                    *slot = Some(point_id(&candidate, center)?.to_string());
                    candidate.curves.get_mut(index).ok_or_else(|| invalid("missing biarc curve"))?.fixed = false;
                }
            }
            *candidate.biarc_splines.get_mut(path).ok_or_else(|| invalid("missing biarc spline"))? = spline;
        }
        candidate.refresh_biarc_splines()?;
        *self = candidate;
        Ok(())
    }

    pub(crate) fn biarc_owns_point(&self, point: usize) -> bool {
        self.biarc_dependent_points().get(point).copied().unwrap_or(false)
    }

    /// Mark the derived junctions and centers of biarc splines, including centers that a line
    /// doesn't currently use.
    pub fn biarc_dependent_points(&self) -> Vec<bool> {
        let mut out = vec![false; self.points.len()];
        if self.biarc_splines.is_empty() {
            return out;
        }
        let mark = |out: &mut [bool], index| {
            if let Some(slot) = out.get_mut(index) {
                *slot = true;
            }
        };
        let pieces: HashMap<_, _> = self.curves.iter().map(|curve| (curve.id.as_str(), curve)).collect();
        let points: HashMap<_, _> = self.points.iter().enumerate().map(|(index, point)| (point.id.as_str(), index)).collect();
        for spline in &self.biarc_splines {
            for span in &spline.spans {
                let [left, right] = &span.curves;
                if let Some((_, joint)) = pieces.get(left.as_str()).and_then(|curve| drawn_ends(curve)) {
                    mark(&mut out, joint);
                }
                for id in [left, right] {
                    if let Some(CurveKind::Arc { c, .. }) = pieces.get(id.as_str()).map(|curve| &curve.kind) {
                        mark(&mut out, *c);
                    }
                }
                for id in span.centers.iter().flatten() {
                    if let Some(&index) = points.get(id.as_str()) {
                        mark(&mut out, index);
                    }
                }
            }
        }
        out
    }

    pub(crate) fn include_biarc_members(&self, which: &mut Vec<usize>) {
        if which.is_empty() || self.biarc_splines.is_empty() {
            return;
        }
        let selected: HashSet<_> = which.iter().filter_map(|&i| self.curves.get(i).map(|curve| curve.id.as_str())).collect();
        let related: HashSet<_> = self
            .biarc_splines
            .iter()
            .filter(|spline| spline.all_curves().any(|id| selected.contains(id)))
            .flat_map(BiarcSpline::all_curves)
            .collect();
        which.extend(self.curves.iter().enumerate().filter_map(|(index, curve)| related.contains(curve.id.as_str()).then_some(index)));
    }

    pub(crate) fn include_biarc_points(&self, which_curves: &mut Vec<usize>, which_points: &[usize]) {
        if which_points.is_empty() || self.biarc_splines.is_empty() {
            return;
        }
        // The reserved center of a line owns no curve directly but still belongs to its group.
        let selected: HashSet<_> = which_points.iter().filter_map(|&i| self.points.get(i).map(|point| point.id.as_str())).collect();
        let related: HashSet<_> = self
            .biarc_splines
            .iter()
            .filter(|spline| spline.reserved_centers().any(|id| selected.contains(id)))
            .flat_map(BiarcSpline::all_curves)
            .collect();
        which_curves.extend(self.curves.iter().enumerate().filter_map(|(index, curve)| related.contains(curve.id.as_str()).then_some(index)));
    }

    pub(crate) fn prune_biarc_splines(&mut self, removed: &BTreeSet<String>) {
        if removed.is_empty() || self.biarc_splines.is_empty() {
            return;
        }
        let selected = |spline: &BiarcSpline| spline.all_curves().any(|id| removed.contains(id));
        let released: HashSet<_> = self.biarc_splines.iter().filter(|spline| selected(spline)).flat_map(BiarcSpline::all_curves).collect();
        let reserved: HashSet<_> = self.biarc_splines.iter().filter(|spline| selected(spline)).flat_map(BiarcSpline::reserved_centers).collect();
        for curve in &self.curves {
            if released.contains(curve.id.as_str()) {
                for index in curve.kind.point_ids() {
                    if index != 0
                        && let Some(point) = self.points.get_mut(index)
                    {
                        point.fixed = false;
                    }
                }
            }
        }
        for point in &mut self.points {
            if reserved.contains(point.id.as_str()) {
                point.fixed = false;
            }
        }
        self.biarc_splines.retain(|spline| !selected(spline));
    }
}

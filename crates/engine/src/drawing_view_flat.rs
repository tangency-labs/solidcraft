//! Flat pattern views (#13030): a sheet-metal body's flat as `sheet.flat_pattern` reports it
//! ([`crate::cmd::flat_reply`], the one reader), lying on its own plane turned a [`QuarterTurn`] at a
//! time. Its outlines are visible lines through the reply's points (within 0.001 mm), its cut-outs
//! visible lines along their exact shapes, so a round hole stays one circle. A bend line bent up is a
//! centre line, one bent down a phantom line, both narrow (ISO 128 types 04 and 05). A flat has no
//! hidden lines, so none are worked out and no model is evaluated.

use serde_json::Value;
use solidcore_record::{Seg2, Vec2};
use solidcraft_doc::drawing::view::FlatOf;
use solidcraft_geom::Aabb2;
use solidcraft_geom::drawing_view::{QuarterTurn, ViewFrame};
use solidcraft_geom::paper::line::{LineGroup, LineKind};
use solidcraft_geom::paper::{Dashes, Group, Item, ObjectKind};
use solidcraft_geom::prelude::*;

use super::lines::drawable;
use crate::params::bad;
use crate::{Result, Session};

/// A flat pattern view's flat on the view's plane: turned, not yet scaled or placed.
#[derive(Clone, Debug)]
pub struct Flat {
    /// Each loop of the flat, its outlines first, then its cut-outs.
    loops: Vec<Vec<Seg2>>,
    /// Each bend line's ends, and whether it bends up.
    bends: Vec<(Vec2, Vec2, bool)>,
    /// The box round its outlines.
    pub model: Aabb2,
}

/// The flat of `of`'s body in `s`'s design, turned as `of` is; refused in `sheet.flat_pattern`'s
/// words when the body is no sheet metal, or its flat doesn't match it.
pub fn flat(s: &Session, of: &FlatOf, cmd: &str) -> Result<Flat> {
    flat_from(&crate::cmd::flat_reply(s, Some(&of.body), cmd)?, of.turn, cmd)
}

/// The flat `reply` (`sheet.flat_pattern`'s) reports, turned `turn`.
pub fn flat_from(reply: &Value, turn: QuarterTurn, cmd: &str) -> Result<Flat> {
    let unread = |key: &str| bad(cmd, format!("the flat pattern's `{key}` can't be read"));
    let turn = Turn::new(turn);
    let outlines = list(reply, "outline").ok_or_else(|| unread("outline"))?;
    let outlines = outlines.iter().map(|ring| points(ring, &turn).map(|ring| closed(&ring))).collect::<Option<Vec<_>>>();
    let outlines = outlines.ok_or_else(|| unread("outline"))?;
    let model = outlines.iter().flatten().fold(Aabb2::EMPTY, |b, seg| b.union(&seg.bounds()));
    let cut_outs = list(reply, "cutout_shapes").ok_or_else(|| unread("cutout_shapes"))?;
    let cut_outs = cut_outs.iter().map(|shape| shape_segs(shape, &turn)).collect::<Option<Vec<_>>>().ok_or_else(|| unread("cutout_shapes"))?;
    let bend = |b: &Value| Some((point(b.get("start")?, &turn)?, point(b.get("end")?, &turn)?, b.get("direction")? == "up"));
    let bends = list(reply, "bends").ok_or_else(|| unread("bends"))?.iter().map(bend).collect::<Option<Vec<_>>>().ok_or_else(|| unread("bends"))?;
    Ok(Flat { loops: outlines.into_iter().chain(cut_outs).collect(), bends, model })
}

/// A [`QuarterTurn`] on the flat's own plane: each point through the turn's basis, whose axes are
/// signed unit axes, so it maps exactly; an arc's angles turn by its degrees.
struct Turn {
    x: Vec2,
    y: Vec2,
    angle: f64,
}

impl Turn {
    fn new(turn: QuarterTurn) -> Turn {
        let basis = turn.basis();
        Turn { x: Vec2::new(basis.x.x, basis.x.y), y: Vec2::new(basis.y.x, basis.y.y), angle: f64::from(u16::from(turn)).to_radians() }
    }

    fn apply(&self, p: Vec2) -> Vec2 {
        Vec2::new(p.dot(self.x), p.dot(self.y))
    }
}

/// The array at `key` of a reply.
fn list<'a>(reply: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    reply.get(key)?.as_array()
}

/// A reply's `[x, y]`, turned.
fn point(v: &Value, turn: &Turn) -> Option<Vec2> {
    let at = |i: usize| v.get(i)?.as_f64();
    Some(turn.apply(Vec2::new(at(0)?, at(1)?)))
}

fn points(v: &Value, turn: &Turn) -> Option<Vec<Vec2>> {
    v.as_array()?.iter().map(|p| point(p, turn)).collect()
}

/// The lines round a loop of `points`, back to its first, which a ring doesn't repeat.
fn closed(points: &[Vec2]) -> Vec<Seg2> {
    let mut ring = points.to_vec();
    if let Some(&first) = points.first().filter(|first| points.last() != Some(*first)) {
        ring.push(first);
    }
    lines(&ring)
}

/// The lines from each of `points` to the next.
fn lines(points: &[Vec2]) -> Vec<Seg2> {
    points.windows(2).filter_map(|pair| if let [a, b] = pair { Some(Seg2::Line { a: *a, b: *b }) } else { None }).collect()
}

/// A cut-out's exact shape as the reply gives it (`loop_shape`), turned: a whole circle as two half
/// arcs, or its lines, arcs and point lists.
fn shape_segs(shape: &Value, turn: &Turn) -> Option<Vec<Seg2>> {
    use std::f64::consts::PI;
    if let Some(circle) = shape.get("circle") {
        let (center, radius) = (point(circle.get("center")?, turn)?, circle.get("radius")?.as_f64()?);
        return Some([0.0, PI].map(|start| Seg2::Arc { center, radius, start: start + turn.angle, sweep: PI }).to_vec());
    }
    let mut segs = Vec::new();
    for seg in shape.get("segments")?.as_array()? {
        if let Some([a, b]) = seg.get("line").and_then(Value::as_array).map(Vec::as_slice) {
            segs.push(Seg2::Line { a: point(a, turn)?, b: point(b, turn)? });
        } else if let Some(arc) = seg.get("arc") {
            let degrees = |key: &str| arc.get(key).and_then(Value::as_f64).map(f64::to_radians);
            segs.push(Seg2::Arc {
                center: point(arc.get("center")?, turn)?,
                radius: arc.get("radius")?.as_f64()?,
                start: degrees("start_deg")? + turn.angle,
                sweep: degrees("sweep_deg")?,
            });
        } else {
            segs.extend(lines(&points(seg.get("polyline")?, turn)?));
        }
    }
    Some(segs)
}

/// `flat` through `frame` as the paper scene group of the view `object`, in line `group`: each loop one
/// closed visible stroke, then each bend line one narrow dashed stroke, a long dash at its middle.
pub fn flat_group(flat: &Flat, frame: &ViewFrame, object: u64, group: LineGroup) -> Group {
    let width_mm = LineKind::Visible.width_mm(group);
    let loops = flat.loops.iter().filter_map(|segs| {
        let path: Vec<Seg2> = segs.iter().map(|seg| frame.seg_to_paper(seg)).filter_map(drawable).collect();
        (!path.is_empty()).then_some(Item::Stroke { path, closed: true, width_mm, pattern: None })
    });
    let map = frame.paper_map();
    let bends = flat.bends.iter().filter_map(|&(a, b, up)| {
        let kind = if up { LineKind::Centre } else { LineKind::Phantom };
        let path = vec![drawable(Seg2::Line { a: map.apply(a), b: map.apply(b) })?];
        let pattern = kind.dashes_mm(group).map(|lengths_mm| centred(lengths_mm, path.iter().map(Seg2::length).sum()));
        Some(Item::Stroke { path, closed: false, width_mm: kind.width_mm(group), pattern })
    });
    Group { object, kind: ObjectKind::View, items: loops.chain(bends).collect() }
}

/// `lengths_mm` started where a line `length_mm` long has its first, long dash at its middle.
fn centred(lengths_mm: Vec<f64>, length_mm: f64) -> Dashes {
    let period: f64 = lengths_mm.iter().sum();
    let long = lengths_mm.first().copied().unwrap_or_default();
    let phase = (long / 2.0 - length_mm / 2.0).rem_euclid(period);
    // Rounding can land on the period itself, which a pattern's phase stays below.
    Dashes { lengths_mm, phase_mm: if phase < period { phase } else { 0.0 } }
}

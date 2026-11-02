//! 3D sketch constraints (#10608): relations among the fit points of curves drawn in a 3D sketch
//! ([`crate::Wire::fit`]), solved in a pass of their own after the planar sketch. The planar
//! solve never sees them and a 3D constraint never moves planar geometry; their freedoms and
//! their failures are reported apart from the planar sketch's ([`WireReport`]).
//!
//! A constraint names a drawn curve by its wire id and one of its fit points by index
//! ([`WirePoint`]). Axes are those of the wires' own coordinate space ([`crate::WireSpace`]): a
//! sketch in a component is constrained in the component's frame. All wires one constraint
//! names share a space.
//!
//! Points made coincident are one unknown, and a fixed point is a constant. The rest is solved
//! group by connected group with Levenberg–Marquardt, each constraint's exact Jacobian coming
//! from the same formula as its residuals ([`dual`]).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::linalg::{self, Mat};
use crate::model::{MAX_CONSTRAINTS, Result, Sketch, SketchError, in_range3};
use crate::union_find::{find, union};
use crate::wire::{Wire, arc_from_3_points, arc_polyline, drawn_polyline};

#[path = "wire_dual.rs"]
mod dual;
use dual::{Dual, Num, V};

/// Every residual below this (mm) ends the solve.
const TOL: f64 = 1e-10;
/// A solution is accepted when every residual is below this (mm).
pub const ACCEPT: f64 = 1e-6;
/// Most Levenberg–Marquardt iterations per connected group.
const MAX_ITERS: usize = 200;
/// Most unknowns (point coordinates) in one connected group: the normal equations are dense.
pub const MAX_WIRE_VARS: usize = 1_500;
/// Most residual rows in one connected group: the freedom count reads a dense Jacobian of
/// rows by unknowns (at most 36 MB).
pub const MAX_WIRE_ROWS: usize = 2 * MAX_WIRE_VARS;
/// The multiply-adds one solve may spend on its linear systems and its freedom count, a few
/// seconds' work at most. A design that asks for more is left where it is and reported as too large to
/// solve in time, rather than hold the window.
const WORK_BUDGET: f64 = 4e9;
/// The most a point moves in one solver step, as a share of its group's size (more when a length
/// or a dragged point has further to go): a step that turns a line moves its end along an arc in
/// small steps, rather than straight off to where the linear model says, which stretches it.
const MAX_STEP: f64 = 0.05;
/// A solve gives up on a group once it has stalled far from a solution: over the last
/// `STALL_ITERS` iterations the sum of squared residuals fell by at most `STALL_DECREASE` of
/// itself while the largest residual is above `STALL_RESIDUAL` (mm), as the planar solve does.
const STALL_ITERS: usize = 20;
const STALL_DECREASE: f64 = 1e-5;
const STALL_RESIDUAL: f64 = 1e-4;
/// Two fit points closer than this (mm) meet: drawn end to end, they are joined.
const MEET: f64 = 1e-9;
/// Chord-height tolerance (mm) of a re-sampled arc, as when it was drawn.
const ARC_TOL: f64 = 1e-3;

/// One fit point of a drawn 3D curve: `index` into its wire's `fit`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WirePoint {
    pub wire: String,
    pub index: usize,
}

/// An axis of the wires' coordinate space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn unit(self) -> Vec3 {
        match self {
            Axis::X => Vec3::new(1.0, 0.0, 0.0),
            Axis::Y => Vec3::new(0.0, 1.0, 0.0),
            Axis::Z => Vec3::new(0.0, 0.0, 1.0),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Axis::X => "x",
            Axis::Y => "y",
            Axis::Z => "z",
        }
    }
    /// The axis a direction runs along, either way, when it runs along one.
    pub fn along(d: Vec3) -> Option<Axis> {
        let n = d.len();
        [Axis::X, Axis::Y, Axis::Z].into_iter().find(|a| n > 1e-12 && d.cross(a.unit()).len() <= 1e-9 * n)
    }
}

/// The expressions placing a 3D point, over the design's parameters, in the coordinates of its
/// curve's space; a coordinate without one keeps the number in its Fix.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PointExprs3 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z: Option<String>,
}

impl PointExprs3 {
    /// The expressions set, each with what it sets.
    pub fn list(&self) -> Vec<(&'static str, &String)> {
        [("3D Point X", &self.x), ("3D Point Y", &self.y), ("3D Point Z", &self.z)]
            .into_iter()
            .filter_map(|(what, e)| e.as_ref().map(|e| (what, e)))
            .collect()
    }

    /// Mutable view of [`PointExprs3::list`], for renaming the parameters they refer to.
    pub fn list_mut(&mut self) -> Vec<&mut String> {
        [&mut self.x, &mut self.y, &mut self.z].into_iter().filter_map(Option::as_mut).collect()
    }

    /// Each axis's expression, in x, y, z order.
    pub fn by_axis(&self) -> [Option<&String>; 3] {
        [self.x.as_ref(), self.y.as_ref(), self.z.as_ref()]
    }
}

/// Most a 3D length dimension may ask for (mm).
pub const MAX_WIRE_LENGTH: f64 = 1e6;
/// The shortest distance a 3D distance dimension drives (mm): a distance of nothing has no
/// direction to move along; two points at one place are made coincident instead.
const MIN_WIRE_DISTANCE: f64 = 1e-6;
/// An angle this close to 0 or 180 degrees (radians) holds the lines parallel, the way it was
/// asked (2 freedoms, not 1).
const PARALLEL_ANGLE: f64 = 1e-9;

/// What a 3D constraint asks for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WireConstraintKind {
    /// Two fit points at the same place.
    Coincident { a: WirePoint, b: WirePoint },
    /// A fit point on the infinite line through a drawn 3D line.
    PointOnLine { p: WirePoint, line: String },
    /// A fit point held where it was fixed. With `exprs`, those coordinates follow the design's
    /// parameters (applied before each solve).
    Fix {
        p: WirePoint,
        at: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exprs: Option<Box<PointExprs3>>,
    },
    /// A drawn 3D line parallel to an axis.
    AlongAxis { line: String, axis: Axis },
    /// The distance between two fit points (a line's length is the distance between its ends).
    Distance { a: WirePoint, b: WirePoint, value: f64 },
    /// The distance from `a` to `b` along an axis, the way it was measured: `reversed` when `b`
    /// lay before `a` along the axis, so the dimension keeps that side.
    AxisDistance {
        a: WirePoint,
        b: WirePoint,
        axis: Axis,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        reversed: bool,
        value: f64,
    },
    /// The angle (radians, 0 to π) between the directions of two drawn 3D lines, each from its
    /// start to its end, or from its end to its start where `*_reversed`: two lines that meet
    /// are measured from the end they share, as the corner they make.
    Angle {
        a: String,
        b: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        a_reversed: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        b_reversed: bool,
        value: f64,
    },
    /// The radius of a drawn 3D arc.
    Radius { arc: String, value: f64 },
}

impl WireConstraintKind {
    /// The fit points it reads, lines as their two ends.
    pub fn points(&self) -> Vec<WirePoint> {
        let ends = |line: &str| [WirePoint { wire: line.to_string(), index: 0 }, WirePoint { wire: line.to_string(), index: 1 }];
        match self {
            WireConstraintKind::Coincident { a, b } => vec![a.clone(), b.clone()],
            WireConstraintKind::PointOnLine { p, line } => {
                let mut v = vec![p.clone()];
                v.extend(ends(line));
                v
            }
            WireConstraintKind::Fix { p, .. } => vec![p.clone()],
            WireConstraintKind::AlongAxis { line, .. } => ends(line).to_vec(),
            WireConstraintKind::Distance { a, b, .. } | WireConstraintKind::AxisDistance { a, b, .. } => vec![a.clone(), b.clone()],
            WireConstraintKind::Angle { a, b, .. } => ends(a).into_iter().chain(ends(b)).collect(),
            WireConstraintKind::Radius { arc, .. } => (0..3).map(|index| WirePoint { wire: arc.clone(), index }).collect(),
        }
    }

    /// The wires it names.
    pub fn wires(&self) -> Vec<&str> {
        match self {
            WireConstraintKind::Coincident { a, b } => vec![&a.wire, &b.wire],
            WireConstraintKind::PointOnLine { p, line } => vec![&p.wire, line],
            WireConstraintKind::Fix { p, .. } => vec![&p.wire],
            WireConstraintKind::AlongAxis { line, .. } => vec![line],
            WireConstraintKind::Distance { a, b, .. } | WireConstraintKind::AxisDistance { a, b, .. } => vec![&a.wire, &b.wire],
            WireConstraintKind::Angle { a, b, .. } => vec![a, b],
            WireConstraintKind::Radius { arc, .. } => vec![arc],
        }
    }

    /// Whether it is a dimension (it has a value).
    pub fn is_dimension(&self) -> bool {
        self.value().is_some()
    }

    /// A dimension's value (mm, or radians for an angle).
    pub fn value(&self) -> Option<f64> {
        match self {
            WireConstraintKind::Distance { value, .. }
            | WireConstraintKind::AxisDistance { value, .. }
            | WireConstraintKind::Angle { value, .. }
            | WireConstraintKind::Radius { value, .. } => Some(*value),
            _ => None,
        }
    }

    /// Set a dimension's value; other constraints are unchanged.
    pub fn set_value(&mut self, v: f64) {
        match self {
            WireConstraintKind::Distance { value, .. }
            | WireConstraintKind::AxisDistance { value, .. }
            | WireConstraintKind::Angle { value, .. }
            | WireConstraintKind::Radius { value, .. } => *value = v,
            _ => {}
        }
    }

    /// Whether its residuals are lengths (mm), not angles: only those say how far a point has
    /// still to go.
    fn in_mm(&self) -> bool {
        !matches!(self, WireConstraintKind::Angle { .. })
    }

    /// Whether its value is an angle.
    pub fn is_angle(&self) -> bool {
        matches!(self, WireConstraintKind::Angle { .. })
    }

    /// What a dimension measures with its points at `at` (in [`Self::points`] order); `None`
    /// where it measures nothing (an arc whose points are in a line has no radius).
    fn measure_at(&self, at: &[Vec3]) -> Option<f64> {
        let p = |i: usize| at.get(i).copied().unwrap_or(Vec3::ZERO);
        let v = match self {
            WireConstraintKind::Distance { .. } => p(0).dist(p(1)),
            WireConstraintKind::AxisDistance { axis, reversed, .. } => {
                let d = (p(1) - p(0)).dot(axis.unit());
                if *reversed { -d } else { d }
            }
            WireConstraintKind::Angle { a_reversed, b_reversed, .. } => {
                let (u, v) = (directed(p(0), p(1), *a_reversed), directed(p(2), p(3), *b_reversed));
                u.cross(v).len().atan2(u.dot(v))
            }
            WireConstraintKind::Radius { .. } => {
                let (a, b, c) = (p(0), p(1), p(2));
                let twice_area = (b - a).cross(c - a).len();
                if twice_area <= 1e-12 * (a.dist(b) * c.dist(a)).max(1e-300) {
                    return None;
                }
                a.dist(b) * b.dist(c) * c.dist(a) / (2.0 * twice_area)
            }
            _ => return None,
        };
        v.is_finite().then_some(v)
    }

    /// Its residuals, from the positions of [`Self::points`] in that order, appended to `out`.
    fn residuals<T: Num>(&self, at: &[V<T>], out: &mut Vec<T>) {
        let zero = V { x: T::num(0.0), y: T::num(0.0), z: T::num(0.0) };
        let p = |i: usize| at.get(i).copied().unwrap_or(zero);
        match self {
            WireConstraintKind::Coincident { .. } => out.extend((p(0) - p(1)).xyz()),
            WireConstraintKind::PointOnLine { .. } => {
                // The point's offset from the line, square to it: (p − a) × d / |d|.
                let d = p(2) - p(1);
                let n = d.len();
                let n = if n.val() > 1e-12 { n } else { T::num(1e-12) };
                out.extend((p(0) - p(1)).cross(d).scale(T::num(1.0) / n).xyz());
            }
            WireConstraintKind::Fix { at, .. } => out.extend((p(0) - V::of(*at)).xyz()),
            WireConstraintKind::AlongAxis { axis, .. } => {
                let d = p(1) - p(0);
                out.extend(match axis {
                    Axis::X => [d.y, d.z],
                    Axis::Y => [d.z, d.x],
                    Axis::Z => [d.x, d.y],
                });
            }
            WireConstraintKind::Distance { value, .. } => out.push((p(1) - p(0)).len() - T::num(*value)),
            WireConstraintKind::AxisDistance { axis, reversed, value, .. } => {
                let d = (p(1) - p(0)).dot(V::of(axis.unit()));
                out.push(if *reversed { -d } else { d } - T::num(*value));
            }
            WireConstraintKind::Angle { a_reversed, b_reversed, value, .. } => {
                let unit = |v: V<T>| {
                    let n = v.len();
                    v.scale(T::num(1.0) / if n.val() > 1e-12 { n } else { T::num(1e-12) })
                };
                let (u, v) = (unit(directed(p(0), p(1), *a_reversed)), unit(directed(p(2), p(3), *b_reversed)));
                if *value <= PARALLEL_ANGLE {
                    // The same direction: smooth where the lines are parallel, unlike the angle.
                    out.extend((u - v).xyz());
                } else if *value >= std::f64::consts::PI - PARALLEL_ANGLE {
                    out.extend((u + v).xyz());
                } else {
                    // The angle itself: smooth everywhere but where the lines are parallel, which
                    // this value is not (a solve that starts there is nudged off it).
                    out.push(u.cross(v).len().atan2(u.dot(v)) - T::num(*value));
                }
            }
            WireConstraintKind::Radius { value, .. } => {
                // The circle through the arc's three points: |ab| |bc| |ca| / (2 |ab × ac|).
                let (a, b, c) = (p(0), p(1), p(2));
                let r = (b - a).len() * (c - b).len() * (a - c).len() / (T::num(2.0) * (b - a).cross(c - a).len());
                out.push(r - T::num(*value));
            }
        }
    }

    /// Its residuals in plain numbers.
    fn residuals_at(&self, at: &[Vec3]) -> Vec<f64> {
        let at: Vec<V<f64>> = at.iter().map(|p| V::of(*p)).collect();
        let mut out = Vec::new();
        self.residuals(&at, &mut out);
        out
    }

    /// How many residuals it has.
    fn rows(&self) -> usize {
        self.residuals_at(&vec![Vec3::ZERO; self.points().len()]).len()
    }

    /// The same relation, whichever order its operands were given in.
    fn same_as(&self, o: &WireConstraintKind) -> bool {
        use WireConstraintKind::*;
        match (self, o) {
            (Coincident { a, b }, Coincident { a: c, b: d }) => (a == c && b == d) || (a == d && b == c),
            (PointOnLine { p, line }, PointOnLine { p: q, line: m }) => p == q && line == m,
            (Fix { p, .. }, Fix { p: q, .. }) => p == q,
            (AlongAxis { line, axis }, AlongAxis { line: m, axis: b }) => line == m && axis == b,
            _ => false,
        }
    }

    /// A dimension of the same kind on the same geometry (a second one could only conflict or
    /// repeat it).
    fn same_dimension(&self, o: &WireConstraintKind) -> bool {
        use WireConstraintKind::*;
        let pair = |a: &WirePoint, b: &WirePoint, c: &WirePoint, d: &WirePoint| (a == c && b == d) || (a == d && b == c);
        match (self, o) {
            (Distance { a, b, .. }, Distance { a: c, b: d, .. }) => pair(a, b, c, d),
            (AxisDistance { a, b, axis, .. }, AxisDistance { a: c, b: d, axis: e, .. }) => axis == e && pair(a, b, c, d),
            (Angle { a, b, .. }, Angle { a: c, b: d, .. }) => (a == c && b == d) || (a == d && b == c),
            (Radius { arc, .. }, Radius { arc: other, .. }) => arc == other,
            _ => false,
        }
    }
}

/// A line's direction from `a` to `b`, or from `b` to `a` when `reversed`.
fn directed<P: std::ops::Sub<Output = P>>(a: P, b: P, reversed: bool) -> P {
    if reversed { a - b } else { b - a }
}

/// A 3D constraint as a sketch stores it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireConstraint {
    pub id: String,
    #[serde(flatten)]
    pub kind: WireConstraintKind,
    /// For a dimension: the model parameter that drives its value (e.g. `d1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    /// A driven (reference) dimension: it measures, it does not constrain.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub driven: bool,
}

/// How a 3D solve went: apart from the planar sketch's report, whose freedoms and failures it
/// does not change.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WireReport {
    /// Every 3D constraint holds.
    pub ok: bool,
    /// Remaining degrees of freedom of the drawn 3D points (3 per point, less what the 3D
    /// constraints determine). Zero from a solve that did not count them.
    pub dof: usize,
    /// The constraints left violated by a solve that failed.
    pub failing: Vec<String>,
    /// The unknowns of a connected group too large to solve at once ([`MAX_WIRE_VARS`]
    /// unknowns, [`MAX_WIRE_ROWS`] residuals), with its residuals in `oversized_rows`.
    pub oversized: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oversized_rows: Option<usize>,
    /// The solve ran out of work ([`WORK_BUDGET`]) before every group was solved: those left
    /// are where they were, and none of their constraints is said to conflict.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub out_of_time: bool,
    /// The freedom count ran out of work: `dof` counts some groups as if unconstrained.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub uncounted: bool,
}

impl WireReport {
    fn new() -> WireReport {
        WireReport { ok: true, ..WireReport::default() }
    }

    /// Why it failed, in the words a refused edit uses; `conflict` words the list of the
    /// constraints left violated.
    pub fn failure(&self, conflict: impl FnOnce(&str) -> String) -> String {
        if let Some(n) = self.oversized {
            let rows = self.oversized_rows.unwrap_or(0);
            return format!(
                "the 3D sketch joins {n} unknowns (3D point coordinates) and {rows} constraint equations in one connected group, more than the {MAX_WIRE_VARS} unknowns or {MAX_WIRE_ROWS} equations the 3D solver takes at once: split the 3D curves over several sketches"
            );
        }
        if self.failing.is_empty() && self.out_of_time {
            return "the 3D sketch is too large to solve in time: split its 3D curves over several sketches".into();
        }
        conflict(&self.failing.join(", "))
    }
}

/// A point of one group's constraint: an unknown (its index), or a constant.
#[derive(Clone, Copy)]
enum Pt {
    Var(usize),
    At(Vec3),
}

/// One constraint of a group, its points as unknowns or constants.
struct Block<'a> {
    id: &'a str,
    kind: &'a WireConstraintKind,
    pts: Vec<Pt>,
    /// The distinct unknowns it reads, in the order its derivatives are seeded.
    vars: Vec<usize>,
}

impl Block<'_> {
    fn new<'a>(id: &'a str, kind: &'a WireConstraintKind, pts: Vec<Pt>) -> Block<'a> {
        let mut vars = Vec::new();
        for p in &pts {
            if let Pt::Var(v) = p
                && !vars.contains(v)
            {
                vars.push(*v);
            }
        }
        Block { id, kind, pts, vars }
    }

    fn at(x: &[f64], v: usize) -> Vec3 {
        Vec3::new(x.get(3 * v).copied().unwrap_or(0.0), x.get(3 * v + 1).copied().unwrap_or(0.0), x.get(3 * v + 2).copied().unwrap_or(0.0))
    }

    fn residuals(&self, x: &[f64], out: &mut Vec<f64>) {
        let at: Vec<V<f64>> = self
            .pts
            .iter()
            .map(|p| match p {
                Pt::Var(v) => V::of(Self::at(x, *v)),
                Pt::At(q) => V::of(*q),
            })
            .collect();
        self.kind.residuals(&at, out);
    }

    /// Its residuals with their derivatives by [`Self::vars`] (three per unknown).
    fn jacobian(&self, x: &[f64]) -> Vec<Dual> {
        let at: Vec<V<Dual>> = self
            .pts
            .iter()
            .map(|p| match p {
                Pt::Var(v) => {
                    let q = Self::at(x, *v);
                    let k = 3 * self.vars.iter().position(|u| u == v).unwrap_or(0);
                    V { x: Dual::var(q.x, k), y: Dual::var(q.y, k + 1), z: Dual::var(q.z, k + 2) }
                }
                Pt::At(q) => V::of(*q),
            })
            .collect();
        let mut out = Vec::new();
        self.kind.residuals(&at, &mut out);
        out
    }

    fn holds(&self, x: &[f64]) -> bool {
        let mut r = Vec::new();
        self.residuals(x, &mut r);
        r.iter().all(|v| v.abs() <= ACCEPT)
    }
}

impl Sketch {
    /// A drawn (not linked) 3D curve by id.
    fn drawn_wire(&self, id: &str) -> Option<&Wire> {
        self.wires.iter().find(|w| w.id == id && w.link.is_none() && !w.fit.is_empty())
    }

    /// The drawn 3D curves by id, for looking up many.
    fn drawn_wires(&self) -> HashMap<&str, &Wire> {
        self.wires.iter().filter(|w| w.link.is_none() && !w.fit.is_empty()).map(|w| (w.id.as_str(), w)).collect()
    }

    /// Whether `id` is a drawn straight 3D line (two fit points, not an arc).
    pub fn is_drawn_line(&self, id: &str) -> bool {
        self.drawn_wire(id).is_some_and(|w| w.fit.len() == 2 && w.arc.is_none())
    }

    /// The fit point a reference names: `<wire>.start`, `<wire>.end`, `<wire>.through` (an
    /// arc's middle point), `<wire>.<n>` (fit point n), or a 3D point's own id.
    pub fn wire_point(&self, r: &str) -> Option<WirePoint> {
        if let Some(w) = self.drawn_wire(r).filter(|w| w.fit.len() == 1) {
            return Some(WirePoint { wire: w.id.clone(), index: 0 });
        }
        let (id, role) = r.rsplit_once('.')?;
        let w = self.drawn_wire(id)?;
        let n = w.fit.len();
        let index = match role {
            "start" => 0,
            "end" => n.checked_sub(1)?,
            "through" if w.arc.is_some() => 1,
            other => other.parse::<usize>().ok().filter(|i| *i < n)?,
        };
        Some(WirePoint { wire: w.id.clone(), index })
    }

    /// Where a fit point is now.
    pub fn wire_point_at(&self, p: &WirePoint) -> Option<Vec3> {
        self.drawn_wire(&p.wire).and_then(|w| w.fit.get(p.index)).copied()
    }

    /// Check a 3D constraint against the sketch: its curves exist, are drawn, are lines where a
    /// line is needed, and share one coordinate space.
    pub fn validate_wire_constraint(&self, k: &WireConstraintKind) -> Result<()> {
        validate_in(k, false, &|id| self.drawn_wire(id))
    }

    /// Check every stored 3D constraint.
    pub fn validate_wire_constraints(&self) -> Result<()> {
        if self.wire_constraints.len() > MAX_CONSTRAINTS {
            return Err(SketchError::TooLarge);
        }
        let drawn = self.drawn_wires();
        // Not another entity's id either: a delete by id would take the wrong one.
        let others: std::collections::HashSet<&str> = self
            .points
            .iter()
            .map(|p| p.id.as_str())
            .chain(self.curves.iter().map(|c| c.id.as_str()))
            .chain(self.constraints.iter().map(|c| c.id.as_str()))
            .chain(self.wires.iter().map(|w| w.id.as_str()))
            .chain(self.links.iter().map(|l| l.id.as_str()))
            .chain(self.patterns.iter().map(|p| p.id.as_str()))
            .collect();
        let mut ids = std::collections::BTreeSet::new();
        for c in &self.wire_constraints {
            if c.id.is_empty() || c.id.len() > 512 || others.contains(c.id.as_str()) || !ids.insert(c.id.as_str()) {
                return Err(SketchError::Invalid("3D constraint ids are invalid".into()));
            }
            validate_in(&c.kind, c.driven, &|id| drawn.get(id).copied()).map_err(|e| match e {
                SketchError::Invalid(m) => SketchError::Invalid(format!("{}: {m}", c.id)),
                e => e,
            })?;
        }
        Ok(())
    }

    /// Add a 3D constraint; one that says what another already says returns that one's id.
    pub fn add_wire_constraint(&mut self, kind: WireConstraintKind) -> Result<String> {
        self.add_wire_dimension(kind, None, false)
    }

    /// Add a 3D constraint or dimension; a dimension takes its model parameter (`param`) or is
    /// driven (it measures, and its value need only be a number). A dimension on geometry
    /// another dimension of that kind already measures is refused, naming it.
    pub fn add_wire_dimension(&mut self, kind: WireConstraintKind, param: Option<String>, driven: bool) -> Result<String> {
        let driven = driven && kind.is_dimension();
        validate_in(&kind, driven, &|id| self.drawn_wire(id))?;
        if let Some(same) = self.wire_constraints.iter().find(|c| c.kind.same_as(&kind)) {
            return Ok(same.id.clone());
        }
        if let Some(same) = self.wire_constraints.iter().find(|c| c.kind.same_dimension(&kind)) {
            return Err(SketchError::Invalid(format!("that is already dimensioned by {}", same.id)));
        }
        if self.constraints.len().saturating_add(self.wire_constraints.len()) >= MAX_CONSTRAINTS {
            return Err(SketchError::TooLarge);
        }
        let id = self.fresh("k")?;
        let param = if driven || !kind.is_dimension() { None } else { param };
        self.wire_constraints.push(WireConstraint { id: id.clone(), kind, param, driven });
        Ok(id)
    }

    /// An Angle between two drawn 3D lines, value unset: lines that meet at an end (or are
    /// joined there) are measured from that end, as the corner a drawing shows.
    pub fn angle_kind(&self, a: &str, b: &str) -> Option<WireConstraintKind> {
        let line = |id: &str| self.drawn_wire(id).filter(|w| w.fit.len() == 2 && w.arc.is_none());
        let (la, lb) = (line(a)?, line(b)?);
        let joined = |p: WirePoint, q: WirePoint| {
            self.wire_constraints
                .iter()
                .any(|c| matches!(&c.kind, WireConstraintKind::Coincident { a, b } if (*a == p && *b == q) || (*a == q && *b == p)))
        };
        let angle =
            |a_reversed: bool, b_reversed: bool| WireConstraintKind::Angle { a: a.to_string(), b: b.to_string(), a_reversed, b_reversed, value: 0.0 };
        for ia in [0usize, 1] {
            for ib in [0usize, 1] {
                let meet = matches!((la.fit.get(ia), lb.fit.get(ib)), (Some(p), Some(q)) if p.dist(*q) < MEET)
                    || joined(WirePoint { wire: a.to_string(), index: ia }, WirePoint { wire: b.to_string(), index: ib });
                if meet {
                    return Some(angle(ia == 1, ib == 1));
                }
            }
        }
        Some(angle(false, false))
    }

    /// What a 3D dimension measures now.
    pub fn measure_wire_dimension(&self, k: &WireConstraintKind) -> Option<f64> {
        let at: Option<Vec<Vec3>> = k.points().iter().map(|p| self.wire_point_at(p)).collect();
        k.measure_at(&at?)
    }

    /// The model parameters that drive the sketch's dimensions, planar and 3D.
    pub fn dimension_params(&self) -> Vec<&String> {
        self.constraints.iter().filter_map(|c| c.param.as_ref()).chain(self.wire_constraints.iter().filter_map(|c| c.param.as_ref())).collect()
    }

    /// Mutable view of [`Sketch::dimension_params`] (for renaming or re-pointing them).
    pub fn dimension_params_mut(&mut self) -> Vec<&mut Option<String>> {
        self.constraints.iter_mut().map(|c| &mut c.param).chain(self.wire_constraints.iter_mut().map(|c| &mut c.param)).collect()
    }

    /// The (dimension id, parameter) of every dimension driven by a parameter, planar and 3D.
    pub fn dimension_param_ids(&self) -> Vec<(&str, &str)> {
        self.constraints
            .iter()
            .filter_map(|c| c.param.as_deref().map(|p| (c.id.as_str(), p)))
            .chain(self.wire_constraints.iter().filter_map(|c| c.param.as_deref().map(|p| (c.id.as_str(), p))))
            .collect()
    }

    /// The expressions placing 3D points, each with what it sets.
    pub fn wire_point_exprs(&self) -> Vec<(&'static str, &String)> {
        self.wire_constraints
            .iter()
            .filter_map(|c| match &c.kind {
                WireConstraintKind::Fix { exprs: Some(e), .. } => Some(e.list()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// Mutable view of [`Sketch::wire_point_exprs`].
    pub fn wire_point_exprs_mut(&mut self) -> Vec<&mut String> {
        self.wire_constraints
            .iter_mut()
            .filter_map(|c| match &mut c.kind {
                WireConstraintKind::Fix { exprs: Some(e), .. } => Some(e.list_mut()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// Join drawn 3D curves that meet: each end of the curves `ids` (a 3D point is its own end)
    /// that is at the same place as an end of another drawn curve, in the same space, gets a
    /// Coincident with it, unless the two are joined already. Lines drawn end to end, one call
    /// or one segment at a time, stay joined when their constraints are solved. Returns the
    /// constraints added.
    pub fn join_drawn_ends(&mut self, ids: &[String]) -> Result<Vec<String>> {
        let ends: Vec<(WirePoint, Vec3, crate::WireSpace)> = self
            .wires
            .iter()
            .filter(|w| w.link.is_none() && !w.fit.is_empty())
            .flat_map(|w| {
                let last = w.fit.len() - 1;
                let idx: Vec<usize> = if last == 0 { vec![0] } else { vec![0, last] };
                idx.into_iter().filter_map(move |index| w.fit.get(index).map(|at| (WirePoint { wire: w.id.clone(), index }, *at, w.space)))
            })
            .collect();
        // The ends already joined, as sets.
        let slot: HashMap<&WirePoint, usize> = ends.iter().enumerate().map(|(i, e)| (&e.0, i)).collect();
        let mut parent: Vec<usize> = (0..ends.len()).collect();
        for c in &self.wire_constraints {
            if let WireConstraintKind::Coincident { a, b } = &c.kind
                && let (Some(i), Some(j)) = (slot.get(a), slot.get(b))
            {
                union(&mut parent, *i, *j);
            }
        }
        // Ends sorted along x, so each looks only at its near neighbours.
        let mut order: Vec<usize> = (0..ends.len()).collect();
        order.sort_by(|a, b| {
            let (pa, pb) = (ends.get(*a).map_or(0.0, |e| e.1.x), ends.get(*b).map_or(0.0, |e| e.1.x));
            pa.total_cmp(&pb)
        });
        let rank: Vec<usize> = {
            let mut r = vec![0; ends.len()];
            for (k, i) in order.iter().enumerate() {
                if let Some(x) = r.get_mut(*i) {
                    *x = k;
                }
            }
            r
        };
        let wanted: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
        let mut pairs = Vec::new();
        for (i, (p, at, space)) in ends.iter().enumerate() {
            if !wanted.contains(p.wire.as_str()) {
                continue;
            }
            let near = |j: &usize| ends.get(*j).is_some_and(|(q, b, s)| q.wire != p.wire && s == space && at.dist(*b) < MEET);
            let k = rank.get(i).copied().unwrap_or(0);
            let below = order.get(..k).unwrap_or(&[]).iter().rev().take_while(|j| ends.get(**j).is_some_and(|e| at.x - e.1.x < MEET));
            let above = order.get(k + 1..).unwrap_or(&[]).iter().take_while(|j| ends.get(**j).is_some_and(|e| e.1.x - at.x < MEET));
            let mut found: Vec<usize> = below.chain(above).copied().filter(near).collect();
            found.sort_unstable();
            for j in found {
                if find(&mut parent, i) != find(&mut parent, j) {
                    union(&mut parent, i, j);
                    if let (Some(a), Some(b)) = (ends.get(j), ends.get(i)) {
                        pairs.push((a.0.clone(), b.0.clone()));
                    }
                }
            }
        }
        pairs.into_iter().map(|(a, b)| self.add_wire_constraint(WireConstraintKind::Coincident { a, b })).collect()
    }

    /// Drop the 3D constraints that no longer check out: they name a curve the sketch no longer
    /// has, a fit point it no longer has, or are damaged. Returns how many went.
    pub fn prune_wire_constraints(&mut self) -> usize {
        let drawn = self.drawn_wires();
        let keep: Vec<bool> = self.wire_constraints.iter().map(|c| validate_in(&c.kind, c.driven, &|id| drawn.get(id).copied()).is_ok()).collect();
        let before = self.wire_constraints.len();
        let mut keep = keep.into_iter();
        self.wire_constraints.retain(|_| keep.next().unwrap_or(false));
        before - self.wire_constraints.len()
    }

    /// Drop the 3D constraints that name curve `wire` (its points were rebuilt, a motion trace
    /// sampled again). Returns how many went.
    pub fn drop_wire_constraints_on(&mut self, wire: &str) -> usize {
        let before = self.wire_constraints.len();
        self.wire_constraints.retain(|c| !c.kind.wires().contains(&wire));
        before - self.wire_constraints.len()
    }

    /// Whether a fit point is fixed, itself or through the points joined to it.
    pub fn wire_point_fixed(&self, p: &WirePoint) -> bool {
        let mut joined = vec![p.clone()];
        let mut k = 0;
        while let Some(q) = joined.get(k).cloned() {
            k += 1;
            if k > 4096 {
                break;
            }
            for c in &self.wire_constraints {
                match &c.kind {
                    WireConstraintKind::Fix { p, .. } if *p == q => return true,
                    WireConstraintKind::Coincident { a, b } if *a == q && !joined.contains(b) => joined.push(b.clone()),
                    WireConstraintKind::Coincident { a, b } if *b == q && !joined.contains(a) => joined.push(a.clone()),
                    _ => {}
                }
            }
        }
        false
    }

    /// The 3D constraints that hold as the curves are now (no solve).
    pub fn wire_constraints_holding(&self) -> std::collections::HashSet<String> {
        let drawn = self.drawn_wires();
        self.wire_constraints
            .iter()
            .filter(|c| {
                let at: Option<Vec<Vec3>> =
                    c.kind.points().iter().map(|p| drawn.get(p.wire.as_str()).and_then(|w| w.fit.get(p.index)).copied()).collect();
                at.is_some_and(|at| c.kind.residuals_at(&at).iter().all(|v| v.abs() <= ACCEPT))
            })
            .map(|c| c.id.clone())
            .collect()
    }

    /// Whether every 3D constraint holds as the curves are now (no solve).
    pub fn wire_constraints_hold(&self) -> bool {
        let drawn = self.drawn_wires();
        self.wire_constraints.iter().filter(|c| !c.driven).all(|c| {
            let at: Option<Vec<Vec3>> =
                c.kind.points().iter().map(|p| drawn.get(p.wire.as_str()).and_then(|w| w.fit.get(p.index)).copied()).collect();
            at.is_some_and(|at| c.kind.residuals_at(&at).iter().all(|v| v.abs() <= ACCEPT))
        })
    }

    /// [`Sketch::solve_wires_with`], counting the freedoms left.
    pub fn solve_wires(&mut self, hold: Option<&WirePoint>) -> WireReport {
        self.solve_wires_with(hold, true)
    }

    /// Solve the 3D constraints, moving drawn fit points. With `hold`, that point is kept at
    /// its position when the constraints allow (a drag); otherwise it moves like any other.
    /// With `count`, the freedoms left are counted (a rank pass the frames of a drag skip).
    /// Planar geometry is never touched. A failed solve leaves every point where it was.
    pub fn solve_wires_with(&mut self, hold: Option<&WirePoint>, count: bool) -> WireReport {
        let mut rep = WireReport::new();
        // Every fit point of every drawn curve, by curve.
        let mut first: HashMap<String, (usize, usize)> = HashMap::new();
        let mut positions: Vec<Vec3> = Vec::new();
        for w in self.wires.iter().filter(|w| w.link.is_none() && !w.fit.is_empty()) {
            first.insert(w.id.clone(), (positions.len(), w.fit.len()));
            positions.extend(&w.fit);
        }
        if self.wire_constraints.iter().all(|c| c.driven) {
            rep.dof = if count { positions.len().saturating_mul(3) } else { 0 };
            self.measure_driven();
            return rep;
        }
        let slot = |p: &WirePoint| first.get(&p.wire).filter(|(_, n)| p.index < *n).map(|(s, _)| s + p.index);
        let cons: Vec<(&str, &WireConstraintKind, Vec<usize>)> = self
            .wire_constraints
            .iter()
            .filter(|c| !c.driven)
            .filter_map(|c| c.kind.points().iter().map(slot).collect::<Option<Vec<usize>>>().map(|pts| (c.id.as_str(), &c.kind, pts)))
            .collect();
        // Points made coincident are one: classes of points, each one unknown (or constant).
        let mut parent: Vec<usize> = (0..positions.len()).collect();
        for (_, k, pts) in &cons {
            if let (WireConstraintKind::Coincident { .. }, [a, b]) = (k, pts.as_slice()) {
                union(&mut parent, *a, *b);
            }
        }
        let class: Vec<usize> = (0..positions.len()).map(|s| find(&mut parent, s)).collect();
        let held = hold.and_then(slot);
        let held_class = held.and_then(|h| class.get(h).copied());
        // Where each class starts: where it is fixed, else where the held point was put, else
        // where its first point is.
        let mut start: HashMap<usize, Vec3> = HashMap::new();
        for (s, c) in class.iter().enumerate() {
            if let Some(at) = positions.get(s) {
                start.entry(*c).or_insert(*at);
            }
        }
        if let (Some(h), Some(c)) = (held, held_class)
            && let Some(at) = positions.get(h)
        {
            start.insert(c, *at);
        }
        // Classes a failure leaves where their points are: the rest still move.
        let mut failed: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let mut fixed: HashMap<usize, Vec3> = HashMap::new();
        for (id, k, pts) in &cons {
            if let (WireConstraintKind::Fix { at, .. }, Some(c)) = (k, pts.first().and_then(|s| class.get(*s))) {
                match fixed.get(c) {
                    // Two fixes of one point (joined points) in different places.
                    Some(was) if was.dist(*at) > ACCEPT => {
                        rep.ok = false;
                        rep.failing.push(id.to_string());
                        failed.insert(*c);
                    }
                    Some(_) => {}
                    None => {
                        fixed.insert(*c, *at);
                        start.insert(*c, *at);
                    }
                }
            }
        }
        // The other constraints, grouped by the free classes they connect.
        let rel: Vec<usize> = (0..cons.len())
            .filter(|i| cons.get(*i).is_some_and(|c| !matches!(c.1, WireConstraintKind::Coincident { .. } | WireConstraintKind::Fix { .. })))
            .collect();
        let mut gparent: Vec<usize> = (0..positions.len()).collect();
        for i in &rel {
            let Some((_, _, pts)) = cons.get(*i) else { continue };
            let free: Vec<usize> = pts.iter().filter_map(|s| class.get(*s).copied()).filter(|c| !fixed.contains_key(c)).collect();
            for w in free.windows(2) {
                union(&mut gparent, w[0], w[1]);
            }
        }
        let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
        for i in &rel {
            let Some((id, k, pts)) = cons.get(*i) else { continue };
            match pts.iter().filter_map(|s| class.get(*s).copied()).find(|c| !fixed.contains_key(c)) {
                Some(c) => groups.entry(find(&mut gparent, c)).or_default().push(*i),
                // Every point fixed: it holds or it doesn't.
                None => {
                    let at: Vec<Vec3> = pts.iter().filter_map(|s| class.get(*s).and_then(|c| start.get(c)).copied()).collect();
                    if k.residuals_at(&at).iter().any(|v| v.abs() > ACCEPT) {
                        rep.ok = false;
                        rep.failing.push(id.to_string());
                        failed.extend(pts.iter().filter_map(|s| class.get(*s).copied()));
                    }
                }
            }
        }
        let mut solved: HashMap<usize, Vec3> = HashMap::new();
        let mut rank = 0usize;
        let mut work = 0.0;
        // Each group's classes and constraints, for a curve that fails to rebuild below.
        let mut group_of: HashMap<usize, usize> = HashMap::new();
        let mut group_cons: Vec<Vec<String>> = Vec::new();
        // The solved groups, counted once every group is solved: the count must not use up
        // the work a later group needs to solve.
        let mut to_count = Vec::new();
        for members in groups.values() {
            let mut vars: Vec<usize> = Vec::new();
            let mut var_of: HashMap<usize, usize> = HashMap::new();
            let mut blocks = Vec::with_capacity(members.len());
            for i in members {
                let Some((id, k, pts)) = cons.get(*i) else { continue };
                let pts: Vec<Pt> = pts
                    .iter()
                    .map(|s| {
                        let c = class.get(*s).copied().unwrap_or(*s);
                        match fixed.get(&c) {
                            Some(at) => Pt::At(*at),
                            None => Pt::Var(*var_of.entry(c).or_insert_with(|| {
                                vars.push(c);
                                vars.len() - 1
                            })),
                        }
                    })
                    .collect();
                blocks.push(Block::new(id, k, pts));
            }
            let n = vars.len().saturating_mul(3);
            let rows: usize = blocks.iter().map(|b| b.kind.rows()).sum();
            for c in &vars {
                group_of.insert(*c, group_cons.len());
            }
            group_cons.push(blocks.iter().map(|b| b.id.to_string()).collect());
            if n > MAX_WIRE_VARS || rows > MAX_WIRE_ROWS {
                rep.ok = false;
                if rep.oversized.unwrap_or(0) <= n {
                    rep.oversized = Some(n);
                    rep.oversized_rows = Some(rows);
                }
                failed.extend(vars.iter().copied());
                continue;
            }
            let x0: Vec<f64> = vars.iter().flat_map(|c| xyz(start.get(c).copied().unwrap_or(Vec3::ZERO))).collect();
            let hold_var = held_class.and_then(|c| var_of.get(&c).copied());
            let mut x = Lm::Diverged;
            if let Some(h) = hold_var {
                x = lm(&blocks, n, &x0, Some(h), &mut work);
                if matches!(&x, Lm::Done(x) if !blocks.iter().all(|b| b.holds(x))) {
                    x = Lm::Diverged;
                }
            }
            if matches!(x, Lm::Diverged) {
                x = lm(&blocks, n, &x0, None, &mut work);
            }
            match x {
                Lm::Done(x) if blocks.iter().all(|b| b.holds(&x)) => {
                    for (v, c) in vars.iter().enumerate() {
                        solved.insert(*c, Block::at(&x, v));
                    }
                    if count {
                        to_count.push((blocks, n, rows, x));
                    }
                }
                Lm::Done(x) => {
                    rep.ok = false;
                    rep.failing.extend(blocks.iter().filter(|b| !b.holds(&x)).map(|b| b.id.to_string()));
                    failed.extend(vars.iter().copied());
                }
                Lm::Diverged => {
                    rep.ok = false;
                    rep.failing.extend(blocks.iter().map(|b| b.id.to_string()));
                    failed.extend(vars.iter().copied());
                }
                // Out of work: this group and the ones after it stay where they are, and their
                // constraints are not said to conflict.
                Lm::OutOfTime => {
                    rep.ok = false;
                    rep.out_of_time = true;
                    failed.extend(vars.iter().copied());
                    work = f64::INFINITY;
                }
            }
        }
        for (blocks, n, rows, x) in &to_count {
            match group_rank(blocks, *n, *rows, x, &mut work) {
                Some(r) => rank = rank.saturating_add(r),
                None => rep.uncounted = true,
            }
        }
        if count {
            let mut classes: Vec<usize> = class.clone();
            classes.sort_unstable();
            classes.dedup();
            let free = classes.iter().filter(|c| !fixed.contains_key(c)).count();
            rep.dof = free.saturating_mul(3).saturating_sub(rank);
        }
        // Each point where its class is, but for the classes a failure leaves where they were,
        // rebuilding each curve from its fit points. A curve that can no longer be built (an
        // arc's points now in a line) fails its group, which stays where it was; the rest are
        // placed again without it.
        for _ in 0..=group_cons.len() {
            let place = |s: usize| match class.get(s) {
                Some(c) if failed.contains(c) => positions.get(s).copied(),
                Some(c) => solved.get(c).or_else(|| start.get(c)).copied(),
                None => positions.get(s).copied(),
            };
            let mut rebuilt = Vec::new();
            let mut broken = None;
            for (wi, w) in self.wires.iter().enumerate().filter(|(_, w)| w.link.is_none() && !w.fit.is_empty()) {
                let Some((s, n)) = first.get(&w.id) else { continue };
                let fit: Vec<Vec3> = (0..*n).map(|i| place(s + i).unwrap_or(Vec3::ZERO)).collect();
                if fit == w.fit {
                    continue;
                }
                let built = match w.arc {
                    _ if fit.iter().any(|p| !in_range3(*p)) => None,
                    Some(_) => match fit.as_slice() {
                        [a, b, c] => arc_from_3_points(*a, *b, *c).map(|arc| (Some(arc), arc_polyline(&arc, ARC_TOL))),
                        _ => None,
                    },
                    None => drawn_polyline(&fit).ok().map(|pts| (None, pts)),
                };
                match built {
                    Some((arc, pts)) => rebuilt.push((wi, fit, arc, pts)),
                    None => {
                        broken = Some((*s, *n));
                        break;
                    }
                }
            }
            let Some((s, n)) = broken else {
                for (wi, fit, arc, pts) in rebuilt {
                    if let Some(w) = self.wires.get_mut(wi) {
                        w.fit = fit;
                        w.arc = arc;
                        w.pts = pts;
                    }
                }
                self.measure_driven();
                return rep;
            };
            rep.ok = false;
            for c in (s..s + n).filter_map(|i| class.get(i).copied()) {
                match group_of.get(&c) {
                    Some(g) => {
                        failed.extend(group_of.iter().filter(|(_, h)| *h == g).map(|(c, _)| *c));
                        for id in group_cons.get(*g).into_iter().flatten() {
                            if !rep.failing.contains(id) {
                                rep.failing.push(id.clone());
                            }
                        }
                    }
                    None => {
                        failed.insert(c);
                    }
                }
            }
        }
        self.measure_driven();
        rep
    }

    /// Driven 3D dimensions take what they measure (one that measures nothing keeps its value).
    fn measure_driven(&mut self) {
        let measured: Vec<Option<f64>> =
            self.wire_constraints.iter().map(|c| if c.driven { self.measure_wire_dimension(&c.kind) } else { None }).collect();
        for (c, m) in self.wire_constraints.iter_mut().zip(measured) {
            if let Some(v) = m {
                c.kind.set_value(v);
            }
        }
    }

    /// Remove 3D constraints by id; returns how many went.
    pub fn remove_wire_constraints(&mut self, ids: &[String]) -> usize {
        let before = self.wire_constraints.len();
        self.wire_constraints.retain(|c| !ids.contains(&c.id));
        before - self.wire_constraints.len()
    }
}

/// [`Sketch::validate_wire_constraint`], with the drawn curves looked up by `drawn`.
/// A driven dimension's value need only be a number: it is whatever it measures.
fn validate_in<'a>(k: &WireConstraintKind, driven: bool, drawn: &dyn Fn(&str) -> Option<&'a Wire>) -> Result<()> {
    let bad = |m: String| Err(SketchError::Invalid(m));
    for p in k.points() {
        if drawn(&p.wire).and_then(|w| w.fit.get(p.index)).is_none() {
            return bad(format!("`{}` has no drawn 3D point {}", p.wire, p.index));
        }
    }
    let line = |id: &str| drawn(id).is_some_and(|w| w.fit.len() == 2 && w.arc.is_none());
    match k {
        WireConstraintKind::PointOnLine { line: l, .. } | WireConstraintKind::AlongAxis { line: l, .. } if !line(l) => {
            return bad(format!("`{l}` is not a drawn 3D line"));
        }
        WireConstraintKind::Coincident { a, b } if a == b => return bad("a point cannot be coincident with itself".into()),
        // A line's or an arc's own ends together would squeeze it away; a spline may close.
        WireConstraintKind::Coincident { a, b } if a.wire == b.wire && drawn(&a.wire).is_some_and(|w| w.fit.len() <= 3) => {
            return bad(format!("`{}` cannot be joined to itself", a.wire));
        }
        WireConstraintKind::PointOnLine { p, line } if p.wire == *line => return bad("a line's own end is already on it".into()),
        WireConstraintKind::Fix { at, .. } if !in_range3(*at) => return bad("3D point out of range".into()),
        WireConstraintKind::Distance { a, b, .. } | WireConstraintKind::AxisDistance { a, b, .. } if a == b => {
            return bad("a distance needs two different points".into());
        }
        WireConstraintKind::Angle { a, b, .. } if a == b || !line(a) || !line(b) => {
            return bad("an angle needs two different drawn 3D lines".into());
        }
        WireConstraintKind::Radius { arc, .. } if !drawn(arc).is_some_and(|w| w.arc.is_some()) => {
            return bad(format!("`{arc}` is not a drawn 3D arc"));
        }
        _ => {}
    }
    match k.value() {
        Some(v) if driven && !v.is_finite() => return bad("a 3D dimension's value must be a number".into()),
        Some(v) if !driven => {
            let ok = match k {
                WireConstraintKind::Angle { .. } => (0.0..=std::f64::consts::PI).contains(&v),
                WireConstraintKind::Radius { .. } => v > 1e-9 && v <= MAX_WIRE_LENGTH,
                WireConstraintKind::AxisDistance { .. } => v.abs() <= MAX_WIRE_LENGTH,
                _ => (MIN_WIRE_DISTANCE..=MAX_WIRE_LENGTH).contains(&v),
            };
            if !ok {
                return bad(match k {
                    WireConstraintKind::Angle { .. } => "a 3D angle must be between 0 and 180 degrees".to_string(),
                    WireConstraintKind::Radius { .. } => format!("a 3D radius must be above 0 and at most {MAX_WIRE_LENGTH} mm"),
                    WireConstraintKind::AxisDistance { .. } => format!("a 3D distance along an axis must be at most {MAX_WIRE_LENGTH} mm either way"),
                    _ => format!(
                        "a 3D distance must be above 0 and at most {MAX_WIRE_LENGTH} mm (make two points coincident to put them at one place)"
                    ),
                });
            }
        }
        _ => {}
    }
    if let WireConstraintKind::Fix { exprs: Some(e), .. } = k
        && e.list().iter().any(|(_, x)| x.is_empty() || x.len() > 4096)
    {
        return bad("a 3D point's expression must be 1 to 4096 characters".into());
    }
    let spaces: Vec<_> = k.wires().iter().filter_map(|id| drawn(id).map(|w| w.space)).collect();
    if spaces.windows(2).any(|w| w[0] != w[1]) {
        return bad("these 3D curves are in different coordinate spaces (one fixed in the world, one in its component)".into());
    }
    Ok(())
}

/// How a Levenberg–Marquardt run ended.
enum Lm {
    /// Where it ended (solved or not).
    Done(Vec<f64>),
    /// The numbers stopped being finite.
    Diverged,
    /// The solve's work ran out ([`WORK_BUDGET`]).
    OutOfTime,
}

/// Levenberg–Marquardt on one connected group of `n` unknowns from `x0`; with `hold`, that
/// unknown point is also pulled to where it starts. The damping is the same for every unknown
/// (so a step moves the points as little as it can) and scales with the largest curvature (so a
/// group solves alike at any size); no point moves more than [`MAX_STEP`] of the group's size
/// in one step.
fn lm(blocks: &[Block], n: usize, x0: &[f64], hold: Option<usize>, work: &mut f64) -> Lm {
    let target = hold.map(|h| Block::at(x0, h));
    let residuals = |x: &[f64]| -> Vec<f64> {
        let mut r = Vec::new();
        for b in blocks {
            b.residuals(x, &mut r);
        }
        if let (Some(h), Some(t)) = (hold, target) {
            r.extend(xyz(Block::at(x, h) - t));
        }
        r
    };
    let cost = |r: &[f64]| r.iter().map(|v| v * v).sum::<f64>();
    // The group's size: every point its constraints read, fixed ones too.
    let size = {
        let all = blocks.iter().flat_map(|b| b.pts.iter()).map(|p| match p {
            Pt::Var(v) => Block::at(x0, *v),
            Pt::At(q) => *q,
        });
        let far = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let (lo, hi) = all.fold((far, far * -1.0), |(lo, hi), p| (lo.min(p), hi.max(p)));
        if lo.x <= hi.x { lo.dist(hi).max(1.0) } else { 1.0 }
    };
    let mut x = x0.to_vec();
    let mut r = residuals(&x);
    let mut costs = vec![cost(&r)];
    let mut lambda = 1e-3;
    let mut nudges = 0;
    for it in 0..MAX_ITERS {
        if r.iter().all(|v| v.abs() < TOL) {
            break;
        }
        if let Some(was) = it.checked_sub(STALL_ITERS).and_then(|i| costs.get(i)) {
            let now = costs.last().copied().unwrap_or(0.0);
            if was - now <= STALL_DECREASE * was && r.iter().any(|v| v.abs() > STALL_RESIDUAL) {
                break;
            }
        }
        // (JᵀJ + λI) dx = −Jᵀr, built constraint by constraint over the unknowns each reads.
        let mut a = Mat::zeros(n, n);
        let mut g = vec![0.0; n];
        for b in blocks {
            let rows = b.jacobian(&x);
            let cols: Vec<usize> = b.vars.iter().flat_map(|v| [3 * v, 3 * v + 1, 3 * v + 2]).collect();
            for row in &rows {
                for (i, ci) in cols.iter().enumerate() {
                    let ji = row.d.get(i).copied().unwrap_or(0.0);
                    if ji == 0.0 {
                        continue;
                    }
                    if let Some(gi) = g.get_mut(*ci) {
                        *gi -= ji * row.v;
                    }
                    for (j, cj) in cols.iter().enumerate() {
                        let jj = row.d.get(j).copied().unwrap_or(0.0);
                        if jj != 0.0 {
                            a.add(*ci, *cj, ji * jj);
                        }
                    }
                }
            }
        }
        if let Some(h) = hold {
            let t = target.unwrap_or(Vec3::ZERO);
            let d = Block::at(&x, h) - t;
            for (axis, v) in xyz(d).into_iter().enumerate() {
                a.add(3 * h + axis, 3 * h + axis, 1.0);
                if let Some(gi) = g.get_mut(3 * h + axis) {
                    *gi -= v;
                }
            }
        }
        let top = (0..n).map(|c| a.get(c, c)).fold(0.0f64, f64::max);
        // How far a point has still to go, from the residuals that are lengths and the held
        // point's distance from where it was put.
        let held_off = hold.zip(target).map_or(0.0, |(h, t)| Block::at(&x, h).dist(t));
        let off_mm = blocks.iter().filter(|b| b.kind.in_mm()).fold(held_off, |m, b| {
            let mut v = Vec::new();
            b.residuals(&x, &mut v);
            v.iter().fold(m, |m, r| m.max(r.abs()))
        });
        let mut improved = false;
        for _ in 0..12 {
            if *work > WORK_BUDGET {
                // Out of work, though perhaps already solved to what is accepted.
                return if r.iter().all(|v| v.abs() <= ACCEPT) { Lm::Done(x) } else { Lm::OutOfTime };
            }
            let mut damped = a.clone();
            for c in 0..n {
                damped.add(c, c, lambda * (top + 1e-12));
            }
            let Some(mut dx) = linalg::solve_counted(damped, g.clone(), work) else {
                lambda *= 10.0;
                continue;
            };
            // At least twice what is still off, so a length asked to grow far gets there.
            let reach = (MAX_STEP * size).max(2.0 * off_mm);
            let longest = (0..n / 3).map(|v| Block::at(&dx, v).len()).fold(0.0f64, f64::max);
            if longest > reach {
                let s = reach / longest;
                dx.iter_mut().for_each(|v| *v *= s);
            }
            let trial: Vec<f64> = x.iter().zip(&dx).map(|(a, b)| a + b).collect();
            let rt = residuals(&trial);
            if rt.iter().all(|v| v.is_finite()) && cost(&rt) < cost(&r) {
                x = trial;
                r = rt;
                lambda = (lambda / 3.0).max(1e-12);
                improved = true;
                break;
            }
            lambda *= 4.0;
        }
        costs.push(cost(&r));
        if !improved {
            // No step lowers the cost: a constraint whose residual does not change to first
            // order where it starts (lines exactly or nearly parallel that must make an angle,
            // the through point of a semicircle whose radius must change) stalls the rest. Its
            // own points are nudged off that start, a few times at most.
            if nudges < MAX_NUDGES && nudge(blocks, &mut x, nudges) {
                nudges += 1;
                r = residuals(&x);
                lambda = 1e-3;
                continue;
            }
            break;
        }
    }
    if x.iter().all(|v| v.is_finite()) { Lm::Done(x) } else { Lm::Diverged }
}

/// Most times one run nudges a stalled start (see [`nudge`]).
const MAX_NUDGES: usize = 3;

/// Move the free points of the constraints still off by a thousandth of each one's own size,
/// in directions fixed by `round` (the same every run). Returns whether any moved.
fn nudge(blocks: &[Block], x: &mut [f64], round: usize) -> bool {
    let mut moved = false;
    let stuck: Vec<&Block> = blocks.iter().filter(|b| !b.holds(x)).collect();
    for b in stuck {
        let at: Vec<Vec3> = b
            .pts
            .iter()
            .map(|p| match p {
                Pt::Var(v) => Block::at(x, *v),
                Pt::At(q) => *q,
            })
            .collect();
        let size = at.iter().flat_map(|p| at.iter().map(move |q| p.dist(*q))).fold(0.0f64, f64::max).max(1e-3);
        for v in &b.vars {
            for axis in 0..3 {
                let k = (3 * v + axis + 7 * round) as f64 + 1.0;
                if let Some(c) = x.get_mut(3 * v + axis) {
                    *c += 1e-3 * size * ((k * 0.618_033_988_749_895).fract() - 0.5);
                    moved = true;
                }
            }
        }
    }
    moved
}

/// A point's coordinates.
fn xyz(p: Vec3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

/// The rank of a solved group's constraints, the freedoms they take, its work added to `work`;
/// `None` when the solve's work runs out.
fn group_rank(blocks: &[Block], n: usize, rows: usize, x: &[f64], work: &mut f64) -> Option<usize> {
    if *work > WORK_BUDGET {
        return None;
    }
    let mut j = Mat::zeros(rows, n);
    let mut row = 0;
    for b in blocks {
        for r in b.jacobian(x) {
            for (i, v) in b.vars.iter().enumerate() {
                for axis in 0..3 {
                    j.set(row, 3 * v + axis, r.d.get(3 * i + axis).copied().unwrap_or(0.0));
                }
            }
            row += 1;
        }
    }
    linalg::rank_counted(&j, 1e-8, work, WORK_BUDGET)
}

#[cfg(test)]
#[path = "wire_solve_tests.rs"]
mod tests;

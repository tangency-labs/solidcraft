//! 3D sketch constraints (#10608): relations among the fit points of curves drawn in a 3D sketch
//! ([`crate::Wire::fit`]), solved in a pass of their own after the planar sketch. The planar
//! solve never sees them, so a sketch without them solves exactly as before, and a 3D constraint
//! never moves planar geometry.
//!
//! A constraint names a drawn curve by its wire id and one of its fit points by index
//! ([`WirePoint`]). Axes are those of the wires' own coordinate space ([`crate::WireSpace`]): a
//! sketch in a component is constrained in the component's frame. All wires one constraint
//! names share a space.

use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec3;

use crate::linalg::{self, Mat};
use crate::model::{MAX_CONSTRAINTS, Result, Sketch, SketchError, in_range3};
use crate::wire::{arc_from_3_points, arc_polyline, fit_polyline};

/// Every residual below this (mm) ends the solve.
const TOL: f64 = 1e-10;
/// A solution is accepted when every residual is below this (mm).
pub const ACCEPT: f64 = 1e-6;
/// Most Levenberg–Marquardt iterations per connected group.
const MAX_ITERS: usize = 200;
/// Most unknowns (point coordinates) in one connected group: the normal equations are dense.
pub const MAX_WIRE_VARS: usize = 1_500;
/// The step for the numerical derivative of a residual (mm).
const STEP: f64 = 1e-6;
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
    /// start to its end.
    Angle { a: String, b: String, value: f64 },
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

    /// Whether its value is an angle.
    pub fn is_angle(&self) -> bool {
        matches!(self, WireConstraintKind::Angle { .. })
    }

    /// What a dimension measures with its points at `at` (in [`Self::points`] order).
    fn measure_at(&self, at: &[Vec3]) -> Option<f64> {
        let p = |i: usize| at.get(i).copied().unwrap_or(Vec3::ZERO);
        match self {
            WireConstraintKind::Distance { .. } => Some(p(0).dist(p(1))),
            WireConstraintKind::AxisDistance { axis, reversed, .. } => {
                let d = (p(1) - p(0)).dot(axis.unit());
                Some(if *reversed { -d } else { d })
            }
            WireConstraintKind::Angle { .. } => {
                let (u, v) = (p(1) - p(0), p(3) - p(2));
                Some(u.cross(v).len().atan2(u.dot(v)))
            }
            WireConstraintKind::Radius { .. } => {
                // The circle through the arc's three points: |ab| |bc| |ca| / (2 |ab × ac|).
                let (a, b, c) = (p(0), p(1), p(2));
                let twice_area = (b - a).cross(c - a).len();
                Some(if twice_area > 1e-18 { a.dist(b) * b.dist(c) * c.dist(a) / (2.0 * twice_area) } else { MAX_WIRE_LENGTH * 10.0 })
            }
            _ => None,
        }
    }

    /// Its residuals, from the positions of [`Self::points`] in that order.
    fn residuals(&self, at: &[Vec3]) -> Vec<f64> {
        let p = |i: usize| at.get(i).copied().unwrap_or(Vec3::ZERO);
        match self {
            WireConstraintKind::Coincident { .. } => {
                let d = p(0) - p(1);
                vec![d.x, d.y, d.z]
            }
            WireConstraintKind::PointOnLine { .. } => {
                // The point's offset from the line, square to it: (p − a) × d / |d|.
                let d = p(2) - p(1);
                let n = d.len().max(1e-12);
                let c = (p(0) - p(1)).cross(d) * (1.0 / n);
                vec![c.x, c.y, c.z]
            }
            WireConstraintKind::Fix { at, .. } => {
                let d = p(0) - *at;
                vec![d.x, d.y, d.z]
            }
            WireConstraintKind::AlongAxis { axis, .. } => {
                let d = p(1) - p(0);
                match axis {
                    Axis::X => vec![d.y, d.z],
                    Axis::Y => vec![d.z, d.x],
                    Axis::Z => vec![d.x, d.y],
                }
            }
            WireConstraintKind::Distance { value, .. }
            | WireConstraintKind::AxisDistance { value, .. }
            | WireConstraintKind::Angle { value, .. }
            | WireConstraintKind::Radius { value, .. } => vec![self.measure_at(at).unwrap_or(0.0) - *value],
        }
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

/// How a 3D solve went.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WireReport {
    /// Every 3D constraint holds.
    pub ok: bool,
    /// Remaining degrees of freedom of the drawn 3D points (3 per point, less what the 3D
    /// constraints determine).
    pub dof: usize,
    /// The constraints left violated by a solve that failed.
    pub failing: Vec<String>,
    /// The unknowns of a connected group too large to solve at once.
    pub oversized: Option<usize>,
}

impl WireReport {
    fn new() -> WireReport {
        WireReport { ok: true, ..WireReport::default() }
    }
}

impl Sketch {
    /// A drawn (not linked) 3D curve by id.
    fn drawn_wire(&self, id: &str) -> Option<&crate::Wire> {
        self.wires.iter().find(|w| w.id == id && w.link.is_none() && !w.fit.is_empty())
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
        let bad = |m: String| Err(SketchError::Invalid(m));
        for p in k.points() {
            if self.wire_point_at(&p).is_none() {
                return bad(format!("`{}` has no drawn 3D point {}", p.wire, p.index));
            }
        }
        match k {
            WireConstraintKind::PointOnLine { line, .. } | WireConstraintKind::AlongAxis { line, .. } if !self.is_drawn_line(line) => {
                return bad(format!("`{line}` is not a drawn 3D line"));
            }
            WireConstraintKind::Coincident { a, b } if a == b => return bad("a point cannot be coincident with itself".into()),
            WireConstraintKind::PointOnLine { p, line } if p.wire == *line => return bad("a line's own end is already on it".into()),
            WireConstraintKind::Fix { at, .. } if !in_range3(*at) => return bad("3D point out of range".into()),
            WireConstraintKind::Distance { a, b, .. } | WireConstraintKind::AxisDistance { a, b, .. } if a == b => {
                return bad("a distance needs two different points".into());
            }
            WireConstraintKind::Angle { a, b, .. } if a == b || !self.is_drawn_line(a) || !self.is_drawn_line(b) => {
                return bad("an angle needs two different drawn 3D lines".into());
            }
            WireConstraintKind::Radius { arc, .. } if !self.drawn_wire(arc).is_some_and(|w| w.arc.is_some()) => {
                return bad(format!("`{arc}` is not a drawn 3D arc"));
            }
            _ => {}
        }
        if let Some(v) = k.value() {
            let ok = match k {
                WireConstraintKind::Angle { .. } => (0.0..=std::f64::consts::PI).contains(&v),
                WireConstraintKind::Radius { .. } => v > 1e-9 && v <= MAX_WIRE_LENGTH,
                WireConstraintKind::AxisDistance { .. } => v.abs() <= MAX_WIRE_LENGTH,
                _ => (0.0..=MAX_WIRE_LENGTH).contains(&v),
            };
            if !ok {
                return bad(match k {
                    WireConstraintKind::Angle { .. } => "a 3D angle must be between 0 and 180 degrees".to_string(),
                    WireConstraintKind::Radius { .. } => format!("a 3D radius must be above 0 and at most {MAX_WIRE_LENGTH} mm"),
                    _ => format!("a 3D distance must be at most {MAX_WIRE_LENGTH} mm (and not negative)"),
                });
            }
        }
        if let WireConstraintKind::Fix { exprs: Some(e), .. } = k
            && e.list().iter().any(|(_, x)| x.is_empty() || x.len() > 4096)
        {
            return bad("a 3D point's expression must be 1 to 4096 characters".into());
        }
        let spaces: Vec<_> = k.wires().iter().filter_map(|id| self.drawn_wire(id).map(|w| w.space)).collect();
        if spaces.windows(2).any(|w| w[0] != w[1]) {
            return bad("these 3D curves are in different coordinate spaces (one fixed in the world, one in its component)".into());
        }
        Ok(())
    }

    /// Check every stored 3D constraint (as a design is opened).
    pub fn validate_wire_constraints(&self) -> Result<()> {
        if self.wire_constraints.len() > MAX_CONSTRAINTS {
            return Err(SketchError::TooLarge);
        }
        let mut ids = std::collections::BTreeSet::new();
        for c in &self.wire_constraints {
            if c.id.is_empty() || c.id.len() > 512 || !ids.insert(c.id.as_str()) {
                return Err(SketchError::Invalid("3D constraint ids are invalid".into()));
            }
            self.validate_wire_constraint(&c.kind)?;
        }
        Ok(())
    }

    /// Add a 3D constraint; one that says what another already says returns that one's id.
    pub fn add_wire_constraint(&mut self, kind: WireConstraintKind) -> Result<String> {
        self.add_wire_dimension(kind, None, false)
    }

    /// Add a 3D constraint or dimension; a dimension takes its model parameter (`param`) or is
    /// driven (it measures). A dimension on geometry another dimension of that kind already
    /// measures is refused, naming it.
    pub fn add_wire_dimension(&mut self, kind: WireConstraintKind, param: Option<String>, driven: bool) -> Result<String> {
        self.validate_wire_constraint(&kind)?;
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
        let driven = driven && kind.is_dimension();
        let param = if driven || !kind.is_dimension() { None } else { param };
        self.wire_constraints.push(WireConstraint { id: id.clone(), kind, param, driven });
        Ok(id)
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

    /// Join drawn 3D curves end to end: each curve's end coincident with the next one's start,
    /// where they meet. Returns the constraints added.
    pub fn join_drawn_wires(&mut self, ids: &[String]) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for pair in ids.windows(2) {
            let [a, b] = pair else { continue };
            let (Some(end), Some(start)) = (self.wire_point(&format!("{a}.end")), self.wire_point(&format!("{b}.start"))) else { continue };
            let meet = self.wire_point_at(&end).zip(self.wire_point_at(&start)).is_some_and(|(p, q)| p.dist(q) < 1e-9);
            if meet {
                out.push(self.add_wire_constraint(WireConstraintKind::Coincident { a: end, b: start })?);
            }
        }
        Ok(out)
    }

    /// Drop the 3D constraints that name a curve the sketch no longer has, or a fit point it no
    /// longer has.
    pub(crate) fn prune_wire_constraints(&mut self) {
        let keep: Vec<bool> = self.wire_constraints.iter().map(|c| self.validate_wire_constraint(&c.kind).is_ok()).collect();
        let mut keep = keep.into_iter();
        self.wire_constraints.retain(|_| keep.next().unwrap_or(false));
    }

    /// Solve the 3D constraints, moving drawn fit points. With `hold`, that point is kept at
    /// its position when the constraints allow (a drag); otherwise it moves like any other.
    /// Planar geometry is never touched. A failed solve leaves every point where it was.
    pub fn solve_wires(&mut self, hold: Option<&WirePoint>) -> WireReport {
        let mut rep = WireReport::new();
        // The unknowns: every fit point of every drawn curve, three coordinates each.
        let mut slots: Vec<WirePoint> = Vec::new();
        let mut slot_of = std::collections::HashMap::new();
        for w in self.wires.iter().filter(|w| w.link.is_none()) {
            for index in 0..w.fit.len() {
                let p = WirePoint { wire: w.id.clone(), index };
                slot_of.insert(p.clone(), slots.len());
                slots.push(p);
            }
        }
        let total = slots.len().saturating_mul(3);
        if self.wire_constraints.iter().all(|c| c.driven) {
            rep.dof = total;
            let measured: Vec<Option<f64>> = self.wire_constraints.iter().map(|c| self.measure_wire_dimension(&c.kind)).collect();
            for (c, m) in self.wire_constraints.iter_mut().zip(measured) {
                if let Some(v) = m.filter(|v| v.is_finite()) {
                    c.kind.set_value(v);
                }
            }
            return rep;
        }
        let cons: Vec<(String, WireConstraintKind, Vec<usize>)> = self
            .wire_constraints
            .iter()
            .filter(|c| !c.driven)
            .filter_map(|c| {
                let pts: Option<Vec<usize>> = c.kind.points().iter().map(|p| slot_of.get(p).copied()).collect();
                pts.map(|pts| (c.id.clone(), c.kind.clone(), pts))
            })
            .collect();
        // Connected groups of points.
        let mut parent: Vec<usize> = (0..slots.len()).collect();
        for (_, _, pts) in &cons {
            for w in pts.windows(2) {
                crate::union_find::union(&mut parent, w[0], w[1]);
            }
        }
        let held = hold.and_then(|h| slot_of.get(h).copied());
        let fixed_at: std::collections::HashMap<usize, Vec3> = cons
            .iter()
            .filter_map(|(_, k, pts)| match k {
                WireConstraintKind::Fix { at, .. } => pts.first().map(|p| (*p, *at)),
                _ => None,
            })
            .collect();
        let mut positions: Vec<Vec3> = slots.iter().map(|p| self.wire_point_at(p).unwrap_or(Vec3::ZERO)).collect();
        let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
        for (ci, (_, _, pts)) in cons.iter().enumerate() {
            if let Some(first) = pts.first() {
                groups.entry(crate::union_find::find(&mut parent, *first)).or_default().push(ci);
            }
        }
        let mut rank = 0usize;
        for members in groups.values() {
            let mut pts: Vec<usize> = members.iter().flat_map(|ci| cons.get(*ci).map(|c| c.2.clone()).unwrap_or_default()).collect();
            pts.sort_unstable();
            pts.dedup();
            let n = pts.len().saturating_mul(3);
            if n > MAX_WIRE_VARS {
                rep.ok = false;
                rep.oversized = Some(n);
                continue;
            }
            let group: Vec<&(String, WireConstraintKind, Vec<usize>)> = members.iter().filter_map(|ci| cons.get(*ci)).collect();
            // A fixed point starts where it is fixed.
            let start: Vec<Vec3> =
                pts.iter().map(|i| fixed_at.get(i).copied().or_else(|| positions.get(*i).copied()).unwrap_or(Vec3::ZERO)).collect();
            let fixed: Vec<bool> = pts.iter().map(|i| fixed_at.contains_key(i)).collect();
            let hold_at = held.and_then(|h| pts.iter().position(|p| *p == h));
            let mut solved = None;
            if let Some(k) = hold_at {
                solved = lm(&group, &pts, &start, &fixed, Some(k)).filter(|r| r.1.is_empty());
            }
            let (x, failing, r) = match solved {
                Some(s) => s,
                None => match lm(&group, &pts, &start, &fixed, None) {
                    Some(s) => s,
                    None => (start.clone(), group.iter().map(|c| c.0.clone()).collect(), 0),
                },
            };
            if failing.is_empty() {
                for (k, i) in pts.iter().enumerate() {
                    if let (Some(slot), Some(v)) = (positions.get_mut(*i), x.get(k)) {
                        *slot = *v;
                    }
                }
                rank = rank.saturating_add(r);
            } else {
                rep.ok = false;
                rep.failing.extend(failing);
            }
        }
        rep.dof = total.saturating_sub(rank);
        if !rep.ok {
            return rep;
        }
        // Write the points back, rebuilding each curve from its fit points; a curve that can no
        // longer be built (an arc's points now in a line) fails the solve and nothing moves.
        let mut fits: Vec<(usize, Vec<Vec3>)> = Vec::new();
        for (wi, w) in self.wires.iter().enumerate().filter(|(_, w)| w.link.is_none() && !w.fit.is_empty()) {
            let fit: Vec<Vec3> = (0..w.fit.len())
                .map(|index| slot_of.get(&WirePoint { wire: w.id.clone(), index }).and_then(|s| positions.get(*s)).copied().unwrap_or(Vec3::ZERO))
                .collect();
            if fit != w.fit {
                fits.push((wi, fit));
            }
        }
        let mut rebuilt = Vec::with_capacity(fits.len());
        for (wi, fit) in fits {
            let Some(w) = self.wires.get(wi) else { continue };
            if fit.iter().any(|p| !in_range3(*p)) {
                rep.ok = false;
                rep.failing.extend(self.wire_constraints.iter().filter(|c| c.kind.wires().contains(&w.id.as_str())).map(|c| c.id.clone()));
                return rep;
            }
            let built = match w.arc {
                Some(_) => match fit.as_slice() {
                    [a, b, c] => arc_from_3_points(*a, *b, *c).map(|arc| (Some(arc), arc_polyline(&arc, ARC_TOL))),
                    _ => None,
                },
                None if fit.windows(2).all(|p| p[0].dist(p[1]) >= 1e-9) => Some((None, fit_polyline(&fit))),
                None => None,
            };
            match built {
                Some((arc, pts)) => rebuilt.push((wi, fit, arc, pts)),
                None => {
                    rep.ok = false;
                    rep.failing.extend(self.wire_constraints.iter().filter(|c| c.kind.wires().contains(&w.id.as_str())).map(|c| c.id.clone()));
                    return rep;
                }
            }
        }
        for (wi, fit, arc, pts) in rebuilt {
            if let Some(w) = self.wires.get_mut(wi) {
                w.fit = fit;
                w.arc = arc;
                w.pts = pts;
            }
        }
        // Driven dimensions measure the solved geometry.
        let measured: Vec<Option<f64>> =
            self.wire_constraints.iter().map(|c| if c.driven { self.measure_wire_dimension(&c.kind) } else { None }).collect();
        for (c, m) in self.wire_constraints.iter_mut().zip(measured) {
            if let Some(v) = m.filter(|v| v.is_finite()) {
                c.kind.set_value(v);
            }
        }
        rep
    }

    /// Remove 3D constraints by id; returns how many went.
    pub fn remove_wire_constraints(&mut self, ids: &[String]) -> usize {
        let before = self.wire_constraints.len();
        self.wire_constraints.retain(|c| !ids.contains(&c.id));
        before - self.wire_constraints.len()
    }
}

/// Levenberg–Marquardt on one connected group. `pts` are the group's point slots, `start` their
/// positions; with `hold`, that point (an index into `pts`) is also pulled to where it starts.
/// Returns the positions, the constraints still violated (empty when solved) and the rank of
/// the constraints' Jacobian at the result, or `None` when the numbers stop being finite.
#[allow(clippy::type_complexity)]
fn lm(
    group: &[&(String, WireConstraintKind, Vec<usize>)],
    pts: &[usize],
    start: &[Vec3],
    fixed: &[bool],
    hold: Option<usize>,
) -> Option<(Vec<Vec3>, Vec<String>, usize)> {
    // A fixed point is a constant, as in the planar solve: only the others are unknowns.
    let mut var = Vec::with_capacity(pts.len());
    let mut count = 0usize;
    for k in 0..pts.len() {
        if fixed.get(k).copied().unwrap_or(false) {
            var.push(None);
        } else {
            var.push(Some(count));
            count += 1;
        }
    }
    let hold = hold.filter(|k| var.get(*k).copied().flatten().is_some());
    let n = count * 3;
    let local: Vec<Vec<usize>> = group.iter().map(|c| c.2.iter().filter_map(|p| pts.iter().position(|q| q == p)).collect()).collect();
    let target = hold.and_then(|k| start.get(k).copied());
    let pos = |x: &[f64], k: usize| match var.get(k).copied().flatten() {
        Some(v) => {
            Vec3::new(x.get(3 * v).copied().unwrap_or(0.0), x.get(3 * v + 1).copied().unwrap_or(0.0), x.get(3 * v + 2).copied().unwrap_or(0.0))
        }
        None => start.get(k).copied().unwrap_or(Vec3::ZERO),
    };
    let residuals = |x: &[f64]| -> Vec<f64> {
        let mut r = Vec::new();
        for (c, l) in group.iter().zip(&local) {
            let at: Vec<Vec3> = l.iter().map(|k| pos(x, *k)).collect();
            r.extend(c.1.residuals(&at));
        }
        if let (Some(k), Some(t)) = (hold, target) {
            let d = pos(x, k) - t;
            r.extend([d.x, d.y, d.z]);
        }
        r
    };
    // The Jacobian, each constraint differentiated numerically over its own points only.
    let jacobian = |x: &[f64], with_hold: bool| -> Mat {
        let rows: usize =
            group.iter().zip(&local).map(|(c, l)| c.1.residuals(&vec![Vec3::ZERO; l.len()]).len()).sum::<usize>() + if with_hold { 3 } else { 0 };
        let mut j = Mat::zeros(rows, n);
        let mut row = 0;
        for (c, l) in group.iter().zip(&local) {
            let at: Vec<Vec3> = l.iter().map(|k| pos(x, *k)).collect();
            let r0 = c.1.residuals(&at);
            // Central differences: the derivative's error is of the order STEP², so a solve
            // with freedom left moves a point (nearly) straight to where it must go.
            let moved = |slot: usize, axis: usize, by: f64| -> Vec<f64> {
                let mut m = at.clone();
                if let Some(p) = m.get_mut(slot) {
                    match axis {
                        0 => p.x += by,
                        1 => p.y += by,
                        _ => p.z += by,
                    }
                }
                c.1.residuals(&m)
            };
            for (slot, k) in l.iter().enumerate() {
                let Some(v) = var.get(*k).copied().flatten() else { continue };
                for axis in 0..3 {
                    let (up, down) = (moved(slot, axis, STEP), moved(slot, axis, -STEP));
                    for (i, (a, b)) in down.iter().zip(&up).enumerate() {
                        j.add(row + i, 3 * v + axis, (b - a) / (2.0 * STEP));
                    }
                }
            }
            row += r0.len();
        }
        if let (true, Some(v)) = (with_hold, hold.and_then(|k| var.get(k).copied().flatten())) {
            for axis in 0..3 {
                j.set(row + axis, 3 * v + axis, 1.0);
            }
        }
        j
    };
    let mut x: Vec<f64> = start.iter().zip(&var).filter(|(_, v)| v.is_some()).flat_map(|(p, _)| [p.x, p.y, p.z]).collect();
    let cost = |r: &[f64]| r.iter().map(|v| v * v).sum::<f64>();
    let mut r = residuals(&x);
    let mut lambda = 1e-3;
    for _ in 0..MAX_ITERS {
        if r.iter().all(|v| v.abs() < TOL) {
            break;
        }
        let j = jacobian(&x, hold.is_some());
        // (JᵀJ + λI) dx = −Jᵀr: the damping keeps each step short, so a group with freedom left
        // moves as little as it can.
        let mut a = Mat::zeros(n, n);
        let mut g = vec![0.0; n];
        for row in 0..j.rows {
            let rv = r.get(row).copied().unwrap_or(0.0);
            for c in 0..n {
                let jc = j.get(row, c);
                if jc == 0.0 {
                    continue;
                }
                if let Some(gc) = g.get_mut(c) {
                    *gc -= jc * rv;
                }
                for d in 0..n {
                    let jd = j.get(row, d);
                    if jd != 0.0 {
                        a.add(c, d, jc * jd);
                    }
                }
            }
        }
        let mut improved = false;
        for _ in 0..12 {
            let mut damped = a.clone();
            for c in 0..n {
                damped.add(c, c, lambda);
            }
            let Some(dx) = linalg::solve(damped, g.clone()) else {
                lambda *= 10.0;
                continue;
            };
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
        if !improved {
            break;
        }
    }
    if !x.iter().all(|v| v.is_finite()) {
        return None;
    }
    // Which constraints are left violated (the hold is not a constraint).
    let mut failing = Vec::new();
    for (c, l) in group.iter().zip(&local) {
        let at: Vec<Vec3> = l.iter().map(|k| pos(&x, *k)).collect();
        if c.1.residuals(&at).iter().any(|v| v.abs() > ACCEPT) {
            failing.push(c.0.clone());
        }
    }
    let j = jacobian(&x, false);
    let (_, rank) = linalg::determined_vars(&j, 1e-8);
    let fixed_count = pts.len() - count;
    Some(((0..pts.len()).map(|k| pos(&x, k)).collect(), failing, rank + 3 * fixed_count))
}

#[cfg(test)]
#[path = "wire_solve_tests.rs"]
mod tests;

//! Equation curves (#8352): a fit point spline whose points come from formulas. The sketch keeps
//! the formulas beside the spline; the design's parameters are worked out in the document layer
//! (`solidcraft_doc::sketch_equation`), which places the points again before every solve. The
//! points are fixed for the solver, so dragging never bends the curve, while other geometry can
//! still share its end points.

use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec2;

use crate::model::{CurveKind, Result, Sketch, SketchError};

/// Longest formula or range expression an equation curve takes (characters).
pub const MAX_EQUATION_TEXT: usize = 1024;

/// A curve drawn from formulas: x(t) and y(t) over t from `start` to `end`, or (with `x` left
/// out) the explicit y = f(x) over x from `start` to `end`. The formulas use the design's
/// expression language and parameters; the variable is a plain number, so `sin(t)` reads it in
/// radians, and a result without a unit is in the design's length unit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EquationCurve {
    /// The fit point spline it places (a curve id of the sketch).
    pub curve: String,
    /// x(t). `None`: the explicit form, where the variable is x itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
    /// y(t), or f(x) in the explicit form.
    pub y: String,
    /// Where the variable starts and ends.
    pub start: String,
    pub end: String,
    /// How far (mm) the spline may stray from the formulas: the number of points was chosen for
    /// it when the curve was made or last edited.
    pub tolerance: f64,
}

impl EquationCurve {
    /// The name the formulas use for the curve's variable.
    pub fn variable(&self) -> &'static str {
        if self.x.is_some() { "t" } else { "x" }
    }

    /// The formulas, each with what it sets (for the parameters a sketch uses).
    pub fn list(&self) -> Vec<(&'static str, &String)> {
        let mut out = Vec::with_capacity(4);
        if let Some(x) = &self.x {
            out.push(("Curve X", x));
        }
        out.extend([("Curve Y", &self.y), ("Curve Start", &self.start), ("Curve End", &self.end)]);
        out
    }

    /// Mutable view of [`EquationCurve::list`], for renaming the parameters they refer to.
    pub fn list_mut(&mut self) -> Vec<&mut String> {
        let mut out: Vec<&mut String> = self.x.iter_mut().collect();
        out.extend([&mut self.y, &mut self.start, &mut self.end]);
        out
    }
}

impl Sketch {
    /// The equation behind a curve, when it is an equation curve.
    pub fn equation_of(&self, curve: &str) -> Option<&EquationCurve> {
        self.equations.iter().find(|e| e.curve == curve)
    }

    /// Add an equation curve: a cubic fit spline through `at` (fixed points), closed smoothly
    /// when `closed` (then the last position is the first one again and is not repeated), with
    /// `equation` kept beside it. Returns the curve's index.
    pub fn add_equation_curve(&mut self, at: &[Vec2], closed: bool, mut equation: EquationCurve) -> Result<usize> {
        let pts = self.equation_points(at, closed, None)?;
        let curve = self.add_curve(CurveKind::Spline { pts, control: false, degree: 3, periodic: closed, handles: Vec::new() }, None)?;
        equation.curve = self.curves.get(curve).map(|c| c.id.clone()).ok_or_else(|| SketchError::Invalid("the new curve is missing".into()))?;
        self.equations.push(equation);
        Ok(curve)
    }

    /// Give an equation curve new points (its formulas were edited): the start point (and the
    /// end point of an open curve) keep their ids, so geometry joined there stays joined; the
    /// points in between are made again, and the old ones go with whatever referred to them.
    pub fn replace_equation_curve(&mut self, curve: usize, at: &[Vec2], closed: bool, equation: EquationCurve) -> Result<()> {
        let Some(CurveKind::Spline { pts: old, control: false, .. }) = self.curves.get(curve).map(|c| c.kind.clone()) else {
            return Err(SketchError::Invalid("an equation curve must be a fit point spline".into()));
        };
        let (Some(first), Some(last)) = (old.first().copied(), old.last().copied()) else {
            return Err(SketchError::Invalid("the equation curve has no points".into()));
        };
        let id = self.curves.get(curve).map(|c| c.id.clone()).ok_or_else(|| SketchError::Invalid("unknown curve".into()))?;
        let keep_end = (last != first).then_some(last);
        let pts = self.equation_points(at, closed, Some((first, keep_end)))?;
        let unused: Vec<usize> = old.iter().copied().filter(|p| !pts.contains(p)).collect();
        if let Some(c) = self.curves.get_mut(curve) {
            c.kind = CurveKind::Spline { pts, control: false, degree: 3, periodic: closed, handles: Vec::new() };
        }
        // The old points no other curve uses (the in-between points, and an end a closed curve
        // no longer has) go, with the constraints on them.
        let mut gone: Vec<usize> = unused.into_iter().filter(|p| *p != 0 && !self.curves.iter().any(|c| c.kind.uses(*p))).collect();
        gone.sort_unstable();
        gone.dedup();
        self.remove_points(&gone);
        match self.equations.iter_mut().find(|e| e.curve == id) {
            Some(e) => *e = EquationCurve { curve: id, ..equation },
            None => self.equations.push(EquationCurve { curve: id, ..equation }),
        }
        Ok(())
    }

    /// The point indices of an equation curve through `at`: new fixed points, or (`reuse`) the
    /// given start point and, for an open curve, end point moved to where the curve now starts
    /// and ends.
    fn equation_points(&mut self, at: &[Vec2], closed: bool, reuse: Option<(usize, Option<usize>)>) -> Result<Vec<usize>> {
        let wanted = if closed { 3 } else { 2 };
        if at.len() < wanted || at.len() > crate::curves::MAX_SPLINE_POINTS {
            return Err(SketchError::Invalid(format!("an equation curve needs {wanted} to {} points", crate::curves::MAX_SPLINE_POINTS)));
        }
        if self.points.len().saturating_add(at.len()) > crate::model::MAX_POINTS {
            return Err(SketchError::TooLarge);
        }
        let place = |sk: &mut Sketch, existing: Option<usize>, p: Vec2| -> Result<usize> {
            let i = match existing {
                Some(i) => {
                    let q = sk.points.get_mut(i).ok_or_else(|| SketchError::Invalid("missing equation curve point".into()))?;
                    q.pos = Sketch::check_pos(p)?;
                    i
                }
                None => sk.add_point(p, None)?,
            };
            if let Some(q) = sk.points.get_mut(i) {
                q.fixed = true;
            }
            Ok(i)
        };
        let (first_at, rest) = at.split_first().ok_or_else(|| SketchError::Invalid("no points".into()))?;
        let first = place(self, reuse.map(|r| r.0), *first_at)?;
        let mut pts = vec![first];
        let (middle, end_at) = if closed {
            // The last position is the first one again: the curve closes on its first point.
            (rest.split_last().map(|(_, m)| m).unwrap_or(&[]), None)
        } else {
            let (end_at, middle) = rest.split_last().ok_or_else(|| SketchError::Invalid("no end point".into()))?;
            (middle, Some(*end_at))
        };
        for p in middle {
            pts.push(place(self, None, *p)?);
        }
        match end_at {
            Some(p) => pts.push(place(self, reuse.and_then(|r| r.1), p)?),
            None => pts.push(first),
        }
        Ok(pts)
    }

    /// Move an equation curve's points to `at` (one position per point of the spline, in order;
    /// a closed curve's last point is its first one, and its position is skipped). Every point
    /// is fixed. Used each time the formulas are worked out again.
    pub fn place_equation_points(&mut self, curve: usize, at: &[Vec2]) -> Result<()> {
        let Some(CurveKind::Spline { pts, control: false, .. }) = self.curves.get(curve).map(|c| &c.kind) else {
            return Err(SketchError::Invalid("an equation curve must be a fit point spline".into()));
        };
        if pts.len() != at.len() {
            return Err(SketchError::Invalid("the equation curve's point count changed".into()));
        }
        let moves: Vec<(usize, Vec2)> = pts.iter().copied().zip(at.iter().copied()).collect();
        for (i, p) in moves {
            let p = Self::check_pos(p)?;
            let q = self.points.get_mut(i).ok_or_else(|| SketchError::Invalid("missing equation curve point".into()))?;
            q.pos = p;
            q.fixed = true;
        }
        Ok(())
    }

    /// The equation curves' formulas, each with the curve's variable: a parameter of that name
    /// is hidden from the formulas by it, so a rename of it must not touch them.
    pub fn equation_exprs_mut(&mut self) -> Vec<(&'static str, &mut String)> {
        self.equations
            .iter_mut()
            .flat_map(|e| {
                let variable = e.variable();
                e.list_mut().into_iter().map(move |s| (variable, s))
            })
            .collect()
    }

    /// Forget the equations of removed curves.
    pub(crate) fn prune_equations(&mut self, removed: &std::collections::BTreeSet<String>) {
        self.equations.retain(|e| !removed.contains(&e.curve));
    }
}

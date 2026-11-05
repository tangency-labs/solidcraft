//! Values typed as definitions, `name = expression` (#4071): any value input, typed or sent as
//! a command's parameter, can name its value. Until the design stores it, such a text evaluates
//! as its expression (`expr::definition`), so a command reads and checks it like any other value;
//! [`Document::define_inline`] then makes the user parameter `name` with that expression and
//! leaves `name` where the text was.

use serde::Serialize;

use crate::expr::{self, Kind};
use crate::params::ExprRole;
use crate::walk::Walk;
use crate::{DocError, Document, FeatureKind, Result};

/// A user parameter a value typed as `name = expression` defined.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Defined {
    pub name: String,
    pub expression: String,
    pub unit: String,
}

/// How the place a definition was typed in reads a bare number, which the parameter keeps.
#[derive(Clone, Copy)]
enum Reads<'a> {
    /// A typed value of this kind: lengths in the design's units, angles in degrees.
    Typed(Kind),
    /// A model parameter's (a dimension's) own unit.
    Unit(&'a str),
    /// A primitive's placement coordinate: millimetres whatever the design's units.
    Millimetres,
    /// A value of no fixed kind: the parameter takes its value's unit.
    Inferred,
}

/// A definition found: the name, its expression and its unit (`None`: its value's own).
struct Pending {
    name: String,
    expression: String,
    unit: Option<String>,
}

/// The definitions found so far: each slot's text becomes the name, the definition is kept.
struct Found {
    units: String,
    defs: Vec<Pending>,
    problem: Option<String>,
}

impl Found {
    /// If `text` is a definition, note it and leave the name in its place.
    fn take(&mut self, text: &mut String, reads: Reads) {
        let Some((name, rhs)) = expr::definition(text) else { return };
        if rhs.is_empty() {
            self.problem.get_or_insert_with(|| format!("`{name} =` needs a value after the `=`"));
            return;
        }
        let (expression, unit) = match reads {
            Reads::Typed(Kind::Length) => {
                let units = expr::length_unit_or_mm(&self.units);
                (with_unit(rhs, units), Some(units.to_string()))
            }
            Reads::Typed(Kind::Angle) => (rhs.to_string(), Some("deg".to_string())),
            Reads::Typed(Kind::Unitless) => (rhs.to_string(), Some(String::new())),
            Reads::Unit(u) => (rhs.to_string(), Some(u.to_string())),
            Reads::Millimetres => (rhs.to_string(), Some("mm".to_string())),
            Reads::Inferred => (rhs.to_string(), None),
        };
        let name = name.to_string();
        match self.defs.iter().find(|d| d.name == name) {
            // The same definition in two places (one typed value a command stores twice).
            Some(d) if d.expression == expression && d.unit == unit => {}
            Some(_) => {
                self.problem.get_or_insert_with(|| format!("`{name}` is defined twice with different values"));
                return;
            }
            None => self.defs.push(Pending { name: name.clone(), expression, unit }),
        }
        *text = name;
    }

    /// [`Found::take`] for an expression field.
    fn take_expr(&mut self, e: &mut expr::Expr, reads: Reads) {
        if expr::definition(e.as_str()).is_none() {
            return;
        }
        let mut text = e.as_str().to_string();
        self.take(&mut text, reads);
        *e = text.into();
    }
}

/// A bare number in the design's length unit, as a typed length reads it.
fn with_unit(rhs: &str, units: &str) -> String {
    if units != "mm" && expr::is_bare_number(rhs) { format!("{} {units}", rhs.trim()) } else { rhs.to_string() }
}

impl Document {
    /// Store every value typed as `name = expression`: the user parameter `name` with that
    /// expression (a bare number read as the value would read it, in that unit), and `name`
    /// where the text was. A name that is taken or reserved is refused, as is a definition where
    /// none can be made: in a parameter's own expression or a configuration cell. All or nothing:
    /// on an error the design is unchanged.
    pub fn define_inline(&mut self) -> Result<Vec<Defined>> {
        // Where a definition is refused: a parameter's expression defines nothing else (the
        // parameters command renames the parameter instead), nor does a configuration row.
        if let Some(p) = self.params.iter().find(|p| !p.model && expr::definition(&p.expr).is_some()) {
            let other = expr::definition(&p.expr).map(|(n, _)| n).unwrap_or_default();
            return Err(DocError::Invalid(format!(
                "the expression of `{}` cannot define another parameter `{other}`: add `{other}` as a parameter of its own",
                p.name
            )));
        }
        // Only a parameter's cells are expressions (a text parameter's are texts).
        let numeric: Vec<bool> =
            self.configs.columns.iter().map(|c| matches!(c, crate::config::Column::Param(n) if self.text_param(n).is_none())).collect();
        let mut cells = self.configs.rows.iter().flat_map(|r| r.cells.iter().zip(&numeric).filter(|(_, n)| **n).map(|(c, _)| c));
        if let Some(other) = cells.find_map(|c| expr::definition(c).map(|(n, _)| n)) {
            return Err(DocError::Invalid(format!("a configuration cell cannot define the parameter `{other}`: add it in Parameters first")));
        }
        let mut next = self.clone();
        let mut found = Found { units: next.units.clone(), defs: Vec::new(), problem: None };
        for p in next.params.iter_mut().filter(|p| p.model) {
            let unit = p.unit.clone();
            found.take(&mut p.expr, Reads::Unit(&unit));
        }
        for f in &mut next.features {
            f.kind.walk_exprs_mut(&mut |(_, e, kind, role)| {
                let reads = if role == ExprRole::Placement { Reads::Millimetres } else { Reads::Typed(kind) };
                found.take_expr(e, reads);
            });
            if let FeatureKind::Sketch { sketch, .. } = &mut f.kind {
                for p in &mut sketch.patterns {
                    if let Some(e) = p.count_expression.as_mut() {
                        found.take(e, Reads::Typed(Kind::Unitless));
                    }
                    for (key, e) in &mut p.expressions {
                        let kind = match key.as_str() {
                            "count" | "count2" => Kind::Unitless,
                            "angle" => Kind::Angle,
                            _ => Kind::Length,
                        };
                        found.take(e, Reads::Typed(kind));
                    }
                }
                for link in &mut sketch.links {
                    if let solidcraft_sketch::LinkSource::Text { exprs, .. } = &mut link.source {
                        for e in [&mut exprs.x, &mut exprs.y, &mut exprs.height].into_iter().flatten() {
                            found.take(e, Reads::Typed(Kind::Length));
                        }
                        if let Some(e) = exprs.angle.as_mut() {
                            found.take(e, Reads::Typed(Kind::Angle));
                        }
                    }
                }
            }
        }
        for e in next.assembly.exprs_mut() {
            found.take(e, Reads::Inferred);
        }
        for r in &mut next.sheet.rules {
            let kinds = r.fields().map(|(_, _, k)| k);
            for ((_, e), k) in r.fields_mut().into_iter().zip(kinds) {
                found.take(e, Reads::Typed(k));
            }
        }
        for r in &mut next.plastic.rules {
            let kinds = r.fields().map(|(_, _, k)| k);
            for ((_, e), k) in r.fields_mut().into_iter().zip(kinds) {
                found.take(e, Reads::Typed(k));
            }
        }
        for c in &mut next.canvases {
            c.plane.walk_exprs_mut(&mut |(_, e, kind, _)| found.take_expr(e, Reads::Typed(kind)));
        }
        if let Some(problem) = found.problem {
            return Err(DocError::Invalid(problem));
        }
        if found.defs.is_empty() {
            return Ok(Vec::new());
        }
        for d in &found.defs {
            if let Some(problem) = Self::param_name_problem(&d.name) {
                return Err(DocError::Invalid(problem));
            }
            if self.name_taken(&d.name) {
                return Err(DocError::Invalid(format!(
                    "there is already a parameter `{}`: type `{}` to use it, or change it in Parameters",
                    d.name, d.name
                )));
            }
        }
        let mut out = Vec::with_capacity(found.defs.len());
        for d in found.defs {
            // A value of no fixed kind (a joint's) is evaluated without the design's units, so
            // its bare number stays a millimetre length whatever they are (#8865): `w = 5` is
            // what `5` is there.
            let unit = d.unit.unwrap_or_else(|| Self::infer_unit_in(&next.param_values().0, &d.expression, "mm"));
            next.add_user_param(&d.name, &d.expression, Some(&unit), None)
                .map_err(|e| DocError::Invalid(format!("`{} = {}`: {e}", d.name, d.expression)))?;
            let unit = next.param(&d.name).map(|p| p.unit.clone()).unwrap_or_default();
            out.push(Defined { name: d.name, expression: d.expression, unit });
        }
        *self = next;
        Ok(out)
    }
}

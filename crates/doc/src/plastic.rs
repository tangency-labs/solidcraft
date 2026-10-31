//! Plastic rules: wall thickness, draft, radii and clearances for molded parts, as expressions
//! (`Thickness` inside them is the rule's thickness). A body assigned a rule (or every body,
//! when a rule is active) takes its draft and clearance as the defaults of plastic features.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::expr::{self, Kind, Value};
use crate::{DocError, Document, Result};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlasticRule {
    pub name: String,
    #[serde(default)]
    pub material: String,
    pub thickness: String,
    pub nominal_radius: String,
    pub clearance: String,
    pub knife_edge: String,
    pub reveal_height: String,
    pub thickness_variation: String,
    pub draft: String,
    pub max_thickness: String,
    pub min_thickness: String,
    pub min_draft: String,
}

impl PlasticRule {
    fn new(name: &str, material: &str, inch: bool, thickness: &str, max: &str, min: &str) -> PlasticRule {
        let (clearance, knife, reveal) = if inch { ("0.004 in", "0.04 in", "0.02 in") } else { ("0.1 mm", "1 mm", "0.5 mm") };
        PlasticRule {
            name: name.into(),
            material: material.into(),
            thickness: thickness.into(),
            nominal_radius: "0.5 * Thickness".into(),
            clearance: clearance.into(),
            knife_edge: knife.into(),
            reveal_height: reveal.into(),
            thickness_variation: "0.15 * Thickness".into(),
            draft: "2 deg".into(),
            max_thickness: max.into(),
            min_thickness: min.into(),
            min_draft: "0.5 deg".into(),
        }
    }

    /// The rule library (the first is the default).
    pub fn library() -> Vec<PlasticRule> {
        vec![
            PlasticRule::new("ABS (1.5mm)", "ABS Plastic", false, "1.5 mm", "3.5 mm", "1.2 mm"),
            PlasticRule::new("Nylon (PA6) (2.5mm)", "Nylon 6", false, "2.5 mm", "3 mm", "0.8 mm"),
            PlasticRule::new("Polypropylene (PP) (3 mm)", "Polypropylene", false, "3 mm", "3.8 mm", "0.8 mm"),
            PlasticRule::new("ABS (0.1 in)", "ABS Plastic", true, "0.1 in", "3.5 mm", "1.2 mm"),
            PlasticRule::new("Nylon (PA6) (0.1 in)", "Nylon 6", true, "0.1 in", "3 mm", "0.8 mm"),
            PlasticRule::new("Polypropylene (PP) (0.1 in)", "Polypropylene", true, "0.1 in", "3.8 mm", "0.8 mm"),
        ]
    }

    /// The expressions, by field name (as commands and the dialog name them).
    pub fn fields(&self) -> [(&'static str, &String, Kind); 10] {
        [
            ("thickness", &self.thickness, Kind::Length),
            ("nominal_radius", &self.nominal_radius, Kind::Length),
            ("clearance", &self.clearance, Kind::Length),
            ("knife_edge", &self.knife_edge, Kind::Length),
            ("reveal_height", &self.reveal_height, Kind::Length),
            ("thickness_variation", &self.thickness_variation, Kind::Length),
            ("draft", &self.draft, Kind::Angle),
            ("max_thickness", &self.max_thickness, Kind::Length),
            ("min_thickness", &self.min_thickness, Kind::Length),
            ("min_draft", &self.min_draft, Kind::Angle),
        ]
    }

    pub fn field_mut(&mut self, name: &str) -> Option<&mut String> {
        Some(match name {
            "thickness" => &mut self.thickness,
            "nominal_radius" => &mut self.nominal_radius,
            "clearance" => &mut self.clearance,
            "knife_edge" => &mut self.knife_edge,
            "reveal_height" => &mut self.reveal_height,
            "thickness_variation" => &mut self.thickness_variation,
            "draft" => &mut self.draft,
            "max_thickness" => &mut self.max_thickness,
            "min_thickness" => &mut self.min_thickness,
            "min_draft" => &mut self.min_draft,
            _ => return None,
        })
    }

    /// The expressions to change, by field name, in the order of [`PlasticRule::fields`].
    pub fn fields_mut(&mut self) -> [(&'static str, &mut String); 10] {
        [
            ("thickness", &mut self.thickness),
            ("nominal_radius", &mut self.nominal_radius),
            ("clearance", &mut self.clearance),
            ("knife_edge", &mut self.knife_edge),
            ("reveal_height", &mut self.reveal_height),
            ("thickness_variation", &mut self.thickness_variation),
            ("draft", &mut self.draft),
            ("max_thickness", &mut self.max_thickness),
            ("min_thickness", &mut self.min_thickness),
            ("min_draft", &mut self.min_draft),
        ]
    }
}

/// The design's own plastic rules, the active one and the rules assigned to bodies.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlasticSettings {
    /// Rules made or edited in this design (a library rule edited here is copied in).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<PlasticRule>,
    /// Applies to bodies with no rule of their own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    /// Body name → rule name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub assigned: BTreeMap<String, String>,
}

impl PlasticSettings {
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty() && self.active.is_none() && self.assigned.is_empty()
    }
}

/// A rule's values (mm, radians).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PlasticValues {
    pub thickness: f64,
    pub nominal_radius: f64,
    pub clearance: f64,
    pub knife_edge: f64,
    pub reveal_height: f64,
    pub thickness_variation: f64,
    pub draft: f64,
    pub max_thickness: f64,
    pub min_thickness: f64,
    pub min_draft: f64,
}

impl Document {
    /// A rule by name: the design's own first, then the library.
    pub fn plastic_rule(&self, name: &str) -> Option<PlasticRule> {
        self.plastic.rules.iter().find(|r| r.name == name).cloned().or_else(|| PlasticRule::library().into_iter().find(|r| r.name == name))
    }

    /// Every rule: the library (as edited in this design) and the design's own.
    pub fn plastic_rules(&self) -> Vec<PlasticRule> {
        let mut out: Vec<PlasticRule> = PlasticRule::library().into_iter().map(|l| self.plastic_rule(&l.name).unwrap_or(l)).collect();
        let own: Vec<PlasticRule> = self.plastic.rules.iter().filter(|r| !out.iter().any(|o| o.name == r.name)).cloned().collect();
        out.extend(own);
        out
    }

    /// The rule that applies to a body: its own, else the active one.
    pub fn plastic_rule_for(&self, body: &str) -> Option<PlasticRule> {
        self.plastic.assigned.get(body).or(self.plastic.active.as_ref()).and_then(|n| self.plastic_rule(n))
    }

    pub fn plastic_values(&self, vals: &BTreeMap<String, Value>, r: &PlasticRule) -> Result<PlasticValues> {
        let look = |n: &str| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`")));
        let t = expr::eval_with(&r.thickness, &look)?.to_kind(Kind::Length)?;
        if !(t > 0.0 && t < 1e4) {
            return Err(DocError::Invalid("the plastic thickness must be positive".into()));
        }
        let with_t = |e: &str, k: Kind| -> Result<f64> {
            let f = |n: &str| if n == "Thickness" { Ok(Value::length(t)) } else { look(n) };
            let v = expr::eval_with(e, &f)?.to_kind(k)?;
            if !v.is_finite() || v < 0.0 {
                return Err(DocError::Invalid(format!("plastic rule `{}`: `{e}` must not be negative", r.name)));
            }
            Ok(v)
        };
        Ok(PlasticValues {
            thickness: t,
            nominal_radius: with_t(&r.nominal_radius, Kind::Length)?,
            clearance: with_t(&r.clearance, Kind::Length)?,
            knife_edge: with_t(&r.knife_edge, Kind::Length)?,
            reveal_height: with_t(&r.reveal_height, Kind::Length)?,
            thickness_variation: with_t(&r.thickness_variation, Kind::Length)?,
            draft: with_t(&r.draft, Kind::Angle)?,
            max_thickness: with_t(&r.max_thickness, Kind::Length)?,
            min_thickness: with_t(&r.min_thickness, Kind::Length)?,
            min_draft: with_t(&r.min_draft, Kind::Angle)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_values() {
        let d = Document::default();
        let vals = BTreeMap::new();
        let abs = d.plastic_values(&vals, &PlasticRule::library()[0]).unwrap();
        assert_eq!((abs.thickness, abs.nominal_radius, abs.clearance), (1.5, 0.75, 0.1));
        assert!((abs.thickness_variation - 0.225).abs() < 1e-12);
        assert!((abs.draft - 2f64.to_radians()).abs() < 1e-12);
        let pp_in = d.plastic_values(&vals, &d.plastic_rule("Polypropylene (PP) (0.1 in)").unwrap()).unwrap();
        assert!((pp_in.thickness - 2.54).abs() < 1e-9 && (pp_in.nominal_radius - 1.27).abs() < 1e-9 && (pp_in.clearance - 0.1016).abs() < 1e-9);
        assert_eq!(d.plastic_rules().len(), 6);
    }
}

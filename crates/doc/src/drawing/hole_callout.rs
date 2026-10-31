//! The content of a hole callout (W29a, #7605): count, diameter or thread, depth or through,
//! counterbore and countersink, worked out from the model's Hole feature, and its text.
//!
//! - A callout names one hole by persistent ids only ([`HoleRef`]): the Hole feature's id and,
//!   for a hole drilled at sketch points, the point's id. Never a face index or a nearest point.
//! - It resolves to one of four outcomes ([`Resolution`]): resolved, suppressed (the feature is
//!   suppressed), failed (the feature did not evaluate, or a later feature removed its hole) or
//!   lost (the feature or the point is gone). A lost callout keeps its ids and is never moved to
//!   another hole. An id below the design's id counter that names no feature and nothing else
//!   reads as lost; an id the design never gave, or one of a component, folder, canvas or other
//!   item that is not a feature, is an error. A point the feature doesn't drill is lost, since a
//!   callout can't tell a deleted point from one that was never there.
//! - The values come from the feature record, evaluated with the design's parameters as the
//!   model evaluates it, so the callout follows edits. Through or blind comes from the feature
//!   and the hole's own named faces (`F<id>:bore:…`): a hole with a depth is blind while its
//!   bore keeps a floor on its axis, and through when the depth reaches past the material. A
//!   hole whose bore is in the model the feature made but not in the model as it is now was
//!   removed by a later feature, and reads as failed with that reason. The depth is the
//!   feature's, measured from the face the hole starts on.
//! - The text: numbers through the shared length formatter ([`crate::display_units`]), the
//!   words in the drawing's content language. Nothing is read from a view, and nothing is saved.
//!
//! Holes made by other features, and the copies a pattern or a mirror makes of a hole, are not
//! supported yet.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec3;

use crate::expr::{Kind, Value};
use crate::{DocError, Document, Feature, FeatureKind, HoleKind, Model, ModelState, Result, display_units};

/// The most bytes of a sketch point id a callout takes.
pub const MAX_POINT_BYTES: usize = 256;
/// The most decimals a callout's numbers take.
pub const MAX_DECIMALS: u8 = 6;
/// The decimals a callout's numbers get unless asked for others: the length-decimals preference's
/// default, as dimensions take theirs, so a callout and a dimension on one sheet read alike.
pub const DEFAULT_DECIMALS: u8 = 2;
/// The most holes one Hole feature drills (as the feature itself takes).
const MAX_HOLES: usize = 10_000;
/// The largest length or diameter a callout shows, in millimetres.
const MAX_LENGTH_MM: f64 = 1e9;

/// One hole, by persistent ids.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoleRef {
    /// The Hole feature's id.
    pub feature: u64,
    /// Its sketch point id, for a hole drilled at sketch points; `None` for a hole placed by position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<String>,
}

/// What a callout's reference comes to.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Resolution {
    Resolved {
        callout: HoleCallout,
    },
    /// The feature is suppressed.
    Suppressed,
    /// The feature did not evaluate.
    Failed {
        reason: String,
    },
    /// The feature or the point is gone.
    Lost {
        reason: String,
    },
}

/// A resolved hole callout: what the feature drills there, in millimetres and radians.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HoleCallout {
    /// How many holes the feature drills.
    pub count: usize,
    /// The drilled diameter (for a tapped hole, the tap drill).
    pub diameter: f64,
    /// The thread of a tapped hole (`M8`), shown in place of the diameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    pub extent: Extent,
    pub form: Form,
    /// Where the hole starts: on the face it is drilled into.
    pub start: [f64; 3],
    /// The name of that face, in the model before the hole (empty when it has none).
    pub start_face: String,
    /// The drilling direction, into the material.
    pub direction: [f64; 3],
}

/// How far a hole goes.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Extent {
    /// Through the material.
    Through,
    /// To this depth below its start face (to the shoulder of a drill point).
    Blind { depth: f64 },
}

/// The hole's form at its top.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Form {
    Simple,
    /// A drill point at the bottom of a blind hole.
    Drilled {
        tip_angle: f64,
    },
    Counterbore {
        diameter: f64,
        depth: f64,
    },
    Countersink {
        diameter: f64,
        angle: f64,
    },
}

/// How a callout's text is written: the drawing's length unit and content language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextStyle {
    /// A design unit (`mm`, `cm`, `m`, `in`, `ft`).
    pub units: String,
    pub decimals: u8,
    /// The drawing's content language, a BCP 47 tag.
    pub language: String,
}

impl TextStyle {
    /// Refuses an unknown unit, too many decimals or a malformed language tag.
    pub fn validate(&self) -> Result<()> {
        if !["mm", "cm", "m", "in", "ft"].contains(&self.units.as_str()) {
            let unit: String = self.units.chars().take(32).collect();
            return Err(DocError::Invalid(format!("unknown length unit `{}` (mm, cm, m, in or ft)", unit.escape_debug())));
        }
        if self.decimals > MAX_DECIMALS {
            return Err(DocError::Invalid(format!("decimals are 0 to {MAX_DECIMALS}")));
        }
        if !super::is_language_tag(&self.language) {
            return Err(DocError::Invalid("the language is not a BCP 47 tag (such as `en` or `de-DE`)".into()));
        }
        Ok(())
    }
}

impl HoleRef {
    /// Refuses a point id that is empty, too long or holds control characters.
    pub fn validate(&self) -> Result<()> {
        if let Some(p) = &self.point
            && (p.is_empty() || p.len() > MAX_POINT_BYTES || p.chars().any(char::is_control))
        {
            return Err(DocError::Invalid(format!("a sketch point id is 1 to {MAX_POINT_BYTES} bytes, without control characters")));
        }
        Ok(())
    }
}

/// The callout of the hole `hole` names, in the design `doc` evaluated as `model`.
pub fn resolve(doc: &Document, model: &Model, hole: &HoleRef) -> Result<Resolution> {
    hole.validate()?;
    let Some(f) = doc.feature(hole.feature) else {
        if let Some(kind) = other_item(doc, hole.feature) {
            return Err(DocError::Invalid(format!("{} is a {kind}, not a Hole feature", hole.feature)));
        }
        // An id the design gave once belonged to something that has since been removed.
        if hole.feature >= 1 && hole.feature < doc.next_id {
            return Ok(Resolution::Lost { reason: format!("feature {} is gone", hole.feature) });
        }
        return Err(DocError::Unknown(format!("feature {}", hole.feature)));
    };
    let FeatureKind::Hole { points, .. } = &f.kind else {
        return Err(DocError::Invalid(format!("not supported yet: hole callouts of {} features (only Hole features have them)", f.kind.base_name())));
    };
    // Which hole of the feature: its point's place among the feature's points.
    let index = match (points, &hole.point) {
        (Some(sp), _) if sp.ids.len() > MAX_HOLES => return Err(DocError::Invalid(format!("a hole needs 1…{MAX_HOLES} points"))),
        (Some(sp), None) if !sp.single => {
            return Err(DocError::Invalid(format!("feature {} drills at {} sketch points: name one with `point`", f.id, sp.ids.len())));
        }
        (Some(sp), point) => {
            let id = match point {
                Some(p) => p,
                None => sp.ids.first().ok_or_else(|| DocError::Invalid("a Single hole needs its sketch point".into()))?,
            };
            let Some(i) = sp.ids.iter().position(|x| x == id) else {
                return Ok(Resolution::Lost { reason: format!("feature {} drills no hole at point `{}`", f.id, id.escape_debug()) });
            };
            if !sketch_has_point(doc, sp.sketch.0, id) {
                return Ok(Resolution::Lost { reason: format!("point `{}` is gone from sketch {}", id.escape_debug(), sp.sketch.0) });
            }
            // A Single hole drills at its first point only.
            if sp.single && i != 0 {
                let first = sp.ids.first().map(|p| p.escape_debug().to_string()).unwrap_or_default();
                return Ok(Resolution::Lost { reason: format!("feature {} drills only at point `{first}`", f.id) });
            }
            i
        }
        (None, Some(_)) => return Err(DocError::Invalid(format!("feature {} is placed by position: it takes no `point`", f.id))),
        (None, None) => 0,
    };
    if f.suppressed {
        return Ok(Resolution::Suppressed);
    }
    let own = match model.result(f.id) {
        None => return Ok(Resolution::Failed { reason: "not evaluated".into() }),
        Some(r) if r.skipped => return Ok(Resolution::Failed { reason: "not evaluated: it is after the end of the timeline".into() }),
        Some(r) if r.error.is_some() => return Ok(Resolution::Failed { reason: r.error.clone().unwrap_or_default() }),
        Some(r) => r.state.clone(),
    };
    let (vals, _) = crate::eval::evaluation_values(doc);
    let before = model.state_before(f.id);
    measure(&vals, &before, &own, &model.state(), f, index)
}

/// What the id `id`, which names no feature, names instead, if anything.
fn other_item(doc: &Document, id: u64) -> Option<&'static str> {
    let a = &doc.assembly;
    [
        ("component", doc.components.iter().any(|c| c.id == id)),
        ("browser folder", doc.browser_groups.iter().any(|g| g.id == id)),
        ("timeline folder", doc.timeline_groups.iter().any(|g| g.id == id)),
        ("canvas", doc.canvases.iter().any(|c| c.id == id)),
        ("joint", a.joints.iter().any(|j| j.id == id)),
        ("contact set", a.contact_sets.iter().any(|c| c.id == id)),
        ("motion study", a.studies.iter().any(|s| s.id == id)),
        ("exploded view", a.exploded.iter().any(|v| v.id == id)),
    ]
    .into_iter()
    .find_map(|(kind, is)| is.then_some(kind))
}

/// Whether the sketch feature `sketch` still has the point `id`.
fn sketch_has_point(doc: &Document, sketch: u64, id: &str) -> bool {
    matches!(doc.feature(sketch).map(|f| &f.kind), Some(FeatureKind::Sketch { sketch, .. }) if sketch.points.iter().any(|p| p.id == id))
}

/// Hole `index` of the Hole feature `f`, drilled into `before`, made in `own` (the model just
/// after the feature) and found in `after` (the model as it is now). Failed when `after` has no
/// face of that hole.
fn measure(
    vals: &BTreeMap<String, Value>,
    before: &ModelState,
    own: &ModelState,
    after: &ModelState,
    f: &Feature,
    index: usize,
) -> Result<Resolution> {
    let FeatureKind::Hole { diameter, depth, hole, thread, to, .. } = &f.kind else { return Err(DocError::Invalid("not a hole".into())) };
    let (spots, dir) = crate::eval::hole_spots(vals, before, f)?;
    if spots.is_empty() || spots.len() > MAX_HOLES {
        return Err(DocError::Invalid(format!("a hole needs 1…{MAX_HOLES} points")));
    }
    let at = *spots.get(index).ok_or_else(|| DocError::Invalid("the hole's point is not drilled".into()))?;
    if !(at.is_finite() && dir.is_finite()) {
        return Err(DocError::Invalid("the hole's place is out of range".into()));
    }
    let length = |e: &str, what: &str| -> Result<f64> {
        let v = crate::eval::val(vals, e, Kind::Length)?;
        if !(v.is_finite() && v > 0.0 && v < MAX_LENGTH_MM) {
            return Err(DocError::Invalid(format!("the hole's {what} is not a positive length")));
        }
        Ok(v)
    };
    let angle = |e: &str, what: &str| -> Result<f64> {
        let v = crate::eval::val(vals, e, Kind::Angle)?;
        if !(v.is_finite() && v > 0.0 && v < std::f64::consts::PI) {
            return Err(DocError::Invalid(format!("the hole's {what} is not an angle between 0 and 180°")));
        }
        Ok(v)
    };
    let diameter = length(diameter, "diameter")?;
    let form = match hole {
        HoleKind::Simple => Form::Simple,
        HoleKind::Drilled { tip_angle } => Form::Drilled { tip_angle: angle(tip_angle, "tip angle")? },
        HoleKind::Counterbore { cb_diameter, cb_depth, tip_angle: _ } => {
            Form::Counterbore { diameter: length(cb_diameter, "counterbore diameter")?, depth: length(cb_depth, "counterbore depth")? }
        }
        HoleKind::Countersink { cs_diameter, cs_angle, tip_angle: _ } => {
            Form::Countersink { diameter: length(cs_diameter, "countersink diameter")?, angle: angle(cs_angle, "countersink angle")? }
        }
    };
    let thread = match thread {
        Some(t) if crate::parse_metric_thread(t).is_some() && t.trim().len() <= 32 => Some(t.trim().to_string()),
        Some(_) => return Err(DocError::Invalid("the hole's thread is not an ISO metric thread".into())),
        None => None,
    };
    // The depth from the start face: as given, or to the To Object.
    let depth = match crate::eval::hole_depth_to(before, &at, dir, &to.map(Vec3::from), depth)? {
        Some(d) => Some(length(&d, "depth")?),
        None => None,
    };
    let Some(floor) = bore_floor(after, f.id, at, dir, diameter / 2.0) else {
        let reason = match bore_floor(own, f.id, at, dir, diameter / 2.0) {
            Some(_) => "a later feature removed the hole",
            None => "the hole cuts no material",
        };
        return Ok(Resolution::Failed { reason: reason.into() });
    };
    let extent = match depth {
        Some(depth) if floor => Extent::Blind { depth },
        _ => Extent::Through,
    };
    let start_face = crate::naming::face_names_at(before, &[at]).into_iter().next().unwrap_or_default();
    let callout = HoleCallout { count: spots.len(), diameter, thread, extent, form, start: at.into(), start_face, direction: dir.into() };
    Ok(Resolution::Resolved { callout })
}

/// Whether the bore of feature `id` at `at` (drilled along `dir`, of radius `r`) keeps a floor in
/// `st`: a face of the bore (`F<id>:bore:…`) across its axis below the start, the flat bottom or
/// the drill point. `None` when no face of that bore is in the model at all.
fn bore_floor(st: &ModelState, id: u64, at: Vec3, dir: Vec3, r: f64) -> Option<bool> {
    use solidcraft_kernel::FaceSurface;
    let prefix = format!("F{id}:bore:");
    let (mut wall, mut floor) = (false, false);
    for b in st.bodies.iter().filter(|b| !b.body.is_mesh()) {
        let names = crate::naming::face_names(b);
        if !names.iter().any(|n| n.starts_with(&prefix)) {
            continue;
        }
        // One pass over the body's triangles for all its faces, not one per bore face.
        let spots = crate::naming::face_spots(b);
        for (name, spot) in names.iter().zip(spots.iter()) {
            let Some(spot) = spot.filter(|_| name.starts_with(&prefix)) else { continue };
            let v = spot.point - at;
            let along = v.dot(dir);
            let off = (v - dir * along).len();
            // On this hole's axis and below its start.
            if along <= 0.0 || off > r * 1.001 + 1e-6 {
                continue;
            }
            // The wall is a cylinder about the axis; a flat bottom or a drill point lies across
            // it. Only a face of no known surface is told by where its spot is: the wall at the
            // radius (a facet of a curved face lies inside it by its sag, so this is a fallback).
            let across = match spot.surface {
                Some(FaceSurface::Cylinder { axis, .. }) if axis.dot(dir).abs() > 1.0 - 1e-6 => false,
                Some(FaceSurface::Plane { normal, .. }) if normal.dot(dir).abs() > 1.0 - 1e-6 => true,
                Some(FaceSurface::Cone { axis, .. }) if axis.dot(dir).abs() > 1.0 - 1e-6 => true,
                _ => off < r * 0.999 - 1e-6,
            };
            if across {
                floor = true;
            } else {
                wall = true;
            }
        }
    }
    (wall || floor).then_some(floor)
}

/// The callout's text, one line each: the hole, then its counterbore or countersink.
pub fn text(c: &HoleCallout, style: &TextStyle) -> Result<Vec<String>> {
    style.validate()?;
    let len = |mm: f64| display_units::length_number(mm, &style.units, style.decimals);
    let count = if c.count > 1 { format!("{}× ", c.count) } else { String::new() };
    let size = match &c.thread {
        Some(t) => t.clone(),
        None => format!("Ø{}", len(c.diameter)),
    };
    let extent = match c.extent {
        Extent::Through => format!(" {}", words(&style.language).through),
        Extent::Blind { depth } => format!(" ↧ {}", len(depth)),
    };
    let mut lines = vec![format!("{count}{size}{extent}")];
    match c.form {
        Form::Simple | Form::Drilled { .. } => {}
        Form::Counterbore { diameter, depth } => lines.push(format!("⌴ Ø{} ↧ {}", len(diameter), len(depth))),
        Form::Countersink { diameter, angle } => {
            lines.push(format!("⌵ Ø{} × {:.*}°", len(diameter), usize::from(style.decimals.min(MAX_DECIMALS)), angle.to_degrees()))
        }
    }
    Ok(lines)
}

/// The words of a callout in one content language.
struct Words {
    through: &'static str,
}

/// The words in `language` (its primary subtag); English for a language without its own yet.
fn words(language: &str) -> Words {
    let primary = language.split('-').next().unwrap_or_default().to_ascii_lowercase();
    match primary.as_str() {
        "de" => Words { through: "DURCH" },
        _ => Words { through: "THRU" },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(units: &str, decimals: u8, language: &str) -> TextStyle {
        TextStyle { units: units.into(), decimals, language: language.into() }
    }

    fn callout(count: usize, extent: Extent, form: Form) -> HoleCallout {
        HoleCallout { count, diameter: 6.6, thread: None, extent, form, start: [0.0; 3], start_face: String::new(), direction: [0.0, 0.0, -1.0] }
    }

    #[test]
    fn text_has_the_count_the_size_the_extent_and_the_top_form() {
        let en = style("mm", 1, "en");
        let cb = callout(2, Extent::Through, Form::Counterbore { diameter: 11.0, depth: 6.8 });
        assert_eq!(text(&cb, &en).unwrap(), ["2× Ø6.6 THRU", "⌴ Ø11.0 ↧ 6.8"]);
        let cs = callout(1, Extent::Blind { depth: 12.0 }, Form::Countersink { diameter: 12.0, angle: 90f64.to_radians() });
        assert_eq!(text(&cs, &en).unwrap(), ["Ø6.6 ↧ 12.0", "⌵ Ø12.0 × 90.0°"]);
        let mut tapped = callout(1, Extent::Blind { depth: 12.0 }, Form::Simple);
        tapped.thread = Some("M8".into());
        assert_eq!(text(&tapped, &style("mm", 0, "en")).unwrap(), ["M8 ↧ 12"]);
        assert_eq!(text(&callout(1, Extent::Through, Form::Simple), &style("in", 2, "en")).unwrap(), ["Ø0.2598 THRU"]);
    }

    #[test]
    fn words_follow_the_content_language() {
        let through = callout(6, Extent::Through, Form::Simple);
        assert_eq!(text(&through, &style("mm", 1, "de-DE")).unwrap(), ["6× Ø6.6 DURCH"]);
        assert_eq!(text(&through, &style("mm", 1, "en-GB")).unwrap(), ["6× Ø6.6 THRU"]);
        // A language without its own words yet is written in English.
        assert_eq!(text(&through, &style("mm", 1, "ja")).unwrap(), ["6× Ø6.6 THRU"]);
    }

    #[test]
    fn a_bad_style_or_point_is_refused() {
        let c = callout(1, Extent::Through, Form::Simple);
        for bad in [style("furlong", 1, "en"), style("mm", 7, "en"), style("mm", 1, "e"), style("mm", 1, "en_GB"), style("mm", 1, "")] {
            assert!(text(&c, &bad).is_err(), "{bad:?}");
        }
        for point in ["", "a\nb", &"p".repeat(MAX_POINT_BYTES + 1)] {
            assert!(HoleRef { feature: 1, point: Some(point.into()) }.validate().is_err(), "{point:?}");
        }
        assert!(HoleRef { feature: 1, point: Some("p3".into()) }.validate().is_ok());
    }
}

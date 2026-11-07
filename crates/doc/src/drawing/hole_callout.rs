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
//! - The numbers (diameter, depth, thread, form) come from the feature record, evaluated with
//!   the design's parameters as the model evaluates it, so the callout follows edits. Only
//!   through or blind is read from the model as it is now, from the hole's own named faces
//!   (`F<id>:bore:…`): a hole with a depth is blind while its bore keeps a floor on its axis, and
//!   through when the depth reaches past the material. A hole whose bore is in the model the
//!   feature made but not in the model as it is now was removed by a later feature, and reads as
//!   failed with that reason. A bore whose wall is no longer of the feature's diameter, or whose
//!   flat floor is no longer at its depth, was changed by a later feature that kept its faces'
//!   names (an offset face, a scale), and reads as failed too; a drill point's cone and the
//!   counterbore or countersink are not checked. The depth is the feature's, measured from the
//!   face the hole starts on. A later `solid.move` of the hole's body is followed, so the hole is
//!   looked for where the move put it, and `start` and `direction` are in the frame of the model
//!   as it is now (of the hole's own component, the same for every placement of it). A body
//!   moved any other way reads as failed.
//! - A tapped hole takes any thread the Hole feature takes (ISO metric, UNC/UNF, G, Rp). With
//!   its own thread depth, the drill gets a line of its own and the thread line that depth, both
//!   from the face the hole starts on: the feature counts its thread from below any counterbore
//!   or countersink, so the callout adds that recess. A thread that reaches the floor of a blind
//!   hole is one line.
//! - A hole in sheet metal reads as failed, not supported yet: the sheet is rebuilt from its own
//!   record with the hole in it, so no face is named after the hole.
//! - `count` is the feature's; the extent is this hole's. One feature can drill through on one
//!   hole and blind on another (a stepped part), so `6×` does not say the six share it.
//! - The text: numbers through the shared length formatter ([`crate::display_units`]), the
//!   words in the drawing's content language. Nothing is read from a view, and nothing is saved.
//!
//! Holes made by other features, and the copies a pattern or a mirror makes of a hole, are not
//! supported yet.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
use solidcraft_kernel::PointClass;

use crate::expr::{Kind, Value};
use crate::{DocError, Document, Feature, FeatureKind, HoleKind, Mat, Model, ModelState, Result, display_units};

/// The decimals a callout's numbers get unless asked for others: the one default dimensions take
/// too, so a callout and a dimension on one sheet read alike.
pub use super::dimension::DEFAULT_DECIMALS;
/// The most decimals a callout's numbers take: as many as a dimension's.
pub use super::dimension::MAX_DECIMALS;

/// The most bytes of a sketch point id a callout takes.
pub const MAX_POINT_BYTES: usize = 256;
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
    /// The thread of a tapped hole (`M8`, `1/4-20 UNC`, `G 1/4`), shown in place of the diameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    /// The thread's depth when the feature gives it one, from the face the hole starts on as the
    /// drill depth is: the feature's own thread depth (from where the thread starts) plus any
    /// counterbore depth or countersink depth above it. Without one, the thread runs the hole's
    /// length.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_depth: Option<f64>,
    pub extent: Extent,
    pub form: Form,
    /// Where the hole starts: on the face it is drilled into, in the frame of the model as it is
    /// now (after a later `solid.move` of its body) and of the hole's own component.
    pub start: [f64; 3],
    /// The name of that face, in the model before the hole (empty when it has none).
    pub start_face: String,
    /// The drilling direction, into the material, in the frame of `start`.
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
    /// A length unit of the expression unit table (`mm`, `in`, `yd`, …), as a drawing fixes it.
    pub units: String,
    pub decimals: u8,
    /// The drawing's content language, a BCP 47 tag.
    pub language: String,
}

impl TextStyle {
    /// Refuses an unknown unit, too many decimals or a malformed language tag.
    pub fn validate(&self) -> Result<()> {
        if !super::is_length_unit(&self.units) {
            let unit: String = self.units.chars().take(32).collect();
            return Err(DocError::Invalid(format!("unknown length unit `{}` (such as mm or in)", unit.escape_debug())));
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
    let motion = |body: &str| later_motion(doc, model, &vals, f.id, body);
    measure(&vals, &before, &own, &model.state(), f, index, &motion)
}

/// The rigid motion the Move features after feature `id` give the body `body`, in order: what
/// carries a place in the model just after `id` to the model as it is now. A Move that copies
/// the body leaves it where it is.
fn later_motion(doc: &Document, model: &Model, vals: &BTreeMap<String, Value>, id: u64, body: &str) -> Result<Mat> {
    let mut m = crate::IDENTITY;
    let Some(i) = model.results.iter().position(|r| r.id == id) else { return Ok(m) };
    for r in model.results.iter().skip(i + 1).filter(|r| !r.skipped && r.error.is_none()) {
        let Some(FeatureKind::Move { bodies, translate, rotate_axis, angle, copy }) = doc.feature(r.id).filter(|f| !f.suppressed).map(|f| &f.kind)
        else {
            continue;
        };
        if *copy || !bodies.iter().any(|b| *b == body) {
            continue;
        }
        let [x, y, z] = translate;
        let t =
            Vec3::new(crate::eval::val(vals, x, Kind::Length)?, crate::eval::val(vals, y, Kind::Length)?, crate::eval::val(vals, z, Kind::Length)?);
        let angle = match angle {
            Some(a) => crate::eval::val(vals, a, Kind::Angle)?,
            None => 0.0,
        };
        let axis = rotate_axis.map(Vec3::from).unwrap_or(Vec3::Z);
        m = crate::mat_mul(&crate::rigid(t, Vec3::ZERO, axis, angle), &m);
    }
    Ok(m)
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
/// after the feature) and found in `after` (the model as it is now), where `motion` (of the body
/// it is in) carried it. Failed when `after` has no face of that hole there.
fn measure(
    vals: &BTreeMap<String, Value>,
    before: &ModelState,
    own: &ModelState,
    after: &ModelState,
    f: &Feature,
    index: usize,
    motion: &dyn Fn(&str) -> Result<Mat>,
) -> Result<Resolution> {
    let FeatureKind::Hole { diameter, depth, hole, thread, thread_depth, to, .. } = &f.kind else {
        return Err(DocError::Invalid("not a hole".into()));
    };
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
    // Any thread the feature takes; a metric one as written, a catalogued one by its designation.
    let thread = match thread {
        Some(t) => match crate::parse_tapped_thread(t).filter(|_| t.trim().len() <= 32) {
            Some(tapped) if tapped.family == crate::TappedThreadFamily::Metric => Some(metric_as_written(t)),
            Some(tapped) => Some(tapped.designation),
            None => return Err(DocError::Invalid("the hole's thread is not one a tapped hole takes (ISO metric, UNC, UNF, G or Rp)".into())),
        },
        None => None,
    };
    let thread_depth = match thread_depth {
        Some(d) if thread.is_some() => Some(length(d, "thread depth")?),
        _ => None,
    };
    // The depth from the start face: as given, or to the To Object.
    let depth = match crate::eval::hole_depth_to(before, &at, dir, &to.map(Vec3::from), depth)? {
        Some(d) => Some(length(&d, "depth")?),
        None => None,
    };
    let r = diameter / 2.0;
    // The thread's depth from the start face, as the drill's: the feature's runs from where the
    // thread starts, below any counterbore or countersink, so that recess is added.
    let recess = match form {
        Form::Counterbore { depth, .. } => depth,
        Form::Countersink { diameter: sink, angle } => ((sink - diameter) / 2.0).max(0.0) / (angle / 2.0).tan(),
        Form::Simple | Form::Drilled { .. } => 0.0,
    };
    let thread_depth = thread_depth.map(|t| t + recess).filter(|t| t.is_finite());
    // The face it starts on, named in the model before it (where the feature drilled it).
    let start_face = crate::naming::face_names_at(before, &[at]).into_iter().next().unwrap_or_default();
    let Some(made) = bore_floor(own, f.id, at, dir, r, depth) else {
        // A sheet is rebuilt from its own record with the hole in it, so the hole's faces aren't
        // named after it.
        let reason = if in_sheet(before, at, dir, r) {
            "not supported yet: hole callouts in sheet metal (the sheet keeps no faces named after the hole)"
        } else {
            "the hole cuts no material"
        };
        return Ok(Resolution::Failed { reason: reason.into() });
    };
    // Where later moves of its body put it, and the feature's other holes with it.
    let m = motion(&made.body)?;
    let (start, direction) = (crate::apply_point(&m, at), crate::apply_vector(&m, dir));
    let Some(now) = bore_floor(after, f.id, start, direction, r, depth) else {
        let holes: Vec<Vec3> = spots.iter().map(|p| crate::apply_point(&m, *p)).collect();
        let reason = if stray_bore(after, f.id, &holes, direction, r) {
            "a later feature removed or moved the hole (only a Move of its body is followed)"
        } else {
            "a later feature removed the hole"
        };
        return Ok(Resolution::Failed { reason: reason.into() });
    };
    if now.changed {
        return Ok(Resolution::Failed { reason: "a later feature changed the hole: its diameter or depth no longer is the feature's".into() });
    }
    let extent = match depth {
        Some(depth) if now.floor => Extent::Blind { depth },
        _ => Extent::Through,
    };
    let callout = HoleCallout {
        count: spots.len(),
        diameter,
        thread,
        thread_depth,
        extent,
        form,
        start: start.into(),
        start_face,
        direction: direction.into(),
    };
    Ok(Resolution::Resolved { callout })
}

/// A metric thread as written, with the `M` in capitals (`m8x1` → `M8x1`).
fn metric_as_written(t: &str) -> String {
    let t = t.trim();
    match t.strip_prefix('m') {
        Some(rest) => format!("M{rest}"),
        None => t.to_string(),
    }
}

/// The bore of a hole as a model has it.
struct Bore {
    /// It keeps a floor (blind).
    floor: bool,
    /// The first body it is in.
    body: String,
    /// Its wall is not of the feature's diameter, or its flat floor not at the feature's depth: a
    /// later feature changed it and kept its faces' names (an offset face, a scale).
    changed: bool,
}

/// Whether the hole drilled at `at` along `dir` (of radius `r`) starts in a sheet-metal body of
/// `st`.
fn in_sheet(st: &ModelState, at: Vec3, dir: Vec3, r: f64) -> bool {
    let inside = at + dir * (r.min(1.0) * 1e-3);
    st.sheets.iter().filter_map(|sh| st.body(&sh.body)).any(|b| !b.body.is_mesh() && matches!(b.body.classify_point(inside), Ok(PointClass::Inside)))
}

/// Whether a body of `st` has a face of a bore of feature `id` that lies on none of the axes of
/// its holes at `holes` (drilled along `dir`, of radius `r`): one of them is somewhere else now.
/// The feature's other holes, still in place, don't count.
fn stray_bore(st: &ModelState, id: u64, holes: &[Vec3], dir: Vec3, r: f64) -> bool {
    let prefix = format!("F{id}:bore:");
    let on_an_axis = |p: Vec3| {
        holes.iter().any(|at| {
            let v = p - *at;
            (v - dir * v.dot(dir)).len() <= r * 1.001 + 1e-6
        })
    };
    st.bodies.iter().filter(|b| !b.body.is_mesh()).any(|b| {
        let names = crate::naming::face_names(b);
        if !names.iter().any(|n| n.starts_with(&prefix)) {
            return false;
        }
        let spots = crate::naming::face_spots(b);
        names.iter().zip(spots.iter()).any(|(name, spot)| name.starts_with(&prefix) && spot.is_some_and(|s| !on_an_axis(s.point)))
    })
}

/// The bore of feature `id` at `at` (drilled along `dir`, of radius `r`, `depth` deep when blind)
/// in `st`, and whether it keeps a floor: a face of the bore (`F<id>:bore:…`) across its axis
/// below the start, the flat bottom or the drill point. `None` when no face of that bore is there
/// at all.
fn bore_floor(st: &ModelState, id: u64, at: Vec3, dir: Vec3, r: f64, depth: Option<f64>) -> Option<Bore> {
    use solidcraft_kernel::FaceSurface;
    let prefix = format!("F{id}:bore:");
    let (mut wall, mut floor, mut changed) = (false, false, false);
    let mut body = None;
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
                Some(FaceSurface::Cylinder { axis, radius, .. }) if axis.dot(dir).abs() > 1.0 - 1e-6 => {
                    changed |= (radius - r).abs() > 1e-4 * (1.0 + r);
                    false
                }
                Some(FaceSurface::Plane { normal, .. }) if normal.dot(dir).abs() > 1.0 - 1e-6 => {
                    // A flat floor lies at the depth (a drill point's cone, below it, isn't checked).
                    changed |= depth.is_some_and(|d| (along - d).abs() > 1e-4 * (1.0 + d));
                    true
                }
                Some(FaceSurface::Cone { axis, .. }) if axis.dot(dir).abs() > 1.0 - 1e-6 => true,
                _ => off < r * 0.999 - 1e-6,
            };
            if across {
                floor = true;
            } else {
                wall = true;
            }
            body.get_or_insert_with(|| b.name.clone());
        }
    }
    body.filter(|_| wall || floor).map(|body| Bore { floor, body, changed })
}

/// The callout's text, one line each, the count on the first: for a thread of its own depth,
/// shorter than the hole, the thread first (ASME Y14.5, ISO 6410), then the drill and its extent;
/// then the counterbore or countersink. Both depths read from the face the hole starts on. A
/// tapped hole whose thread runs its length is one line, the thread in place of the drill.
pub fn text(c: &HoleCallout, style: &TextStyle) -> Result<Vec<String>> {
    style.validate()?;
    let len = |mm: f64| display_units::length_number(mm, &style.units, style.decimals);
    let count = if c.count > 1 { format!("{}× ", c.count) } else { String::new() };
    // A thread at least as deep as a blind hole runs its length: one line, as with no depth of
    // its own (the feature keeps a thread within the hole's wall, so it is never deeper).
    let thread_depth = match (c.thread_depth, &c.extent) {
        (Some(t), Extent::Blind { depth }) if t >= depth - 1e-9 * depth.max(1.0) => None,
        (t, _) => t,
    };
    let extent = match c.extent {
        Extent::Through => format!(" {}", words(&style.language).through),
        Extent::Blind { depth } => format!(" ↧ {}", len(depth)),
    };
    let mut lines = match (&c.thread, thread_depth) {
        (Some(t), Some(depth)) => vec![format!("{count}{t} ↧ {}", len(depth)), format!("Ø{}{extent}", len(c.diameter))],
        (Some(t), None) => vec![format!("{count}{t}{extent}")],
        (None, _) => vec![format!("{count}Ø{}{extent}", len(c.diameter))],
    };
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
        HoleCallout {
            count,
            diameter: 6.6,
            thread: None,
            thread_depth: None,
            extent,
            form,
            start: [0.0; 3],
            start_face: String::new(),
            direction: [0.0, 0.0, -1.0],
        }
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
        // Any length unit a drawing may fix, such as yards or micrometres.
        assert_eq!(text(&callout(1, Extent::Through, Form::Simple), &style("um", 0, "en")).unwrap(), ["Ø6600 THRU"]);
    }

    #[test]
    fn a_thread_of_its_own_depth_gets_its_own_line() {
        let mut c = callout(2, Extent::Blind { depth: 15.0 }, Form::Simple);
        c.diameter = 6.75;
        c.thread = Some("M8".into());
        assert_eq!(text(&c, &style("mm", 2, "en")).unwrap(), ["2× M8 ↧ 15.00"]);
        c.thread_depth = Some(10.0);
        assert_eq!(text(&c, &style("mm", 2, "en")).unwrap(), ["2× M8 ↧ 10.00", "Ø6.75 ↧ 15.00"]);
        // As deep as the hole, or (never from the model, which keeps a thread within the wall)
        // deeper: the thread runs its length, one line as before, never two that disagree.
        for t in [15.0, 16.0] {
            c.thread_depth = Some(t);
            assert_eq!(text(&c, &style("mm", 2, "en")).unwrap(), ["2× M8 ↧ 15.00"]);
        }
        // Through: the thread's line keeps its own depth (from the start face, the counterbore's
        // 4 included), and no blind depth is shown anywhere.
        c.thread_depth = Some(14.0);
        c.extent = Extent::Through;
        c.form = Form::Counterbore { diameter: 14.0, depth: 4.0 };
        assert_eq!(text(&c, &style("mm", 2, "de")).unwrap(), ["2× M8 ↧ 14.00", "Ø6.75 DURCH", "⌴ Ø14.00 ↧ 4.00"]);
    }

    #[test]
    fn a_metric_thread_is_shown_with_a_capital_m() {
        assert_eq!(metric_as_written(" m8x1 "), "M8x1");
        assert_eq!(metric_as_written("M8"), "M8");
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

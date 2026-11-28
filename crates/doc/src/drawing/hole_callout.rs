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
//!   (`F<id>:bore:…`, or `F<id>:point:<point>:bore:…` at a sketch point): a hole with a depth
//!   is blind while its bore keeps a floor on its axis, and through when the depth reaches past
//!   the material. A hole whose bore is in the model the
//!   feature made but not in the model as it is now was removed by a later feature, and reads as
//!   failed with that reason. A bore whose wall is no longer of the feature's diameter, or whose
//!   flat floor is no longer at its depth, was changed by a later feature that kept its faces'
//!   names (an offset face, a scale), and reads as failed too; a drill point's cone and the
//!   counterbore or countersink are not checked. The depth is the feature's, measured from the
//!   face the hole starts on, which must still start it just outside its widest rim: a hole whose
//!   start face a later cut, press-pull or boss moved or covered reads as failed. So does a
//!   through hole with anything of its body on its axis as far as it drills (a later join that
//!   closed it), and a hole that a later pattern or mirror copies (the copies are not counted
//!   yet). A later `solid.move` of the hole's body is followed, so the hole is
//!   looked for where the move put it, and `start` and `direction` are in the frame of the model
//!   as it is now (of the hole's own component, the same for every placement of it). A body
//!   moved any other way reads as failed.
//! - A tapped hole takes any thread the Hole feature takes (ISO metric, UNC/UNF, G, Rp), by its
//!   standard designation however it was typed (`m8X1` is `M8x1`, the coarse `M8x1.25` is `M8`),
//!   with the tolerance class and hand the feature keeps (`M22x1-6H-LH`, `1/4-20 UNC-2B-LH`). With
//!   its own thread depth, the drill gets a line of its own and the thread line that depth, both
//!   from the face the hole starts on: the feature counts its thread from below any counterbore
//!   or countersink, so the callout adds that recess. A thread that reaches the floor of a blind
//!   hole is one line.
//! - A hole in sheet metal reads as failed, not supported yet: the sheet is rebuilt from its own
//!   record with the hole in it, so no face is named after the hole.
//! - `count` is the feature's; the extent is this hole's. One feature can drill through on one
//!   hole and blind on another (a stepped part), so `6×` does not say the six share it.
//! - The text: lengths through the shared length formatter ([`crate::display_units`]) and the
//!   diameter sign dimensions use, as a dimension's are; a length that would round to 0 takes the
//!   decimals that show its first significant digit. An angle in degrees with decimals of its own.
//!   The words in the drawing's content language. Nothing is read from a view, and nothing is
//!   saved.
//!
//! Holes made by other features, and the copies a pattern or a mirror makes of a hole, are not
//! supported yet: a hole that is copied reads as failed, not supported yet.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
use solidcraft_kernel::PointClass;
use solidcraft_kernel::affine;

use super::dimension::DIAMETER_SIGN;
use crate::display_units::NumberFormat;
use crate::expr::{Kind, Value};
use crate::thread_forms::ThreadHand;
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
    /// The thread of a tapped hole by its standard designation (`M8`, `M8x1`, `1/4-20 UNC`,
    /// `G 1/4`), shown in place of the diameter with its class and hand.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    /// The tapped thread's tolerance class as the feature keeps it (`6H`, `2B`), when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_class: Option<String>,
    /// The tapped thread's hand, right or left: present with `thread`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hand: Option<ThreadHand>,
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
            return Err(DocError::Invalid(format!("{} is {kind}, not a Hole feature", hole.feature)));
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
            let present = sketch_points(doc, sp.sketch.0);
            if !present.contains(id.as_str()) {
                return Ok(Resolution::Lost { reason: format!("point `{}` is gone from sketch {}", id.escape_debug(), sp.sketch.0) });
            }
            // A Single hole drills at its first point only.
            if sp.single && i != 0 {
                let first = sp.ids.first().map(|p| p.escape_debug().to_string()).unwrap_or_default();
                return Ok(Resolution::Lost { reason: format!("feature {} drills only at point `{first}`", f.id) });
            }
            // The feature drills only the points still in its sketch (#8087), so its hole is this point's
            // place among those.
            sp.ids[..i].iter().filter(|x| present.contains(x.as_str())).count()
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
    let copied = |body: &str| later_copy(doc, model, f.id, body);
    measure(doc, &vals, &before, &own, &model.state(), f, index, &motion, &copied)
}

/// Whether a Pattern or Mirror after feature `id` copies its holes: one that replays the feature
/// (also through a pattern or mirror it replays) or copies the body `body` they are in.
fn later_copy(doc: &Document, model: &Model, id: u64, body: &str) -> bool {
    let Some(i) = model.results.iter().position(|r| r.id == id) else { return false };
    model.results.iter().skip(i + 1).filter(|r| !r.skipped && r.error.is_none()).any(|r| {
        let Some(g) = doc.feature(r.id).filter(|g| !g.suppressed) else { return false };
        let (FeatureKind::Pattern { features, bodies, .. } | FeatureKind::Mirror { features, bodies, .. }) = &g.kind else { return false };
        bodies.iter().any(|b| *b == body) || crate::eval::replayed_sources(doc, features).iter().any(|s| s.id == id)
    })
}

/// The rigid motion the Move features after feature `id` give the body `body`, in order: what
/// carries a place in the model just after `id` to the model as it is now. A Move that copies
/// the body leaves it where it is.
fn later_motion(doc: &Document, model: &Model, vals: &BTreeMap<String, Value>, id: u64, body: &str) -> Result<Mat> {
    let mut m = crate::IDENTITY;
    let Some(i) = model.results.iter().position(|r| r.id == id) else { return Ok(m) };
    for r in model.results.iter().skip(i + 1).filter(|r| !r.skipped && r.error.is_none()) {
        let Some(f) = doc.feature(r.id).filter(|f| !f.suppressed) else { continue };
        let FeatureKind::Move { bodies, copy, .. } = &f.kind else { continue };
        if *copy || !bodies.iter().any(|b| *b == body) {
            continue;
        }
        // Read against the model the Move was evaluated on, so a turn about a picked edge turns
        // where eval turned it. A lost edge was already reported on the Move itself.
        let (t, origin, axis, angle) = crate::eval::move_motion(doc, vals, &model.state_before(r.id), f, &mut None)?;
        m = crate::mat_mul(&crate::rigid(t, origin, axis, angle), &m);
    }
    Ok(m)
}

/// What the id `id`, which names no feature, names instead, if anything: a component or an
/// occurrence, or any item [`Document::next_item_id`] gives ids to.
fn other_item(doc: &Document, id: u64) -> Option<&'static str> {
    let a = &doc.assembly;
    [
        ("a component", doc.components.iter().any(|c| c.id == id)),
        ("an occurrence", doc.occurrences.iter().any(|o| o.id == id)),
        ("a browser folder", doc.browser_groups.iter().any(|g| g.id == id)),
        ("a timeline folder", doc.timeline_groups.iter().any(|g| g.id == id)),
        ("a canvas", doc.canvases.iter().any(|c| c.id == id)),
        ("a point cloud", doc.point_clouds.iter().any(|c| c.id == id)),
        ("a note", doc.notes.iter().any(|n| n.id == id)),
        ("a joint", a.joints.iter().any(|j| j.id == id)),
        ("a contact set", a.contact_sets.iter().any(|c| c.id == id)),
        ("a motion study", a.studies.iter().any(|s| s.id == id)),
        ("an exploded view", a.exploded.iter().any(|v| v.id == id)),
        ("a rigid group", a.rigid_groups.iter().any(|g| g.id == id)),
        ("a configuration row", doc.configs.rows.iter().any(|r| r.id == id)),
    ]
    .into_iter()
    .find_map(|(kind, is)| is.then_some(kind))
}

/// The ids of the points the sketch feature `sketch` has now: none when it is no sketch.
fn sketch_points(doc: &Document, sketch: u64) -> BTreeSet<&str> {
    match doc.feature(sketch).map(|f| &f.kind) {
        Some(FeatureKind::Sketch { sketch, .. }) => sketch.points.iter().map(|p| p.id.as_str()).collect(),
        _ => BTreeSet::new(),
    }
}

/// Hole `index` of the Hole feature `f`, drilled into `before`, made in `own` (the model just
/// after the feature) and found in `after` (the model as it is now), where `motion` (of the body
/// it is in) carried it. Failed when `after` has no face of that hole there.
fn measure(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    before: &ModelState,
    own: &ModelState,
    after: &ModelState,
    f: &Feature,
    index: usize,
    motion: &dyn Fn(&str) -> Result<Mat>,
    copied: &dyn Fn(&str) -> bool,
) -> Result<Resolution> {
    let FeatureKind::Hole { diameter, depth, hole, thread, thread_depth, thread_class, hand, to, .. } = &f.kind else {
        return Err(DocError::Invalid("not a hole".into()));
    };
    // Measured as the evaluator drills it: a hole at sketch points in its sketch's frame, meeting
    // each body it cuts where that body is shown (#17623). `frames` holds each such body's frame
    // until the bore is found; the callout gives the hole in the frame of the body it is in.
    let frames = crate::eval::ToolFrames::install(doc, f, before);
    let (before, own) = (crate::eval::targets_as_shown(before, f, &[])?, crate::eval::targets_as_shown(own, f, &[])?);
    let before: &ModelState = &before;
    let (holes, dir) = crate::eval::hole_spots(doc, vals, before, f)?;
    if holes.is_empty() || holes.len() > MAX_HOLES {
        return Err(DocError::Invalid(format!("a hole needs 1…{MAX_HOLES} points")));
    }
    let spot = holes.get(index).ok_or_else(|| DocError::Invalid("the hole's point is not drilled".into()))?;
    let (at, bore) = (spot.at, bore_prefix(f.id, spot.point));
    let spots: Vec<Vec3> = holes.iter().map(|spot| spot.at).collect();
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
        HoleKind::Counterbore { cb_diameter, cb_depth, tip_angle: _, bridge: _ } => {
            Form::Counterbore { diameter: length(cb_diameter, "counterbore diameter")?, depth: length(cb_depth, "counterbore depth")? }
        }
        HoleKind::Countersink { cs_diameter, cs_angle, tip_angle: _ } => {
            Form::Countersink { diameter: length(cs_diameter, "countersink diameter")?, angle: angle(cs_angle, "countersink angle")? }
        }
    };
    // Any thread the feature takes, by its standard designation however it was typed (ISO 965-1
    // and ISO 261: `M8x1`, a coarse thread without its pitch, `M8`), with the class and hand the
    // feature keeps. Eval refuses a class the thread doesn't take; a hand-edited file is checked
    // again here, and a hole's thread is never both hands.
    let tapped = match thread {
        Some(t) => match crate::parse_tapped_thread(t).filter(|_| t.trim().len() <= 32) {
            Some(tapped) => Some(tapped),
            None => return Err(DocError::Invalid("the hole's thread is not one a tapped hole takes (ISO metric, UNC, UNF, G or Rp)".into())),
        },
        None => None,
    };
    let thread_class = match (thread_class, &tapped) {
        (Some(class), Some(tapped)) => {
            tapped.check_class(class).map_err(DocError::Invalid)?;
            Some(class.clone())
        }
        _ => None,
    };
    let hand = match (&tapped, hand) {
        (Some(_), ThreadHand::Both) => return Ok(Resolution::Failed { reason: "a tapped hole's thread is right- or left-hand, never both".into() }),
        (Some(_), h) => Some(*h),
        (None, _) => None,
    };
    let thread = tapped.map(|t| t.designation);
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
    let Some(made) = bore_floor(&own, &bore, at, dir, r, depth) else {
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
    let frame = crate::eval::tool_frame(&made.body).unwrap_or(crate::IDENTITY);
    drop(frames);
    let m = crate::mat_mul(&motion(&made.body)?, &frame);
    let (start, direction) = (crate::apply_point(&m, at), crate::apply_vector(&m, dir));
    let Some(now) = bore_floor(after, &bore, start, direction, r, depth) else {
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
    if copied(&made.body) {
        return Ok(Resolution::Failed { reason: "not supported yet: the copies a pattern or mirror makes of a hole are not counted".into() });
    }
    // The face the hole starts on still starts it, just outside its widest rim (the counterbore's
    // or countersink's): a later cut, press-pull or boss there keeps the hole's faces at the old
    // depth. A ray along the hole from above the rim, on either side where the bore's wall faces,
    // meets the start face first among the bodies with the hole's faces (a split hole is in two),
    // as far down as it did just after the hole. It starts twice the rim's radius up, above a
    // start face curved round the hole (a hole in a cylinder).
    let rim = match form {
        Form::Counterbore { diameter, .. } | Form::Countersink { diameter, .. } => diameter / 2.0,
        Form::Simple | Form::Drilled { .. } => 0.0,
    }
    .max(r);
    let rim = rim + 1e-3 * (1.0 + rim);
    let start_hit = |st: &ModelState, from: Vec3, along: Vec3| {
        first_face(st, &bore, from, along).filter(|(_, name)| !start_face.is_empty() && *name == start_face).map(|(hit, _)| hit)
    };
    let beside = made
        .outward
        .into_iter()
        .flat_map(|u| [1.0, -1.0].map(|side| at + u * (side * rim) - dir * (2.0 * rim)))
        .find_map(|from| start_hit(&own, from, dir).map(|hit| (from, hit)));
    let Some((from, hit)) = beside else {
        return Ok(Resolution::Failed { reason: "could not check the face the hole starts on: it is not just outside the hole's rim".into() });
    };
    let still = start_hit(after, crate::apply_point(&m, from), direction).is_some_and(|now| (now - hit).abs() <= 1e-4 * (1.0 + hit));
    if !still {
        return Ok(Resolution::Failed { reason: "a later feature moved or covered the face the hole starts on".into() });
    }
    let extent = match depth {
        Some(depth) if now.floor => Extent::Blind { depth },
        _ => Extent::Through,
    };
    // A through hole has nothing of its body on its axis, as far as it drills (through all, or
    // its depth): a later feature closed it or put material in its way.
    let below = 1e-3 * (1.0 + r);
    if matches!(extent, Extent::Through)
        && let Some((hit, _)) = first_face(after, &bore, start + direction * below, direction)
        && depth.is_none_or(|depth| hit + below < depth)
    {
        let deep = hit + below;
        return Ok(Resolution::Failed {
            reason: format!("a later feature closed the hole or put material on its axis: it is now blind, {deep:.2} mm deep"),
        });
    }
    let callout = HoleCallout {
        count: spots.len(),
        diameter,
        thread,
        thread_class,
        hand,
        thread_depth,
        extent,
        form,
        start: start.into(),
        start_face,
        direction: direction.into(),
    };
    Ok(Resolution::Resolved { callout })
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
    /// The unit direction from the axis out to a spot on its wall, across the axis.
    outward: Option<Vec3>,
}

/// Whether the hole drilled at `at` along `dir` (of radius `r`) starts in a sheet-metal body of
/// `st`.
fn in_sheet(st: &ModelState, at: Vec3, dir: Vec3, r: f64) -> bool {
    let inside = at + dir * (r.min(1.0) * 1e-3);
    st.surface_sheets
        .iter()
        .filter_map(|sh| st.body(&sh.body))
        .any(|b| !b.body.is_mesh() && matches!(b.body.classify_point(inside), Ok(PointClass::Inside)))
}

/// The start of the names of the faces of the bore of feature `id` drilled at `point` (none at a
/// position): `F<id>:bore:`, or `F<id>:<point role>:bore:` at a sketch point, where each point's hole
/// names its faces with its point's role (`F3:point:p2:bore:side:0`).
fn bore_prefix(id: u64, point: Option<&str>) -> String {
    match point {
        Some(p) => format!("F{id}:{}:bore:", crate::eval::hole_point_role(p)),
        None => format!("F{id}:bore:"),
    }
}

/// Whether a face name, after its feature's `F<id>:`, is a bore's: `bore:…`, alone or after a point's
/// role (`point:<id>:` or `point-digest:<hex>:`, which hold no `:` of their own).
fn is_bore_role(role: &str) -> bool {
    let role = match role.strip_prefix("point:").or_else(|| role.strip_prefix("point-digest:")) {
        Some(after) => after.split_once(':').map_or("", |(_, rest)| rest),
        None => role,
    };
    role.starts_with("bore:")
}

/// Whether a body of `st` has a face of a bore of feature `id` that lies on none of the axes of
/// its holes at `holes` (drilled along `dir`, of radius `r`): one of them is somewhere else now.
/// The feature's other holes, still in place, don't count.
fn stray_bore(st: &ModelState, id: u64, holes: &[Vec3], dir: Vec3, r: f64) -> bool {
    let feature = format!("F{id}:");
    let is_bore = |name: &str| name.strip_prefix(feature.as_str()).is_some_and(is_bore_role);
    let on_an_axis = |p: Vec3| {
        holes.iter().any(|at| {
            let along = affine::distance_along((*at).into(), dir.into(), p.into());
            (p - *at - dir * along).len() <= r * 1.001 + 1e-6
        })
    };
    st.bodies.iter().filter(|b| !b.body.is_mesh()).any(|b| {
        let names = crate::naming::face_names(b);
        if !names.iter().any(|n| is_bore(n)) {
            return false;
        }
        let spots = crate::naming::face_spots(b);
        names.iter().zip(spots.iter()).any(|(name, spot)| is_bore(name) && spot.is_some_and(|s| !on_an_axis(s.point)))
    })
}

/// The cosine of the angle between unit directions `a` and `b`, through the kernel
/// ([`affine::distance_along`] from the origin).
fn cosine(a: Vec3, b: Vec3) -> f64 {
    affine::distance_along([0.0; 3], b.into(), a.into())
}

/// The bore whose faces' names start with `prefix` ([`bore_prefix`]) at `at` (drilled along
/// `dir`, of radius `r`, `depth` deep when blind) in `st`, and whether it keeps a floor: a face of
/// the bore across its axis below the start, the flat bottom or the drill point. `None` when no face of that bore is there
/// at all.
fn bore_floor(st: &ModelState, prefix: &str, at: Vec3, dir: Vec3, r: f64, depth: Option<f64>) -> Option<Bore> {
    use solidcraft_kernel::FaceSurface;
    let (mut wall, mut floor, mut changed) = (false, false, false);
    let (mut body, mut outward) = (None, None);
    for b in st.bodies.iter().filter(|b| !b.body.is_mesh()) {
        let names = crate::naming::face_names(b);
        if !names.iter().any(|n| n.starts_with(prefix)) {
            continue;
        }
        // One pass over the body's triangles for all its faces, not one per bore face.
        let spots = crate::naming::face_spots(b);
        for (name, spot) in names.iter().zip(spots.iter()) {
            let Some(spot) = spot.filter(|_| name.starts_with(prefix)) else { continue };
            // The wall is a cylinder along the axis. Its radius is checked wherever its spot lies:
            // a wall moved out lies past the radius the spots are looked for in below.
            let cylinder = match spot.surface {
                Some(FaceSurface::Cylinder { axis, radius, .. }) if cosine(Vec3::from(axis), dir).abs() > 1.0 - 1e-6 => Some(radius),
                _ => None,
            };
            changed |= cylinder.is_some_and(|radius| (radius - r).abs() > 1e-4 * (1.0 + r));
            let v = spot.point - at;
            let along = affine::distance_along(at.into(), dir.into(), spot.point.into());
            let off = (v - dir * along).len();
            // On this hole's axis and below its start.
            if along <= 0.0 || off > r * 1.001 + 1e-6 {
                continue;
            }
            // A flat bottom or a drill point lies across the axis. Only a face of no known surface
            // is told by where its spot is: the wall at the radius (a facet of a curved face lies
            // inside it by its sag, so this is a fallback).
            let across = match spot.surface {
                _ if cylinder.is_some() => false,
                Some(FaceSurface::Plane { normal, .. }) if cosine(Vec3::from(normal), dir).abs() > 1.0 - 1e-6 => {
                    // A flat floor lies at the depth (a drill point's cone, below it, isn't checked).
                    changed |= depth.is_some_and(|d| (along - d).abs() > 1e-4 * (1.0 + d));
                    true
                }
                Some(FaceSurface::Cone { axis, .. }) if cosine(Vec3::from(axis), dir).abs() > 1.0 - 1e-6 => true,
                _ => off < r * 0.999 - 1e-6,
            };
            if across {
                floor = true;
            } else {
                wall = true;
                // Out from the axis towards this spot of the wall.
                if off > r * 0.5 {
                    outward.get_or_insert((v - dir * along) * (1.0 / off));
                }
            }
            body.get_or_insert_with(|| b.name.clone());
        }
    }
    body.filter(|_| wall || floor).map(|body| Bore { floor, body, changed, outward })
}

/// The first face met by the ray from `from` along `along` among the bodies of `st` that have a
/// face of the bore whose names start with `prefix`, as its distance and its name.
fn first_face(st: &ModelState, prefix: &str, from: Vec3, along: Vec3) -> Option<(f64, String)> {
    st.bodies
        .iter()
        .filter(|b| !b.body.is_mesh())
        .filter_map(|b| {
            let names = crate::naming::face_names(b);
            if !names.iter().any(|n| n.starts_with(prefix)) {
                return None;
            }
            let (hit, face) = b.body.raycast_face(from, along)?;
            Some((hit, names.get(face).cloned().unwrap_or_default()))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// The callout's text, one line each, the count on the first: for a thread of its own depth,
/// shorter than the hole, the thread first (ASME Y14.5, ISO 6410), then the drill and its extent;
/// then the counterbore or countersink. Both depths read from the face the hole starts on. A
/// tapped hole whose thread runs its length is one line, the thread in place of the drill.
pub fn text(c: &HoleCallout, style: &TextStyle) -> Result<Vec<String>> {
    style.validate()?;
    let len = |mm: f64| length(mm, style);
    let diameter = len(c.diameter)?;
    let count = if c.count > 1 { format!("{}× ", c.count) } else { String::new() };
    // A thread at least as deep as a blind hole runs its length: one line, as with no depth of
    // its own (the feature keeps a thread within the hole's wall, so it is never deeper).
    let thread_depth = match (c.thread_depth, &c.extent) {
        (Some(t), Extent::Blind { depth }) if t >= depth - 1e-9 * depth.max(1.0) => None,
        (t, _) => t,
    };
    let extent = match c.extent {
        Extent::Through => format!(" {}", words(&style.language).through),
        Extent::Blind { depth } => format!(" ↧ {}", len(depth)?),
    };
    let thread = c.thread.as_ref().map(|t| designation(t, c.thread_class.as_deref(), c.hand));
    let mut lines = match (thread, thread_depth) {
        (Some(t), Some(depth)) => vec![format!("{count}{t} ↧ {}", len(depth)?), format!("{DIAMETER_SIGN}{diameter}{extent}")],
        (Some(t), None) => vec![format!("{count}{t}{extent}")],
        (None, _) => vec![format!("{count}{DIAMETER_SIGN}{diameter}{extent}")],
    };
    match c.form {
        Form::Simple | Form::Drilled { .. } => {}
        Form::Counterbore { diameter, depth } => lines.push(format!("⌴ {DIAMETER_SIGN}{} ↧ {}", len(diameter)?, len(depth)?)),
        Form::Countersink { diameter, angle } => lines.push(format!("⌵ {DIAMETER_SIGN}{} × {}°", len(diameter)?, degrees(angle)?)),
    }
    Ok(lines)
}

/// A thread's designation with its tolerance class and hand: the class after the size, then `LH`
/// for a left-hand thread, each after a hyphen (ISO 965-1's designation of a metric thread,
/// `M22x1-6H-LH`; ASME B1.1's of a Unified one, `1/4-20 UNC-2B-LH`). A right-hand thread has no
/// mark of its hand.
fn designation(thread: &str, class: Option<&str>, hand: Option<ThreadHand>) -> String {
    let mut out = thread.to_string();
    if let Some(class) = class {
        out.push('-');
        out.push_str(class);
    }
    if hand == Some(ThreadHand::Left) {
        out.push_str("-LH");
    }
    out
}

/// A length in millimetres as the callout shows it: by the shared length formatter, as a
/// dimension's number is, so a callout and a dimension on one sheet read alike (including the
/// decimal mark, which is the formatter's and not the content language's). A positive length
/// that would read 0 at the style's decimals takes the decimals that show its first significant
/// digit instead (`Ø0.05`, never `Ø0`), as inch lengths already keep a meaningful fourth digit; one
/// too small to show even with [`MAX_DECIMALS`] is refused.
fn length(mm: f64, style: &TextStyle) -> Result<String> {
    let nonzero = |s: &str| s.bytes().any(|b| (b'1'..=b'9').contains(&b));
    let shown = display_units::length_number(mm, &style.units, style.decimals);
    if mm.is_nan() || mm <= 0.0 || nonzero(&shown) {
        return Ok(shown);
    }
    // The decimals of its first significant digit, or one more where the binary value rounds
    // down there (0.0995 → 0.10).
    let value = mm * display_units::unit_scale(&style.units).0;
    let first = (-value.log10()).ceil();
    if first.is_finite() && first <= f64::from(MAX_DECIMALS) {
        let first = (first.max(0.0) as u8).max(style.decimals);
        for decimals in [first, first.saturating_add(1).min(MAX_DECIMALS)] {
            let shown = display_units::length_number(mm, &style.units, decimals);
            if nonzero(&shown) {
                return Ok(shown);
            }
        }
    }
    let unit: String = style.units.chars().take(32).collect();
    Err(DocError::Invalid(format!("a length of the hole is too small to show in {} with up to {MAX_DECIMALS} decimals", unit.escape_debug())))
}

/// The decimals an angle in a callout is shown with: the default drafting profile's angle
/// decimals, trailing zeros dropped, so a countersink reads `90°` or `82.5°` whatever decimals
/// its lengths take (ISO 129-1 writes angles in degrees).
const ANGLE_DECIMALS: u8 = 2;

/// An angle in radians, in degrees as the callout shows it.
fn degrees(radians: f64) -> Result<String> {
    let f = NumberFormat { trailing_zeros: false, ..NumberFormat::default() };
    let text = display_units::format_number(radians.to_degrees(), ANGLE_DECIMALS, &f)?.text;
    // Trailing zeros dropped leave a whole number's point behind (`90.`).
    Ok(text.trim_end_matches('.').to_string())
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
            thread_class: None,
            hand: None,
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
        assert_eq!(text(&cs, &en).unwrap(), ["Ø6.6 ↧ 12.0", "⌵ Ø12.0 × 90°"]);
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

    /// BTUSALLC/solidcraft issue 19: the class after the size, then `-LH` (ISO 965-1, ASME B1.1).
    #[test]
    fn a_threads_class_and_hand_follow_its_size() {
        let mut c = callout(1, Extent::Blind { depth: 15.0 }, Form::Simple);
        c.thread = Some("M22x1".into());
        c.thread_class = Some("6H".into());
        c.hand = Some(ThreadHand::Left);
        assert_eq!(text(&c, &style("mm", 2, "en")).unwrap(), ["M22x1-6H-LH ↧ 15.00"]);
        c.thread_depth = Some(10.0);
        assert_eq!(text(&c, &style("mm", 2, "en")).unwrap(), ["M22x1-6H-LH ↧ 10.00", "Ø6.60 ↧ 15.00"]);
        assert_eq!(designation("1/4-20 UNC", Some("2B"), Some(ThreadHand::Right)), "1/4-20 UNC-2B");
        assert_eq!(designation("1/4-20 UNC", Some("2B"), Some(ThreadHand::Left)), "1/4-20 UNC-2B-LH");
        assert_eq!(designation("M10", None, Some(ThreadHand::Left)), "M10-LH");
        assert_eq!(designation("M8", None, Some(ThreadHand::Right)), "M8");
    }

    /// BTUSALLC/solidcraft issue 20: the thread's designation is the catalogue's, however typed.
    #[test]
    fn a_metric_thread_is_shown_by_its_standard_designation() {
        let shown = |t: &str| crate::parse_tapped_thread(t).map(|t| t.designation);
        assert_eq!(shown(" m8x1 ").as_deref(), Some("M8x1"));
        assert_eq!(shown("M8X1").as_deref(), Some("M8x1"));
        assert_eq!(shown("M8").as_deref(), Some("M8"));
        assert_eq!(shown("M8x1.25").as_deref(), Some("M8"), "coarse: no pitch (ISO 261)");
    }

    /// BTUSALLC/solidcraft issue 20: an angle with decimals of its own; a small length never 0;
    /// the numbers and the diameter sign as a dimension's on the same sheet, in any language.
    #[test]
    fn angles_and_small_lengths_read_as_a_drawing_writes_them() {
        let cs = |angle: f64| callout(1, Extent::Through, Form::Countersink { diameter: 12.0, angle: angle.to_radians() });
        assert_eq!(text(&cs(90.0), &style("mm", 6, "en")).unwrap(), ["Ø6.600000 THRU", "⌵ Ø12.000000 × 90°"]);
        assert_eq!(text(&cs(82.0), &style("in", 0, "en")).unwrap(), ["Ø0.2598 THRU", "⌵ Ø0.4724 × 82°"]);
        assert_eq!(text(&cs(82.5), &style("mm", 0, "en")).unwrap(), ["Ø7 THRU", "⌵ Ø12 × 82.5°"]);
        assert_eq!(ANGLE_DECIMALS, crate::drafting_profile::DraftingProfile::default().angle_decimals);
        // 90° made from radians and back is not exactly 90.
        assert_eq!(degrees(std::f64::consts::FRAC_PI_2).unwrap(), "90");
        let mut tiny = callout(1, Extent::Blind { depth: 0.2 }, Form::Simple);
        tiny.diameter = 0.05;
        assert_eq!(text(&tiny, &style("mm", 0, "en")).unwrap(), ["Ø0.05 ↧ 0.2"]);
        assert_eq!(text(&tiny, &style("mm", 3, "en")).unwrap(), ["Ø0.050 ↧ 0.200"]);
        assert_eq!(text(&tiny, &style("um", 0, "en")).unwrap(), ["Ø50 ↧ 200"]);
        // Too small to show even with every decimal: refused, never `Ø0`.
        tiny.diameter = 1e-7;
        assert!(text(&tiny, &style("mm", 0, "en")).unwrap_err().to_string().contains("too small to show"));
        // A dimension and a callout on one German sheet read alike: the decimal mark is the
        // shared formatter's, not the language's, and the diameter sign is the dimensions'.
        let c = callout(1, Extent::Through, Form::Simple);
        let dimension = super::super::dimension::reading_label(super::super::dimension::LabelKind::Diameter, 6.6, "mm", 2);
        assert_eq!(text(&c, &style("mm", 2, "de-DE")).unwrap(), [format!("{dimension} DURCH")]);
        assert_eq!(dimension, "Ø6.60");
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

//! Dogbone reliefs (CNC routing): a round bite of the bit's diameter cut into each inside
//! corner edge of a body a round bit would leave rounded, so a square part or board seats in
//! it. The corners are picked edges, or the inside corners at picked planar faces (a pocket's
//! floor, a plate's face a slot runs through). The kernel says which face each face pick means
//! (on the exact faces), which edges are inside corners and where each relief cylinder goes
//! (`solidcraft_kernel::dogbone`); this module picks the body and the edges, cuts the cylinders
//! and keeps what was relieved and skipped.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_kernel::{self as kernel, dogbone as db};

use crate::expr::{Kind, Value};
use crate::{DocError, Document, Feature, FeatureKind, ModelState, Operation, Result};

/// Where a relief sits in its corner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DogboneStyle {
    /// On the corner's bisector, the circle through the sharp corner. Of all reliefs that reach
    /// the sharp corner, it removes the least material.
    #[default]
    Normal,
    /// On the bisector, stopping `gap` short of the sharp corner, for mating parts whose corners
    /// are slightly broken.
    Minimal,
    /// Centred on one wall's line, the longer or the shorter one, the circle through the corner.
    /// At a square corner it bites that wall only; at a sharper corner it also bites the other
    /// wall.
    TBone,
}

impl DogboneStyle {
    pub fn is_normal(&self) -> bool {
        *self == DogboneStyle::Normal
    }

    /// Its id in commands and files.
    pub fn id(self) -> &'static str {
        match self {
            DogboneStyle::Normal => "normal",
            DogboneStyle::Minimal => "minimal",
            DogboneStyle::TBone => "t_bone",
        }
    }

    /// The style an id names.
    pub fn parse(s: &str) -> Option<DogboneStyle> {
        match s {
            "normal" => Some(DogboneStyle::Normal),
            "minimal" => Some(DogboneStyle::Minimal),
            "t_bone" | "tbone" => Some(DogboneStyle::TBone),
            _ => None,
        }
    }
}

/// Which wall a T-bone relief bites.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DogboneSide {
    /// The longer wall at the corner.
    #[default]
    Longer,
    /// The shorter wall at the corner.
    Shorter,
}

impl DogboneSide {
    pub fn is_longer(&self) -> bool {
        *self == DogboneSide::Longer
    }

    /// Its id in commands and files.
    pub fn id(self) -> &'static str {
        match self {
            DogboneSide::Longer => "longer",
            DogboneSide::Shorter => "shorter",
        }
    }

    /// The side an id names.
    pub fn parse(s: &str) -> Option<DogboneSide> {
        match s {
            "longer" | "long" => Some(DogboneSide::Longer),
            "shorter" | "short" => Some(DogboneSide::Shorter),
            _ => None,
        }
    }
}

/// The share of the bit's diameter a minimal relief stops short of the corner by when no gap is
/// given: a tenth of the radius.
pub const DEFAULT_GAP_SHARE: f64 = 0.05;

/// The largest bit diameter and gap a dogbone takes (mm), 100 m: far past any router bit or
/// part. The command refuses more, as the feature does.
pub const MAX_LENGTH: f64 = 100_000.0;

/// What a dogbone feature relieved and skipped in its last evaluation.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct DogboneReport {
    /// The relieved corner edges, by a point at the middle of each.
    pub relieved: Vec<Vec3>,
    /// The edges (or picked faces) skipped: a point on each and why, as a reason id.
    pub skipped: Vec<(Vec3, &'static str)>,
}

/// A skip reason's id (`convex`, `too_open`, …), as commands report it.
pub fn reason_id(why: &db::Why) -> &'static str {
    match why {
        db::Why::Corner(db::Skip::Convex) => "convex",
        db::Why::Smooth => "flat",
        db::Why::Corner(db::Skip::TooOpen) => "too_open",
        db::Why::Corner(db::Skip::Unreached) => "gap_too_large",
        db::Why::NotStraight => "not_straight",
        db::Why::NotTwoFaces => "not_between_two_faces",
        db::Why::NotPlanar => "not_planar",
        db::Why::NotAlong => "not_along_axis",
        db::Why::NoCorners => "no_corners",
    }
}

/// How many skipped edges a feature's warning names before "and N more".
const NAMED_SKIPS: usize = 6;

/// Cut the reliefs of dogbone feature `f` from its body in `st`.
pub(crate) fn evaluate(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState, warning: &mut Option<String>) -> Result<()> {
    let FeatureKind::Dogbone { edges, faces, diameter, style, gap, side, max_angle, along, body } = &f.kind else {
        return Err(DocError::Invalid("not a dogbone".into()));
    };
    let d = crate::eval::val(vals, diameter, Kind::Length)?;
    if !(d.is_finite() && d > 0.0 && d <= MAX_LENGTH) {
        return Err(DocError::Invalid(format!("the bit diameter must be more than 0 and at most {MAX_LENGTH} mm")));
    }
    let radius = d / 2.0;
    let relief_style = match style {
        DogboneStyle::Normal => db::Style::Normal,
        DogboneStyle::Minimal => {
            let g = match gap {
                Some(e) => crate::eval::val(vals, e, Kind::Length)?,
                None => d * DEFAULT_GAP_SHARE,
            };
            if !(g.is_finite() && (0.0..=MAX_LENGTH).contains(&g)) {
                return Err(DocError::Invalid(format!("the gap must be at least 0 and at most {MAX_LENGTH} mm")));
            }
            db::Style::Minimal { gap: g }
        }
        DogboneStyle::TBone => db::Style::TBone {
            side: match side {
                DogboneSide::Longer => db::Side::Longer,
                DogboneSide::Shorter => db::Side::Shorter,
            },
        },
    };
    let largest = match max_angle {
        Some(e) => crate::eval::val(vals, e, Kind::Angle)?,
        None => std::f64::consts::FRAC_PI_2,
    };
    if !(largest.is_finite() && largest > 0.0 && largest <= std::f64::consts::PI) {
        return Err(DocError::Invalid("the largest corner angle must be between 0° and 180°".into()));
    }
    // A zero axis is the kernel's to refuse; the command never saves one.
    if along.as_ref().is_some_and(|a| !a.0.is_finite()) {
        return Err(DocError::Invalid("the bit's axis must be a direction".into()));
    }
    let picks: Vec<Vec3> = edges.iter().map(|p| p.0).chain(faces.iter().map(|p| p.0)).collect();
    if picks.is_empty() {
        return Err(DocError::Invalid("pick the inside corner edges, or the faces whose inside corners to relieve".into()));
    }
    let i = crate::eval::body_at(st, body, &picks)?;
    let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("the dogbone's body is no longer in the model".into())) };
    if mb.body.is_mesh() {
        return Err(DocError::Invalid(format!("{} is a mesh body: dogbones need a solid", mb.name)));
    }
    let mut notes = Vec::new();
    // Edges by their names first, as a fillet finds its edges after upstream edits.
    let edge_idx = if edges.is_empty() {
        Vec::new()
    } else {
        let mut w = None;
        let all: Vec<usize> = (0..edges.len()).collect();
        let at = crate::eval::blend_edges_on(&mb, f, edges, &all, &mut w)?;
        notes.extend(w);
        kernel::blend_edges(&mb.body, &at)?
    };
    // Faces by the exact face nearest each pick, within the reach any face pick has.
    let face_points: Vec<Vec3> = faces.iter().map(|p| p.0).collect();
    let options = db::BodyOptions {
        relief: db::Options { radius, style: relief_style, max_angle: largest },
        along: along.as_ref().map(|a| [a.0.x, a.0.y, a.0.z]),
    };
    let plan = db::plan(&mb.body, &edge_idx, &face_points, crate::appearance::pick_tolerance(&mb), &options)?;
    let report = DogboneReport {
        relieved: plan.tools.iter().map(|t| Vec3::new(t.at[0], t.at[1], t.at[2])).collect(),
        skipped: plan.skipped.iter().map(|s| (Vec3::new(s.at[0], s.at[1], s.at[2]), reason_id(&s.why))).collect(),
    };
    // The feature warns about the edges it was given that it skipped, and a picked face with no
    // corners; the other edges a picked face reaches (a plate's outside corners) are listed in
    // the command's reply only.
    let warned: Vec<(Vec3, &'static str)> = plan
        .skipped
        .iter()
        .filter(|s| s.face.is_some() || s.edge.is_some_and(|e| edge_idx.contains(&e)))
        .map(|s| (Vec3::new(s.at[0], s.at[1], s.at[2]), reason_id(&s.why)))
        .collect();
    if !warned.is_empty() {
        notes.push(skipped_note(&warned));
    }
    if plan.tools.is_empty() {
        // Nothing relieved: say why for every edge looked at.
        let mut all = notes.clone();
        if !report.skipped.is_empty() && warned.len() < report.skipped.len() {
            all.push(skipped_note(&report.skipped));
        }
        let why = if all.is_empty() { String::new() } else { format!(" ({})", all.join("; ")) };
        return Err(DocError::Invalid(format!("no inside corner to relieve{why}")));
    }
    let mut tools = Vec::new();
    for t in &plan.tools {
        let c = t.cylinder;
        let cylinder = kernel::cylinder(Vec3::new(c.base[0], c.base[1], c.base[2]), Vec3::new(c.axis[0], c.axis[1], c.axis[2]), c.radius, c.height)?;
        tools.push(crate::lineage::cylinder_tool(cylinder));
    }
    st.dogbones.insert(f.id, report);
    if !notes.is_empty() {
        *warning = Some(notes.join("; "));
    }
    crate::eval::apply_traced(doc, st, f, tools, Operation::Cut, std::slice::from_ref(&mb.name))
}

/// What a feature's warning says about the edges it skipped.
fn skipped_note(skipped: &[(Vec3, &'static str)]) -> String {
    let named: Vec<String> =
        skipped.iter().take(NAMED_SKIPS).map(|(p, why)| format!("[{:.3}, {:.3}, {:.3}] {}", p.x, p.y, p.z, why.replace('_', " "))).collect();
    let more = skipped.len().saturating_sub(NAMED_SKIPS);
    let tail = if more > 0 { format!(" and {more} more") } else { String::new() };
    format!("skipped {} edge{}: {}{tail}", skipped.len(), if skipped.len() == 1 { "" } else { "s" }, named.join(", "))
}

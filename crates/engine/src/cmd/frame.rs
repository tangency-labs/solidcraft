//! Frames and weldments (#4003). `frame.member` lays structural members along the lines of a frame
//! sketch, their cross-section a built-in profile, a standard size or a profile sketch;
//! `frame.trim` trims or extends them to other members (butt joints), planes and faces;
//! `frame.corner` mitres two members that meet at their ends.
//! `frame.profile.list` lists the built-in profiles and the standard sizes,
//! `frame.profile.properties` works out a profile's section, and `frame.profile.insert` draws one
//! into a sketch.

use solidcraft_geom::prelude::*;
use std::collections::BTreeMap;

use serde_json::Value;
use solidcore_record::{Loop2, Seg2, Vec2};
use solidcraft_doc::expr::Kind;
use solidcraft_doc::frame::sizes::{self, Family};
use solidcraft_doc::frame::{self, Anchor, CornerKind, MemberProfile, MemberRef, Profile, ProfileSpec, Side, TrimEnd, TrimTarget};
use solidcraft_doc::{FeatureKind, SketchId};
use solidcraft_sketch::Sketch;

use super::features::{add_feature, check_expr, sketch_id};
use super::sketch::edit;
use super::{CommandSpec, in_sketch, not_in_sketch};
use crate::params::{bad, bool_, expr, num, str_, string_list, vec2};
use crate::schema::features::FeatureResult;
use crate::schema::frame::{
    CornerParams, CreatedIds, FrameMemberParams, ProfileInsertParams, ProfileInsertResult, ProfileKindInfo, ProfileListParams, ProfileListResult,
    ProfileOut, ProfilePropertiesParams, ProfilePropertiesResult, ProfileSection, SizeFamilyInfo, TrimParams,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const TRIM_CONTRACT: TypedContract<TrimParams> = TypedContract::new();
const CORNER_CONTRACT: TypedContract<CornerParams> = TypedContract::new();

#[cfg(test)]
#[path = "frame_tests.rs"]
mod tests;

const MEMBER_CONTRACT: TypedContract<FrameMemberParams> = TypedContract::new();
const LIST_CONTRACT: TypedContract<ProfileListParams> = TypedContract::new();
const PROPERTIES_CONTRACT: TypedContract<ProfilePropertiesParams> = TypedContract::new();
const INSERT_CONTRACT: TypedContract<ProfileInsertParams> = TypedContract::new();

/// The profile parameter, as every frame command documents it.
macro_rules! profile_doc {
    () => {
        "profile: {kind: bar|round_bar|rect_tube|round_tube|angle|channel|i_beam, …its dimensions: each a number in the design's units or a length expression over its parameters (\"tube_wall\", \"2 in\"); a channel's flange_slope is an angle (a number of degrees)} (frame.profile.list gives each kind's dimensions and an example; dimensions left out take defaults from the profile's size) | {size: a standard steel size by name, such as \"RSC 203x89\", \"PFC 200x90x30\", \"RSA 50x50x5\", \"SHS 100x100x5\", \"RHS 200x100x6.3\" or \"CHS 42.4x3.2\" (frame.profile.list lists them), alone: it brings its dimensions}"
    };
}

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("frame.member", "Frame Member", crate::checked!("frame.member", member))
        .at("WELDMENTS", "CREATE")
        .icon("frame_member")
        .enabled(not_in_sketch)
        .contract(&MEMBER_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params(concat!(
        "sketch: the frame sketch (id or name; 2D or 3D); lines?: [curve ids] (one member each; default: every straight line drawn in the sketch: not construction, not projected or otherwise linked, not of no length); ",
        profile_doc!(),
        " | profile_sketch: a sketch whose profile is the section, in its own x and y around its origin, with profiles?: picks as solid.extrude takes them (default: its material, nested loops alternating, so two circles make a tube); anchor?: origin|center|centroid|top_left|top|top_right|left|right|bottom_left|bottom|bottom_right (the profile point that sits on each line, default origin); locate?: [x,y] (the profile point that sits on each line, in mm from the profile's own origin: a bar's or an angle's bottom-left corner, a channel's web back at mid-height, the centre of other profiles; takes precedence over `anchor` and keeps the member attached at that point when the profile size changes); angle?: angle expr (the profile turned about the line; default 0: its y as near world up as the line allows, a vertical member's x along world X); mirror?: bool (the profile mirrored across its y axis); name?; body_name? (what the members are named after, then numbered: `Rail`, `Rail (2)`…; default the size's name, `RSC 203x89`, the profile's description, `Rect Tube 40x40x3`, or the profile sketch's name); names?: {line id: name} (names to keep, as Edit Feature keeps them). One body per line, from its start to its end; each member keeps its name when other lines are added or deleted. Returns the feature and the bodies it made"
    )),
    CommandSpec::new("frame.trim", "Trim/Extend", crate::checked!("frame.trim", trim)).contract(&TRIM_CONTRACT).output(output_schema_for::<FeatureResult>).at("WELDMENTS", "MODIFY").icon("frame_trim").enabled(not_in_sketch).params(concat!(
        "members: [member…] (the members to trim or extend: each a member's body name, or {feature: the frame member feature (name or id), line: its line's id}); ",
        "to: [target…] (or one target: what each member's end is trimmed or extended to: a member (its body name, or {member, side?: right|left|top|bottom of its profile; default the side the trimmed member meets}): a butt joint against that side, worked out from its line and profile so it follows edits; or {plane: XY|XZ|YZ|a construction plane|{origin, normal}|{face: [x,y,z]} a planar face}, or {plane_ref} a plane as the design saves it, as Edit Feature keeps it); ",
        "end?: auto|start|end (which end of each member moves; default auto: for each target the end nearer it); gap?: length expr (the weld gap between each end and its target, default 0, at most 1 m). ",
        "Each member's end that meets a target is rebuilt flat against it (a slanted face cuts it at a slant); a member keeps its body and name. Members changed by later features are refused as members or member targets; use a planar face target for a modified body. Every member must meet a target, and two targets may not move the same end. Returns the feature and the bodies it changed"
    )),
    CommandSpec::new("frame.corner", "Corner", crate::checked!("frame.corner", corner)).contract(&CORNER_CONTRACT).output(output_schema_for::<FeatureResult>).icon("frame_trim").enabled(not_in_sketch).params(concat!(
        "members: [member, member] (the two members that meet at the corner: each a member's body name, or {feature: the frame member feature (name or id), line: its line's id}); ",
        "kind?: mitre (default: both cut on the plane halfway between them); gap?: length expr (the weld gap between the two cut faces, default 0, at most 1 m). ",
        "The members meet where the lines through the middle of their profiles cross, which must be at an end of each (within its profile's size); each member's end there is rebuilt cut on the corner plane, extending or shortening it, and keeps its body and name. ",
        "Refused, with the reason, for parallel members, members whose middle lines pass apart or cross away from their ends, one member twice, and members changed by later features. Returns the feature and the bodies it changed"
    )),
    CommandSpec::new("frame.profile.list", "Profile Library", crate::checked!("frame.profile.list", list))
        .icon("library")
        .noundo()
        .doc_only()
        .contract(&LIST_CONTRACT)
        .output(output_schema_for::<ProfileListResult>)
        .params("family?: RSC|PFC|RSA|SHS|RHS|CHS (list only that family's sizes). Returns kinds: [{kind, label, required, optional, example}]: the built-in profiles, each SolidCraft's own parametric definition; and sizes: [{family, kind, standard, source, names, additional?}]: the standard steel sizes by name, each family with the published table its dimensions come from (frame.profile.properties with {profile: {size}} gives a size's dimensions and section)"),
    CommandSpec::new("frame.profile.properties", "Profile Properties", crate::checked!("frame.profile.properties", properties))
        .icon("measure")
        .noundo()
        .doc_only()
        .contract(&PROPERTIES_CONTRACT)
        .output(output_schema_for::<ProfilePropertiesResult>)
        .params(concat!(
        profile_doc!(),
        "; density?: kg/m³ (adds mass_per_m, kg/m). Returns profile (every default filled in), area (mm²), centroid [x,y] (mm, from the bounding box centre), ixx, iyy, ixy (mm⁴ about the centroid), width, height"
    )),
    CommandSpec::new("frame.profile.insert", "Insert Profile", crate::checked!("frame.profile.insert", insert))
        .icon("import")
        .enabled(in_sketch)
        .contract(&INSERT_CONTRACT)
        .output(output_schema_for::<ProfileInsertResult>)
        .params(concat!(
        profile_doc!(),
        "; at?: [x,y] (sketch mm, default the origin); anchor?: origin|center|centroid|top_left|top|top_right|left|right|bottom_left|bottom|bottom_right (the profile point placed on `at`: its origin, the default and a built-in profile's centre, its bounding box's centre, its centroid, or a corner or edge middle of its bounding box); angle?: radians, counter-clockwise; fixed?: bool (default true: the curves are locked, so solving the sketch keeps the shape); sketch?: id|name (default: the sketch being edited). Returns the section as frame.profile.properties does",
        created_doc!()
    )),
];

/// The `profile` parameter: its kind and each dimension as given, a number or an expression; or a
/// standard size by name, alone.
fn profile_spec(cmd: &str, p: &Value) -> Result<ProfileSpec> {
    let v = p.get("profile").ok_or_else(|| bad(cmd, "`profile` is required (frame.profile.list gives the kinds)"))?;
    if frame::size_with_dimensions(v) {
        return Err(bad(cmd, format!("`profile`: {}", frame::SIZE_ALONE)));
    }
    serde_json::from_value(v.clone()).map_err(|e| bad(cmd, format!("`profile`: {e}")))
}

/// The profile a spec makes in this design: a bare length in the design's units, a bare angle in
/// degrees, an expression over its parameters.
fn resolve(s: &Session, cmd: &str, spec: &ProfileSpec) -> Result<Profile> {
    let doc = &s.doc;
    spec.resolve(|name, e, kind| {
        doc.eval(&doc.with_design_unit(e, kind), kind).map_err(|err| solidcraft_doc::DocError::Invalid(format!("profile `{name}`: {err}")))
    })
    .map_err(|e| bad(cmd, e.to_string()))
}

/// The profile of `p`, worked out in this design, and the standard size it is, if it is one.
fn profile(s: &Session, cmd: &str, p: &Value) -> Result<(Profile, Option<String>)> {
    let spec = profile_spec(cmd, p)?;
    Ok((resolve(s, cmd, &spec)?, spec.size))
}

/// A frame command's reply, built from its schema's type so the two can't drift apart.
fn frame_reply(cmd: &str, r: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(r).map_err(|e| bad(cmd, e.to_string()))
}

fn list(_: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.profile.list";
    let only = match p.get("family") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_str()
                .and_then(|f| Family::ALL.into_iter().find(|x| x.prefix() == f))
                .ok_or_else(|| bad(cmd, "`family` must be RSC, PFC, RSA, SHS, RHS or CHS"))?,
        ),
    };
    let kinds = frame::profile_kinds().iter().map(ProfileKindInfo::from).collect();
    let all = sizes::standard_sizes();
    let sizes = Family::ALL
        .into_iter()
        .filter(|f| only.is_none_or(|o| o == *f))
        .filter_map(|family| {
            let members: Vec<&sizes::StandardSize> = all.iter().filter(|s| s.family == family).collect();
            let first = members.first()?;
            Some(SizeFamilyInfo {
                family: family.prefix().to_string(),
                kind: first.profile.kind().to_string(),
                standard: family.standard().to_string(),
                source: family.source().to_string(),
                names: members.iter().map(|s| s.name.clone()).collect(),
                additional: members.iter().filter(|s| s.additional).map(|s| s.name.clone()).collect(),
            })
        })
        .collect();
    frame_reply(cmd, ProfileListResult { kinds, sizes, param_warnings: Vec::new() })
}

/// The profile's section as the commands report it.
fn section(cmd: &str, profile: &Profile, size: Option<String>, density: Option<f64>) -> Result<ProfileSection> {
    let region = profile.region().map_err(|e| bad(cmd, e.to_string()))?;
    let s = frame::section(&region);
    Ok(ProfileSection {
        profile: ProfileOut(profile.filled()),
        size,
        area: s.area,
        centroid: s.centroid,
        ixx: s.ixx,
        iyy: s.iyy,
        ixy: s.ixy,
        width: s.width,
        height: s.height,
        // mm² × 1e-6 m²/mm² × kg/m³ = kg/m.
        mass_per_m: density.map(|rho| s.area * 1e-6 * rho),
    })
}

fn properties(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.profile.properties";
    let (profile, size) = profile(s, cmd, p)?;
    let density = match p.get("density") {
        None | Some(Value::Null) => None,
        Some(_) => Some(
            num(p, "density")
                .filter(|d| *d > 0.0 && *d <= 1e6)
                .ok_or_else(|| bad(cmd, "`density` must be a positive number of kg/m³ (at most 1e6)"))?,
        ),
    };
    frame_reply(cmd, ProfilePropertiesResult { section: section(cmd, &profile, size, density)?, param_warnings: Vec::new() })
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.profile.insert";
    let (profile, size) = profile(s, cmd, p)?;
    let at = match p.get("at") {
        None | Some(Value::Null) => Vec2::ZERO,
        Some(v) => vec2(v).ok_or_else(|| bad(cmd, "`at` must be [x, y] in sketch millimetres"))?,
    };
    let anchor: Anchor = match p.get("anchor") {
        None | Some(Value::Null) => Anchor::default(),
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| {
            bad(cmd, "`anchor` must be origin, center, centroid, top_left, top, top_right, left, right, bottom_left, bottom or bottom_right")
        })?,
    };
    let angle = match p.get("angle") {
        None | Some(Value::Null) => 0.0,
        Some(_) => num(p, "angle").filter(|a| a.abs() <= 1e6).ok_or_else(|| bad(cmd, "`angle` must be a number of radians"))?,
    };
    let fixed = match p.get("fixed") {
        None | Some(Value::Null) => true,
        Some(_) => bool_(p, "fixed").ok_or_else(|| bad(cmd, "`fixed` must be true or false"))?,
    };
    let region = profile.region().map_err(|e| bad(cmd, e.to_string()))?;
    let placed = frame::placed(&region, frame::anchor_point(std::slice::from_ref(&region), anchor), at, angle);
    let section = section(cmd, &profile, size, None)?;
    let (created, info) = edit(s, p, cmd, false, |sk, _| {
        let before = sk.clone();
        for l in std::iter::once(&placed.outer).chain(&placed.holes) {
            add_loop(sk, l, fixed, cmd)?;
        }
        serde_json::from_value::<CreatedIds>(super::sketch_created(Some(&before), sk)).map_err(|e| bad(cmd, e.to_string()))
    })?;
    let sketch = serde_json::from_value(info).map_err(|e| bad(cmd, e.to_string()))?;
    frame_reply(cmd, ProfileInsertResult { created, section, sketch, param_warnings: Vec::new() })
}

/// Draw the closed loop `l` (lines and arcs) into the sketch, each segment sharing its end
/// points with its neighbours; a loop of two half arcs of one circle becomes a circle.
fn add_loop(sk: &mut Sketch, l: &Loop2, fixed: bool, cmd: &str) -> Result<()> {
    let mut curves = Vec::new();
    if let [Seg2::Arc { center: c0, radius: r0, sweep: s0, .. }, Seg2::Arc { center: c1, radius: r1, sweep: s1, .. }] = l.segs.as_slice()
        && c0.dist(*c1) <= 1e-9
        && (r0 - r1).abs() <= 1e-9
        && ((s0 + s1).abs() - std::f64::consts::TAU).abs() <= 1e-9
    {
        curves.push(sk.add_circle(*c0, *r0, None, None)?);
    } else {
        let pts = l.segs.iter().map(|g| sk.add_point(g.start(), None)).collect::<std::result::Result<Vec<_>, _>>()?;
        let n = pts.len();
        for (i, g) in l.segs.iter().enumerate() {
            let (Some(&a), Some(&b)) = (pts.get(i), pts.get((i + 1) % n)) else {
                return Err(bad(cmd, "a profile segment is missing its end"));
            };
            let c = match *g {
                Seg2::Line { .. } => sk.add_line_pts(a, b, None)?,
                // The sketch's arcs run counter-clockwise: a clockwise one is drawn from its end.
                Seg2::Arc { center, sweep, .. } if sweep >= 0.0 => sk.add_arc(center, g.start(), g.end(), [None, Some(a), Some(b)], None)?,
                Seg2::Arc { center, .. } => sk.add_arc(center, g.end(), g.start(), [None, Some(b), Some(a)], None)?,
                _ => return Err(bad(cmd, "not supported yet: profiles with free-form edges")),
            };
            curves.push(c);
        }
    }
    if fixed {
        for c in curves {
            if let Some(cu) = sk.curves.get_mut(c) {
                cu.fixed = true;
            }
        }
    }
    Ok(())
}

fn member(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.member";
    let path_sketch = sketch_id(s, p.get("sketch"), cmd, "sketch")?;
    let lines = match p.get("lines") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) if !a.is_empty() && a.len() <= frame::MAX_MEMBERS && a.iter().all(Value::is_string) => {
            let lines = string_list(cmd, p, "lines")?;
            let mut seen = std::collections::HashSet::new();
            if let Some(twice) = lines.iter().find(|l| !seen.insert(l.as_str())) {
                return Err(bad(cmd, format!("line `{twice}` is listed twice")));
            }
            lines
        }
        Some(_) => Err(bad(cmd, format!("`lines` must list 1…{} curve ids", frame::MAX_MEMBERS)))?,
    };
    let body_name = match p.get("body_name") {
        None | Some(Value::Null) => None,
        Some(_) => Some(
            str_(p, "body_name")
                .map(str::trim)
                .filter(|n| !n.is_empty() && n.len() <= 128)
                .ok_or_else(|| bad(cmd, "`body_name` must be 1…128 characters"))?
                .to_string(),
        ),
    };
    let (profile, default_name) = match (p.get("profile"), p.get("profile_sketch")) {
        (Some(_), None) => {
            let spec = profile_spec(cmd, p)?;
            let pr = resolve(s, cmd, &spec)?;
            pr.region().map_err(|e| bad(cmd, e.to_string()))?;
            let name = spec.size.clone().unwrap_or_else(|| pr.description());
            (MemberProfile::Library { profile: spec }, name)
        }
        (None, Some(v)) => {
            let id = sketch_id(s, Some(v), cmd, "profile_sketch")?;
            if id == path_sketch {
                return Err(bad(cmd, "the profile sketch must not be the frame sketch"));
            }
            let name = s.doc.feature(id).map(|f| f.name.clone()).unwrap_or_else(|| "Member".into());
            (MemberProfile::Sketch { sketch: SketchId(id), profiles: super::feature_profiles(p, cmd)? }, name)
        }
        _ => return Err(bad(cmd, "give `profile` (a built-in profile) or `profile_sketch` (a sketch), one of them")),
    };
    let anchor: Anchor = match p.get("anchor") {
        None | Some(Value::Null) => Anchor::default(),
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| {
            bad(cmd, "`anchor` must be origin, center, centroid, top_left, top, top_right, left, right, bottom_left, bottom or bottom_right")
        })?,
    };
    let locate: Option<[f64; 2]> = match p.get("locate") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let a = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| bad(cmd, "`locate` must be [x, y] in the profile's own coordinates"))?;
            let x = a[0].as_f64().filter(|v| v.is_finite()).ok_or_else(|| bad(cmd, "`locate` x must be a finite length"))?;
            let y = a[1].as_f64().filter(|v| v.is_finite()).ok_or_else(|| bad(cmd, "`locate` y must be a finite length"))?;
            if x.abs() > frame::MAX_PROFILE_SIZE || y.abs() > frame::MAX_PROFILE_SIZE {
                return Err(bad(cmd, "`locate` is outside the largest profile"));
            }
            Some([x, y])
        }
    };
    let angle = match p.get("angle") {
        None | Some(Value::Null) => None,
        Some(_) => {
            let e = expr(p, "angle").ok_or_else(|| bad(cmd, "`angle` must be an angle expression"))?;
            check_expr(s, &e, solidcraft_doc::expr::Kind::Angle, cmd, "angle")?;
            Some(e)
        }
    };
    let mirror = match p.get("mirror") {
        None | Some(Value::Null) => false,
        Some(_) => bool_(p, "mirror").ok_or_else(|| bad(cmd, "`mirror` must be true or false"))?,
    };
    let kept: BTreeMap<String, String> = match p.get("names") {
        None | Some(Value::Null) => BTreeMap::new(),
        Some(Value::Object(m)) if m.len() <= frame::MAX_MEMBERS => m
            .iter()
            .map(|(line, v)| {
                let name = v.as_str().map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128);
                name.map(|n| (line.clone(), n.to_string()))
                    .ok_or_else(|| bad(cmd, format!("`names`: the name of line `{line}` must be 1…128 characters")))
            })
            .collect::<Result<_>>()?,
        Some(_) => return Err(bad(cmd, format!("`names` must map up to {} line ids to names", frame::MAX_MEMBERS))),
    };
    let base = body_name.clone().unwrap_or(default_name);
    let mut kind = FeatureKind::FrameMember {
        path_sketch: SketchId(path_sketch),
        lines,
        profile,
        anchor,
        locate,
        angle,
        mirror,
        body_name,
        names: BTreeMap::new(),
    };
    // Each member's name is pinned to its line as the feature is made, so deleting or adding a
    // line later leaves the other members' names alone.
    let st = s.model.state();
    let ids = frame::member_line_ids(&st, &kind).map_err(|e| bad(cmd, e.to_string()))?;
    let pinned: BTreeMap<String, String> = ids.iter().cloned().zip(frame::member_names(&st, &ids, &kept, &base)).collect();
    if let FeatureKind::FrameMember { names, .. } = &mut kind {
        *names = pinned;
    }
    // The feature names its own members (`body_name`, `names`), not by position.
    let mut p = p.clone();
    if let Some(o) = p.as_object_mut() {
        o.remove("body_name");
        o.remove("body_names");
        o.remove("names");
    }
    add_feature(s, &p, kind)
}

/// A member from a parameter: its body name, or `{feature, line}`. It is kept by its feature's
/// name and its line, so a trim follows the member when its body is rebuilt or renamed.
fn member_ref(s: &Session, v: &Value, cmd: &str) -> Result<MemberRef> {
    let feature_name = |id: u64| s.doc.feature(id).map(|f| f.name.clone());
    match v {
        Value::String(body) => {
            let st = s.model.state();
            let rec = st.frame_member(body.trim()).ok_or_else(|| bad(cmd, format!("`{}` is not a frame member's body", body.trim())))?;
            let feature = feature_name(rec.feature).ok_or_else(|| bad(cmd, format!("the feature of `{}` is gone", body.trim())))?;
            Ok(MemberRef { feature: feature.into(), line: rec.line.clone() })
        }
        Value::Object(o) => {
            let f = match o.get("feature") {
                Some(Value::String(n)) => s.doc.find_feature(n.trim()),
                Some(Value::Number(n)) => n.as_u64().and_then(|id| s.doc.feature(id)),
                _ => None,
            }
            .ok_or_else(|| bad(cmd, "a member's `feature` must name a frame member feature (name or id)"))?;
            if !matches!(f.kind, FeatureKind::FrameMember { .. }) {
                return Err(bad(cmd, format!("`{}` is not a frame member feature", f.name)));
            }
            let line = o
                .get("line")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|l| !l.is_empty() && l.len() <= 128)
                .ok_or_else(|| bad(cmd, "a member's `line` must be its line's id"))?;
            Ok(MemberRef { feature: f.name.clone().into(), line: line.to_string() })
        }
        _ => Err(bad(cmd, "a member is its body's name or {feature, line}")),
    }
}

/// A trim target from a parameter: a member (body name, `{member, side?}` or `{feature, line}`)
/// or `{plane}`, in the active component's frame. A body face is attached, as a sketch on it is,
/// and kept in `faces` too.
fn trim_target(s: &Session, v: &Value, cmd: &str, faces: &mut Vec<solidcraft_doc::PlaneRef>) -> Result<TrimTarget> {
    match v {
        // A plane as the design saves it, as Edit Feature passes a trim's planes back.
        Value::Object(o) if o.contains_key("plane_ref") => {
            let plane =
                serde_json::from_value(o.get("plane_ref").cloned().unwrap_or(Value::Null)).map_err(|e| bad(cmd, format!("`plane_ref`: {e}")))?;
            Ok(TrimTarget::Plane { plane })
        }
        Value::Object(o) if o.contains_key("plane") => {
            let pv = o.get("plane");
            let plane = match pv {
                // Captured in the authoring frame already.
                Some(face @ Value::Object(po)) if po.contains_key("face") => {
                    let plane = super::sketch::face::capture(s, face, cmd)?;
                    faces.push(plane.clone());
                    plane
                }
                _ => {
                    let mut plane = super::features::plane_param(s, pv, cmd)?;
                    super::component::plane_to_active_frame(s, &mut plane);
                    plane
                }
            };
            Ok(TrimTarget::Plane { plane })
        }
        Value::Object(o) if o.contains_key("member") => {
            let member = member_ref(s, o.get("member").unwrap_or(&Value::Null), cmd)?;
            let side: Option<Side> = match o.get("side") {
                None | Some(Value::Null) => None,
                Some(v) => Some(serde_json::from_value(v.clone()).map_err(|_| bad(cmd, "`side` must be right, left, top or bottom"))?),
            };
            Ok(TrimTarget::Member { member, side })
        }
        _ => Ok(TrimTarget::Member { member: member_ref(s, v, cmd)?, side: None }),
    }
}

fn trim(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.trim";
    let members = match p.get("members") {
        Some(Value::Array(a)) if !a.is_empty() && a.len() <= frame::MAX_MEMBERS => {
            a.iter().map(|v| member_ref(s, v, cmd)).collect::<Result<Vec<_>>>()?
        }
        Some(v @ Value::String(_)) => vec![member_ref(s, v, cmd)?],
        _ => return Err(bad(cmd, format!("`members` must list 1…{} members (body names or {{feature, line}})", frame::MAX_MEMBERS))),
    };
    let mut seen = std::collections::HashSet::new();
    if let Some(twice) = members.iter().find(|m| !seen.insert(*m)) {
        return Err(bad(cmd, format!("member `{}` of `{}` is listed twice", twice.line, twice.feature)));
    }
    let mut faces = Vec::new();
    let to = match p.get("to") {
        Some(Value::Array(a)) if !a.is_empty() && a.len() <= frame::MAX_TRIM_TARGETS => {
            a.iter().map(|v| trim_target(s, v, cmd, &mut faces)).collect::<Result<Vec<_>>>()?
        }
        Some(v @ (Value::String(_) | Value::Object(_))) => vec![trim_target(s, v, cmd, &mut faces)?],
        _ => return Err(bad(cmd, format!("`to` must list 1…{} targets (members or {{plane}})", frame::MAX_TRIM_TARGETS))),
    };
    let end: TrimEnd = match p.get("end") {
        None | Some(Value::Null) => TrimEnd::Auto,
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| bad(cmd, "`end` must be auto, start or end"))?,
    };
    let gap = gap(s, p, cmd)?;
    // A face's attachment holds the placements it was picked under, as a sketch's does.
    for face in &faces {
        super::sketch::face::capture_positions(s, face)?;
    }
    // Every target is in the active component's frame already.
    super::features::add_feature_in(s, p, FeatureKind::FrameTrim { members, to, end, gap }, false)
}

/// The weld `gap` parameter of a trim or a corner: none, or a length expression of 0…1 m.
fn gap(s: &Session, p: &Value, cmd: &str) -> Result<Option<solidcraft_doc::Expr>> {
    match p.get("gap") {
        None | Some(Value::Null) => Ok(None),
        Some(_) => {
            let e = expr(p, "gap").ok_or_else(|| bad(cmd, "`gap` must be a length expression"))?;
            let g = s.doc.eval(&s.doc.with_design_unit(&e, Kind::Length), Kind::Length).map_err(|err| bad(cmd, format!("gap: {err}")))?;
            if !(0.0..=frame::MAX_TRIM_GAP).contains(&g) {
                return Err(bad(cmd, format!("`gap` must be 0…{} mm", frame::MAX_TRIM_GAP)));
            }
            Ok(Some(e))
        }
    }
}

fn corner(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.corner";
    let members = match p.get("members") {
        Some(Value::Array(a)) if a.len() == 2 => a.iter().map(|v| member_ref(s, v, cmd)).collect::<Result<Vec<_>>>()?,
        _ => return Err(bad(cmd, "`members` must list the two members that meet (body names or {feature, line})")),
    };
    if members.first() == members.get(1) {
        return Err(bad(cmd, "`members` lists the same member twice: a corner needs two"));
    }
    let kind: CornerKind = match p.get("kind") {
        None | Some(Value::Null) => CornerKind::Mitre,
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| bad(cmd, "`kind` must be mitre"))?,
    };
    let gap = gap(s, p, cmd)?;
    add_feature(s, p, FeatureKind::FrameCorner { members, kind, gap })
}

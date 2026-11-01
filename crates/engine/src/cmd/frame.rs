//! Frames and weldments (#4003). `frame.member` lays structural members along the lines of a frame
//! sketch, their cross-section a built-in profile or a profile sketch. `frame.profile.list` lists
//! the built-in profiles, `frame.profile.properties` works out a profile's section, and
//! `frame.profile.insert` draws one into a sketch.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcraft_doc::expr::Kind;
use solidcraft_doc::frame::{self, Anchor, MemberProfile, Profile, ProfileSpec};
use solidcraft_doc::{FeatureKind, SketchId};
use solidcraft_geom::{Loop2, Seg2, Vec2};
use solidcraft_sketch::Sketch;

use super::features::{add_feature, check_expr, sketch_id};
use super::sketch::edit;
use super::{CommandSpec, in_sketch, not_in_sketch};
use crate::params::{bad, bool_, expr, num, str_, string_list, vec2};
use crate::schema::features::FeatureResult;
use crate::schema::frame::MemberParams;
use crate::schema::frame::{CopeDxfParams, CopeDxfResult, CopeParams};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const MEMBER_CONTRACT: TypedContract<MemberParams> = TypedContract::new();
const COPE_CONTRACT: TypedContract<CopeParams> = TypedContract::new();
const COPE_DXF_CONTRACT: TypedContract<CopeDxfParams> = TypedContract::new();

#[path = "frame_cope.rs"]
mod cope;

#[cfg(test)]
#[path = "frame_tests.rs"]
mod tests;

/// The profile parameter, as every frame command documents it.
macro_rules! profile_doc {
    () => {
        "profile: {kind: bar|round_bar|rect_tube|round_tube|angle|channel|i_beam, …its dimensions: each a number in the design's units or a length expression over its parameters (\"tube_wall\", \"2 in\")} (frame.profile.list gives each kind's dimensions and an example; dimensions left out take defaults from the profile's size)"
    };
}

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("frame.cope", "Cope Round Tube", crate::checked!("frame.cope", cope::run))
        .icon("frame_cope").enabled(not_in_sketch).contract(&COPE_CONTRACT).output(output_schema_for::<FeatureResult>)
        .params("member: round-tube frame member; tools: 1..16 distinct untreated straight round-tube members in the same component; end: start|end (original path). Retained cut to mating outside cylinders, filling their bores for the cut. Axes must meet the endpoint, mate OD >= member OD; joint sine >= 0.001. Refuses cap/bend interference or other body edits. name?: feature name. Edit with timeline.redefine; undoable; follows upstream edits and body renames."),
    CommandSpec::new("frame.export_cope_dxf", "Export Cope Template as DXF", crate::checked!("frame.export_cope_dxf", cope::export))
        .icon("dxf").noundo().enabled(not_in_sketch).contract(&COPE_DXF_CONTRACT).output(output_schema_for::<CopeDxfResult>)
        .params("member: retained round-tube member; end: start|end; seam?: angle expression (default 0, relative to terminal x); tolerance?: 0.001..1 mm (default 0.01, bounded flat/wrapped interpolation error); path?: DXF destination (atomic scoped write, otherwise dxf text). 1:1 mm, outer circumference; positive station into surviving stock from original endpoint; right-hand wrap about that inward axis. Open profile with both seam copies; datum, member/end labels and 100 mm scale check. At most 65536 segments; increase tolerance if refused. Returns physical datum, profile and drawing metadata. Does not alter the design."),
    CommandSpec::new("frame.member", "Frame Member", crate::checked!("frame.member", member)).at("WELDMENTS", "CREATE").icon("frame_member").enabled(not_in_sketch).contract(&MEMBER_CONTRACT).output(output_schema_for::<FeatureResult>).params(concat!(
        "sketch: the frame sketch (id or name; 2D or 3D); lines?: [line or circular arc ids] (one member each; default: every straight line drawn in the sketch: not construction, not projected or otherwise linked, not of no length); continuous?: bool (default false; true: one member along explicit lines in connected tangent path order, reversing individual curves as needed; open paths only; its name uses the stable names key `$path`); ",
        profile_doc!(),
        " | profile_sketch: a sketch whose profile is the section, in its own x and y around its origin, with profiles?: picks as solid.extrude takes them (default: its material, nested loops alternating, so two circles make a tube); anchor?: origin|center|centroid|top_left|top|top_right|left|right|bottom_left|bottom|bottom_right (the profile point that sits on each line, default origin); angle?: angle expr (the profile turned about the line; default 0: its y as near world up as the line allows, a vertical member's x along world X); mirror?: bool (the profile mirrored across its y axis); name?; body_name? (what the members are named after, then numbered: `Rail`, `Rail (2)`…; default the profile's description, `Rect Tube 40x40x3`, or the profile sketch's name); names?: {line id: name} (names to keep, as Edit Feature keeps them). One body per selected curve, or one body for a continuous path; each member keeps its name when other members are added or deleted. Returns the feature and the bodies it made"
    )),
    CommandSpec::new("frame.profile.list", "Profile Library", list).icon("library")
        .noundo()
        .doc_only()
        .params("Returns kinds: [{kind, label, required, optional, example}]: the built-in profiles, each SolidCraft's own parametric definition"),
    CommandSpec::new("frame.profile.properties", "Profile Properties", properties).icon("measure").noundo().doc_only().params(concat!(
        profile_doc!(),
        "; density?: kg/m³ (adds mass_per_m, kg/m). Returns profile (every default filled in), area (mm²), centroid [x,y] (mm, from the bounding box centre), ixx, iyy, ixy (mm⁴ about the centroid), width, height"
    )),
    CommandSpec::new("frame.profile.insert", "Insert Profile", insert).icon("import").enabled(in_sketch).params(concat!(
        profile_doc!(),
        "; at?: [x,y] (sketch mm, default the origin); anchor?: origin|center|centroid|top_left|top|top_right|left|right|bottom_left|bottom|bottom_right (the profile point placed on `at`: its origin, the default and a built-in profile's centre, its bounding box's centre, its centroid, or a corner or edge middle of its bounding box); angle?: radians, counter-clockwise; fixed?: bool (default true: the curves are locked, so solving the sketch keeps the shape); sketch?: id|name (default: the sketch being edited). Returns the section as frame.profile.properties does",
        created_doc!()
    )),
];

/// The `profile` parameter: its kind and each dimension as given, a number or an expression.
fn profile_spec(cmd: &str, p: &Value) -> Result<ProfileSpec> {
    let v = p.get("profile").ok_or_else(|| bad(cmd, "`profile` is required (frame.profile.list gives the kinds)"))?;
    serde_json::from_value(v.clone()).map_err(|e| bad(cmd, format!("`profile`: {e}")))
}

/// The profile a spec makes in this design: a bare number in the design's units, an expression
/// over its parameters.
fn resolve(s: &Session, cmd: &str, spec: &ProfileSpec) -> Result<Profile> {
    let doc = &s.doc;
    spec.resolve(|name, e| {
        doc.eval(&doc.with_design_unit(e, Kind::Length), Kind::Length)
            .map_err(|err| solidcraft_doc::DocError::Invalid(format!("profile `{name}`: {err}")))
    })
    .map_err(|e| bad(cmd, e.to_string()))
}

/// The profile of `p`, worked out in this design.
fn profile(s: &Session, cmd: &str, p: &Value) -> Result<Profile> {
    resolve(s, cmd, &profile_spec(cmd, p)?)
}

fn list(_: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!({"kinds": frame::profile_kinds()}))
}

/// The profile's section as the commands report it.
fn section_json(cmd: &str, profile: &Profile, density: Option<f64>) -> Result<Value> {
    let region = profile.region().map_err(|e| bad(cmd, e.to_string()))?;
    let s = frame::section(&region);
    let mut out = json!({
        "profile": profile.filled(),
        "area": s.area,
        "centroid": s.centroid,
        "ixx": s.ixx,
        "iyy": s.iyy,
        "ixy": s.ixy,
        "width": s.width,
        "height": s.height,
    });
    if let (Some(rho), Some(o)) = (density, out.as_object_mut()) {
        // mm² × 1e-6 m²/mm² × kg/m³ = kg/m.
        o.insert("mass_per_m".into(), json!(s.area * 1e-6 * rho));
    }
    Ok(out)
}

fn properties(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.profile.properties";
    let profile = profile(s, cmd, p)?;
    let density = match p.get("density") {
        None | Some(Value::Null) => None,
        Some(_) => Some(
            num(p, "density")
                .filter(|d| *d > 0.0 && *d <= 1e6)
                .ok_or_else(|| bad(cmd, "`density` must be a positive number of kg/m³ (at most 1e6)"))?,
        ),
    };
    section_json(cmd, &profile, density)
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.profile.insert";
    let profile = profile(s, cmd, p)?;
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
    let section = section_json(cmd, &profile, None)?;
    let (created, info) = edit(s, p, cmd, false, |sk, _| {
        let before = sk.clone();
        for l in std::iter::once(&placed.outer).chain(&placed.holes) {
            add_loop(sk, l, fixed, cmd)?;
        }
        Ok(super::sketch_created(Some(&before), sk))
    })?;
    Ok(json!({"created": created, "section": section, "sketch": info}))
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
    let continuous = match p.get("continuous") {
        None | Some(Value::Null) => false,
        Some(_) => bool_(p, "continuous").ok_or_else(|| bad(cmd, "`continuous` must be true or false"))?,
    };
    if continuous && lines.is_empty() {
        return Err(bad(cmd, "a continuous member needs explicit `lines` in path order"));
    }
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
            (MemberProfile::Library { profile: spec }, pr.description())
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
        continuous,
        profile,
        anchor,
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

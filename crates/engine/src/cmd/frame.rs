//! Frames and weldments (#4003). `frame.member` lays structural members along the lines of a frame
//! sketch and between pairs of points (#15705), their cross-section a built-in profile, a
//! standard size or a profile sketch. `frame.profile.list` lists the built-in profiles and the
//! standard sizes, `frame.profile.properties` works out a profile's section, and
//! `frame.profile.insert` draws one into a sketch.
//! `frame.remove_member` takes tubes out of a Frame Member (#15705), and `frame.multi_trim` sets, changes or removes
//! one of its corner overrides (#17854).

use solidcraft_geom::prelude::*;
use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcore_record::{Loop2, Seg2, Vec2};
use solidcraft_doc::construct::{DatumItem, GeoRef};
use solidcraft_doc::frame::sizes::{self, Family};
use solidcraft_doc::frame::{self, Anchor, CornerOverride, CornerType, MemberPair, MemberProfile, MemberRef, Profile, ProfileSpec};
use solidcraft_doc::{Feature, FeatureKind, ModelState, SketchId};
use solidcraft_sketch::Sketch;

use super::features::{add_feature, check_expr, sketch_id};
use super::sketch::edit;
use super::{CommandSpec, in_sketch, not_in_sketch};
use crate::params::{bad, bool_, expr, num, str_, string_list, vec2};
use crate::schema::features::FeatureResult;
use crate::schema::frame::{
    CORNER_ENTRY, CreatedIds, FrameMemberParams, MemberFeatureDeleted, MultiTrimParams, ProfileInsertParams, ProfileInsertResult, ProfileKindInfo,
    ProfileListParams, ProfileListResult, ProfileOut, ProfilePropertiesParams, ProfilePropertiesResult, ProfileSection, RemoveMemberParams,
    RemoveMemberResult, SizeFamilyInfo, TRIM_KIND, corner_entry, removed_members, trim_kind, trim_members,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

#[cfg(test)]
#[path = "frame_tests.rs"]
mod tests;

const MEMBER_CONTRACT: TypedContract<FrameMemberParams> = TypedContract::new();
const LIST_CONTRACT: TypedContract<ProfileListParams> = TypedContract::new();
const PROPERTIES_CONTRACT: TypedContract<ProfilePropertiesParams> = TypedContract::new();
const INSERT_CONTRACT: TypedContract<ProfileInsertParams> = TypedContract::new();
const REMOVE_CONTRACT: TypedContract<RemoveMemberParams> = TypedContract::new();
const MULTI_TRIM_CONTRACT: TypedContract<MultiTrimParams> = TypedContract::new();

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
        "sketch?: the frame sketch (id or name; 2D or 3D); lines?: [curve ids] (one member each; default: every straight line drawn in the sketch: not construction, not projected or otherwise linked, not of no length); pairs?: [{a, b}] (one straight member from point a to point b each, after the lines' members, found again whenever the design changes; a and b are point references: {vertex: [x,y,z]}, {sketch, point} (a 3D sketch node as `wire:index`), {construct_point: name}, a snap point or the origin, as saved ({type, …}) too; typed coordinates are not points here; each pair is named pair1, pair2… and a pair's id is never given again; Edit Feature gives back the pairs it keeps with their `id` and `next_pair`, and an `id` without `next_pair` is refused); give `sketch`, `pairs` or both; ",
        profile_doc!(),
        " | profile_sketch: a sketch whose profile is the section, in its own x and y around its origin, with profiles?: picks as solid.extrude takes them (default: its material, nested loops alternating, so two circles make a tube); anchor?: origin|center|centroid|top_left|top|top_right|left|right|bottom_left|bottom|bottom_right (the profile point that sits on each line, default origin); locate?: [x,y] (the profile point that sits on each line, in mm from the profile's own origin: a bar's or an angle's bottom-left corner, a channel's web back at mid-height, the centre of other profiles; takes precedence over `anchor` and keeps the member attached at that point when the profile size changes); corner?: auto|none|mitre|butt (how members ending at one point are cut; default none); flip?: bool (a butt's other member runs through); fit_ends?: bool (fit free ends to earlier Frame Members' tubes; default false); coped?: bool (cut every butt joint to the other tube's real shape, radii kept: butts, ends on a tube's middle, crossings, overlaps and fitted ends, mitres left as they are; not under none; default false; an older `coped_butt`, the frame's or an override's, reads as butt with coped); corners?: [{members: [2 to 1000 line or pair ids, or {line}], kind: auto|none|mitre|butt, gap?: length expr}] (that corner's, crossing's or overlap's own type, and its own weld gap; the members in order: the first runs through a butt, or stays whole at a crossing or overlap, and of three or more the first two make the corner; an older script's `corner` reads as `kind`, and its `flip` is saved as the order today's rules give with the first two swapped; an overlap takes no gap); gap?: length expr (the weld gap at every welded end, 0…1000 mm, default 0: each mitre plane half of it back, a butting or fitted end, a crossing's pieces' included, that far off the member it meets; through members keep their length, overlaps are notched without it); angle?: angle expr (the profile turned about the line; default 0: its y as near world up as the line allows, a vertical member's x along world X); mirror?: bool (the profile mirrored across its y axis); name?; body_name? (what the members are named after, then numbered: `Rail`, `Rail (2)`…; default the size's name, `RSC 203x89`, the profile's description, `Rect Tube 40x40x3`, or the profile sketch's name); names?: {line or pair id: name} (names to keep, as Edit Feature keeps them). One body per line or pair, from its start to its end; each member keeps its name when other lines or pairs are added or deleted. Returns the feature and the bodies it made"
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
    // Reached from menus and scripts: no ribbon button.
    CommandSpec::new("frame.remove_member", "Remove Tube", crate::checked!("frame.remove_member", remove_member))
        .icon("delete")
        .enabled(not_in_sketch)
        .contract(&REMOVE_CONTRACT)
        .output(output_schema_for::<RemoveMemberResult>)
        .params("members: [{feature: id|name, line: the tube's line or pair id}] (1…1000, all of one Frame Member). Takes those tubes out as one undo step: a pair goes from the feature's pairs, a sketch line from its lines (the line stays in its sketch; a feature on every line of its sketch lists the others from then on), and the corner overrides that name a tube taken out go too. Taking out the last tube deletes the Frame Member. Returns the edited feature as frame.member does, or {deleted: the feature's name}"),
    // The dialog's Corner Overrides rows are its UI: no ribbon button.
    CommandSpec::new("frame.multi_trim", "Multi-Trim Corner", crate::checked!("frame.multi_trim", multi_trim))
        .icon("frame_member")
        .enabled(not_in_sketch)
        .contract(&MULTI_TRIM_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("feature: the Frame Member (id or name); members: [2 to 1000 line or pair ids, or {feature?: the frame's own name, line}] (a corner, crossing or overlap of the frame, the members in order: the first runs through a butt, or stays whole at a crossing or overlap, and of three or more the first two make the corner); kind?: auto|none|mitre|butt (default auto: the frame's own type, with this order); flip?: bool (the first two in the order today's rules give them, swapped; without it the members are saved as given); gap?: length expr (this corner's weld gap, 0…1000 mm; default the frame's; an overlap takes none); remove?: bool (take the override of these members away, back to the frame's own rules; no kind, flip or gap with it). Replaces the override of the same members (any order) or adds one, as one undo step; the frame stays one timeline entry. Returns the edited feature as frame.member does"),
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

/// `pairs[i].{a|b}` is a point: a vertex, a sketch point (or 3D sketch node), a construction
/// point, a snap point, the origin or a component's O. Typed coordinates are not.
fn is_point(g: &GeoRef) -> bool {
    match g {
        GeoRef::Origin | GeoRef::Vertex { .. } | GeoRef::ConstructPoint { .. } | GeoRef::SketchPoint { .. } | GeoRef::Snap { .. } => true,
        GeoRef::Datum { datum } => datum.item == DatumItem::O,
        _ => false,
    }
}

/// `pairs` and `next_pair`: each pair `{id?, a, b}`, its ends point references as `construct`
/// reads them. A given id is kept when it is a `pair<N>` given once, below `next_pair` (Edit
/// Feature gives back the pairs it keeps, and the feature's `next_pair`); without `next_pair`, a
/// new Frame Member, ids are the command's to give. The others take `pair<next>` on from the
/// highest, so no id is given twice. Returns the pairs and the next pair's number.
fn member_pairs(s: &Session, p: &Value, cmd: &str) -> Result<(Vec<MemberPair>, u32)> {
    let given_next = match p.get("next_pair") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| bad(cmd, "`next_pair` must be a whole number (the number the next new pair's id takes)"))?,
        ),
    };
    let list = match p.get("pairs") {
        None | Some(Value::Null) => return Ok((Vec::new(), given_next.unwrap_or(0))),
        Some(Value::Array(a)) if !a.is_empty() && a.len() <= frame::MAX_MEMBERS => a,
        Some(_) => return Err(bad(cmd, format!("`pairs` must list one pair of points or more, at most {}: [{{a, b}}]", frame::MAX_MEMBERS))),
    };
    let mut read = Vec::with_capacity(list.len());
    let mut taken = std::collections::BTreeSet::new();
    for (i, v) in list.iter().enumerate() {
        let o = v.as_object().ok_or_else(|| bad(cmd, format!("`pairs[{i}]` must be {{a, b}}: two point references")))?;
        let id = match o.get("id") {
            None | Some(Value::Null) => None,
            Some(id) => {
                let id = id.as_str().unwrap_or_default();
                let Some(next) = given_next else {
                    return Err(bad(cmd, format!("`pairs[{i}].id` `{id}`: ids are given by the command, not the caller")));
                };
                let n = frame::parse_pair_id(id).filter(|n| *n < next).ok_or_else(|| {
                    bad(cmd, format!("`pairs[{i}].id` `{id}` is not one of the feature's pair ids (pair1, pair2… below `next_pair`)"))
                })?;
                if !taken.insert(n) {
                    return Err(bad(cmd, format!("pair id `{id}` is given twice")));
                }
                Some(id.to_string())
            }
        };
        let end = |key: &str| -> Result<GeoRef> {
            let what = format!("pairs[{i}].{key}");
            let g = super::construct::geo(s, o.get(key), cmd, &what)?;
            if is_point(&g) {
                Ok(g)
            } else {
                Err(bad(cmd, format!("`{what}` must be a point: a vertex, a sketch point, a construction point or a snap point")))
            }
        };
        read.push((id, end("a")?, end("b")?));
    }
    let too_many = || bad(cmd, "pair ids have run out of numbers");
    let past_highest = match taken.last() {
        Some(n) => n.checked_add(1).ok_or_else(too_many)?,
        None => 1,
    };
    let mut next = given_next.unwrap_or(0).max(past_highest);
    let mut pairs = Vec::with_capacity(read.len());
    for (id, a, b) in read {
        let id = match id {
            Some(id) => id,
            None => {
                let id = frame::pair_id(next);
                next = next.checked_add(1).ok_or_else(too_many)?;
                id
            }
        };
        pairs.push(MemberPair { id, a, b });
    }
    Ok((pairs, next))
}

fn member(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.member";
    // A `sketch` that is there is read, `null` included, as before pairs.
    let path_sketch = match p.get("sketch") {
        None => None,
        Some(v) => Some(sketch_id(s, Some(v), cmd, "sketch")?),
    };
    let (pairs, next_pair) = member_pairs(s, p, cmd)?;
    if path_sketch.is_none() && pairs.is_empty() {
        return Err(bad(cmd, "give `sketch` (the frame sketch, its lines), `pairs` (pairs of points), or both"));
    }
    if path_sketch.is_none() && !matches!(p.get("lines"), None | Some(Value::Null)) {
        return Err(bad(cmd, "`lines` are the lines of `sketch`: give the sketch"));
    }
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
                .filter(|n| !n.is_empty() && n.chars().count() <= 128)
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
            if Some(id) == path_sketch {
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
    let mut corner: CornerType = match p.get("corner") {
        None | Some(Value::Null) => CornerType::None,
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| bad(cmd, "`corner` must be auto, none, mitre or butt"))?,
    };
    let fit_ends = match p.get("fit_ends") {
        None | Some(Value::Null) => false,
        Some(_) => bool_(p, "fit_ends").ok_or_else(|| bad(cmd, "`fit_ends` must be true or false"))?,
    };
    let mut coped = match p.get("coped") {
        None | Some(Value::Null) => false,
        Some(_) => bool_(p, "coped").ok_or_else(|| bad(cmd, "`coped` must be true or false"))?,
    };
    let flip = match p.get("flip") {
        None | Some(Value::Null) => false,
        Some(_) => bool_(p, "flip").ok_or_else(|| bad(cmd, "`flip` must be true or false"))?,
    };
    // Each entry, and its Flip where it gives one (saved below as an order).
    let (mut corners, flips): (Vec<CornerOverride>, Vec<Option<bool>>) = match p.get("corners") {
        None | Some(Value::Null) => (Vec::new(), Vec::new()),
        Some(Value::Array(a)) if a.len() <= frame::MAX_MEMBERS => {
            a.iter().map(|v| corner_entry(v).ok_or_else(|| bad(cmd, CORNER_ENTRY))).collect::<Result<Vec<_>>>()?.into_iter().unzip()
        }
        Some(_) => return Err(bad(cmd, format!("`corners` must be a list of up to {} corner overrides", frame::MAX_MEMBERS))),
    };
    // A new frame has no name for its own members to name: any feature is another frame's.
    if corners.iter().flat_map(|c| &c.members).any(|m| m.feature.is_some()) {
        return Err(bad(cmd, "`corners`: not supported yet: a corner with a member of another frame"));
    }
    for gap in corners.iter_mut().filter_map(|c| c.gap.as_mut()) {
        *gap = design_gap(s, gap, cmd, "`corners` gap")?.into();
    }
    // An older script's Coped corner type is Butt with Coped (#17854).
    frame::read_coped(&mut corner, &mut corners, &mut coped);
    let angle = match p.get("angle") {
        None | Some(Value::Null) => None,
        Some(_) => {
            let e = expr(p, "angle").ok_or_else(|| bad(cmd, "`angle` must be an angle expression"))?;
            check_expr(s, &e, solidcraft_doc::expr::Kind::Angle, cmd, "angle")?;
            Some(e)
        }
    };
    let gap = match p.get("gap") {
        None | Some(Value::Null) => None,
        Some(_) => {
            let e = expr(p, "gap").ok_or_else(|| bad(cmd, "`gap` must be a length expression"))?;
            Some(design_gap(s, &e, cmd, "`gap`")?.into())
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
                let name = v.as_str().map(str::trim).filter(|n| !n.is_empty() && n.chars().count() <= 128);
                name.map(|n| (line.clone(), n.to_string()))
                    .ok_or_else(|| bad(cmd, format!("`names`: the name of line `{line}` must be 1…128 characters")))
            })
            .collect::<Result<_>>()?,
        Some(_) => return Err(bad(cmd, format!("`names` must map up to {} line ids to names", frame::MAX_MEMBERS))),
    };
    let base = body_name.clone().unwrap_or(default_name);
    let mut kind = FeatureKind::FrameMember {
        path_sketch: path_sketch.map(SketchId),
        lines,
        profile,
        anchor,
        locate,
        corner,
        flip,
        fit_ends,
        coped,
        corners,
        gap,
        angle,
        mirror,
        body_name,
        names: BTreeMap::new(),
        pairs,
        next_pair,
    };
    let st = s.model.state();
    if flips.iter().any(Option::is_some) {
        // The feature as it will be added: in the active component, whose frame it is laid out in.
        let mut laid = kind.clone();
        super::component::to_active_frame(s, &mut laid);
        let mut v = serde_json::to_value(&laid).map_err(|e| bad(cmd, e.to_string()))?;
        if let Some(o) = v.as_object_mut() {
            o.extend([("id".to_string(), json!(0)), ("name".to_string(), json!("")), ("component".to_string(), json!(s.active_component))]);
        }
        let mut f: Feature = serde_json::from_value(v).map_err(|e| bad(cmd, e.to_string()))?;
        flips_as_order(&s.doc, &st, &mut f, &flips);
        if let (FeatureKind::FrameMember { corners: ordered, .. }, FeatureKind::FrameMember { corners, .. }) = (f.kind, &mut kind) {
            *corners = ordered;
        }
    }
    // Each member's name is pinned to its line as the feature is made, so deleting or adding a
    // line later leaves the other members' names alone.
    let (ids, later) = frame::member_parts(&st, &kind).map_err(|e| bad(cmd, e.to_string()))?;
    if let FeatureKind::FrameMember { corners, path_sketch, .. } = &kind {
        let mut pairs = std::collections::HashSet::new();
        for c in corners {
            let lines: Vec<String> = c.lines().map(str::to_string).collect();
            no_piece(&lines, &ids, &later).map_err(|e| bad(cmd, format!("`corners`: {e}")))?;
            let mut sorted: Vec<&str> = c.lines().collect();
            sorted.sort();
            sorted.dedup();
            let n = lines.len();
            // A crossing's override names every line through it, under every type that cuts crossings.
            let crossing = || path_sketch.is_some_and(|ps| frame::members_cross(&st, ps.0, &lines));
            if n > 2 && corner != CornerType::Auto && !crossing() {
                return Err(bad(cmd, "`corners`: an override of three or more members needs `corner` auto"));
            }
            if !(2..=frame::MAX_MEMBERS).contains(&n) || sorted.len() != n || !sorted.iter().all(|m| ids.iter().any(|id| id == m)) {
                return Err(bad(
                    cmd,
                    format!("`corners`: each corner needs 2 to {} different members (lines or pairs) of the frame: {:?}", frame::MAX_MEMBERS, lines),
                ));
            }
            if !pairs.insert(sorted) {
                return Err(bad(cmd, format!("`corners`: the corner of {lines:?} is listed twice")));
            }
        }
    }
    let mut pinned: BTreeMap<String, String> = ids.iter().cloned().zip(frame::member_names(&st, &ids, &later, &kept, &base)).collect();
    // A piece split off where another tube overlaps it (`L>X`) is found only as the frame is built: a name given or
    // kept for it stays pinned under its id.
    pinned.extend(kept.into_iter().filter(|(id, _)| frame::overlap_parent(&ids, id).is_some()));
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

/// Refused when one of an override's `lines` is a piece of a line cut where tubes cross (`ids`, `later`: the frame's
/// [`frame::member_parts`]): an override names a corner by its lines and pairs. A piece is told by where it is, not by
/// its id: a sketch id may hold `>` itself.
fn no_piece(lines: &[String], ids: &[String], later: &frame::LaterPieces) -> std::result::Result<(), String> {
    let piece = |m: &&String| ids.iter().zip(later).any(|(id, l)| l.is_some() && id == *m);
    match lines.iter().find(piece) {
        Some(piece) => Err(format!("`{piece}` is a piece of a tube cut where tubes cross: an override names the lines and pairs that meet")),
        None => Ok(()),
    }
}

/// A weld gap typed as `e`, as it is saved: a bare length means the design's unit (`0.1` is `0.1 in` in an inch
/// design, as a new feature's input is), then checked within 0…[`frame::MAX_GAP`] mm, `what` naming it.
fn design_gap(s: &Session, e: &str, cmd: &str, what: &str) -> Result<String> {
    let e = s.doc.with_design_unit(e, solidcraft_doc::expr::Kind::Length);
    let g = s.doc.eval(&e, solidcraft_doc::expr::Kind::Length).map_err(|err| bad(cmd, format!("{what}: {err}")))?;
    frame::checked_gap(g, what).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(e)
}

/// Each corner of the Frame Member `f` whose entry gives a Flip (`flips`, by entry) saved as an order: the order
/// today's rules give its members ([`frame::default_orders`], the frame laid out once in `st`, the model before it),
/// the first two swapped when set. One that names no corner keeps its order: the checks after, or the build, say so.
pub(super) fn flips_as_order(doc: &solidcraft_doc::Document, st: &ModelState, f: &mut Feature, flips: &[Option<bool>]) {
    let FeatureKind::FrameMember { corners, .. } = &f.kind else { return };
    let flipped: Vec<(usize, bool)> = flips.iter().enumerate().filter_map(|(k, flip)| Some((k, (*flip)?))).collect();
    let sets: Vec<Vec<String>> =
        flipped.iter().map(|(k, _)| corners.get(*k).map(|c| c.lines().map(str::to_string).collect()).unwrap_or_default()).collect();
    if sets.is_empty() {
        return;
    }
    let Ok(orders) = frame::default_orders(doc, st, f, &sets) else { return };
    let FeatureKind::FrameMember { corners, .. } = &mut f.kind else { return };
    for ((k, flip), order) in flipped.into_iter().zip(orders) {
        let (Some(c), Some(mut order)) = (corners.get_mut(k), order) else { continue };
        if let ([a, b, ..], true) = (order.as_mut_slice(), flip) {
            std::mem::swap(a, b);
        }
        c.members = order.into_iter().map(|line| MemberRef { feature: None, line }).collect();
    }
}

fn remove_member(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.remove_member";
    let members = removed_members(p.get("members")).map_err(|e| bad(cmd, e))?;
    let mut id = None;
    for (feature, _) in &members {
        let f = super::edit::feature_id(s, Some(feature), cmd)?;
        if id.is_some_and(|id| id != f) {
            return Err(bad(cmd, "`members` must all be tubes of one Frame Member"));
        }
        id = Some(f);
    }
    let f = id.and_then(|id| s.doc.feature(id)).cloned().ok_or_else(|| bad(cmd, "`members` must name a Frame Member"))?;
    let mut kind = f.kind.clone();
    let FeatureKind::FrameMember { path_sketch, lines, names, pairs, corners, .. } = &mut kind else {
        return Err(bad(cmd, format!("`{}` is not a Frame Member", f.name)));
    };
    // A copy names its tubes by position: each keeps the name it has now on its member, or every
    // tube after the one taken out would take the name of the one before.
    let pins = super::browser::member_pins(s, f.id).filter(|_| !f.body_names.is_empty());
    let by_position = pins.is_some();
    if let Some(pins) = pins {
        *names = pins;
    }
    let mut gone: std::collections::BTreeSet<String> = members.into_iter().map(|(_, line)| line).collect();
    let is_pair = |line: &str, pairs: &[MemberPair]| pairs.iter().any(|p| p.id == line);
    // Listed lines are known without laying the frame out: one that no longer lays out (a line of it gone from
    // its sketch) can still lose a tube, by line.
    let parts = if path_sketch.is_some() && gone.iter().any(|line| !is_pair(line, pairs)) {
        match frame::member_parts(&s.model.state_before(f.id), &f.kind) {
            Ok(parts) => Some(parts),
            Err(e) if lines.is_empty() => return Err(bad(cmd, e.to_string())),
            Err(_) => None,
        }
    } else {
        None
    };
    // A piece split off where another tube overlaps it (`L>X`, found only as the frame is built, so listed nowhere)
    // goes with its tube L.
    let known: Vec<String> =
        parts.as_ref().map_or_else(|| lines.clone(), |(every, _)| every.clone()).into_iter().chain(pairs.iter().map(|p| p.id.clone())).collect();
    gone = gone.into_iter().map(|m| frame::overlap_parent(&known, &m).cloned().unwrap_or(m)).collect();
    if let Some((every, later)) = parts {
        // Under Auto a tube may be a later piece of a line cut where tubes cross: its whole line goes, once.
        let line_of = |k: usize| later.get(k).copied().flatten().map_or(k, |(l, _)| l);
        gone = gone.into_iter().map(|m| every.iter().position(|id| *id == m).and_then(|k| every.get(line_of(k)).cloned()).unwrap_or(m)).collect();
        // The pieces that go with it, its own and those it cut, take their pinned names along.
        let cut = |i: usize| every.get(i).is_some_and(|id| gone.contains(id));
        let pieces: std::collections::HashSet<&str> =
            every.iter().zip(&later).filter(|(_, p)| p.is_some_and(|(l, x)| cut(l) || cut(x))).map(|(id, _)| id.as_str()).collect();
        names.retain(|id, _| !pieces.contains(id.as_str()));
        // No `lines` means every line of the sketch: they are listed as they are now before one goes,
        // with the rule evaluation uses. A later piece is no line of its own.
        if lines.is_empty() {
            *lines = every.iter().zip(&later).filter(|(l, piece)| piece.is_none() && !is_pair(l, pairs)).map(|(l, _)| l.clone()).collect();
        }
    }
    let gone: std::collections::BTreeSet<&str> = gone.iter().map(String::as_str).collect();
    if let Some(line) = gone.iter().find(|line| !is_pair(line, pairs) && !lines.iter().any(|l| l == *line)) {
        return Err(bad(cmd, format!("`{line}` is not a tube of {}", f.name)));
    }
    let had_lines = !lines.is_empty();
    lines.retain(|l| !gone.contains(l.as_str()));
    pairs.retain(|p| !gone.contains(p.id.as_str()));
    names.retain(|l, _| !gone.contains(l.as_str()) && frame::overlap_parent(&known, l).is_none_or(|l| !gone.contains(l.as_str())));
    // A corner override of a tube taken out names a corner that is gone with it.
    corners.retain(|c| !c.lines().any(|m| gone.contains(m)));
    // `next_pair` stays: a removed pair's id is never given again.
    if had_lines && lines.is_empty() {
        // No lines left, which an empty `lines` with a sketch would read as every line.
        *path_sketch = None;
    }
    if path_sketch.is_none() && pairs.is_empty() {
        // A Frame Member can't be without members: its last tube takes it, as Delete would.
        let delete = super::find_command("timeline.delete").ok_or_else(|| bad(cmd, "no command `timeline.delete`"))?;
        delete.run_nested(s, &json!({ "feature": f.id }))?;
        return frame_reply(cmd, RemoveMemberResult::Deleted(MemberFeatureDeleted { deleted: f.name, param_warnings: Vec::new() }));
    }
    let before = s.model.state();
    if let Some(slot) = s.doc_mut().feature_mut(f.id) {
        slot.kind = kind;
        if by_position {
            slot.body_names.clear();
        }
    }
    s.refresh();
    super::features::feature_reply(s, f.id, &before, true)
}

/// `frame.multi_trim`: one corner override of a Frame Member set, changed or (`remove`) taken away, in place: the
/// frame stays one timeline entry.
fn multi_trim(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.multi_trim";
    let id = super::edit::feature_id(s, p.get("feature"), cmd)?;
    let f = s.doc.feature(id).cloned().ok_or_else(|| bad(cmd, "`feature` must name a Frame Member"))?;
    let FeatureKind::FrameMember { corners, .. } = &f.kind else {
        return Err(bad(cmd, format!("`{}` is not a Frame Member", f.name)));
    };
    let given = |k: &str| !matches!(p.get(k), None | Some(Value::Null));
    let remove = given("remove") && bool_(p, "remove").ok_or_else(|| bad(cmd, "`remove` must be true or false"))?;
    if remove && ["kind", "flip", "gap"].into_iter().any(given) {
        return Err(bad(cmd, "`remove` takes the override away: give no `kind`, `flip` or `gap` with it"));
    }
    let mut members = trim_members(p.get("members")).map_err(|e| bad(cmd, e))?;
    for m in &mut members {
        match &m.feature {
            Some(n) if *n == f.name => m.feature = None,
            Some(_) => return Err(bad(cmd, "`members`: not supported yet: a corner with a member of another frame")),
            None => {}
        }
    }
    let lines: Vec<String> = members.iter().map(|m| m.line.clone()).collect();
    let set = |lines: &mut dyn Iterator<Item = &str>| {
        let mut set: Vec<String> = lines.map(str::to_string).collect();
        set.sort();
        set
    };
    let sorted = set(&mut lines.iter().map(String::as_str));
    if sorted.windows(2).any(|w| w.first() == w.get(1)) {
        return Err(bad(cmd, format!("`members` {lines:?}: a member is listed twice")));
    }
    let at = corners.iter().position(|c| set(&mut c.lines()) == sorted);
    let mut new_corners = corners.clone();
    if remove {
        let at = at.ok_or_else(|| bad(cmd, format!("{} has no override of {lines:?}", f.name)))?;
        new_corners.remove(at);
    } else {
        // As `frame.member` refuses it. A frame that no longer lays out is the evaluation's to report.
        if let Ok((ids, later)) = frame::member_parts(&s.model.state_before(id), &f.kind) {
            no_piece(&lines, &ids, &later).map_err(|e| bad(cmd, format!("`members`: {e}")))?;
        }
        let kind = match p.get("kind") {
            None | Some(Value::Null) => CornerType::Auto,
            Some(v) => trim_kind(v).ok_or_else(|| bad(cmd, TRIM_KIND))?,
        };
        let flip = match given("flip") {
            false => None,
            true => Some(bool_(p, "flip").ok_or_else(|| bad(cmd, "`flip` must be true or false"))?),
        };
        let gap = match given("gap") {
            false => None,
            true => {
                let e = expr(p, "gap").ok_or_else(|| bad(cmd, "`gap` must be a length expression"))?;
                Some(design_gap(s, &e, cmd, "`gap`")?.into())
            }
        };
        // The set must meet, as evaluation requires; its order today's rules give it, for Flip (given false: that order).
        let order = frame::default_orders(&s.doc, &s.model.state_before(id), &f, std::slice::from_ref(&lines))
            .map_err(|e| bad(cmd, e.to_string()))?
            .pop()
            .flatten()
            .ok_or_else(|| bad(cmd, format!("`members` {lines:?}: a corner override is not at a corner of the frame")))?;
        if let Some(flip) = flip {
            let mut order = order;
            if let ([a, b, ..], true) = (order.as_mut_slice(), flip) {
                std::mem::swap(a, b);
            }
            members = order.into_iter().map(|line| MemberRef { feature: None, line }).collect();
        }
        let record = CornerOverride { members, kind, gap };
        let full = new_corners.len() >= frame::MAX_MEMBERS;
        match at.and_then(|k| new_corners.get_mut(k)) {
            Some(slot) => *slot = record,
            None if full => return Err(bad(cmd, format!("{} already has {} corner overrides", f.name, frame::MAX_MEMBERS))),
            None => new_corners.push(record),
        }
    }
    let before = s.model.state();
    if let Some(FeatureKind::FrameMember { corners, .. }) = s.doc_mut().feature_mut(id).map(|f| &mut f.kind) {
        *corners = new_corners;
    }
    s.refresh();
    super::features::feature_reply(s, id, &before, true)
}

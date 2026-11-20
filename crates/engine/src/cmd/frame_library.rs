//! The design's profile library (#4003): `frame.library.list`, `.add`, `.edit`, `.remove` and
//! `.replace` keep the named frame profiles members are made from, and `frame.profile.points` lists
//! a profile's placement points. A member made from a library profile (`frame.member` with
//! `library`) follows every edit to it.

use serde_json::{Value, json};
use solidcraft_doc::FeatureKind;
use solidcraft_doc::expr::Kind;
use solidcraft_doc::frame::{self, MemberProfile, ProfileSpec};
use solidcraft_doc::frame_library::{self as library, LibraryProfile, LibrarySection, MAX_LIBRARY_PROFILES, NamedPoint};

use super::CommandSpec;
use crate::params::bad;
use crate::schema::frame::ProfileOut;
use crate::schema::frame_library::{
    LibraryAddParams, LibraryEditParams, LibraryEntryOut, LibraryEntryResult, LibraryFeatureError, LibraryListParams, LibraryListResult,
    LibraryRemoveParams, LibraryRemoveResult, LibraryReplaceParams, LibraryReplaceResult, PointOut, PointParam, ProfilePointsParams,
    ProfilePointsResult, SectionOut, typed,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

#[cfg(test)]
#[path = "frame_library_tests.rs"]
mod tests;

const LIST_CONTRACT: TypedContract<LibraryListParams> = TypedContract::new();
const ADD_CONTRACT: TypedContract<LibraryAddParams> = TypedContract::new();
const EDIT_CONTRACT: TypedContract<LibraryEditParams> = TypedContract::new();
const REMOVE_CONTRACT: TypedContract<LibraryRemoveParams> = TypedContract::new();
const REPLACE_CONTRACT: TypedContract<LibraryReplaceParams> = TypedContract::new();
const POINTS_CONTRACT: TypedContract<ProfilePointsParams> = TypedContract::new();

/// The keys add and edit share, as their parameter docs list them.
macro_rules! entry_doc {
    () => {
        "category?: [levels, outermost first, at most 3]; profile?: a built-in profile ({kind, …dimensions}: numbers in the design's units or length expressions) | profile_sketch?: a sketch whose section (its material, or `profiles` picked as solid.extrude takes them) is copied into the entry; points?: [{name, at: [x, y] mm} | {name, sketch_point: id with profile_sketch}] (placement points it names, beyond those every profile has and those its kind generates); default_point?: the placement point members sit on unless they name another (\"\" clears); material?, description?, stock_length? (a length expression; \"\" clears each)"
    };
}

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("frame.library.list", "List Library Profiles", crate::checked!("frame.library.list", list))
        .icon("library")
        .noundo()
        .doc_only()
        .contract(&LIST_CONTRACT)
        .output(output_schema_for::<LibraryListResult>)
        .params("Returns profiles: [{id, name, category, section, profile (built-in, in mm), points (every placement point: [{name, at}]), named_points, default_point, material, description, stock_length, stock_length_mm, area, used_by (features), error}]: the design's profile library"),
    CommandSpec::new("frame.library.add", "Add Library Profile", crate::checked!("frame.library.add", add))
        .icon("library")
        .contract(&ADD_CONTRACT)
        .output(output_schema_for::<LibraryEntryResult>)
        .params(concat!(
            "name (unique in the library ignoring case); ",
            entry_doc!(),
            ". One of profile and profile_sketch. Returns the profile as frame.library.list lists it; members use it by id (frame.member library)"
        )),
    CommandSpec::new("frame.library.edit", "Edit Library Profile", crate::checked!("frame.library.edit", edit))
        .icon("library")
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<LibraryEntryResult>)
        .params(concat!(
            "id (its id or name); name? (a new name); ",
            entry_doc!(),
            ". Keys left out keep their values; every member made from it is rebuilt. Returns the profile and the features that failed to rebuild"
        )),
    CommandSpec::new("frame.library.remove", "Remove Library Profile", crate::checked!("frame.library.remove", remove))
        .icon("library")
        .contract(&REMOVE_CONTRACT)
        .output(output_schema_for::<LibraryRemoveResult>)
        .params("id (its id or name). Refused while members are made from it: frame.library.replace moves them to another profile first"),
    CommandSpec::new("frame.library.replace", "Replace Library Profile", crate::checked!("frame.library.replace", replace))
        .icon("library")
        .contract(&REPLACE_CONTRACT)
        .output(output_schema_for::<LibraryReplaceResult>)
        .params("from, to (library profiles, by id or name): every member made from `from` is made from `to` instead. Returns the features moved"),
    CommandSpec::new("frame.profile.points", "Profile Placement Points", crate::checked!("frame.profile.points", points))
        .icon("library")
        .noundo()
        .doc_only()
        .contract(&POINTS_CONTRACT)
        .output(output_schema_for::<ProfilePointsResult>)
        .params("profile: a built-in profile | library: a library profile's id or name, one of them. Returns points: [{name, at: [x, y] mm}]: every placement point, those every profile has (origin, center, centroid, the corners and edge middles of its bounding box), then those its kind generates (an angle's heel, heel_inside, toe_x…), then those a library profile names"),
];

fn reply(cmd: &str, r: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(r).map_err(|e| bad(cmd, e.to_string()))
}

/// A length expression worked out in this design: a bare number in the design's units.
fn length(s: &Session, e: &str) -> std::result::Result<f64, String> {
    value(s, e, Kind::Length)
}

/// An expression's value in this design, a bare number in the design's unit for `kind`.
fn value(s: &Session, e: &str, kind: Kind) -> std::result::Result<f64, String> {
    let doc = &s.doc;
    doc.eval(&doc.with_design_unit(&solidcraft_doc::Expr::new(e), kind), kind).map_err(|err| err.to_string())
}

fn point_out(points: &[(String, solidcore_record::Vec2)]) -> Vec<PointOut> {
    points.iter().map(|(name, at)| PointOut { name: name.clone(), at: [at.x, at.y] }).collect()
}

/// An entry as the commands report it, worked out in this design.
fn entry_out(s: &Session, e: &LibraryProfile) -> LibraryEntryOut {
    let resolved = e.resolve(|label, ex, kind| value(s, ex, kind).map_err(|err| solidcraft_doc::DocError::Invalid(format!("`{label}`: {err}"))));
    let used_by = s.doc.features_using_library_profile(&e.id).into_iter().filter_map(|id| s.doc.feature(id).map(|f| f.name.clone())).collect();
    let (points, profile, area, error) = match &resolved {
        Ok(r) => (
            point_out(&r.points),
            r.builtin.as_ref().map(|p| ProfileOut(p.filled())),
            Some(r.regions.iter().map(|g| frame::section(g).area).sum()),
            None,
        ),
        Err(err) => (Vec::new(), None, None, Some(err.to_string())),
    };
    LibraryEntryOut {
        id: e.id.clone(),
        name: e.name.clone(),
        category: e.category.clone(),
        section: SectionOut(serde_json::to_value(&e.section).unwrap_or(Value::Null)),
        profile,
        points,
        named_points: e.points.iter().map(|p| PointOut { name: p.name.clone(), at: p.at }).collect(),
        default_point: e.default_point.clone(),
        material: e.material.clone(),
        description: e.description.clone(),
        stock_length: e.stock_length.as_ref().map(|x| x.to_string()),
        stock_length_mm: e.stock_length.as_ref().and_then(|x| length(s, x).ok()),
        area,
        used_by,
        error,
    }
}

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.library.list";
    let _: LibraryListParams = typed(cmd, p)?;
    let profiles = s.doc.frame_profiles.iter().map(|e| entry_out(s, e)).collect();
    reply(cmd, LibraryListResult { profiles, param_warnings: Vec::new() })
}

/// The library entry `key` names, by id or name.
fn find(s: &Session, cmd: &str, key: &str) -> Result<LibraryProfile> {
    library::find(&s.doc.frame_profiles, key).cloned().ok_or_else(|| {
        let names: Vec<&str> = s.doc.frame_profiles.iter().map(|p| p.name.as_str()).collect();
        if names.is_empty() {
            bad(cmd, format!("no library profile `{key}`: the library is empty (frame.library.add adds one)"))
        } else {
            bad(cmd, format!("no library profile `{key}` (the library has {})", names.join(", ")))
        }
    })
}

/// Optional text: `None` keeps the value, empty text clears it, other text is trimmed.
fn text(v: &Option<String>) -> Option<Option<String>> {
    v.as_ref().map(|t| Some(t.trim().to_string()).filter(|t| !t.is_empty()))
}

/// What add and edit take, applied to `e`. The section is replaced when one is given; the named
/// points when `points` is given.
struct Changes<'a> {
    category: &'a Option<Vec<String>>,
    profile: Option<&'a ProfileSpec>,
    profile_sketch: Option<&'a str>,
    profiles: Option<&'a Value>,
    points: &'a Option<Vec<PointParam>>,
    default_point: &'a Option<String>,
    material: &'a Option<String>,
    description: &'a Option<String>,
    stock_length: &'a Option<String>,
}

fn apply(s: &Session, cmd: &str, e: &mut LibraryProfile, c: &Changes) -> Result<()> {
    if let Some(levels) = c.category {
        e.category = levels.iter().map(|l| l.trim().to_string()).collect();
    }
    let mut sketch = None;
    match (c.profile, c.profile_sketch) {
        (Some(_), Some(_)) => return Err(bad(cmd, "give `profile` (a built-in profile) or `profile_sketch` (a sketch), not both")),
        (Some(spec), None) => {
            if c.profiles.is_some() {
                return Err(bad(cmd, "`profiles` picks among a `profile_sketch`'s profiles; it isn't used with `profile`"));
            }
            e.section = LibrarySection::BuiltIn { profile: spec.clone() };
        }
        (None, Some(key)) => {
            let f = s
                .doc
                .find_feature(key)
                .filter(|f| matches!(f.kind, FeatureKind::Sketch { .. }))
                .ok_or_else(|| bad(cmd, format!("no sketch `{key}`")))?;
            let id = solidcraft_doc::SketchId(f.id);
            let sel = match c.profiles {
                Some(v) => super::feature_profiles(&json!({ "profiles": v }), cmd)?,
                None => solidcraft_doc::ProfileSel::All,
            };
            let st = s.model.state();
            let regions = frame::sketch_section(&s.doc, &st, id, &sel).map_err(|err| bad(cmd, format!("`profile_sketch`: {err}")))?;
            e.section = LibrarySection::Drawn { regions };
            sketch = Some(id);
        }
        (None, None) if c.profiles.is_some() => return Err(bad(cmd, "`profiles` picks among a `profile_sketch`'s profiles; give the sketch too")),
        (None, None) => {}
    }
    if let Some(points) = c.points {
        let st = s.model.state();
        e.points = points
            .iter()
            .map(|p| {
                let at = match (&p.at, &p.sketch_point, sketch) {
                    (Some(at), None, _) => *at,
                    (None, Some(id), Some(sk)) => {
                        let v = frame::sketch_point(&st, sk, id)
                            .ok_or_else(|| bad(cmd, format!("placement point `{}`: the sketch has no point `{id}`", p.name)))?;
                        [v.x, v.y]
                    }
                    (None, Some(_), None) => {
                        return Err(bad(cmd, format!("placement point `{}`: `sketch_point` needs `profile_sketch` in the same call", p.name)));
                    }
                    _ => return Err(bad(cmd, format!("placement point `{}`: give `at` or `sketch_point`, one of them", p.name))),
                };
                Ok(NamedPoint { name: p.name.trim().to_string(), at })
            })
            .collect::<Result<_>>()?;
    }
    if let Some(v) = text(c.default_point) {
        e.default_point = v;
    }
    if let Some(v) = text(c.material) {
        e.material = v;
    }
    if let Some(v) = text(c.description) {
        e.description = v;
    }
    if let Some(v) = text(c.stock_length) {
        if let Some(x) = &v {
            let mm = length(s, x).map_err(|err| bad(cmd, format!("`stock_length`: {err}")))?;
            if !(mm > 0.0) {
                return Err(bad(cmd, "`stock_length` must be a positive length"));
            }
        }
        e.stock_length = v.map(solidcraft_doc::Expr::new);
    }
    e.check().map_err(|err| bad(cmd, err.to_string()))?;
    let r = e
        .resolve(|label, ex, kind| value(s, ex, kind).map_err(|err| solidcraft_doc::DocError::Invalid(format!("profile `{label}`: {err}"))))
        .map_err(|err| bad(cmd, err.to_string()))?;
    if r.regions.len() != 1 {
        return Err(bad(
            cmd,
            format!("the section is {} separate pieces: a member's section is one piece (pick one with `profiles`)", r.regions.len()),
        ));
    }
    if let Some(name) = &e.default_point {
        library::find_point(&r.points, name).map_err(|err| bad(cmd, format!("`default_point`: {err}")))?;
    }
    Ok(())
}

/// The name is free in the library (ignoring case), apart from entry `except`.
fn check_free(s: &Session, cmd: &str, name: &str, except: Option<&str>) -> Result<()> {
    match s.doc.frame_profiles.iter().find(|p| p.name.eq_ignore_ascii_case(name) && Some(p.id.as_str()) != except) {
        Some(p) => Err(bad(cmd, format!("the library already has a profile named `{}`", p.name))),
        None => Ok(()),
    }
}

/// The features using entry `id` that failed after a rebuild.
fn feature_errors(s: &Session, id: &str) -> Vec<LibraryFeatureError> {
    let using = s.doc.features_using_library_profile(id);
    s.model
        .results
        .iter()
        .filter(|r| using.contains(&r.id))
        .filter_map(|r| r.error.as_ref().map(|e| LibraryFeatureError { feature: r.name.clone(), error: e.clone() }))
        .collect()
}

fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.library.add";
    let a: LibraryAddParams = typed(cmd, p)?;
    if s.doc.frame_profiles.len() >= MAX_LIBRARY_PROFILES {
        return Err(bad(cmd, format!("the library holds at most {MAX_LIBRARY_PROFILES} profiles")));
    }
    if a.profile.is_none() && a.profile_sketch.is_none() {
        return Err(bad(cmd, "give `profile` (a built-in profile) or `profile_sketch` (a sketch), one of them"));
    }
    let name = a.name.trim().to_string();
    check_free(s, cmd, &name, None)?;
    let mut e = LibraryProfile {
        id: library::next_id(&s.doc.frame_profiles),
        name,
        category: Vec::new(),
        // Replaced by `apply`, which requires a section here.
        section: LibrarySection::Drawn { regions: Vec::new() },
        points: Vec::new(),
        default_point: None,
        material: None,
        description: None,
        stock_length: None,
    };
    let changes = Changes {
        category: &a.category,
        profile: a.profile.as_ref().map(|x| &x.0),
        profile_sketch: a.profile_sketch.as_ref().map(|x| x.0.as_str()),
        profiles: a.profiles.as_ref().map(|x| &x.0),
        points: &a.points,
        default_point: &a.default_point,
        material: &a.material,
        description: &a.description,
        stock_length: &a.stock_length,
    };
    apply(s, cmd, &mut e, &changes)?;
    s.doc_mut().frame_profiles.push(e.clone());
    s.refresh();
    reply(cmd, LibraryEntryResult { profile: entry_out(s, &e), errors: Vec::new(), param_warnings: Vec::new() })
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.library.edit";
    let a: LibraryEditParams = typed(cmd, p)?;
    let mut e = find(s, cmd, &a.id)?;
    if let Some(name) = &a.name {
        let name = name.trim().to_string();
        check_free(s, cmd, &name, Some(&e.id))?;
        e.name = name;
    }
    let changes = Changes {
        category: &a.category,
        profile: a.profile.as_ref().map(|x| &x.0),
        profile_sketch: a.profile_sketch.as_ref().map(|x| x.0.as_str()),
        profiles: a.profiles.as_ref().map(|x| &x.0),
        points: &a.points,
        default_point: &a.default_point,
        material: &a.material,
        description: &a.description,
        stock_length: &a.stock_length,
    };
    apply(s, cmd, &mut e, &changes)?;
    let id = e.id.clone();
    if let Some(slot) = s.doc_mut().frame_profiles.iter_mut().find(|x| x.id == id) {
        *slot = e.clone();
    }
    s.refresh();
    let errors = feature_errors(s, &id);
    reply(cmd, LibraryEntryResult { profile: entry_out(s, &e), errors, param_warnings: Vec::new() })
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.library.remove";
    let a: LibraryRemoveParams = typed(cmd, p)?;
    let e = find(s, cmd, &a.id)?;
    let using: Vec<String> =
        s.doc.features_using_library_profile(&e.id).into_iter().filter_map(|id| s.doc.feature(id).map(|f| f.name.clone())).collect();
    if !using.is_empty() {
        return Err(bad(
            cmd,
            format!("{} is used by {}: move those members to another profile first (frame.library.replace)", e.name, using.join(", ")),
        ));
    }
    s.doc_mut().frame_profiles.retain(|x| x.id != e.id);
    s.refresh();
    reply(cmd, LibraryRemoveResult { removed: e.id, param_warnings: Vec::new() })
}

fn replace(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.library.replace";
    let a: LibraryReplaceParams = typed(cmd, p)?;
    let (from, to) = (find(s, cmd, &a.from)?, find(s, cmd, &a.to)?);
    if from.id == to.id {
        return Err(bad(cmd, "`from` and `to` are the same profile"));
    }
    let using = s.doc.features_using_library_profile(&from.id);
    let features: Vec<String> = using.iter().filter_map(|id| s.doc.feature(*id).map(|f| f.name.clone())).collect();
    if !using.is_empty() {
        for f in s.doc_mut().features.iter_mut().filter(|f| using.contains(&f.id)) {
            if let FeatureKind::FrameMember { profile: MemberProfile::LibraryProfile { id }, .. } = &mut f.kind {
                *id = to.id.clone();
            }
        }
        s.refresh();
    }
    let errors = feature_errors(s, &to.id);
    reply(cmd, LibraryReplaceResult { features, errors, param_warnings: Vec::new() })
}

fn points(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.profile.points";
    let a: ProfilePointsParams = typed(cmd, p)?;
    let pts = match (&a.profile, &a.library) {
        (Some(spec), None) => {
            let pr = spec
                .0
                .resolve(|name, ex, kind| value(s, ex, kind).map_err(|err| solidcraft_doc::DocError::Invalid(format!("profile `{name}`: {err}"))))
                .map_err(|err| bad(cmd, err.to_string()))?;
            let region = pr.region().map_err(|err| bad(cmd, err.to_string()))?;
            library::placement_points(std::slice::from_ref(&region), Some(&pr), &[])
        }
        (None, Some(key)) => {
            let e = find(s, cmd, key)?;
            e.resolve(|label, ex, kind| value(s, ex, kind).map_err(|err| solidcraft_doc::DocError::Invalid(format!("profile `{label}`: {err}"))))
                .map_err(|err| bad(cmd, err.to_string()))?
                .points
        }
        _ => return Err(bad(cmd, "give `profile` (a built-in profile) or `library` (a library profile), one of them")),
    };
    reply(cmd, ProfilePointsResult { points: point_out(&pts), param_warnings: Vec::new() })
}

/// The library profile a `frame.member` call names (`library`, by id or name), as the member's
/// profile and what its members are named after.
pub(super) fn member_profile(s: &Session, cmd: &str, key: &str) -> Result<(MemberProfile, String)> {
    let e = find(s, cmd, key)?;
    Ok((MemberProfile::LibraryProfile { id: e.id.clone() }, e.name))
}

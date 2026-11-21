//! Amalgamate (#18799). An Amalgam Tag labels bodies of a design Remove, Add or New, which makes
//! the design a tool that other designs place: where it is placed, the Remove bodies are cut
//! first, then the Add bodies are joined, and the New bodies come in as a part of their own.
use std::path::Path;
use std::sync::Arc;

use super::{CommandSpec, not_in_sketch};
use crate::params::bad;
use crate::schema::amalgam::{AmalgamTagParams, AmalgamateParams};
use crate::schema::features::FeatureResult;
use crate::schema::{Contract, StringList, TypedContract, output_schema_for};
use crate::{Result, Session};
use serde_json::Value;
use solidcraft_doc::amalgam::{AmalgamSource, AmalgamTool, check_placement, seed_key};
use solidcraft_doc::{BodyName, DesignLink, FeatureKind};

const TAG_CONTRACT: TypedContract<AmalgamTagParams> = TypedContract::new();
const PLACE_CONTRACT: TypedContract<AmalgamateParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("solid.amalgam_tag", "Amalgam Tag", crate::checked!("solid.amalgam_tag", amalgam_tag))
        .contract(&TAG_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .enabled(not_in_sketch)
        .params("remove?: body | [body]; add?: body | [body]; new?: body | [body] (at least one body in all, each in one list only); seat?: the tool's origin, a frame as solid.align's from_frame takes one ({sketch, point|curve}, or a joint origin as joint.create takes one: {face | circle | edge | point: [x, y, z]}), on this design (default: the design's origin): makes this design a tool that other designs place with Amalgamate (#18799). Where it is placed, the seat goes onto the place (its z on the place's z), the remove bodies are cut from the bodies named for them, then the add bodies are joined into the bodies named for those, and the new bodies come in as a part of their own. Remove and add bodies must be closed solids; no mesh bodies. Changes no geometry. The tag's name is the feature's `name`; a design may hold several tags, each a tool of its own."),
    CommandSpec::new("solid.amalgamate", "Amalgamate", crate::checked!("solid.amalgamate", amalgamate))
        .contract(&PLACE_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .enabled(not_in_sketch)
        .params("path: the tool design (.sc); configuration?: its row, by name or id (null or left out: its saved state); tag?: the Amalgam Tag to place, by name (default: the last one there); seeds: [frame] (1 to 256, each as solid.align's from_frame takes one: {sketch, point|curve}, or a joint origin on a face, circle or edge: {face | circle | edge: [x, y, z]}); turns?: 0-3 quarter turns about each seed's z; flip?: bool (each copy turned over); remove_from?: [body] (required when the tool has Remove bodies, refused when it has none); add_to?: [body] (the same for Add bodies): places the tool (#18799), read from the file now and kept in this design, at every seed, its seat on the seed. At every seed the tool's Remove bodies are cut from remove_from first; only then are its Add bodies joined into add_to. Each copy's faces are named after its seed, so removing a seed renames nothing at the others. Not yet: a tool with New bodies."),
];

fn amalgam_tag(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.amalgam_tag";
    // The contract's own message, whatever the parameter check is set to: it names the key.
    if let Some(first) = TAG_CONTRACT.check(p).first() {
        return Err(bad(cmd, first.message.clone()));
    }
    let args: AmalgamTagParams = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let (remove, add, new) = (names(args.remove), names(args.add), names(args.new));
    if remove.is_empty() && add.is_empty() && new.is_empty() {
        return Err(bad(cmd, "tag at least one body: give `remove`, `add` or `new`"));
    }
    let seat = match args.seat {
        None => None,
        Some(v) => {
            let (frame, on) = super::features_more::frame_param(s, &v, cmd, "seat")?;
            if on != s.active_component {
                return Err(bad(cmd, "not supported yet: a `seat` on another component than the tag's"));
            }
            frame.resolve(&s.doc, &s.model.state()).map_err(|e| bad(cmd, format!("`seat`: {e}")))?;
            Some(frame)
        }
    };
    super::features::add_feature(s, p, FeatureKind::AmalgamTag { remove, add, new, seat })
}

fn names(list: Option<StringList>) -> Vec<BodyName> {
    list.map(|l| l.0.into_iter().map(BodyName).collect()).unwrap_or_default()
}

fn amalgamate(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.amalgamate";
    if let Some(first) = PLACE_CONTRACT.check(p).first() {
        return Err(bad(cmd, first.message.clone()));
    }
    let args: AmalgamateParams = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let given = args.path.as_deref().ok_or_else(|| bad(cmd, "`path` is required"))?;
    let tool = read_tool(s, p, given, args.tag.as_deref(), cmd)?;
    // The seeds, each found on this design now and named by what it is found by.
    let st = s.model.state();
    let mut seeds = Vec::with_capacity(args.seeds.len());
    let mut keys: Vec<String> = Vec::with_capacity(args.seeds.len());
    for (i, v) in args.seeds.iter().enumerate() {
        let at = format!("seeds[{i}]");
        let (frame, on) = super::features_more::frame_param(s, v, cmd, &at)?;
        if on != s.active_component {
            return Err(bad(cmd, format!("not supported yet: `{at}` on another component than the feature's")));
        }
        let key = seed_key(&frame).map_err(|e| bad(cmd, format!("`{at}`: {e}")))?;
        if keys.contains(&key) {
            return Err(bad(cmd, format!("`{at}` is at the same place as an earlier seed")));
        }
        frame.resolve_frame(&s.doc, &st).map_err(|e| bad(cmd, format!("`{at}`: {e}")))?;
        keys.push(key);
        seeds.push(frame);
    }
    let (remove_from, add_to) = (names(args.remove_from), names(args.add_to));
    let turns = args.turns.unwrap_or(0);
    check_placement(&tool, seeds.len(), turns, &remove_from, &add_to).map_err(|e| bad(cmd, e.to_string()))?;
    let kind = FeatureKind::Amalgamate { tool: Box::new(tool), seeds, turns, flip: args.flip.unwrap_or(false), remove_from, add_to };
    super::features::add_feature(s, p, kind)
}

/// The tool the tag `tag` (default: the last one) labels in the design at `given`, as the
/// design's configuration row in `p` builds it: the tagged bodies at the end of its timeline,
/// moved so the tag's seat is the origin, as B-rep records, with the design's names for their
/// faces.
fn read_tool(s: &Session, p: &Value, given: &str, tag: Option<&str>, cmd: &str) -> Result<AmalgamTool> {
    use super::component_link::{configuration_id, configuration_key, link_refused, opened_link, read_link};
    use super::design_link::{absolute, same_file, stored};
    let selection = configuration_key(p, None, cmd)?;
    link_refused(s.path.as_deref(), given).map_err(|e| bad(cmd, e.to_string()))?;
    let file = absolute(Path::new(given));
    if s.path.as_deref().is_some_and(|own| same_file(Path::new(own), &file)) {
        return Err(bad(cmd, "a design can't place itself as a tool"));
    }
    let shown = file.display().to_string();
    let bytes = read_link(&file, s.path.as_deref())?;
    let configuration = configuration_id(&bytes, selection).map_err(|e| bad(cmd, e.to_string()))?;
    let part = opened_link(&file, &bytes, configuration, s.validation())?;
    if let Some(e) = part.model.results.iter().find_map(|r| r.error.clone()) {
        return Err(bad(cmd, format!("{shown}: the tool design has a feature that fails ({e}); fix it first")));
    }
    let tags: Vec<&solidcraft_doc::Feature> = part.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::AmalgamTag { .. })).collect();
    let feature = match tag {
        Some(name) => tags.iter().rev().find(|f| f.name == name).copied().ok_or_else(|| {
            let known: Vec<String> = tags.iter().map(|f| format!("`{}`", f.name)).collect();
            bad(
                cmd,
                format!("{shown} has no Amalgam Tag named `{name}` (it has {})", if known.is_empty() { "none".into() } else { known.join(", ") }),
            )
        })?,
        None => tags.last().copied().ok_or_else(|| bad(cmd, format!("{shown} has no Amalgam Tag: tag the tool's bodies there first")))?,
    };
    let FeatureKind::AmalgamTag { remove, add, new, seat } = &feature.kind else {
        return Err(bad(cmd, "the tag is not an Amalgam Tag"));
    };
    if !new.is_empty() {
        return Err(bad(
            cmd,
            format!("not supported yet: `{}` has New bodies, which come in as a part of their own in a later change", feature.name),
        ));
    }
    let st = part.model.state();
    let seat_frame = match seat {
        Some(seat) => seat.resolve_frame(&part.doc, &st).map_err(|e| bad(cmd, format!("{shown}: the tag's seat: {e}")))?,
        None => solidcraft_doc::IDENTITY,
    };
    let into_seat = solidcraft_doc::mat_inverse(&seat_frame).ok_or_else(|| bad(cmd, format!("{shown}: the tag's seat is not a frame")))?;
    let (mut bodies, mut roles, mut faces) = (Vec::new(), Vec::new(), Vec::new());
    for (name, role) in solidcraft_doc::amalgam::roles(remove, add, new) {
        let b = st.body(name).ok_or_else(|| bad(cmd, format!("{shown}: the tagged body `{name}` is not there at the end of the tool design")))?;
        if part.doc.model_body_component(b) != 0 {
            return Err(bad(cmd, format!("not supported yet: `{name}` is in a component of the tool design")));
        }
        let moved = solidcraft_kernel::transform_matrix(&b.body, into_seat).map_err(|e| bad(cmd, format!("{shown}: `{name}`: {e}")))?;
        let brep = solidcraft_kernel::brep_record(&moved).map_err(|e| bad(cmd, format!("{shown}: `{name}`: {e}")))?;
        let count = brep.faces.len();
        let mut carried: Vec<String> = solidcraft_doc::naming::face_names(b).iter().cloned().collect();
        carried.resize(count, String::new());
        faces.push(carried);
        roles.push(role);
        bodies.push(solidcraft_kernel::RecordBody {
            name: name.to_string(),
            path: Vec::new(),
            instance: None,
            color: None,
            paint: None,
            hidden: false,
            placement: None,
            face_origin: (0..count as u64).collect(),
            brep: Some(brep),
            sheet: false,
            copy_of: None,
            mesh: None,
        });
    }
    let tree = vec![solidcraft_kernel::StepNode {
        name: feature.name.clone(),
        transform: solidcraft_doc::IDENTITY,
        bodies: (0..bodies.len()).collect(),
        items: Vec::new(),
        left_out: Vec::new(),
        children: Vec::new(),
    }];
    let records = solidcraft_kernel::ImportRecords { bodies, tree, warnings: Vec::new(), system: "SolidCraft".into() };
    Ok(AmalgamTool {
        source: AmalgamSource {
            path: stored(s.path.as_deref(), &file),
            configuration: configuration.map(|c| c.row),
            configuration_identity: configuration.and_then(|c| c.identity),
            file_sha256: Some(DesignLink::file_digest(&bytes)),
        },
        tag: feature.name.clone(),
        records: Arc::new(records),
        roles,
        faces,
    })
}

#[cfg(test)]
#[path = "amalgam_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;

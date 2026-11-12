//! Public geometry: reference elements of a skeleton component (sketches, construction planes,
//! axes and points) shared as a named group. An element is published under the name its feature
//! has at the time; the feature id says which element it is, so renaming the feature later
//! changes nothing, and deleting it takes it out of the group.

use serde_json::Value;
use solidcraft_doc::{PublicGeometry, PublishedItem};

use super::CommandSpec;
use super::component::reply;
use crate::params::{bad, str_};
use crate::schema::public_geometry::{
    Publication, PublicationsParams, PublicationsResult, PublishParams, PublishResult, PublishedEntry, UnpublishParams, UnpublishResult,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const MAX_ITEMS: usize = 10_000;
const MAX_NAME: usize = 128;

const PUBLISH_CONTRACT: TypedContract<PublishParams> = TypedContract::new();
const UNPUBLISH_CONTRACT: TypedContract<UnpublishParams> = TypedContract::new();
const PUBLICATIONS_CONTRACT: TypedContract<PublicationsParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("geometry.publish", "Public Geometry", crate::checked!("geometry.publish", publish))
        .icon("construction")
        .contract(&PUBLISH_CONTRACT)
        .output(output_schema_for::<PublishResult>)
        .params("name: the public geometry's name (unique, up to 128 characters); items: [feature ids or names] of sketches and construction planes, axes or points of a skeleton component. Each is published under its feature's name"),
    CommandSpec::new("geometry.unpublish", "Remove Public Geometry", crate::checked!("geometry.unpublish", unpublish))
        .icon("construction")
        .contract(&UNPUBLISH_CONTRACT)
        .output(output_schema_for::<UnpublishResult>)
        .params("name: a public geometry's name; removes the whole group, the features stay"),
    CommandSpec::new("geometry.publications", "Public Geometries", crate::checked!("geometry.publications", publications))
        .icon("construction")
        .noundo()
        .contract(&PUBLICATIONS_CONTRACT)
        .output(output_schema_for::<PublicationsResult>)
        .params("→ publications: [{name, items: [{name, feature, type}]}], one per public geometry"),
];

/// A feature id or name as given in `items`.
fn item_key(v: &Value, cmd: &str) -> Result<String> {
    match v {
        Value::String(t) if !t.trim().is_empty() && t.len() <= 4096 => Ok(t.trim().to_string()),
        Value::Number(n) => Ok(n.to_string()),
        _ => Err(bad(cmd, "an item must be a feature id or name")),
    }
}

/// The publishable feature `item` names: its id and name. Refuses what cannot be published and
/// what already is.
fn resolve(s: &Session, cmd: &str, item: &str) -> Result<(u64, String)> {
    // An id names one feature; a name may be shared between skeletons, so the active
    // component's wins and anything still ambiguous is refused.
    let mut found: Vec<_> = s.doc.features.iter().filter(|f| f.id.to_string() == item || f.name.eq_ignore_ascii_case(item)).collect();
    if found.len() > 1 && found.iter().any(|f| f.id.to_string() == item) {
        found.retain(|f| f.id.to_string() == item);
    }
    if found.len() > 1 {
        let in_active: Vec<_> = found.iter().copied().filter(|f| f.component == s.active_component).collect();
        if in_active.len() == 1 {
            found = in_active;
        }
    }
    let f = match found.as_slice() {
        [] => return Err(bad(cmd, format!("no feature `{item}`"))),
        [f] => *f,
        _ => return Err(bad(cmd, format!("`{item}` is ambiguous: give its id"))),
    };
    if !s.doc.is_skeleton(f.component) {
        return Err(bad(cmd, format!("`{}` is not in a skeleton component: only skeleton elements can be published", f.name)));
    }
    if !f.kind.is_reference() {
        return Err(bad(cmd, format!("`{}` is not a sketch or a construction plane, axis or point", f.name)));
    }
    if let Some(g) = s.doc.public_geometry.iter().find(|g| g.items.iter().any(|i| i.feature == f.id)) {
        return Err(bad(cmd, format!("`{}` is already published in `{}`", f.name, g.name)));
    }
    Ok((f.id, f.name.clone()))
}

fn entries(s: &Session, items: &[PublishedItem]) -> Vec<PublishedEntry> {
    let kind = |id| s.doc.feature(id).map_or("", |f| f.kind.base_name()).to_string();
    items.iter().map(|i| PublishedEntry { name: i.name.clone(), feature: i.feature, kind: kind(i.feature) }).collect()
}

fn publish(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "geometry.publish";
    let name = str_(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(cmd, "`name` is required"))?;
    if name.chars().count() > MAX_NAME || name.chars().any(char::is_control) {
        return Err(bad(cmd, format!("`name` must be plain text of at most {MAX_NAME} characters")));
    }
    if s.doc.public_geometry.iter().any(|g| g.name.eq_ignore_ascii_case(name)) {
        return Err(bad(cmd, format!("a public geometry `{name}` already exists")));
    }
    let raw = p.get("items").and_then(Value::as_array).filter(|a| !a.is_empty() && a.len() <= MAX_ITEMS);
    let raw = raw.ok_or_else(|| bad(cmd, "`items` must list the features to publish"))?;
    let mut items: Vec<PublishedItem> = Vec::with_capacity(raw.len());
    for v in raw {
        let (feature, own) = resolve(s, cmd, &item_key(v, cmd)?)?;
        if items.iter().any(|e| e.feature == feature) {
            return Err(bad(cmd, format!("`{own}` is listed twice")));
        }
        if items.iter().chain(s.doc.public_geometry.iter().flat_map(|g| g.items.iter())).any(|e| e.name.eq_ignore_ascii_case(&own)) {
            return Err(bad(cmd, format!("the name `{own}` is already published")));
        }
        items.push(PublishedItem { name: own, feature });
    }
    if s.doc.public_geometry.iter().map(|g| g.items.len()).sum::<usize>() + items.len() > MAX_ITEMS {
        return Err(bad(cmd, "too many published elements"));
    }
    let out = PublishResult { name: name.to_string(), items: entries(s, &items), param_warnings: Vec::new() };
    s.doc_mut().public_geometry.push(PublicGeometry { name: name.to_string(), items });
    reply(out)
}

fn unpublish(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_(p, "name").map(str::trim).unwrap_or_default();
    let at = s.doc.public_geometry.iter().position(|g| g.name.eq_ignore_ascii_case(name));
    let at = at.ok_or_else(|| bad("geometry.unpublish", format!("no public geometry `{name}`")))?;
    let gone = s.doc_mut().public_geometry.remove(at);
    reply(UnpublishResult { name: gone.name, removed: gone.items.len(), param_warnings: Vec::new() })
}

fn publications(s: &mut Session, _: &Value) -> Result<Value> {
    let publications = s.doc.public_geometry.iter().map(|g| Publication { name: g.name.clone(), items: entries(s, &g.items) }).collect();
    reply(PublicationsResult { publications, param_warnings: Vec::new() })
}

//! Amalgamate (#18799). An Amalgam Tag labels bodies of a design Remove, Add or New, which makes
//! the design a tool that other designs place: where it is placed, the Remove bodies are cut
//! first, then the Add bodies are joined, and the New bodies come in as a part of their own.
use super::{CommandSpec, not_in_sketch};
use crate::params::bad;
use crate::schema::amalgam::AmalgamTagParams;
use crate::schema::features::FeatureResult;
use crate::schema::{Contract, StringList, TypedContract, output_schema_for};
use crate::{Result, Session};
use serde_json::Value;
use solidcraft_doc::{BodyName, FeatureKind};

const TAG_CONTRACT: TypedContract<AmalgamTagParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("solid.amalgam_tag", "Amalgam Tag", crate::checked!("solid.amalgam_tag", amalgam_tag))
    .contract(&TAG_CONTRACT)
    .output(output_schema_for::<FeatureResult>)
    .enabled(not_in_sketch)
    .params("remove?: body | [body]; add?: body | [body]; new?: body | [body] (at least one body in all, each in one list only): makes this design a tool that other designs place with Amalgamate (#18799). Where it is placed, the remove bodies are cut from the bodies named for them, then the add bodies are joined into the bodies named for those, and the new bodies come in as a part of their own. Remove and add bodies must be closed solids; no mesh bodies. Changes no geometry. The tag's name is the feature's `name`; a design may hold several tags, each a tool of its own.")];

fn amalgam_tag(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.amalgam_tag";
    let args: AmalgamTagParams = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let allowed = TAG_CONTRACT.allowed_keys();
    if let Some(key) = p.as_object().and_then(|object| object.keys().find(|key| !allowed.contains(key.as_str()))) {
        return Err(bad(cmd, format!("unknown parameter `{key}`")));
    }
    let names = |list: Option<StringList>| list.map(|l| l.0.into_iter().map(BodyName).collect::<Vec<_>>()).unwrap_or_default();
    let (remove, add, new) = (names(args.remove), names(args.add), names(args.new));
    if remove.is_empty() && add.is_empty() && new.is_empty() {
        return Err(bad(cmd, "tag at least one body: give `remove`, `add` or `new`"));
    }
    super::features::add_feature(s, p, FeatureKind::AmalgamTag { remove, add, new })
}

#[cfg(test)]
#[path = "amalgam_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;

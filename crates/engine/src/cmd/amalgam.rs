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
    .params("remove?: body | [body]; add?: body | [body]; new?: body | [body] (at least one body in all, each in one list only); seat?: the tool's origin, a frame as solid.align's from_frame takes one ({sketch, point|curve}, or a joint origin as joint.create takes one: {face | circle | edge | point: [x, y, z]}), on this design (default: the design's origin): makes this design a tool that other designs place with Amalgamate (#18799). Where it is placed, the seat goes onto the place (its z on the place's z), the remove bodies are cut from the bodies named for them, then the add bodies are joined into the bodies named for those, and the new bodies come in as a part of their own. Remove and add bodies must be closed solids; no mesh bodies. Changes no geometry. The tag's name is the feature's `name`; a design may hold several tags, each a tool of its own.")];

fn amalgam_tag(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.amalgam_tag";
    // The contract's own message, whatever the parameter check is set to: it names the key.
    if let Some(first) = TAG_CONTRACT.check(p).first() {
        return Err(bad(cmd, first.message.clone()));
    }
    let args: AmalgamTagParams = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let names = |list: Option<StringList>| list.map(|l| l.0.into_iter().map(BodyName).collect::<Vec<_>>()).unwrap_or_default();
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

#[cfg(test)]
#[path = "amalgam_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;

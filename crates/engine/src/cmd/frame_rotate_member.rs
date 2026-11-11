//! Rotate Member: turn one frame member about its own line.
use super::{CommandSpec, not_in_sketch};
use crate::params::bad;
use crate::schema::features::FeatureResult;
use crate::schema::rotate_member::RotateMemberParams;
use crate::schema::{Contract, TypedContract, output_schema_for};
use crate::{Result, Session};
use serde_json::{Value, json};
use solidcraft_doc::{FeatureKind, end_cap};

const CONTRACT: TypedContract<RotateMemberParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] =
    &[CommandSpec::new("frame.rotate_member", "Rotate Member", crate::checked!("frame.rotate_member", rotate_member_cmd))
        .contract(&CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .at("WELDMENTS", "MODIFY")
        .icon("rotate_member")
        .enabled(not_in_sketch)
        .params("face: {face:[x,y,z],body,index,path?} a face of the frame member to turn, picked in world mm; angle: the angle the member is turned to about its own line, counter-clockwise looking back from the line's end (an angle or a parameter expression; it replaces the member's own angle). Adds a step after the Frame Member. Common name field supported.")];

fn rotate_member_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.rotate_member";
    let args: RotateMemberParams = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let allowed = CONTRACT.allowed_keys();
    if let Some(key) = p.as_object().and_then(|object| object.keys().find(|key| !allowed.contains(key.as_str()))) {
        return Err(bad(cmd, format!("unknown parameter `{key}`")));
    }
    let pick = args.face;
    let mut value = json!({"face": pick.face.0, "body": pick.body, "index": pick.index});
    if let Some(path) = pick.path {
        value["path"] = json!(path);
    }
    let plane = super::sketch::face::capture(s, &value, cmd)?;
    // Pin each member's name to its line, so the step finds its member after renames and when
    // other lines come and go.
    if let Some(anchor) = super::sketch::face::anchor(&plane)
        && let Some(pins) = super::browser::member_pins(s, anchor.target.origin_feature)
    {
        let source = s.doc_mut().feature_mut(anchor.target.origin_feature).ok_or_else(|| bad(cmd, "the member's feature disappeared"))?;
        if let FeatureKind::FrameMember { names, .. } = &mut source.kind {
            if names.len().saturating_add(pins.keys().filter(|id| !names.contains_key(*id)).count()) > solidcraft_doc::frame::MAX_MEMBERS {
                return Err(bad(cmd, "too many member names to pin"));
            }
            names.extend(pins);
            source.body_names.clear();
        }
    }
    let member = end_cap::member(&s.doc, &s.model.state(), &plane).map_err(|_| bad(cmd, "Select a face of a frame member"))?;
    if super::sketch::face::anchor(&plane).is_some_and(|anchor| !anchor.path.is_empty()) {
        return Err(bad(cmd, "Activate the member's component"));
    }
    let kind = FeatureKind::FrameMemberTurn { member, angle: args.angle.0 };
    for (label, expr, kind) in kind.inputs() {
        super::features::check_expr(s, &expr, kind, cmd, label)?;
    }
    super::features::add_feature_in(s, p, kind, false)
}

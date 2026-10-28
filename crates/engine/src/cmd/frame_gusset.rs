//! Gussets between two exact planar face attachments.
use super::{CommandSpec, not_in_sketch};
use crate::params::bad;
use crate::schema::features::FeatureResult;
use crate::schema::gusset::{GussetParams, Placement};
use crate::schema::{Contract, TypedContract, output_schema_for};
use crate::{Result, Session};
use serde_json::{Value, json};
use solidcraft_doc::{FeatureKind, gusset};
const CONTRACT: TypedContract<GussetParams> = TypedContract::new();
pub static COMMANDS:&[CommandSpec]=&[
    CommandSpec::new("frame.gusset","Gusset",crate::checked!("frame.gusset",gusset))
    .contract(&CONTRACT).output(output_schema_for::<FeatureResult>).at("WELDMENTS","CREATE").icon("gusset").enabled(not_in_sketch)
    .params("supports: exactly two {face:[x,y,z],body,index,path?} planar face picks in world mm; leg1, leg2, thickness: lengths in design units or parameter expressions; relief?: inside-corner setback along each leg (default 0); clip?: equal-distance clipping at both outer tips (default 0: triangle); offset?: signed distance along the joint from the centre of the faces' overlapping span (default 0); placement?: center|positive|negative (default center). Ordered outward support normals define the positive joint/thickness direction by their cross product. Contact strips must fit entirely on both finite faces. Creates a separate body; does not join, drill or add a weld. Common name/body_name fields supported."),
];
fn gusset(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.gusset";
    let args: GussetParams = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let allowed = CONTRACT.allowed_keys();
    if let Some(key) = p.as_object().and_then(|object| object.keys().find(|key| !allowed.contains(key.as_str()))) {
        return Err(bad(cmd, format!("unknown parameter `{key}`")));
    }
    if args.supports.len() != 2 {
        return Err(bad(cmd, "exactly two planar support faces are required"));
    }
    let mut supports = Vec::new();
    for pick in args.supports {
        let mut value = json!({"face":pick.face.0,"body":pick.body,"index":pick.index});
        if let Some(path) = pick.path {
            value["path"] = json!(path);
        }
        let plane = super::sketch::face::capture(s, &value, cmd)?;
        if let Some(anchor) = super::sketch::face::anchor(&plane)
            && let Some(pins) = super::browser::member_pins(s, anchor.target.origin_feature)
        {
            let source = s.doc_mut().feature_mut(anchor.target.origin_feature).ok_or_else(|| bad(cmd, "support producer disappeared"))?;
            if let FeatureKind::FrameMember { names, .. } = &mut source.kind {
                if names.len().saturating_add(pins.keys().filter(|id| !names.contains_key(*id)).count()) > solidcraft_doc::frame::MAX_MEMBERS {
                    return Err(bad(cmd, "gusset attachment exceeds member name budget"));
                }
                names.extend(pins);
                source.body_names.clear();
            }
        }
        let member = gusset::member(&s.doc, &s.model.state(), &plane).map_err(|e| bad(cmd, e.to_string()))?;
        super::sketch::face::capture_positions(s, &plane)?;
        supports.push(gusset::Support { plane, member });
    }
    let kind = FeatureKind::FrameGusset {
        supports,
        leg1: args.leg1.0,
        leg2: args.leg2.0,
        thickness: args.thickness.0,
        relief: args.relief.map(|v| v.0).unwrap_or_else(|| "0 mm".into()),
        clip: args.clip.map(|v| v.0).unwrap_or_else(|| "0 mm".into()),
        offset: args.offset.map(|v| v.0).unwrap_or_else(|| "0 mm".into()),
        placement: match args.placement {
            Placement::Center => gusset::Placement::Center,
            Placement::Positive => gusset::Placement::Positive,
            Placement::Negative => gusset::Placement::Negative,
        },
    };
    for (label, expr, kind) in kind.inputs() {
        super::features::check_expr(s, &expr, kind, cmd, label)?;
    }
    super::features::add_feature_in(s, p, kind, false)
}

//! End caps on the open ends of frame members.
use super::{CommandSpec, not_in_sketch};
use crate::params::bad;
use crate::schema::end_cap::{Direction, EndCapParams};
use crate::schema::features::FeatureResult;
use crate::schema::{Contract, TypedContract, output_schema_for};
use crate::{Result, Session};
use serde_json::{Value, json};
use solidcraft_doc::{FeatureKind, end_cap};

const CONTRACT: TypedContract<EndCapParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("frame.end_cap", "End Cap", crate::checked!("frame.end_cap", end_cap_cmd))
    .contract(&CONTRACT)
    .output(output_schema_for::<FeatureResult>)
    .at("WELDMENTS", "CREATE")
    .icon("end_cap")
    .enabled(not_in_sketch)
    .params("faces: [{face:[x,y,z],body,index,path?}] planar end faces of frame members, picked in world mm (1–256); thickness: length in design units or a parameter expression; offset?: inset of the plate from the profile's outer edge, a length; ratio?: the inset as a share of the thickness (default 0.5; give offset or ratio, not both); chamfer?: corner chamfer length along each edge from the corner (default 0; a rounded corner needs at least its rounding); direction?: outward|inward (default outward: past the end; inward: the member is cut back by the thickness and the cap takes its place). One separate body per face, named `<member> End Cap`; the outline is the profile's outer outline (round and rectangular bars and tubes, and other convex outlines of lines and arcs). Common name/body_names fields supported.")];

fn end_cap_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.end_cap";
    let args: EndCapParams = serde_json::from_value(p.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let allowed = CONTRACT.allowed_keys();
    if let Some(key) = p.as_object().and_then(|object| object.keys().find(|key| !allowed.contains(key.as_str()))) {
        return Err(bad(cmd, format!("unknown parameter `{key}`")));
    }
    if args.faces.is_empty() || args.faces.len() > end_cap::MAX_FACES {
        return Err(bad(cmd, format!("pick 1 to {} end faces", end_cap::MAX_FACES)));
    }
    if args.offset.is_some() && args.ratio.is_some() {
        return Err(bad(cmd, "give `offset` or `ratio`, not both"));
    }
    let mut ends = Vec::with_capacity(args.faces.len());
    for pick in args.faces {
        let mut value = json!({"face": pick.face.0, "body": pick.body, "index": pick.index});
        if let Some(path) = pick.path {
            value["path"] = json!(path);
        }
        let plane = super::sketch::face::capture(s, &value, cmd)?;
        // Pin each member's name to its line, so the cap finds its member after renames and
        // when other lines come and go.
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
        let member = end_cap::member(&s.doc, &s.model.state(), &plane).map_err(|e| bad(cmd, e.to_string()))?;
        super::sketch::face::capture_positions(s, &plane)?;
        ends.push(end_cap::End { plane, member });
    }
    let ratio = match (&args.offset, args.ratio) {
        (None, None) => Some(end_cap::DEFAULT_RATIO.to_string().into()),
        (_, r) => r.map(|r| r.0),
    };
    let kind = FeatureKind::FrameEndCap {
        ends,
        thickness: args.thickness.0,
        offset: args.offset.map(|v| v.0),
        ratio,
        chamfer: args.chamfer.map(|v| v.0).unwrap_or_else(|| "0 mm".into()),
        direction: match args.direction {
            Direction::Outward => end_cap::Direction::Outward,
            Direction::Inward => end_cap::Direction::Inward,
        },
    };
    for (label, expr, kind) in kind.inputs() {
        super::features::check_expr(s, &expr, kind, cmd, label)?;
    }
    super::features::add_feature_in(s, p, kind, false)
}

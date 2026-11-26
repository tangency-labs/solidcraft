//! Sketch blocks (#4069): sets of sketch curves that keep their shape while the set moves, turns
//! and, with scale free, scales as one. See `solidcraft_sketch::SketchBlock`.

use serde_json::{Value, json};
use solidcraft_sketch::{BlockMade, BlockScale};

use super::sketch::edit;
use super::{CommandSpec, in_sketch};
use crate::params::{bad, string_list};
use crate::schema::sketch_block::{BlockEditParams, BlockParams, BlockResult, ExplodeBlockParams, ExplodeBlockResult, SCALE_MESSAGE, scale_of};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Sel, Session};

const BLOCK_CONTRACT: TypedContract<BlockParams> = TypedContract::new();
const EXPLODE_CONTRACT: TypedContract<ExplodeBlockParams> = TypedContract::new();
const EDIT_CONTRACT: TypedContract<BlockEditParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sketch.block", "Make Block", crate::checked!("sketch.block", make_block))
        .in_sketch_part()
        .at("SKETCH", "MODIFY")
        .icon("c_block")
        .enabled(in_sketch)
        .from_selection(from_selection)
        .contract(&BLOCK_CONTRACT)
        .output(output_schema_for::<BlockResult>)
        .params(
            "curves: [ids] (from the selection: its curves, and a selected point as the origin); scale?: fixed|free (default fixed: the block keeps its size, 3 degrees of freedom; free: its proportions, one length dimension sets its size, 4); origin?: point id the block origin goes on (default the curves' bounding-box centre); name? (at most 200 characters). The curves keep their shape relative to each other and move as one; a construction line is added as the block's axis, from its origin along sketch +X: place the block with a coincident on the origin and turn it with parallel or an angle on the axis. Nothing moves. The constraints wholly inside the curves are held (kept, not solved) until Explode Block. Refused: curves in another block, projected curves, text, offset outputs, pattern geometry. Returns block, axis, origin, held, dof",
        ),
    CommandSpec::new("sketch.explode_block", "Explode Block", crate::checked!("sketch.explode_block", explode_block))
        .in_sketch_part()
        .icon("c_block")
        .enabled(in_sketch)
        .contract(&EXPLODE_CONTRACT)
        .output(output_schema_for::<ExplodeBlockResult>)
        .params("block: id. Turns the block back into plain geometry, its axis staying as a plain construction line. Its held constraints that hold where the geometry is now drive again (a free block's dimensions that are plain numbers scale with it first); the others (a turned block's horizontals, a resized block's dimensions driven by parameters) are removed. Returns curves (the former members), restored and dropped (constraint ids)"),
    CommandSpec::new("sketch.block_edit", "Edit Block", crate::checked!("sketch.block_edit", edit_block))
        .in_sketch_part()
        .icon("c_block")
        .enabled(in_sketch)
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<BlockResult>)
        .params("block: id; scale?: fixed|free; name? (empty removes it); add?: [curve ids]; remove?: [member ids]. Captures the block's shape again from where its curves are now; held constraints no longer inside it are released as Explode Block releases them (`dropped` lists those removed). Returns as sketch.block"),
];

/// Make Block's parameters from the session's selection: its sketch curves, and a selected point
/// as the origin.
fn from_selection(s: &Session) -> Option<Value> {
    super::sketch_store::active(s)?;
    let (mut curves, mut origin) = (Vec::new(), None);
    for sel in &s.selection {
        match sel {
            Sel::SketchCurve { id, .. } => curves.push(id.clone()),
            Sel::SketchPoint { id } if origin.is_none() => origin = Some(id.clone()),
            _ => return None,
        }
    }
    if curves.is_empty() {
        return None;
    }
    Some(match origin {
        Some(o) => json!({"curves": curves, "origin": o}),
        None => json!({"curves": curves}),
    })
}

fn scale_param(cmd: &str, p: &Value) -> Result<Option<BlockScale>> {
    match p.get("scale") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => scale_of(v).map(Some).ok_or_else(|| bad(cmd, format!("`scale` {SCALE_MESSAGE}"))),
    }
}

/// An optional text parameter, refused when it is not text.
fn text_param<'a>(cmd: &str, p: &'a Value, key: &str) -> Result<Option<&'a str>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(t)) => Ok(Some(t)),
        Some(_) => Err(bad(cmd, format!("`{key}` must be text"))),
    }
}

fn block_id<'a>(cmd: &str, p: &'a Value) -> Result<&'a str> {
    text_param(cmd, p, "block")?.ok_or_else(|| bad(cmd, "`block` must name a sketch block"))
}

fn reply(cmd: &str, value: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(value).map_err(|e| bad(cmd, e.to_string()))
}

fn block_reply(cmd: &str, made: BlockMade, dropped: Vec<String>, info: Value) -> Result<Value> {
    let dof = info.get("dof").and_then(Value::as_u64).map_or(0, |d| d as usize);
    reply(
        cmd,
        BlockResult {
            block: made.block,
            axis: made.axis,
            origin: made.origin,
            held: made.held,
            dropped,
            dof,
            sketch: info,
            param_warnings: Vec::new(),
        },
    )
}

fn make_block(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.block";
    let curves = string_list(cmd, p, "curves")?;
    if curves.is_empty() {
        return Err(bad(cmd, "`curves` must name the curves of the block"));
    }
    let scale = scale_param(cmd, p)?.unwrap_or_default();
    let origin = text_param(cmd, p, "origin")?;
    let name = text_param(cmd, p, "name")?.map(str::to_string);
    let (made, info) = edit(s, p, cmd, true, |sk, _| {
        let at = origin.map(|o| sk.resolve_point(o).ok_or_else(|| bad(cmd, format!("unknown point `{o}`")))).transpose()?;
        sk.make_block(&curves, scale, at, name).map_err(|e| bad(cmd, e.to_string()))
    })?;
    block_reply(cmd, made, Vec::new(), info)
}

fn explode_block(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.explode_block";
    let block = block_id(cmd, p)?;
    let model_edit = !s.sheet_sketching();
    let ((curves, released), info) = edit(s, p, cmd, true, |sk, doc| {
        let out = sk.explode_block(block).map_err(|_| bad(cmd, format!("unknown sketch block `{block}`")))?;
        if model_edit {
            doc.prune_model_params();
        }
        Ok(out)
    })?;
    reply(cmd, ExplodeBlockResult { curves, restored: released.restored, dropped: released.dropped, sketch: info, param_warnings: Vec::new() })
}

fn edit_block(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.block_edit";
    let block = block_id(cmd, p)?;
    let scale = scale_param(cmd, p)?;
    let name = text_param(cmd, p, "name")?.map(|n| Some(n.to_string()));
    let add = string_list(cmd, p, "add")?;
    let remove = string_list(cmd, p, "remove")?;
    let model_edit = !s.sheet_sketching();
    let ((made, released), info) = edit(s, p, cmd, true, |sk, doc| {
        if !sk.blocks.iter().any(|b| b.id == block) {
            return Err(bad(cmd, format!("unknown sketch block `{block}`")));
        }
        let out = sk.edit_block(block, scale, name, &add, &remove).map_err(|e| bad(cmd, e.to_string()))?;
        if model_edit {
            doc.prune_model_params();
        }
        Ok(out)
    })?;
    block_reply(cmd, made, released.dropped, info)
}

/// Refuse `curves` (ids) that are block members, for a command that changes curves one by one.
pub(super) fn refuse_members(sk: &solidcraft_sketch::Sketch, curves: &[String], cmd: &str) -> Result<()> {
    match curves.iter().find_map(|c| sk.block_refusal(c)) {
        Some(why) => Err(EngineError::BadParams { cmd: cmd.to_string(), msg: why }),
        None => Ok(()),
    }
}

/// Refuse a change to the constraint `id` that a block holds: it does not drive while the block
/// exists.
pub(super) fn refuse_held(sk: &solidcraft_sketch::Sketch, id: &str, cmd: &str) -> Result<()> {
    match sk.blocks.iter().find(|b| b.held.iter().any(|h| h == id)) {
        Some(b) => Err(bad(cmd, format!("`{id}` is held by block {}: explode it to change it", b.label()))),
        None => Ok(()),
    }
}

/// Refuse a change to the parameters `names` when one of them drives only dimensions that blocks
/// hold, directly, and nothing else names it: the change would do nothing until the blocks are
/// exploded.
pub(crate) fn refuse_held_parameters(doc: &solidcraft_doc::Document, names: &[&str], cmd: &str) -> Result<()> {
    for name in names {
        let mut users = doc.features.iter().filter_map(|f| match &f.kind {
            solidcraft_doc::FeatureKind::Sketch { sketch, .. } => Some(sketch),
            _ => None,
        });
        let (mut held, mut free) = (None, false);
        for sk in &mut users {
            for c in sk.constraints.iter().filter(|c| c.param.as_deref() == Some(*name)) {
                match sk.blocks.iter().find(|b| b.held.contains(&c.id)) {
                    Some(b) => held = held.or_else(|| Some((c.id.clone(), b.label().to_string()))),
                    None => free = true,
                }
            }
        }
        let named_elsewhere =
            doc.all_param_exprs().into_iter().any(|(other, e, _)| other != *name && solidcraft_doc::expr::references(&e).iter().any(|r| r == name));
        if let (Some((id, block)), false, false) = (held, free, named_elsewhere) {
            return Err(bad(cmd, format!("`{name}` drives only `{id}`, which block {block} holds: explode the block to change it")));
        }
    }
    Ok(())
}

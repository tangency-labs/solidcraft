//! Sketch blocks (#4069): sets of sketch curves that keep their shape while the set moves, turns
//! and, with scale free, scales as one. See `solidcraft_sketch::SketchBlock`.

use serde_json::{Value, json};
use solidcraft_sketch::{BlockMade, BlockScale};

use super::sketch::edit;
use super::{CommandSpec, in_sketch};
use crate::params::{bad, string_list};
use crate::schema::sketch_block::{
    BlockCloseParams, BlockCloseResult, BlockEditParams, BlockOpenParams, BlockOpenResult, BlockParams, BlockResult, ExplodeBlockParams,
    ExplodeBlockResult, SCALE_MESSAGE, scale_of,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Sel, Session};

const BLOCK_CONTRACT: TypedContract<BlockParams> = TypedContract::new();
const EXPLODE_CONTRACT: TypedContract<ExplodeBlockParams> = TypedContract::new();
const EDIT_CONTRACT: TypedContract<BlockEditParams> = TypedContract::new();
const OPEN_CONTRACT: TypedContract<BlockOpenParams> = TypedContract::new();
const CLOSE_CONTRACT: TypedContract<BlockCloseParams> = TypedContract::new();

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
            "curves: [ids] (from the selection: its curves, and a selected point as the origin); scale?: fixed|free (default fixed: the block keeps its size, 3 degrees of freedom; free: its proportions, one length dimension sets its size, 4); origin?: point id the block origin goes on (default the curves' bounding-box centre); name? (at most 200 characters); show_constraints?: bool (default false: while the block is closed, the constraints and dimensions it holds are hidden). The curves keep their shape relative to each other and move as one; a construction line is added as the block's axis, from its origin along sketch +X: place the block with a coincident on the origin and turn it with parallel or an angle on the axis. Nothing moves. The constraints wholly inside the curves are held (kept, not solved) until Explode Block. Refused: curves in another block, projected curves, text, offset outputs, pattern geometry. Returns block, axis, origin, held, dof",
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
        .params("block: id; scale?: fixed|free; name? (empty removes it); add?: [curve ids]; remove?: [member ids]; show_constraints?: bool (draw the constraints and dimensions the block holds while it is closed, faded; default as it is, and hidden on a new block). Captures the block's shape again from where its curves are now; held constraints no longer inside it are released as Explode Block releases them (`dropped` lists those removed). Refused while the block is open (sketch.block_open), except show_constraints given alone, which changes nothing else. Returns as sketch.block"),
    CommandSpec::new("sketch.block_open", "Edit Contents", crate::checked!("sketch.block_open", open_block))
        .in_sketch_part()
        .icon("c_block")
        .enabled(in_sketch)
        .contract(&OPEN_CONTRACT)
        .output(output_schema_for::<BlockOpenResult>)
        .params("block: id. Opens the block to edit its curves in place, without exploding it: the block stays where it is (its axis, and everything outside the block, is held where it is while it is open), the constraints it held drive again (those that no longer hold where the geometry is, as after a turn or a resize, are removed: `dropped`), and its curves can be trimmed, filleted, dimensioned, dragged one by one, deleted, and drawn to. One block is open at a time: another open block is closed first (`closed`). Finishing the sketch closes it. Returns block, restored, dropped, closed?"),
    CommandSpec::new("sketch.block_close", "Close Block", crate::checked!("sketch.block_close", close_block))
        .in_sketch_part()
        .icon("c_block")
        .enabled(in_sketch)
        .contract(&CLOSE_CONTRACT)
        .output(output_schema_for::<BlockCloseResult>)
        .params("block?: id (default the sketch's open block). Captures the open block's new shape into the same block, relative to its axis, which did not move: the curves drawn while it was open join it (`added`; text, projected curves, offset outputs and pattern geometry stay outside: `left_out`), and the constraints wholly inside it are held again. Constraints from outside onto its axis or its points stay as they are. A block left with no curve goes with its axis (`removed`). Returns block, held, added, left_out?, removed?, dof"),
];

/// Make Block's parameters from the session's selection: its sketch curves, and a selected point
/// as the origin.
fn from_selection(s: &Session) -> Option<Value> {
    let receiver = if s.sheet_sketching() { s.active_sheet_sketch.as_ref().map(|entry| entry.sketch) } else { super::sketch_store::active(s) };
    receiver?;
    let (mut curves, mut origin) = (Vec::new(), None);
    for sel in &s.selection {
        match sel {
            Sel::SketchCurve { id, sketch } if sketch.is_none() || *sketch == receiver => curves.push(id.clone()),
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

/// `show_constraints`, refused when it is not true or false.
fn show_param(cmd: &str, p: &Value) -> Result<Option<bool>> {
    match p.get("show_constraints") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, "`show_constraints` must be true or false")),
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
    text_param(cmd, p, "block")?.ok_or_else(|| bad(cmd, "`block` is required"))
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
    let show = show_param(cmd, p)?.unwrap_or(false);
    let (made, info) = edit(s, p, cmd, true, |sk, _| {
        let at = origin.map(|o| sk.resolve_point(o).ok_or_else(|| bad(cmd, format!("unknown point `{o}`")))).transpose()?;
        let made = sk.make_block(&curves, scale, at, name).map_err(|e| bad(cmd, e.to_string()))?;
        if show {
            sk.set_block_show_held(&made.block, true).map_err(|e| bad(cmd, e.to_string()))?;
        }
        Ok(made)
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
    if model_edit {
        s.doc_mut().prune_model_params();
    }
    reply(cmd, ExplodeBlockResult { curves, restored: released.restored, dropped: released.dropped, sketch: info, param_warnings: Vec::new() })
}

fn edit_block(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.block_edit";
    let block = block_id(cmd, p)?;
    let scale = scale_param(cmd, p)?;
    let name = text_param(cmd, p, "name")?.map(|n| Some(n.to_string()));
    let add = string_list(cmd, p, "add")?;
    let remove = string_list(cmd, p, "remove")?;
    let show = show_param(cmd, p)?;
    let only_show = show.is_some() && scale.is_none() && name.is_none() && add.is_empty() && remove.is_empty();
    let model_edit = !s.sheet_sketching();
    let ((made, released), info) = edit(s, p, cmd, true, |sk, doc| {
        if !sk.blocks.iter().any(|b| b.id == block) {
            return Err(bad(cmd, format!("unknown sketch block `{block}`")));
        }
        let out = match show {
            Some(show) if only_show => (sk.set_block_show_held(block, show).map_err(|e| bad(cmd, e.to_string()))?, Default::default()),
            _ => {
                let out = sk.edit_block(block, scale, name, &add, &remove).map_err(|e| bad(cmd, e.to_string()))?;
                if let Some(show) = show {
                    sk.set_block_show_held(block, show).map_err(|e| bad(cmd, e.to_string()))?;
                }
                out
            }
        };
        if model_edit {
            doc.prune_model_params();
        }
        Ok(out)
    })?;
    if model_edit {
        s.doc_mut().prune_model_params();
    }
    block_reply(cmd, made, released.dropped, info)
}

fn open_block(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.block_open";
    let block = block_id(cmd, p)?;
    let model_edit = !s.sheet_sketching();
    // A block stays open only while its sketch is edited.
    if model_edit && super::sketch_store::active(s) != Some(super::sketch_store::target(s, p, cmd)?) {
        return Err(bad(cmd, "Edit Contents opens a block of the sketch being edited: edit that sketch first"));
    }
    let ((released, closed), info) = edit(s, p, cmd, true, |sk, doc| {
        if !sk.blocks.iter().any(|b| b.id == block) {
            return Err(bad(cmd, format!("unknown sketch block `{block}`")));
        }
        let out = sk.open_block(block).map_err(|e| bad(cmd, e.to_string()))?;
        if model_edit {
            doc.prune_model_params();
        }
        Ok(out)
    })?;
    if model_edit {
        s.doc_mut().prune_model_params();
    }
    reply(
        cmd,
        BlockOpenResult {
            block: block.to_string(),
            restored: released.restored,
            dropped: released.dropped,
            closed,
            sketch: info,
            param_warnings: Vec::new(),
        },
    )
}

fn close_block(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.block_close";
    let given = text_param(cmd, p, "block")?.map(str::to_string);
    let model_edit = !s.sheet_sketching();
    let ((block, closed), info) = edit(s, p, cmd, false, |sk, doc| {
        let block = match given {
            Some(b) if sk.blocks.iter().any(|k| k.id == b) => b,
            Some(b) => return Err(bad(cmd, format!("unknown sketch block `{b}`"))),
            None => sk.open_block_now().map(|b| b.id.clone()).ok_or_else(|| bad(cmd, "no block of the sketch is open"))?,
        };
        let closed = sk.close_block(&block).map_err(|e| bad(cmd, e.to_string()))?;
        if model_edit {
            doc.prune_model_params();
        }
        Ok((block, closed))
    })?;
    if model_edit {
        s.doc_mut().prune_model_params();
    }
    let dof = info.get("dof").and_then(Value::as_u64).map_or(0, |d| d as usize);
    reply(
        cmd,
        BlockCloseResult {
            block,
            removed: closed.made.is_none(),
            held: closed.made.map(|m| m.held).unwrap_or_default(),
            added: closed.added,
            left_out: closed.left_out,
            dof,
            sketch: info,
            param_warnings: Vec::new(),
        },
    )
}

/// Close every block open in the design's model sketches, in place (not an undo step of its
/// own): leaving a sketch closes its open block.
pub(crate) fn close_open_blocks(doc: &mut solidcraft_doc::Document) {
    let open: Vec<u64> = doc
        .features
        .iter()
        .filter(|f| matches!(&f.kind, solidcraft_doc::FeatureKind::Sketch { sketch, .. } if sketch.open_block_now().is_some()))
        .map(|f| f.id)
        .collect();
    for id in open {
        close_in(doc, id);
    }
}

/// Close the open block of the sketch being edited, as leaving the sketch does.
pub(crate) fn close_on_leaving(s: &mut Session) {
    let Some(id) = super::sketch_store::active(s) else { return };
    if !super::sketch_store::read(&s.doc, id).is_ok_and(|sk| sk.open_block_now().is_some()) {
        return;
    }
    let mut doc = (*s.doc).clone();
    if close_in(&mut doc, id) {
        s.set_doc(doc);
    }
}

/// Close the open blocks of model sketch `id` in `doc`; false when there is no such sketch.
fn close_in(doc: &mut solidcraft_doc::Document, id: u64) -> bool {
    let Ok(sk) = super::sketch_store::read(doc, id) else { return false };
    let mut sk = sk.clone();
    sk.close_open_blocks();
    super::sketch_store::write(doc, id, sk).is_ok()
}

/// What an edit of a sketch with blocks must not do, checked from the sketch as it was before the
/// edit: add (or make driving) a constraint wholly inside a closed block, and, while a block is
/// open, change anything outside it.
pub(super) struct BlockGuard {
    pre: std::collections::HashSet<String>,
    was_driven: std::collections::HashSet<String>,
    /// The sketch before the edit, while a block is open in it.
    open: Option<solidcraft_sketch::Sketch>,
}

impl BlockGuard {
    /// The guard for an edit of `sk`; none when it has no block.
    pub(super) fn new(sk: &solidcraft_sketch::Sketch) -> Option<BlockGuard> {
        if sk.blocks.is_empty() {
            return None;
        }
        Some(BlockGuard {
            pre: sk.constraints.iter().map(|c| c.id.clone()).collect(),
            was_driven: sk.constraints.iter().filter(|c| c.driven).map(|c| c.id.clone()).collect(),
            open: sk.open_block_now().is_some().then(|| sk.clone()),
        })
    }

    /// Refuse what the edit that made `sk` did against its blocks.
    pub(super) fn check(&self, sk: &solidcraft_sketch::Sketch, cmd: &str) -> Result<()> {
        if let Some(before) = &self.open
            && let Some(id) = sk.outside_change(before)
        {
            let block = before.open_block_now().map(|b| b.label().to_string()).unwrap_or_default();
            return Err(bad(cmd, format!("`{id}` is outside block {block}, which is open for editing its contents: close the block first")));
        }
        let changed: Vec<&solidcraft_sketch::Constraint> =
            sk.constraints.iter().filter(|c| !c.driven && (!self.pre.contains(&c.id) || self.was_driven.contains(&c.id))).collect();
        if changed.is_empty() {
            return Ok(());
        }
        match sk.loose_inside_closed(changed) {
            Some((c, block)) => Err(bad(
                cmd,
                format!("`{c}` would lie wholly inside block {block}, which keeps its shape: open the block with Edit Contents to change its shape"),
            )),
            None => Ok(()),
        }
    }
}

/// Refuse a delete that would take the axis of the open block: it holds the block where it is.
pub(super) fn refuse_open_axis(sk: &solidcraft_sketch::Sketch, curves: &[usize], points: &[usize], cmd: &str) -> Result<()> {
    let Some(b) = sk.open_block_now() else { return Ok(()) };
    let axis = sk.curve_index(&b.axis);
    let ends = axis.and_then(|a| sk.curves.get(a)).and_then(|c| c.kind.ends());
    if axis.is_some_and(|a| curves.contains(&a)) || ends.is_some_and(|(o, x)| points.contains(&o) || points.contains(&x)) {
        return Err(bad(
            cmd,
            format!("`{}` is the axis of block {}, which is open for editing its contents: close the block first", b.axis, b.label()),
        ));
    }
    Ok(())
}

/// Close every block left open in a sketch nobody is editing, in place, as part of the command
/// that ran: a sketch's open block lasts only while the sketch is edited, whichever way the edit
/// ended (Finish, Finish with AutoConstrain, a sheet sketch's finish, an undo or redo that left
/// the sketch).
pub(crate) fn close_unedited(s: &mut Session) {
    let active = s.active_sketch;
    let sheet = s.active_sheet_sketch.as_ref().map(|e| (e.drawing, e.sketch));
    let model: Vec<u64> = s
        .doc
        .features
        .iter()
        .filter(|f| {
            Some(f.id) != active && matches!(&f.kind, solidcraft_doc::FeatureKind::Sketch { sketch, .. } if sketch.open_block_now().is_some())
        })
        .map(|f| f.id)
        .collect();
    let drawn = s.doc.drawings.iter().any(|d| d.sketches.iter().any(|r| sheet != Some((d.id, r.id)) && r.sketch.open_block_now().is_some()));
    if model.is_empty() && !drawn {
        return;
    }
    let doc = s.doc_mut();
    for id in model {
        close_in(doc, id);
    }
    if drawn {
        for d in &mut doc.drawings {
            let id = d.id;
            if !d.sketches.iter().any(|r| sheet != Some((id, r.id)) && r.sketch.open_block_now().is_some()) {
                continue;
            }
            for r in &mut std::sync::Arc::make_mut(d).sketches {
                if sheet != Some((id, r.id)) && r.sketch.open_block_now().is_some() {
                    std::sync::Arc::make_mut(r).sketch.close_open_blocks();
                }
            }
        }
    }
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
        Some(b) => Err(bad(cmd, format!("`{id}` is held by block {}: open the block with Edit Contents to change it", b.label()))),
        None => Ok(()),
    }
}

/// Refuse a change to the parameters `names` when one of them drives only dimensions that blocks
/// hold, directly, and nothing else names it: the change would do nothing until the blocks are
/// opened with Edit Contents or exploded.
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
        let names = |e: &str| solidcraft_doc::expr::references(e).iter().any(|r| r == name);
        // A sheet dimension keeps its own expression, which can name the parameter.
        let on_a_sheet = doc
            .drawings
            .iter()
            .flat_map(|d| &d.sketches)
            .any(|r| r.sketch.constraints.iter().any(|c| c.param.as_deref().is_some_and(names) && !r.sketch.is_held(&c.id)));
        let named_elsewhere = on_a_sheet || doc.all_param_exprs().into_iter().any(|(other, e, _)| other != *name && names(&e));
        if let (Some((id, block)), false, false) = (held, free, named_elsewhere) {
            return Err(bad(cmd, format!("`{name}` drives only `{id}`, which block {block} holds: open the block with Edit Contents to change it")));
        }
    }
    Ok(())
}

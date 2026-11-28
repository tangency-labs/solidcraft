//! Draw and edit Grease Pencil strokes stored in model sketches.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_doc::{IDENTITY, Mat};
use solidcraft_kernel::{affine, points};
use solidcraft_sketch::{DEFAULT_INK_DIAMETER, MAX_FIT_POINTS, Sketch, WirePoint, WireSpace};

use super::{edit, edit_drawn, inverse_owner_frame, owner_frame, place_points, point_values, points3, reply, sketch_store};
use crate::cmd::{CommandSpec, component};
use crate::params::bad;
use crate::schema::inspect::SketchTarget;
use crate::schema::sketch_3d::{
    Curves3dResult, EditInkParams, EraseInkParams, GreasePencilParams, InkEditResult, InkStroke, Move3dResult, MoveInkParams, PasteInkParams,
    PasteInkResult, ScaleInkParams,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sketch.grease_pencil", "Grease Pencil", crate::checked!("sketch.grease_pencil", grease_pencil))
        .icon("spline")
        .key("G")
        .contract(&TypedContract::<GreasePencilParams>::new())
        .output(output_schema_for::<Curves3dResult>)
        .params("points: [[x,y,z]…] world fit points in mm; diameter?: brush diameter in mm (default 1); color?: sRGB bytes [r,g,b]; sketch?: id or name (default: active). Adds one freehand stroke and enables 3D Sketch."),
    CommandSpec::new("sketch.paste_ink", "Paste Strokes", crate::checked!("sketch.paste_ink", paste_ink))
        .icon("paste")
        .contract(&TypedContract::<PasteInkParams>::new())
        .output(output_schema_for::<PasteInkResult>)
        .params("strokes: [{points: world fit points in mm, diameter: width in mm, color?: sRGB bytes}]; sketch?: target (default: active sketch, or a new sketch). Pastes and selects the strokes in one undo step."),
    CommandSpec::new("sketch.erase_ink", "Grease Eraser", crate::checked!("sketch.erase_ink", erase_ink))
        .icon("delete")
        .key("Shift+G")
        .contract(&TypedContract::<EraseInkParams>::new())
        .output(output_schema_for::<InkEditResult>)
        .params("strokes: [{sketch: id or name, wire: stroke id}]. Erases the strokes in one undo step."),
    CommandSpec::new("sketch.edit_ink", "Edit Stroke", crate::checked!("sketch.edit_ink", edit_ink_style))
        .icon("appearance")
        .contract(&TypedContract::<EditInkParams>::new())
        .output(output_schema_for::<InkEditResult>)
        .params("strokes: [{sketch: id or name, wire: stroke id}], diameter?: width in mm, color?: sRGB bytes [r,g,b]. Give at least one style change."),
    CommandSpec::new("sketch.move_ink", "Move Grease Pencil Stroke", crate::checked!("sketch.move_ink", move_ink))
        .icon("sketch_move")
        .contract(&TypedContract::<MoveInkParams>::new())
        .output(output_schema_for::<Move3dResult>)
        .params("strokes: [{sketch: id or name, wire: stroke id}]; one of by: [dx,dy,dz] or matrix: rigid world transform. One undo step."),
    CommandSpec::new("sketch.scale_ink", "Scale Strokes", crate::checked!("sketch.scale_ink", scale_ink))
        .icon("scale")
        .contract(&TypedContract::<ScaleInkParams>::new())
        .output(output_schema_for::<Move3dResult>)
        .params("strokes: [{sketch: id or name, wire: stroke id}], factor: 0.01 to 100. Uniformly scales strokes and their thickness around the selection center in one undo step."),
];

fn grease_pencil(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.grease_pencil";
    let color = p
        .get("color")
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value::<[u8; 3]>(value.clone()).map_err(|_| bad(cmd, "`color` must be [r, g, b] with values from 0 to 255")))
        .transpose()?;
    let diameter = p
        .get("diameter")
        .filter(|v| !v.is_null())
        .map(|v| v.as_f64().filter(|w| (1e-9..=1e9).contains(w)).ok_or_else(|| bad(cmd, "`diameter` must be from 1e-9 to 1e9 mm")))
        .transpose()?
        .unwrap_or(DEFAULT_INK_DIAMETER);
    let inverse = inverse_owner_frame(s, p, cmd)?;
    let inputs = point_values(cmd, p, 2)?;
    let points = points3(s, cmd, &inputs, &inverse)?;
    let fitted: Vec<_> = points.iter().map(|(point, _)| *point).collect();
    let ((id, constraints), info) = edit(s, p, cmd, false, |sk, _| {
        let id = add_ink(sk, &fitted, diameter, color)?;
        // Ink has no automatic joints. Only positions explicitly set by expressions are held.
        let held: Vec<_> = points
            .into_iter()
            .enumerate()
            .filter_map(|(index, (_, expr))| expr.map(|expr| (WirePoint { wire: id.clone(), index }, Some(expr))))
            .collect();
        let constraints = place_points(sk, cmd, &held)?;
        Ok((id, constraints))
    })?;
    reply(cmd, Curves3dResult { curves: vec![id], sketch: info, constraints, created: None, param_warnings: Vec::new() })
}

fn add_ink(sketch: &mut Sketch, points: &[Vec3], diameter: f64, color: Option<[u8; 3]>) -> Result<String> {
    let id = sketch.add_drawn_wire_in(points, WireSpace::Component)?;
    sketch.view.three_d = true;
    if let Some(wire) = sketch.wires.last_mut() {
        wire.ink_diameter = Some(diameter);
        wire.ink_color = color;
    }
    Ok(id)
}

fn paste_ink(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.paste_ink";
    let params = PasteInkParams::deserialize(p).map_err(|err| bad(cmd, err.to_string()))?;
    let existing = if params.sketch.is_some() || sketch_store::active(s).is_some() { Some(sketch_store::target(s, p, cmd)?) } else { None };
    let (mut sketch, owner) = if let Some(id) = existing {
        (sketch_store::read(&s.doc, id)?.clone(), owner_frame(s, id, cmd)?)
    } else {
        (Sketch::new(), s.active_component_frame())
    };
    let inverse = affine::inverse(&owner).map_err(|_| bad(cmd, "the sketch owner placement is not invertible"))?;
    if !(1..=crate::params::MAX_STRING_LIST).contains(&params.strokes.len()) {
        return Err(bad(cmd, "give at least one stroke"));
    }
    let mut strokes = Vec::with_capacity(params.strokes.len());
    for stroke in params.strokes {
        if !(2..=MAX_FIT_POINTS).contains(&stroke.points.len()) || !(1e-9..=1e9).contains(&stroke.diameter) {
            return Err(bad(cmd, "a stroke needs 2 to 500 points and a valid diameter"));
        }
        let points: Vec<_> = stroke.points.into_iter().map(|p| Vec3::from(affine::apply_point(&inverse, p))).collect();
        strokes.push(add_ink(&mut sketch, &points, stroke.diameter, stroke.color)?);
    }
    let sketch = if let Some(id) = existing {
        sketch_store::write(s.doc_mut(), id, sketch)?;
        id
    } else {
        let plane = solidcraft_doc::PlaneRef::Origin { name: "XY".into() };
        let component = s.active_component;
        let feature = solidcraft_doc::FeatureKind::Sketch { plane, sketch };
        let name = s.doc.unique_name("SketchGP");
        let id = s.doc_mut().add_feature(feature, Some(&name))?;
        if let Some(feature) = s.doc_mut().feature_mut(id) {
            feature.component = component;
        }
        id
    };
    s.selection = strokes.iter().map(|id| crate::Sel::ExternalSketchCurve { sketch, id: id.clone() }).collect();
    s.selection_all_bodies = false;
    reply(cmd, PasteInkResult { sketch, strokes })
}

fn move_ink(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.move_ink";
    let params = MoveInkParams::deserialize(p).map_err(|err| bad(cmd, err.to_string()))?;
    let transform = match (params.matrix, params.by) {
        (Some(matrix), None) => component::matrix::rigid(&matrix.0, 1e-9, cmd)?,
        (None, Some(delta)) => {
            if !points::points_meet([0.0; 3], delta, 1e6).unwrap_or(false) {
                return Err(bad(cmd, "`by` must be a world offset within 1,000,000 mm"));
            }
            affine::translation(delta).map_err(|e| bad(cmd, e.to_string()))?
        }
        _ => return Err(bad(cmd, "give one of `by` or `matrix`")),
    };
    transform_strokes(s, cmd, params.strokes, transform, 1.0)
}

fn scale_ink(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.scale_ink";
    let params = ScaleInkParams::deserialize(p).map_err(|err| bad(cmd, err.to_string()))?;
    if !(0.01..=100.0).contains(&params.factor) {
        return Err(bad(cmd, "`factor` must be from 0.01 to 100"));
    }
    transform_strokes(s, cmd, params.strokes, IDENTITY, params.factor)
}

fn stroke_groups(s: &Session, strokes: Vec<InkStroke>, cmd: &str) -> Result<BTreeMap<u64, BTreeSet<String>>> {
    if !(1..=crate::params::MAX_STRING_LIST).contains(&strokes.len()) {
        return Err(bad(cmd, "`strokes` must be a nonempty list of {sketch, wire}"));
    }
    let mut groups = BTreeMap::<_, BTreeSet<_>>::new();
    for InkStroke { sketch, wire } in strokes {
        let target = match sketch {
            SketchTarget::Id(id) => Value::from(id),
            SketchTarget::Name(name) => Value::from(name),
        };
        let sketch = sketch_store::target(s, &json!({"sketch": target}), cmd)?;
        let sk = sketch_store::read(&s.doc, sketch)?;
        if !sk.wire_index(&wire).and_then(|i| sk.wires.get(i)).is_some_and(|w| w.ink_diameter.is_some()) {
            return Err(bad(cmd, format!("`{wire}` is not a Grease Pencil stroke in that sketch")));
        }
        groups.entry(sketch).or_default().insert(wire);
    }
    Ok(groups)
}

fn edit_ink_style(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.edit_ink";
    let style = EditInkParams::deserialize(p).map_err(|err| bad(cmd, err.to_string()))?;
    if let Some(diameter) = style.diameter
        && !(1e-9..=1e9).contains(&diameter)
    {
        return Err(bad(cmd, "give a diameter from 0.000000001 to 1000000000 mm"));
    }
    if style.diameter.is_none() && style.color.is_none() {
        return Err(bad(cmd, "give a width or color"));
    }
    let mut strokes = 0;
    for (id, ids) in stroke_groups(s, style.strokes, cmd)? {
        let mut sk = sketch_store::read(&s.doc, id)?.clone();
        for wire in sk.wires.iter_mut().filter(|wire| ids.contains(&wire.id)) {
            if let Some(color) = style.color {
                wire.ink_color = Some(color);
            }
            if let Some(diameter) = style.diameter {
                wire.ink_diameter = Some(diameter);
            }
            strokes += 1;
        }
        sketch_store::write(s.doc_mut(), id, sk)?;
    }
    reply(cmd, InkEditResult { strokes })
}

fn erase_ink(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.erase_ink";
    let params = EraseInkParams::deserialize(p).map_err(|err| bad(cmd, err.to_string()))?;
    let mut strokes = 0;
    for (id, ids) in stroke_groups(s, params.strokes, cmd)? {
        strokes += ids.len();
        delete_strokes(s, id, &ids.into_iter().collect::<Vec<_>>())?;
    }
    reply(cmd, InkEditResult { strokes })
}

/// Ink-only removal can skip solving the planar sketch. Keep even an empty sketch if another
/// feature uses it: erasing ink is never a request to delete the dependent feature.
pub(in crate::cmd) fn delete_strokes(s: &mut Session, id: u64, ids: &[String]) -> Result<()> {
    let mut sk = sketch_store::read(&s.doc, id)?.clone();
    sk.remove_wires(ids);
    let empty = crate::edit_start::empty_geometry(&sk)
        && sk.links.is_empty()
        && sk.constraints.is_empty()
        && sk.wire_constraints.is_empty()
        && sk.patterns.is_empty()
        && sk.offsets.is_empty()
        && sk.freeform_offsets.is_empty();
    if empty
        && !s.doc.feature(id).is_some_and(|sketch| {
            s.doc.features.iter().any(|f| f.id != id && (f.kind.input_sketches().contains(&id) || f.kind.names_feature(id, &sketch.name)))
        })
    {
        if s.active_sketch == Some(id) {
            // Close via the shared receiver to restore the marker and notify the view.
            // Finish sees the original stroke data and therefore retains the sketch.
            s.finish_edit_receivers()?;
        }
        s.doc_mut().delete_feature(id)?;
    } else {
        sketch_store::write(s.doc_mut(), id, sk)?;
    }
    Ok(())
}

fn transform_strokes(s: &mut Session, cmd: &str, strokes: Vec<InkStroke>, world_move: Mat, factor: f64) -> Result<Value> {
    let groups =
        stroke_groups(s, strokes, cmd)?.into_iter().map(|(id, wires)| Ok((id, wires, owner_frame(s, id, cmd)?))).collect::<Result<Vec<_>>>()?;
    let world_move = if factor == 1.0 {
        world_move
    } else {
        // Place the evaluated strokes through the same frames used to edit them below.
        let model = s.model.state();
        let sketches = groups
            .iter()
            .map(|(id, ids, owner)| {
                let sketch = model.sketch(*id).ok_or_else(|| bad(cmd, "the sketch is not evaluated"))?;
                Ok((&sketch.sketch, ids, owner))
            })
            .collect::<Result<Vec<_>>>()?;
        let points = sketches.into_iter().flat_map(|(sketch, ids, owner)| {
            sketch.wires.iter().filter(move |wire| ids.contains(&wire.id)).flat_map(move |wire| {
                wire.pts.iter().map(
                    move |&point| {
                        if wire.space == WireSpace::Component { affine::apply_point(owner, point.into()) } else { point.into() }
                    },
                )
            })
        });
        let center = points::point_bounds_center(points).ok_or_else(|| bad(cmd, "the strokes have no evaluated points"))?;
        affine::scaling(center, factor).map_err(|e| bad(cmd, e.to_string()))?
    };
    let mut moved = 0;
    let mut sketch = Value::Null;
    for (id, wires, owner) in groups {
        let picks: Vec<_> = wires.into_iter().map(|id| (id, None)).collect();
        let target = json!({"sketch": id});
        let inverse = affine::inverse(&owner).map_err(|_| bad(cmd, "the sketch owner placement is not invertible"))?;
        let world_from_local = affine::then(&owner, &world_move).map_err(|e| bad(cmd, e.to_string()))?;
        let local_move = affine::then(&world_from_local, &inverse).map_err(|e| bad(cmd, e.to_string()))?;
        let (count, solved) = edit_drawn(s, &target, cmd, &picks, factor, |space, q| {
            Vec3::from(affine::apply_point(if space == WireSpace::Component { &local_move } else { &world_move }, q.into()))
        })?;
        moved += count;
        sketch = solved;
    }
    reply(cmd, Move3dResult { moved, curves: Vec::new(), wire: None, index: None, sketch, param_warnings: Vec::new() })
}

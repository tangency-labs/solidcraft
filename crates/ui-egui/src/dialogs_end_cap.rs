//! End caps on frame members: the faces to close and the plate's sizes. The command checks the
//! faces and the plate.
use crate::{
    SolveApp,
    dialogs::{Kind, shown_expr},
    selection::{PLANAR_FACES, SelInput},
    ui_kit::{choice_row, expr_row},
};
use serde_json::{Map, Value, json};
use solidcraft_engine::{
    Sel,
    doc::{FeatureKind, end_cap::Direction, expr::Kind as ValueKind},
};

#[derive(Clone, Debug)]
pub struct EndCap {
    pub thickness: String,
    /// 0: outward, 1: inward.
    pub direction: usize,
    /// 0: the inset as a ratio of the thickness, 1: as a length.
    pub inset_kind: usize,
    pub ratio: String,
    pub offset: String,
    pub chamfer: String,
}

impl Default for EndCap {
    fn default() -> Self {
        Self { thickness: "6 mm".into(), direction: 0, inset_kind: 0, ratio: "0.5".into(), offset: "3 mm".into(), chamfer: "0 mm".into() }
    }
}

pub fn start(_: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "frame.end_cap").then(|| (Kind::EndCap(EndCap::default()), vec![SelInput::new("ui.end_cap.faces", PLANAR_FACES, true)]))
}

pub fn rows(app: &SolveApp, ui: &mut egui::Ui, c: &mut EndCap) -> bool {
    let mut enter = expr_row(ui, app.tr("ui.thickness"), &mut c.thickness);
    choice_row(ui, app.tr("ui.end_cap.direction"), "end_cap_direction", &mut c.direction, &[app.tr("ui.end_cap.outward"), app.tr("ui.end_cap.inward")]);
    choice_row(ui, app.tr("ui.end_cap.inset_kind"), "end_cap_inset_kind", &mut c.inset_kind, &[app.tr("ui.end_cap.ratio"), app.tr("ui.end_cap.length")]);
    if c.inset_kind == 0 {
        enter |= expr_row(ui, app.tr("ui.end_cap.inset_ratio"), &mut c.ratio);
    } else {
        enter |= expr_row(ui, app.tr("ui.end_cap.inset"), &mut c.offset);
    }
    enter |= expr_row(ui, app.tr("ui.end_cap.chamfer"), &mut c.chamfer);
    enter
}

pub fn commands(app: &SolveApp, c: &EndCap, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<Vec<(String, Value)>, String> {
    let faces: Vec<Value> = crate::selection::items(inputs, 0)
        .iter()
        .filter_map(|s| match s {
            Sel::Face { body, index, point } => Some(json!({"face": point, "body": body, "index": index})),
            _ => None,
        })
        .collect();
    if faces.is_empty() {
        return Err(app.tr("error.end_cap_faces").into());
    }
    let direction = if c.direction == 1 { "inward" } else { "outward" };
    let mut args = json!({"faces": faces, "thickness": c.thickness, "direction": direction, "chamfer": c.chamfer});
    if let Value::Object(args) = &mut args {
        if c.inset_kind == 0 {
            args.insert("ratio".into(), json!(c.ratio));
        } else {
            args.insert("offset".into(), json!(c.offset));
        }
        for (k, v) in extra {
            args.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(vec![("frame.end_cap".into(), args)])
}

pub fn for_feature(app: &SolveApp, kind: &FeatureKind) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let FeatureKind::FrameEndCap { ends, thickness, offset, ratio, chamfer, direction } = kind else { return None };
    let (_, mut inputs) = start(app, "frame.end_cap")?;
    let length = |e: &solidcraft_engine::doc::Expr| shown_expr(e, ValueKind::Length);
    let c = EndCap {
        thickness: length(thickness),
        direction: usize::from(*direction == Direction::Inward),
        inset_kind: usize::from(offset.is_some()),
        ratio: ratio.as_ref().map_or_else(|| "0.5".into(), |e| shown_expr(e, ValueKind::Unitless)),
        offset: offset.as_ref().map_or_else(|| "3 mm".into(), length),
        chamfer: length(chamfer),
    };
    let state = app.session.model.state();
    let world = app.session.world_state();
    let mut items = Vec::new();
    for end in ends {
        if let Some(anchor) = solidcraft_engine::doc::face_anchor::plane_face_anchor(&end.plane)
            && let Some(body) = state.body(&anchor.target.body)
        {
            let name = app.session.doc.world_body_name(&state, &body.name, anchor.component, &anchor.path);
            let indices = app.session.doc.configuration_face_indices(body, &anchor.target).ok();
            if let Some(body) = world.body(&name) {
                let mesh = body.mesh();
                if let Some((tri, index)) =
                    mesh.triangles.iter().zip(&mesh.tri_face).find(|(_, i)| indices.as_ref().is_some_and(|is| is.contains(&(**i as usize))))
                    && let Some([a, b, c]) = mesh.tri(tri)
                {
                    items.push(Sel::Face { body: name, index: *index as usize, point: (a + b + c) / 3. });
                }
            }
        }
    }
    if let Some(input) = inputs.first_mut() {
        input.items = items;
    }
    Some((Kind::EndCap(c), inputs, Map::new()))
}

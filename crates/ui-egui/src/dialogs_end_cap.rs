//! End caps on frame members: the faces to close and the plate's sizes. The command checks the
//! faces and the plate.
use crate::{
    SolveApp,
    dialogs::{Kind, check, shown_expr},
    selection::{PLANAR_FACES, SelInput},
    ui_kit::{IconSegment, choice_row, expr_row, row, segmented_icons},
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
    /// The plate set in from the profile's outside; off: flush with it.
    pub use_offset: bool,
    /// 0: the inset as a ratio of the thickness, 1: as a length.
    pub inset_kind: usize,
    pub ratio: String,
    pub offset: String,
    pub chamfer: String,
    /// Editing: the saved ends (1-based) no face could be found for. The faces start empty then,
    /// so OK never drops those ends unasked: what the user picks again is the new set of ends.
    pub unresolved: Vec<usize>,
}

impl Default for EndCap {
    fn default() -> Self {
        Self {
            thickness: "6 mm".into(),
            direction: 0,
            use_offset: false,
            inset_kind: 0,
            ratio: "0.5".into(),
            offset: "3 mm".into(),
            chamfer: "0 mm".into(),
            unresolved: Vec::new(),
        }
    }
}

pub fn start(_: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "frame.end_cap").then(|| (Kind::EndCap(EndCap::default()), vec![SelInput::new("ui.end_cap.faces", PLANAR_FACES, true)]))
}

pub fn rows(app: &SolveApp, ui: &mut egui::Ui, c: &mut EndCap, inputs: &[SelInput]) -> bool {
    if !c.unresolved.is_empty() && crate::selection::items(inputs, 0).is_empty() {
        crate::ui_kit::text(ui, &unresolved(app, c), crate::ui_kit::Text::Hint("end_cap_unresolved"));
    }
    let mut enter = expr_row(ui, app.tr("ui.thickness"), &mut c.thickness);
    // On top of the end, or set into it with the member cut back: the icons show which.
    row(ui, app.tr("ui.end_cap.direction"), |ui| {
        let options = [
            IconSegment { key: "outward", icon: "end_cap_outward", tip: app.tr("ui.end_cap.outward").to_string() },
            IconSegment { key: "inward", icon: "end_cap_inward", tip: app.tr("ui.end_cap.inward").to_string() },
        ];
        segmented_icons(ui, &mut c.direction, &options, &[]);
    });
    // Off: the plate is flush with the outside of the profile.
    row(ui, app.tr("ui.offset"), |ui| check(ui, &mut c.use_offset));
    if c.use_offset {
        choice_row(
            ui,
            app.tr("ui.end_cap.inset_kind"),
            "end_cap_inset_kind",
            &mut c.inset_kind,
            &[app.tr("ui.end_cap.ratio"), app.tr("ui.end_cap.length")],
        );
        if c.inset_kind == 0 {
            enter |= expr_row(ui, app.tr("ui.end_cap.inset_ratio"), &mut c.ratio);
        } else {
            enter |= expr_row(ui, app.tr("ui.end_cap.inset"), &mut c.offset);
        }
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
        return Err(if c.unresolved.is_empty() { app.tr("error.end_cap_faces").into() } else { unresolved(app, c) });
    }
    let direction = if c.direction == 1 { "inward" } else { "outward" };
    let mut args = json!({"faces": faces, "thickness": c.thickness, "direction": direction, "chamfer": c.chamfer});
    if let Value::Object(args) = &mut args {
        // Neither: the plate is flush with the outside of the profile.
        if c.use_offset {
            let (key, value) = if c.inset_kind == 0 { ("ratio", &c.ratio) } else { ("offset", &c.offset) };
            args.insert(key.into(), json!(value));
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
    let mut c = EndCap {
        thickness: length(thickness),
        direction: usize::from(*direction == Direction::Inward),
        use_offset: offset.is_some() || ratio.is_some(),
        inset_kind: usize::from(offset.is_some()),
        ratio: ratio.as_ref().map_or_else(|| "0.5".into(), |e| shown_expr(e, ValueKind::Unitless)),
        offset: offset.as_ref().map_or_else(|| "3 mm".into(), length),
        chamfer: length(chamfer),
        ..EndCap::default()
    };
    let state = app.session.model.state();
    let world = app.session.world_state().ok();
    let mut items = Vec::new();
    for (k, end) in ends.iter().enumerate() {
        let found = items.len();
        if let Some(anchor) = solidcraft_engine::doc::face_anchor::plane_face_anchor(&end.plane)
            && let Some(body) = state.body(&anchor.target.body)
        {
            let name = app.session.doc.world_body_name(&state, &body.name, anchor.component, &anchor.path);
            let indices = app.session.doc.configuration_face_indices(body, &anchor.target).ok();
            if let Some(body) = world.as_ref().and_then(|w| w.body(&name)) {
                let mesh = body.mesh();
                if let Some((tri, index)) =
                    mesh.triangles.iter().zip(&mesh.tri_face).find(|(_, i)| indices.as_ref().is_some_and(|is| is.contains(&(**i as usize))))
                    && let Some([a, b, c]) = mesh.tri(tri)
                {
                    items.push(Sel::Face { body: name, index: *index as usize, point: (a + b + c) / 3. });
                }
            }
        }
        if items.len() == found {
            c.unresolved.push(k + 1);
        }
    }
    // An end not found: the user picks the ends again rather than confirming a shorter list.
    if c.unresolved.is_empty()
        && let Some(input) = inputs.first_mut()
    {
        input.items = items;
    }
    Some((Kind::EndCap(c), inputs, Map::new()))
}

/// Which saved ends could not be found, and what to do about it.
fn unresolved(app: &SolveApp, c: &EndCap) -> String {
    let ends = c.unresolved.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
    app.tr("error.end_cap_unresolved").replace("{ends}", &ends)
}

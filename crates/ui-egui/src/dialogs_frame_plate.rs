//! Mounting plate form. Geometry and validation stay in `frame.plate`.

use serde_json::{Map, Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::FeatureKind;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::frame::{PlateHoles, PlateShape};

use crate::SolveApp;
use crate::dialogs::{Kind, shown_expr};
use crate::selection::{PLANAR_FACES, SelInput};
use crate::ui_kit::{choice_row, count_row, expr_row};

#[derive(Clone, Debug)]
pub struct Plate {
    pub shape: usize,
    pub width: String,
    pub height: String,
    pub diameter: String,
    pub thickness: String,
    pub offset: String,
    pub x_offset: String,
    pub y_offset: String,
    pub angle: String,
    pub pattern: usize,
    pub hole_diameter: String,
    pub pitch_x: String,
    pub pitch_y: String,
    pub pitch_diameter: String,
    pub count: String,
    pub pattern_angle: String,
}

impl Default for Plate {
    fn default() -> Self {
        Self {
            shape: 0,
            width: "100 mm".into(),
            height: "100 mm".into(),
            diameter: "100 mm".into(),
            thickness: "8 mm".into(),
            offset: "0 mm".into(),
            x_offset: "0 mm".into(),
            y_offset: "0 mm".into(),
            angle: "0 deg".into(),
            pattern: 0,
            hole_diameter: "10 mm".into(),
            pitch_x: "70 mm".into(),
            pitch_y: "70 mm".into(),
            pitch_diameter: "70 mm".into(),
            count: "4".into(),
            pattern_angle: "0 deg".into(),
        }
    }
}

pub fn start(_app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "frame.plate").then(|| (Kind::FramePlate(Plate::default()), vec![SelInput::new("Face", PLANAR_FACES, false)]))
}

pub fn rows(app: &SolveApp, ui: &mut egui::Ui, p: &mut Plate) -> bool {
    let mut enter = false;
    choice_row(ui, app.tr("ui.plate_shape"), "plate_shape", &mut p.shape, &[app.tr("ui.rectangle"), app.tr("ui.plate_round")]);
    if p.shape == 0 {
        enter |= expr_row(ui, app.tr("ui.plate_width"), &mut p.width);
        enter |= expr_row(ui, app.tr("ui.plate_height"), &mut p.height);
    } else {
        enter |= expr_row(ui, app.tr("ui.diameter"), &mut p.diameter);
    }
    enter |= expr_row(ui, app.tr("ui.thickness"), &mut p.thickness);
    enter |= expr_row(ui, app.tr("ui.offset"), &mut p.offset);
    enter |= expr_row(ui, app.tr("ui.plate_x_offset"), &mut p.x_offset);
    enter |= expr_row(ui, app.tr("ui.plate_y_offset"), &mut p.y_offset);
    enter |= expr_row(ui, app.tr("ui.angle"), &mut p.angle);
    choice_row(
        ui,
        app.tr("ui.plate_holes"),
        "plate_holes",
        &mut p.pattern,
        &[app.tr("ui.plate_none"), app.tr("ui.plate_rect_pattern"), app.tr("ui.plate_circle_pattern")],
    );
    if p.pattern != 0 {
        enter |= expr_row(ui, app.tr("ui.plate_hole_diameter"), &mut p.hole_diameter);
        if p.pattern == 1 {
            enter |= expr_row(ui, app.tr("ui.plate_pitch_x"), &mut p.pitch_x);
            enter |= expr_row(ui, app.tr("ui.plate_pitch_y"), &mut p.pitch_y);
        } else {
            enter |= expr_row(ui, app.tr("ui.plate_bolt_circle"), &mut p.pitch_diameter);
            let count = |text: &str| text.parse::<f64>().ok();
            enter |= count_row(ui, app.tr("ui.plate_count"), &mut p.count, &crate::ui_kit::Count { min: 2, max: 128, eval: &count });
            enter |= expr_row(ui, app.tr("ui.plate_pattern_angle"), &mut p.pattern_angle);
        }
    }
    enter
}

pub fn commands(app: &SolveApp, p: &Plate, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<Vec<(String, Value)>, String> {
    let [Sel::Face { body, index, point }] = crate::selection::items(inputs, 0) else { return Err(app.tr("error.plate_support").into()) };
    let mut args = json!({"face":point,"body":body,"index":index,"thickness":p.thickness,"offset":p.offset,"x_offset":p.x_offset,"y_offset":p.y_offset,"angle":p.angle});
    match p.shape {
        0 => {
            args["width"] = json!(p.width);
            args["height"] = json!(p.height);
        }
        1 => {
            args["shape"] = json!("round");
            args["diameter"] = json!(p.diameter);
        }
        _ => return Err(app.tr("error.plate_shape").into()),
    }
    args["holes"] = match p.pattern {
        0 => json!({"pattern":"none"}),
        1 => json!({"pattern":"rectangle","diameter":p.hole_diameter,"pitch_x":p.pitch_x,"pitch_y":p.pitch_y}),
        2 => {
            json!({"pattern":"circle","diameter":p.hole_diameter,"pitch_diameter":p.pitch_diameter,"count":p.count.trim().parse::<u32>().map_err(|_| app.tr("error.plate_count"))?,"angle":p.pattern_angle})
        }
        _ => return Err(app.tr("error.plate_pattern").into()),
    };
    if let Value::Object(object) = &mut args {
        for (key, value) in extra {
            object.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
    Ok(vec![("frame.plate".into(), args)])
}

pub fn for_feature(app: &SolveApp, kind: &FeatureKind) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let FeatureKind::FramePlate { plane, shape, holes, thickness, offset, x_offset, y_offset, angle, .. } = kind else { return None };
    let (_, mut inputs) = start(app, "frame.plate")?;
    let mut p = Plate::default();
    let length = |e: &solidcraft_engine::doc::Expr| shown_expr(e, ValueKind::Length);
    match shape {
        PlateShape::Rectangle { width, height } => {
            p.width = length(width);
            p.height = length(height);
        }
        PlateShape::Round { diameter } => {
            p.shape = 1;
            p.diameter = length(diameter);
        }
    }
    p.thickness = length(thickness);
    p.offset = length(offset);
    p.x_offset = length(x_offset);
    p.y_offset = length(y_offset);
    p.angle = shown_expr(angle, ValueKind::Angle);
    match holes {
        PlateHoles::None => {}
        PlateHoles::Rectangle { diameter, pitch_x, pitch_y } => {
            p.pattern = 1;
            p.hole_diameter = length(diameter);
            p.pitch_x = length(pitch_x);
            p.pitch_y = length(pitch_y);
        }
        PlateHoles::Circle { diameter, pitch_diameter, count, angle } => {
            p.pattern = 2;
            p.hole_diameter = length(diameter);
            p.pitch_diameter = length(pitch_diameter);
            p.count = count.to_string();
            p.pattern_angle = shown_expr(angle, ValueKind::Angle);
        }
    }
    // Pick a point on material, rather than the area centroid inside a hollow member's bore.
    if let Some(anchor) = solidcraft_engine::doc::face_anchor::plane_face_anchor(plane) {
        let state = app.session.model.state();
        if let Some(body) = state.body(&anchor.target.body) {
            let name = app.session.doc.world_body_name(&state, &body.name, anchor.component, &anchor.path);
            let indices = app.session.doc.configuration_face_indices(body, &anchor.target).ok();
            let world = app.session.world_state();
            if let Some(placed) = world.body(&name) {
                let mesh = placed.mesh();
                if let Some((triangle, index)) =
                    mesh.triangles.iter().zip(&mesh.tri_face).find(|(_, index)| indices.as_ref().is_some_and(|is| is.contains(&(**index as usize))))
                    && let Some([a, b, c]) = mesh.tri(triangle)
                    && let Some(input) = inputs.first_mut()
                {
                    input.items = vec![Sel::Face { body: name, index: *index as usize, point: (a + b + c) / 3. }];
                }
            }
        }
    }
    Some((Kind::FramePlate(p), inputs, Map::new()))
}

//! Living Hinge (`solid.living_hinge`): rows of staggered slots cut through a plate inside a sketch's
//! area. One field per row; the command checks the values.
use crate::{
    SolveApp,
    dialogs::Kind,
    selection::{BODIES, PROFILES, SelInput, SelectionExt as _, VERTICES},
    ui_kit::{choice_row, expr_row},
};
use serde_json::{Map, Value, json};
use solidcraft_engine::Sel;

const CMD: &str = "solid.living_hinge";
/// The clip choices, in the order of the Clip row.
const CLIPS: [&str; 3] = ["run_out", "shorten", "whole"];

#[derive(Clone, Debug)]
pub struct LivingHinge {
    /// 0: along X, 1: along Y, 2: at the angle in `angle`.
    pub axis: usize,
    pub angle: String,
    pub slot_length: String,
    pub bridge: String,
    pub row_spacing: String,
    pub slot_width: String,
    /// An index into [`CLIPS`].
    pub clip: usize,
    pub margin: String,
}

impl Default for LivingHinge {
    /// The metric row of the design's defaults (Boxes.py's flex range for 3 mm plywood).
    fn default() -> Self {
        Self {
            axis: 0,
            angle: "0 deg".into(),
            slot_length: "20 mm".into(),
            bridge: "3 mm".into(),
            row_spacing: "1.5 mm".into(),
            slot_width: "0.2 mm".into(),
            clip: 0,
            margin: "0 mm".into(),
        }
    }
}

pub fn start(_: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == CMD).then(|| {
        (
            Kind::LivingHinge(LivingHinge::default()),
            vec![
                SelInput::new("ui.profiles", PROFILES, true),
                SelInput::new("ui.living_hinge.plate", BODIES, true),
                SelInput::new("Anchor", VERTICES, false),
            ],
        )
    })
}

pub fn rows(_: &SolveApp, ui: &mut egui::Ui, h: &mut LivingHinge, _: &[SelInput]) -> bool {
    choice_row(
        ui,
        "ui.living_hinge.bend_axis",
        "living_hinge_axis",
        &mut h.axis,
        &["ui.living_hinge.along_x", "ui.living_hinge.along_y", "ui.living_hinge.at_angle"],
    );
    let mut enter = false;
    if h.axis == 2 {
        enter |= expr_row(ui, "ui.angle", &mut h.angle);
    }
    enter |= expr_row(ui, "ui.living_hinge.slot_length", &mut h.slot_length);
    enter |= expr_row(ui, "ui.living_hinge.bridge", &mut h.bridge);
    enter |= expr_row(ui, "ui.living_hinge.row_spacing", &mut h.row_spacing);
    enter |= expr_row(ui, "ui.living_hinge.slot_width", &mut h.slot_width);
    choice_row(
        ui,
        "ui.living_hinge.clip",
        "living_hinge_clip",
        &mut h.clip,
        &["ui.living_hinge.run_out", "ui.living_hinge.shorten", "ui.living_hinge.whole"],
    );
    enter |= expr_row(ui, "ui.living_hinge.margin", &mut h.margin);
    enter
}

pub fn commands(_: &SolveApp, h: &LivingHinge, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<Vec<(String, Value)>, String> {
    let ps = crate::selection::items(inputs, 0).profiles();
    let Some(sketch) = ps.first().map(|p| p.0) else { return Err("Pick the sketch area to fill with slots".into()) };
    let profiles: Vec<usize> = ps.iter().filter(|p| p.0 == sketch).map(|p| p.1).collect();
    let angle = match h.axis {
        0 => "0 deg",
        1 => "90 deg",
        _ => h.angle.as_str(),
    };
    let mut args = json!({
        "sketch": sketch,
        "profiles": profiles,
        "slot_length": h.slot_length,
        "bridge": h.bridge,
        "row_spacing": h.row_spacing,
        "slot_width": h.slot_width,
        "angle": angle,
        "clip": CLIPS.get(h.clip).copied().unwrap_or("run_out"),
        "margin": h.margin,
    });
    if let Value::Object(args) = &mut args {
        let plates = crate::selection::items(inputs, 1).body_names();
        if !plates.is_empty() {
            args.insert("targets".into(), json!(plates));
        }
        if let Some(Sel::Vertex { point, .. }) = crate::selection::items(inputs, 2).first() {
            args.insert("anchor".into(), crate::selection::point_value(*point));
        }
        for (k, v) in extra {
            args.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(vec![(CMD.into(), args)])
}

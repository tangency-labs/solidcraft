//! Living Hinge (`solid.living_hinge`): rows of staggered slots cut through a plate inside a sketch's
//! area. One field per row; the command checks the values.
use crate::{
    SolveApp,
    dialogs::Kind,
    selection::{BODIES, PROFILES, SelInput, SelectionExt as _, VERTICES},
    ui_kit::{BackingTooltipExt as _, choice_row, expr_row},
};
use serde_json::{Map, Value, json};
use solidcore_record::living_hinge::HingeClip;
use solidcraft_engine::{Sel, doc::FeatureKind};

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

/// The four lengths of a new dialog (design 13.3), in mm: the metric row, or the inch row for an
/// inch or foot design. Not run through `length_input::defaults`, which would keep the number and
/// swap the unit (a 0.2 mm kerf would become 0.2 in).
fn default_lengths_mm(unit: &str) -> [f64; 4] {
    match unit {
        "in" | "ft" => [0.75 * 25.4, 0.125 * 25.4, 0.0625 * 25.4, 0.008 * 25.4],
        _ => [20.0, 3.0, 1.5, 0.2],
    }
}

/// A new dialog's values, the lengths written in the design's unit.
fn new_hinge(doc: &solidcraft_engine::doc::Document) -> LivingHinge {
    let shown = |mm: f64| crate::length_input::shown(doc, mm);
    let [slot_length, bridge, row_spacing, slot_width] = default_lengths_mm(crate::length_input::unit(doc));
    LivingHinge {
        slot_length: shown(slot_length),
        bridge: shown(bridge),
        row_spacing: shown(row_spacing),
        slot_width: shown(slot_width),
        margin: shown(0.0),
        ..LivingHinge::default()
    }
}

pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == CMD).then(|| {
        (
            Kind::LivingHinge(new_hinge(&app.session.doc)),
            vec![
                SelInput::new("ui.profiles", PROFILES, true),
                SelInput::new("ui.living_hinge.plate", BODIES, true).tip("ui.living_hinge.plate_tip"),
                SelInput::new("Anchor", VERTICES, false).tip("ui.living_hinge.anchor_tip"),
            ],
        )
    })
}

/// A row with its tooltip, the message `tip`.
fn tipped<R>(ui: &mut egui::Ui, tip: &str, row: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let row = ui.scope(row);
    row.response.backing_on_hover_text(solidcraft_ui_text::tr(tip));
    row.inner
}

pub fn rows(app: &SolveApp, ui: &mut egui::Ui, h: &mut LivingHinge, _: &[SelInput]) -> bool {
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
    enter |= tipped(ui, "ui.living_hinge.bridge_tip", |ui| expr_row(ui, "ui.living_hinge.bridge", &mut h.bridge));
    enter |= tipped(ui, "ui.living_hinge.row_spacing_tip", |ui| expr_row(ui, "ui.living_hinge.row_spacing", &mut h.row_spacing));
    enter |= tipped(ui, "ui.living_hinge.slot_width_tip", |ui| expr_row(ui, "ui.living_hinge.slot_width", &mut h.slot_width));
    tipped(ui, "ui.living_hinge.clip_tip", |ui| {
        choice_row(
            ui,
            "ui.living_hinge.clip",
            "living_hinge_clip",
            &mut h.clip,
            &["ui.living_hinge.run_out", "ui.living_hinge.shorten", "ui.living_hinge.whole"],
        )
    });
    enter |= tipped(ui, "ui.living_hinge.margin_tip", |ui| expr_row(ui, "ui.living_hinge.margin", &mut h.margin));
    if let Some(reply) = &app.preview.report {
        let lines = report_lines(&app.session.doc, reply);
        for (i, line) in lines.iter().enumerate() {
            let r = ui.label(line);
            if i == 2 {
                let outline = reply.get("outline_length_mm").and_then(Value::as_f64).unwrap_or(0.0);
                let outline = crate::length_input::shown(&app.session.doc, outline);
                r.backing_on_hover_text(solidcraft_ui_text::locale().format("ui.living_hinge.report_cut_tip", &[("outline", &outline)]));
            }
        }
        if let Some(slow) = slow_warning(reply) {
            let t = solidcraft_theme::Tokens::get();
            ui.add(egui::Label::new(egui::RichText::new(slow).color(t.warning).size(t.font_small)).wrap());
        }
    }
    enter
}

/// The report lines under the fields (design 13.4), from a preview's reply: counts, the cut
/// length and the band's width, lengths in the design's unit.
fn report_lines(doc: &solidcraft_engine::doc::Document, reply: &Value) -> Vec<String> {
    let l = solidcraft_ui_text::locale();
    report_args(doc, reply).iter().map(|(id, args)| l.format(id, &args.iter().map(|(k, v)| (*k, v.as_str())).collect::<Vec<_>>())).collect()
}

/// The slow warning under the report lines (design 13.4): a hinge of more than
/// `SLOW_CUT_SLOTS` slots takes a while to cut. Over the cap the preview is refused instead.
fn slow_warning(reply: &Value) -> Option<String> {
    let slots = reply.get("slots").and_then(Value::as_u64)?;
    let limit = solidcraft_engine::doc::SLOW_CUT_SLOTS;
    (slots > limit as u64)
        .then(|| solidcraft_ui_text::locale().format("ui.living_hinge.report_slow", &[("slots", &slots.to_string()), ("limit", &limit.to_string())]))
}

/// Each report line's message id and its arguments.
fn report_args(doc: &solidcraft_engine::doc::Document, reply: &Value) -> Vec<(&'static str, Vec<(&'static str, String)>)> {
    let n = |k: &str| reply.get(k).and_then(Value::as_u64).unwrap_or(0).to_string();
    let len = |k: &str| crate::length_input::shown(doc, reply.get(k).and_then(Value::as_f64).unwrap_or(0.0));
    vec![
        ("ui.living_hinge.report_slots", vec![("slots", n("slots")), ("rows", n("rows"))]),
        ("ui.living_hinge.report_kinds", vec![("run_out", n("run_out")), ("shortened", n("shortened")), ("left_out", n("left_out"))]),
        ("ui.living_hinge.report_cut", vec![("length", len("cut_length_mm"))]),
        ("ui.living_hinge.report_band", vec![("width", len("band_width_mm"))]),
    ]
}

pub fn commands(app: &SolveApp, h: &LivingHinge, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<Vec<(String, Value)>, String> {
    let ps = crate::selection::items(inputs, 0).profiles();
    let Some(sketch) = ps.first().map(|p| p.0) else { return Err(app.tr("error.living_hinge_profiles").into()) };
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

/// The bend-axis choice of a stored angle: 0 along X, 1 along Y, else 2 (at that angle).
fn axis_of(angle: &str) -> usize {
    match angle.trim().trim_end_matches("deg").trim().parse::<f64>() {
        Ok(a) if a.abs() < 1e-9 => 0,
        Ok(a) if (a - 90.).abs() < 1e-9 => 1,
        _ => 2,
    }
}

/// The dialog of a saved Living Hinge, for Edit Feature: the stored values as typed, the sketch's
/// profiles and the plate picked again, a typed anchor kept as it was.
pub fn for_feature(app: &SolveApp, kind: &FeatureKind) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let FeatureKind::LivingHinge { sketch, profiles, slot_length, bridge, row_spacing, slot_width, angle, anchor, clip, margin, targets } = kind
    else {
        return None;
    };
    let (_, mut inputs) = start(app, CMD)?;
    let d = LivingHinge::default();
    let angle = angle.as_deref().map_or_else(|| "0 deg".to_string(), str::to_string);
    let h = LivingHinge {
        axis: axis_of(&angle),
        angle,
        slot_length: slot_length.as_str().into(),
        bridge: bridge.as_str().into(),
        row_spacing: row_spacing.as_str().into(),
        slot_width: slot_width.as_deref().map_or(d.slot_width, str::to_string),
        clip: match clip {
            HingeClip::RunOut => 0,
            HingeClip::Shorten => 1,
            HingeClip::Whole => 2,
        },
        margin: margin.as_deref().map_or(d.margin, str::to_string),
    };
    let st = crate::open_meshing::session_world(&app.session);
    let indices = st.sketch(sketch.0).map(|ss| crate::dialogs::profile_indices(ss, profiles)).unwrap_or_default();
    if let Some(input) = inputs.get_mut(0) {
        input.items = indices.into_iter().map(|index| Sel::Profile { sketch: sketch.0, index }).collect();
    }
    if let Some(input) = inputs.get_mut(1) {
        input.items = targets.iter().map(|name| Sel::Body { name: name.0.clone() }).collect();
    }
    let mut extra = Map::new();
    if let Some(anchor) = anchor {
        extra.insert("anchor".into(), crate::dialogs::pt_expr(anchor));
    }
    Some((Kind::LivingHinge(h), inputs, extra))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Services;
    use solidcraft_engine::Session;

    #[test]
    fn living_hinge_axis_of_a_stored_angle() {
        assert_eq!(axis_of("0 deg"), 0);
        assert_eq!(axis_of("0"), 0);
        assert_eq!(axis_of("90 deg"), 1);
        assert_eq!(axis_of("45 deg"), 2);
        assert_eq!(axis_of("a + 1 deg"), 2);
    }

    /// Edit Feature fills the dialog from the saved hinge, and OK sends the same values back.
    #[test]
    fn living_hinge_edit_feature_refills_the_dialog() {
        let mut s = Session::default();
        let run = |s: &mut Session, id: &str, p: Value| {
            s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
        };
        run(&mut s, "solid.box", json!({"length": 120, "width": 80, "height": 3, "body_name": "Wall"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 3, "name": "HingeArea"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [40, 0], "p1": [80, 80]}));
        run(&mut s, "sketch.finish", json!({}));
        run(
            &mut s,
            "solid.living_hinge",
            json!({"sketch": "HingeArea", "slot_length": "22 mm", "bridge": "4 mm", "row_spacing": "2 mm", "angle": "90 deg",
                "clip": "whole", "margin": "1 mm", "targets": ["Wall"], "name": "Hinge"}),
        );
        let app = SolveApp::new(s, Services::default());
        let kind = app.session.doc.find_feature("Hinge").unwrap().kind.clone();
        let (Kind::LivingHinge(h), inputs, extra) = for_feature(&app, &kind).unwrap() else { panic!("not a living hinge") };
        assert_eq!((h.axis, h.clip), (1, 2), "{h:?}");
        assert_eq!((h.slot_length.as_str(), h.bridge.as_str(), h.row_spacing.as_str()), ("22 mm", "4 mm", "2 mm"));
        assert_eq!((h.slot_width.as_str(), h.margin.as_str()), ("0.2 mm", "1 mm"));
        assert!(!inputs[0].items.is_empty(), "the area's profiles are picked");
        assert_eq!(crate::selection::items(&inputs, 1).body_names(), vec!["Wall".to_string()]);
        assert!(!extra.contains_key("anchor"));
        let cmds = commands(&app, &h, &inputs, &extra).unwrap();
        let args = &cmds[0].1;
        assert_eq!((args["angle"].as_str(), args["clip"].as_str()), (Some("90 deg"), Some("whole")), "{args}");
        assert_eq!(args["targets"], json!(["Wall"]), "{args}");
        // Other features are not a living hinge.
        assert!(for_feature(&app, &app.session.doc.find_feature("HingeArea").unwrap().kind.clone()).is_none());
    }

    /// The report lines read the preview's reply, lengths in the design's unit.
    #[test]
    fn living_hinge_report_lines_from_the_reply() {
        let s = Session::default();
        let reply = json!({"slots": 95, "rows": 27, "run_out": 28, "shortened": 0, "left_out": 26,
            "cut_length_mm": 1757.8, "outline_length_mm": 3566.5, "band_width_mm": 40.0});
        let args = report_args(&s.doc, &reply);
        let flat: Vec<String> = args.iter().flat_map(|(_, a)| a.iter().map(|(k, v)| format!("{k}={v}"))).collect();
        assert_eq!(flat, ["slots=95", "rows=27", "run_out=28", "shortened=0", "left_out=26", "length=1757.8 mm", "width=40 mm"]);
        assert_eq!(report_lines(&s.doc, &reply).len(), 4);
    }

    /// A new dialog's lengths follow the design unit's row (design 13.3), not the number with the
    /// unit swapped.
    #[test]
    fn living_hinge_new_dialog_defaults_in_the_design_unit() {
        let mut s = Session::default();
        let h = new_hinge(&s.doc);
        assert_eq!([h.slot_length, h.bridge, h.row_spacing, h.slot_width, h.margin], ["20 mm", "3 mm", "1.5 mm", "0.2 mm", "0 mm"]);
        std::sync::Arc::make_mut(&mut s.doc).units = "cm".into();
        let h = new_hinge(&s.doc);
        assert_eq!([h.slot_length, h.bridge, h.row_spacing, h.slot_width], ["2 cm", "0.3 cm", "0.15 cm", "0.02 cm"]);
        std::sync::Arc::make_mut(&mut s.doc).units = "in".into();
        let h = new_hinge(&s.doc);
        assert_eq!([h.slot_length, h.bridge, h.row_spacing, h.slot_width], ["0.75 in", "0.125 in", "0.0625 in", "0.008 in"]);
    }

    /// The slow warning shows only past `SLOW_CUT_SLOTS` slots.
    #[test]
    fn living_hinge_slow_warning_past_the_slow_count() {
        let limit = solidcraft_engine::doc::SLOW_CUT_SLOTS as u64;
        assert!(slow_warning(&json!({"slots": limit})).is_none());
        assert!(slow_warning(&json!({})).is_none());
        assert!(slow_warning(&json!({"slots": limit + 1})).is_some());
    }
}

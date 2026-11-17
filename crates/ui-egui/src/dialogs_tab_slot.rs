//! Tab and Slot (`solid.tab_slot`): tabs on the edge of one plate through slots in the plate it
//! meets at a right angle. Two plate picks and one field per row; an empty field is auto (the
//! command's default). The command checks the values.
use crate::{
    SolveApp,
    dialogs::Kind,
    selection::{BODIES, SelInput, SelectionExt as _},
    ui_kit::{choice_row, expr_row},
};
use serde_json::{Map, Value, json};

const CMD: &str = "solid.tab_slot";

#[derive(Clone, Debug, Default)]
pub struct TabSlot {
    /// 0: by count, 1: by spacing.
    pub layout: usize,
    pub count: String,
    pub spacing: String,
    pub tab_width: String,
    pub end_distance: String,
    pub clearance: String,
    pub protrusion: String,
}

/// A new dialog's values: every field auto but the clearance, 0.1 mm in the design's unit.
fn new_tab_slot(doc: &solidcraft_engine::doc::Document) -> TabSlot {
    TabSlot { clearance: crate::length_input::shown(doc, 0.1), ..TabSlot::default() }
}

pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == CMD).then(|| {
        (
            Kind::TabSlot(new_tab_slot(&app.session.doc)),
            vec![SelInput::new("ui.tab_slot.tab_plates", BODIES, true), SelInput::new("ui.tab_slot.slot_plates", BODIES, true)],
        )
    })
}

pub fn rows(_app: &SolveApp, ui: &mut egui::Ui, t: &mut TabSlot, _inputs: &mut [SelInput]) -> bool {
    choice_row(ui, "ui.tab_slot.layout", "tab_slot_layout", &mut t.layout, &["ui.tab_slot.by_count", "ui.tab_slot.by_spacing"]);
    let mut enter = false;
    if t.layout == 0 {
        enter |= expr_row(ui, "ui.tab_slot.count", &mut t.count);
    } else {
        enter |= expr_row(ui, "ui.tab_slot.spacing", &mut t.spacing);
    }
    enter |= expr_row(ui, "ui.tab_slot.tab_width", &mut t.tab_width);
    enter |= expr_row(ui, "ui.tab_slot.end_distance", &mut t.end_distance);
    enter |= expr_row(ui, "ui.tab_slot.clearance", &mut t.clearance);
    enter |= expr_row(ui, "ui.tab_slot.protrusion", &mut t.protrusion);
    enter
}

pub fn commands(app: &SolveApp, t: &TabSlot, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<Vec<(String, Value)>, String> {
    let tabs = crate::selection::items(inputs, 0).body_names();
    let slots = crate::selection::items(inputs, 1).body_names();
    if tabs.is_empty() || slots.is_empty() {
        return Err(app.tr("error.tab_slot_plates").into());
    }
    let mut args = Map::new();
    args.insert("tab_plates".into(), json!(tabs));
    args.insert("slot_plates".into(), json!(slots));
    let (count, spacing) = if t.layout == 0 { (t.count.as_str(), "") } else { ("", t.spacing.as_str()) };
    for (k, v) in [
        ("count", count),
        ("spacing", spacing),
        ("tab_width", t.tab_width.as_str()),
        ("end_distance", t.end_distance.as_str()),
        ("clearance", t.clearance.as_str()),
        ("protrusion", t.protrusion.as_str()),
    ] {
        if !v.trim().is_empty() {
            args.insert(k.into(), json!(v.trim()));
        }
    }
    for (k, v) in extra {
        args.entry(k.clone()).or_insert_with(|| v.clone());
    }
    Ok(vec![(CMD.into(), Value::Object(args))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Services;
    use solidcraft_engine::{Sel, Session};

    /// The plate picks and the filled rows become the command's arguments; empty rows are left
    /// out (auto), and only the layout's own row is sent.
    #[test]
    fn tab_slot_dialog_builds_the_command_args() {
        let app = SolveApp::new(Session::default(), Services::default());
        let (Kind::TabSlot(mut t), mut inputs) = start(&app, CMD).unwrap() else { panic!("not a tab and slot") };
        assert_eq!(t.clearance, "0.1 mm");
        assert!(commands(&app, &t, &inputs, &Map::new()).is_err(), "no plates picked");
        inputs[0].items = vec![Sel::Body { name: "Back".into() }];
        inputs[1].items = vec![Sel::Body { name: "Bottom".into() }, Sel::Body { name: "Side".into() }];
        (t.count, t.spacing, t.tab_width) = ("2".into(), "30 mm".into(), "12 mm".into());
        let cmds = commands(&app, &t, &inputs, &Map::new()).unwrap();
        assert_eq!(cmds[0].0, CMD);
        assert_eq!(
            cmds[0].1,
            json!({"tab_plates": ["Back"], "slot_plates": ["Bottom", "Side"], "count": "2", "tab_width": "12 mm", "clearance": "0.1 mm"})
        );
        t.layout = 1;
        let args = &commands(&app, &t, &inputs, &Map::new()).unwrap()[0].1;
        assert_eq!((args.get("count"), args["spacing"].as_str()), (None, Some("30 mm")), "{args}");
    }
}

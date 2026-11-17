//! Tab and Slot (`solid.tab_slot`): tabs on the edge of one plate through slots in the plate it
//! meets at a right angle. Two plate picks and one field per row; an empty field is auto (the
//! command's default). The command checks the values.
use crate::{
    SolveApp,
    dialogs::Kind,
    selection::{BODIES, SelInput, SelectionExt as _},
    ui_kit::{BackingTooltipExt as _, choice_row, expr_row},
};
use serde_json::{Map, Value, json};
use solidcraft_engine::{Sel, doc::FeatureKind};

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
            vec![
                SelInput::new("ui.tab_slot.tab_plates", BODIES, true).tip("ui.tab_slot.tab_plates_tip"),
                SelInput::new("ui.tab_slot.slot_plates", BODIES, true).tip("ui.tab_slot.slot_plates_tip"),
            ],
        )
    })
}

pub fn rows(app: &SolveApp, ui: &mut egui::Ui, t: &mut TabSlot, _inputs: &mut [SelInput]) -> bool {
    choice_row(ui, "ui.tab_slot.layout", "tab_slot_layout", &mut t.layout, &["ui.tab_slot.by_count", "ui.tab_slot.by_spacing"]);
    let mut enter = false;
    if t.layout == 0 {
        enter |= tipped(ui, "ui.tab_slot.count_tip", |ui| expr_row(ui, "ui.tab_slot.count", &mut t.count));
    } else {
        enter |= tipped(ui, "ui.tab_slot.spacing_tip", |ui| expr_row(ui, "ui.tab_slot.spacing", &mut t.spacing));
    }
    enter |= tipped(ui, "ui.tab_slot.tab_width_tip", |ui| expr_row(ui, "ui.tab_slot.tab_width", &mut t.tab_width));
    enter |= tipped(ui, "ui.tab_slot.end_distance_tip", |ui| expr_row(ui, "ui.tab_slot.end_distance", &mut t.end_distance));
    enter |= tipped(ui, "ui.tab_slot.clearance_tip", |ui| expr_row(ui, "ui.tab_slot.clearance", &mut t.clearance));
    enter |= tipped(ui, "ui.tab_slot.protrusion_tip", |ui| expr_row(ui, "ui.tab_slot.protrusion", &mut t.protrusion));
    if let Some(reply) = &app.preview.report {
        let l = solidcraft_ui_text::locale();
        for (id, args) in summary_args(reply) {
            // The skipped line names every pair: it wraps instead of running past the dialog.
            let text = l.format(id, &args.iter().map(|(k, v)| (*k, v.as_str())).collect::<Vec<_>>());
            ui.add(egui::Label::new(text).wrap());
        }
    }
    enter
}

/// The summary under the fields (design v2 12.4), from a preview's reply, as (message id,
/// arguments): the plate joints and tabs made, then the skipped pairs with the reason for each.
fn summary_args(reply: &Value) -> Vec<(&'static str, Vec<(&'static str, String)>)> {
    let joints = reply["plate_joints"].as_array().map_or(0, Vec::len);
    let tabs = reply["tabs"].as_u64().unwrap_or(0);
    let count = |n: u64, one: &str, many: &str| solidcraft_ui_text::format(if n == 1 { one } else { many }, &[("count", &n.to_string())]);
    let joints = count(joints as u64, "ui.tab_slot.plate_joints_one", "ui.tab_slot.plate_joints");
    let tabs = count(tabs, "ui.tab_slot.tabs_one", "ui.tab_slot.tabs");
    let mut lines = vec![("ui.tab_slot.summary", vec![("plate_joints", joints), ("tabs", tabs)])];
    let skipped: Vec<String> = reply["skipped"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            let text = |k: &str| s[k].as_str().unwrap_or("").to_string();
            format!("{} → {} {}", text("tab_plate"), text("slot_plate"), text("detail")).trim_end().to_string()
        })
        .collect();
    if !skipped.is_empty() {
        lines.push(("ui.tab_slot.summary_skipped", vec![("count", skipped.len().to_string()), ("list", skipped.join(", "))]));
    }
    lines
}

/// One row with a tooltip over the whole of it (label and field).
fn tipped(ui: &mut egui::Ui, tip: &str, row: impl FnOnce(&mut egui::Ui) -> bool) -> bool {
    let row = ui.scope(row);
    row.response.backing_on_hover_text(solidcraft_ui_text::tr(tip));
    row.inner
}

pub fn commands(app: &SolveApp, t: &TabSlot, inputs: &[SelInput], extra: &Map<String, Value>) -> Result<Vec<(String, Value)>, String> {
    let tabs = crate::selection::items(inputs, 0).body_names();
    let slots = crate::selection::items(inputs, 1).body_names();
    if tabs.is_empty() || slots.is_empty() {
        return Err(app.tr("error.tab_slot_plates").into());
    }
    // Edit Feature keeps each stored pick (its key and near point) in `extra`: a plate still
    // picked is sent back as it was, so its tools keep their names; a new one is a plain name.
    let picks = |list: &str, names: Vec<String>| -> Value {
        let stored = extra.get(list).and_then(Value::as_array);
        names
            .into_iter()
            .map(|name| stored.and_then(|s| s.iter().find(|p| p["body"].as_str() == Some(name.as_str()))).cloned().unwrap_or_else(|| json!(name)))
            .collect()
    };
    let mut args = Map::new();
    args.insert("tab_plates".into(), picks("tab_plates", tabs));
    args.insert("slot_plates".into(), picks("slot_plates", slots));
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

/// Edit Feature: the dialog of a stored Tab and Slot, its plates picked and its rows as typed.
/// The stored picks and `next_plate_key` ride in `extra`, so OK sends the keys back.
pub fn for_feature(app: &SolveApp, kind: &FeatureKind) -> Option<(Kind, Vec<SelInput>, Map<String, Value>)> {
    let FeatureKind::TabSlot { tab_plates, slot_plates, next_plate_key, count, spacing, tab_width, end_distance, clearance, protrusion } = kind
    else {
        return None;
    };
    let (_, mut inputs) = start(app, CMD)?;
    let text = |e: &Option<solidcraft_engine::doc::Expr>| e.as_ref().map_or_else(String::new, |e| e.as_str().to_string());
    let t = TabSlot {
        layout: usize::from(count.is_none() && spacing.is_some()),
        count: text(count),
        spacing: text(spacing),
        tab_width: text(tab_width),
        end_distance: text(end_distance),
        clearance: clearance.as_str().to_string(),
        protrusion: text(protrusion),
    };
    let mut extra = Map::new();
    for (i, (list, picks)) in [("tab_plates", tab_plates), ("slot_plates", slot_plates)].into_iter().enumerate() {
        if let Some(input) = inputs.get_mut(i) {
            input.items = picks.iter().map(|p| Sel::Body { name: p.body.0.clone() }).collect();
        }
        extra.insert(list.into(), serde_json::to_value(picks).unwrap_or_default());
    }
    extra.insert("next_plate_key".into(), json!(next_plate_key));
    Some((Kind::TabSlot(t), inputs, extra))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Services;
    use solidcraft_engine::Session;

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

#[cfg(test)]
mod edit_tests {
    use super::*;
    use crate::Services;
    use solidcraft_engine::Session;

    /// Edit Feature refills the picks and rows; OK sends each kept plate back with its stored
    /// key and near point, a new plate as a plain name, and `next_plate_key`.
    #[test]
    fn tab_slot_for_feature_sends_the_stored_keys_back() {
        let mut s = Session::default();
        for (corner, size, name) in [([0, 0, 0], [160, 100, 6], "Bottom"), ([20, 30, 6], [120, 6, 60], "Wall")] {
            let p = json!({"corner": corner, "length": size[0], "width": size[1], "height": size[2], "body_name": name, "operation": "new"});
            s.execute("solid.box", &p).unwrap();
        }
        let p = json!({"tab_plates": [{"body": "Wall", "near": [80, 33, 6]}], "slot_plates": ["Bottom"], "spacing": "40 mm", "tab_width": "12 mm"});
        s.execute("solid.tab_slot", &p).unwrap();
        let app = SolveApp::new(s, Services::default());
        let kind = app.session.doc.features.last().unwrap().kind.clone();
        let (Kind::TabSlot(t), mut inputs, extra) = for_feature(&app, &kind).unwrap() else { panic!("not a tab and slot") };
        assert_eq!((t.layout, t.spacing.as_str(), t.tab_width.as_str(), t.count.as_str()), (1, "40 mm", "12 mm", ""));
        assert_eq!(inputs[0].items, vec![Sel::Body { name: "Wall".into() }]);
        let args = &commands(&app, &t, &inputs, &extra).unwrap()[0].1;
        assert_eq!(args["tab_plates"], json!([{"key": 1, "body": "Wall", "near": [80.0, 33.0, 6.0]}]), "{args}");
        assert_eq!((&args["slot_plates"], &args["next_plate_key"]), (&json!([{"key": 2, "body": "Bottom"}]), &json!(3)), "{args}");
        inputs[1].items.push(Sel::Body { name: "Side".into() });
        let args = &commands(&app, &t, &inputs, &extra).unwrap()[0].1;
        assert_eq!(args["slot_plates"][1], json!("Side"), "{args}");
    }

    /// The summary lines read the command's reply: one plate joint cut with its tabs; a skipped
    /// pair is named with its reason.
    #[test]
    fn tab_slot_summary_from_the_reply() {
        let mut s = Session::default();
        for (corner, size, name) in [([0, 0, 0], [160, 100, 6], "Bottom"), ([20, 30, 6], [120, 6, 60], "Wall")] {
            let p = json!({"corner": corner, "length": size[0], "width": size[1], "height": size[2], "body_name": name, "operation": "new"});
            s.execute("solid.box", &p).unwrap();
        }
        let p = json!({"tab_plates": ["Wall"], "slot_plates": ["Bottom"], "count": "2", "tab_width": "12 mm"});
        let reply = s.execute("solid.tab_slot", &p).unwrap();
        let lines = summary_args(&reply);
        assert_eq!(lines, [("ui.tab_slot.summary", vec![("plate_joints", "1 plate joint".to_string()), ("tabs", "2 tabs".to_string())])], "{reply}");
        let reply = json!({"plate_joints": [], "tabs": 0, "skipped": [
            {"tab_plate": "Back", "slot_plate": "Bottom", "reason": "swapped", "detail": "is the other way round"},
            {"tab_plate": "Side", "slot_plate": "Bottom", "reason": "sheet_metal", "detail": ""}]});
        let lines = summary_args(&reply);
        assert_eq!(lines[1].0, "ui.tab_slot.summary_skipped");
        assert_eq!(lines[1].1, [("count", "2".to_string()), ("list", "Back → Bottom is the other way round, Side → Bottom".to_string())]);
        assert_eq!(lines[0].1, [("plate_joints", "0 plate joints".to_string()), ("tabs", "0 tabs".to_string())]);
        assert_eq!(summary_args(&json!({})).len(), 1, "an empty reply still has the counts line");
    }
}

//! Move/Copy changes the text source, never independent generated outlines.
use crate::{SolveApp, dialogs::Kind, selection::SelInput};
use serde_json::{Value, json};
use solidcraft_engine::{Sel, doc::expr::Kind as ValueKind, sketch::LinkKind};

#[derive(Clone, Debug)]
pub struct TextMove {
    sketch: u64,
    link: String,
    values: [String; 3],
    copy: bool,
}

pub(crate) fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    if id != "sketch.move" {
        return None;
    }
    let entity = crate::sketch_text::selected_entity(app)?;
    let sketch = app.session.active_sketch?;
    let sk = app.session.doc.sketch(sketch).ok()?;
    let link = sk.curves.iter().find(|c| c.id == entity)?.link.clone()?;
    let (_, unit) = crate::prefs::unit_scale(&app.session.doc.units);
    Some((Kind::TextMove(TextMove { sketch, link, values: [format!("0 {unit}"), format!("0 {unit}"), "0 deg".into()], copy: false }), vec![]))
}

pub(crate) fn valid(app: &SolveApp, k: &TextMove) -> bool {
    app.session.active_sketch == Some(k.sketch)
        && app.session.doc.sketch(k.sketch).ok().and_then(|sk| sk.link(&k.link)).is_some_and(|l| l.kind == LinkKind::Text)
}

pub(crate) fn rows(ui: &mut egui::Ui, k: &mut TextMove) -> bool {
    let [x, y, angle] = &mut k.values;
    let mut enter = crate::ui_kit::value_pair(ui, "Position", [x, y], ["X", "Y"], Some(["Distance X", "Distance Y"]));
    enter |= crate::ui_kit::row(ui, "Angle", |ui| crate::ui_kit::field(ui, angle));
    crate::ui_kit::options(ui, "text_move_options", &mut [("Create Copy", &mut k.copy)]);
    enter
}

pub(crate) fn commands(app: &SolveApp, k: &TextMove) -> Result<Vec<(String, Value)>, String> {
    if !valid(app, k) {
        return Err("The text is no longer in the active sketch".into());
    }
    let doc = &app.session.doc;
    let mut values = [0.0; 3];
    for (i, (input, value)) in k.values.iter().zip(&mut values).enumerate() {
        let kind = if i == 2 { ValueKind::Angle } else { ValueKind::Length };
        *value = doc.eval(&doc.with_design_unit(input.trim(), kind), kind).map_err(|e| e.to_string())?;
        if !value.is_finite() {
            return Err("Move values must be finite".into());
        }
    }
    let sk = doc.sketch(k.sketch).map_err(|e| e.to_string())?;
    let curve = sk.curves.iter().find(|c| c.link.as_deref() == Some(&k.link)).ok_or("The text has no outlines")?;
    let Some(solidcraft_engine::sketch::LinkSource::Text { at, .. }) = sk.link(&k.link).map(|l| &l.source) else {
        return Err("The source is no longer text".into());
    };
    Ok(vec![(
        "sketch.move".into(),
        json!({"entities":[curve.id],"translate":[values[0],values[1]],
        "angle":values[2].to_degrees(),"center":[at.x,at.y],"copy":k.copy}),
    )])
}

pub(crate) fn highlighted(app: &SolveApp) -> Vec<Sel> {
    let link = match app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::TextMove(k)) => Some(k.link.as_str()),
        _ => app.viewport.text_drag.as_ref().map(|(_, link)| link.as_str()),
    };
    let Some(link) = link else {
        return vec![];
    };
    let Some(sk) = app.session.active_sketch.and_then(|sid| app.session.doc.sketch(sid).ok()) else {
        return vec![];
    };
    sk.curves.iter().filter(|c| c.link.as_deref() == Some(link)).map(|c| Sel::SketchCurve { id: c.id.clone(), sketch: None }).collect()
}

/// Esc restores the document and both history stacks to the start of this gesture.
pub(crate) fn cancel_drag(app: &mut SolveApp) -> bool {
    let Some((checkpoint, _)) = app.viewport.text_drag.take() else {
        return false;
    };
    let _ = app.session.execute("edit.restore_checkpoint", &json!({"checkpoint":checkpoint}));
    app.viewport.point_drag = None;
    crate::drag_snap::clear();
    true
}

pub(crate) fn end_drag(app: &mut SolveApp) {
    if let Some((checkpoint, _)) = app.viewport.text_drag.take() {
        let _ = app.session.execute("edit.restore_checkpoint", &json!({"checkpoint":checkpoint,"forget":true}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;
    use solidcraft_engine::sketch::LinkSource;

    fn setup() -> (Harness, String) {
        let (mut h, link, _) = crate::test_support::selected_text("II", [3, 4]);
        h.app.start("sketch.move");
        h.frames(3);
        assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::TextMove(_))));
        (h, link)
    }

    fn replace(h: &mut Harness, field: &str, value: &str) {
        h.step(&json!({"click":{"handle":field}})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"text":value})).unwrap();
    }

    #[test]
    fn text_move_dialog_uses_units_and_one_undo_and_preserves_cancel() {
        let (mut h, link) = setup();
        let before = h.app.session.doc.to_json();
        let undo = h.app.session.undo.len();
        replace(&mut h, "dialog:Distance X", "1 in");
        replace(&mut h, "dialog:Distance Y", "-2 mm");
        replace(&mut h, "dialog:Angle", "90 deg");
        assert_eq!(h.app.session.doc.to_json(), before);
        h.step(&json!({"click":{"handle":"dialog:OK"}})).unwrap();
        h.frames(2);
        assert!(h.app.dialog.is_none());
        let sk = h.app.session.doc.sketch(1).unwrap();
        let LinkSource::Text { at, angle, text, .. } = &sk.link(&link).unwrap().source else { panic!() };
        assert!(at.dist(solidcraft_engine::geom::Vec2::new(28.4, 2.0)) < 1e-8);
        assert!((*angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert_eq!(text, "II");
        assert_eq!(h.app.session.undo.len(), undo + 1);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), before);
        let curve = h.app.session.doc.sketch(1).unwrap().curves[0].id.clone();
        h.app.session.selection = vec![Sel::SketchCurve { id: curve, sketch: None }];
        h.app.start("sketch.move");
        h.frames(3);
        replace(&mut h, "dialog:Distance X", "17");
        h.step(&json!({"widget":"Cancel"})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), before);
        assert_eq!(h.app.session.undo.len(), undo);
    }

    #[test]
    fn text_move_dialog_copies_an_editable_source() {
        let (mut h, link) = setup();
        let original = h.app.session.doc.sketch(1).unwrap().link(&link).unwrap().source.clone();
        replace(&mut h, "dialog:Distance X", "20");
        h.step(&json!({"click":{"handle":"dialog:Create Copy"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:OK"}})).unwrap();
        h.frames(2);
        let sk = h.app.session.doc.sketch(1).unwrap();
        assert_eq!(sk.links.len(), 2);
        assert_eq!(sk.link(&link).unwrap().source, original);
        let new_link = sk.links.iter().find(|l| l.id != link).unwrap().id.clone();
        h.app.run("sketch.edit_text", json!({"link":new_link,"text":"O"})).unwrap();
        assert_eq!(h.app.session.doc.sketch(1).unwrap().link(&link).unwrap().source, original);
    }

    #[test]
    fn text_move_cancels_before_switching_documents() {
        let (mut h, link) = setup();
        crate::dialogs::cancel(&mut h.app);
        let before = h.app.session.doc.to_json();
        let checkpoint = h.app.session.execute("edit.checkpoint", &json!({})).unwrap()["checkpoint"].as_u64().unwrap();
        let curve = h.app.session.doc.sketch(1).unwrap().curves[0].id.clone();
        h.app.session.execute("sketch.drag", &json!({"entity":curve,"from":[0,0],"to":[2,3],"live":true})).unwrap();
        h.app.viewport.text_drag = Some((checkpoint, link));
        h.app.viewport.point_drag = Some((curve, 0, None));
        crate::documents::open_tab(&mut h.app, solidcraft_engine::Session::default()).unwrap();
        assert!(h.app.viewport.text_drag.is_none());
        assert!(h.app.viewport.point_drag.is_none());
        crate::documents::switch(&mut h.app, 0);
        assert_eq!(h.app.session.doc.to_json(), before);
    }
}

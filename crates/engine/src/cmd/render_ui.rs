//! Planned Render interactions, with explicit non-mutating refusals.
use super::CommandSpec;

macro_rules! waiting {
    ($id:literal, $label:literal, $tab:literal, $panel:literal, $reason:literal) => {
        CommandSpec::new($id, $label, |_, _| Err(crate::EngineError::Other(concat!("not supported yet: ", $reason).into())))
            .enabled(|_| Err(concat!("not supported yet: ", $reason).into()))
            .planned("5493")
            .noundo()
            .doc_only()
            .at($tab, $panel)
            .icon("placeholder")
            .params("No supported parameters. Always refuses without changing the design or history.")
    };
}

pub static COMMANDS: &[CommandSpec] = &[
    waiting!("appearance.paint", "Paint Appearance", "RENDER APPEARANCE", "APPEARANCE", "product picking and appearance painting"),
    waiting!("appearance.pick_up", "Pick Up Appearance", "RENDER APPEARANCE", "APPEARANCE", "product picking and appearance sampling"),
    waiting!("appearance.replace", "Replace Appearance", "RENDER APPEARANCE", "APPEARANCE", "replacing all uses of an appearance"),
    waiting!("render.lights.solo", "Solo Light", "RENDER SCENE", "LIGHTS", "light visibility and solo controls"),
    waiting!("render.compare", "Compare Images", "RENDER", "RESULT", "retained image comparison"),
    waiting!("render.batch", "Render All Shots", "RENDER", "BATCH", "the saved-shot render queue"),
];

#[cfg(test)]
#[test]
fn render_workspace_planned_commands_refuse_without_mutation() {
    use serde_json::{Value, json};
    let mut session = crate::Session::default();
    session.execute("render.settings.set", &json!({"name":"Studio","set":{"width":800}})).unwrap();
    session.execute("render.settings.set", &json!({"name":"Studio","set":{"width":900}})).unwrap();
    session.execute("edit.undo", &json!({})).unwrap();
    let document = session.doc.clone();
    let before = (session.undo.len(), session.redo.len(), session.revision, session.evaluations, session.is_dirty());
    for command in COMMANDS {
        assert!(command.planned.is_some());
        assert!(!command.info(&session).enabled);
        for params in [json!({}), Value::Null, json!([]), json!({"target":1e300,"name":"hostile"})] {
            let error = session.execute(command.id, &params).unwrap_err().to_string();
            assert!(error.contains("not supported yet"), "{error}");
            assert!(std::sync::Arc::ptr_eq(&document, &session.doc));
            assert_eq!(before, (session.undo.len(), session.redo.len(), session.revision, session.evaluations, session.is_dirty()));
        }
    }
}

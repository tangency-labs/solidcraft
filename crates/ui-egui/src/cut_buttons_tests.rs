//! The cut buttons (#7905): Extrude Cut, Revolve Cut, Sweep Cut and Loft Cut open their tool's
//! dialog with the operation set to Cut, and a preference makes the first two buttons beside
//! Extrude and Revolve.
use serde_json::json;

use crate::dialogs::{CUT_TOOLS, Dialog, Kind, OPS, apply_commands};
use crate::scenario::Harness;

fn operation(d: &Dialog) -> Option<&'static str> {
    match &d.kind {
        Kind::Extrude { operation, .. } | Kind::Revolve { operation, .. } | Kind::Sweep { operation, .. } | Kind::Loft { operation, .. } => {
            OPS.get(*operation).copied()
        }
        _ => None,
    }
}

#[test]
fn each_cut_command_opens_its_tools_dialog_set_to_cut() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Stock"})).unwrap();
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Pocket"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [3, 3], "p1": [7, 7]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    for (cut, tool) in CUT_TOOLS {
        let d = Dialog::for_command(&h.app, cut).unwrap_or_else(|| panic!("{cut}: no dialog"));
        let plain = Dialog::for_command(&h.app, tool).unwrap();
        assert_eq!(std::mem::discriminant(&d.kind), std::mem::discriminant(&plain.kind), "{cut}: {tool}'s dialog");
        assert_eq!(operation(&d), Some("cut"), "{cut}");
        assert_ne!(operation(&plain), Some("cut"), "{tool} keeps its own default");
    }
    assert!(matches!(Dialog::for_command(&h.app, "solid.extrude_cut").unwrap().kind, Kind::Extrude { auto_op: false, .. }), "not guessed");

    // From the toolbar's start, OK cuts the pocket out of the box.
    h.app.start("solid.extrude_cut");
    h.frames(2);
    let d = h.app.dialog.as_ref().expect("the Extrude dialog");
    let commands = apply_commands(&h.app, d).unwrap();
    assert_eq!(commands[0].0, "solid.extrude");
    assert_eq!(commands[0].1["operation"], "cut");
}

/// The cut commands (#7905) shown in the ribbon's CREATE menu, the Create menu's drop-down and
/// command search, as the person sees them.
fn offered_cuts(h: &mut Harness) -> (Vec<&'static str>, Vec<&'static str>, Vec<&'static str>) {
    let lists = crate::toolbar_custom::panel_lists("MODEL");
    let (_, cmds, _) = lists.iter().find(|(panel, ..)| *panel == "CREATE").unwrap();
    let cut = |id: &str| CUT_TOOLS.iter().find(|(c, _)| *c == id).map(|(c, _)| *c);
    let menu = cmds.iter().filter(|c| crate::workspace::offered(&h.app, c.id)).filter_map(|c| cut(c.id)).collect();
    let search = crate::shortcut_box::search(&h.app, "cut").into_iter().filter_map(|(id, ..)| cut(id)).collect();
    h.call("ui.set", json!({"tab": "MODEL"}));
    h.settle();
    h.step(&json!({"click":{"handle":"panel:MODEL:CREATE"}})).unwrap();
    h.settle();
    let drawn = CUT_TOOLS.iter().map(|(c, _)| *c).filter(|c| crate::scenario::handle_at(&format!("dropdown:{c}")).is_some()).collect();
    h.step(&json!({"key":"Escape"})).unwrap();
    h.settle();
    (menu, search, drawn)
}

#[test]
fn the_preference_puts_extrude_cut_and_revolve_cut_beside_their_tools() {
    let mut h = Harness::new();
    let lists = crate::toolbar_custom::panel_lists("MODEL");
    let (_, cmds, promoted) = lists.iter().find(|(panel, ..)| *panel == "CREATE").unwrap();
    let ids =
        |h: &Harness| -> Vec<&str> { crate::toolbar_custom::buttons(&h.app, "MODEL", "CREATE", cmds, *promoted).iter().map(|c| c.id).collect() };
    let all: Vec<&str> = CUT_TOOLS.iter().map(|(c, _)| *c).collect();
    // Off, as by default: MODEL › CREATE and command search look as they do without this change.
    assert_eq!(ids(&h), ["solid.extrude", "solid.revolve", "solid.hole"], "off by default");
    assert_eq!(offered_cuts(&mut h), (vec![], vec![], vec![]), "no menu entries, no search rows, nothing drawn");
    assert!(all.iter().all(|id| solidcraft_engine::find_command(id).is_some()), "still commands for scripts and keys");
    // On: the buttons follow their tools, and all four are in the menu and in search.
    h.call("ui.prefs", json!({"cut_buttons": true}));
    assert_eq!(ids(&h), ["solid.extrude", "solid.extrude_cut", "solid.revolve", "solid.revolve_cut", "solid.hole"]);
    let (menu, search, drawn) = offered_cuts(&mut h);
    assert_eq!(menu, all);
    assert_eq!(drawn, all, "in the drop-down");
    assert!(search.contains(&"solid.extrude_cut") && search.contains(&"solid.loft_cut"), "{search:?}");
    h.settle();
    assert!(crate::scenario::handle_at("button:solid.extrude_cut").is_some(), "drawn in the ribbon");
    // Off again: gone, even a cut button someone pinned while it was on.
    h.app.toolbar_custom.panels.insert("DESIGN/MODEL/CREATE".into(), vec!["solid.extrude".into(), "solid.extrude_cut".into()]);
    h.call("ui.prefs", json!({"cut_buttons": false}));
    assert_eq!(ids(&h), ["solid.extrude"]);
    assert_eq!(offered_cuts(&mut h), (vec![], vec![], vec![]));
}

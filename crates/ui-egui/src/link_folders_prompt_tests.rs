use super::*;
use crate::scenario::Harness;
use serde_json::json;

/// An assembly whose linked parts each sit in their own folder: the folders are listed one a line
/// in a list that scrolls, and the title and the buttons stay on screen in a short window.
#[test]
fn many_folders_scroll_and_keep_the_buttons_on_screen() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width": 1280, "height": 720}));
    let folders: Vec<String> = (0..60).map(|i| format!("/home/user/designs/pod/parts/241-{i:04}-00-A-part-with-a-long-name")).collect();
    let session = h.app.session.uid;
    h.app.link_folders_prompt.push(LinkFoldersPrompt { session, folders: folders.clone(), components: vec![1, 2] });
    h.frames(3);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));
    let list = crate::scenario::rect_of("dialog:list:link_folders").expect("the folders are listed");
    assert!(list.height() < 400.0, "the list is held to a few rows, not one per folder: {list:?}");
    for (handle, label) in [("dialog:ok", "Load"), ("dialog:cancel", "Not now")] {
        let at = crate::scenario::handle_at(handle).unwrap_or_else(|| panic!("{label} is shown"));
        assert!(screen.contains(at), "{label} is on screen at {at:?}");
    }
    let header = crate::scenario::rect_of("dialog:header").expect("the title is shown");
    assert!(screen.contains_rect(header), "the title is on screen: {header:?}");
    assert!(h.widget(&folders[0]).is_some(), "the folders are listed one a line");
    // The list's corner drags it taller and wider; the dialog widens with it, its close button
    // follows the list's right edge, and the buttons stay on screen.
    let close = crate::scenario::handle_at("dialog:close").expect("the close button is shown");
    let t = solidcraft_theme::Tokens::get();
    let grip = egui::pos2(list.right() - t.space_10 - 3.0, list.bottom() - 3.0);
    let to = grip + egui::vec2(200.0, 150.0);
    h.frame_with(vec![egui::Event::PointerMoved(grip)]);
    h.frame_with(vec![egui::Event::PointerButton { pos: grip, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
    for step in 1..=10 {
        h.frame_with(vec![egui::Event::PointerMoved(grip + (to - grip) * step as f32 / 10.0)]);
    }
    h.frame_with(vec![egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
    h.frames(2);
    let taller = crate::scenario::rect_of("dialog:list:link_folders").expect("the folders are still listed");
    assert!(taller.height() > list.height() + 100.0, "dragging the corner makes the list taller: {list:?} -> {taller:?}");
    assert!(taller.width() > list.width() + 100.0, "and wider: {list:?} -> {taller:?}");
    let moved = crate::scenario::handle_at("dialog:close").expect("the close button is still shown");
    assert!(moved.x > close.x + 50.0, "the close button moves right with the dialog: {close:?} -> {moved:?}");
    assert!((moved.x - taller.right()).abs() < 60.0, "the close button sits over the list's right edge: {moved:?}, {taller:?}");
    assert!(screen.contains(moved), "the close button is on screen at {moved:?}");
    for (handle, label) in [("dialog:ok", "Load"), ("dialog:cancel", "Not now")] {
        let at = crate::scenario::handle_at(handle).unwrap_or_else(|| panic!("{label} is shown"));
        assert!(screen.contains(at), "{label} is still on screen at {at:?}");
    }
    // Not now still answers.
    h.step(&json!({"click": {"handle": "dialog:cancel"}})).unwrap();
    assert!(h.app.link_folders_prompt.is_empty(), "Not now closes the question");
    assert!(held(&h.app, 1) && held(&h.app, 2), "Not now holds the links back");
}

use super::*;
use crate::scenario::Harness;
use serde_json::json;

/// An assembly whose linked parts each sit in their own folder: the folders are listed one a line
/// in a list that scrolls, and the title and the buttons stay on screen in a short window. The
/// list's corner drags it taller and wider, and the dialog follows in the same frame.
#[test]
fn many_folders_scroll_and_keep_the_buttons_on_screen() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width": 1280, "height": 720}));
    let session = h.app.session.uid;
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));
    // A question about one folder first: its short list is not where the next question starts.
    h.app.link_folders_prompt.push(LinkFoldersPrompt { session, folders: vec!["/home/user/designs/bracket".into()], components: vec![9] });
    h.frames(3);
    let one = crate::scenario::rect_of("dialog:list:link_folders").expect("the folder is listed");
    h.step(&json!({"click": {"handle": "dialog:cancel"}})).unwrap();
    h.frames(2);

    let folders: Vec<String> = (0..60).map(|i| format!("/home/user/designs/pod/parts/241-{i:04}-00-A-part-with-a-long-name")).collect();
    h.app.link_folders_prompt.push(LinkFoldersPrompt { session, folders: folders.clone(), components: vec![1, 2] });
    h.frames(3);
    let list = crate::scenario::rect_of("dialog:list:link_folders").expect("the folders are listed");
    assert!(list.height() < 400.0, "the list is held to a few rows, not one per folder: {list:?}");
    assert!(list.height() > one.height() + 50.0, "a longer list starts taller than the one-folder list before it: {one:?} -> {list:?}");
    for (handle, label) in [("dialog:ok", "Load"), ("dialog:cancel", "Not now")] {
        let at = crate::scenario::handle_at(handle).unwrap_or_else(|| panic!("{label} is shown"));
        assert!(screen.contains(at), "{label} is on screen at {at:?}");
    }
    let header = crate::scenario::rect_of("dialog:header").expect("the title is shown");
    assert!(screen.contains_rect(header), "the title is on screen: {header:?}");
    assert!(h.widget(&folders[0]).is_some(), "the folders are listed one a line");

    // Drag the corner 200 px right and 150 px down. On every frame of the drag the list's right
    // edge keeps its place under the title row's, so the list never runs past the dialog.
    let close = crate::scenario::handle_at("dialog:close").expect("the close button is shown");
    let edge = header.right() - list.right();
    let grip = crate::scenario::handle_at("dialog:resize:link_folders").expect("the list has a corner grip");
    let to = grip + egui::vec2(200.0, 150.0);
    h.frame_with(vec![egui::Event::PointerMoved(grip)]);
    h.frame_with(vec![egui::Event::PointerButton { pos: grip, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
    for step in 1..=5 {
        h.frame_with(vec![egui::Event::PointerMoved(grip + (to - grip) * step as f32 / 5.0)]);
        let list = crate::scenario::rect_of("dialog:list:link_folders").expect("the folders are listed while dragging");
        let header = crate::scenario::rect_of("dialog:header").expect("the title is shown while dragging");
        assert!((header.right() - list.right() - edge).abs() < 3.0, "step {step}: the list stays inside the dialog: {header:?}, {list:?}");
    }
    h.frame_with(vec![egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
    h.frames(2);
    let taller = crate::scenario::rect_of("dialog:list:link_folders").expect("the folders are still listed");
    assert!(taller.height() > list.height() + 100.0, "dragging the corner makes the list taller: {list:?} -> {taller:?}");
    assert!(taller.width() > list.width() + 100.0, "and wider: {list:?} -> {taller:?}");
    let moved = crate::scenario::handle_at("dialog:close").expect("the close button is still shown");
    assert!(moved.x > close.x + 50.0, "the close button moves right with the dialog: {close:?} -> {moved:?}");
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

/// The question's sentence names no folders: they are in the list below it. No language may
/// still show the placeholder the sentence once had.
#[test]
fn no_language_shows_a_placeholder_in_the_question() {
    let _h = Harness::new();
    assert!(!solidcraft_ui_text::locales().is_empty(), "the catalogs are installed");
    for catalog in solidcraft_ui_text::locales() {
        let text = solidcraft_ui_text::Locale::for_language(catalog.id).text("linked_files.other_folders_detail");
        assert!(!text.contains('{'), "{}: {text}", catalog.id);
    }
}

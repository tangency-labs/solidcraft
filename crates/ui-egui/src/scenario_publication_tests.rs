use super::*;

#[test]
fn publication_keeps_order_aliases_and_last_value() {
    clear_handles();
    let first = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(30.0, 40.0));
    let second = first.translate(egui::vec2(80.0, 90.0));
    let at = first.left_center();
    publish_target("target", first);
    publish_rect("target", second);
    publish_handle("target", at);
    publish_target_as("alias_handle", "alias_rect", second);
    assert_eq!(handles(""), vec![("target".into(), first.center()), ("target".into(), at), ("alias_handle".into(), second.center())]);
    assert_eq!(rects(""), vec![("target".into(), first), ("target".into(), second), ("alias_rect".into(), second)]);
    assert_eq!(handle_at("target"), Some(at));
    assert_eq!(rect_of("target"), Some(second));
    clear_handles();
    assert!(handles("").is_empty());
    assert!(rects("").is_empty());
}

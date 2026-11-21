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

fn widget_node(role: egui::accesskit::Role, text: &str, x: f64) -> egui::accesskit::Node {
    let mut node = egui::accesskit::Node::new(role);
    node.set_label(text);
    node.set_bounds(egui::accesskit::Rect { x0: x, y0: 10.0, x1: x + 20.0, y1: 30.0 });
    node
}

#[test]
fn widget_lookup_prefers_click_targets_to_same_text_tooltips_in_either_node_order() {
    use egui::accesskit::{Action, NodeId, Role};
    for (role, text) in [(Role::Button, "Panel actions"), (Role::MenuItem, "Right")] {
        let mut control = widget_node(role, text, 10.0);
        control.add_action(Action::Click);
        let tooltip = widget_node(Role::Label, text, 100.0);
        let mut nodes = vec![(NodeId(1), control), (NodeId(2), tooltip)];
        for _ in 0..2 {
            assert_eq!(widget_centres(&nodes, text).first(), Some(&[20.0, 20.0]), "{role:?}");
            nodes.reverse();
        }
    }
}

#[test]
fn widget_lookup_keeps_labels_values_and_shortcut_fallbacks() {
    use egui::accesskit::{NodeId, Role};
    let label = widget_node(Role::Label, "Type", 10.0);
    let mut field = widget_node(Role::TextInput, "", 40.0);
    field.set_value("20");
    let shortcut = widget_node(Role::Button, "Save Ctrl+S", 70.0);
    let nodes = vec![(NodeId(1), label), (NodeId(2), field), (NodeId(3), shortcut)];
    assert_eq!(widget_centres(&nodes, "Type"), vec![[20.0, 20.0]]);
    assert_eq!(widget_centres(&nodes, "20"), vec![[50.0, 20.0]]);
    assert_eq!(widget_centres(&nodes, "Save"), vec![[80.0, 20.0]]);
    assert!(widget_centres(&nodes, "Missing").is_empty());
}

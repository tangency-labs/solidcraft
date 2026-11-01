use super::*;
use crate::ui_kit::{
    FinishKind,
    test_support::{frame, pointer},
};

#[test]
fn chain_has_one_active_ring_and_empty_slots_and_returns_each_click() {
    let ctx = crate::fonts::test_context();
    let t = Tokens::get();
    let sphere = Sphere { color: t.render_accent, kind: FinishKind::Satin, opacity: None };
    for active in LookStep::ALL {
        for filled in [false, true] {
            let looks = if filled { [Some(sphere); 4] } else { [None; 4] };
            let clicked = std::cell::Cell::new(None);
            let mut draw = |ui: &mut Ui| {
                clicked.set(look_chain(ui, "chain", ["Face", "Body", "Component", "Material"], looks, active));
            };
            frame(&ctx, vec![], &mut draw);
            assert_eq!(clicked.get(), None);
            for step in LookStep::ALL {
                let key = format!("chain:{}", step.name());
                let pos = scenario::handle_at(&key).unwrap();
                assert!(scenario::rect_of("chain").unwrap().contains(pos));
                frame(&ctx, pointer(pos, true), &mut draw);
                frame(&ctx, pointer(pos, false), &mut draw);
                assert_eq!(clicked.get(), Some(step));
            }
            let output = ctx.run_ui(Default::default(), |ui| {
                look_chain(ui, "paint", ["Face", "Body", "Component", "Material"], looks, active);
            });
            let rings: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Circle(c) if c.stroke.color == t.render_accent => Some(c),
                    _ => None,
                })
                .collect();
            assert_eq!(rings.len(), 1);
            let active_rect = scenario::rect_of(&format!("paint:{}", active.name())).unwrap();
            assert_eq!(rings[0].center.x, active_rect.center().x);
            let meshes = output.shapes.iter().filter(|s| matches!(s.shape, egui::Shape::Mesh(_))).count();
            let dashes = output.shapes.iter().filter(|s| matches!(s.shape, egui::Shape::Path(_))).count();
            assert_eq!(meshes, if filled { 4 } else { 0 });
            assert_eq!(dashes, if filled { 0 } else { 48 });
        }
    }
}

#[test]
fn disabled_chain_does_not_return_clicks() {
    let ctx = crate::fonts::test_context();
    let mut clicked = None;
    let mut draw = |ui: &mut Ui| {
        ui.add_enabled_ui(false, |ui| {
            clicked = look_chain(ui, "disabled", ["Face", "Body", "Component", "Material"], [None; 4], LookStep::Body);
        });
    };
    frame(&ctx, vec![], &mut draw);
    let pos = scenario::handle_at("disabled:face").unwrap();
    frame(&ctx, pointer(pos, true), &mut draw);
    frame(&ctx, pointer(pos, false), &mut draw);
    assert_eq!(clicked, None);
}

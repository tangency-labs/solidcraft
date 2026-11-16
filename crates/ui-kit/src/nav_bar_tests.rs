//! The bar component driven with plain data, no app: what each click returns.
use super::*;
use crate::{NavigationEdge, NavigationIdle, NavigationKind};
use egui::{Event, PointerButton, RawInput};

fn slot(index: usize, id: &'static str, kind: NavigationKind, x: f32) -> SlotView<'static> {
    SlotView {
        index,
        id,
        kind,
        icon: id,
        text: id.to_owned(),
        name: id.to_owned(),
        tip: id.to_owned(),
        key: None,
        selected: false,
        enabled: true,
        hidden: false,
        show_text: false,
        rect: Rect::from_min_size(pos2(x, 304.0), vec2(26.0, 26.0)),
        alias: None,
    }
}

/// Fit and Styles on the bar; Zoom and View folded into More.
fn bar() -> Bar<'static> {
    Bar {
        rect: Rect::from_min_size(pos2(90.0, 300.0), vec2(110.0, 34.0)),
        size: Default::default(),
        edge: NavigationEdge::Bottom,
        idle: NavigationIdle::Always,
        active: false,
        slots: vec![
            slot(0, "fit", NavigationKind::Action, 100.0),
            SlotView { alias: Some("test:styles"), ..slot(1, "styles", NavigationKind::Menu, 134.0) },
        ],
        overflow: vec![slot(2, "zoom", NavigationKind::Action, 0.0), slot(3, "view", NavigationKind::Menu, 0.0)],
        more: Rect::from_min_size(pos2(168.0, 304.0), vec2(26.0, 26.0)),
        labels: Default::default(),
    }
}

/// Styles closes on a pick; View stays open and has a submenu.
fn menu(id: &str) -> Menu {
    match id {
        "styles" => Menu { entries: vec![MenuEntry::Row(MenuRow::new("wire", "Wire", "", "test:wire"))], close_on_pick: true },
        "view" => {
            let mut preset = MenuRow::new("preset", "Preset", "", "test:preset");
            preset.submenu = vec![MenuRow::new("fast", "Fast", "", "test:fast")];
            Menu {
                entries: vec![
                    MenuEntry::Heading("Show".into()),
                    MenuEntry::Row(MenuRow::new("grid", "Grid", "", "test:grid")),
                    MenuEntry::Separator,
                    MenuEntry::Row(preset),
                ],
                close_on_pick: false,
            }
        }
        _ => Menu::default(),
    }
}

struct Rig {
    ctx: egui::Context,
    bar: Bar<'static>,
}

impl Rig {
    fn new() -> Rig {
        let rig = Rig { ctx: crate::test_support::context(), bar: bar() };
        rig.frame(Vec::new());
        rig
    }

    fn frame(&self, events: Vec<Event>) -> Option<NavAction> {
        solidcraft_ui_registry::clear_handles();
        let mut action = None;
        crate::test_support::raw_frame(
            &self.ctx,
            RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 400.0))), events, focused: true, ..Default::default() },
            |ui| action = show(ui, &self.bar, menu),
        );
        action
    }

    /// A press and release of `button` at `at`, then two quiet frames so a popup it opened is
    /// measured and placed: what the release returned.
    fn click_with(&self, at: egui::Pos2, button: PointerButton) -> Option<NavAction> {
        let press = |pressed| vec![Event::PointerMoved(at), Event::PointerButton { pos: at, button, pressed, modifiers: Default::default() }];
        self.frame(vec![Event::PointerMoved(at)]);
        let action = self.frame(press(true)).or(self.frame(press(false)));
        self.frame(Vec::new());
        self.frame(Vec::new());
        action
    }

    fn click(&self, at: egui::Pos2) -> Option<NavAction> {
        self.click_with(at, PointerButton::Primary)
    }

    fn click_target(&self, name: &str) -> Option<NavAction> {
        let at = solidcraft_ui_registry::rect_of(name).expect("the target is drawn").center();
        self.click(at)
    }

    fn drawn(&self, name: &str) -> bool {
        solidcraft_ui_registry::rect_of(name).is_some()
    }
}

#[test]
fn a_slot_or_a_row_of_its_menu_is_returned_and_a_closing_menu_closes() {
    let rig = Rig::new();
    assert_eq!(rig.click(pos2(113.0, 317.0)), Some(NavAction::Slot("fit".into())), "an action slot is returned, not run");
    assert_eq!(rig.click(pos2(147.0, 317.0)), None, "a menu slot opens its menu");
    assert_eq!(solidcraft_ui_registry::handle_at("test:styles"), Some(pos2(147.0, 317.0)), "a slot's alias names it too");
    assert_eq!(rig.click_target("test:wire"), Some(NavAction::Item { menu: "styles".into(), item: "wire".into() }));
    assert!(!rig.drawn("test:wire"), "a pick closes a menu that asks for it");
}

#[test]
fn more_returns_a_folded_slot_and_opens_a_folded_menu_that_stays_open() {
    let rig = Rig::new();
    assert_eq!(rig.click_target("nav:more"), None);
    assert_eq!(rig.click_target("nav:slot:zoom"), Some(NavAction::Slot("zoom".into())), "a folded slot is returned");
    assert!(!rig.drawn("nav:slot:zoom"), "and More closes");
    rig.click_target("nav:more");
    assert_eq!(rig.click_target("nav:slot:view"), None, "a folded menu slot opens its menu");
    assert!(rig.drawn("test:grid") && !rig.drawn("nav:slot:zoom"), "in place of More");
    assert_eq!(rig.click_target("test:grid"), Some(NavAction::Item { menu: "view".into(), item: "grid".into() }));
    assert!(rig.drawn("test:grid"), "a menu that does not close on a pick stays open");
    assert_eq!(rig.click_target("test:preset"), None, "a row with a submenu opens it");
    assert_eq!(rig.click_target("test:fast"), Some(NavAction::Item { menu: "view".into(), item: "fast".into() }), "a submenu row by its own id");
}

#[test]
fn the_context_menus_return_customise_hide_reset_and_remove_with_the_slot_index() {
    let rig = Rig::new();
    for (id, item) in [("customise", ContextItem::Customise), ("hide", ContextItem::Hide), ("reset", ContextItem::Reset)] {
        rig.click_with(pos2(94.0, 317.0), PointerButton::Secondary);
        assert!(!rig.drawn("nav:context:remove"), "the bar's own menu");
        assert_eq!(rig.click_target(&format!("nav:context:{id}")), Some(NavAction::Context(item)));
    }
    rig.click_with(pos2(147.0, 317.0), PointerButton::Secondary);
    assert!(!rig.drawn("nav:context:hide"), "a slot's menu");
    assert_eq!(rig.click_target("nav:context:remove"), Some(NavAction::Context(ContextItem::Remove(1))), "removes the slot at its stored index");
    rig.click_target("nav:more");
    rig.frame(vec![Event::PointerMoved(pos2(0.0, 0.0))]);
    let folded = solidcraft_ui_registry::rect_of("nav:slot:view").expect("More is open").center();
    rig.click_with(folded, PointerButton::Secondary);
    assert_eq!(rig.click_target("nav:context:remove"), Some(NavAction::Context(ContextItem::Remove(3))), "a folded slot too");
}

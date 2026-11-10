//! The ribbon from plain data, no app: how groups fit, and what a click on the tab row or the
//! panel row returns.
use super::*;

fn sizes(buttons: &[usize]) -> Vec<PanelSize> {
    buttons.iter().map(|&b| PanelSize { buttons: vec![ButtonSize { width: BUTTON_MIN_WIDTH, min_width: BUTTON_MIN_WIDTH }; b] }).collect()
}

fn used_with_more(plans: &[Plan]) -> f32 {
    used(plans) + if plans.iter().any(|p| p.fit == Fit::More) { MORE_ROOM } else { 0.0 }
}

#[test]
fn panels_give_up_one_button_at_a_time_then_move_to_more_and_none_is_dropped() {
    // A panel with 14 buttons and three default ones.
    let panels = sizes(&[14, 4, 2, 2]);
    let full: f32 = [14, 4, 2, 2].iter().map(|&b| panel_width(&vec![BUTTON_MIN_WIDTH; b]) + PANEL_GAP).sum();
    let all = |plans: &[Plan]| plans.iter().map(|p| (p.fit, p.shown)).collect::<Vec<_>>();
    assert_eq!(all(&fit(&panels, full)), [(Fit::Buttons, 14), (Fit::Buttons, 4), (Fit::Buttons, 2), (Fit::Buttons, 2)]);
    // A little short: the widest panel hands one button to its drop-down; the others keep all theirs.
    assert_eq!(all(&fit(&panels, full - 10.0)), [(Fit::Buttons, 13), (Fit::Buttons, 4), (Fit::Buttons, 2), (Fit::Buttons, 2)]);
    // Every panel down to one button (collapsed) fits.
    let collapsed = 4.0 * (COLLAPSED_WIDTH + PANEL_GAP);
    assert_eq!(all(&fit(&panels, collapsed)), [(Fit::Collapsed, 1); 4]);
    // Narrower: the last one goes into the » menu, and the room it frees goes back to buttons.
    let more = fit(&panels, collapsed - 1.0);
    assert_eq!(more.iter().map(|p| p.fit).collect::<Vec<_>>(), [Fit::Collapsed, Fit::Collapsed, Fit::Collapsed, Fit::More]);
    assert!(more.iter().take(3).all(|p| p.shown >= 1));
    let more = fit(&sizes(&[14, 4, 2, 2, 2, 2]), 5.0 * (COLLAPSED_WIDTH + PANEL_GAP) + MORE_ROOM - 1.0);
    assert_eq!(more.last().unwrap().fit, Fit::More);
    assert!(more.iter().any(|p| p.fit == Fit::Buttons && p.shown > 1), "overflow room restores buttons");
    // Then all of them.
    assert!(fit(&panels, 0.0).iter().all(|p| p.fit == Fit::More));
    assert_eq!(fit(&panels, f32::NAN).len(), 4);
    // No cap: every additional button keeps its width and gap.
    assert_eq!(panel_width(&[BUTTON_MIN_WIDTH; 30]) - panel_width(&[BUTTON_MIN_WIDTH; 20]), 10.0 * (BUTTON_MIN_WIDTH + BUTTON_GAP));
}

/// #2108: whatever the room, the toolbar uses it. It never leaves more than one button's width
/// blank while something is hidden, never runs past the room, and shrinks steadily as the
/// room does (so a few pixels never flip it back and forth).
#[test]
fn panels_fill_the_room_they_have_at_every_width() {
    let layouts = [
        // SOLID: CREATE, MODIFY, ASSEMBLE, CONFIGURE, CONSTRUCT, INSPECT, INSERT, SELECT.
        vec![(4, false), (4, false), (3, false), (2, false), (2, false), (2, false), (2, false), (0, false)],
        // SKETCH: labelled CREATE, then MODIFY … FINISH SKETCH.
        vec![(8, true), (2, false), (3, false), (6, false), (2, false), (2, false), (0, false), (1, false)],
        // Many pins on one panel.
        vec![(14, false), (4, false), (2, false), (2, false)],
    ];
    for layout in layouts {
        let panels: Vec<PanelSize> = layout
            .iter()
            .map(|&(buttons, labelled)| PanelSize {
                buttons: (0..buttons)
                    .map(|i| ButtonSize {
                        width: if labelled { BUTTON_MAX_WIDTH - (i % 3) as f32 * 5.0 } else { BUTTON_MIN_WIDTH },
                        min_width: BUTTON_MIN_WIDTH,
                    })
                    .collect(),
            })
            .collect();
        let mut last: Option<(f32, usize)> = None;
        for room in (0..=2000).rev().map(|r| r as f32) {
            let plans = fit(&panels, room);
            let width = used_with_more(&plans);
            let hidden = plans
                .iter()
                .zip(&panels)
                .any(|(p, s)| p.fit != Fit::Buttons || p.shown < s.buttons.len() || p.widths.iter().zip(&s.buttons).any(|(w, b)| *w < b.width));
            let more = plans.iter().filter(|p| p.fit == Fit::More).count();
            if more < panels.len() {
                assert!(width <= room + 0.01, "{layout:?} at {room}: {width} overruns");
            }
            // One button: an icon's stride, or a collapsed panel's when the next panel waits in the » menu.
            let button = if more > 0 { COLLAPSED_WIDTH + PANEL_GAP } else { BUTTON_MIN_WIDTH + BUTTON_GAP };
            if hidden && more < panels.len() {
                assert!(room - width < button, "{layout:?} at {room}: {} px blank while something is hidden", room - width);
            }
            // Fewer pixels never bring a panel back out of the » menu.
            if let Some((_, prev_more)) = last {
                assert!(more >= prev_more, "{layout:?} at {room}: a panel came back out of the » menu");
            }
            last = Some((width, more));
            // A panel shows its icons: one at the very least.
            for (p, s) in plans.iter().zip(&panels) {
                if p.fit != Fit::More && !s.buttons.is_empty() {
                    assert!(p.shown >= 1, "{layout:?} at {room}: a panel without icons");
                }
            }
        }
    }
}

#[test]
fn variable_width_buttons_narrow_before_rightmost_equal_panels_lose_buttons() {
    let panels = vec![PanelSize {
        buttons: vec![
            ButtonSize { width: 76.0, min_width: BUTTON_MIN_WIDTH },
            ButtonSize { width: 63.0, min_width: BUTTON_MIN_WIDTH },
            ButtonSize { width: 54.0, min_width: BUTTON_MIN_WIDTH },
        ],
    }];
    let full = used(&panels.iter().map(Plan::full).collect::<Vec<_>>());
    let narrow = fit(&panels, full - 1.0);
    assert_eq!(narrow[0].shown, 3);
    assert_eq!(narrow[0].widths, [75.0, 62.0, 54.0]);
    let minimum = panel_width(&[BUTTON_MIN_WIDTH; 3]) + PANEL_GAP;
    let narrow = fit(&panels, minimum);
    assert_eq!(narrow[0].shown, 3);
    assert_eq!(narrow[0].widths, [BUTTON_MIN_WIDTH; 3]);
    assert_eq!(fit(&panels, minimum - 1.0)[0].shown, 2);

    let equal = sizes(&[3, 3]);
    let full = used(&equal.iter().map(Plan::full).collect::<Vec<_>>());
    assert_eq!(fit(&equal, full - 1.0).iter().map(|p| p.shown).collect::<Vec<_>>(), [3, 2]);
}

#[test]
fn constraint_gallery_stays_fixed_and_hands_all_cells_to_the_menu_together() {
    let panels = vec![PanelSize {
        buttons: vec![
            ButtonSize { width: BUTTON_MAX_WIDTH, min_width: BUTTON_MIN_WIDTH },
            ButtonSize { width: GALLERY_WIDTH, min_width: GALLERY_WIDTH },
        ],
    }];
    let minimum = panel_width(&[BUTTON_MIN_WIDTH, GALLERY_WIDTH]) + PANEL_GAP;
    let narrow = fit(&panels, minimum);
    assert_eq!(narrow[0].shown, 2);
    assert_eq!(narrow[0].widths, [BUTTON_MIN_WIDTH, GALLERY_WIDTH]);
    let collapsed = fit(&panels, minimum - 1.0);
    assert_eq!((collapsed[0].fit, collapsed[0].shown), (Fit::Collapsed, 1));
}

const HANDLES: Handles<'static> = Handles {
    button: &["t:button:"],
    button_rect: "t:rect:",
    split: "t:split:",
    cell: &["t:cell:"],
    collapsed: "t:collapsed:",
    section: "t:section:",
    section_target: "t:section_target:",
    group: "t:group:",
    running: "t:running:",
    reveal: "t:reveal",
    more: "t:more",
    more_row: "t:more_row:",
    collapsed_count: "t:collapsed",
    more_count: "t:in_more",
    trimmed_count: "t:trimmed",
    drag: "t:drag",
    drag_remove: "t:drag_remove",
};

fn button(id: &'static str, enabled: bool) -> Button<'static> {
    Button {
        id,
        run: id,
        label: id,
        name: id,
        icon: "box",
        size: ButtonSize { width: BUTTON_MAX_WIDTH, min_width: BUTTON_MIN_WIDTH },
        enabled,
        pressed: false,
        exit: false,
        split: None,
        app_paints: false,
        covers: &[],
    }
}

fn group(key: &'static str, buttons: Vec<Button<'static>>) -> Group<'static> {
    let more = buttons
        .iter()
        .map(|b| MoreRow { id: b.id, label: b.label, icon: b.icon, shortcut: "", enabled: b.enabled, checked: None, hint: None })
        .collect();
    let commands = buttons.iter().map(|b| b.id).collect();
    Group {
        key,
        name: key,
        label: key,
        tip: key,
        buttons,
        cells: Vec::new(),
        commands,
        icon: Some("box"),
        enabled: true,
        open: false,
        running: None,
        more,
    }
}

/// Two groups, the second with a disabled button, in a row `width` wide.
struct RowRig {
    ctx: egui::Context,
    width: f32,
    time: std::cell::Cell<f64>,
    /// Each text the last frame painted, at its centre.
    texts: std::cell::RefCell<Vec<(String, egui::Pos2)>>,
    /// The commands dragged off the row.
    removed: std::cell::RefCell<Vec<String>>,
}

impl RowRig {
    fn new(width: f32) -> RowRig {
        let rig = RowRig {
            ctx: crate::ui_kit::test_support::context(),
            width,
            time: std::cell::Cell::new(0.0),
            texts: Default::default(),
            removed: Default::default(),
        };
        rig.frame(Vec::new());
        rig
    }

    fn frame(&self, events: Vec<egui::Event>) -> Vec<Action> {
        solidcraft_ui_registry::clear_handles();
        self.time.set(self.time.get() + 1.0 / 60.0);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 400.0))),
            time: Some(self.time.get()),
            events,
            focused: true,
            ..Default::default()
        };
        let t = Tokens::get();
        let row = Row {
            groups: vec![
                group("A", vec![button("a.one", true), button("a.two", true)]),
                group("B", vec![button("b.on", true), button("b.off", false)]),
            ],
            labels: true,
            accent: crate::workspace::tab_accent(&t, "MODEL"),
            exit: false,
            reveal: None,
            more_tip: "More",
            handles: HANDLES,
        };
        let mut actions = Vec::new();
        let output = crate::ui_kit::test_support::raw_frame(&self.ctx, input, |ui| {
            let rect = Rect::from_min_size(pos2(0.0, 30.0), vec2(self.width, GROUP_HEIGHT + 1.0));
            actions = show(ui, &row, rect, &mut |_, _| false).actions;
            let dropped = actions.iter().any(|a| matches!(a, Action::Drop { .. }));
            self.removed.borrow_mut().extend(drag_end(ui, rect, None, dropped, &|_| Some("box".to_string()), &HANDLES));
        });
        *self.texts.borrow_mut() =
            crate::ui_kit::test_support::text_shapes(&output).map(|s| (s.galley.text().to_string(), s.visual_bounding_rect().center())).collect();
        actions
    }

    fn click(&self, at: egui::Pos2) -> Vec<Action> {
        self.time.set(self.time.get() + 1.0);
        let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        let mut actions = self.frame(vec![egui::Event::PointerMoved(at)]);
        actions.extend(self.frame(vec![egui::Event::PointerMoved(at), press(true)]));
        actions.extend(self.frame(vec![egui::Event::PointerMoved(at), press(false)]));
        actions
    }

    /// Where the last frame painted `text`.
    fn text(&self, text: &str) -> egui::Pos2 {
        self.texts.borrow().iter().find(|(t, _)| t == text).map(|(_, at)| *at).unwrap_or_else(|| panic!("{text} is painted"))
    }

    fn at(&self, name: &str) -> egui::Pos2 {
        solidcraft_ui_registry::handle_at(name)
            .or_else(|| solidcraft_ui_registry::rect_of(name).map(|r| r.center()))
            .unwrap_or_else(|| panic!("{name} is drawn"))
    }
}

/// A click on a button returns its command, with the button's rect; a disabled one returns
/// nothing; a click on a group's label asks for its menu.
#[test]
fn a_click_on_a_button_returns_its_command_and_a_disabled_one_nothing() {
    let rig = RowRig::new(800.0);
    let actions = rig.click(rig.at("t:button:a.two"));
    let [Action::Run { id, rect, place }] = actions.as_slice() else { panic!("{actions:?}") };
    assert_eq!((id.as_str(), *place), ("a.two", Place::Button));
    assert_eq!(Some(*rect), solidcraft_ui_registry::rect_of("t:rect:a.two"));
    assert_eq!(rig.click(rig.at("t:button:b.off")), Vec::new(), "a disabled button runs nothing");
    assert_eq!(rig.click(rig.at("t:section:B")), vec![Action::Section { group: 1 }]);
    assert_eq!(solidcraft_ui_registry::count_of("t:in_more"), Some(0.0));
}

/// Too narrow for both groups: the last goes into the » menu, whose rows run their commands, and
/// a disabled row runs nothing there either.
#[test]
fn a_group_that_does_not_fit_is_in_the_more_menu() {
    let rig = RowRig::new(COLLAPSED_WIDTH + MORE_ROOM + 8.0);
    assert_eq!(solidcraft_ui_registry::count_of("t:in_more"), Some(1.0));
    assert_eq!(solidcraft_ui_registry::count_of("t:collapsed"), Some(1.0));
    assert!(solidcraft_ui_registry::handle_at("t:button:b.on").is_none(), "B is not on the row");
    assert_eq!(rig.click(rig.at("t:collapsed:A")), vec![Action::Open { group: 0 }]);
    let open_b = || {
        assert_eq!(rig.click(rig.at("t:more")), Vec::new(), "» opens its menu");
        rig.frame(Vec::new());
        let b = rig.text("B");
        for _ in 0..3 {
            rig.frame(vec![egui::Event::PointerMoved(b)]);
        }
    };
    open_b();
    let actions = rig.click(rig.at("t:more_row:b.on"));
    let [Action::Run { id, place, .. }] = actions.as_slice() else { panic!("{actions:?}") };
    assert_eq!((id.as_str(), *place), ("b.on", Place::More));
    open_b();
    assert_eq!(rig.click(rig.at("t:more_row:b.off")), Vec::new(), "a disabled row runs nothing");
}

const TAB_HANDLES: TabHandles<'static> = TabHandles { tab: "t:tab:", tab_rect: "t:tab_rect:", add: "t:add", action: "t:action:" };

/// A tab row with three tabs (the last contextual, under an alias), the + button and two buttons
/// after the search field, one of them not ready.
struct TabsRig {
    ctx: egui::Context,
    time: std::cell::Cell<f64>,
}

impl TabsRig {
    fn new() -> TabsRig {
        let rig = TabsRig { ctx: crate::ui_kit::test_support::context(), time: std::cell::Cell::new(0.0) };
        rig.frame(Vec::new());
        rig
    }

    fn frame(&self, events: Vec<egui::Event>) -> Vec<TabAction> {
        solidcraft_ui_registry::clear_handles();
        self.time.set(self.time.get() + 1.0 / 60.0);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 400.0))),
            time: Some(self.time.get()),
            events,
            focused: true,
            ..Default::default()
        };
        let t = Tokens::get();
        let accent = crate::workspace::tab_accent(&t, "MODEL");
        let row = Tabs {
            tabs: vec![
                Tab { id: "ONE", label: "One", accent, contextual: false, aliases: &[] },
                Tab { id: "TWO", label: "Two", accent, contextual: false, aliases: &[] },
                Tab { id: "EDIT", label: "Edit", accent, contextual: true, aliases: &["OLD"] },
            ],
            active: "ONE",
            add: Some("Tabs"),
            menu_open_from: &|_| false,
            search_width: 120.0,
            buttons: vec![TabButton { id: "go", label: "Go", ready: Ok(()) }, TabButton { id: "stop", label: "Stop", ready: Err("Nothing runs") }],
            floating: false,
            underline: None,
            handles: TAB_HANDLES,
        };
        let mut actions = Vec::new();
        crate::ui_kit::test_support::raw_frame(&self.ctx, input, |ui| {
            actions = tabs(ui, &row, Rect::from_min_size(egui::Pos2::ZERO, vec2(900.0, TAB_ROW_HEIGHT))).actions;
        });
        actions
    }

    fn press(&self, at: egui::Pos2, button: egui::PointerButton) -> Vec<TabAction> {
        self.time.set(self.time.get() + 1.0);
        let event = |pressed| egui::Event::PointerButton { pos: at, button, pressed, modifiers: egui::Modifiers::NONE };
        let mut actions = self.frame(vec![egui::Event::PointerMoved(at)]);
        actions.extend(self.frame(vec![egui::Event::PointerMoved(at), event(true)]));
        actions.extend(self.frame(vec![egui::Event::PointerMoved(at), event(false)]));
        actions
    }

    fn click(&self, name: &str) -> Vec<TabAction> {
        let at = solidcraft_ui_registry::rect_of(name).map(|r| r.center()).or_else(|| solidcraft_ui_registry::handle_at(name));
        self.press(at.unwrap_or_else(|| panic!("{name} is drawn")), egui::PointerButton::Primary)
    }
}

/// A click on a tab selects it (saying whether it was the selected one), a right-click asks for
/// the menu, the + button for its menu at its rect, and a button that is ready runs while one
/// that is not runs nothing; an alias is published at its tab.
#[test]
fn the_tab_row_returns_what_was_clicked() {
    let rig = TabsRig::new();
    assert_eq!(rig.click("t:tab_rect:TWO"), vec![TabAction::Select { index: 1, already: false }]);
    assert_eq!(rig.click("t:tab_rect:ONE"), vec![TabAction::Select { index: 0, already: true }]);
    let edit = solidcraft_ui_registry::rect_of("t:tab_rect:EDIT").expect("the contextual tab is drawn");
    assert_eq!(solidcraft_ui_registry::handle_at("t:tab:OLD"), Some(edit.center()), "its alias is published at it");
    let actions = rig.press(edit.center(), egui::PointerButton::Secondary);
    assert!(matches!(actions.as_slice(), [TabAction::Menu { .. }]), "{actions:?}");
    let add = solidcraft_ui_registry::rect_of("t:add").expect("the + button is drawn");
    assert_eq!(rig.click("t:add"), vec![TabAction::Add { rect: add }]);
    assert_eq!(rig.click("t:action:go"), vec![TabAction::Run { id: "go".into() }]);
    assert_eq!(rig.click("t:action:stop"), Vec::new(), "a button that is not ready runs nothing");
}

use egui::{Event, RawInput};

/// One pass of a fake two-row drop-down at `menu`; returns the tips shown this pass.
fn tip_pass(ctx: &egui::Context, menu: Rect, time: f64, at: egui::Pos2) -> Vec<(usize, Rect)> {
    let raw = RawInput { events: vec![Event::PointerMoved(at)], time: Some(time), ..Default::default() };
    let mut shown: Vec<(usize, Rect)> = Vec::new();
    crate::ui_kit::test_support::raw_frame(ctx, raw, |ui| {
        let ctx = ui.ctx();
        crate::ui_kit::Container::new(ctx, crate::ui_kit::Surface::Popup).area(egui::Id::unique("test_dropdown")).fixed_pos(menu.min).show(
            ctx,
            |ui| {
                for row in 0..2 {
                    let rect = Rect::from_min_size(menu.min + vec2(0.0, row as f32 * 32.0), vec2(menu.width(), 32.0));
                    let resp = ui.interact(rect, egui::Id::unique(("test_row", row)), Sense::hover());
                    let tip = if row == 0 {
                        menu_tip(ui, menu, &resp, |ui| {
                            ui.label("first row");
                        })
                    } else {
                        menu_tip(ui, menu, &resp, |ui| {
                            ui.label("second row");
                        })
                    };
                    if let Some(rect) = tip {
                        shown.push((row, rect));
                    }
                }
            },
        );
    });
    shown
}

/// egui registers hover a pass or two after the pointer arrives (measured), while leaving
/// a row drops hover the same pass. Timings below keep a margin around that lag.
#[test]
fn each_newly_hovered_row_waits_the_full_tip_delay() {
    let ctx = crate::ui_kit::test_support::context();
    let menu = Rect::from_min_size(pos2(20.0, 20.0), vec2(220.0, 64.0));
    let a = menu.min + vec2(10.0, 16.0);
    let b = menu.min + vec2(10.0, 48.0);
    let mut tips = Vec::new();
    for (time, at, expect) in [
        (0.0, a, None),
        (0.1, a, None),
        (0.3, a, None), // hover registers around here; the first row's delay starts
        (0.5, a, None), // still inside the delay
        (0.9, a, Some(0)),
        (1.0, b, None),    // the first row lost hover the moment the pointer left it
        (1.2, b, None),    // the second row must not inherit the open tip's grace (#10481)
        (1.4, b, None),    // its own delay cannot have elapsed yet, whenever hover registered
        (2.2, b, Some(1)), // after its own full delay, with margin for late registration
    ] {
        let got = tip_pass(&ctx, menu, time, at);
        match expect {
            Some(row) => {
                assert_eq!(got.len(), 1, "exactly row {row}'s tip at t={time}, got {got:?}");
                assert_eq!(got[0].0, row, "wrong row's tip at t={time}");
                let tip = got[0].1;
                assert!(tip.min.x >= menu.max.x, "the tip sits beside the menu, never over its rows: {tip:?} vs {menu:?}");
                tips.push((row, tip));
            }
            None => assert!(got.is_empty(), "no tip expected at t={time}, got {got:?}"),
        }
    }
    assert_eq!(tips.len(), 2, "both rows showed a tip after their own delay");
}

fn menu_style() -> MenuStyle<'static> {
    MenuStyle {
        pin_tip: "Pin",
        unpin_tip: "Unpin",
        accent: crate::workspace::tab_accent(&Tokens::get(), "MODEL"),
        focus: egui::Id::unique("test_menu"),
        entry: &["t:entry:"],
        pin: "t:pin:",
        hot: "t:hot:",
        reveal: "t:reveal",
    }
}

fn entry(id: &'static str, enabled: bool) -> MenuEntry<'static> {
    MenuEntry { id, label: id, icon: "box", shortcut: "", enabled, checked: None, pinned: false, tip: None, app_paints: false }
}

/// A menu of two rows (the second disabled) or, `grid`, the same as a grid; each frame's choice.
struct MenuRig {
    ctx: egui::Context,
    grid: bool,
    time: std::cell::Cell<f64>,
}

impl MenuRig {
    fn new(grid: bool) -> MenuRig {
        let rig = MenuRig { ctx: crate::ui_kit::test_support::context(), grid, time: std::cell::Cell::new(0.0) };
        rig.frame(Vec::new(), 0.0);
        rig
    }

    fn frame(&self, events: Vec<egui::Event>, wait: f64) -> Vec<(String, MenuAction)> {
        solidcraft_ui_registry::clear_handles();
        self.time.set(self.time.get() + 1.0 / 60.0 + wait);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
            time: Some(self.time.get()),
            events,
            focused: true,
            ..Default::default()
        };
        let entries = [entry("m.on", true), entry("m.off", false)];
        let style = menu_style();
        let mut chosen = Vec::new();
        crate::ui_kit::test_support::raw_frame(&self.ctx, input, |ui| {
            if self.grid {
                let grouped: Vec<(Option<&str>, MenuEntry)> = entries.iter().map(|e| (None, *e)).collect();
                let out = grid(ui, &grouped, &[], &style);
                chosen.extend(out.action.and_then(|(i, a)| entries.get(i).map(|e| (e.id.to_string(), a))));
            } else {
                for e in &entries {
                    let (action, _) = menu_row(ui, e, 220.0, None, &style);
                    chosen.extend(action.map(|a| (e.id.to_string(), a)));
                }
            }
        });
        chosen
    }

    fn press(&self, at: egui::Pos2, button: egui::PointerButton, wait: f64) -> Vec<(String, MenuAction)> {
        let event = |pressed| egui::Event::PointerButton { pos: at, button, pressed, modifiers: egui::Modifiers::NONE };
        let mut chosen = self.frame(vec![egui::Event::PointerMoved(at)], 1.0);
        chosen.extend(self.frame(vec![egui::Event::PointerMoved(at)], wait));
        chosen.extend(self.frame(vec![egui::Event::PointerMoved(at), event(true)], 0.0));
        chosen.extend(self.frame(vec![egui::Event::PointerMoved(at), event(false)], 0.0));
        chosen
    }

    fn at(&self, name: &str) -> egui::Pos2 {
        solidcraft_ui_registry::handle_at(name).unwrap_or_else(|| panic!("{name} is drawn"))
    }
}

/// A row or grid icon is hot under the pointer, runs its command when clicked, opens its menu on
/// a right-click, runs nothing when disabled, and pins or unpins from its pin.
#[test]
fn a_menu_entry_returns_what_was_chosen() {
    for grid in [false, true] {
        let rig = MenuRig::new(grid);
        let on = rig.at("t:entry:m.on");
        let chosen = rig.press(on, egui::PointerButton::Primary, 0.0);
        assert!(matches!(chosen.as_slice(), [(id, MenuAction::Run(rect))] if id == "m.on" && rect.contains(on)), "grid {grid}: {chosen:?}");
        assert_eq!(solidcraft_ui_registry::handle_at("t:hot:m.on"), Some(on), "grid {grid}: the entry under the pointer is hot");
        let chosen = rig.press(on, egui::PointerButton::Secondary, 0.0);
        assert!(matches!(chosen.as_slice(), [(id, MenuAction::Menu(_))] if id == "m.on"), "grid {grid}: {chosen:?}");
        assert_eq!(rig.press(rig.at("t:entry:m.off"), egui::PointerButton::Primary, 0.0), Vec::new(), "grid {grid}: a disabled entry runs nothing");
        // A grid icon's pin takes clicks once the pointer has rested on the icon.
        let wait = if grid { PIN_DELAY + 0.1 } else { 0.0 };
        rig.press(on, egui::PointerButton::Primary, wait);
        rig.frame(vec![egui::Event::PointerMoved(on)], wait);
        let pin = rig.at("t:pin:m.on");
        let chosen = rig.press(pin, egui::PointerButton::Primary, 0.0);
        assert_eq!(chosen, vec![("m.on".to_string(), MenuAction::TogglePin)], "grid {grid}");
    }
}

impl RowRig {
    /// Press at `from`, move past the drag threshold, then to `to`, and release there.
    fn drag(&self, from: egui::Pos2, to: egui::Pos2) -> Vec<Action> {
        let button =
            |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        let mut actions = self.frame(vec![egui::Event::PointerMoved(from)]);
        actions.extend(self.frame(vec![button(from, true)]));
        actions.extend(self.frame(vec![egui::Event::PointerMoved(from + vec2(0.0, 10.0))]));
        actions.extend(self.frame(vec![egui::Event::PointerMoved(to)]));
        actions.extend(self.frame(vec![egui::Event::PointerMoved(to)]));
        actions.extend(self.frame(vec![button(to, false)]));
        actions
    }
}

/// A button dragged onto another goes before it, onto a group's label to its end; dragged off
/// the row and released it is removed, its ghost crossed out while it would be.
#[test]
fn buttons_are_dropped_before_a_button_at_a_groups_end_or_removed_off_the_row() {
    let rig = RowRig::new(800.0);
    let drops = |actions: Vec<Action>| actions.into_iter().filter(|a| matches!(a, Action::Drop { .. })).collect::<Vec<_>>();
    let actions = rig.drag(rig.at("t:button:a.one"), rig.at("t:button:a.two"));
    assert_eq!(drops(actions), vec![Action::Drop { id: "a.one".into(), group: 0, before: Some("a.two".into()) }]);
    let actions = rig.drag(rig.at("t:button:a.two"), rig.at("t:section:B"));
    assert_eq!(drops(actions), vec![Action::Drop { id: "a.two".into(), group: 1, before: None }]);
    assert!(rig.removed.borrow().is_empty(), "nothing left the row");
    let from = rig.at("t:button:b.on");
    let mut actions = rig.frame(vec![egui::Event::PointerMoved(from)]);
    let press = |pressed| egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    actions.extend(rig.frame(vec![press(true)]));
    actions.extend(rig.frame(vec![egui::Event::PointerMoved(from + vec2(0.0, 10.0))]));
    let away = pos2(500.0, 300.0);
    actions.extend(rig.frame(vec![egui::Event::PointerMoved(away)]));
    assert!(solidcraft_ui_registry::handle_at("t:drag_remove").is_some(), "the ghost shows it would be removed");
    actions.extend(rig.frame(vec![egui::Event::PointerButton {
        pos: away,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    }]));
    assert_eq!(drops(actions), Vec::new());
    assert_eq!(*rig.removed.borrow(), ["b.on"]);
}

/// A button the app paints itself is drawn as the app resolves it after the pass: pressed, it
/// is published and counted.
#[test]
fn a_deferred_button_is_painted_as_the_app_resolves_it() {
    let ctx = crate::ui_kit::test_support::context();
    let t = Tokens::get();
    let mut on = button("a.on", true);
    on.app_paints = true;
    let row = Row {
        groups: vec![group("A", vec![on, button("a.off", true)])],
        labels: true,
        accent: crate::workspace::tab_accent(&t, "MODEL"),
        exit: false,
        reveal: None,
        more_tip: "More",
        handles: HANDLES,
    };
    solidcraft_ui_registry::clear_handles();
    let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 400.0))), ..Default::default() };
    let mut deferred = Vec::new();
    crate::ui_kit::test_support::raw_frame(&ctx, input, |ui| {
        let out = show(ui, &row, Rect::from_min_size(pos2(0.0, 30.0), vec2(800.0, GROUP_HEIGHT + 1.0)), &mut |_, _| false);
        deferred = out.buttons.into_iter().filter_map(|d| d.deferred).collect();
        assert_eq!(deferred.len(), 1, "only the button the app paints is deferred");
        let look = DeferredLook {
            id: "a.on".into(),
            selected: true,
            running: false,
            focus_ring: true,
            label: Some("On".into()),
            enabled: true,
            name: "On".into(),
            checkable: true,
            accent: crate::workspace::tab_accent(&t, "MODEL"),
        };
        paint_deferred(ui, deferred.drain(..).map(|d| (d, look.clone())).collect(), "t:ns");
    });
    assert!(solidcraft_ui_registry::rect_of("t:ns:pressed:a.on").is_some(), "the pressed button is published");
    assert_eq!(solidcraft_ui_registry::count_of("t:ns:pressed"), Some(1.0));
}

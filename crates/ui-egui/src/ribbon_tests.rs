//! The ribbon from plain data, no app: how groups fit, and what a click on the row returns.
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
}

impl RowRig {
    fn new(width: f32) -> RowRig {
        let rig = RowRig { ctx: crate::fonts::test_context(), width, time: std::cell::Cell::new(0.0), texts: Default::default() };
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
            accent: TabAccent::for_tab(&t, "MODEL"),
            exit: false,
            reveal: None,
            more_tip: "More",
            handles: HANDLES,
        };
        let mut actions = Vec::new();
        let output = crate::test_support::frame(&self.ctx, input, |ui| {
            let rect = Rect::from_min_size(pos2(0.0, 30.0), vec2(self.width, GROUP_HEIGHT + 1.0));
            actions = show(ui, &row, rect, &mut |_, _| false).actions;
        });
        *self.texts.borrow_mut() =
            crate::test_support::text_shapes(&output).map(|s| (s.galley.text().to_string(), s.visual_bounding_rect().center())).collect();
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

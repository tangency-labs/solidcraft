//! The History bar driven with plain data, no app: what each click and drag returns.
use super::*;
use egui::{Event, Modifiers, PointerButton, RawInput};
use std::cell::Cell;

const HANDLES: Handles<'static> = Handles { grip: "test:grip", marker: "test:marker", step: "test:", controls: "test:ctl:" };

fn bar(steps: usize, marker: usize, play: bool) -> Bar<'static> {
    let t = Tokens::get();
    Bar {
        title: "Steps",
        marker_tip: "Drag me",
        play,
        playing: false,
        play_labels: crate::ui_kit::PlayLabels { play: "Play", stop: "Stop" },
        accent: crate::workspace::tab_accent(&t, "MODEL"),
        drag_accent: crate::workspace::tab_accent(&t, "SKETCH"),
        marker,
        steps,
        floating: false,
        handles: HANDLES,
    }
}

/// The steps in order; `folded` draws one chip of the app's own for those slots instead.
struct Steps {
    labels: Vec<&'static str>,
    folded: Option<(usize, usize)>,
    /// Each step's state and whether it failed, by slot (default: neither).
    marks: Vec<(StepState, bool)>,
}

impl Contents for Steps {
    fn entries(&mut self, strip: &mut Strip<'_, '_>) {
        let mut slot = 0;
        while let Some(label) = self.labels.get(slot).copied() {
            if let Some((start, end)) = self.folded.filter(|(start, _)| *start == slot) {
                strip.chip(start, end, false, 0, |ui, _, at| {
                    solidcraft_ui_registry::publish_handle("test:folded", at + vec2(20.0, 14.0));
                    let _ = ui;
                    40.0
                });
                slot = end;
                continue;
            }
            let tip = || format!("{label} tip");
            let (state, error) = self.marks.get(slot).copied().unwrap_or_default();
            strip.step(&Step {
                id: slot as u64 + 1,
                slot,
                label,
                icon: "box",
                icon_accent: None,
                state,
                error,
                warning: false,
                ink: None,
                tip: &tip,
            });
            slot += 1;
        }
    }
}

struct Rig {
    ctx: egui::Context,
    bar: Bar<'static>,
    rect: Rect,
    labels: Vec<&'static str>,
    folded: Option<(usize, usize)>,
    marks: Vec<(StepState, bool)>,
    time: Cell<f64>,
    hovered: Cell<Option<u64>>,
}

impl Rig {
    fn new(labels: &[&'static str], marker: usize) -> Rig {
        let rig = Rig {
            ctx: crate::ui_kit::test_support::context(),
            bar: bar(labels.len(), marker, true),
            rect: Rect::from_min_size(pos2(0.0, 100.0), vec2(700.0, 40.0)),
            labels: labels.to_vec(),
            folded: None,
            marks: Vec::new(),
            time: Cell::new(0.0),
            hovered: Cell::new(None),
        };
        rig.frame(Vec::new());
        rig
    }

    fn frame_with(&self, events: Vec<Event>, modifiers: Modifiers) -> Vec<Action> {
        self.output(events, modifiers).0
    }

    /// A frame's actions and what it painted.
    fn output(&self, events: Vec<Event>, modifiers: Modifiers) -> (Vec<Action>, egui::FullOutput) {
        solidcraft_ui_registry::clear_handles();
        self.time.set(self.time.get() + 1.0 / 60.0);
        let mut actions = Vec::new();
        let events = std::iter::once(Event::ModifiersChanged(modifiers)).chain(events).collect();
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 300.0))),
            time: Some(self.time.get()),
            events,
            focused: true,
            ..Default::default()
        };
        let output = crate::ui_kit::test_support::raw_frame(&self.ctx, input, |ui| {
            let head = header(ui, &self.bar, self.rect);
            actions.extend(head.actions);
            let strip_rect = Rect::from_min_max(pos2(head.x, self.rect.top()), self.rect.right_bottom());
            let mut steps = Steps { labels: self.labels.clone(), folded: self.folded, marks: self.marks.clone() };
            let out = strip(ui, &self.bar, self.rect, strip_rect, "test_strip", &mut steps);
            self.hovered.set(out.hovered);
            actions.extend(out.actions);
        });
        (actions, output)
    }

    fn frame(&self, events: Vec<Event>) -> Vec<Action> {
        self.frame_with(events, Modifiers::NONE)
    }

    fn at(&self, name: &str) -> Pos2 {
        solidcraft_ui_registry::handle_at(name).unwrap_or_else(|| panic!("{name} is drawn"))
    }

    /// The marker's centre (it is published as a rect).
    fn marker(&self) -> Pos2 {
        solidcraft_ui_registry::rect_of("test:marker").map(|r| r.center()).unwrap_or_else(|| panic!("the marker is drawn"))
    }

    fn press(&self, at: Pos2, button: PointerButton, pressed: bool, modifiers: Modifiers) -> Vec<Action> {
        self.frame_with(vec![Event::PointerMoved(at), Event::PointerButton { pos: at, button, pressed, modifiers }], modifiers)
    }

    /// A click well after the last one, so the two are never a double click.
    fn click_with(&self, at: Pos2, button: PointerButton, modifiers: Modifiers) -> Vec<Action> {
        self.time.set(self.time.get() + 1.0);
        let mut actions = self.frame(vec![Event::PointerMoved(at)]);
        actions.extend(self.press(at, button, true, modifiers));
        actions.extend(self.press(at, button, false, modifiers));
        actions
    }

    fn click(&self, at: Pos2) -> Vec<Action> {
        self.click_with(at, PointerButton::Primary, Modifiers::NONE)
    }

    /// Press at `from`, move past the drag threshold, then to `to`, and release there.
    fn drag(&self, from: Pos2, to: Pos2) -> Vec<Action> {
        let mut actions = self.frame(vec![Event::PointerMoved(from)]);
        actions.extend(self.press(from, PointerButton::Primary, true, Modifiers::NONE));
        actions.extend(self.frame(vec![Event::PointerMoved(from + vec2(0.0, 10.0))]));
        actions.extend(self.frame(vec![Event::PointerMoved(to)]));
        actions.extend(self.frame(vec![Event::PointerMoved(to)]));
        actions.extend(self.press(to, PointerButton::Primary, false, Modifiers::NONE));
        actions
    }
}

#[test]
fn a_click_on_a_step_returns_select_with_its_modifiers_and_runs_nothing() {
    let rig = Rig::new(&["A", "B", "C"], 2);
    let b = rig.at("test:B");
    assert_eq!(rig.click(b), vec![Action::Select { step: 2, modifiers: Modifiers::NONE }]);
    assert_eq!(rig.click_with(b, PointerButton::Primary, Modifiers::SHIFT), vec![Action::Select { step: 2, modifiers: Modifiers::SHIFT }]);
    assert_eq!(rig.frame(vec![Event::PointerMoved(b)]), Vec::new(), "a hover chooses nothing");
    assert_eq!(rig.hovered.get(), Some(2), "the step under the pointer is returned");
    assert_eq!(rig.frame(vec![Event::PointerMoved(pos2(790.0, 290.0))]), Vec::new());
    assert_eq!(rig.hovered.get(), None);
}

#[test]
fn a_double_click_returns_edit_after_the_select_and_a_right_click_its_menu() {
    let rig = Rig::new(&["A", "B", "C"], 2);
    let c = rig.at("test:C");
    let mut actions = rig.click(c);
    actions.extend(rig.press(c, PointerButton::Primary, true, Modifiers::NONE));
    actions.extend(rig.press(c, PointerButton::Primary, false, Modifiers::NONE));
    let select = Action::Select { step: 3, modifiers: Modifiers::NONE };
    assert_eq!(actions, vec![select.clone(), select, Action::Edit(3)], "the second click selects again, then edits");
    assert_eq!(rig.click_with(c, PointerButton::Secondary, Modifiers::NONE), vec![Action::Menu { step: 3, at: c }]);
}

#[test]
fn a_dragged_step_returns_reorder_with_the_slot_and_side_of_the_marker() {
    let rig = Rig::new(&["A", "B", "C"], 2);
    let (a, c) = (rig.at("test:A"), rig.at("test:C"));
    // A..B, the marker, C; the end is one chip past C.
    let end = c + vec2(32.0 - 14.0 + 2.0, 0.0);
    assert_eq!(rig.drag(a, end), vec![Action::Reorder { step: 1, slot: 3, before: false }], "after C, past the marker");
    assert_eq!(rig.drag(c, a - vec2(12.0, 0.0)), vec![Action::Reorder { step: 3, slot: 0, before: true }], "before A");
    let marker = rig.marker();
    let left = marker - vec2(5.0, 0.0);
    let right = marker + vec2(5.0, 0.0);
    assert_eq!(rig.drag(a, left), vec![Action::Reorder { step: 1, slot: 2, before: true }], "the marker's slot, before it");
    assert_eq!(rig.drag(a, right), vec![Action::Reorder { step: 1, slot: 2, before: false }], "the marker's slot, after it");
}

#[test]
fn the_dragged_marker_returns_seek_and_a_drag_that_leaves_the_window_returns_nothing() {
    let rig = Rig::new(&["A", "B", "C"], 2);
    let (a, marker) = (rig.at("test:A"), rig.marker());
    assert_eq!(rig.drag(marker, a - vec2(12.0, 0.0)), vec![Action::Seek(Seek::Slot { slot: 0, before: true })]);
    let marker = rig.marker();
    assert_eq!(rig.drag(marker, marker), vec![Action::Seek(Seek::Slot { slot: 2, before: false })], "dropped where it is");
    // Released as the pointer leaves the window: the drag ends with no drop.
    let marker = rig.marker();
    rig.frame(vec![Event::PointerMoved(marker)]);
    rig.press(marker, PointerButton::Primary, true, Modifiers::NONE);
    rig.frame(vec![Event::PointerMoved(a)]);
    let gone = rig.frame_with(
        vec![Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }, Event::PointerGone],
        Modifiers::NONE,
    );
    assert_eq!(gone, Vec::new());
    assert_eq!(rig.click(rig.at("test:A")), vec![Action::Select { step: 1, modifiers: Modifiers::NONE }], "nothing is still dragged");
}

#[test]
fn a_folded_chip_of_the_apps_own_takes_its_slots_and_the_marker_inside_it_goes_after_it() {
    let mut rig = Rig::new(&["A", "B", "C", "D"], 2);
    rig.folded = Some((1, 3));
    rig.frame(Vec::new());
    let (a, folded, d) = (rig.at("test:A"), rig.at("test:folded"), rig.at("test:D"));
    assert!(solidcraft_ui_registry::handle_at("test:B").is_none(), "folded steps have no chip");
    let marker = rig.marker();
    assert!(folded.x < marker.x && marker.x < d.x, "the marker inside a folded chip is drawn after it");
    assert_eq!(rig.drag(a, folded), vec![Action::Reorder { step: 1, slot: 1, before: true }], "a drop on a folded chip lands before it");
    assert_eq!(rig.drag(d, folded - vec2(20.0, 0.0)), vec![Action::Reorder { step: 4, slot: 1, before: true }]);
}

#[test]
fn the_controls_return_seek_and_play_and_their_menu() {
    let rig = Rig::new(&["A", "B"], 1);
    for (control, action) in [
        ("test:ctl:first", Action::Seek(Seek::First)),
        ("test:ctl:back", Action::Seek(Seek::Back)),
        ("test:ctl:play", Action::Play),
        ("test:ctl:forward", Action::Seek(Seek::Forward)),
        ("test:ctl:last", Action::Seek(Seek::Last)),
    ] {
        assert_eq!(rig.click(rig.at(control)), vec![action], "{control}");
    }
    let back = rig.at("test:ctl:back");
    assert_eq!(rig.click_with(back, PointerButton::Secondary, Modifiers::NONE), vec![Action::ControlsMenu { at: back }]);
}

#[test]
fn without_play_the_controls_close_up_and_a_narrow_bar_leaves_the_title_out() {
    let mut rig = Rig::new(&["A"], 1);
    rig.bar.play = false;
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::handle_at("test:ctl:play").is_none());
    assert_eq!(rig.click(rig.at("test:ctl:forward")), vec![Action::Seek(Seek::Forward)]);
    let grip = rig.at("test:grip");
    rig.rect = Rect::from_min_size(rig.rect.min, vec2(100.0, 40.0));
    rig.frame(Vec::new());
    assert!(rig.at("test:grip").x < grip.x, "the grip alone, without the title");
}

#[test]
fn an_empty_bar_has_the_marker_at_its_start_and_returns_only_its_drop() {
    let rig = Rig::new(&[], 0);
    assert_eq!(rig.frame(Vec::new()), Vec::new());
    let marker = rig.marker();
    let first = rig.at("test:ctl:last");
    assert!(marker.x > first.x, "the marker after the controls");
    assert_eq!(rig.drag(marker, marker + vec2(80.0, 0.0)), vec![Action::Seek(Seek::Slot { slot: 0, before: false })]);
}

#[test]
fn a_step_after_the_marker_dropped_at_the_marker_returns_the_side_it_is_dropped_on() {
    let rig = Rig::new(&["A", "B", "C"], 2);
    let c = rig.at("test:C");
    let marker = rig.marker();
    assert_eq!(rig.drag(c, marker - vec2(5.0, 0.0)), vec![Action::Reorder { step: 3, slot: 2, before: true }], "before it: built");
    let marker = rig.marker();
    assert_eq!(rig.drag(rig.at("test:C"), marker + vec2(5.0, 0.0)), vec![Action::Reorder { step: 3, slot: 2, before: false }], "after it");
}

/// Every filled rect and circle a frame painted.
fn painted(output: &egui::FullOutput) -> (Vec<egui::epaint::RectShape>, Vec<egui::epaint::CircleShape>) {
    let (mut rects, mut circles) = (Vec::new(), Vec::new());
    for clipped in &output.shapes {
        crate::ui_kit::test_support::each_shape(&clipped.shape, &mut |shape| match shape {
            egui::Shape::Rect(rect) => rects.push(rect.clone()),
            egui::Shape::Circle(circle) => circles.push(*circle),
            _ => {}
        });
    }
    (rects, circles)
}

#[test]
fn a_steps_state_reaches_its_chip_and_the_floating_card_dots_the_last_done_step() {
    let t = Tokens::get();
    let mut rig = Rig::new(&["A", "B", "C", "D"], 4);
    let done = StepState { last_done: true, ..StepState::default() };
    rig.marks = vec![
        (StepState::default(), false),
        (StepState { highlighted: true, ..StepState::default() }, false),
        (StepState { rolled_back: true, ..StepState::default() }, false),
        (done, true),
    ];
    rig.frame(Vec::new());
    let (_, output) = rig.output(Vec::new(), Modifiers::NONE);
    let (rects, circles) = painted(&output);
    let chip = |label: &str| {
        let centre = rig.at(&format!("test:{label}"));
        let rect = Rect::from_center_size(centre, vec2(28.0, 28.0));
        rects.iter().find(|r| r.rect == rect).cloned().unwrap_or_else(|| panic!("{label}'s chip is painted"))
    };
    let (plain, lit, rolled, failed) = (chip("A"), chip("B"), chip("C"), chip("D"));
    assert_eq!(plain.fill, t.timeline_item);
    assert_eq!((lit.fill, lit.stroke.color), (t.hover, rig.bar.accent.color), "highlighted: lit as though hovered");
    assert!(rolled.fill.a() < plain.fill.a(), "rolled back: faded");
    assert_eq!((failed.fill, failed.stroke.color), (t.form_error_fill, t.form_error), "an error marks the chip");
    let dot = |rig: &Rig| {
        let d = rig.at("test:D");
        let at = pos2(d.x, d.y + 14.0 + t.space_3);
        let (_, output) = rig.output(Vec::new(), Modifiers::NONE);
        painted(&output).1.iter().any(|c| c.center == at && c.fill == rig.bar.accent.color)
    };
    assert!(!circles.iter().any(|c| c.fill == rig.bar.accent.color && c.radius == t.space_2), "a docked bar draws no dot");
    assert!(!dot(&rig));
    rig.bar.floating = true;
    rig.frame(Vec::new());
    assert!(dot(&rig), "the floating card dots the last step before the marker");
    rig.marks[3] = (StepState::default(), true);
    rig.frame(Vec::new());
    assert!(!dot(&rig), "only that step");
}

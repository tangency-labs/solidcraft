//! The History bar driven with plain data, no app: what each click and drag returns.
use super::*;
use egui::{Event, Modifiers, PointerButton, RawInput};
use std::cell::Cell;
use std::sync::Arc;

const HANDLES: Handles<'static> = Handles { marker: "test:marker", step: "test:" };

thread_local! {
    /// How many times a step's hover text was written.
    static TIPS: Cell<usize> = const { Cell::new(0) };
}

fn bar(steps: usize, marker: usize) -> Bar<'static> {
    let t = Tokens::get();
    Bar {
        marker_tip: "Drag me",
        accent: solidcraft_theme::TabAccent::for_ramp(&t, solidcraft_theme::AccentRamp::Main),
        drag_accent: solidcraft_theme::TabAccent::for_ramp(&t, solidcraft_theme::AccentRamp::Sketch),
        marker,
        steps,
        floating: false,
        reorder: true,
        handles: HANDLES,
    }
}

/// What the app does on the strip before a step, other than drawing steps.
#[derive(Clone, Copy, Debug)]
enum Between {
    /// Something inline that draws nothing.
    Inline,
    /// Takes the strip's ui.
    Ui,
}

/// The steps in order; `folded` draws one chip of the app's own for those slots instead.
struct Steps {
    labels: Vec<&'static str>,
    folded: Option<(usize, usize)>,
    /// Done just before the step in this slot.
    between: Option<(usize, Between)>,
    /// Each step's state and whether it failed, by slot (default: neither).
    marks: Vec<(StepState, bool)>,
    /// Each step's handle name kept ready, by slot (empty: the bar builds them).
    ready: Vec<Arc<str>>,
    /// The strip's scope, which the chips' ids and the drag's are made from.
    scope: Option<egui::Id>,
}

impl Contents for Steps {
    fn entries(&mut self, strip: &mut Strip<'_, '_>) {
        self.scope = Some(strip.ui().scope_id());
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
            match self.between.filter(|(at, _)| *at == slot).map(|(_, between)| between) {
                Some(Between::Inline) => strip.inline(|_, _, x, _| x),
                Some(Between::Ui) => {
                    let _ = strip.ui();
                }
                None => {}
            }
            let tip = || {
                TIPS.with(|tips| tips.set(tips.get() + 1));
                format!("{label} tip")
            };
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
                handle: self.ready.get(slot),
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
    between: Option<(usize, Between)>,
    marks: Vec<(StepState, bool)>,
    /// The handle names the steps bring ready, by slot (empty: the bar builds them).
    ready: Vec<Arc<str>>,
    time: Cell<f64>,
    hovered: Cell<Option<u64>>,
    scope: Cell<Option<egui::Id>>,
}

impl Rig {
    fn new(labels: &[&'static str], marker: usize) -> Rig {
        let rig = Rig {
            ctx: crate::test_support::context(),
            bar: bar(labels.len(), marker),
            rect: Rect::from_min_size(pos2(0.0, 100.0), vec2(700.0, 40.0)),
            labels: labels.to_vec(),
            folded: None,
            between: None,
            marks: Vec::new(),
            ready: Vec::new(),
            time: Cell::new(0.0),
            hovered: Cell::new(None),
            scope: Cell::new(None),
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
        let output = crate::test_support::raw_frame(&self.ctx, input, |ui| {
            let ready = self.ready.clone();
            let mut steps =
                Steps { labels: self.labels.clone(), folded: self.folded, between: self.between, marks: self.marks.clone(), ready, scope: None };
            let out = strip(ui, &self.bar, self.rect, self.rect, "test_strip", &mut steps);
            self.hovered.set(out.hovered);
            self.scope.set(steps.scope);
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
fn without_reorder_a_dragged_step_returns_nothing_and_a_click_still_selects() {
    let mut rig = Rig::new(&["A", "B", "C"], 2);
    rig.bar.reorder = false;
    let (a, c) = (rig.at("test:A"), rig.at("test:C"));
    let end = c + vec2(32.0 - 14.0 + 2.0, 0.0);
    assert_eq!(rig.drag(a, end), Vec::new(), "no reorder");
    assert_eq!(rig.drag(c, a - vec2(12.0, 0.0)), Vec::new());
    assert_eq!(rig.click(c), vec![Action::Select { step: 3, modifiers: Modifiers::NONE }]);
    // the marker still drags
    assert_eq!(rig.drag(rig.marker(), a - vec2(12.0, 0.0)), vec![Action::Seek { slot: 0, before: true }]);
}

#[test]
fn the_dragged_marker_returns_seek_and_a_drag_that_leaves_the_window_returns_nothing() {
    let rig = Rig::new(&["A", "B", "C"], 2);
    let (a, marker) = (rig.at("test:A"), rig.marker());
    assert_eq!(rig.drag(marker, a - vec2(12.0, 0.0)), vec![Action::Seek { slot: 0, before: true }]);
    let marker = rig.marker();
    assert_eq!(rig.drag(marker, marker), vec![Action::Seek { slot: 2, before: false }], "dropped where it is");
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
fn an_empty_bar_has_the_marker_at_its_start_and_returns_only_its_drop() {
    let rig = Rig::new(&[], 0);
    assert_eq!(rig.frame(Vec::new()), Vec::new());
    let marker = rig.marker();
    assert!((marker.x - rig.rect.left() - Tokens::get().space_12 / 2.0).abs() <= 1.0, "the empty strip starts at its marker");
    assert_eq!(rig.drag(marker, marker + vec2(80.0, 0.0)), vec![Action::Seek { slot: 0, before: false }]);
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
        crate::test_support::each_shape(&clipped.shape, &mut |shape| match shape {
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

/// `n` steps named S0, S1…: 40 of them run well past the 700-point strip.
fn many(n: usize) -> Vec<&'static str> {
    (0..n).map(|k| &*Box::leak(format!("S{k}").into_boxed_str())).collect()
}

/// The labels of the steps whose chips a frame painted, in slot order.
fn chips_painted(rig: &Rig, output: &egui::FullOutput) -> Vec<&'static str> {
    let (rects, _) = painted(output);
    let label_painted = |label: &&'static str| {
        let rect = Rect::from_center_size(rig.at(&format!("test:{label}")), vec2(28.0, 28.0));
        rects.iter().any(|r| r.rect == rect)
    };
    rig.labels.iter().copied().filter(label_painted).collect()
}

#[test]
fn steps_that_bring_their_handles_publish_those_names_shared_at_the_same_points() {
    let mut rig = Rig::new(&many(40), 20);
    let built = solidcraft_ui_registry::handles("test:");
    assert_eq!(built.len(), 40, "every step publishes its handle");
    assert!(built.iter().any(|(_, at)| at.x > rig.rect.right()), "steps out of view among them");
    // A spelling the bar never builds itself, so only the names brought can publish it.
    rig.ready = rig.labels.iter().map(|label| Arc::from(format!("ready:{label}"))).collect();
    let before: Vec<usize> = rig.ready.iter().map(Arc::strong_count).collect();
    rig.frame(Vec::new());
    let ready = solidcraft_ui_registry::handles("ready:");
    let expected: Vec<(String, Pos2)> = built.iter().map(|(name, at)| (name.replacen("test:", "ready:", 1), *at)).collect();
    assert_eq!(ready, expected, "the names brought, in the same order and at the same points");
    assert!(solidcraft_ui_registry::handles("test:").is_empty(), "no name is built");
    let after: Vec<usize> = rig.ready.iter().map(Arc::strong_count).collect();
    assert_eq!(after, before.iter().map(|n| n + 1).collect::<Vec<_>>(), "the registry holds the names brought, not copies");
    rig.ready = Vec::new();
    rig.frame(Vec::new());
    assert_eq!(solidcraft_ui_registry::handles("test:"), built, "built again as before");
}

/// Hovers `at` and holds the pointer still long enough for a tip to show; returns how many tips
/// were written and the last frame's output.
fn hover_still(rig: &Rig, at: Pos2) -> (usize, egui::FullOutput) {
    TIPS.with(|tips| tips.set(0));
    rig.frame(vec![Event::PointerMoved(at)]);
    let mut output = None;
    for _ in 0..4 {
        rig.time.set(rig.time.get() + 0.5);
        output = Some(rig.output(Vec::new(), Modifiers::NONE).1);
    }
    let output = output.unwrap_or_else(|| panic!("a frame ran"));
    (TIPS.with(Cell::get), output)
}

#[test]
fn a_step_out_of_view_keeps_its_handle_but_is_neither_painted_nor_hovered() {
    let rig = Rig::new(&many(40), 40);
    // A step in view, for comparison: painted, hovered, and its hover text written.
    let s5 = rig.at("test:S5");
    let (tips, output) = hover_still(&rig, s5);
    assert!(tips > 0, "a step in view writes its hover text");
    assert_eq!(rig.hovered.get(), Some(6), "and is hovered");
    assert!(chips_painted(&rig, &output).contains(&"S5"), "and painted");
    let s39 = rig.at("test:S39");
    assert!(s39.x > rig.rect.right(), "the last step is out of view: {s39:?}");
    let (tips, output) = hover_still(&rig, s39);
    assert_eq!(tips, 0, "its hover text is not written");
    let drawn = chips_painted(&rig, &output);
    assert!(drawn.contains(&"S0") && drawn.contains(&"S20"), "the steps in view are painted: {drawn:?}");
    assert!(!drawn.contains(&"S30") && !drawn.contains(&"S39"), "those out of view are not: {drawn:?}");
    assert_eq!(rig.hovered.get(), None, "nor hovered");
    assert_eq!(rig.at("test:S39"), s39, "it still keeps its place");
}

/// How many times one frame's steps out of view read the focus and the open popups.
fn quiet_reads(rig: &Rig) -> usize {
    QUIET_READS.with(|reads| reads.set(0));
    rig.frame(Vec::new());
    QUIET_READS.with(Cell::get)
}

#[test]
fn steps_out_of_view_read_the_focus_and_popups_once_per_run() {
    assert_eq!(quiet_reads(&Rig::new(&["A", "B", "C"], 2)), 0, "every step in view: no read");
    let rig = Rig::new(&many(60), 60);
    assert!(rig.at("test:S0").x > rig.rect.left() && rig.at("test:S59").x > rig.rect.right());
    assert_eq!(quiet_reads(&rig), 1, "one run out of view after those in view: one read");
    // Scrolled so steps in view sit between a run out of view on each side.
    let over = rig.at("test:S5");
    for _ in 0..20 {
        let wheel = Event::MouseWheel {
            source: egui::MouseWheelSource::Unknown,
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -100.0),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        };
        rig.frame(vec![Event::PointerMoved(over), wheel]);
        if rig.at("test:S5").x < rig.rect.left() - 30.0 {
            break;
        }
    }
    for _ in 0..30 {
        rig.frame(Vec::new());
    }
    let (first, last) = (rig.at("test:S0"), rig.at("test:S59"));
    assert!(first.x < rig.rect.left() - 20.0 && last.x > rig.rect.right() + 20.0, "out of view on both sides: {first:?} {last:?}");
    assert_eq!(quiet_reads(&rig), 2, "a run out of view on each side of those in view: two reads");
}

/// A chip of the app's own, something inline or a use of the strip's ui between two runs of
/// steps out of view can move the focus or open a popup: the run after it reads them again.
#[test]
fn what_the_app_draws_between_steps_out_of_view_makes_the_next_run_read_again() {
    let mut rig = Rig::new(&many(60), 60);
    assert!(rig.at("test:S30").x > rig.rect.right(), "S30 to S59 are out of view");
    assert_eq!(quiet_reads(&rig), 1, "one run");
    rig.folded = Some((40, 42));
    assert_eq!(quiet_reads(&rig), 2, "a folded chip of the app's own between two runs");
    rig.folded = None;
    rig.between = Some((40, Between::Inline));
    assert_eq!(quiet_reads(&rig), 2, "something inline between two runs");
    rig.between = Some((40, Between::Ui));
    assert_eq!(quiet_reads(&rig), 2, "the strip's ui taken between two runs");
    rig.between = None;
    assert_eq!(quiet_reads(&rig), 1, "one run again");
}

#[test]
fn a_focused_or_dragged_step_out_of_view_stays_alive_and_an_open_popup_keeps_every_step() {
    let rig = Rig::new(&many(40), 40);
    let scope = rig.scope.get().unwrap_or_else(|| panic!("the strip was laid out"));
    let alive = |rig: &Rig| {
        let (_, output) = rig.output(Vec::new(), Modifiers::NONE);
        chips_painted(rig, &output)
    };
    let plain = alive(&rig);
    assert!(!plain.contains(&"S30") && !plain.contains(&"S35"));
    // S31 (id 32) has the focus: it alone of those out of view is drawn.
    let s31 = scope.with(("feat", 32u64));
    rig.ctx.memory_mut(|m| m.request_focus(s31));
    let focused = alive(&rig);
    assert!(focused.contains(&"S31") && !focused.contains(&"S30") && !focused.contains(&"S32"), "{focused:?}");
    assert!(rig.ctx.memory(|m| m.has_focus(s31)), "it keeps the focus while it is out of view");
    rig.ctx.memory_mut(|m| m.surrender_focus(s31));
    assert_eq!(alive(&rig), plain, "the focus gone, it is left out again");
    // S35 (id 36) is being dragged.
    let drag_id = scope.with("tl_drag");
    rig.ctx.data_mut(|d| d.insert_temp(drag_id, Drag::Step(36)));
    let dragged = alive(&rig);
    assert!(dragged.contains(&"S35") && !dragged.contains(&"S34"), "{dragged:?}");
    rig.ctx.data_mut(|d| d.remove::<Drag>(drag_id));
    assert_eq!(alive(&rig), plain);
    // Under an open popup every step is alive.
    egui::Popup::open_id(&rig.ctx, egui::Id::unique("test popup"));
    assert_eq!(alive(&rig), rig.labels, "every step is drawn under an open popup");
}

/// A strip narrower than its steps, scrolled to its end, shows the marker after the last step
/// inside its clip: the marker is part of the scroll range, not cut off past it.
#[test]
fn scrolled_to_its_end_the_strip_shows_the_marker_after_the_last_step() {
    let labels: Vec<&'static str> = (0..40).map(|i| &*Box::leak(format!("Step {i}").into_boxed_str())).collect();
    let mut rig = Rig::new(&labels, labels.len());
    rig.rect = Rect::from_min_size(pos2(0.0, 100.0), vec2(300.0, 40.0));
    rig.frame(Vec::new());
    let over = rig.rect.center();
    for _ in 0..30 {
        rig.time.set(rig.time.get() + 0.1);
        let wheel = Event::MouseWheel {
            source: egui::MouseWheelSource::Unknown,
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -2000.0),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        };
        rig.frame(vec![Event::PointerMoved(over), wheel]);
    }
    rig.frame(Vec::new());
    let marker = solidcraft_ui_registry::rect_of("test:marker").expect("the marker is drawn");
    let last = rig.at("test:Step 39");
    assert!(rig.rect.contains(last), "scrolled to the end: the last step {last:?} shows in {:?}", rig.rect);
    assert!(marker.left() >= rig.rect.left() && marker.right() <= rig.rect.right(), "the marker {marker:?} lies inside the strip {:?}", rig.rect);
}

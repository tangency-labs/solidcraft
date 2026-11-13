//! The Browser driven by a fake host, no app: what each click, key and drag returns. The host keeps
//! its own selection, visibility and folds, and applies the panel's events after each frame, as an
//! app does.
use super::*;
use crate::menu::Item;
use egui::{Event as Input, Modifiers, PointerButton, RawInput};
use serde_json::json;
use std::cell::{Cell, RefCell};

const BODIES: [(NodeId, &str); 3] = [(2, "A"), (3, "B"), (4, "C")];
const GROUP: NodeId = 5;
const SETTINGS: NodeId = 6;

fn group_menu() -> Vec<Item> {
    vec![Item::action("ui.rename", "Rename", "rename"), Item::sep(), Item::action("ui.ungroup", "Ungroup", "folder").with(json!({ "group": 5 }))]
}

/// The fake host: a Bodies folder holding A, B and C (and `extra` more), a group G, and a
/// settings row that is no item.
#[derive(Default)]
struct Host {
    selected: Vec<NodeId>,
    hidden: Vec<NodeId>,
    folds: FoldMemory,
    extra: usize,
}

impl Host {
    fn rows(&self, rows: &mut Rows<'_>) {
        let open = self.folds.is_open("bodies", true);
        rows.row(Node {
            id: 1,
            key: "bodies",
            row: Row { fold: Some(open), eye: Some(!self.hidden.contains(&1)), icon: "folder", label: "Bodies", ..Default::default() },
            click_folds: true,
            drops: Drops { before: false, into: true },
            ..Default::default()
        });
        if open {
            let extra: Vec<(NodeId, String)> = (0..self.extra).map(|i| (100 + i as NodeId, format!("Body {i}"))).collect();
            let mut bodies: Vec<(NodeId, &str)> = BODIES.to_vec();
            bodies.extend(extra.iter().map(|(id, label)| (*id, label.as_str())));
            for (id, label) in bodies {
                rows.row(Node {
                    id,
                    key: label,
                    row: Row {
                        depth: 1,
                        eye: Some(!self.hidden.contains(&id)),
                        icon: "body",
                        label,
                        name: true,
                        selected: self.selected.contains(&id),
                        ..Default::default()
                    },
                    select: Selectable::Yes(0),
                    rename: Rename::SlowClick,
                    drag: Some(0),
                    drops: Drops { before: true, into: false },
                    ..Default::default()
                });
            }
        }
        rows.row(Node {
            id: GROUP,
            key: "g",
            row: Row { fold: Some(self.folds.is_open("g", true)), icon: "folder", label: "G", name: true, ..Default::default() },
            select: Selectable::Yes(1),
            drag: Some(1),
            drops: Drops { before: true, into: true },
            menu: NodeMenu::Items(&group_menu),
            ..Default::default()
        });
        rows.row(Node {
            id: SETTINGS,
            key: "settings",
            row: Row { icon: "settings", label: "Settings", ..Default::default() },
            menu: NodeMenu::Host,
            ..Default::default()
        });
    }

    fn apply(&mut self, events: &[Event]) {
        for event in events {
            match event {
                Event::Select { id, how } => match how {
                    How::Only => self.selected = vec![*id],
                    How::Toggle if self.selected.contains(id) => self.selected.retain(|s| s != id),
                    How::Toggle => self.selected.push(*id),
                    How::Range(ids) => self.selected.clone_from(ids),
                    How::RangeAdd(ids) => {
                        for id in ids {
                            if !self.selected.contains(id) {
                                self.selected.push(*id);
                            }
                        }
                    }
                },
                Event::Eye { id, visible: false } => self.hidden.push(*id),
                Event::Eye { id, visible: true } => self.hidden.retain(|h| h != id),
                Event::Fold { id, open } => {
                    let key = if *id == 1 { "bodies" } else { "g" };
                    self.folds.set_open(key, true, *open);
                }
                _ => {}
            }
        }
    }
}

struct Rig {
    ctx: egui::Context,
    time: Cell<f64>,
    host: RefCell<Host>,
    state: RefCell<BrowserState>,
    keyboard: Cell<bool>,
    search: Cell<bool>,
    /// Whether G takes a drop inside it (the host's rule).
    into_g: Cell<bool>,
    height: Cell<f32>,
    drawn: RefCell<Vec<Drawn>>,
}

impl Rig {
    fn new() -> Rig {
        let rig = Rig {
            ctx: crate::test_support::context(),
            time: Cell::new(0.0),
            host: RefCell::new(Host::default()),
            state: RefCell::new(BrowserState::default()),
            keyboard: Cell::new(false),
            search: Cell::new(false),
            into_g: Cell::new(true),
            height: Cell::new(500.0),
            drawn: RefCell::new(Vec::new()),
        };
        rig.frame(Vec::new());
        rig
    }

    fn frame_with(&self, events: Vec<Input>, modifiers: Modifiers) -> Vec<Event> {
        solidcraft_ui_registry::clear_handles();
        self.time.set(self.time.get() + 1.0 / 60.0);
        let events = std::iter::once(Input::ModifiersChanged(modifiers)).chain(events).collect();
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 600.0))),
            time: Some(self.time.get()),
            events,
            focused: true,
            ..Default::default()
        };
        let mut out = Output::default();
        let _ = crate::test_support::raw_frame(&self.ctx, input, |ui| {
            ui.set_width(280.0);
            let host = self.host.borrow();
            let mut state = self.state.borrow_mut();
            let into_g = self.into_g.get();
            let accept = move |_: &Dragged, target: NodeId, zone: Zone| into_g || !(target == GROUP && zone == Zone::Into);
            let mut browser = Browser::new(&mut state, Style::default(), 7).accept(&accept).keyboard(self.keyboard.get());
            if self.search.get() {
                browser = browser.header(Header { title: "Objects", count: "6", collapsible: true, search: Some("Search") });
            }
            out = ui.allocate_ui(vec2(280.0, self.height.get()), |ui| browser.show(ui, |rows| host.rows(rows))).inner;
        });
        self.host.borrow_mut().apply(&out.events);
        *self.drawn.borrow_mut() = out.drawn;
        out.events
    }

    fn frame(&self, events: Vec<Input>) -> Vec<Event> {
        self.frame_with(events, Modifiers::NONE)
    }

    /// A frame `seconds` after the last one.
    fn wait(&self, seconds: f64) -> Vec<Event> {
        self.time.set(self.time.get() + seconds);
        self.frame(Vec::new())
    }

    fn key(&self, key: egui::Key, modifiers: Modifiers) -> Vec<Event> {
        self.frame_with(vec![Input::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }], modifiers)
    }

    fn at(&self, name: &str) -> Pos2 {
        solidcraft_ui_registry::handle_at(name).unwrap_or_else(|| panic!("{name} is drawn"))
    }

    fn rect(&self, id: NodeId) -> Rect {
        self.drawn.borrow().iter().find(|d| d.id == id).map(|d| d.rect).unwrap_or_else(|| panic!("row {id} is drawn"))
    }

    fn labels(&self) -> Vec<String> {
        self.drawn.borrow().iter().map(|d| d.label.clone()).collect()
    }

    fn press(&self, at: Pos2, button: PointerButton, pressed: bool, modifiers: Modifiers) -> Vec<Event> {
        self.frame_with(vec![Input::PointerMoved(at), Input::PointerButton { pos: at, button, pressed, modifiers }], modifiers)
    }

    /// A click well after the last one, so the two are never a double click.
    fn click_with(&self, at: Pos2, button: PointerButton, modifiers: Modifiers) -> Vec<Event> {
        self.time.set(self.time.get() + 1.0);
        let mut events = self.frame_with(vec![Input::PointerMoved(at)], modifiers);
        events.extend(self.press(at, button, true, modifiers));
        events.extend(self.press(at, button, false, modifiers));
        events
    }

    fn click(&self, at: Pos2) -> Vec<Event> {
        self.click_with(at, PointerButton::Primary, Modifiers::NONE)
    }

    fn click_mods(&self, at: Pos2, modifiers: Modifiers) -> Vec<Event> {
        self.click_with(at, PointerButton::Primary, modifiers)
    }

    fn double_click(&self, at: Pos2) -> Vec<Event> {
        self.time.set(self.time.get() + 1.0);
        let mut events = self.frame(vec![Input::PointerMoved(at)]);
        for pressed in [true, false, true, false] {
            events.extend(self.press(at, PointerButton::Primary, pressed, Modifiers::NONE));
        }
        events
    }

    /// Press at `from`, move past the drag threshold, then to `to`, and release there.
    fn drag(&self, from: Pos2, to: Pos2) -> Vec<Event> {
        self.time.set(self.time.get() + 1.0);
        let mut events = self.frame(vec![Input::PointerMoved(from)]);
        events.extend(self.press(from, PointerButton::Primary, true, Modifiers::NONE));
        events.extend(self.frame(vec![Input::PointerMoved(from + vec2(0.0, 8.0))]));
        events.extend(self.frame(vec![Input::PointerMoved(to)]));
        events.extend(self.frame(vec![Input::PointerMoved(to)]));
        events.extend(self.press(to, PointerButton::Primary, false, Modifiers::NONE));
        events
    }
}

#[test]
fn a_click_selects_only_that_row_cmd_toggles_and_shift_takes_the_range() {
    let rig = Rig::new();
    let (a, b, c) = (rig.at("row:A"), rig.at("row:B"), rig.at("row:C"));
    assert_eq!(rig.click(a), vec![Event::Select { id: 2, how: How::Only }]);
    assert_eq!(rig.click_mods(c, Modifiers::COMMAND), vec![Event::Select { id: 4, how: How::Toggle }]);
    assert_eq!(rig.host.borrow().selected, vec![2, 4]);
    assert_eq!(rig.click(b), vec![Event::Select { id: 3, how: How::Only }]);
    assert_eq!(rig.click_mods(a, Modifiers::SHIFT), vec![Event::Select { id: 2, how: How::Range(vec![2, 3]) }], "from the anchor, B");
    assert_eq!(rig.click_mods(c, Modifiers::SHIFT | Modifiers::COMMAND), vec![Event::Select { id: 4, how: How::RangeAdd(vec![3, 4]) }]);
    assert_eq!(rig.host.borrow().selected, vec![2, 3, 4]);
    assert_eq!(rig.state.borrow().anchor(), Some(3), "a range keeps its anchor");
}

#[test]
fn a_range_never_mixes_kinds_and_rows_that_are_no_items_only_click() {
    let rig = Rig::new();
    assert_eq!(rig.click(rig.at("row:A")), vec![Event::Select { id: 2, how: How::Only }]);
    // G is another kind of row than the bodies: Shift on it starts over from it.
    assert_eq!(rig.click_mods(rig.at("row:G"), Modifiers::SHIFT), vec![Event::Select { id: GROUP, how: How::Only }]);
    assert_eq!(rig.state.borrow().anchor(), Some(GROUP));
    assert_eq!(rig.click(rig.at("row:Settings")), vec![Event::Click(SETTINGS)], "no selection");
    assert_eq!(rig.host.borrow().selected, vec![GROUP], "the selection is unchanged");
}

#[test]
fn the_fold_arrow_folds_and_a_folder_label_click_folds_too() {
    let rig = Rig::new();
    assert_eq!(rig.click(rig.at("fold:Bodies")), vec![Event::Fold { id: 1, open: false }]);
    assert_eq!(rig.labels(), ["Bodies", "G", "Settings"], "the host folded it");
    assert_eq!(rig.click(rig.at("row:Bodies")), vec![Event::Click(1), Event::Fold { id: 1, open: true }]);
    assert_eq!(rig.labels(), ["Bodies", "A", "B", "C", "G", "Settings"]);
}

#[test]
fn a_slower_second_click_on_a_name_opens_the_editor_and_a_double_click_opens_the_row() {
    let rig = Rig::new();
    let a = rig.at("row:A");
    assert_eq!(rig.click(a), vec![Event::Select { id: 2, how: How::Only }]);
    assert_eq!(rig.click(a), Vec::new(), "the second click is held for the double-click delay");
    assert_eq!(rig.state.borrow().renaming(), None);
    assert_eq!(rig.wait(0.4), Vec::new());
    assert_eq!(rig.state.borrow().renaming(), Some(2), "then it renames");
    rig.state.borrow_mut().cancel_rename();

    let b = rig.at("row:B");
    assert_eq!(rig.double_click(b), vec![Event::Select { id: 3, how: How::Only }, Event::Open(3)]);
    assert_eq!(rig.wait(0.4), Vec::new());
    assert_eq!(rig.state.borrow().renaming(), None, "a double-click never renames");
}

#[test]
fn a_modified_click_or_a_click_on_another_row_never_renames() {
    let rig = Rig::new();
    let (a, b) = (rig.at("row:A"), rig.at("row:B"));
    rig.click(a);
    assert_eq!(rig.click_mods(a, Modifiers::COMMAND), vec![Event::Select { id: 2, how: How::Toggle }]);
    rig.click(b);
    rig.click(a);
    rig.wait(0.4);
    assert_eq!(rig.state.borrow().renaming(), None);
}

#[test]
fn the_inline_editor_commits_on_enter_cancels_on_escape_and_reopens_when_refused() {
    let rig = Rig::new();
    rig.state.borrow_mut().start_rename(2, "A");
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::handle_at("rename:A").is_some(), "the editor is in A's row");
    rig.frame(vec![Input::Text("Z".into())]);
    let events = rig.key(egui::Key::Enter, Modifiers::NONE);
    assert!(matches!(events.as_slice(), [Event::Rename { id: 2, text }] if text.ends_with('Z')), "{events:?}");
    assert_eq!(rig.state.borrow().renaming(), None);

    rig.state.borrow_mut().refuse_rename(2, "Z", "That name is taken");
    assert_eq!(rig.state.borrow().renaming(), Some(2), "a refused name keeps the editor");
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert_eq!(rig.key(egui::Key::Escape, Modifiers::NONE), Vec::new(), "Escape cancels: nothing is renamed");
    assert_eq!(rig.state.borrow().renaming(), None);
}

#[test]
fn an_eye_click_sets_one_row_and_a_drag_over_the_eyes_is_one_gesture() {
    let rig = Rig::new();
    assert_eq!(rig.click(rig.at("eye:B")), vec![Event::Eye { id: 3, visible: false }]);
    assert_eq!(rig.click(rig.at("eye:B")), vec![Event::Eye { id: 3, visible: true }]);

    let (a, b, c) = (rig.at("eye:A"), rig.at("eye:B"), rig.at("eye:C"));
    let mut events = rig.frame(vec![Input::PointerMoved(a)]);
    events.extend(rig.press(a, PointerButton::Primary, true, Modifiers::NONE));
    events.extend(rig.frame(vec![Input::PointerMoved(b)]));
    events.extend(rig.frame(vec![Input::PointerMoved(c)]));
    events.extend(rig.frame(vec![Input::PointerMoved(c)]));
    events.extend(rig.press(c, PointerButton::Primary, false, Modifiers::NONE));
    assert_eq!(
        events,
        vec![
            Event::EyeGesture(Gesture::Begin),
            Event::Eye { id: 2, visible: false },
            Event::Eye { id: 3, visible: false },
            Event::Eye { id: 4, visible: false },
            Event::EyeGesture(Gesture::End),
        ]
    );
    assert_eq!(rig.host.borrow().hidden, vec![2, 3, 4]);
}

#[test]
fn a_dragged_row_moves_before_a_row_or_into_one_by_the_host_rules() {
    let rig = Rig::new();
    let (a, b, c) = (rig.at("row:A"), rig.at("row:B"), rig.at("row:C"));
    // An unselected row drags alone.
    assert_eq!(rig.drag(c, a), vec![Event::Move { ids: vec![4], target: 2, zone: Zone::Before }]);
    // A selected row drags the selection of its kind.
    rig.click(a);
    rig.click_mods(b, Modifiers::COMMAND);
    assert_eq!(rig.drag(b, c), vec![Event::Move { ids: vec![2, 3], target: 4, zone: Zone::Before }]);
    // G takes a drop before it in its top part, and inside it below that.
    let g = rig.rect(GROUP);
    assert_eq!(rig.drag(c, pos2(g.center().x, g.top() + 3.0)), vec![Event::Move { ids: vec![4], target: GROUP, zone: Zone::Before }]);
    assert_eq!(rig.drag(c, g.center()), vec![Event::Move { ids: vec![4], target: GROUP, zone: Zone::Into }]);
    rig.into_g.set(false);
    assert_eq!(rig.drag(c, g.center()), Vec::new(), "the host's rule refuses it");
    assert_eq!(rig.drag(c, c + vec2(0.0, 4.0)), Vec::new(), "never onto itself");
}

#[test]
fn another_kind_of_drag_passes_over_the_rows_untouched() {
    let rig = Rig::new();
    egui::DragAndDrop::set_payload(&rig.ctx, 9_u32);
    let g = rig.rect(GROUP).center();
    assert_eq!(rig.frame(vec![Input::PointerMoved(g)]), Vec::new());
    assert!(egui::DragAndDrop::payload::<u32>(&rig.ctx).is_some(), "the payload is left for its own target");
    egui::DragAndDrop::clear_payload(&rig.ctx);
}

#[test]
fn a_document_switch_cancels_only_its_own_drag() {
    let ctx = crate::test_support::context();
    egui::DragAndDrop::set_payload(&ctx, Dragged { scope: 7, class: 0, ids: vec![2] });
    cancel_drag(&ctx, 8);
    assert!(egui::DragAndDrop::payload::<Dragged>(&ctx).is_some());
    cancel_drag(&ctx, 7);
    assert!(egui::DragAndDrop::payload::<Dragged>(&ctx).is_none());
}

#[test]
fn a_row_menu_returns_the_chosen_item_and_a_host_menu_returns_where_it_opened() {
    let rig = Rig::new();
    assert_eq!(rig.click_with(rig.at("row:G"), PointerButton::Secondary, Modifiers::NONE), Vec::new());
    rig.frame(Vec::new());
    assert_eq!(
        rig.click(rig.at("browser:menu:ui.ungroup")),
        vec![Event::Action { id: GROUP, action: "ui.ungroup".into(), params: Some(json!({ "group": 5 })) }]
    );
    let settings = rig.at("row:Settings");
    assert_eq!(rig.click_with(settings, PointerButton::Secondary, Modifiers::NONE), vec![Event::Menu { id: SETTINGS, at: settings }]);
}

#[test]
fn a_revealed_row_is_scrolled_into_view_and_reported_once() {
    let rig = Rig::new();
    rig.host.borrow_mut().extra = 30;
    rig.height.set(120.0);
    rig.frame(Vec::new());
    rig.state.borrow_mut().reveal(129);
    let mut revealed = Vec::new();
    // The scroll is animated: give it well over its longest run (a third of a second).
    for _ in 0..12 {
        revealed.extend(rig.wait(0.1).into_iter().filter(|e| matches!(e, Event::Revealed(_))));
    }
    assert_eq!(revealed, vec![Event::Revealed(129)]);
    assert!(rig.rect(129).bottom() <= 600.0);
}

#[test]
fn keyboard_and_search_are_off_by_default() {
    let rig = Rig::new();
    rig.click(rig.at("row:A"));
    assert_eq!(rig.key(egui::Key::ArrowDown, Modifiers::NONE), Vec::new());
    assert!(solidcraft_ui_registry::handle_at("browser:search").is_none(), "no header, no search");
}

#[test]
fn with_the_keyboard_on_arrows_move_the_selection_and_enter_opens() {
    let rig = Rig::new();
    rig.keyboard.set(true);
    assert_eq!(rig.click(rig.at("row:A")), vec![Event::Select { id: 2, how: How::Only }]);
    assert_eq!(rig.key(egui::Key::ArrowDown, Modifiers::NONE), vec![Event::Select { id: 3, how: How::Only }]);
    assert_eq!(rig.key(egui::Key::ArrowDown, Modifiers::SHIFT), vec![Event::Select { id: 4, how: How::Range(vec![3, 4]) }]);
    assert_eq!(rig.key(egui::Key::Enter, Modifiers::NONE), vec![Event::Open(4)]);
    // A press outside the panel takes the keys back.
    rig.click(pos2(390.0, 590.0));
    assert_eq!(rig.key(egui::Key::ArrowUp, Modifiers::NONE), Vec::new());
}

#[test]
fn with_search_on_the_rows_are_the_matches_and_the_rows_above_them() {
    let rig = Rig::new();
    rig.search.set(true);
    rig.frame(Vec::new());
    assert!(solidcraft_ui_registry::handle_at("browser:search").is_some());
    rig.state.borrow_mut().set_query("c");
    rig.frame(Vec::new());
    rig.frame(Vec::new());
    assert_eq!(rig.labels(), ["Bodies", "C"]);
    rig.state.borrow_mut().set_query("");
    rig.frame(Vec::new());
    assert_eq!(rig.labels(), ["Bodies", "A", "B", "C", "G", "Settings"]);
    // The header folds the card.
    rig.click(rig.at("browser:header"));
    assert!(rig.state.borrow().collapsed());
    assert_eq!(rig.labels(), Vec::<String>::new());
}

#[test]
fn fold_memory_keeps_rows_opened_or_closed_by_hand_whichever_way_they_start() {
    let mut folds = FoldMemory::default();
    assert!(folds.is_open("a", true));
    assert!(!folds.is_open("b", false));
    assert!(folds.set_open("a", true, false));
    assert!(!folds.set_open("a", true, false), "no change");
    assert!(!folds.is_open("a", true));
    assert!(folds.toggle("b", false));
    assert!(folds.is_open("b", false));
    folds.apply(false, ["a", "b"]);
    assert!(!folds.is_open("a", true) && !folds.is_open("b", false));
}

#[test]
fn order_before_moves_the_dragged_entries_before_the_target_or_to_the_end() {
    assert_eq!(order_before(&[1, 2, 3, 4], &[4, 1], Some(&3)), vec![2, 4, 1, 3]);
    assert_eq!(order_before(&[1, 2, 3], &[1], None), vec![2, 3, 1]);
    assert_eq!(order_before(&[1, 2, 3], &[2], Some(&9)), vec![1, 3, 2], "an unknown target is the end");
}

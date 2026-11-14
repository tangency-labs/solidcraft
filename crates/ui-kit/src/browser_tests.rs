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
struct Host {
    selected: Vec<NodeId>,
    hidden: Vec<NodeId>,
    folds: FoldMemory,
    extra: usize,
    /// How the bodies are renamed.
    rename: Rename,
    /// When C's drag starts, the host adds this undrawn item to the set, with its own data.
    hidden_in_drag: Option<NodeId>,
    /// G's own drop rule: the one zone it takes.
    g_takes: Option<Zone>,
    /// B is laid out by the host itself, out of view: passed, not drawn.
    pass_b: bool,
    /// Each row's response this frame.
    outs: RefCell<Vec<(NodeId, RowOut)>>,
}

impl Default for Host {
    fn default() -> Self {
        Host {
            selected: Vec::new(),
            hidden: Vec::new(),
            folds: FoldMemory::default(),
            extra: 0,
            rename: Rename::Inline,
            hidden_in_drag: None,
            g_takes: None,
            pass_b: false,
            outs: RefCell::new(Vec::new()),
        }
    }
}

impl Host {
    /// One row, its response kept; on C's drag start the host hands the panel its own set.
    fn emit(&self, rows: &mut Rows<'_>, node: Node<'_>) {
        let id = node.id;
        let out = rows.row(node);
        if out.drag_started
            && id == 4
            && let Some(hidden) = self.hidden_in_drag
        {
            rows.drag_with(vec![4, hidden], Some(Payload::new("C and a closed group's item")));
        }
        self.outs.borrow_mut().push((id, out));
    }

    fn rows(&self, rows: &mut Rows<'_>) {
        self.outs.borrow_mut().clear();
        let open = self.folds.is_open("bodies", true);
        self.emit(
            rows,
            Node {
                id: 1,
                key: "bodies",
                row: Row { fold: Some(open), eye: Some(!self.hidden.contains(&1)), icon: "folder", label: "Bodies", ..Default::default() },
                click_folds: true,
                drops: Drops { before: false, into: true },
                ..Default::default()
            },
        );
        if open {
            let extra: Vec<(NodeId, String)> = (0..self.extra).map(|i| (100 + i as NodeId, format!("Body {i}"))).collect();
            let mut bodies: Vec<(NodeId, &str)> = BODIES.to_vec();
            bodies.extend(extra.iter().map(|(id, label)| (*id, label.as_str())));
            for (id, label) in bodies {
                if id == 3 && self.pass_b {
                    rows.pass(id, Selectable::Yes(0));
                    continue;
                }
                self.emit(
                    rows,
                    Node {
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
                        rename: self.rename,
                        drag: Some(0),
                        drops: Drops { before: true, into: false },
                        ..Default::default()
                    },
                );
            }
        }
        let g_takes = self.g_takes;
        let g_rule = move |_: &Dragged, zone: Zone| Some(zone) == g_takes;
        self.emit(
            rows,
            Node {
                id: GROUP,
                accept: g_takes.is_some().then_some(&g_rule as &dyn Fn(&Dragged, Zone) -> bool),
                key: "g",
                row: Row { fold: Some(self.folds.is_open("g", true)), icon: "folder", label: "G", name: true, ..Default::default() },
                select: Selectable::Yes(1),
                drag: Some(1),
                drops: Drops { before: true, into: true },
                menu: NodeMenu::Items(&group_menu),
                ..Default::default()
            },
        );
        self.emit(
            rows,
            Node {
                id: SETTINGS,
                key: "settings",
                row: Row { icon: "settings", label: "Settings", ..Default::default() },
                menu: NodeMenu::Host,
                ..Default::default()
            },
        );
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
    /// The panel's options under test.
    per_row: Cell<bool>,
    scroll: Cell<bool>,
    active: Cell<bool>,
    /// The drop rule also asks for this scope (another document's drags never land).
    scope_rule: Cell<bool>,
    drawn: RefCell<Vec<Drawn>>,
    content_height: Cell<f32>,
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
            per_row: Cell::new(false),
            scroll: Cell::new(true),
            active: Cell::new(true),
            scope_rule: Cell::new(false),
            drawn: RefCell::new(Vec::new()),
            content_height: Cell::new(0.0),
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
            let (into_g, scope_rule) = (self.into_g.get(), self.scope_rule.get());
            let accept = move |dragged: &Dragged, target: NodeId, zone: Zone| {
                (into_g || !(target == GROUP && zone == Zone::Into)) && (!scope_rule || dragged.source.scope == 7)
            };
            let mut browser = Browser::new(&mut state, Style::default(), 7)
                .accept(&accept)
                .keyboard(self.keyboard.get())
                .events_per_row(self.per_row.get())
                .scroll(self.scroll.get())
                .active(self.active.get());
            if self.search.get() {
                browser = browser.header(Header { title: "Objects", count: "6", collapsible: true, search: Some("Search") });
            }
            out = ui.allocate_ui(vec2(280.0, self.height.get()), |ui| browser.show(ui, |rows| host.rows(rows))).inner;
        });
        let mut events = out.events;
        if self.per_row.get() {
            events.extend(self.host.borrow().outs.borrow().iter().flat_map(|(_, row)| row.events.clone()));
        }
        self.host.borrow_mut().apply(&events);
        *self.drawn.borrow_mut() = out.drawn;
        self.content_height.set(out.content_height);
        events
    }

    /// Row `id`'s response in the last frame.
    fn out(&self, id: NodeId) -> RowOut {
        self.host.borrow().outs.borrow().iter().find(|(row, _)| *row == id).map(|(_, out)| out.clone()).unwrap_or_default()
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
    // The click's frame drew the rows before the host had the event; the host's fold shows in the
    // rows it emits on the next frame.
    assert_eq!(rig.click(rig.at("fold:Bodies")), vec![Event::Fold { id: 1, open: false }]);
    assert_eq!(rig.labels(), ["Bodies", "A", "B", "C", "G", "Settings"], "drawn before the host folded it");
    rig.frame(Vec::new());
    assert_eq!(rig.labels(), ["Bodies", "G", "Settings"], "the host folded it");
    assert_eq!(rig.click(rig.at("row:Bodies")), vec![Event::Click(1), Event::Fold { id: 1, open: true }]);
    rig.frame(Vec::new());
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
    assert_eq!(
        rig.drag(c, a),
        vec![Event::Move { ids: vec![4], target: 2, zone: Zone::Before, source: Source { scope: 7, presentation: 0 }, data: None }]
    );
    // A selected row drags the selection of its kind.
    rig.click(a);
    rig.click_mods(b, Modifiers::COMMAND);
    assert_eq!(
        rig.drag(b, c),
        vec![Event::Move { ids: vec![2, 3], target: 4, zone: Zone::Before, source: Source { scope: 7, presentation: 0 }, data: None }]
    );
    // G takes a drop before it in its top part, and inside it below that.
    let g = rig.rect(GROUP);
    assert_eq!(
        rig.drag(c, pos2(g.center().x, g.top() + 3.0)),
        vec![Event::Move { ids: vec![4], target: GROUP, zone: Zone::Before, source: Source { scope: 7, presentation: 0 }, data: None }]
    );
    assert_eq!(
        rig.drag(c, g.center()),
        vec![Event::Move { ids: vec![4], target: GROUP, zone: Zone::Into, source: Source { scope: 7, presentation: 0 }, data: None }]
    );
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
    egui::DragAndDrop::set_payload(&ctx, Dragged { source: Source { scope: 7, presentation: 0 }, class: 0, ids: vec![2], data: None });
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

#[test]
fn a_host_renamed_row_asks_the_host_on_a_slow_click_and_on_request() {
    let rig = Rig::new();
    rig.host.borrow_mut().rename = Rename::Host;
    let a = rig.at("row:A");
    rig.click(a);
    assert_eq!(rig.click(a), Vec::new(), "the slow second click is held");
    assert_eq!(rig.wait(0.4), vec![Event::RenameRequested(2)]);
    assert_eq!(rig.state.borrow().renaming(), None, "the host opens its own editor");
    // A rename asked for elsewhere (F2, a menu) goes the row's way.
    rig.state.borrow_mut().request_rename(3);
    assert_eq!(rig.frame(Vec::new()), vec![Event::RenameRequested(3)]);
    rig.host.borrow_mut().rename = Rename::Inline;
    rig.state.borrow_mut().request_rename(3);
    assert_eq!(rig.frame(Vec::new()), Vec::new());
    assert_eq!(rig.state.borrow().renaming(), Some(3), "an inline row opens the kit's editor");
    let out = rig.out(3);
    assert!(out.widget.is_some() && out.label_rect.is_some(), "a host editor has the row's id and name area");
}

#[test]
fn with_events_per_row_each_row_hands_back_its_own_and_the_frame_none() {
    let rig = Rig::new();
    rig.per_row.set(true);
    rig.frame(Vec::new());
    assert_eq!(rig.click(rig.at("row:B")), vec![Event::Select { id: 3, how: How::Only }]);
    let b = rig.out(3);
    assert!(b.clicked && b.hovered);
    assert_eq!(b.events, vec![Event::Select { id: 3, how: How::Only }], "B's response holds B's click");
    assert!(rig.out(2).events.is_empty());
    // While a drag hovers a row that would take it, its response says where it would land.
    let c = rig.at("row:C");
    let g = rig.rect(GROUP).center();
    rig.frame(vec![Input::PointerMoved(c)]);
    rig.press(c, PointerButton::Primary, true, Modifiers::NONE);
    rig.frame(vec![Input::PointerMoved(c + vec2(0.0, 8.0))]);
    assert!(rig.out(4).drag_started);
    rig.frame(vec![Input::PointerMoved(g)]);
    rig.frame(vec![Input::PointerMoved(g)]);
    assert_eq!(rig.out(GROUP).drop_zone, Some(Zone::Into));
    assert_eq!(rig.out(3).drop_zone, None);
    let dropped = rig.press(g, PointerButton::Primary, false, Modifiers::NONE);
    assert!(matches!(dropped.as_slice(), [Event::Move { target: GROUP, zone: Zone::Into, .. }]), "{dropped:?}");
}

#[test]
fn a_drag_carries_its_source_and_the_hosts_set_and_data_and_the_host_rule_decides() {
    let rig = Rig::new();
    rig.host.borrow_mut().hidden_in_drag = Some(99);
    let (a, c) = (rig.at("row:A"), rig.at("row:C"));
    let events = rig.drag(c, a);
    let [Event::Move { ids, target: 2, zone: Zone::Before, source, data: Some(data) }] = events.as_slice() else { panic!("{events:?}") };
    assert_eq!(ids, &vec![4, 99], "the host's set, with an item no row draws");
    assert_eq!(*source, Source { scope: 7, presentation: 0 });
    assert_eq!(data.get::<&str>(), Some(&"C and a closed group's item"));

    // Another presentation of the same document: the host's rule takes it.
    rig.scope_rule.set(true);
    let g = rig.rect(GROUP).center();
    let other = Dragged { source: Source { scope: 7, presentation: 3 }, class: 0, ids: vec![50], data: None };
    egui::DragAndDrop::set_payload(&rig.ctx, other.clone());
    rig.frame(vec![Input::PointerMoved(g)]);
    let events = rig.press(g, PointerButton::Primary, false, Modifiers::NONE);
    assert_eq!(events, vec![Event::Move { ids: vec![50], target: GROUP, zone: Zone::Into, source: other.source, data: None }]);
    // Another document: refused by the same rule.
    egui::DragAndDrop::set_payload(&rig.ctx, Dragged { source: Source { scope: 8, presentation: 0 }, ..other });
    rig.frame(vec![Input::PointerMoved(g)]);
    assert_eq!(rig.press(g, PointerButton::Primary, false, Modifiers::NONE), Vec::new());
}

#[test]
fn without_a_scroll_area_the_rows_take_the_room_they_need() {
    let rig = Rig::new();
    rig.height.set(60.0);
    rig.frame(Vec::new());
    assert!(!rig.labels().contains(&"Settings".to_owned()), "a scroll area clips the rows to its height");
    rig.scroll.set(false);
    rig.frame(Vec::new());
    assert_eq!(rig.labels(), ["Bodies", "A", "B", "C", "G", "Settings"]);
    assert!(rig.content_height.get() > 60.0);
}

#[test]
fn an_inactive_presentation_draws_and_emits_nothing_and_ends_its_gestures() {
    let rig = Rig::new();
    // An eye drag under way ends when its presentation goes.
    let (a, b) = (rig.at("eye:A"), rig.at("eye:B"));
    rig.frame(vec![Input::PointerMoved(a)]);
    rig.press(a, PointerButton::Primary, true, Modifiers::NONE);
    assert_eq!(rig.frame(vec![Input::PointerMoved(b)]).first(), Some(&Event::EyeGesture(Gesture::Begin)));
    rig.active.set(false);
    assert_eq!(rig.frame(vec![Input::PointerMoved(b)]), vec![Event::EyeGesture(Gesture::End)]);
    assert!(rig.labels().is_empty(), "nothing is drawn");
    assert!(rig.ctx.dragged_id().is_none());
    assert_eq!(rig.press(b, PointerButton::Primary, false, Modifiers::NONE), Vec::new());

    // A row drag it began is cancelled.
    rig.active.set(true);
    rig.frame(Vec::new());
    let c = rig.at("row:C");
    rig.frame(vec![Input::PointerMoved(c)]);
    rig.press(c, PointerButton::Primary, true, Modifiers::NONE);
    rig.frame(vec![Input::PointerMoved(c + vec2(0.0, 8.0))]);
    assert!(egui::DragAndDrop::has_payload_of_type::<Dragged>(&rig.ctx));
    rig.active.set(false);
    assert_eq!(rig.frame(vec![Input::PointerMoved(c + vec2(0.0, 12.0))]), Vec::new());
    assert!(!egui::DragAndDrop::has_payload_of_type::<Dragged>(&rig.ctx));
    assert!(rig.ctx.dragged_id().is_none());
}

#[test]
fn a_rows_own_drop_rule_decides_and_its_top_part_falls_back_to_inside() {
    let rig = Rig::new();
    let (c, g) = (rig.at("row:C"), rig.rect(GROUP));
    // G takes drops only before it: over its middle a drop never lands before it.
    rig.host.borrow_mut().g_takes = Some(Zone::Before);
    assert_eq!(rig.drag(c, g.center()), Vec::new());
    let events = rig.drag(c, pos2(g.center().x, g.top() + 3.0));
    assert!(matches!(events.as_slice(), [Event::Move { target: GROUP, zone: Zone::Before, .. }]), "{events:?}");
    // Its own rule overrides the panel's: inside only, even in its top part.
    rig.host.borrow_mut().g_takes = Some(Zone::Into);
    rig.into_g.set(false);
    let events = rig.drag(c, pos2(g.center().x, g.top() + 3.0));
    assert!(matches!(events.as_slice(), [Event::Move { target: GROUP, zone: Zone::Into, .. }]), "{events:?}");
}

#[test]
fn a_rows_response_has_its_widget_response_and_whether_a_press_was_on_a_control() {
    let rig = Rig::new();
    rig.click(rig.at("row:B"));
    let b = rig.out(3);
    assert!(b.response.as_ref().is_some_and(|r| b.rect == Some(r.rect)));
    assert!(b.eye_rect.is_some_and(|eye| eye.contains(rig.at("eye:B"))), "the eye's area holds the eye");
    assert!(!b.on_control);
    let eye = rig.at("eye:B");
    rig.frame(vec![Input::PointerMoved(eye)]);
    rig.press(eye, PointerButton::Primary, true, Modifiers::NONE);
    assert!(rig.out(3).on_control, "the press is on the eye");
    rig.press(eye, PointerButton::Primary, false, Modifiers::NONE);
}

#[test]
fn a_row_the_host_passes_over_keeps_its_place_in_a_range() {
    let rig = Rig::new();
    rig.host.borrow_mut().pass_b = true;
    rig.frame(Vec::new());
    assert_eq!(rig.labels(), ["Bodies", "A", "C", "G", "Settings"], "B is not drawn");
    rig.click(rig.at("row:A"));
    assert_eq!(rig.click_mods(rig.at("row:C"), Modifiers::SHIFT), vec![Event::Select { id: 4, how: How::Range(vec![2, 3, 4]) }]);
}

#[test]
fn the_gestures_of_a_presentation_not_drawn_end_from_the_context() {
    let rig = Rig::new();
    let (a, b) = (rig.at("eye:A"), rig.at("eye:B"));
    rig.frame(vec![Input::PointerMoved(a)]);
    rig.press(a, PointerButton::Primary, true, Modifiers::NONE);
    assert_eq!(rig.frame(vec![Input::PointerMoved(b)]).first(), Some(&Event::EyeGesture(Gesture::Begin)));
    let ended = end_gestures(&rig.ctx, &mut rig.state.borrow_mut(), Source { scope: 7, presentation: 0 });
    assert_eq!(ended, vec![Event::EyeGesture(Gesture::End)]);
    assert!(rig.ctx.dragged_id().is_none());
    assert!(end_gestures(&rig.ctx, &mut rig.state.borrow_mut(), Source { scope: 7, presentation: 0 }).is_empty(), "once");
}

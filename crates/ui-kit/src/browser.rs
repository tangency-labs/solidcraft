//! The Browser: a tree panel drawn from the host's rows. The host emits its rows each frame
//! ([`Rows::row`], only those of open branches, and may skip the cost of rows scrolled out of view
//! with [`Rows::next_visible`] and [`Rows::skip`]); the panel draws them with [`tree_row`] and
//! returns what the user did as [`Event`]s. It never changes the host's state: selection,
//! visibility, folds, names and order are the host's, and each event maps to one of its commands.
//! The events come back after the frame's rows are drawn, so the host's answer shows in the rows it
//! emits next frame: on [`Event::Fold`] it closes the row and stops emitting its children itself
//! (the panel draws exactly the rows it is given).
//!
//! The panel keeps, per presentation, what only the gesture needs ([`BrowserState`]): the Shift
//! anchor and the rows' order, the slow second click that renames, the inline name editor, the
//! dragged rows and their drop marks, the eye drag, the row to reveal, the search text and the
//! keyboard cursor. Keyboard arrows and search are options, off by default.
//!
//! A host that acts on each row as it draws it turns on [`Browser::events_per_row`]: each row's
//! events come back in its [`RowOut`] instead of the frame's [`Output`]. A host with its own name
//! editor marks its rows [`Rename::Host`]; a host showing the rows twice gives each presentation its
//! own [`BrowserState`] and id ([`Browser::presentation`]), and marks a hidden one inactive
//! ([`Browser::active`]), which ends the gestures it holds.
//!
//! Selection follows the platform: a click selects that row only, Cmd/Ctrl toggles it, Shift takes
//! the rows from the anchor (Shift with Cmd/Ctrl adds them). Rows that are no item
//! ([`Selectable::No`]) only report a click.

use std::collections::{BTreeSet, HashSet};

use egui::{Key, Modifiers, Pos2, Rect, Sense, Stroke, pos2, vec2};
use solidcraft_theme::Tokens;

use crate::BackingTooltipExt as _;
use crate::menu::{Item, Menu, MenuItem};
use crate::{Row, RowResp, eye_drag_id, tree_row, tree_row_rect};

/// A row's identity, chosen by the host and stable from frame to frame.
pub type NodeId = u64;

/// A second click on the same name later than this (seconds) is a new gesture, not a rename.
const SLOW_RENAME_WINDOW: f64 = 3.0;
/// The top part of a row that takes a drop before it, when it also takes one inside it.
const BEFORE_ZONE: f32 = 0.3;

/// Whether a row is an item the selection holds, and its kind: a Shift range and a dragged set
/// only take rows of one kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Selectable {
    /// A folder, a setting, a step: a click reports [`Event::Click`].
    #[default]
    No,
    Yes(u32),
}

/// How a row's name is renamed: by a slower second click on it (once the double-click delay has
/// passed), F2 with the keyboard on, or [`BrowserState::request_rename`] (a menu's Rename).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rename {
    #[default]
    Never,
    /// In the panel's own editor, in the row.
    Inline,
    /// By the host: the panel reports [`Event::RenameRequested`] and the host opens its editor.
    Host,
}

/// The drops a row takes: before it (a line over it), inside it (a box round it). A row taking
/// both takes a drop before it in its top part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drops {
    pub before: bool,
    pub into: bool,
}

/// A row's context menu.
#[derive(Clone, Copy, Default)]
pub enum NodeMenu<'a> {
    #[default]
    None,
    /// The host opens its own menu on [`Event::Menu`].
    Host,
    /// The panel shows these entries (built only while the menu is open) and reports the chosen
    /// one as [`Event::Action`].
    Items(&'a dyn Fn() -> Vec<Item>),
}

/// One row as the host gives it.
#[derive(Default)]
pub struct Node<'a> {
    pub id: NodeId,
    /// The host's stable key for the row (automation and drawn-row geometry).
    pub key: &'a str,
    /// What is drawn: `fold` is the row's current open state, `eye` its visibility.
    pub row: Row<'a>,
    /// The text the name editor starts from; the label when `None`.
    pub name: Option<&'a str>,
    pub select: Selectable,
    pub rename: Rename,
    /// The row can be dragged, carrying this kind.
    pub drag: Option<u32>,
    pub drops: Drops,
    pub menu: NodeMenu<'a>,
    /// A click on the row's label folds it as its arrow does (folders), after [`Event::Click`].
    pub click_folds: bool,
    /// The row's own drop rule (where a drag may land on it), in place of the panel's.
    pub accept: Option<&'a dyn Fn(&Dragged, Zone) -> bool>,
}

/// How the rows are drawn this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    /// Compact single-line scene rows.
    pub compact: bool,
    /// The eye beside the fold arrow instead of at the row's end.
    pub eye_left: bool,
}

/// The card's title band: title, count text, a chevron folding the card, and a search action.
#[derive(Clone, Copy, Debug, Default)]
pub struct Header<'a> {
    pub title: &'a str,
    pub count: &'a str,
    pub collapsible: bool,
    /// The search field's hint; `None`: no search.
    pub search: Option<&'a str>,
}

/// How a click changes the selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum How {
    Only,
    Toggle,
    /// The rows of the clicked row's kind from the anchor to it, in drawn order.
    Range(Vec<NodeId>),
    /// The same, added to the selection.
    RangeAdd(Vec<NodeId>),
}

/// Where a dragged set lands on its target row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Before,
    Into,
}

/// The ends of one drag over the eyes: the host makes the eyes it set one undo step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    Begin,
    End,
}

/// What the user did, for the host to run.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Select {
        id: NodeId,
        how: How,
    },
    /// A plain click on a row that is no item.
    Click(NodeId),
    /// A double-click on the row, or Enter on it.
    Open(NodeId),
    Fold {
        id: NodeId,
        open: bool,
    },
    /// One per row a click or an eye drag set.
    Eye {
        id: NodeId,
        visible: bool,
    },
    EyeGesture(Gesture),
    /// The row's radio.
    Activate(NodeId),
    /// The name editor kept `text` (Enter, or leaving it).
    Rename {
        id: NodeId,
        text: String,
    },
    /// Dragged rows dropped on `target`: where they came from and the host's data, as dragged.
    Move {
        ids: Vec<NodeId>,
        target: NodeId,
        zone: Zone,
        source: Source,
        data: Option<Payload>,
    },
    /// A trailing action (its target) or a menu entry (its id and parameters).
    Action {
        id: NodeId,
        action: String,
        params: Option<serde_json::Value>,
    },
    /// The button after the name ([`Row::action`]).
    NameAction(NodeId),
    /// A secondary click on a [`NodeMenu::Host`] row.
    Menu {
        id: NodeId,
        at: Pos2,
    },
    /// The row [`BrowserState::reveal`] asked for is in view.
    Revealed(NodeId),
    /// A [`Rename::Host`] row is to be renamed.
    RenameRequested(NodeId),
}

/// Where dragged rows come from: the host's scope (its document) and the presentation (one of the
/// places the same rows are shown).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Source {
    pub scope: u64,
    pub presentation: u64,
}

/// The host's own data riding on a drag, of any type.
#[derive(Clone)]
pub struct Payload(std::sync::Arc<dyn std::any::Any + Send + Sync>);

impl Payload {
    pub fn new<T: std::any::Any + Send + Sync>(value: T) -> Self {
        Payload(std::sync::Arc::new(value))
    }

    /// The data, when it is a `T`.
    pub fn get<T: std::any::Any>(&self) -> Option<&T> {
        self.0.downcast_ref::<T>()
    }
}

/// The same data, not equal values: a drag's data is compared by identity.
impl PartialEq for Payload {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for Payload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Payload(..)")
    }
}

/// The rows being dragged: where from, their kind, their ids (the selected rows of that kind as
/// drawn, or the set the host gave with [`Rows::drag_with`]) and the host's data.
#[derive(Clone, Debug, PartialEq)]
pub struct Dragged {
    pub source: Source,
    pub class: u32,
    pub ids: Vec<NodeId>,
    pub data: Option<Payload>,
}

/// The host's drop rules over where a drag comes from and where it would land: may `Dragged` land
/// on the row `NodeId` in `Zone`? Without them a row takes drags of its own scope.
pub type Accept<'a> = &'a dyn Fn(&Dragged, NodeId, Zone) -> bool;

/// A row on screen this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawn {
    pub id: NodeId,
    pub key: String,
    pub label: String,
    pub rect: Rect,
    /// The fold arrow's area.
    pub fold: Option<Rect>,
    pub open: Option<bool>,
}

/// What a frame of the panel returns.
#[derive(Debug, Default)]
pub struct Output {
    pub events: Vec<Event>,
    /// The row under the pointer.
    pub hovered: Option<NodeId>,
    pub drawn: Vec<Drawn>,
    /// The pointer is over a row (or a row holds the pointer's gesture).
    pub pointer_owned: bool,
    pub content_height: f32,
}

/// What a row reports back as it is drawn, for a host acting on it there (lighting, its own
/// editor, a drag set): the gestures are still the panel's.
#[derive(Clone, Debug, Default)]
pub struct RowOut {
    pub rect: Option<Rect>,
    /// Where the name is drawn (a host's own editor goes there).
    pub label_rect: Option<Rect>,
    /// The row's widget id.
    pub widget: Option<egui::Id>,
    pub hovered: bool,
    /// A click the panel took as a click on the row (selection or [`Event::Click`]).
    pub clicked: bool,
    pub double: bool,
    /// A secondary click, where.
    pub secondary: Option<Pos2>,
    /// A drag of the row began: [`Rows::drag_with`] may give its set now.
    pub drag_started: bool,
    /// Where a drag over the row, which the rules accept, would land.
    pub drop_zone: Option<Zone>,
    pub fold_at: Option<Pos2>,
    pub eye_at: Option<Pos2>,
    /// The eye's area, within the rows' clip (a host's own target for it).
    pub eye_rect: Option<Rect>,
    pub radio_at: Option<Pos2>,
    /// The row's widget response (tooltips, a host's own drags).
    pub response: Option<egui::Response>,
    /// The press under way began on the fold arrow, eye, radio or an action, not on the row.
    pub on_control: bool,
    /// The row's events, with [`Browser::events_per_row`] (else they are in [`Output::events`]).
    pub events: Vec<Event>,
}

#[derive(Clone, Debug)]
struct Editor {
    id: NodeId,
    text: String,
    focused: bool,
    error: Option<String>,
}

impl Editor {
    fn new(id: NodeId, text: &str) -> Self {
        Editor { id, text: text.to_owned(), focused: false, error: None }
    }
}

/// A row as the keyboard moves over it next frame.
#[derive(Clone, Debug)]
struct KeyRow {
    id: NodeId,
    select: Selectable,
    fold: Option<bool>,
    rename: Rename,
    name: String,
}

/// What the panel keeps between frames, one per presentation; the host owns the value.
#[derive(Debug, Default)]
pub struct BrowserState {
    anchor: Option<NodeId>,
    /// This frame's and the last frame's selectable rows, with their kind, in drawn order.
    order: Vec<(NodeId, u32)>,
    previous: Vec<(NodeId, u32)>,
    /// The last click: row, time, whether it was a plain click on a renamable row.
    last_click: Option<(NodeId, f64, bool)>,
    /// A slow second click held until the double-click delay: row, when it renames, when made.
    pending: Option<(NodeId, f64, f64)>,
    editor: Option<Editor>,
    /// A rename asked for outside a click, done when its row is drawn.
    rename_request: Option<NodeId>,
    /// The widget a drag or eye drag of this presentation holds.
    drag_widget: Option<egui::Id>,
    reveal: Option<NodeId>,
    scroll_to: Option<NodeId>,
    /// An eye drag that began in this presentation is under way.
    eye_gesture: bool,
    /// Rows that acted this frame: egui may lay a frame out twice, and an action runs once.
    claims: (u64, HashSet<NodeId>),
    collapsed: bool,
    search_open: bool,
    query: String,
    /// The rows the query shows (matches and the rows above them), from the last frame.
    shown: Option<HashSet<NodeId>>,
    keys: Vec<KeyRow>,
    cursor: Option<NodeId>,
    focused: bool,
}

impl BrowserState {
    /// Open the name editor on row `id`, starting from `text` (F2, a menu's Rename, a double-click).
    pub fn start_rename(&mut self, id: NodeId, text: &str) {
        self.pending = None;
        self.editor = Some(Editor::new(id, text));
    }

    /// The host refused the name the editor kept: the editor opens again with it and `message`.
    pub fn refuse_rename(&mut self, id: NodeId, text: &str, message: &str) {
        self.editor = Some(Editor { error: Some(message.to_owned()), ..Editor::new(id, text) });
    }

    /// Rename row `id` its way ([`Rename`]) when it is next drawn: a menu's Rename, F2.
    pub fn request_rename(&mut self, id: NodeId) {
        self.rename_request = Some(id);
    }

    pub fn cancel_rename(&mut self) {
        self.editor = None;
    }

    /// The row whose name is being edited.
    pub fn renaming(&self) -> Option<NodeId> {
        self.editor.as_ref().map(|e| e.id)
    }

    /// Scroll row `id` into view when it is drawn, then report [`Event::Revealed`]. The host opens
    /// the rows above it.
    pub fn reveal(&mut self, id: NodeId) {
        self.reveal = Some(id);
    }

    /// Where the next Shift range starts.
    pub fn anchor(&self) -> Option<NodeId> {
        self.anchor
    }

    /// Search for `query` (the search option must be on to filter).
    pub fn set_query(&mut self, query: &str) {
        query.clone_into(&mut self.query);
        self.search_open = true;
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// The card is folded to its header.
    pub fn collapsed(&self) -> bool {
        self.collapsed
    }

    pub fn set_collapsed(&mut self, collapsed: bool) {
        self.collapsed = collapsed;
    }

    /// The panel takes the arrow keys (keyboard option on, and the last press was on a row).
    pub fn focused(&self) -> bool {
        self.focused
    }
}

/// Open and closed rows kept by key, whichever way each starts: a row opened or closed by hand is
/// noted in both sets, so it keeps its state when its default changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FoldMemory {
    pub collapsed: BTreeSet<String>,
    pub expanded: BTreeSet<String>,
}

impl FoldMemory {
    pub fn is_open(&self, key: &str, default: bool) -> bool {
        if default { !self.collapsed.contains(key) } else { self.expanded.contains(key) }
    }

    /// Open or close a row; whether that changed it.
    pub fn set_open(&mut self, key: &str, default: bool, open: bool) -> bool {
        let was = self.is_open(key, default);
        if open {
            self.collapsed.remove(key);
            self.expanded.insert(key.to_owned());
        } else {
            self.collapsed.insert(key.to_owned());
            self.expanded.remove(key);
        }
        was != open
    }

    /// Flip a row; its new state.
    pub fn toggle(&mut self, key: &str, default: bool) -> bool {
        let open = !self.is_open(key, default);
        self.set_open(key, default, open);
        open
    }

    /// Expand All or Collapse All over `keys`.
    pub fn apply<'k>(&mut self, open: bool, keys: impl IntoIterator<Item = &'k str>) {
        for key in keys {
            self.set_open(key, false, open);
        }
    }
}

/// `top` with the `dragged` entries moved to just before `before` (to the end without it, or when
/// it is not in `top`).
pub fn order_before<T: PartialEq + Clone>(top: &[T], dragged: &[T], before: Option<&T>) -> Vec<T> {
    let mut order: Vec<T> = top.iter().filter(|e| !dragged.contains(e)).cloned().collect();
    let at = before.and_then(|b| order.iter().position(|e| e == b)).unwrap_or(order.len());
    for (i, e) in dragged.iter().enumerate() {
        order.insert(at + i, e.clone());
    }
    order
}

/// Drop a drag of rows from `scope` (its document went away); other drags are left alone.
pub fn cancel_drag(ctx: &egui::Context, scope: u64) {
    if egui::DragAndDrop::payload::<Dragged>(ctx).is_some_and(|d| d.source.scope == scope) {
        egui::DragAndDrop::clear_payload(ctx);
    }
}

/// The panel for one frame.
pub struct Browser<'s> {
    state: &'s mut BrowserState,
    style: Style,
    scope: u64,
    presentation: u64,
    accept: Option<Accept<'s>>,
    header: Option<Header<'s>>,
    keyboard: bool,
    shrink: bool,
    scroll: bool,
    active: bool,
    per_row: bool,
}

impl<'s> Browser<'s> {
    /// `scope` is the host's document: without drop rules, rows dragged in another scope never land.
    pub fn new(state: &'s mut BrowserState, style: Style, scope: u64) -> Self {
        Browser {
            state,
            style,
            scope,
            presentation: 0,
            accept: None,
            header: None,
            keyboard: false,
            shrink: true,
            scroll: true,
            active: true,
            per_row: false,
        }
    }

    /// Which of the host's presentations of these rows this is (a drag's [`Source`]).
    pub fn presentation(mut self, presentation: u64) -> Self {
        self.presentation = presentation;
        self
    }

    /// Whether the presentation is shown. An inactive one draws nothing and emits nothing, except
    /// that an eye drag it holds ends ([`Gesture::End`]) and a row drag it began is cancelled.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// The rows in a scroll area (the default), or laid out in the host's own layout.
    pub fn scroll(mut self, scroll: bool) -> Self {
        self.scroll = scroll;
        self
    }

    /// Each row's events come back in its [`RowOut`] as it is drawn, not in [`Output::events`]
    /// (which keeps the panel's own: the eye gesture, the keys).
    pub fn events_per_row(mut self, on: bool) -> Self {
        self.per_row = on;
        self
    }

    /// The host's drop rules; without them a row taking drops takes any drag of its scope.
    pub fn accept(mut self, rules: Accept<'s>) -> Self {
        self.accept = Some(rules);
        self
    }

    pub fn header(mut self, header: Header<'s>) -> Self {
        self.header = Some(header);
        self
    }

    /// Arrow keys move the selection, Left and Right fold, Enter opens and F2 renames, while the
    /// panel has the keyboard.
    pub fn keyboard(mut self, on: bool) -> Self {
        self.keyboard = on;
        self
    }

    /// The rows' area hugs its rows (the default), or fills the height it is given.
    pub fn shrink(mut self, on: bool) -> Self {
        self.shrink = on;
        self
    }

    /// Draw the panel in `ui`. Its widget ids (rows, scroll offset, editor) come from `ui`'s scope
    /// id, stable from frame to frame: a host showing two presentations in one scope gives each its
    /// own (`ui.push_id`).
    pub fn show(self, ui: &mut egui::Ui, rows: impl FnOnce(&mut Rows<'_>)) -> Output {
        let Browser { state, style, scope, presentation, accept, header, keyboard, shrink, scroll, active, per_row } = self;
        let source = Source { scope, presentation };
        if !active {
            return inactive(ui, state, source);
        }
        let base = ui.scope_id().with("sc_browser");
        let hint = header.and_then(|h| h.search);
        if hint.is_none() {
            state.search_open = false;
        }
        if let Some(header) = &header {
            header_band(ui, header, state);
        }
        if state.search_open
            && let Some(hint) = hint
        {
            let _ = crate::search_field(ui, base.with("search"), &mut state.query, hint);
        }
        if state.collapsed && header.is_some_and(|h| h.collapsible) {
            return Output::default();
        }
        let query = (state.search_open && !state.query.trim().is_empty()).then(|| state.query.trim().to_lowercase());
        let searching = query.is_some();
        if !searching {
            state.shown = None;
        }
        if !state.order.is_empty() {
            state.previous = std::mem::take(&mut state.order);
        }
        let mut events = if keyboard { keys(ui, state) } else { Vec::new() };
        let eye_before = ui.data(|d| d.get_temp::<bool>(eye_drag_id())).is_some();
        let eye_foreign = eye_before && !state.eye_gesture;
        let (now, mods, pressed) = ui.input(|i| (i.time, i.modifiers, i.pointer.any_pressed()));
        let delay = ui.ctx().options(|o| o.input_options.max_double_click_delay);
        let frame_nr = ui.ctx().cumulative_frame_nr();
        let body = |ui: &mut egui::Ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut list = Rows {
                ui,
                state: &mut *state,
                accept,
                base,
                source,
                style,
                query,
                keyboard,
                per_row,
                now,
                mods,
                pressed,
                delay,
                frame_nr,
                eye_foreign,
                done: Done::default(),
            };
            rows(&mut list);
            list.done
        };
        let (done, panel, content_height) = if scroll {
            let area = egui::ScrollArea::vertical()
                .id_salt(base.with("rows"))
                .min_scrolled_height(0.0)
                .auto_shrink([false, shrink])
                // A drag on the rows drags rows: only the bar and the wheel scroll.
                .scroll_source(egui::scroll_area::ScrollSource { scroll_bar: true, mouse_wheel: true, ..egui::scroll_area::ScrollSource::NONE })
                .show(ui, body);
            (area.inner, area.inner_rect, area.content_size.y)
        } else {
            let laid = ui.scope(body);
            (laid.inner, laid.response.rect, laid.response.rect.height())
        };
        events.extend(done.events);
        let eye_after = ui.data(|d| d.get_temp::<bool>(eye_drag_id())).is_some();
        if eye_after && !eye_before {
            state.eye_gesture = true;
            events.insert(0, Event::EyeGesture(Gesture::Begin));
        }
        let down = ui.input(|i| i.pointer.primary_down());
        if state.eye_gesture && !down {
            ui.data_mut(|d| d.remove_temp::<bool>(eye_drag_id()));
            state.eye_gesture = false;
            events.push(Event::EyeGesture(Gesture::End));
        }
        if !down {
            state.drag_widget = None;
        }
        if let Some((id, class, selected)) = done.drag_start {
            let (ids, data) = done.drag_set.unwrap_or_else(|| {
                let ids = if selected { done.selected.iter().filter(|(_, c)| *c == Some(class)).map(|(i, _)| *i).collect() } else { vec![id] };
                (ids, None)
            });
            egui::DragAndDrop::set_payload(ui.ctx(), Dragged { source, class, ids, data });
        }
        if searching {
            let shown = search_shown(&done.matches);
            if state.shown.as_ref() != Some(&shown) {
                state.shown = Some(shown);
                ui.ctx().request_repaint();
            }
        }
        if keyboard {
            state.keys = done.keys;
        }
        if done.clicked_row {
            state.focused = true;
        } else if pressed && !ui.input(|i| i.pointer.press_origin()).is_some_and(|p| panel.contains(p)) {
            state.focused = false;
        }
        Output { events, hovered: done.hovered, drawn: done.drawn, pointer_owned: done.pointer_owned, content_height }
    }
}

/// A hidden presentation: it draws nothing and emits nothing, and the gestures it holds end.
fn inactive(ui: &mut egui::Ui, state: &mut BrowserState, source: Source) -> Output {
    Output { events: end_gestures(ui.ctx(), state, source), ..Default::default() }
}

/// End what a presentation holds, for one that is hidden or not drawn at all this frame: an eye
/// drag ends ([`Gesture::End`], the only event), a row drag it began is cancelled and its widget
/// released, and a held slow click or rename request is dropped.
pub fn end_gestures(ctx: &egui::Context, state: &mut BrowserState, source: Source) -> Vec<Event> {
    let mut events = Vec::new();
    if state.eye_gesture {
        ctx.data_mut(|d| d.remove_temp::<bool>(eye_drag_id()));
        state.eye_gesture = false;
        events.push(Event::EyeGesture(Gesture::End));
    }
    if egui::DragAndDrop::payload::<Dragged>(ctx).is_some_and(|d| d.source == source) {
        egui::DragAndDrop::clear_payload(ctx);
    }
    if let Some(widget) = state.drag_widget.take()
        && ctx.dragged_id() == Some(widget)
    {
        ctx.stop_dragging();
    }
    state.pending = None;
    state.rename_request = None;
    events
}

/// What the rows gathered this frame.
#[derive(Default)]
struct Done {
    events: Vec<Event>,
    drawn: Vec<Drawn>,
    hovered: Option<NodeId>,
    pointer_owned: bool,
    /// Selected rows and the kind each drags as.
    selected: Vec<(NodeId, Option<u32>)>,
    /// A row drag began: row, kind, whether the row is selected.
    drag_start: Option<(NodeId, u32, bool)>,
    /// The set and data the host gave for it.
    drag_set: Option<(Vec<NodeId>, Option<Payload>)>,
    /// While searching: (row, depth, matches).
    matches: Vec<(NodeId, usize, bool)>,
    keys: Vec<KeyRow>,
    clicked_row: bool,
}

/// The host's side of a frame: it emits its rows here, in order.
pub struct Rows<'r> {
    ui: &'r mut egui::Ui,
    state: &'r mut BrowserState,
    accept: Option<Accept<'r>>,
    base: egui::Id,
    source: Source,
    style: Style,
    query: Option<String>,
    keyboard: bool,
    per_row: bool,
    now: f64,
    mods: Modifiers,
    pressed: bool,
    delay: f64,
    frame_nr: u64,
    /// An eye drag that began in another presentation: this one's eyes leave it alone.
    eye_foreign: bool,
    done: Done,
}

impl Rows<'_> {
    /// The query while searching: emit the rows of closed branches too, the panel keeps the
    /// matches and the rows above them.
    pub fn searching(&self) -> Option<&str> {
        self.query.as_deref()
    }

    /// Whether the next row would be on screen: a host may [`Rows::skip`] one that is not.
    pub fn next_visible(&self) -> bool {
        self.ui.is_rect_visible(tree_row_rect(&*self.ui, &Row { compact: self.style.compact, ..Default::default() }))
    }

    /// Take an off-screen row's place without drawing it. Ignored while searching.
    pub fn skip(&mut self, id: NodeId, select: Selectable) {
        if self.query.is_some() {
            return;
        }
        if let Selectable::Yes(class) = select {
            self.state.order.push((id, class));
        }
        let rect = tree_row_rect(&*self.ui, &Row { compact: self.style.compact, ..Default::default() });
        self.ui.allocate_space(rect.size());
        self.scroll_requests(id, rect);
    }

    /// The rows and data a drag that began on the row just drawn carries ([`RowOut::drag_started`]),
    /// instead of the selected rows of its kind as drawn: a host's set may hold items no row
    /// draws (the selected items in a closed group).
    pub fn drag_with(&mut self, ids: Vec<NodeId>, data: Option<Payload>) {
        if self.done.drag_start.is_some() {
            self.done.drag_set = Some((ids, data));
        }
    }

    /// A row the host lays out itself out of view (it takes the row's place): kept in the rows'
    /// order, so a Shift range across it is whole. Ignored while searching.
    pub fn pass(&mut self, id: NodeId, select: Selectable) {
        if self.query.is_none()
            && let Selectable::Yes(class) = select
        {
            self.state.order.push((id, class));
        }
    }

    /// The UI the rows are drawn in, for a host's own lines between rows.
    pub fn ui(&mut self) -> &mut egui::Ui {
        &mut *self.ui
    }

    /// Draw one row.
    pub fn row(&mut self, node: Node<'_>) -> RowOut {
        let Node { id, key, row, name, select, rename, drag, drops, menu, click_folds, accept } = node;
        let (depth, label, secondary, fold, eye, selected, actions) =
            (row.depth, row.label, row.secondary, row.fold, row.eye, row.selected, row.actions);
        let handle = row.handle.unwrap_or(label);
        let name = name.unwrap_or(label);
        let first_event = self.done.events.len();
        if let Selectable::Yes(class) = select {
            self.state.order.push((id, class));
        }
        if selected {
            self.done.selected.push((id, drag));
        }
        if self.keyboard {
            self.done.keys.push(KeyRow { id, select, fold, rename, name: name.to_owned() });
        }
        if let Some(query) = self.query.as_deref() {
            let matches = label.to_lowercase().contains(query) || secondary.to_lowercase().contains(query);
            self.done.matches.push((id, depth, matches));
            if self.state.shown.as_ref().is_some_and(|shown| !shown.contains(&id)) {
                return RowOut::default();
            }
        }
        // While searching every branch with a match is shown open.
        let open = if self.query.is_some() { fold.map(|_| true) } else { fold };
        let widget = self.base.with(id);
        let mut out = tree_row(self.ui, widget, &Row { compact: self.style.compact, eye_left: self.style.eye_left, fold: open, ..row });
        let visible = out.resp.is_some();
        if let Some(rect) = out.rect {
            if visible {
                let fold_rect = out.fold_at.map(|at| {
                    Rect::from_center_size(pos2(at.x, rect.center().y), vec2(Tokens::get().space_16 + Tokens::get().space_3, rect.height()))
                });
                self.done.drawn.push(Drawn { id, key: key.to_owned(), label: label.to_owned(), rect, fold: fold_rect, open });
                self.done.pointer_owned |= out.hovered || out.resp.as_ref().is_some_and(egui::Response::contains_pointer);
            }
            self.scroll_requests(id, rect);
        }
        if out.hovered && out.resp.as_ref().is_some_and(egui::Response::contains_pointer) {
            self.done.hovered = Some(id);
        }
        self.slow_rename(id, rename, name, &mut out);
        if self.state.rename_request == Some(id) {
            self.state.rename_request = None;
            self.rename(id, rename, name);
        }
        let editing = self.state.editor.as_ref().is_some_and(|e| e.id == id);
        if self.claim(id, &out) {
            if out.fold {
                self.done.events.push(Event::Fold { id, open: !open.unwrap_or(false) });
            }
            if out.eye && !self.eye_foreign {
                self.done.events.push(Event::Eye { id, visible: !eye.unwrap_or(true) });
            }
            if out.radio {
                self.done.events.push(Event::Activate(id));
            }
            if out.action {
                self.done.events.push(Event::NameAction(id));
            }
            for (response, action) in out.actions.iter().zip(actions) {
                if response.clicked() {
                    self.done.events.push(Event::Action { id, action: action.target.to_owned(), params: None });
                }
            }
            if out.double {
                self.done.events.push(Event::Open(id));
            } else if out.clicked && !editing {
                self.click(id, select, click_folds, open);
            }
            if let (Some(at), NodeMenu::Host) = (out.secondary, menu) {
                self.done.events.push(Event::Menu { id, at });
            }
        }
        if let (NodeMenu::Items(items), Some(response)) = (menu, out.resp.as_ref()) {
            let mut chosen = None;
            Menu::context(response, |ui| {
                for item in items() {
                    if item.is_sep() {
                        Menu::separator(ui);
                        continue;
                    }
                    if item.is_heading() {
                        Menu::heading(ui, &item.label);
                        continue;
                    }
                    let entry = MenuItem::from_item(&item, false).show(ui);
                    solidcraft_ui_registry::publish_handle(&format!("browser:menu:{}", item.id), entry.rect.center());
                    if entry.clicked() && item.enabled {
                        chosen = Some((item.id.clone(), item.params.clone()));
                        ui.close();
                    }
                }
            });
            if let Some((action, params)) = chosen {
                self.done.events.push(Event::Action { id, action, params });
            }
        }
        let mut drag_started = false;
        let mut drop_zone = None;
        if let Some(response) = out.resp.clone() {
            if response.drag_started() {
                // A row drag or an eye drag: this presentation holds the widget until release.
                self.state.drag_widget = Some(widget);
            }
            if let Some(class) = drag
                && response.drag_started()
                && !out.on_control
                && !editing
            {
                self.done.drag_start = Some((id, class, selected));
                drag_started = true;
            }
            if drops.before || drops.into {
                drop_zone = self.drop_target(id, &response, drops, accept);
            }
        }
        if editing {
            self.editor(id, &out, handle);
        }
        let events = if self.per_row { self.done.events.split_off(first_event.min(self.done.events.len())) } else { Vec::new() };
        RowOut {
            rect: out.rect,
            label_rect: out.label_rect,
            widget: out.row_id,
            hovered: out.hovered,
            clicked: out.clicked && !out.double,
            double: out.double,
            secondary: out.secondary,
            drag_started,
            drop_zone,
            fold_at: out.fold_at,
            eye_at: out.eye_at,
            eye_rect: out.eye_rect,
            radio_at: out.radio_at,
            response: out.resp.clone(),
            on_control: out.on_control,
            events,
        }
    }

    /// Rename row `id` its way: the panel's editor, or a request to the host.
    fn rename(&mut self, id: NodeId, rename: Rename, name: &str) {
        match rename {
            Rename::Never => {}
            Rename::Inline => self.state.editor = Some(Editor::new(id, name)),
            Rename::Host => self.done.events.push(Event::RenameRequested(id)),
        }
    }

    /// A row's actions run once a frame, however often egui lays the frame out.
    fn claim(&mut self, id: NodeId, out: &RowResp) -> bool {
        let acts = out.clicked
            || out.double
            || out.secondary.is_some()
            || out.fold
            || out.eye
            || out.radio
            || out.action
            || out.actions.iter().any(egui::Response::clicked);
        if !acts {
            return false;
        }
        let (frame, claimed) = &mut self.state.claims;
        if *frame != self.frame_nr {
            *frame = self.frame_nr;
            claimed.clear();
        }
        claimed.insert(id)
    }

    /// Reveal and keyboard scrolling, when the row they ask for is laid out.
    fn scroll_requests(&mut self, id: NodeId, rect: Rect) {
        if self.state.reveal == Some(id) {
            self.ui.scroll_to_rect(rect, Some(egui::Align::Center));
            if self.ui.is_rect_visible(rect) && self.ui.clip_rect().contains(rect.center()) {
                self.state.reveal = None;
                self.done.events.push(Event::Revealed(id));
            }
        }
        if self.state.scroll_to == Some(id) {
            self.ui.scroll_to_rect(rect, None);
            self.state.scroll_to = None;
        }
    }

    /// A slower second click on a renamable row's name opens the editor once the double-click delay
    /// has passed with no further click: the click is held back until then, so the first click of a
    /// double-click never renames. A double-click, or a press anywhere else, drops it.
    fn slow_rename(&mut self, id: NodeId, rename: Rename, name: &str, out: &mut RowResp) {
        let now = self.now;
        if !now.is_finite() {
            return;
        }
        if let Some((row, due, made)) = self.state.pending {
            let stale = !(due.is_finite() && made.is_finite()) || now > due + SLOW_RENAME_WINDOW;
            if stale || (self.pressed && now > made) || (row == id && out.double) {
                self.state.pending = None;
            } else if row == id {
                if now >= due {
                    self.state.pending = None;
                    self.rename(id, rename, name);
                    return;
                }
                self.ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64((due - now).clamp(0.0, SLOW_RENAME_WINDOW)));
            }
        }
        if !out.resp.as_ref().is_some_and(egui::Response::clicked) {
            return;
        }
        let plain = !(self.mods.command || self.mods.ctrl || self.mods.shift);
        let name_click = rename != Rename::Never && out.clicked && !out.double && plain && self.state.editor.is_none();
        let previous = self.state.last_click.replace((id, now, name_click));
        let slow = name_click
            && previous.is_some_and(|(row, time, was_name)| {
                row == id && was_name && time.is_finite() && now - time > self.delay && now - time <= SLOW_RENAME_WINDOW
            });
        if slow {
            self.state.pending = Some((id, now + self.delay, now));
            self.ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(self.delay.clamp(0.0, SLOW_RENAME_WINDOW)));
            out.clicked = false;
        }
    }

    fn click(&mut self, id: NodeId, select: Selectable, click_folds: bool, open: Option<bool>) {
        self.done.clicked_row = true;
        match select {
            Selectable::No => self.done.events.push(Event::Click(id)),
            Selectable::Yes(class) => {
                let toggle = self.mods.command || self.mods.ctrl;
                let range = self
                    .state
                    .anchor
                    .filter(|_| self.mods.shift)
                    .and_then(|anchor| span(&self.state.previous, anchor, id, class).or_else(|| span(&self.state.order, anchor, id, class)));
                let how = match range {
                    Some(range) if toggle => How::RangeAdd(range),
                    Some(range) => How::Range(range),
                    None => {
                        self.state.anchor = Some(id);
                        if toggle { How::Toggle } else { How::Only }
                    }
                };
                self.state.cursor = Some(id);
                self.done.events.push(Event::Select { id, how });
            }
        }
        if click_folds && let Some(open) = open {
            self.done.events.push(Event::Fold { id, open: !open });
        }
    }

    /// While rows of this scope are dragged over the row, mark where they would land; on release,
    /// report the move. Drags of other kinds of payload are left alone.
    fn drop_target(&mut self, id: NodeId, response: &egui::Response, drops: Drops, own: Option<&dyn Fn(&Dragged, Zone) -> bool>) -> Option<Zone> {
        let dragged = egui::DragAndDrop::payload::<Dragged>(self.ui.ctx())?;
        if dragged.ids.contains(&id) || !response.contains_pointer() {
            return None;
        }
        let r = response.rect;
        let at = self.ui.input(|i| i.pointer.hover_pos().or(i.pointer.latest_pos()));
        let pointer = if at.is_some_and(|p| p.y < r.top() + r.height() * BEFORE_ZONE) { Zone::Before } else { Zone::Into };
        // The zones the drop may take, first choice first: a row taking one takes it anywhere; a
        // row taking both takes a drop before it in its top part, or inside it there when the
        // rules refuse before, and inside it below that. A drop over the row's middle never lands
        // before it.
        let zones: &[Zone] = match (drops.before, drops.into, pointer) {
            (true, true, Zone::Before) => &[Zone::Before, Zone::Into],
            (true, true, Zone::Into) | (false, true, _) => &[Zone::Into],
            (true, false, _) => &[Zone::Before],
            (false, false, _) => &[],
        };
        let accepts = |zone: Zone| match (own, self.accept) {
            (Some(rule), _) => rule(&dragged, zone),
            (None, Some(rule)) => rule(&dragged, id, zone),
            (None, None) => dragged.source.scope == self.source.scope,
        };
        let zone = zones.iter().copied().find(|zone| accepts(*zone))?;
        let t = Tokens::get();
        match zone {
            Zone::Before => {
                self.ui.painter().hline(r.x_range(), r.top(), Stroke::new(2.0, t.accent));
            }
            Zone::Into => {
                self.ui.painter().rect_stroke(r.shrink(0.5), t.radius_sm.saturating_sub(1), Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
            }
        }
        if self.ui.input(|i| i.pointer.any_released()) {
            egui::DragAndDrop::clear_payload(self.ui.ctx());
            self.done.events.push(Event::Move { ids: dragged.ids.clone(), target: id, zone, source: dragged.source, data: dragged.data.clone() });
        }
        Some(zone)
    }

    /// The name editor in its row: Enter or leaving it keeps the name, Escape cancels.
    fn editor(&mut self, id: NodeId, out: &RowResp, handle: &str) {
        let (Some(rect), Some(row)) = (out.label_rect, out.row_id) else { return };
        let Some(mut editor) = self.state.editor.take() else { return };
        self.ui.painter().rect_filled(rect, Tokens::get().radius_sm, Tokens::get().field);
        let field = row.with("browser_rename");
        let response = crate::inline_name(self.ui, rect, field, &mut editor.text);
        solidcraft_ui_registry::publish_handle(&format!("rename:{handle}"), rect.center());
        let was_focused = editor.focused;
        if !editor.focused {
            if let Some(mut state) = egui::TextEdit::load_state(self.ui.ctx(), field) {
                let end = editor.text.chars().count();
                state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(end))));
                state.store(self.ui.ctx(), field);
            }
            response.request_focus();
            editor.focused = true;
        }
        let response = match &editor.error {
            Some(error) => response.backing_on_hover_text(error.clone()),
            None => response,
        };
        let escape = self.ui.input(|i| i.key_pressed(Key::Escape));
        let commit = self.ui.input(|i| i.key_pressed(Key::Enter)) || (was_focused && response.lost_focus());
        if escape {
            // Dropping the editor cancels it.
        } else if commit {
            self.done.events.push(Event::Rename { id, text: editor.text });
        } else {
            self.state.editor = Some(editor);
        }
    }
}

/// The rows of `class` from `anchor` to `id` (either way round) in `order`, or `None` when the
/// order lacks either.
fn span(order: &[(NodeId, u32)], anchor: NodeId, id: NodeId, class: u32) -> Option<Vec<NodeId>> {
    let rows: Vec<NodeId> = order.iter().filter(|(_, c)| *c == class).map(|(row, _)| *row).collect();
    let i = rows.iter().position(|row| *row == anchor)?;
    let j = rows.iter().position(|row| *row == id)?;
    rows.get(i.min(j)..=i.max(j)).map(<[NodeId]>::to_vec)
}

/// The rows a search shows: each match and every row above one. Walking back up the rows, a row is
/// above the nearest shown row after it when it is shallower.
fn search_shown(rows: &[(NodeId, usize, bool)]) -> HashSet<NodeId> {
    let mut shown = HashSet::new();
    let mut below: Option<usize> = None;
    for (id, depth, matches) in rows.iter().rev() {
        if *matches || below.is_some_and(|d| *depth < d) {
            shown.insert(*id);
            below = Some(*depth);
        }
    }
    shown
}

/// The arrow keys, Enter and F2 while the panel has the keyboard, over the last frame's rows.
fn keys(ui: &mut egui::Ui, state: &mut BrowserState) -> Vec<Event> {
    let mut out = Vec::new();
    if !state.focused || state.editor.is_some() {
        return out;
    }
    let mods = ui.input(|i| i.modifiers);
    let take = |ui: &mut egui::Ui, key: Key| ui.input_mut(|i| i.consume_key(mods, key));
    let selectable: Vec<(NodeId, u32)> = state
        .keys
        .iter()
        .filter_map(|r| match r.select {
            Selectable::Yes(class) => Some((r.id, class)),
            Selectable::No => None,
        })
        .collect();
    let step = if take(ui, Key::ArrowDown) {
        Some(true)
    } else if take(ui, Key::ArrowUp) {
        Some(false)
    } else {
        None
    };
    if let Some(down) = step
        && let Some(last) = selectable.len().checked_sub(1)
    {
        let at = state.cursor.and_then(|c| selectable.iter().position(|(id, _)| *id == c));
        let next = match (at, down) {
            (None, true) => 0,
            (None, false) => last,
            (Some(i), true) => (i + 1).min(last),
            (Some(i), false) => i.saturating_sub(1),
        };
        if let Some(&(id, class)) = selectable.get(next) {
            let range = state.anchor.filter(|_| mods.shift).and_then(|anchor| span(&selectable, anchor, id, class));
            let how = match range {
                Some(range) => How::Range(range),
                None => {
                    state.anchor = Some(id);
                    How::Only
                }
            };
            state.cursor = Some(id);
            state.scroll_to = Some(id);
            out.push(Event::Select { id, how });
        }
    }
    let cursor = state.cursor.and_then(|c| state.keys.iter().find(|r| r.id == c)).cloned();
    if let Some(row) = cursor {
        if take(ui, Key::ArrowLeft) && row.fold == Some(true) {
            out.push(Event::Fold { id: row.id, open: false });
        }
        if take(ui, Key::ArrowRight) && row.fold == Some(false) {
            out.push(Event::Fold { id: row.id, open: true });
        }
        if take(ui, Key::Enter) {
            out.push(Event::Open(row.id));
        }
        if take(ui, Key::F2) {
            match row.rename {
                Rename::Never => {}
                Rename::Inline => state.editor = Some(Editor::new(row.id, &row.name)),
                Rename::Host => out.push(Event::RenameRequested(row.id)),
            }
        }
    }
    out
}

/// The title band: the title, the count, the fold chevron (the band folds the card) and the
/// search action. Tokens: form_header_height, font_heading/body, text/dim, space_4/6/12/24.
fn header_band(ui: &mut egui::Ui, header: &Header<'_>, state: &mut BrowserState) {
    let t = Tokens::get();
    let (band, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.form_header_height), Sense::hover());
    let mut right = band.right() - t.space_6;
    if let Some(hint) = header.search {
        let button = Rect::from_center_size(pos2(right - t.space_12, band.center().y), vec2(t.space_24, t.space_24));
        let response = crate::quiet_icon_button_at(ui, button, "zoom", hint);
        solidcraft_ui_registry::publish_target("browser:search", button);
        solidcraft_ui_registry::publish_handle("browser:search", button.center());
        if response.clicked() {
            state.search_open = !state.search_open;
            if !state.search_open {
                state.query.clear();
            }
        }
        right = button.left() - t.space_4;
    }
    if header.collapsible {
        let fold = Rect::from_min_max(band.min, pos2(right.max(band.left()), band.bottom()));
        let response = ui.interact(fold, ui.scope_id().with("sc_browser_header"), Sense::click());
        if response.hovered() || response.has_focus() {
            crate::panel_header_hover(ui.painter(), fold);
        }
        crate::disclosure(ui.painter(), pos2(right - t.form_section_icon_size / 2.0, band.center().y), !state.collapsed, t.text_dim);
        right -= t.form_section_icon_size + t.space_6;
        solidcraft_ui_registry::publish_handle("browser:header", fold.center());
        let tip = if state.collapsed { solidcraft_ui_text::tr("ui.expand") } else { solidcraft_ui_text::tr("ui.collapse") };
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).backing_on_hover_text(tip).clicked() {
            state.collapsed = !state.collapsed;
        }
    }
    if !header.count.is_empty() {
        let galley = ui.painter().layout_no_wrap(header.count.to_owned(), egui::FontId::proportional(t.font_body), t.text_dim);
        let width = galley.size().x;
        ui.painter().galley(pos2(right - width, band.center().y - galley.size().y / 2.0), galley, t.text_dim);
        right -= width + t.space_6;
    }
    let title = Rect::from_min_max(pos2(band.left() + t.space_6, band.top()), pos2(right.max(band.left() + t.space_6), band.bottom()));
    crate::paint_text(ui, title, header.title, crate::type_face::semibold(t.font_heading), t.text, false);
}

#[cfg(test)]
#[path = "browser_tests.rs"]
mod tests;

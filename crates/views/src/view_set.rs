//! The views of one design: Model, Sheet, Render and Animation, shown as sub-tabs of the
//! design's tab. Pure data and small functions, no egui, so the strip, the commands, the menus
//! and the saved form all read the same [`ViewSet`]. A view change reaches the app through
//! `documents::apply_view`, the one place that knows what a kind means for the window.

use serde_json::{Value, json};
use solidcraft_viewport::ViewSpec;

use crate::tiles::{Edge, Placement, Preset, Rect, Tiles};

/// Most views one design keeps open (and most a saved set may hold).
pub const MAX_VIEWS: usize = 64;
/// Longest view name, in characters (what `tab.view.rename` takes too).
pub const MAX_NAME: usize = 64;
/// Longest subject (the id of the sheet a view shows), in bytes.
const MAX_SUBJECT: usize = 256;
/// Closed views kept for Reopen Closed View.
const CLOSED_CAP: usize = 16;
/// Ids beyond this are not exact in JSON numbers: a saved set holding one is refused.
const MAX_ID: u64 = 1 << 53;

/// What a view shows of its design.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ViewKind {
    Model,
    Sheet,
    Render,
    Animation,
}

impl ViewKind {
    /// Every kind, in the order the "+" menu lists them.
    pub const ALL: [ViewKind; 4] = [ViewKind::Model, ViewKind::Sheet, ViewKind::Render, ViewKind::Animation];

    /// This workspace can draw its contents in the frame of a tile.
    pub fn can_tile(self) -> bool {
        matches!(self, Self::Model | Self::Sheet | Self::Render)
    }

    /// The kind's stable key: its name in commands and in the saved form.
    pub fn key(self) -> &'static str {
        match self {
            ViewKind::Model => "model",
            ViewKind::Sheet => "sheet",
            ViewKind::Render => "render",
            ViewKind::Animation => "animation",
        }
    }

    /// The kind with `key` ([`Self::key`]).
    pub fn from_key(key: &str) -> Option<ViewKind> {
        Self::ALL.into_iter().find(|kind| kind.key() == key)
    }

    /// The English word a new view's default name starts from.
    fn label(self) -> &'static str {
        match self {
            ViewKind::Model => "Model",
            ViewKind::Sheet => "Sheet",
            ViewKind::Render => "Render",
            ViewKind::Animation => "Animation",
        }
    }
}

/// A Model or Sheet sketch being edited in one view. Runtime state, never saved with a layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SketchSession {
    pub sketch: u64,
}

/// One view of a design.
#[derive(Clone, Debug, PartialEq)]
pub struct DesignView {
    pub editing: Option<SketchSession>,
    pub compass: bool,
    /// Unique within its design for as long as the set lives; never reused.
    pub id: u64,
    pub kind: ViewKind,
    /// What the sub-tab says: the default ("Sheet 2") until renamed.
    pub name: String,
    /// The sheet this view shows (its id); none for Model, Render and Animation.
    pub subject: Option<String>,
    /// How this view looks at the model (its own camera). None until the view is first left: the
    /// front end then shows it from where the design looks now.
    pub spec: Option<ViewSpec>,
}

/// The maximum number of pop-outs per design, in addition to the main window.
pub const MAX_POPOUTS: usize = 8;

/// A design's window id. Zero names its main window.
pub type WindowId = u32;

/// Which application controls a window shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WindowLevel {
    #[default]
    ViewsOnly,
    Clean,
    Full,
}

impl WindowLevel {
    pub fn key(self) -> &'static str {
        match self {
            Self::ViewsOnly => "views_only",
            Self::Clean => "clean",
            Self::Full => "full",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        [Self::ViewsOnly, Self::Clean, Self::Full].into_iter().find(|level| level.key() == key)
    }
}

/// An arrangement of existing views in one window.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowArrangement {
    pub panel_layout: Option<Value>,
    pub options: super::WindowOptions,
    pub views: Vec<u64>,
    pub active: Option<u64>,
    pub tiles: Option<Tiles>,
    pub level: WindowLevel,
    pub placement: Option<WindowPlacement>,
}

/// Logical window coordinates relative to the selected display.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowPlacement {
    pub display: Option<String>,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub maximised: bool,
    pub fullscreen: bool,
}

impl WindowPlacement {
    fn is_valid(&self) -> bool {
        [self.x, self.y, self.w, self.h].into_iter().all(f32::is_finite) && self.w > 0.0 && self.h > 0.0
    }

    pub fn to_json(&self) -> Value {
        json!({"display": self.display, "x": self.x, "y": self.y, "w": self.w, "h": self.h, "maximised": self.maximised, "fullscreen": self.fullscreen})
    }

    fn from_json(value: &Value) -> Option<Self> {
        let number = |key| value.get(key)?.as_f64().map(|v| v as f32);
        let placement = Self {
            display: value.get("display").and_then(Value::as_str).filter(|name| name.len() <= MAX_SUBJECT).map(str::to_owned),
            x: number("x")?,
            y: number("y")?,
            w: number("w")?,
            h: number("h")?,
            maximised: value.get("maximised").and_then(Value::as_bool).unwrap_or(false),
            fullscreen: value.get("fullscreen").and_then(Value::as_bool).unwrap_or(false),
        };
        placement.is_valid().then_some(placement)
    }
}

/// Place a new pop-out in a display's work area. The source and cell sizes are captured at
/// lift. The work area and pointer use the same logical display coordinates.
pub fn window_rect_for(source: Rect, cell: Option<Rect>, work_area: Rect, pointer: Option<[f32; 2]>, snap: bool) -> Option<Rect> {
    let tab_size = |span: f32| (f64::from(span) * 0.6) as f32;
    let (width, height) = cell.map_or_else(|| (tab_size(source.w), tab_size(source.h)), |cell| (cell.w, cell.h));
    if ![width, height, work_area.w, work_area.h].into_iter().all(|size| size.is_finite() && size > 0.0)
        || ![work_area.x, work_area.y, work_area.x + work_area.w, work_area.y + work_area.h].into_iter().all(f32::is_finite)
        || pointer.is_some_and(|point| !point.into_iter().all(f32::is_finite))
    {
        return None;
    }
    let max_width = work_area.w * 0.9;
    let max_height = work_area.h * 0.9;
    let w = width.clamp(640.0_f32.min(max_width), max_width);
    let h = height.clamp(420.0_f32.min(max_height), max_height);
    let room_x = work_area.w - w;
    let room_y = work_area.h - h;
    let [x, y] = pointer.map_or([room_x * 0.5, room_y * 0.5], |[x, y]| {
        [anchored_window_axis(x - work_area.x - 24.0, room_x, snap), anchored_window_axis(y - work_area.y - 14.0, room_y, snap)]
    });
    Some(Rect { x: work_area.x + x, y: work_area.y + y, w, h })
}

fn anchored_window_axis(wanted: f32, room: f32, snap: bool) -> f32 {
    let position = wanted.clamp(0.0, room);
    if !snap {
        return position;
    }
    let far = room - position;
    if position <= 16.0 && position <= far {
        0.0
    } else if far <= 16.0 {
        room
    } else {
        position
    }
}

/// A pop-out holds its own strip, active view and layout for this design.
#[derive(Clone, Debug, PartialEq)]
pub struct PopOut {
    pub panel_layout: Option<Value>,
    pub options: super::WindowOptions,
    pub id: WindowId,
    pub level: WindowLevel,
    pub placement: Option<WindowPlacement>,
    pub views: ViewSet,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WindowTarget {
    New {
        placement: Option<WindowPlacement>,
    },
    /// Insert at the given strip index after removing the view, or append to another strip.
    Existing {
        window: WindowId,
        index: Option<usize>,
    },
    /// Transfer into an explicitly targeted cell in another window.
    Dock {
        window: WindowId,
        placement: Placement,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Moved {
    pub view: u64,
    pub window: WindowId,
    pub created: bool,
    pub from: WindowId,
    pub closed_source: bool,
    pub placement: Option<WindowPlacement>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveError {
    NoSuchView,
    NoSuchWindow,
    TooManyWindows,
    LastViewOfMainWindow,
    BadGeometry,
    BadPlacement,
}

impl MoveError {
    pub fn code(self) -> &'static str {
        match self {
            Self::NoSuchView => "no_such_view",
            Self::NoSuchWindow => "no_such_window",
            Self::TooManyWindows => "too_many_windows",
            Self::LastViewOfMainWindow => "last_view_of_main_window",
            Self::BadGeometry => "bad_geometry",
            Self::BadPlacement => "bad_placement",
        }
    }
}
/// Camera components shared by the visible Model views in a window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraLinkMode {
    #[default]
    Off,
    Orientation,
    Zoom,
    Both,
}

/// The views of a design in tab order, the active one, and the views closed lately.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewSet {
    views: Vec<DesignView>,
    active: u64,
    /// Closed views with the place each had, the latest last.
    closed: Vec<(usize, DesignView)>,
    next_id: u64,
    window_id: WindowId,
    window_name: Option<String>,
    windows: Vec<PopOut>,
    next_window: WindowId,
    /// Open views belonging to another window of the same design.
    elsewhere: usize,
    /// How the window lays its views out side by side. Dormant while the active view is outside it.
    tiles: Option<Tiles>,
    camera_link: CameraLinkMode,
}

impl Default for ViewSet {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewSet {
    /// A design's first views: one Model view, active.
    pub fn new() -> Self {
        let model = DesignView {
            editing: None,
            id: 1,
            kind: ViewKind::Model,
            name: ViewKind::Model.label().into(),
            subject: None,
            spec: None,
            compass: true,
        };
        Self::strip(vec![model], 1, 2, 0)
    }

    fn strip(views: Vec<DesignView>, active: u64, next_id: u64, window_id: WindowId) -> Self {
        Self {
            views,
            active,
            closed: Vec::new(),
            next_id,
            window_id,
            window_name: None,
            windows: Vec::new(),
            next_window: 1,
            elsewhere: 0,
            tiles: None,
            camera_link: CameraLinkMode::Off,
        }
    }

    /// Arrange the live views in their new strips, reusing existing window identities in order.
    /// The input must assign each open view exactly once and leave at least one in the main window.
    pub fn arrange_windows(&mut self, groups: Vec<WindowArrangement>) -> bool {
        if self.window_id != 0 || groups.is_empty() || groups.len() > MAX_POPOUTS + 1 {
            return false;
        }
        let all = std::iter::once(&*self).chain(self.windows.iter().map(|window| &window.views));
        let mut unassigned: std::collections::BTreeMap<_, _> = all.flat_map(|strip| strip.views.iter().map(|view| (view.id, view.clone()))).collect();
        let mut assigned = Vec::new();
        for group in groups {
            if group.views.is_empty() || group.placement.as_ref().is_some_and(|placement| !placement.is_valid()) {
                return false;
            }
            let views = group.views.iter().map(|id| unassigned.remove(id)).collect::<Option<Vec<_>>>();
            let Some(views) = views else { return false };
            assigned.push((group, views));
        }
        if !unassigned.is_empty() {
            return false;
        }
        let mut assigned = assigned.into_iter();
        let Some((main, views)) = assigned.next() else { return false };
        self.apply_arrangement(main, views);
        let mut old = std::mem::take(&mut self.windows).into_iter();
        for (group, views) in assigned {
            let mut window = old.next().unwrap_or_else(|| {
                let id = self.allocate_window_id();
                PopOut {
                    panel_layout: None,
                    options: super::WindowOptions::default(),
                    id,
                    level: WindowLevel::ViewsOnly,
                    placement: None,
                    views: Self::strip(Vec::new(), 0, self.next_id, id),
                }
            });
            window.level = group.level;
            window.options = group.options;
            window.panel_layout.clone_from(&group.panel_layout);
            if let Some(placement) = &group.placement {
                window.placement = Some(placement.clone());
            }
            window.views.apply_arrangement(group, views);
            self.put_window(window);
        }
        for window in old {
            self.closed.extend(window.views.closed);
        }
        let excess = self.closed.len().saturating_sub(CLOSED_CAP);
        self.closed.drain(..excess);
        true
    }

    fn apply_arrangement(&mut self, group: WindowArrangement, views: Vec<DesignView>) {
        self.views = views;
        let requested = group.active.unwrap_or(self.active);
        if self.get(requested).is_some() {
            self.active = requested;
        } else if let Some(first) = self.views.first() {
            self.active = first.id;
        }
        self.tiles = group.tiles;
        self.prune_tiles();
    }

    /// The open pop-outs in this design. The main strip is this ViewSet itself.
    pub fn windows(&self) -> &[PopOut] {
        &self.windows
    }

    pub fn window(&self, id: WindowId) -> Option<&PopOut> {
        self.windows.iter().find(|window| window.id == id)
    }

    pub fn window_mut(&mut self, id: WindowId) -> Option<&mut PopOut> {
        let next_id = self.allocation_watermark();
        let window = self.windows.iter_mut().find(|window| window.id == id)?;
        // A caller may allocate directly through the mutable strip, without a take/put cycle.
        window.views.next_id = window.views.next_id.max(next_id);
        Some(window)
    }

    pub fn window_name(&self) -> Option<&str> {
        self.window_name.as_deref()
    }

    pub fn rename_window(&mut self, name: &str) -> bool {
        let Some(name) = valid_name(name) else { return false };
        if self.window_id == 0 {
            return false;
        }
        self.window_name = Some(name.to_owned());
        true
    }

    pub fn views_of(&self, id: WindowId) -> Option<&ViewSet> {
        if id == self.window_id { Some(self) } else { self.window(id).map(|window| &window.views) }
    }

    pub fn window_of(&self, view: u64) -> Option<WindowId> {
        if self.get(view).is_some() {
            return Some(self.window_id);
        }
        self.windows.iter().find(|window| window.views.get(view).is_some()).map(|window| window.id)
    }

    /// Move an open view intact without changing its camera or the other windows' layouts.
    pub fn move_to_window(&mut self, view: u64, target: WindowTarget) -> Result<Moved, MoveError> {
        let from = self.window_of(view).ok_or(MoveError::NoSuchView)?;
        if self.window_id != 0 {
            return Err(MoveError::NoSuchWindow);
        }
        let mut docked = None;
        let (window, index, created, placement) = match target {
            WindowTarget::New { placement } => {
                if from == 0 && self.len() == 1 {
                    return Err(MoveError::LastViewOfMainWindow);
                }
                if self.windows.len() >= MAX_POPOUTS {
                    return Err(MoveError::TooManyWindows);
                }
                if placement.as_ref().is_some_and(|placement| !placement.is_valid()) {
                    return Err(MoveError::BadGeometry);
                }
                (self.allocate_window_id(), None, true, placement)
            }
            WindowTarget::Dock { window, placement } => {
                let destination = self.views_of(window).ok_or(MoveError::NoSuchWindow)?;
                if from == window {
                    return Err(MoveError::BadPlacement);
                }
                if from == 0 && self.len() == 1 {
                    return Err(MoveError::LastViewOfMainWindow);
                }
                let kind = self.views_of(from).and_then(|strip| strip.get(view)).ok_or(MoveError::NoSuchView)?.kind;
                let tiles = destination.preview_incoming_placement(view, kind, placement).ok_or(MoveError::BadPlacement)?;
                let active = if tiles.contains(destination.active) { destination.active } else { view };
                docked = Some((tiles, active));
                (window, None, false, self.window(window).and_then(|child| child.placement.clone()))
            }
            WindowTarget::Existing { window, index } => {
                if self.views_of(window).is_none() {
                    return Err(MoveError::NoSuchWindow);
                }
                if from == 0 && window != 0 && self.len() == 1 {
                    return Err(MoveError::LastViewOfMainWindow);
                }
                (window, index, false, self.window(window).and_then(|child| child.placement.clone()))
            }
        };
        let mut result = Moved { view, window, created, from, closed_source: false, placement };
        if from == window {
            let strip = self.strip_mut(window).ok_or(MoveError::NoSuchWindow)?;
            if let Some(index) = index
                && let Some(at) = strip.index_of(view)
            {
                strip.move_view(at, index.min(strip.len() - 1));
            }
            return Ok(result);
        }

        let moved = if from == 0 {
            self.remove_moved(view).ok_or(MoveError::NoSuchView)?
        } else {
            let mut source = self.take_window(from).ok_or(MoveError::NoSuchWindow)?;
            let moved = source.views.remove_moved(view);
            if source.views.views.is_empty() {
                self.reattach(source.views);
                result.closed_source = true;
            } else {
                self.put_window(source);
            }
            moved.ok_or(MoveError::NoSuchView)?
        };
        if created {
            let views = Self::strip(vec![moved], view, self.next_id, window);
            self.put_window(PopOut {
                panel_layout: None,
                options: super::WindowOptions::default(),
                id: window,
                level: WindowLevel::ViewsOnly,
                placement: result.placement.clone(),
                views,
            });
        } else {
            let strip = self.strip_mut(window).ok_or(MoveError::NoSuchWindow)?;
            let at = index.unwrap_or(strip.len()).min(strip.len());
            strip.views.insert(at, moved);
            if let Some((tiles, active)) = docked {
                strip.active = active;
                strip.tiles = (tiles.len() > 1).then_some(tiles);
            } else {
                strip.fill_empty(0, view);
                strip.active = view;
            }
        }
        Ok(result)
    }

    fn strip_mut(&mut self, id: WindowId) -> Option<&mut Self> {
        if id == self.window_id { Some(self) } else { self.window_mut(id).map(|window| &mut window.views) }
    }

    fn allocate_window_id(&mut self) -> WindowId {
        let advance = |id: WindowId| id.checked_add(1).unwrap_or(1);
        let mut id = self.next_window.max(1);
        while self.window(id).is_some() {
            id = advance(id);
        }
        self.next_window = advance(id);
        id
    }

    fn remove_moved(&mut self, view: u64) -> Option<DesignView> {
        let at = self.index_of(view)?;
        let moved = self.views.remove(at);
        if self.active == view
            && let Some(neighbour) = self.views.get(at.saturating_sub(1))
        {
            self.active = neighbour.id;
        }
        self.prune_tiles();
        Some(moved)
    }

    /// Record usable logical geometry for a pop-out.
    pub fn set_placement(&mut self, id: WindowId, placement: WindowPlacement) -> bool {
        if !placement.is_valid() {
            return false;
        }
        let Some(window) = self.window_mut(id) else { return false };
        window.placement = Some(placement);
        true
    }

    /// Temporarily borrow the window's strip for the UI, sharing the design id sequence and cap.
    pub fn take_window(&mut self, id: WindowId) -> Option<PopOut> {
        let at = self.windows.iter().position(|window| window.id == id)?;
        let mut window = self.windows.remove(at);
        let elsewhere = self.len() + self.windows.iter().map(|other| other.views.len()).sum::<usize>();
        window.views.share_window_limits(self.next_id, elsewhere);
        Some(window)
    }

    /// Return the window strip after its UI frame.
    pub fn put_window(&mut self, window: PopOut) {
        self.next_id = self.next_id.max(window.views.next_id);
        let at = self.windows.partition_point(|other| other.id < window.id);
        self.windows.insert(at, window);
    }

    /// Close a pop-out and append its open views to the main strip in their existing order.
    pub fn close_window(&mut self, id: WindowId) -> Option<Vec<u64>> {
        let window = self.take_window(id)?;
        let returned: Vec<_> = window.views.views().iter().map(|view| view.id).collect();
        self.reattach(window.views);
        if let Some(first) = returned.first() {
            self.fill_empty(0, *first);
        }
        Some(returned)
    }

    /// Undo a pop-out close. Reuse each current view so newer edits and camera changes survive.
    pub fn restore_window(&mut self, mut window: PopOut) -> bool {
        if self.window_id != 0 || window.id == 0 || self.windows.len() >= MAX_POPOUTS || self.window(window.id).is_some() {
            return false;
        }
        let ids: Vec<_> = window.views.views.iter().map(|view| view.id).collect();
        if ids.is_empty() || ids.len() >= self.views.len() || ids.iter().any(|id| self.get(*id).is_none()) {
            return false;
        }
        window.views.views = ids.iter().filter_map(|id| self.get(*id).cloned()).collect();
        self.views.retain(|view| !ids.contains(&view.id));
        if self.get(self.active).is_none()
            && let Some(first) = self.views.first()
        {
            self.active = first.id;
        }
        self.prune_tiles();
        // Bring back only still-closed views, retaining their latest state and avoiding copies.
        let old_closed: Vec<_> = window.views.closed.iter().map(|(_, view)| view.id).collect();
        window.views.closed.clear();
        self.closed.retain(|entry| {
            if old_closed.contains(&entry.1.id) {
                window.views.closed.push(entry.clone());
                false
            } else {
                true
            }
        });
        window.views.prune_tiles();
        self.put_window(window);
        true
    }

    /// Close a view. When it was the only view of a pop-out, the window closes too and its
    /// closed-view history can be reopened on the main strip.
    pub fn close_in_window(&mut self, window: WindowId, view: u64) -> bool {
        if window == self.window_id {
            return self.close(view);
        }
        let Some(mut child) = self.take_window(window) else { return false };
        if child.views.len() == 1 && child.views.get(view).is_some() {
            child.views.remove_closed(0);
            self.reattach(child.views);
            return true;
        }
        let closed = child.views.close(view);
        self.put_window(child);
        closed
    }

    pub fn reopen_in_window(&mut self, window: WindowId) -> Option<u64> {
        if window == self.window_id {
            return self.reopen();
        }
        let mut child = self.take_window(window)?;
        let reopened = child.views.reopen();
        self.put_window(child);
        reopened
    }

    /// The ownership, active views and tiled cells all agree.
    pub fn check(&self) -> bool {
        let strip_valid = |strip: &Self| {
            strip.get(strip.active).is_some() && strip.tiles.as_ref().is_none_or(|tiles| tiles.views().into_iter().all(|id| strip.get(id).is_some()))
        };
        let mut ids: Vec<_> = self.ids().collect();
        ids.sort_unstable();
        let mut windows: Vec<_> = self.windows.iter().map(|window| window.id).collect();
        windows.sort_unstable();
        strip_valid(self)
            && self.len() + self.windows.iter().map(|window| window.views.len()).sum::<usize>() <= MAX_VIEWS
            && windows.len() <= MAX_POPOUTS
            && windows.windows(2).all(|pair| pair[0] != pair[1])
            && ids.windows(2).all(|pair| pair[0] != pair[1])
            && self
                .windows
                .iter()
                .all(|window| window.id != 0 && window.views.window_id == window.id && window.views.windows.is_empty() && strip_valid(&window.views))
    }

    /// Move a view intact into its own window, keeping a Model view in the source window.
    pub fn detach(&mut self, id: u64) -> Option<Self> {
        let i = self.index_of(id)?;
        if self.views.len() == 1 {
            if self.at_capacity() {
                return None;
            }
            self.open(ViewKind::Model, None, false);
            if self.views.len() == 1 {
                return None;
            }
        }
        let view = self.views.remove(i);
        if self.active == id {
            self.active = self.views[i.saturating_sub(1)].id;
        }
        self.prune_tiles();
        Some(Self::strip(vec![view], id, self.next_id, 0))
    }

    /// Keep one id sequence and one open-view limit across the design's windows.
    pub fn share_window_limits(&mut self, next_id: u64, elsewhere: usize) {
        self.next_id = self.next_id.max(next_id);
        self.elsewhere = elsewhere;
    }

    /// Open and recently closed views, including those kept when a presentation is parked.
    pub fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.strip_ids().chain(self.windows.iter().flat_map(|window| window.views.strip_ids()))
    }

    /// The next unused view id for this design.
    pub fn next_id(&self) -> u64 {
        self.allocation_watermark()
    }

    /// Return the views of a closed window to this window's tab strip.
    pub fn reattach(&mut self, mut other: Self) {
        self.next_id = self.next_id.max(other.next_id);
        self.views.append(&mut other.views);
        self.closed.append(&mut other.closed);
        let excess = self.closed.len().saturating_sub(CLOSED_CAP);
        self.closed.drain(..excess);
    }

    /// Reflect the engine's single sketch owner across this design's open windows. A missing or
    /// unsupported target leaves the previous state intact. Closing or duplicating a view never
    /// creates a second editing session; moving the original carries its state with it.
    pub fn set_editing(&mut self, editing: Option<(u64, SketchSession)>) -> bool {
        if let Some((id, _)) = editing
            && !self
                .window_of(id)
                .and_then(|window| self.views_of(window))
                .and_then(|views| views.get(id))
                .is_some_and(|view| matches!(view.kind, ViewKind::Model | ViewKind::Sheet))
        {
            return false;
        }
        for view in &mut self.views {
            view.editing = editing.filter(|(id, _)| *id == view.id).map(|(_, state)| state);
        }
        for (_, view) in &mut self.closed {
            view.editing = None;
        }
        for window in &mut self.windows {
            for view in &mut window.views.views {
                view.editing = editing.filter(|(id, _)| *id == view.id).map(|(_, state)| state);
            }
            for (_, view) in &mut window.views.closed {
                view.editing = None;
            }
        }
        true
    }

    /// The views in tab order (never empty).
    pub fn views(&self) -> &[DesignView] {
        &self.views
    }

    /// The shown view.
    pub fn active(&self) -> &DesignView {
        self.views.iter().find(|view| view.id == self.active).unwrap_or(&self.views[0])
    }

    /// How many views are open (at least one).
    pub fn len(&self) -> usize {
        self.views.len()
    }

    /// Whether no view is open (never, a set always keeps one).
    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }

    /// The view with `id`.
    pub fn get(&self, id: u64) -> Option<&DesignView> {
        self.views.iter().find(|view| view.id == id)
    }

    /// Where the view with `id` is in tab order.
    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.views.iter().position(|view| view.id == id)
    }

    /// Whether Reopen Closed View has a view to bring back.
    pub fn can_reopen(&self) -> bool {
        !self.closed.is_empty()
    }

    /// Whether a view can still be shown, including by reopening it.
    pub fn has_open_or_closed(&self, id: u64) -> bool {
        self.ids().any(|known| known == id)
    }

    /// Open a view of `kind` showing `subject` and make it the active one. With `reuse`, a view
    /// already showing the same kind and subject is activated instead. A new view is named by
    /// kind and number ("Sheet 2") and goes last. Full (`MAX_VIEWS`): the active view stays.
    /// An oversized subject or exhausted id space also keeps the active view unchanged.
    /// Returns the id of the view that is active now.
    pub fn open(&mut self, kind: ViewKind, subject: Option<String>, reuse: bool) -> u64 {
        if subject.as_ref().is_some_and(|subject| subject.len() > MAX_SUBJECT) {
            return self.active;
        }
        if reuse && let Some(view) = self.views.iter().find(|view| view.kind == kind && view.subject == subject) {
            self.active = view.id;
            return view.id;
        }
        if self.at_capacity() || self.next_id > MAX_ID {
            return self.active;
        }
        let Some(id) = self.take_id() else { return self.active };
        let name = self.default_name(kind);
        self.views.push(DesignView { editing: None, id, kind, name, subject, spec: None, compass: true });
        self.active = id;
        id
    }

    /// Make the view with `id` the active one. False when there is none.
    pub fn activate(&mut self, id: u64) -> bool {
        let found = self.get(id).is_some();
        if found {
            self.active = id;
        }
        found
    }

    /// Make the `i`th view (0 is first) the active one. False when there is none.
    pub fn activate_index(&mut self, i: usize) -> bool {
        match self.views.get(i) {
            Some(view) => {
                self.active = view.id;
                true
            }
            None => false,
        }
    }

    /// Close the view with `id`, which Reopen can bring back. The last view never closes. When
    /// the active view closes, the one to its left takes over (else the one to its right).
    pub fn close(&mut self, id: u64) -> bool {
        let Some(i) = self.index_of(id) else { return false };
        if self.views.len() < 2 {
            return false;
        }
        self.remove_closed(i);
        if self.active == id {
            self.active = self.views[i.saturating_sub(1)].id;
        }
        self.prune_tiles();
        true
    }

    /// Drop every view, open or closed, that `keep` refuses: views of something the design no
    /// longer has. A dropped view does not go onto the closed stack, so Reopen cannot bring it
    /// back. The set keeps one view: when none would be left, a Model view takes their place. When
    /// the active view goes, the one left of it takes over (else the first). True when any went.
    pub fn drop_views(&mut self, keep: impl Fn(&DesignView) -> bool) -> bool {
        let next_id = self.allocation_watermark();
        // Main must retain a view even when all of its subjects disappear. Check that its
        // replacement can have a fresh identity before removing any windows or closed history.
        if !self.views.iter().any(&keep) && next_id > MAX_ID {
            return false;
        }
        // Closing a window must not forget identities its strip has already allocated.
        self.next_id = next_id;
        let mut changed = false;
        let mut returned_closed = Vec::new();
        self.windows.retain_mut(|window| {
            if !window.views.views().iter().any(&keep) {
                returned_closed.append(&mut window.views.closed);
                changed = true;
                return false;
            }
            changed |= window.views.drop_local_views(&keep);
            true
        });
        self.closed.append(&mut returned_closed);
        changed |= self.drop_local_views(&keep);
        let excess = self.closed.len().saturating_sub(CLOSED_CAP);
        self.closed.drain(..excess);
        changed
    }

    fn drop_local_views(&mut self, keep: &impl Fn(&DesignView) -> bool) -> bool {
        let replacement = if self.views.iter().any(keep) {
            None
        } else {
            let Some(id) = self.take_id() else { return false };
            Some(id)
        };
        let closed = self.closed.len();
        self.closed.retain(|(_, view)| keep(view));
        let Some(first_gone) = self.views.iter().position(|view| !keep(view)) else {
            return self.closed.len() != closed;
        };
        let left = self.index_of(self.active).unwrap_or(first_gone);
        let fallback = self.views.iter().take(left).rev().find(|view| keep(view)).map(|view| view.id);
        self.views.retain(keep);
        if let Some(id) = replacement {
            self.views.push(DesignView {
                editing: None,
                id,
                kind: ViewKind::Model,
                name: ViewKind::Model.label().into(),
                subject: None,
                spec: None,
                compass: true,
            });
        }
        if self.get(self.active).is_none() {
            self.active = fallback.or_else(|| self.views.first().map(|view| view.id)).unwrap_or(self.active);
        }
        self.prune_tiles();
        true
    }

    /// Close every view but the one with `id`, which becomes the active one.
    pub fn close_others(&mut self, id: u64) {
        if self.get(id).is_none() {
            return;
        }
        while let Some(i) = self.views.iter().position(|view| view.id != id) {
            self.remove_closed(i);
        }
        self.active = id;
        self.prune_tiles();
    }

    /// Close the views after the one with `id`; when the active view was among them, `id` takes over.
    pub fn close_right_of(&mut self, id: u64) {
        let Some(i) = self.index_of(id) else { return };
        while self.views.len() > i + 1 {
            self.remove_closed(i + 1);
        }
        if self.index_of(self.active).is_none() {
            self.active = id;
        }
        self.prune_tiles();
    }

    /// Bring back the view closed last, in its old place, and make it active. None when no view
    /// was closed (or the set is full).
    pub fn reopen(&mut self) -> Option<u64> {
        if self.at_capacity() {
            return None;
        }
        let (at, view) = self.closed.pop()?;
        let id = view.id;
        self.views.insert(at.min(self.views.len()), view);
        self.active = id;
        Some(id)
    }

    /// Set the compass on the named view without changing which view is active.
    pub fn set_compass(&mut self, id: u64, on: bool) -> bool {
        let Some(window) = self.window_of(id) else { return false };
        let strip = if window == 0 {
            self
        } else if let Some(window) = self.window_mut(window) {
            &mut window.views
        } else {
            return false;
        };
        let Some(view) = strip.views.iter_mut().find(|view| view.id == id) else { return false };
        if !view.kind.can_tile() {
            return false;
        }
        view.compass = on;
        true
    }

    /// Keep `spec` as how the view `id` looks at the model. False when there is no such view.
    pub fn set_spec(&mut self, id: u64, spec: ViewSpec) -> bool {
        match self.views.iter_mut().find(|view| view.id == id) {
            Some(view) => {
                view.spec = Some(spec);
                true
            }
            None => false,
        }
    }

    /// Give the view with `id` a new name: trimmed, 1 to `MAX_NAME` characters, no control
    /// characters. False when the name or the view is not acceptable.
    pub fn rename(&mut self, id: u64, name: &str) -> bool {
        let Some(name) = valid_name(name) else { return false };
        match self.views.iter_mut().find(|view| view.id == id) {
            Some(view) => {
                view.name = name.to_owned();
                true
            }
            None => false,
        }
    }

    /// Copy the view with `id` into a new one right after it (same kind and subject, a new
    /// default name) and make it active. None when there is no such view or the set is full.
    pub fn duplicate(&mut self, id: u64) -> Option<u64> {
        let i = self.index_of(id)?;
        if self.at_capacity() || self.next_id > MAX_ID {
            return None;
        }
        let (kind, subject, spec) = (self.views[i].kind, self.views[i].subject.clone(), self.views[i].spec);
        let compass = self.views[i].compass;
        let copy = DesignView { editing: None, id: self.take_id()?, kind, name: self.default_name(kind), subject, spec, compass };
        self.active = copy.id;
        self.views.insert(i + 1, copy);
        Some(self.active)
    }

    /// Move the view at place `from` to place `to`, the ones between shifting over. The active
    /// view stays the same view. Places out of range change nothing.
    pub fn move_view(&mut self, from: usize, to: usize) {
        if from >= self.views.len() || to >= self.views.len() || from == to {
            return;
        }
        let view = self.views.remove(from);
        self.views.insert(to, view);
    }

    /// The layout views are shown together in (None: one at a time).
    pub fn tiles(&self) -> Option<&Tiles> {
        self.tiles.as_ref()
    }

    /// The active view is shown together with others: the window draws [`Self::tiles`].
    pub fn is_tiled(&self) -> bool {
        self.tiles.as_ref().is_some_and(|tiles| tiles.contains(self.active))
    }

    /// Show the available workspace views as `preset`, in tab order, with the active one among them.
    pub fn set_layout(&mut self, preset: Preset) -> usize {
        let order = self.layout_order(preset.tiles().max(1));
        self.tiles = Tiles::preset(preset, &order).filter(|tiles| tiles.len() > 1);
        self.tiles.as_ref().map_or(1, Tiles::len)
    }

    /// Like [`Self::set_layout`], but a place without a view is an empty cell.
    pub fn set_layout_padded(&mut self, preset: Preset) -> usize {
        let order = self.layout_order(preset.tiles().max(1));
        self.tiles = if order.is_empty() { None } else { Tiles::preset_padded(preset, &order).filter(|tiles| tiles.len() > 1) };
        self.tiles.as_ref().map_or(1, Tiles::len)
    }

    /// Number of views that can fill the current layout's preset, before adding new angles.
    pub fn layout_view_count(&self) -> usize {
        self.layout_order(crate::MAX_TILES).len()
    }

    fn layout_order(&self, limit: usize) -> Vec<u64> {
        if !self.active().kind.can_tile() {
            return Vec::new();
        }
        let mut order: Vec<_> = self.views.iter().filter(|view| view.kind.can_tile()).map(|view| view.id).take(limit).collect();
        if !order.contains(&self.active)
            && let Some(last) = order.last_mut()
        {
            *last = self.active;
        }
        order
    }

    /// Both views have tile canvases and can share one layout.
    pub fn can_tile_with(&self, id: u64, other: u64) -> bool {
        let kind = |id| self.get(id).map(|view| view.kind).filter(|kind| kind.can_tile());
        matches!((kind(id), kind(other)), (Some(_), Some(_)))
    }

    /// Whether the view could join the layout without replacing an occupied cell.
    pub fn can_show_beside(&self, id: u64) -> bool {
        id != self.active
            && self.can_tile_with(id, self.active)
            && self
                .tiles
                .as_ref()
                .filter(|tiles| tiles.contains(self.active))
                .is_none_or(|tiles| !tiles.contains(id) && tiles.views().into_iter().all(|other| self.can_tile_with(id, other)))
    }

    /// Show the open view `id` in the empty cell number `ordinal` of the layout.
    pub fn fill_empty(&mut self, ordinal: usize, id: u64) -> bool {
        self.is_tiled() && self.can_show_beside(id) && self.tiles.as_mut().is_some_and(|tiles| tiles.fill_empty(ordinal, id))
    }

    /// Close the empty cell number `ordinal`; a layout left with one cell is none.
    pub fn remove_empty(&mut self, ordinal: usize) -> bool {
        let removed = self.tiles.as_mut().is_some_and(|tiles| tiles.remove_empty(ordinal));
        if removed && self.tiles.as_ref().is_some_and(|tiles| tiles.len() < 2) {
            self.tiles = None;
        }
        removed
    }

    /// Show the view `id` beside the active one, on `edge` of it.
    pub fn show_beside(&mut self, id: u64, edge: Edge) -> bool {
        if !self.can_show_beside(id) {
            return false;
        }
        let tiles = self.tiles.get_or_insert(Tiles::Leaf(self.active));
        if !tiles.contains(self.active) {
            *tiles = Tiles::Leaf(self.active);
        }
        let shown = tiles.split_at(self.active, id, edge);
        self.prune_tiles();
        shown
    }

    /// What the window would show if it placed this view at the given target.
    pub fn preview_placement(&self, id: u64, placement: Placement) -> Option<Tiles> {
        self.preview_incoming_placement(id, self.get(id)?.kind, placement)
    }

    /// Preview with the incoming view's kind before transferring it to this strip.
    pub fn preview_incoming_placement(&self, id: u64, kind: ViewKind, placement: Placement) -> Option<Tiles> {
        if !kind.can_tile() || !self.active().kind.can_tile() {
            return None;
        }
        let next = if let Some(tiles) = self.tiles.as_ref().filter(|tiles| tiles.contains(self.active)) {
            tiles.placed(id, placement)
        } else {
            Tiles::Leaf(self.active).placed(id, placement)
        }?;
        next.views().into_iter().all(|other| other == id || self.get(other).is_some_and(|view| view.kind.can_tile())).then_some(next)
    }

    /// Place an open view in the current window. Keep focus if it still has a cell.
    pub fn place_tile(&mut self, id: u64, placement: Placement) -> bool {
        let Some(tiles) = self.preview_placement(id, placement) else { return false };
        if !tiles.contains(self.active) {
            self.active = id;
        }
        self.tiles = (tiles.len() > 1).then_some(tiles);
        true
    }

    /// Move a splitter of the layout ([`Tiles::set_ratio`]).
    pub fn set_split(&mut self, splitter: usize, ratio: f32) -> bool {
        self.tiles.as_mut().is_some_and(|tiles| tiles.set_ratio(splitter, ratio))
    }

    /// Give every tile the same share of the available area without moving views between cells.
    pub fn equal_sizes(&mut self) -> bool {
        self.tiles.as_mut().is_some_and(Tiles::equal_sizes)
    }

    pub fn camera_link(&self) -> CameraLinkMode {
        self.camera_link
    }

    pub fn set_camera_link(&mut self, mode: CameraLinkMode) {
        self.camera_link = mode;
    }

    /// Swap the places of two shown views.
    pub fn swap_tiles(&mut self, a: u64, b: u64) -> bool {
        self.tiles.as_mut().is_some_and(|tiles| tiles.swap(a, b))
    }

    /// Stop showing `id` beside the others: it stays open as a view of its own, reached from the
    /// bar. False when it is not one of the views shown together.
    pub fn unshow(&mut self, id: u64) -> bool {
        let removed = self.tiles.as_mut().is_some_and(|tiles| tiles.remove(id));
        if removed && self.tiles.as_ref().is_some_and(|tiles| tiles.len() < 2) {
            self.tiles = None;
        }
        removed
    }

    /// One view at a time again; the views stay open.
    pub fn untile(&mut self) {
        self.tiles = None;
    }

    /// Drop only views that are gone or lack a tile canvas.
    fn prune_tiles(&mut self) {
        let Some(tiles) = &mut self.tiles else { return };
        let kind = |id| self.views.iter().find(|view| view.id == id).map(|view| view.kind);
        let members = tiles.views();
        for id in members {
            if kind(id).is_none_or(|kind| !kind.can_tile()) {
                tiles.remove(id);
            }
        }
        if tiles.len() < 2 {
            self.tiles = None;
        }
    }

    /// The set as remembered in user settings for a design. Window ids on the flat view list maintain compatibility
    /// with older readers, which ignore them and show all the views in their main window.
    pub fn to_json(&self) -> Value {
        let view_json = |view: &DesignView, window| json!({"id": view.id, "kind": view.kind.key(), "name": view.name, "subject": view.subject, "spec": view.spec, "window": window, "compass": view.compass});
        let strips = std::iter::once(self).chain(self.windows.iter().map(|window| &window.views));
        json!({
            "active": self.active,
            "next_id": self.next_id,
            "next_window": self.next_window,
            "views": strips.clone().flat_map(|strip| strip.views.iter().map(move |view| view_json(view, strip.window_id))).collect::<Vec<_>>(),
            "closed": strips.flat_map(|strip| strip.closed.iter().map(move |(at, view)| json!({"at": at, "view": view_json(view, strip.window_id)}))).collect::<Vec<_>>(),
            "tiles": self.tiles.as_ref().map(Tiles::to_json),
            "windows": self.windows.iter().map(|window| json!({
                "id": window.id,
                "name": window.views.window_name(),
                "level": window.level.key(),
                "options": window.options,
                "panel_layout": window.panel_layout,
                "active": window.views.active,
                "tiles": window.views.tiles().map(Tiles::to_json),
                "placement": window.placement.as_ref().map(WindowPlacement::to_json),
            })).collect::<Vec<_>>(),
        })
    }

    /// Read a saved set. Keep the original validation on view identities and names. Broken
    /// window references fall back to the main strip and empty windows are ignored.
    /// Keep an opaque, bounded UI panel record without parsing panel internals.
    pub fn read_panel_layout(value: &Value) -> Option<Value> {
        // The panel preferences keep their catalogue as an object, older records as its text.
        let size = match value.get("catalogue")? {
            Value::String(record) => record.len(),
            catalogue @ Value::Object(_) => serde_json::to_string(catalogue).ok()?.len(),
            _ => return None,
        };
        (size <= 256 * 1024).then(|| value.clone())
    }

    pub fn from_json(value: &Value) -> Option<ViewSet> {
        let migrated = migrate_sketch_views(value)?;
        let value = &migrated;
        let views = value.get("views")?.as_array()?;
        if views.is_empty() || views.len() > MAX_VIEWS {
            return None;
        }
        let window_id = |item: &Value, key: &str| item.get(key)?.as_u64().and_then(|id| WindowId::try_from(id).ok());
        let mut window_records = Vec::new();
        for item in value.get("windows").and_then(Value::as_array).into_iter().flatten().take(MAX_VIEWS) {
            if let Some(id) = window_id(item, "id").filter(|id| *id != 0)
                && !window_records.iter().any(|(known, _)| *known == id)
                && views.iter().any(|view| window_id(view, "window") == Some(id))
            {
                window_records.push((id, item));
            }
            if window_records.len() >= MAX_POPOUTS {
                break;
            }
        }
        let mut used = Vec::new();
        let mut placed: Vec<_> = views
            .iter()
            .map(|item| {
                let view = view_from_json(item)?;
                if used.contains(&view.id) {
                    return None;
                }
                used.push(view.id);
                let id = window_id(item, "window").filter(|id| window_records.iter().any(|(known, _)| known == id)).unwrap_or(0);
                Some((id, view))
            })
            .collect::<Option<_>>()?;
        if placed.iter().all(|(id, _)| *id != 0)
            && let Some((id, _)) = placed.first_mut()
        {
            *id = 0;
        }
        let mut closed = Vec::new();
        let saved_closed = value.get("closed").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        let oldest = saved_closed.len().saturating_sub(CLOSED_CAP * (1 + MAX_POPOUTS));
        for entry in saved_closed.iter().skip(oldest) {
            let at = entry.get("at").and_then(Value::as_u64).and_then(|at| usize::try_from(at).ok());
            let parsed = entry.get("view").and_then(|item| Some((window_id(item, "window").unwrap_or(0), view_from_json(item)?)));
            if let (Some(at), Some((id, view))) = (at, parsed)
                && !used.contains(&view.id)
            {
                used.push(view.id);
                closed.push((id, (at.min(MAX_VIEWS), view)));
            }
        }
        let highest = used.iter().copied().max().unwrap_or(0);
        let next_id = value.get("next_id").and_then(Value::as_u64).unwrap_or(0).clamp(highest + 1, MAX_ID + 1);
        let mut read_strip = |id, data: &Value| {
            let mut views = Vec::new();
            placed.retain(|(owner, view)| {
                if *owner == id {
                    views.push(view.clone());
                    false
                } else {
                    true
                }
            });
            let first = views.first()?.id;
            let active = data.get("active").and_then(Value::as_u64).filter(|active| views.iter().any(|view| view.id == *active)).unwrap_or(first);
            let mut strip = Self::strip(views, active, next_id, id);
            strip.tiles = data.get("tiles").and_then(Tiles::from_json);
            strip.prune_tiles();
            Some(strip)
        };
        let mut set = read_strip(0, value)?;
        for (id, item) in window_records {
            let Some(mut views) = read_strip(id, item) else { continue };
            if let Some(name) = item.get("name").and_then(Value::as_str) {
                views.rename_window(name);
            }
            let level = item.get("level").and_then(Value::as_str).and_then(WindowLevel::from_key).unwrap_or_default();
            let placement = item.get("placement").and_then(WindowPlacement::from_json);
            let options = item.get("options").cloned().and_then(|value| serde_json::from_value(value).ok()).unwrap_or_default();
            let panel_layout = item.get("panel_layout").and_then(Self::read_panel_layout);
            set.put_window(PopOut { panel_layout, options, id, level, placement, views });
        }
        for (owner, entry) in closed {
            let strip = set.windows.iter_mut().find(|window| window.id == owner).map_or(&mut set.closed, |window| &mut window.views.closed);
            strip.push(entry);
            if strip.len() > CLOSED_CAP {
                strip.remove(0);
            }
        }
        let highest_window = set.windows.iter().map(|window| window.id).max().unwrap_or(0);
        set.next_window = window_id(value, "next_window").filter(|id| *id > highest_window).unwrap_or(highest_window.checked_add(1).unwrap_or(1));
        Some(set)
    }

    fn strip_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.views.iter().map(|view| view.id).chain(self.closed.iter().map(|(_, view)| view.id))
    }

    fn at_capacity(&self) -> bool {
        self.len().saturating_add(self.elsewhere).saturating_add(self.windows.iter().map(|window| window.views.len()).sum::<usize>()) >= MAX_VIEWS
    }

    fn allocation_watermark(&self) -> u64 {
        self.windows.iter().map(|window| window.views.next_id).fold(self.next_id, u64::max)
    }

    fn take_id(&mut self) -> Option<u64> {
        let id = self.allocation_watermark();
        if id > MAX_ID {
            return None;
        }
        self.next_id = id + 1;
        Some(id)
    }

    /// "Sheet 1", "Sheet 2"…: the first name of the kind no open view has. The first Model
    /// and Render views go without a number.
    fn default_name(&self, kind: ViewKind) -> String {
        (1..)
            .map(
                |n| {
                    if n == 1 && matches!(kind, ViewKind::Model | ViewKind::Render) {
                        kind.label().to_owned()
                    } else {
                        format!("{} {n}", kind.label())
                    }
                },
            )
            .find(|name| self.views.iter().all(|view| view.name != *name))
            .unwrap_or_default()
    }

    /// Take the view at `i` out of the set onto the closed stack.
    fn remove_closed(&mut self, i: usize) {
        let mut view = self.views.remove(i);
        view.editing = None;
        self.closed.push((i, view));
        let skip = self.closed.len().saturating_sub(CLOSED_CAP);
        self.closed.drain(..skip);
    }
}

/// `name` as a view name, or None when it is empty, too long or holds control characters.
fn valid_name(name: &str) -> Option<&str> {
    let name = name.trim();
    (!name.is_empty() && name.chars().count() <= MAX_NAME && !name.chars().any(char::is_control)).then_some(name)
}

/// Remove the old Sketch tab kind from remembered UI layouts. Features live in the design
/// document and are never changed here. Reserve removed identities so future tabs cannot inherit
/// their stale camera or placement references.
fn migrate_sketch_views(value: &Value) -> Option<std::borrow::Cow<'_, Value>> {
    let original = value.get("views")?.as_array()?;
    if original.is_empty() || original.len() > MAX_VIEWS {
        return None;
    }
    let closed = value.get("closed").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let oldest = closed.len().saturating_sub(CLOSED_CAP * (1 + MAX_POPOUTS));
    let history = closed.get(oldest..)?;
    let legacy = |view: &Value| view.get("kind").and_then(Value::as_str) == Some("sketch");
    if !original.iter().any(legacy) && !history.iter().filter_map(|entry| entry.get("view")).any(legacy) {
        return Some(std::borrow::Cow::Borrowed(value));
    }
    // Clone only the history the reader can use. Cloning the entire value before trimming
    // duplicated an arbitrarily large discarded prefix whenever a legacy tab was present.
    let mut saved = Value::Object(
        value
            .as_object()?
            .iter()
            .map(|(key, item)| {
                let item = if key == "closed" && item.is_array() { Value::Array(history.to_vec()) } else { item.clone() };
                (key.clone(), item)
            })
            .collect(),
    );
    let mut used = Vec::new();
    for view in original {
        let mut readable = view.clone();
        if legacy(view) {
            readable["kind"] = json!("model");
        }
        let parsed = view_from_json(&readable)?;
        if used.contains(&parsed.id) {
            return None;
        }
        used.push(parsed.id);
    }
    let mut removed = Vec::new();
    let window = |view: &Value| view.get("window").and_then(Value::as_u64).unwrap_or(0);
    let windows = value.get("windows").and_then(Value::as_array);
    let mut known_windows = Vec::new();
    for entry in windows.into_iter().flatten().take(MAX_VIEWS) {
        if let Some(id) = entry.get("id").and_then(Value::as_u64).filter(|id| *id != 0 && WindowId::try_from(*id).is_ok())
            && !known_windows.contains(&id)
            && original.iter().any(|view| window(view) == id)
        {
            known_windows.push(id);
        }
        if known_windows.len() >= MAX_POPOUTS {
            break;
        }
    }
    let owner = |view: &Value| {
        let id = window(view);
        if known_windows.contains(&id) { id } else { 0 }
    };
    let mut retained: Vec<Value> = original
        .iter()
        .filter_map(|view| {
            if legacy(view) {
                removed.push((owner(view), view["id"].as_u64()?));
                None
            } else {
                Some(view.clone())
            }
        })
        .collect();
    // Match the reader's bounded, first-valid closed history. Removed Sketch identities still
    // reserve their place, so a duplicate closed record cannot resurrect them as another kind.
    let keep_closed: Vec<_> = history
        .iter()
        .map(|entry| {
            if entry.get("at").and_then(Value::as_u64).and_then(|at| usize::try_from(at).ok()).is_none() {
                return false;
            }
            let Some(view) = entry.get("view") else { return false };
            let mut readable = view.clone();
            if legacy(view) {
                readable["kind"] = json!("model");
            }
            let Some(parsed) = view_from_json(&readable) else { return false };
            if used.contains(&parsed.id) {
                return false;
            }
            used.push(parsed.id);
            !legacy(view)
        })
        .collect();
    if let Some(entries) = saved.get_mut("closed").and_then(Value::as_array_mut) {
        let mut keep = keep_closed.into_iter();
        entries.retain(|_| keep.next().unwrap_or(false));
    }
    let mut next = value.get("next_id").and_then(Value::as_u64).unwrap_or(0).clamp(used.iter().copied().max().unwrap_or(0) + 1, MAX_ID + 1);
    let mut owners = Vec::new();
    for (id, _) in &removed {
        if !owners.contains(id) {
            owners.push(*id);
        }
    }
    for id in owners {
        let data = if id == 0 { value } else { windows?.iter().find(|entry| entry.get("id").and_then(Value::as_u64) == Some(id))? };
        let active = data.get("active").and_then(Value::as_u64);
        let active_removed = removed.iter().any(|(window, view)| *window == id && Some(*view) == active);
        if !active_removed && retained.iter().any(|view| owner(view) == id) {
            continue;
        }
        let model = retained.iter().find(|view| owner(view) == id && view.get("kind").and_then(Value::as_str) == Some("model"));
        let model_id = if let Some(model) = model {
            model["id"].as_u64()?
        } else {
            if next > MAX_ID {
                return None;
            }
            let name = (1..=MAX_VIEWS + 1).map(|n| if n == 1 { "Model".to_owned() } else { format!("Model {n}") }).find(|name| {
                retained.iter().filter(|view| owner(view) == id).all(|view| view.get("name").and_then(Value::as_str) != Some(name.as_str()))
            })?;
            let model_id = next;
            next += 1;
            retained.push(json!({"id": model_id, "kind": "model", "name": name, "window": id}));
            model_id
        };
        if id == 0 {
            saved["active"] = json!(model_id);
        } else if let Some(entry) = saved
            .get_mut("windows")
            .and_then(Value::as_array_mut)
            .and_then(|entries| entries.iter_mut().find(|entry| entry.get("id").and_then(Value::as_u64) == Some(id)))
        {
            entry["active"] = json!(model_id);
        }
    }
    saved["views"] = Value::Array(retained);
    saved["next_id"] = json!(next);
    Some(std::borrow::Cow::Owned(saved))
}

fn view_from_json(value: &Value) -> Option<DesignView> {
    let id = value.get("id")?.as_u64().filter(|id| (1..=MAX_ID).contains(id))?;
    let kind = ViewKind::from_key(value.get("kind")?.as_str()?)?;
    let name = valid_name(value.get("name")?.as_str()?)?.to_owned();
    let subject = match value.get("subject") {
        None | Some(Value::Null) => None,
        Some(Value::String(subject)) if subject.len() <= MAX_SUBJECT => Some(subject.clone()),
        Some(_) => return None,
    };
    // A spec that does not read back, or holds numbers a camera cannot use, is dropped: the view
    // then starts from where the design looks.
    let spec = value.get("spec").and_then(|spec| serde_json::from_value::<ViewSpec>(spec.clone()).ok()).filter(ViewSpec::is_valid);
    let compass = value.get("compass").and_then(Value::as_bool).unwrap_or(true);
    Some(DesignView { editing: None, id, kind, name, subject, spec, compass })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(set: &ViewSet) -> Vec<&str> {
        set.views().iter().map(|view| view.name.as_str()).collect()
    }

    #[test]
    fn legacy_sketch_tabs_are_removed_from_every_window_and_closed_history() {
        let spec = ViewSpec::new(solidcraft_viewport::Camera { yaw: 0.7, ..Default::default() });
        let saved = json!({
            "views": [
                {"id": 1, "kind": "model", "name": "Angle", "compass": false, "spec": spec},
                {"id": 2, "kind": "sketch", "name": "Sketch 1", "subject": "7"},
                {"id": 3, "kind": "render", "name": "Beauty"},
                {"id": 5, "kind": "model", "name": "Side", "window": 1},
                {"id": 6, "kind": "sketch", "name": "Sketch 2", "window": 1},
                {"id": 7, "kind": "sheet", "name": "Drawing", "subject": "d1", "window": 1}
            ],
            "active": 2,
            "tiles": {"axis": "row", "ratio": 0.375, "first": {"view": 1}, "second": {
                "axis": "column", "ratio": 0.6, "first": {"view": 2}, "second": {"view": 3}
            }},
            "windows": [{"id": 1, "active": 6, "name": "Details", "level": "normal"}],
            "closed": [
                {"at": 1, "view": {"id": 8, "kind": "render", "name": "Closed render"}},
                {"at": 0, "view": {"id": 90, "kind": "sketch", "name": "Closed sketch"}},
                {"at": 0, "view": {"id": 91, "kind": "sketch", "name": "Closed sketch", "window": 1}}
            ]
        });
        let mut set = ViewSet::from_json(&saved).unwrap();
        assert_eq!(set.views().iter().map(|view| view.id).collect::<Vec<_>>(), [1, 3]);
        assert_eq!(set.active().id, 1);
        assert!(!set.active().compass);
        assert_eq!(set.active().spec, Some(spec));
        assert_eq!(set.tiles().unwrap().views(), [1, 3]);
        assert_eq!(set.tiles().unwrap().to_json(), json!({"axis":"row", "ratio":0.375, "first":{"view":1}, "second":{"view":3}}));
        let child = &set.window(1).unwrap().views;
        assert_eq!(child.views().iter().map(|view| view.id).collect::<Vec<_>>(), [5, 7]);
        assert_eq!(child.active().id, 5);
        assert_eq!(child.window_name(), Some("Details"));
        assert_eq!(child.get(7).unwrap().subject.as_deref(), Some("d1"));
        assert!(!child.can_reopen());
        assert_eq!(ViewSet::from_json(&set.to_json()), Some(set.clone()));
        assert!(!set.to_json().to_string().contains("sketch"));
        assert_eq!(set.reopen(), Some(8));
        assert!(!set.can_reopen());
        assert!(set.open(ViewKind::Model, None, false) > 91, "removed identities stay reserved");
    }

    #[test]
    fn legacy_sketch_only_windows_get_a_fresh_model_without_moving_other_views() {
        let saved = json!({
            "views": [
                {"id": 10, "kind": "sketch", "name": "Sketch 1"},
                {"id": 20, "kind": "sketch", "name": "Sketch 2", "window": 1},
                {"id": 30, "kind": "render", "name": "Beauty", "window": 2}
            ],
            "active": 10,
            "windows": [{"id": 1, "active": 20, "name": "Editing"}, {"id": 2, "active": 30}],
            "next_id": 40
        });
        let set = ViewSet::from_json(&saved).unwrap();
        assert_eq!((set.active().id, set.active().kind), (40, ViewKind::Model));
        assert_eq!((set.window(1).unwrap().views.active().id, set.window(1).unwrap().views.active().kind), (41, ViewKind::Model));
        assert_eq!(set.window(1).unwrap().views.window_name(), Some("Editing"));
        assert_eq!(set.window(2).unwrap().views.active().id, 30);
        assert_eq!(set.window_of(30), Some(2));
        assert_eq!(set.next_id(), 42);
        assert_eq!(ViewSet::from_json(&set.to_json()), Some(set));
    }

    #[test]
    fn legacy_kind_migration_preserves_validation_and_non_sketch_focus() {
        assert_eq!(ViewKind::from_key("sketch"), None);
        let mut saved = json!({"views":[
            {"id":1,"kind":"model","name":"Model"},
            {"id":2,"kind":"sketch","name":"Sketch 1"},
            {"id":3,"kind":"render","name":"Render"}
        ], "active":3});
        assert_eq!(ViewSet::from_json(&saved).unwrap().active().id, 3);
        saved["views"][1]["id"] = json!(1);
        assert!(ViewSet::from_json(&saved).is_none(), "a removed kind cannot hide duplicate identities");
        saved["views"][1]["id"] = json!(2);
        saved["views"][1]["name"] = json!("");
        assert!(ViewSet::from_json(&saved).is_none(), "legacy tabs retain input validation");
        saved["views"][1]["name"] = json!("Old");
        saved["views"][1]["kind"] = json!("unknown");
        assert!(ViewSet::from_json(&saved).is_none(), "only the known legacy kind migrates");
        let exhausted = json!({"views":[{"id":MAX_ID,"kind":"sketch","name":"Old"}]});
        assert!(ViewSet::from_json(&exhausted).is_none(), "hostile identity exhaustion never overflows");
    }

    #[test]
    fn legacy_migration_never_resurrects_duplicate_closed_identities() {
        let saved = json!({
            "views": [
                {"id":1,"kind":"model","name":"Model"},
                {"id":2,"kind":"sketch","name":"Open legacy"}
            ],
            "active":2,
            "closed": [
                {"at":1,"view":{"id":2,"kind":"render","name":"Duplicate of open legacy"}},
                {"at":1,"view":{"id":3,"kind":"sketch","name":"First closed legacy"}},
                {"at":1,"view":{"id":3,"kind":"render","name":"Duplicate of closed legacy"}},
                {"at":"invalid","view":{"id":4,"kind":"sketch","name":"Invalid entry"}},
                {"at":1,"view":{"id":4,"kind":"render","name":"First valid four"}},
                {"at":1,"view":{"id":5,"kind":"render","name":"First valid five"}},
                {"at":1,"view":{"id":5,"kind":"sketch","name":"Later legacy duplicate"}},
                {"at":1,"view":{"id":6,"kind":"render","name":"First valid six"}},
                {"at":1,"view":{"id":6,"kind":"render","name":"Later duplicate"}}
            ]
        });
        let mut set = ViewSet::from_json(&saved).unwrap();
        assert_eq!(set.active().id, 1);
        assert_eq!(set.next_id(), 7);
        assert_eq!(set.to_json()["closed"].as_array().unwrap().len(), 3);
        for (id, name) in [(6, "First valid six"), (5, "First valid five"), (4, "First valid four")] {
            assert_eq!(set.reopen(), Some(id));
            assert_eq!(set.active().name, name);
        }
        assert_eq!(set.reopen(), None, "removed open and closed Sketch identities never reappear");
        assert!(set.get(2).is_none() && set.get(3).is_none());
        assert_eq!(ViewSet::from_json(&set.to_json()), Some(set));
    }

    #[test]
    fn legacy_migration_bounds_copied_history_and_keeps_window_and_unknown_metadata() {
        let mut history = vec![json!({
            "at": 0,
            "view": {"id": MAX_ID, "kind": "render", "name": "Ancient", "unused": vec![Value::Null; 8192]}
        })];
        history.extend((0..CLOSED_CAP * (1 + MAX_POPOUTS)).map(|_| Value::Null));
        history.extend([
            json!({"at": 1, "view": {"id": 99, "kind": "sketch", "name": "Closed legacy"}}),
            json!({"at": 1, "view": {"id": 77, "kind": "render", "name": "Closed root"}}),
            json!({"at": 1, "view": {"id": 88, "kind": "sheet", "name": "Closed child", "window": 1}}),
        ]);
        let saved = json!({
            "views": [
                {"id": 1, "kind": "model", "name": "Main"},
                {"id": 2, "kind": "sketch", "name": "Root legacy"},
                {"id": 3, "kind": "sketch", "name": "Child legacy", "window": 1},
                {"id": 4, "kind": "model", "name": "Side", "window": 1}
            ],
            "active": 2,
            "next_id": 0,
            "windows": [{"id": 1, "active": 3, "name": "Details", "level": "full", "extension": {"retained": true}}],
            "closed": history,
            "extension": {"retained": ["metadata", 42]}
        });
        let migrated = migrate_sketch_views(&saved).unwrap();
        assert!(matches!(&migrated, std::borrow::Cow::Owned(_)));
        assert_eq!(migrated["extension"], saved["extension"]);
        assert_eq!(migrated["windows"][0]["extension"], saved["windows"][0]["extension"]);
        assert_eq!(migrated["windows"][0]["name"], "Details");
        assert_eq!(migrated["active"], 1);
        assert_eq!(migrated["windows"][0]["active"], 4);
        let copied = migrated["closed"].as_array().unwrap();
        assert_eq!(copied.iter().map(|entry| entry["view"]["id"].as_u64().unwrap()).collect::<Vec<_>>(), [77, 88]);
        assert_eq!(saved["closed"].as_array().unwrap().len(), 1 + CLOSED_CAP * (1 + MAX_POPOUTS) + 3, "the caller's history stays untouched");
        let set = ViewSet::from_json(&migrated).unwrap();
        assert_eq!(set.next_id(), 100, "removed recent identities stay reserved; discarded ancient ones do not");
        assert_eq!(ViewSet::from_json(&set.to_json()).unwrap().to_json(), set.to_json());
    }
    #[test]
    fn legacy_migration_ignores_closed_records_outside_the_readers_valid_history() {
        let mut closed = vec![json!({"at":0,"view":{"id":MAX_ID,"kind":"render","name":"Old"}})];
        closed.extend((0..CLOSED_CAP * (1 + MAX_POPOUTS)).map(|_| Value::Null));
        closed.push(json!({"at":"invalid","view":{"id":MAX_ID,"kind":"sketch","name":"Invalid"}}));
        let saved = json!({"views":[{"id":5,"kind":"sketch","name":"Old"}], "active":5, "closed":closed});
        let set = ViewSet::from_json(&saved).unwrap();
        assert_eq!(set.active().id, 6);
        assert!(!set.can_reopen());

        let mut ancient = vec![json!({"at":0,"view":{"id":MAX_ID,"kind":"sketch","name":"Ancient"}})];
        ancient.extend((0..CLOSED_CAP * (1 + MAX_POPOUTS)).map(|_| Value::Null));
        let saved = json!({"views":[{"id":1,"kind":"model","name":"Model"}], "closed":ancient});
        assert!(matches!(migrate_sketch_views(&saved), Some(std::borrow::Cow::Borrowed(_))), "ancient closed tabs never trigger migration");
        let set = ViewSet::from_json(&saved).unwrap();
        assert_eq!(set.next_id(), 2);
        assert!(!set.can_reopen());
        assert!(
            migrate_sketch_views(&json!({"views": vec![Value::Null; MAX_VIEWS + 1]})).is_none(),
            "oversized open lists are refused before scanning for legacy kinds"
        );
    }

    #[test]
    fn sketch_editing_is_runtime_only_and_follows_the_original_view() {
        for kind in [ViewKind::Model, ViewKind::Sheet] {
            let mut set = ViewSet::new();
            let original = if kind == ViewKind::Model { set.active().id } else { set.open(kind, Some("sheet-a".into()), false) };
            let editing = SketchSession { sketch: 42 };
            assert!(set.set_editing(Some((original, editing))), "{kind:?}");
            let copy = set.duplicate(original).unwrap();
            assert_eq!(set.get(original).unwrap().editing, Some(editing));
            assert_eq!(set.get(copy).unwrap().editing, None);
            assert_eq!(ViewSet::from_json(&set.to_json()).unwrap().get(original).unwrap().editing, None);
            assert!(set.to_json()["views"].as_array().unwrap().iter().all(|view| view.get("editing").is_none()));
            let moved = set.move_to_window(original, WindowTarget::New { placement: None }).unwrap();
            assert_eq!(set.views_of(moved.window).unwrap().get(original).unwrap().editing, Some(editing));
            let saved = set.to_json();
            assert!(saved["views"].as_array().unwrap().iter().all(|view| view.get("editing").is_none()));
            assert_eq!(ViewSet::from_json(&saved).unwrap().views_of(moved.window).unwrap().get(original).unwrap().editing, None);
            assert!(set.set_editing(Some((copy, SketchSession { sketch: 43 }))));
            assert_eq!(set.views_of(moved.window).unwrap().get(original).unwrap().editing, None);
            assert_eq!(set.get(copy).unwrap().editing, Some(SketchSession { sketch: 43 }));
            for unsupported in [ViewKind::Render, ViewKind::Animation] {
                let target = set.open(unsupported, None, false);
                assert!(!set.set_editing(Some((target, editing))));
                assert_eq!(set.get(target).unwrap().editing, None);
            }
            assert!(!set.set_editing(Some((999, editing))));
            assert_eq!(set.get(copy).unwrap().editing, Some(SketchSession { sketch: 43 }));
            let other_kind = if kind == ViewKind::Model { ViewKind::Sheet } else { ViewKind::Model };
            let other = set.open(other_kind, (other_kind == ViewKind::Sheet).then(|| "sheet-b".into()), false);
            assert!(set.set_editing(Some((other, editing))));
            assert_eq!(set.get(copy).unwrap().editing, None, "switching Model/Sheet preserves one global owner");
            assert_eq!(set.views_of(moved.window).unwrap().get(original).unwrap().editing, None);
            assert!(set.set_editing(Some((original, editing))));
            set.move_to_window(original, WindowTarget::Existing { window: 0, index: None }).unwrap();
            assert_eq!(set.get(original).unwrap().editing, Some(editing), "moving back keeps the original owner");
            assert_eq!(set.get(other).unwrap().editing, None);
            assert!(set.close(original));
            assert!(set.closed.iter().all(|(_, view)| view.editing.is_none()));
            assert_eq!(set.reopen(), Some(original));
            assert_eq!(set.get(original).unwrap().editing, None, "reopening a closed view never reopens an editor");
            assert!(set.set_editing(Some((copy, editing))));
            assert!(set.set_editing(None));
            assert_eq!(set.get(copy).unwrap().editing, None);
        }
    }

    fn sheets(n: usize) -> ViewSet {
        let mut set = ViewSet::new();
        for i in 0..n {
            set.open(ViewKind::Sheet, Some(format!("s{i}")), false);
        }
        set
    }

    #[test]
    fn camera_link_is_local_to_the_window() {
        let mut main = ViewSet::new();
        let second = main.duplicate(main.active().id).unwrap();
        main.set_camera_link(CameraLinkMode::Orientation);

        let moved = main.move_to_window(second, new_window()).unwrap();
        let pop_out = main.window_mut(moved.window).unwrap();
        assert_eq!(pop_out.views.camera_link(), CameraLinkMode::Off);
        pop_out.views.set_camera_link(CameraLinkMode::Zoom);

        assert_eq!(main.camera_link(), CameraLinkMode::Orientation);
        assert_eq!(main.views_of(moved.window).unwrap().camera_link(), CameraLinkMode::Zoom);
    }

    #[test]
    fn detached_view_preserves_identity_and_reattaches_without_id_collisions() {
        let mut main = ViewSet::new();
        let model = main.active().id;
        let second = main.duplicate(model).unwrap();
        assert!(main.rename(second, "Side"));
        assert_eq!(main.set_layout(Preset::SideBySide), 2);
        let mut child = main.detach(second).unwrap();
        assert_eq!(main.active().id, model);
        assert!(!main.is_tiled());
        assert_eq!(child.active().id, second);
        assert_eq!(child.active().name, "Side");

        child.share_window_limits(main.next_id(), main.len());
        let new_child_view = child.open(ViewKind::Model, None, false);
        main.share_window_limits(child.next_id(), child.len());
        let new_main_view = main.open(ViewKind::Model, None, false);
        assert_ne!(new_child_view, new_main_view);
        main.reattach(child);
        assert_eq!(main.len(), 4);
        assert!(main.get(second).is_some());
        assert!(main.get(new_child_view).is_some());
    }

    #[test]
    fn detaching_the_only_view_leaves_a_model_and_obeys_the_shared_limit() {
        let mut main = ViewSet::new();
        let model = main.active().id;
        main.share_window_limits(main.next_id(), MAX_VIEWS - 1);
        assert!(main.detach(model).is_none());
        main.share_window_limits(main.next_id(), 0);
        let child = main.detach(model).unwrap();
        assert_eq!(child.active().id, model);
        assert_ne!(main.active().id, model);
        assert_eq!(main.active().kind, ViewKind::Model);
    }

    #[test]
    fn cross_window_dock_preserves_views_and_rejects_a_stale_cell() {
        let mut views = ViewSet::default();
        let ground = views.active().id;
        let incoming = views.open(ViewKind::Model, None, false);
        let destination = views.open(ViewKind::Model, None, false);
        let window = views.move_to_window(destination, new_window()).unwrap().window;
        let before = views.clone();
        assert_eq!(
            views.move_to_window(incoming, WindowTarget::Dock { window, placement: Placement::Replace(ground) }),
            Err(MoveError::BadPlacement)
        );
        assert_eq!(views, before);

        let placement = Placement::Split { target: destination, edge: Edge::Right };
        let layout = views.views_of(window).unwrap().preview_incoming_placement(incoming, ViewKind::Model, placement).unwrap();
        views.move_to_window(incoming, WindowTarget::Dock { window, placement }).unwrap();
        assert_eq!(views.views_of(window).unwrap().tiles(), Some(&layout));
        assert_eq!(views.views_of(window).unwrap().active().id, destination);
        assert_eq!(views.window_of(incoming), Some(window));
        assert_eq!(views.window_of(ground), Some(0));

        views.move_to_window(incoming, WindowTarget::Dock { window: 0, placement: Placement::Replace(ground) }).unwrap();
        assert_eq!(views.active().id, incoming);
        assert_eq!(views.window_of(ground), Some(0));
        assert_eq!(views.views_of(window).unwrap().active().id, destination);
    }

    fn new_window() -> WindowTarget {
        WindowTarget::New { placement: None }
    }

    #[test]
    fn replacing_a_single_ground_shows_the_new_view_without_tiling() {
        let mut set = models(3);
        assert_eq!(set.preview_placement(2, Placement::Replace(3)), Some(Tiles::Leaf(2)));
        assert_eq!(set.preview_placement(2, Placement::Replace(1)), None);
        assert_eq!(set.preview_placement(2, Placement::Empty(0)), None);
        assert!(set.place_tile(2, Placement::Replace(3)));
        assert_eq!(set.active().id, 2);
        assert!(set.tiles().is_none());
        assert_eq!(set.views().iter().map(|view| view.id).collect::<Vec<_>>(), [1, 2, 3]);
    }

    #[test]
    fn window_rect_uses_the_grab_point_or_centres_without_a_pointer() {
        let display = Rect { x: -1600.0, y: 40.0, w: 1600.0, h: 960.0 };
        let source = Rect { x: 10.0, y: 20.0, w: 1200.0, h: 800.0 };
        let cell = Rect { x: 12.0, y: 80.0, w: 800.0, h: 500.0 };
        assert_eq!(
            window_rect_for(source, Some(cell), display, Some([-1100.0, 400.0]), true),
            Some(Rect { x: -1124.0, y: 386.0, w: 800.0, h: 500.0 }),
        );
        assert_eq!(window_rect_for(source, None, display, None, true), Some(Rect { x: -1160.0, y: 280.0, w: 720.0, h: 480.0 }));
    }

    #[test]
    fn window_rect_snaps_to_the_work_area_and_alt_leaves_the_grab_point() {
        let display = Rect { x: 0.0, y: 28.0, w: 1200.0, h: 800.0 };
        let source = Rect { x: 0.0, y: 0.0, w: 640.0, h: 420.0 };
        let pointer = Some([574.0, 410.0]);
        assert_eq!(window_rect_for(source, Some(source), display, pointer, true), Some(Rect { x: 560.0, y: 408.0, w: 640.0, h: 420.0 }),);
        assert_eq!(window_rect_for(source, Some(source), display, pointer, false), Some(Rect { x: 550.0, y: 396.0, w: 640.0, h: 420.0 }),);
        assert_eq!(window_rect_for(source, Some(source), display, Some([0.0, 28.0]), false), Some(Rect { x: 0.0, y: 28.0, w: 640.0, h: 420.0 }),);
    }

    #[test]
    fn window_rect_recomputes_the_clamp_for_each_display_and_handles_hostile_input() {
        let source = Rect { x: 0.0, y: 0.0, w: 1150.0, h: 1000.0 };
        let small_display = Rect { x: 1200.0, y: 0.0, w: 1000.0, h: 700.0 };
        let large_display = Rect { x: 0.0, y: -800.0, w: 1200.0, h: 800.0 };
        assert_eq!(
            window_rect_for(source, Some(source), small_display, Some([2200.0, 700.0]), true),
            Some(Rect { x: 1300.0, y: 70.0, w: 900.0, h: 630.0 }),
        );
        assert_eq!(
            window_rect_for(source, Some(source), large_display, Some([1200.0, 0.0]), true),
            Some(Rect { x: 120.0, y: -720.0, w: 1080.0, h: 720.0 }),
        );
        let tiny_display = Rect { w: 200.0, h: 100.0, ..large_display };
        assert_eq!(window_rect_for(source, None, tiny_display, None, false), Some(Rect { x: 10.0, y: -795.0, w: 180.0, h: 90.0 }),);
        assert_eq!(window_rect_for(source, None, tiny_display, Some([f32::NAN, 0.0]), false), None);
        assert_eq!(window_rect_for(source, None, Rect { w: -30.0, ..small_display }, None, false), None);
    }

    #[test]
    fn a_pop_out_name_round_trips_without_renaming_its_views() {
        let mut set = models(2);
        let moved = set.move_to_window(2, new_window()).unwrap();
        let child = &mut set.window_mut(moved.window).unwrap().views;
        let view_name = child.active().name.clone();
        assert!(child.rename_window(" Inspection "));
        assert!(!child.rename_window(" "));
        let restored = ViewSet::from_json(&set.to_json()).unwrap();
        let child = restored.views_of(moved.window).unwrap();
        assert_eq!(child.window_name(), Some("Inspection"));
        assert_eq!(child.active().name, view_name);
        assert_eq!(restored.window_name(), None);
    }

    #[test]
    fn moving_the_last_view_out_of_main_is_refused() {
        let mut set = ViewSet::new();
        assert_eq!(set.move_to_window(1, new_window()), Err(MoveError::LastViewOfMainWindow));
        assert_eq!(set.move_to_window(u64::MAX, new_window()), Err(MoveError::NoSuchView));
        assert_eq!(set, ViewSet::new());
    }

    #[test]
    fn a_view_is_in_exactly_one_window_and_keeps_its_spec() {
        let mut set = models(3);
        let spec = ViewSpec::new(solidcraft_viewport::Camera { yaw: 0.5, ..Default::default() });
        assert!(set.set_spec(3, spec));
        set.set_layout(Preset::LargeAndTwo);
        let moved = set.move_to_window(3, new_window()).unwrap();
        assert!(set.check());
        assert_eq!((moved.view, moved.from, moved.created, moved.closed_source), (3, 0, true, false));
        let child = set.views_of(moved.window).unwrap();
        assert_eq!(child.active().spec, Some(spec));
        assert_eq!(child.active().id, 3);
        assert!(child.tiles().is_none());
        assert!(set.get(3).is_none());
        assert_eq!(set.window_of(3), Some(moved.window));
        assert!(!set.tiles().unwrap().contains(3));
    }

    fn existing_window(window: WindowId, index: Option<usize>) -> WindowTarget {
        WindowTarget::Existing { window, index }
    }

    #[test]
    fn moving_between_strips_inserts_at_the_drop_position_and_closes_an_empty_popout() {
        let mut set = models(6);
        let first = set.move_to_window(2, new_window()).unwrap().window;
        let second = set.move_to_window(4, new_window()).unwrap().window;
        let view = set.get(3).unwrap().clone();
        set.move_to_window(5, existing_window(second, None)).unwrap();
        set.move_to_window(view.id, existing_window(second, Some(1))).unwrap();
        let strip_ids = |set: &ViewSet, window| set.views_of(window).unwrap().views().iter().map(|view| view.id).collect::<Vec<_>>();
        assert_eq!(strip_ids(&set, second), [4, 3, 5]);
        assert_eq!(set.views_of(second).unwrap().active(), &view);
        set.move_to_window(4, existing_window(second, Some(usize::MAX))).unwrap();
        assert_eq!(strip_ids(&set, second), [3, 5, 4]);
        let before = set.clone();
        set.move_to_window(5, existing_window(second, None)).unwrap();
        assert_eq!(set, before);

        let returned = set.move_to_window(2, existing_window(0, Some(1))).unwrap();
        assert_eq!((returned.from, returned.window, returned.created, returned.closed_source), (first, 0, false, true));
        assert_eq!(strip_ids(&set, 0), [1, 2, 6]);
        assert!(set.window(first).is_none());
        assert_eq!(strip_ids(&set, second), [3, 5, 4]);
        assert!(set.check());
    }

    #[test]
    fn moving_between_popouts_keeps_each_layout_and_closed_view_history() {
        let mut set = models(6);
        let first = set.move_to_window(2, new_window()).unwrap().window;
        let second = set.move_to_window(3, new_window()).unwrap().window;
        set.move_to_window(4, existing_window(first, None)).unwrap();
        set.move_to_window(5, existing_window(second, None)).unwrap();
        set.set_layout_padded(Preset::SideBySide);
        let main_layout = set.tiles().cloned();
        let mut target = set.take_window(second).unwrap();
        target.views.set_layout_padded(Preset::LargeAndTwo);
        set.put_window(target);

        assert!(set.close_in_window(first, 2));
        let moved = set.move_to_window(4, existing_window(second, None)).unwrap();
        assert!(moved.closed_source);
        assert_eq!(set.tiles(), main_layout.as_ref());
        assert_eq!(set.views_of(second).unwrap().tiles().unwrap().views(), [3, 5, 4]);
        assert_eq!(set.reopen(), Some(2));
        assert_eq!(set.window_of(2), Some(0));
        assert!(set.check());
    }

    #[test]
    fn extent_and_window_id_repair_keep_all_views_and_refuse_a_ninth_popout() {
        let mut set = models(MAX_POPOUTS + 2);
        for view in 2..=MAX_POPOUTS as u64 + 1 {
            set.move_to_window(view, new_window()).unwrap();
        }
        let before = set.to_json();
        let last = MAX_POPOUTS as u64 + 2;
        assert_eq!(set.move_to_window(last, new_window()), Err(MoveError::TooManyWindows));
        assert_eq!(set.move_to_window(last, existing_window(9000, None)), Err(MoveError::NoSuchWindow));
        assert_eq!(set.to_json(), before);

        let mut saved = before.clone();
        saved["views"].as_array_mut().unwrap().iter_mut().find(|view| view["id"] == last).unwrap()["window"] = json!(9000);
        saved["windows"].as_array_mut().unwrap().push(json!({"id": 9000}));
        let repaired = ViewSet::from_json(&saved).unwrap();
        assert_eq!(repaired.to_json(), before);
        assert!(repaired.check());

        let mut saved = before;
        saved["next_window"] = json!(WindowId::MAX);
        let mut set = ViewSet::from_json(&saved).unwrap();
        set.close_window(1).unwrap();
        assert_eq!(set.move_to_window(2, new_window()).unwrap().window, WindowId::MAX);
        set.close_window(2).unwrap();
        assert_eq!(set.move_to_window(3, new_window()).unwrap().window, 1);
        assert!(set.check());
    }

    #[test]
    fn closing_a_window_returns_its_views_in_order_and_undo_uses_current_views() {
        let mut set = models(2);
        let id = set.move_to_window(2, new_window()).unwrap().window;
        let mut child = set.take_window(id).unwrap();
        let other = child.views.open(ViewKind::Model, None, false);
        assert_eq!(child.views.set_layout(Preset::SideBySide), 2);
        set.put_window(child);
        set.set_layout_padded(Preset::SideBySide);
        set.window_mut(id).unwrap().options.card_headers = crate::CardHeaders::Hidden;
        let child = set.window(id).unwrap().clone();
        assert_eq!(set.close_window(id), Some(vec![2, other]));
        assert_eq!(set.views().iter().map(|view| view.id).collect::<Vec<_>>(), [1, 2, other]);
        assert_eq!(set.tiles().unwrap().views(), [1, 2]);
        assert!(set.rename(2, "Returned name"));
        assert!(set.restore_window(child));
        assert_eq!(set.window(id).unwrap().options.card_headers, crate::CardHeaders::Hidden);
        assert_eq!(set.views_of(id).unwrap().get(2).unwrap().name, "Returned name");
        assert_eq!(set.views_of(id).unwrap().tiles().unwrap().views(), [2, other]);
        assert!(set.check());
    }

    #[test]
    fn closing_the_last_popout_view_keeps_it_reopenable_in_main() {
        let mut set = models(2);
        let id = set.move_to_window(2, new_window()).unwrap().window;
        assert!(!set.close_in_window(id, 1));
        assert!(set.close_in_window(id, 2));
        assert_eq!(set.window_of(2), None);
        assert!(set.window(id).is_none());
        assert_eq!(set.reopen_in_window(id), None);
        assert!(set.has_open_or_closed(2));
        assert_eq!(set.reopen_in_window(0), Some(2));
        assert_eq!(set.window_of(2), Some(0));
        assert!(set.check());
    }

    #[test]
    fn a_window_round_trips_and_unknown_membership_and_geometry_are_repaired() {
        let mut set = models(3);
        let placement =
            WindowPlacement { display: Some("Display 2".into()), x: -25.0, y: 16.0, w: 700.0, h: 500.0, maximised: false, fullscreen: false };
        let id = set.move_to_window(2, WindowTarget::New { placement: Some(placement) }).unwrap().window;
        let options = crate::WindowOptions {
            card_headers: crate::CardHeaders::Hover,
            keep_on_top: true,
            interface_size: crate::InterfaceSize::try_from(150).unwrap(),
            ..crate::WindowOptions::new_pop_out()
        };
        let child = set.window_mut(id).unwrap();
        child.options = options;
        child.panel_layout = Some(json!({"catalogue":"opaque workspace positions"}));
        let mut saved = set.to_json();
        let restored = ViewSet::from_json(&saved).unwrap();
        assert_eq!(restored.to_json(), saved);
        assert_eq!(restored.window(id).unwrap().options, options);
        assert_eq!(restored.window(id).unwrap().panel_layout, Some(json!({"catalogue":"opaque workspace positions"})));
        saved["windows"][0]["options"] = json!({"card_headers":"hover","keep_on_top":true});
        let old_options = ViewSet::from_json(&saved).unwrap().window(id).unwrap().options;
        assert!(!old_options.ribbon && old_options.ribbon_auto_hide);
        assert_eq!(old_options.card_headers, crate::CardHeaders::Hover);
        assert_eq!(old_options.interface_size, crate::InterfaceSize::default());
        saved["windows"][0].as_object_mut().unwrap().remove("options");
        saved["windows"][0]["panel_layout"] = json!({"catalogue": []});
        saved["windows"][0]["active"] = json!(5);
        saved["windows"][0]["level"] = json!("unknown future mode");
        saved["windows"][0]["placement"]["h"] = json!(-5);
        saved["views"][0]["window"] = json!(9000);
        let repaired = ViewSet::from_json(&saved).unwrap();
        assert_eq!(repaired.window_of(1), Some(0));
        assert_eq!(repaired.views_of(id).unwrap().active().id, 2);
        assert_eq!(repaired.window(id).unwrap().level, WindowLevel::ViewsOnly);
        assert_eq!(repaired.window(id).unwrap().options, crate::WindowOptions::default());
        assert_eq!(repaired.window(id).unwrap().placement, None);
        assert_eq!(repaired.window(id).unwrap().panel_layout, None);
        assert!(repaired.check());
    }

    #[test]
    fn moving_closing_and_reopening_keep_window_membership_consistent() {
        let mut set = models(3);
        let mut step = 42_u64;
        for _ in 0..300 {
            step = step.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let window = set.windows().first().map(|window| window.id);
            let active = set.active().id;
            match (step >> 16) % 10 {
                0 => {
                    set.open(ViewKind::Model, None, false);
                }
                1 => {
                    let _ = set.move_to_window(active, new_window());
                }
                2 if let Some(window) = window => {
                    set.close_window(window);
                }
                3 if let Some(window) = window => {
                    let view = set.views_of(window).unwrap().active().id;
                    set.close_in_window(window, view);
                }
                4 => {
                    set.close(active);
                }
                5 => {
                    set.reopen();
                }
                6 if let Some(window) = window => {
                    let mut child = set.take_window(window).unwrap();
                    child.views.open(ViewKind::Model, None, false);
                    set.put_window(child);
                }
                7 if let Some(window) = window => {
                    let _ = set.move_to_window(active, existing_window(window, Some(0)));
                }
                8 if let Some(window) = window => {
                    let view = set.views_of(window).unwrap().active().id;
                    let _ = set.move_to_window(view, existing_window(0, Some(0)));
                }
                _ => {
                    set.set_layout_padded(Preset::SideBySide);
                }
            }
            assert!(set.check());
            assert!(ViewSet::from_json(&set.to_json()).unwrap().check());
        }
    }

    #[test]
    fn a_new_set_is_one_active_model_view() {
        let set = ViewSet::new();
        assert_eq!((set.len(), set.active().kind, set.active().name.as_str()), (1, ViewKind::Model, "Model"));
        assert_eq!(ViewSet::default(), set);
    }

    #[test]
    fn open_numbers_names_per_kind_and_activates() {
        let mut set = ViewSet::new();
        let a = set.open(ViewKind::Sheet, Some("a".into()), false);
        let b = set.open(ViewKind::Sheet, Some("b".into()), false);
        let r = set.open(ViewKind::Render, None, false);
        let m = set.open(ViewKind::Model, None, false);
        assert_eq!(names(&set), ["Model", "Sheet 1", "Sheet 2", "Render", "Model 2"]);
        assert_eq!(set.active().id, m);
        assert!(a != b && b != r && r != m);
        set.close(a);
        assert_eq!(set.open(ViewKind::Animation, None, false), set.active().id);
        assert_eq!(set.open(ViewKind::Sheet, Some("c".into()), false), set.active().id);
        assert_eq!(set.active().name, "Sheet 1", "a freed name is used again");
    }

    #[test]
    fn open_reuses_a_view_of_the_same_kind_and_subject_only_when_asked() {
        let mut set = ViewSet::new();
        let a = set.open(ViewKind::Sheet, Some("a".into()), true);
        set.open(ViewKind::Render, None, true);
        assert_eq!(set.open(ViewKind::Sheet, Some("a".into()), true), a);
        assert_eq!((set.len(), set.active().id), (3, a));
        assert_ne!(set.open(ViewKind::Sheet, Some("a".into()), false), a);
        assert_ne!(set.open(ViewKind::Sheet, Some("b".into()), true), a);
        assert_eq!(set.len(), 5);
        assert_eq!(set.open(ViewKind::Model, None, true), set.views()[0].id, "the first Model view is reused");
    }

    #[test]
    fn open_stops_at_the_most_views() {
        let mut set = sheets(MAX_VIEWS - 1);
        assert_eq!(set.len(), MAX_VIEWS);
        let active = set.active().id;
        assert_eq!(set.open(ViewKind::Render, None, false), active);
        assert_eq!(set.duplicate(active), None);
        assert_eq!(set.len(), MAX_VIEWS);
    }

    #[test]
    fn activate_by_id_and_by_index() {
        let mut set = sheets(2);
        let first = set.views()[0].id;
        assert!(set.activate(first) && set.active().id == first);
        assert!(set.activate_index(2) && set.active().id == set.views()[2].id);
        assert!(!set.activate(999) && !set.activate_index(3));
        assert_eq!(set.active().id, set.views()[2].id, "a refused activation changes nothing");
    }

    #[test]
    fn close_never_closes_the_last_view_and_moves_to_a_neighbour() {
        let mut set = ViewSet::new();
        let only = set.active().id;
        assert!(!set.close(only) && set.len() == 1);
        let a = set.open(ViewKind::Sheet, Some("a".into()), false);
        let b = set.open(ViewKind::Sheet, Some("b".into()), false);
        assert!(set.close(b));
        assert_eq!(set.active().id, a, "the left neighbour takes over");
        assert!(set.activate(only) && set.close(only));
        assert_eq!(set.active().id, a, "the first view closing hands over to the right");
        assert!(!set.close(only) && !set.close(12345), "unknown ids close nothing");
        assert!(!set.close(a), "the last one stays");
    }

    #[test]
    fn drop_views_removes_open_and_closed_views_and_never_empties_the_set() {
        let mut set = sheets(3);
        let ids: Vec<u64> = set.views().iter().map(|view| view.id).collect();
        assert!(set.close(ids[2]));
        assert!(set.activate(ids[3]));
        let keep = |view: &DesignView| view.subject.as_deref() != Some("s0") && view.subject.as_deref() != Some("s2");
        assert!(set.drop_views(keep));
        assert_eq!(names(&set), ["Model"], "s1 stays closed, s0 and s2 go");
        assert_eq!(set.active().id, ids[0], "the active view went, the kept view left of it takes over");
        assert_eq!(set.reopen(), Some(ids[2]), "a closed view that is kept can still come back");
        assert!(set.drop_views(|view| view.kind == ViewKind::Model));
        assert!(!set.can_reopen(), "a dropped view never comes back");
        assert!(!set.drop_views(|view| view.kind == ViewKind::Model), "nothing to drop");
        assert!(set.drop_views(|_| false));
        assert_eq!((set.len(), set.active().kind), (1, ViewKind::Model), "with nothing kept a Model view stays");
    }

    #[test]
    fn closing_an_inactive_view_keeps_the_active_one() {
        let mut set = sheets(2);
        let (first, last) = (set.views()[0].id, set.views()[2].id);
        assert!(set.close(first));
        assert_eq!(set.active().id, last);
    }

    #[test]
    fn compass_visibility_is_saved_per_view_and_defaults_to_on() {
        let mut set = ViewSet::new();
        let first = set.active().id;
        let second = set.open(ViewKind::Model, None, false);
        assert!(set.set_compass(first, false));
        let mut saved = set.to_json();
        assert_eq!(ViewSet::from_json(&saved).as_ref(), Some(&set));
        assert!(!set.get(first).unwrap().compass);
        assert!(set.get(second).unwrap().compass);
        saved["views"][0].as_object_mut().unwrap().remove("compass");
        assert!(ViewSet::from_json(&saved).unwrap().get(first).unwrap().compass);
    }

    #[test]
    fn close_others_and_close_right_of() {
        let mut set = sheets(4);
        let ids: Vec<u64> = set.views().iter().map(|view| view.id).collect();
        set.close_right_of(ids[2]);
        assert_eq!(set.views().iter().map(|view| view.id).collect::<Vec<_>>(), &ids[..3], "the active one went: the pivot takes over");
        assert_eq!(set.active().id, ids[2]);
        assert!(set.activate(ids[0]));
        set.close_right_of(ids[1]);
        assert_eq!((set.len(), set.active().id), (2, ids[0]), "an active view that stays stays");
        set.close_right_of(999);
        assert_eq!(set.len(), 2);
        set.close_others(ids[1]);
        assert_eq!((set.len(), set.active().id), (1, ids[1]));
        set.close_others(999);
        assert_eq!(set.len(), 1);
        assert_eq!(set.reopen(), Some(ids[0]), "everything closed can come back, latest first");
    }

    #[test]
    fn reopen_restores_the_latest_closed_view_in_its_place() {
        let mut set = sheets(3);
        let ids: Vec<u64> = set.views().iter().map(|view| view.id).collect();
        assert!(!set.can_reopen());
        set.close(ids[1]);
        set.close(ids[3]);
        assert!(set.can_reopen());
        assert_eq!(set.reopen(), Some(ids[3]));
        assert_eq!(set.active().id, ids[3]);
        assert_eq!(set.reopen(), Some(ids[1]));
        assert_eq!(set.views().iter().map(|view| view.id).collect::<Vec<_>>(), ids);
        assert_eq!(set.reopen(), None);
    }

    #[test]
    fn the_closed_stack_keeps_the_latest_sixteen() {
        let mut set = sheets(20);
        let ids: Vec<u64> = set.views().iter().map(|view| view.id).collect();
        for id in &ids[1..] {
            assert!(set.close(*id));
        }
        let mut back = 0;
        while set.reopen().is_some() {
            back += 1;
        }
        assert_eq!((back, set.len()), (CLOSED_CAP, CLOSED_CAP + 1));
        assert!(set.get(ids[1]).is_none() && set.get(ids[20]).is_some(), "the oldest closes were forgotten");
    }

    #[test]
    fn rename_trims_and_limits() {
        let mut set = ViewSet::new();
        let id = set.active().id;
        assert!(set.rename(id, "  Front  "));
        assert_eq!(set.active().name, "Front");
        assert!(set.rename(id, &"é".repeat(MAX_NAME)));
        let long = "x".repeat(MAX_NAME + 1);
        for bad in ["", "   ", "a\nb", "tab\there", long.as_str()] {
            assert!(!set.rename(id, bad), "{bad:?}");
        }
        assert_eq!(set.active().name, "é".repeat(MAX_NAME));
        assert!(!set.rename(999, "Name"));
    }

    #[test]
    fn duplicate_goes_next_to_the_original_with_a_new_name() {
        let mut set = sheets(1);
        let (model, sheet) = (set.views()[0].id, set.views()[1].id);
        let copy = set.duplicate(sheet).unwrap();
        assert_eq!(set.active().id, copy);
        assert_eq!(names(&set), ["Model", "Sheet 1", "Sheet 2"]);
        assert_eq!(set.views()[2].subject, set.views()[1].subject);
        let model_copy = set.duplicate(model).unwrap();
        assert_eq!(set.index_of(model_copy), Some(1));
        assert_eq!(set.get(model_copy).map(|view| view.name.as_str()), Some("Model 2"));
        assert_eq!(set.duplicate(999), None);
    }

    #[test]
    fn move_view_reorders_and_keeps_the_active_view() {
        let mut set = sheets(3);
        let ids: Vec<u64> = set.views().iter().map(|view| view.id).collect();
        set.activate(ids[1]);
        set.move_view(0, 3);
        assert_eq!(set.views().iter().map(|view| view.id).collect::<Vec<_>>(), [ids[1], ids[2], ids[3], ids[0]]);
        assert_eq!(set.active().id, ids[1]);
        set.move_view(3, 0);
        assert_eq!(set.views().iter().map(|view| view.id).collect::<Vec<_>>(), ids);
        for (from, to) in [(0, 4), (4, 0), (9, 9), (2, 2)] {
            set.move_view(from, to);
        }
        assert_eq!(set.views().iter().map(|view| view.id).collect::<Vec<_>>(), ids);
    }

    #[test]
    fn a_mutable_child_borrow_observes_the_roots_final_allocated_id() {
        let mut set = models(2);
        let remote = set.active().id;
        let window = set.move_to_window(remote, new_window()).unwrap().window;
        set.next_id = MAX_ID;
        assert_eq!(set.open(ViewKind::Model, None, false), MAX_ID);
        let child = set.window_mut(window).unwrap();
        assert_eq!(child.views.next_id(), MAX_ID + 1);
        assert_eq!(child.views.open(ViewKind::Model, None, false), remote);
        assert_eq!(child.views.duplicate(remote), None);
        assert_eq!(set.window_of(MAX_ID), Some(0));
        assert_eq!(set.window_of(remote), Some(window));
        assert_eq!(ViewSet::from_json(&set.to_json()).unwrap().to_json(), set.to_json());
    }
    #[test]
    fn exhausted_global_ids_refuse_fallback_before_windows_or_history_change() {
        let mut set = models(3);
        let remote = set.active().id;
        let window = set.move_to_window(remote, new_window()).unwrap().window;
        assert!(set.close(2));
        let mut saved = set.to_json();
        saved["next_id"] = json!(MAX_ID + 1);
        let mut set = ViewSet::from_json(&saved).unwrap();
        let before = set.clone();
        for keep_remote in [false, true] {
            assert!(!set.drop_views(|view| keep_remote && view.id == remote));
            assert_eq!(set, before, "a refused fallback keeps windows and closed histories intact");
            assert_eq!(set.window_of(remote), Some(window));
        }
        assert_eq!(ViewSet::from_json(&set.to_json()).unwrap().to_json(), set.to_json());
    }

    #[test]
    fn exhausted_global_ids_still_allow_cleanup_that_keeps_a_main_view() {
        let mut set = models(2);
        let main = set.views()[0].id;
        let remote = set.active().id;
        set.move_to_window(remote, new_window()).unwrap();
        let mut saved = set.to_json();
        saved["next_id"] = json!(MAX_ID + 1);
        let mut set = ViewSet::from_json(&saved).unwrap();
        assert!(set.drop_views(|view| view.id == main));
        assert_eq!(set.views().iter().map(|view| view.id).collect::<Vec<_>>(), [main]);
        assert!(set.windows().is_empty());
        assert_eq!(set.next_id(), MAX_ID + 1);
        assert_eq!(set.open(ViewKind::Model, None, false), main);
        assert_eq!(set.duplicate(main), None);
        assert_eq!(ViewSet::from_json(&set.to_json()).unwrap(), set);
    }

    #[test]
    fn a_maximum_id_loaded_in_another_window_exhausts_all_allocators() {
        let mut set = models(2);
        let main = set.views()[0].id;
        let remote = set.active().id;
        let window = set.move_to_window(remote, new_window()).unwrap().window;
        let mut saved = set.to_json();
        saved["views"][1]["id"] = json!(MAX_ID);
        saved["windows"][0]["active"] = json!(MAX_ID);
        saved["next_id"] = json!(0);
        let mut set = ViewSet::from_json(&saved).unwrap();
        let before = set.to_json();
        assert_eq!(set.next_id(), MAX_ID + 1);
        assert_eq!(set.open(ViewKind::Model, None, false), main);
        assert_eq!(set.duplicate(main), None);
        let mut child = set.take_window(window).unwrap();
        assert_eq!(child.views.open(ViewKind::Model, None, false), MAX_ID);
        assert_eq!(child.views.duplicate(MAX_ID), None);
        assert_eq!(child.views.detach(MAX_ID), None, "failed fallback creation cannot empty a detached strip");
        assert!(!child.views.drop_views(|_| false));
        set.put_window(child);
        assert_eq!(set.to_json(), before);
        assert_eq!(ViewSet::from_json(&set.to_json()).unwrap().to_json(), before);
    }

    #[test]
    fn last_legal_fallback_uses_the_highest_window_allocation_watermark() {
        let mut set = models(2);
        let remote = set.active().id;
        let window = set.move_to_window(remote, new_window()).unwrap().window;
        set.next_id = MAX_ID - 1;
        set.window_mut(window).unwrap().views.next_id = MAX_ID;
        assert_eq!(set.next_id(), MAX_ID);
        assert!(set.drop_views(|view| view.id == remote));
        assert_eq!(set.active().id, MAX_ID);
        assert_eq!(set.active().kind, ViewKind::Model);
        assert_eq!(set.active().spec, None);
        assert_eq!(set.window_of(remote), Some(window));
        assert_eq!(set.next_id(), MAX_ID + 1);
        assert_eq!(set.open(ViewKind::Model, None, false), MAX_ID);
        assert_eq!(set.duplicate(MAX_ID), None);
        assert_eq!(ViewSet::from_json(&set.to_json()).unwrap().to_json(), set.to_json());
    }

    #[test]
    fn exhausted_saved_ids_never_allocate_an_unsaveable_view_or_empty_the_set() {
        let mut saved = ViewSet::new().to_json();
        saved["next_id"] = json!(MAX_ID);
        let mut set = ViewSet::from_json(&saved).unwrap();
        let last = set.open(ViewKind::Model, None, false);
        assert_eq!(last, MAX_ID, "the final exact id remains usable");
        assert_eq!(ViewSet::from_json(&set.to_json()).unwrap(), set);
        let exhausted = set.clone();
        assert_eq!(set.open(ViewKind::Sheet, None, false), last);
        assert_eq!(set.duplicate(1), None);
        assert!(!set.drop_views(|_| false), "a fresh fallback identity cannot be allocated");
        assert_eq!(set, exhausted, "refusal happens before any open or closed view changes");
        assert!(set.close(last));
        assert_eq!(set.reopen(), Some(last), "existing identities can still be reopened");
        assert_eq!(ViewSet::from_json(&set.to_json()).unwrap(), set);

        saved["views"][0]["id"] = json!(MAX_ID);
        saved["active"] = json!(MAX_ID);
        saved["next_id"] = json!(u64::MAX);
        let mut set = ViewSet::from_json(&saved).unwrap();
        assert_eq!(set.open(ViewKind::Model, None, false), MAX_ID);
        assert_eq!(set.duplicate(MAX_ID), None);
        assert_eq!(ViewSet::from_json(&set.to_json()).unwrap(), set);
    }

    #[test]
    fn subjects_opened_at_the_byte_limit_round_trip_and_oversized_subjects_are_refused() {
        let mut set = ViewSet::new();
        let subject = "é".repeat(MAX_SUBJECT / 2);
        let id = set.open(ViewKind::Sheet, Some(subject.clone()), false);
        assert_eq!(set.get(id).unwrap().subject.as_ref(), Some(&subject));
        assert_eq!(ViewSet::from_json(&set.to_json()).unwrap(), set);
        let before = set.clone();
        for reuse in [false, true] {
            assert_eq!(set.open(ViewKind::Sheet, Some(format!("{subject}x")), reuse), id);
            assert_eq!(set, before, "the subject is not truncated or admitted into unsaveable state");
        }
    }

    #[test]
    fn hostile_closed_history_reads_only_the_bounded_latest_tail() {
        let mut saved = ViewSet::new().to_json();
        let entry = |id| json!({"at": 1, "view": {"id": id, "kind": "model", "name": "Closed"}});
        let mut history = vec![entry(MAX_ID); 4096];
        let history_limit = (CLOSED_CAP * (1 + MAX_POPOUTS)) as u64;
        history.extend((2..=history_limit + 1).map(entry));
        saved["closed"] = json!(history);
        saved["next_id"] = json!(0);
        let mut set = ViewSet::from_json(&saved).unwrap();
        assert_eq!(set.closed.len(), CLOSED_CAP);
        assert_eq!(set.next_id, history_limit + 2, "discarded history cannot affect allocation");
        assert_eq!(ViewSet::from_json(&set.to_json()).unwrap(), set);
        for id in (history_limit + 2 - CLOSED_CAP as u64..=history_limit + 1).rev() {
            assert_eq!(set.reopen(), Some(id), "the latest valid history keeps its original order");
        }
        assert_eq!(set.reopen(), None);
    }
    #[test]
    fn json_round_trips_views_active_closed_and_ids() {
        let mut set = sheets(3);
        set.open(ViewKind::Render, None, false);
        let ids: Vec<u64> = set.views().iter().map(|view| view.id).collect();
        set.rename(ids[2], "Profile");
        set.close(ids[1]);
        set.activate(ids[3]);
        let back = ViewSet::from_json(&set.to_json()).unwrap();
        assert_eq!(back, set);
        let mut back = back;
        assert_eq!(back.reopen(), Some(ids[1]));
        assert!(back.open(ViewKind::Sheet, None, false) > ids[4], "ids stay unique after a reload");
    }

    #[test]
    fn hostile_json_is_refused_or_repaired_never_a_panic() {
        let view = |id: Value, kind: &str, name: Value| json!({"id": id, "kind": kind, "name": name});
        for bad in [
            Value::Null,
            json!(5),
            json!([]),
            json!({}),
            json!({"views": []}),
            json!({"views": "x"}),
            json!({"views": [view(json!(1), "bogus", json!("A"))]}),
            json!({"views": [view(json!(0), "model", json!("A"))]}),
            json!({"views": [view(json!(-1), "model", json!("A"))]}),
            json!({"views": [view(json!(1.5), "model", json!("A"))]}),
            json!({"views": [view(json!(u64::MAX), "model", json!("A"))]}),
            json!({"views": [view(json!(1), "model", json!(""))]}),
            json!({"views": [view(json!(1), "model", json!(7))]}),
            json!({"views": [view(json!(1), "model", json!("x".repeat(65)))]}),
            json!({"views": [view(json!(1), "model", json!("A")), view(json!(1), "sheet", json!("B"))]}),
            json!({"views": [{"id": 1, "kind": "model", "name": "A", "subject": 3}]}),
            json!({"views": [{"id": 1, "kind": "model", "name": "A", "subject": "s".repeat(257)}]}),
            json!({"views": vec![view(json!(1), "model", json!("A")); MAX_VIEWS + 1]}),
        ] {
            assert!(ViewSet::from_json(&bad).is_none(), "{bad}");
        }
        let repaired = ViewSet::from_json(&json!({
            "active": "nope", "next_id": 0,
            "views": [view(json!(5), "model", json!("A")), view(json!(9), "sheet", json!(" B "))],
            "closed": [
                {"at": u64::MAX, "view": view(json!(5), "model", json!("dup"))},
                {"at": 1, "view": view(json!(2), "bogus", json!("bad"))},
                {"at": "x", "view": view(json!(3), "render", json!("R"))},
                {"at": 7, "view": view(json!(3), "render", json!("R"))},
                17,
            ],
        }))
        .unwrap();
        assert_eq!((repaired.active().id, names(&repaired)), (5, vec!["A", "B"]));
        let mut repaired = repaired;
        assert_eq!(repaired.reopen(), Some(3), "only the sound closed view is kept");
        assert_eq!(repaired.index_of(3), Some(2));
        assert_eq!(repaired.reopen(), None);
        assert!(repaired.open(ViewKind::Sheet, None, false) > 9);
    }
    #[test]
    fn a_views_spec_is_kept_copied_and_saved() {
        let mut set = ViewSet::new();
        let first = set.active().id;
        let camera = solidcraft_viewport::Camera { yaw: 0.5, ..Default::default() };
        assert!(set.set_spec(first, ViewSpec::new(camera)));
        assert!(!set.set_spec(99, ViewSpec::new(camera)));
        let copy = set.duplicate(first).unwrap();
        assert_eq!(set.get(copy).unwrap().spec, Some(ViewSpec::new(camera)));
        let back = ViewSet::from_json(&set.to_json()).unwrap();
        assert_eq!(back, set);
    }

    #[test]
    fn a_saved_spec_with_unusable_numbers_is_dropped_not_drawn() {
        let mut saved = ViewSet::new().to_json();
        let camera = solidcraft_viewport::Camera { distance: -5.0, ..Default::default() };
        saved["views"][0]["spec"] = serde_json::to_value(ViewSpec::new(camera)).unwrap();
        assert_eq!(ViewSet::from_json(&saved).unwrap().active().spec, None);
        saved["views"][0]["spec"] = json!("nonsense");
        assert_eq!(ViewSet::from_json(&saved).unwrap().active().spec, None);
    }

    fn models(count: usize) -> ViewSet {
        let mut set = ViewSet::new();
        for _ in 1..count {
            set.open(ViewKind::Model, None, false);
        }
        set
    }

    #[test]
    fn a_layout_shows_model_and_sheet_views_in_tab_order() {
        let mut set = models(3);
        set.open(ViewKind::Sheet, None, false);
        set.activate_index(1);
        assert_eq!(set.set_layout(Preset::Grid), 4, "three Model views and a Sheet fill the grid");
        assert!(set.is_tiled());
        assert_eq!(set.tiles().unwrap().views().len(), 4);
        assert_eq!(set.set_layout(Preset::SideBySide), 2);
        assert!(set.tiles().unwrap().contains(set.active().id), "the active view is always among the shown");
        assert_eq!(set.set_layout(Preset::Single), 1);
        assert!(set.tiles().is_none() && !set.is_tiled());
    }

    #[test]
    fn render_and_model_share_presets_and_picker_cells() {
        let mut set = models(2);
        let first = set.open(ViewKind::Render, None, false);
        let second = set.duplicate(first).unwrap();
        assert_eq!(set.set_layout(Preset::Grid), 4);
        assert!(set.tiles().unwrap().contains(second));
        assert!(set.tiles().unwrap().contains(first));
        set.activate_index(0);
        assert!(!set.show_beside(first, Edge::Bottom), "a view already shown is not duplicated as a tile");
        set.untile();
        assert!(set.show_beside(first, Edge::Right));
        set.activate(first);
        assert_eq!(set.set_layout_padded(Preset::Grid), 4);
        assert!(set.tiles().unwrap().contains(second), "both Render views can occupy the grid");
        assert_eq!(ViewSet::from_json(&set.to_json()), Some(set.clone()));
        assert!(set.activate_index(0));
        assert_eq!(set.set_layout(Preset::SideBySide), 2);
        assert!(!set.tiles().unwrap().contains(first), "the preset uses tab order, with no kind exception");
        set.untile();
        assert_eq!(set.set_layout(Preset::SideBySide), 2);
        assert!(!set.tiles().unwrap().contains(first), "a hidden Render tab does not replace a Model preset angle");
    }

    #[test]
    fn a_view_taken_out_of_the_layout_stays_open() {
        let mut set = models(3);
        set.set_layout(Preset::Grid);
        let other = set.views().iter().map(|view| view.id).find(|id| *id != set.active().id).unwrap();
        assert!(set.unshow(other));
        assert_eq!(set.len(), 3, "no view closes");
        assert!(!set.tiles().unwrap().contains(other));
        let last = set.tiles().unwrap().views().into_iter().find(|id| *id != set.active().id).unwrap();
        assert!(set.unshow(last));
        assert!(!set.is_tiled(), "under two views there is no layout");
        assert!(!set.unshow(last), "it is not shown any more");
    }

    #[test]
    fn sheet_views_share_layouts_and_one_sheet_can_have_two_views() {
        let mut set = models(2);
        let sheet = set.open(ViewKind::Sheet, Some("drawing/sheet".into()), false);
        let other = set.open(ViewKind::Sheet, Some("drawing/sheet".into()), false);
        assert_ne!(sheet, other);
        assert_eq!(set.set_layout(Preset::Grid), 4);
        assert!(set.tiles().unwrap().contains(sheet) && set.tiles().unwrap().contains(other));
        set.activate_index(0);
        set.untile();
        set.set_layout(Preset::SideBySide);
        set.activate(sheet);
        assert!(!set.is_tiled(), "a hidden view stays open without destroying the layout");
        set.activate_index(0);
        assert!(set.is_tiled());
        assert!(set.show_beside(sheet, Edge::Right));
        assert_eq!(ViewSet::from_json(&set.to_json()), Some(set.clone()));
    }

    #[test]
    fn show_beside_splits_the_active_view_up_to_four() {
        let mut set = models(5);
        let ids: Vec<u64> = set.views().iter().map(|view| view.id).collect();
        set.activate(ids[0]);
        assert!(set.show_beside(ids[1], Edge::Right));
        assert!(set.show_beside(ids[2], Edge::Bottom));
        assert!(set.show_beside(ids[3], Edge::Left));
        assert!(!set.show_beside(ids[4], Edge::Left), "at most four views are shown");
        assert!(!set.show_beside(ids[1], Edge::Left), "a shown view is not shown twice");
        assert_eq!(set.tiles().unwrap().len(), 4);
    }

    #[test]
    fn closing_views_shrinks_the_layout_and_the_last_pair_ends_it() {
        let mut set = models(3);
        set.set_layout(Preset::LargeAndTwo);
        let ids: Vec<u64> = set.views().iter().map(|view| view.id).collect();
        assert!(set.close(ids[2]));
        assert_eq!(set.tiles().unwrap().len(), 2);
        assert!(set.close(ids[1]));
        assert!(set.tiles().is_none(), "one view left is no layout");
        let mut set = models(3);
        set.set_layout(Preset::LargeAndTwo);
        set.close_others(set.active().id);
        assert!(set.tiles().is_none());
    }

    #[test]
    fn a_layout_is_saved_and_a_stale_one_is_repaired() {
        let mut set = models(3);
        set.set_layout(Preset::LargeAndTwo);
        assert!(set.set_split(0, 0.7));
        assert_eq!(ViewSet::from_json(&set.to_json()).unwrap(), set);
        let mut saved = set.to_json();
        saved["tiles"] = Tiles::preset(Preset::SideBySide, &[1, 99]).unwrap().to_json();
        assert!(ViewSet::from_json(&saved).unwrap().tiles().is_none(), "a layout naming a view that is not there is dropped");
        saved["tiles"] = json!("nonsense");
        assert!(ViewSet::from_json(&saved).unwrap().tiles().is_none());
    }
}

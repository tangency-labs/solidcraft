//! The application bar's layout as pure arithmetic: which items fit, at which size, and which go
//! behind the overflow button. No egui types: the bar describes its items as a [`Spec`] (widths
//! in points) and [`layout`] answers with where each one goes ([`Placed`]).
//!
//! The bar is a row of three zones. Start items pack left to right from the left inset, End items
//! right to left from the right inset, and the Centre zone's one flexible item (the document tabs)
//! takes the room between them. Every item has a collapse ladder, its widths from fullest to
//! smallest. While the row is too wide, the lowest-priority item that can still give way steps
//! down its ladder, and an unlocked item already at its last step moves into overflow; the
//! overflow button then takes its own room, once. Locked items step down but never overflow. If
//! even that does not fit, the lowest-priority placed items are left out so nothing overlaps.
//! The application uses the same fit rule and uniform neighbour gap for every item; drawing
//! projection adds no per-item padding, group reservations or position offsets.
//!
//! The input is hostile: widths that are not finite or are negative count as zero, widths are
//! capped, repeated ids and items past [`MAX_ITEMS`] are ignored, and nothing here panics.

/// The most items the layout looks at; later ones are not placed.
pub const MAX_ITEMS: usize = 64;
/// The most ladder steps an item keeps.
pub const MAX_STEPS: usize = 8;
/// The largest width or inset taken; larger (or infinite) values are cut to it.
pub const MAX_WIDTH: f32 = 100_000.0;

/// The part of the bar an item sits in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Zone {
    /// Left-aligned, after the left caption inset.
    Start,
    /// The flexible middle: only its first item is used.
    Centre,
    /// Right-aligned, before the right caption inset.
    End,
}

/// One thing on the bar.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub zone: Zone,
    /// The widths it can be drawn at, fullest first (a label and icon, then the icon alone).
    /// A Centre item's last entry is its least width.
    pub ladder: Vec<f32>,
    /// Higher is given up later.
    pub priority: u8,
    /// Never moved to overflow (it still steps down its ladder).
    pub locked: bool,
    /// Retains a visible slot while still allowing the user to move it.
    pub never_overflow: bool,
    /// A Centre item takes all the room left; otherwise it keeps its least width.
    pub flexible: bool,
}

impl Item {
    /// An unlocked, fixed item.
    pub fn new(id: &str, zone: Zone, ladder: &[f32], priority: u8) -> Self {
        Self { id: id.to_owned(), zone, ladder: ladder.to_vec(), priority, locked: false, never_overflow: false, flexible: false }
    }

    /// This item locked.
    pub fn locked(mut self) -> Self {
        self.locked = true;
        self
    }

    /// This item flexible.
    pub fn flexible(mut self) -> Self {
        self.flexible = true;
        self
    }

    pub fn never_overflow(mut self) -> Self {
        self.never_overflow = true;
        self
    }
}

/// What to lay out: the bar's width, the room the window's caption buttons take at each end, the
/// gap between items, the overflow button's width and the items in source order.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub width: f32,
    pub captions_left: f32,
    pub captions_right: f32,
    pub gap: f32,
    pub overflow_button_width: f32,
    pub items: Vec<Item>,
    /// Drawing measurements for the application projection.
    pub application: Option<application::Policy>,
}

/// Where one item goes: its left edge, its width and the ladder step it is drawn at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub x: f32,
    pub width: f32,
    pub step: usize,
}

impl Placement {
    pub fn right(&self) -> f32 {
        self.x + self.width
    }
}

/// The answer.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Placed {
    /// One entry per item of the spec, in its order. None: behind the overflow button, or left
    /// out (a repeated id, past [`MAX_ITEMS`], or no room even for the locked items).
    pub placements: Vec<Option<Placement>>,
    /// Hidden ids: concession order for plain specs, bar order for application specs.
    pub overflow: Vec<String>,
    /// The overflow button's left edge and width: the leftmost End element. None without overflow.
    pub overflow_button: Option<(f32, f32)>,
    /// The Centre item's left edge and width.
    pub centre: Option<(f32, f32)>,
    /// Drawing geometry projected from the measured application slots.
    pub application: Option<application::Geometry>,
}

fn clean(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, MAX_WIDTH) }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Placed,
    Overflow,
    /// Not taken (repeated id, too many items, a second Centre item) or shed for lack of room.
    Out,
}

/// An item as the loop sees it.
struct Work {
    zone: Zone,
    /// Cleaned ladder: non-empty, never widening.
    ladder: Vec<f32>,
    priority: u8,
    locked: bool,
    flexible: bool,
    step: usize,
    state: State,
}

impl Work {
    fn width(&self) -> f32 {
        // A Centre item counts at its least width while the row is fitted.
        let step = if self.zone == Zone::Centre { self.ladder.len().saturating_sub(1) } else { self.step };
        self.ladder.get(step).copied().unwrap_or(0.0)
    }

    fn can_step(&self) -> bool {
        self.zone != Zone::Centre && self.step + 1 < self.ladder.len()
    }
}

fn card_item(item: &Item) -> bool {
    !item.locked && item.zone != Zone::Centre && !matches!(item.id.as_str(), "managed" | "offered")
}

fn total(work: &[Work], gap: f32, button: f32) -> f32 {
    let mut sum = 0.0_f32;
    let mut count = 0_usize;
    for w in work.iter().filter(|w| w.state == State::Placed) {
        sum += w.width();
        count += 1;
    }
    if work.iter().any(|w| w.state == State::Overflow) {
        sum += button;
        count += 1;
    }
    sum + gap * count.saturating_sub(1) as f32
}
/// Lays `spec` out. Deterministic: ties in priority go to the earlier item in source order.
pub fn layout(spec: &Spec) -> Placed {
    if let Some(policy) = &spec.application {
        return application::place(spec, policy);
    }
    layout_row(spec)
}

fn layout_row(spec: &Spec) -> Placed {
    let (width, left, right) = (clean(spec.width), clean(spec.captions_left), clean(spec.captions_right));
    let (gap, button) = (clean(spec.gap), clean(spec.overflow_button_width));
    // Application chrome is the next neighbour of the End stack. Its boundary uses
    // the same gap as logo/items/tabs; this is independent of edit-card painting.
    let boundary_gap = if spec.application.is_some() { gap } else { 0.0 };
    let room = (width - left - right - boundary_gap).max(0.0);

    let mut seen = std::collections::HashSet::new();
    // A saved list is bounded to 64 movable objects. Application chrome has its own
    // small allowance so a full list of separators cannot truncate File or the tabs.
    let limit = MAX_ITEMS + if spec.application.is_some() { 8 } else { 0 };
    let mut centre_taken = false;
    let mut work: Vec<Work> = spec
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let mut ladder: Vec<f32> = item.ladder.iter().take(MAX_STEPS).map(|&w| clean(w)).collect();
            if ladder.is_empty() {
                ladder.push(0.0);
            }
            let mut previous = MAX_WIDTH;
            for width in &mut ladder {
                *width = width.min(previous);
                previous = *width;
            }
            let taken = index < limit && seen.insert(item.id.as_str());
            let second_centre = taken && item.zone == Zone::Centre && std::mem::replace(&mut centre_taken, true);
            let state = if taken && !second_centre { State::Placed } else { State::Out };
            Work {
                zone: item.zone,
                ladder,
                priority: item.priority,
                locked: item.locked || item.never_overflow,
                flexible: item.flexible,
                step: 0,
                state,
            }
        })
        .collect();

    // Give way, lowest priority first, until the row fits.
    let mut dropped: Vec<usize> = Vec::new();
    while total(&work, gap, button) > room {
        let pick = work
            .iter()
            .enumerate()
            .filter(|(_, w)| w.state == State::Placed && w.zone != Zone::Centre && (w.can_step() || !w.locked))
            .min_by_key(|(i, w)| (w.priority, *i))
            .map(|(i, _)| i);
        let Some(i) = pick else { break };
        if let Some(w) = work.get_mut(i) {
            if w.can_step() {
                w.step += 1;
            } else {
                w.state = State::Overflow;
                dropped.push(i);
            }
        }
    }
    // Still too wide: leave out the lowest-priority placed items, locked ones included.
    while total(&work, gap, button) > room {
        let pick = work.iter().enumerate().filter(|(_, w)| w.state == State::Placed).min_by_key(|(i, w)| (w.priority, *i)).map(|(i, _)| i);
        let Some(i) = pick else { break };
        if let Some(w) = work.get_mut(i) {
            w.state = State::Out;
        }
    }
    let show_button = work.iter().any(|w| w.state == State::Overflow) && total(&work, gap, button) <= room;
    let mut placements: Vec<Option<Placement>> = vec![None; work.len()];
    // Start items, left to right.
    let mut x = left;
    let mut starts = 0_usize;
    for (i, w) in work.iter().enumerate() {
        if w.zone == Zone::Start && w.state == State::Placed {
            let width = w.width();
            if let Some(slot) = placements.get_mut(i) {
                *slot = Some(Placement { x, width, step: w.step });
            }
            x += width;

            starts += 1;
            x += gap;
        }
    }
    let start_end = if starts > 0 { x - gap } else { left };
    // End items, right to left; the overflow button ends up leftmost.
    let mut edge = left + room;
    let mut end_left = edge;
    let mut ends = 0_usize;
    for (i, w) in work.iter().enumerate().rev() {
        if w.zone == Zone::End && w.state == State::Placed {
            let width = w.width();
            let at = (edge - width).max(left);
            if let Some(slot) = placements.get_mut(i) {
                *slot = Some(Placement { x: at, width, step: w.step });
            }
            end_left = at;
            edge = end_left - gap;
            ends += 1;
        }
    }
    let mut overflow_button = None;
    if show_button {
        let at = (edge - button).max(left);
        overflow_button = Some((at, button));
        end_left = at;
        ends += 1;
    }
    // The Centre item takes what is left between them.
    let mut centre = None;
    if let Some((i, w)) = work.iter().enumerate().find(|(_, w)| w.zone == Zone::Centre && w.state == State::Placed) {
        let min = w.width();
        let from = if starts > 0 { start_end + gap } else { start_end };
        let to = if ends > 0 { end_left - gap } else { left + room };
        let width = if w.flexible { (to - from).max(min) } else { min };
        if let Some(slot) = placements.get_mut(i) {
            *slot = Some(Placement { x: from, width, step: 0 });
        }
        centre = Some((from, width));
    }
    let overflow = dropped
        .iter()
        .filter(|&&i| work.get(i).is_some_and(|w| w.state == State::Overflow))
        .filter_map(|&i| spec.items.get(i).map(|item| item.id.clone()))
        .collect();
    Placed { placements, overflow, overflow_button, centre, application: None }
}

/// The application's measured bar and the projection from fitted slots to drawing geometry.
pub mod application {
    use super::{Item, MAX_ITEMS, Placed, Placement, Spec, Zone, clean, layout_row};
    use crate::app_bar_items::{Layout, Zone as SavedZone};

    /// Every adjacent pair, including chrome and tabs, uses this one spacing rule.
    pub const GAP: f32 = 4.0;
    pub const ICON_WIDTH: f32 = 26.0;
    pub const PILL_HEIGHT: f32 = 20.0;
    pub const SEPARATOR_WIDTH: f32 = 1.0;
    pub const EDIT_SEPARATOR_WIDTH: f32 = SPACE_WIDTH;
    pub const CARD_PADDING: f32 = GAP;
    pub const SPACE_WIDTH: f32 = 11.0;
    pub const VERSION_PADDING: f32 = 12.0;
    pub const CONTENT_PADDING: f32 = 8.0;
    pub const PILL_ICON: f32 = 14.0;
    pub const SQUARE_ICON: f32 = 18.0;
    pub const PILL_MIN: f32 = CONTENT_PADDING * 2.0 + PILL_ICON * 2.0 + GAP;
    const TABS_MIN: f32 = 124.0;
    /// Measurements only; no egui state or registry access enters the engine.
    #[derive(Clone)]
    pub struct Measurements {
        pub origin: f32,
        pub name: f32,
        pub file: f32,
        pub leading: Option<f32>,
        /// Whether each action yields before the primary actions, in source order.
        pub actions: Vec<bool>,
        pub quick: usize,
        pub report: f32,
        pub report_icon_only: bool,
        pub pills: Vec<f32>,
        /// Full and compact field widths.
        pub search: Option<(f32, f32)>,
        pub assistant: bool,
        pub version: Option<f32>,
        /// Measured full width of the app-owned Update offer in this frame.
        pub offered: Option<f32>,
        pub managed: Option<f32>,
        pub pixels_per_point: f32,
    }

    /// Counts and label availability only: projection never adds item-specific offsets.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Policy {
        pub(super) editing: bool,
        name: bool,
        action_count: usize,
        quick_count: usize,
        pill_count: usize,
        report_label: bool,
    }
    /// A horizontal drawing span, projected from a packed slot.
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct Span {
        pub left: f32,
        pub right: f32,
    }

    impl Span {
        fn new(left: f32, width: f32) -> Self {
            Self { left, right: left + width }
        }

        pub fn centre(self) -> f32 {
            self.left + (self.right - self.left) / 2.0
        }
    }

    /// All horizontal positions the painter and its caller need. The vertical geometry is
    /// unchanged and remains the painter's responsibility.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct Geometry {
        pub cards: [Option<Span>; 2],
        pub compact: bool,
        pub name_visible: bool,
        pub visible: Vec<String>,
        pub quick_count: usize,
        pub tabs_end: f32,
        pub managed: Option<Span>,
        pub managed_labeled: bool,
        pub reserved_offers: bool,
        pub mark: f32,
        pub name: f32,
        pub leading: Option<Span>,
        pub home: Span,
        pub file: Span,
        pub actions: Vec<Option<f32>>,
        pub quick: Option<(f32, f32)>,
        /// Index into the host's quick list and its independently placed centre.
        pub quick_slots: Vec<(usize, f32)>,
        pub tabs_start: f32,
        pub controls_left: f32,
        pub offer: Span,
        pub offer_labeled: bool,
        pub report: Span,
        pub report_labeled: bool,
        pub search: Option<Span>,
        pub assistant: Option<Span>,
        pub settings: Span,
        pub help: f32,
        pub version: Option<Span>,
        pub pills: Vec<Span>,
    }

    fn slot(spec: &Spec, placed: &Placed, id: &str) -> Option<Placement> {
        spec.items.iter().position(|i| i.id == id).and_then(|i| placed.placements.get(i)).copied().flatten()
    }

    fn build(width: f32, left: f32, right: f32, m: Measurements, saved: Option<(&Layout, &[&str], &[&str], &[&str])>) -> Spec {
        let quick_count = m.quick.min(24);
        let action_count = m.actions.len().min(MAX_ITEMS.saturating_sub(16 + quick_count));
        let pill_count = m.pills.len().min(MAX_ITEMS.saturating_sub(14 + action_count + quick_count));
        let name = clean(m.name);
        let mut slots = Vec::new();
        if m.leading.is_some() {
            slots.push(Item::new("leading", Zone::Start, &[ICON_WIDTH], 85));
        }
        slots.push(Item::new("home", Zone::Start, &[ICON_WIDTH], 85));
        slots.push(Item::new("file", Zone::Start, &[clean(m.file).max(ICON_WIDTH)], 255).never_overflow());
        for (i, wide) in m.actions.iter().take(action_count).enumerate() {
            slots.push(Item::new(&format!("action:{i}"), Zone::Start, &[ICON_WIDTH], if *wide { 80 } else { 90 }));
        }
        for i in 0..quick_count {
            slots.push(Item::new(&format!("quick:{i}"), Zone::Start, &[ICON_WIDTH], 70_u8.saturating_sub(i as u8)));
        }
        for (i, w) in m.pills.iter().take(pill_count).enumerate() {
            slots.push(Item::new(&format!("pill:{i}"), Zone::End, &[clean(*w).max(PILL_MIN), PILL_MIN], 20));
        }
        if m.assistant {
            slots.push(Item::new("assistant", Zone::End, &[ICON_WIDTH], 60));
        }
        if let Some((full, _)) = m.search {
            slots.push(Item::new("search", Zone::End, &[clean(full).max(ICON_WIDTH), ICON_WIDTH], 30));
        }
        slots.push(Item::new("settings", Zone::End, &[ICON_WIDTH], 90));
        slots.push(Item::new("help", Zone::End, &[ICON_WIDTH], 90));
        let report_full = CONTENT_PADDING * 2.0 + PILL_ICON + GAP + clean(m.report);
        let report = if m.report_icon_only { vec![PILL_HEIGHT] } else { vec![report_full, PILL_HEIGHT] };
        slots.push(Item::new("report", Zone::End, &report, 40));
        if let Some(w) = m.version {
            slots.push(Item::new("version", Zone::End, &[clean(w) + VERSION_PADDING * 2.0], 10));
        }

        let mut items = vec![Item::new("logo", Zone::Start, &[ICON_WIDTH + if name > 0.0 { GAP + name } else { 0.0 }, ICON_WIDTH], 0).locked()];
        if let Some((saved, actions, quick, pills)) = saved {
            let defaults = Layout::default_for(quick.iter().copied());
            let mut saved = saved.clone();
            saved.sanitize(&defaults);
            let stable = |id: &str| -> String {
                if id == "leading" {
                    return "documents".into();
                }
                if let Some(i) = id.strip_prefix("action:").and_then(|i| i.parse::<usize>().ok()) {
                    return actions.get(i).copied().unwrap_or("").to_owned();
                }
                if let Some(i) = id.strip_prefix("quick:").and_then(|i| i.parse::<usize>().ok()) {
                    return quick.get(i).map(|id| format!("quick:{id}")).unwrap_or_default();
                }
                if let Some(i) = id.strip_prefix("pill:").and_then(|i| i.parse::<usize>().ok()) {
                    return pills.get(i).copied().unwrap_or("").to_owned();
                }
                id.to_owned()
            };
            for p in saved.items.iter().filter(|p| p.shown) {
                let zone = if p.zone == SavedZone::Start { Zone::Start } else { Zone::End };
                let object_width = if p.id.starts_with("separator:") {
                    Some(SEPARATOR_WIDTH)
                } else if p.id.starts_with("space:") {
                    Some(SPACE_WIDTH)
                } else {
                    None
                };
                if let Some(w) = object_width {
                    items.push(Item::new(&p.id, zone, &[w], 0));
                } else if let Some(slot) = slots.iter().find(|s| stable(&s.id) == p.id) {
                    let mut slot = slot.clone();
                    slot.zone = zone;
                    items.push(slot);
                }
            }
        } else {
            items.extend(slots.iter().filter(|s| s.zone == Zone::Start).cloned());
            if let Some(at) = items.iter().position(|s| s.id == "file") {
                items.insert(at, Item::new("separator:1", Zone::Start, &[SEPARATOR_WIDTH], 0));
            }
            items.push(Item::new("separator:2", Zone::Start, &[SEPARATOR_WIDTH], 0));
            items.extend(slots.iter().filter(|s| s.zone == Zone::End).cloned());
        }
        items.push(Item::new("tabs", Zone::Centre, &[TABS_MIN], 255).locked().flexible());
        for (id, width) in [("managed", m.managed), ("offered", m.offered)] {
            if let Some(w) = width {
                items.push(Item::new(id, Zone::End, &[clean(w).max(ICON_WIDTH), ICON_WIDTH], 55));
            }
        }
        // The source order represents the two stacks. Chrome belongs to the engine;
        // moving it here changes no spacing, and every gap still comes from Spec::gap.
        items.sort_by_key(|i| match i.zone {
            Zone::Start => 0,
            Zone::Centre => 1,
            Zone::End => 2,
        });
        Spec {
            width,
            captions_left: left,
            captions_right: right,
            gap: GAP,
            overflow_button_width: ICON_WIDTH,
            items,
            application: Some(Policy { editing: false, name: name > 0.0, action_count, quick_count, pill_count, report_label: !m.report_icon_only }),
        }
    }

    pub fn spec(width: f32, captions_left: f32, captions_right: f32, m: Measurements) -> Spec {
        build(width, captions_left, captions_right, m, None)
    }

    pub fn spec_with_items(dimensions: (f32, f32, f32), m: Measurements, saved: &Layout, actions: &[&str], quick: &[&str], pills: &[&str]) -> Spec {
        build(dimensions.0, dimensions.1, dimensions.2, m, Some((saved, actions, quick, pills)))
    }

    /// Editing changes only separator slots. Card decoration uses existing gaps; saved data and
    /// all other item ladders stay unchanged. Calling this repeatedly is idempotent.
    pub fn set_editing(spec: &mut Spec, editing: bool) {
        let Some(policy) = &mut spec.application else { return };
        policy.editing = editing;
        for item in &mut spec.items {
            if item.id.starts_with("separator:") {
                item.ladder = vec![if editing { EDIT_SEPARATOR_WIDTH } else { SEPARATOR_WIDTH }];
            }
        }
    }

    pub(super) fn place(spec: &Spec, policy: &Policy) -> Placed {
        let mut p = layout_row(spec);
        let at = |id: &str| slot(spec, &p, id);
        let span = |id: &str| at(id).map(|p| Span::new(p.x, p.width));
        let logo = at("logo");
        let end = p
            .overflow_button
            .map(|b| b.0)
            .into_iter()
            .chain(spec.items.iter().zip(&p.placements).filter(|(i, _)| i.zone == Zone::End).filter_map(|(_, p)| p.map(|p| p.x)))
            .fold((clean(spec.width) - clean(spec.captions_right) - clean(spec.gap)).max(clean(spec.captions_left)), f32::min);
        let tabs = span("tabs").unwrap_or(Span::new(end, 0.0));
        let quick_slots: Vec<_> = (0..policy.quick_count).filter_map(|i| at(&format!("quick:{i}")).map(|p| (i, p.x + p.width / 2.0))).collect();
        let quick = quick_slots.first().map(|&(_, x)| (x, ICON_WIDTH + GAP));
        let report = at("report");
        let offer = at("offered");
        let managed = at("managed");
        let cards = [Zone::Start, Zone::End].map(|zone| {
            let mut edges: Option<(f32, f32)> = None;
            for (item, slot) in spec.items.iter().zip(&p.placements) {
                if policy.editing
                    && item.zone == zone
                    && super::card_item(item)
                    && let Some(slot) = slot
                {
                    edges = Some(edges.map_or((slot.x, slot.right()), |(lo, hi)| (lo.min(slot.x), hi.max(slot.right()))));
                }
            }
            edges.map(|(lo, hi)| {
                let mut left = (lo - CARD_PADDING).max(clean(spec.captions_left));
                let mut right = (hi + CARD_PADDING).min((clean(spec.width) - clean(spec.captions_right)).max(left));
                let fixed = spec.items.iter().zip(&p.placements).filter(|(i, _)| !super::card_item(i)).filter_map(|(_, p)| *p);
                for fixed in fixed.chain(p.overflow_button.map(|(x, width)| Placement { x, width, step: 0 })) {
                    if fixed.right() <= lo {
                        left = left.max(fixed.right());
                    }
                    if fixed.x >= hi {
                        right = right.min(fixed.x);
                    }
                }
                Span { left, right }
            })
        });
        let g = Geometry {
            cards,
            compact: false,
            name_visible: policy.name && logo.is_some_and(|p| p.step == 0),
            visible: spec.items.iter().zip(&p.placements).filter(|(_, p)| p.is_some()).map(|(i, _)| i.id.clone()).collect(),
            mark: logo.map_or(0.0, |p| p.x + ICON_WIDTH / 2.0),
            name: logo.map_or(0.0, |p| p.x + ICON_WIDTH + GAP),
            leading: span("leading"),
            home: span("home").unwrap_or_default(),
            file: span("file").unwrap_or_default(),
            actions: (0..policy.action_count).map(|i| at(&format!("action:{i}")).map(|p| p.x + p.width / 2.0)).collect(),
            quick,
            quick_count: quick_slots.len(),
            quick_slots,
            tabs_start: tabs.left,
            tabs_end: tabs.right,
            controls_left: end,
            report: span("report").unwrap_or_default(),
            report_labeled: policy.report_label && report.is_some_and(|p| p.step == 0),
            settings: span("settings").unwrap_or_default(),
            help: at("help").map_or(0.0, |p| p.x + p.width / 2.0),
            search: span("search"),
            assistant: span("assistant"),
            version: span("version"),
            pills: (0..policy.pill_count).map(|i| span(&format!("pill:{i}")).unwrap_or_default()).collect(),
            offer: span("offered").unwrap_or(Span::new(end, 0.0)),
            offer_labeled: offer.is_some_and(|p| p.step == 0),
            managed: span("managed"),
            managed_labeled: managed.is_some_and(|p| p.step == 0),
            reserved_offers: true,
        };
        p.overflow.sort_by_key(|id| spec.items.iter().position(|i| &i.id == id));
        p.application = Some(g);
        p
    }
}

/// A representative measured bar for property tests; it uses the same uniform item spec.
pub mod default_bar {
    use super::{Item, Spec, application};
    pub const TABS: &str = "tabs";
    pub const TABS_MIN: f32 = 124.0;

    pub fn items() -> Vec<Item> {
        let m = application::Measurements {
            origin: 0.0,
            name: 72.0,
            file: 40.0,
            leading: None,
            actions: vec![false, true, true],
            quick: 0,
            report: 80.0,
            report_icon_only: false,
            pills: vec![90.0, 46.0],
            search: Some((160.0, application::ICON_WIDTH)),
            assistant: true,
            version: Some(40.0),
            offered: Some(100.0),
            managed: None,
            pixels_per_point: 1.0,
        };
        let mut items = application::spec(1200.0, 0.0, 0.0, m).items;
        for item in &mut items {
            item.id = match item.id.as_str() {
                "action:0" => "save".into(),
                "action:1" => "undo".into(),
                "action:2" => "redo".into(),
                "pill:0" => "donate".into(),
                "pill:1" => "discord".into(),
                _ => item.id.clone(),
            };
        }
        items
    }
    pub fn spec(width: f32, captions_left: f32, captions_right: f32) -> Spec {
        Spec {
            width,
            captions_left,
            captions_right,
            gap: application::GAP,
            overflow_button_width: application::ICON_WIDTH,
            items: items(),
            application: None,
        }
    }
}

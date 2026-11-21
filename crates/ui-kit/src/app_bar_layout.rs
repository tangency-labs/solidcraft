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
//! The measured application's [`application::spec`] preserves its established independent
//! zone anchors instead: unusually long names or many quick buttons can still cross the other
//! zone, just as before. Its width thresholds and drawing projections live in the engine too.
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
    /// A Centre item takes all the room left; otherwise it keeps its least width.
    pub flexible: bool,
}

impl Item {
    /// An unlocked, fixed item.
    pub fn new(id: &str, zone: Zone, ladder: &[f32], priority: u8) -> Self {
        Self { id: id.to_owned(), zone, ladder: ladder.to_vec(), priority, locked: false, flexible: false }
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
    /// Retain the application's established zone anchors and width thresholds.
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
    /// The ids behind the overflow button, in the order they were given up (lowest priority first).
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
    layout_row(spec, false)
}

fn layout_row(spec: &Spec, retain_zones: bool) -> Placed {
    let (width, left, right) = (clean(spec.width), clean(spec.captions_left), clean(spec.captions_right));
    let (gap, button) = (clean(spec.gap), clean(spec.overflow_button_width));
    let room = (width - left - right).max(0.0);

    let mut seen = std::collections::HashSet::new();
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
            // Only the first MAX_ITEMS items take part, so `seen` stays small.
            let taken = index < MAX_ITEMS && seen.insert(item.id.as_str());
            let second_centre = taken && item.zone == Zone::Centre && std::mem::replace(&mut centre_taken, true);
            let state = if taken && !second_centre { State::Placed } else { State::Out };
            Work { zone: item.zone, ladder, priority: item.priority, locked: item.locked, flexible: item.flexible, step: 0, state }
        })
        .collect();

    // Give way, lowest priority first, until the row fits.
    let mut dropped: Vec<usize> = Vec::new();
    while !retain_zones && total(&work, gap, button) > room {
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
    while !retain_zones && total(&work, gap, button) > room {
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
            let at = if retain_zones { edge - width } else { (edge - width).max(left) };
            if let Some(slot) = placements.get_mut(i) {
                *slot = Some(Placement { x: at, width, step: w.step });
            }
            edge = at - gap;
            end_left = at;
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

/// The application's measured bar. Its historical width thresholds are deliberately separate
/// from the general priority ladder: changing those thresholds would move existing controls.
pub mod application {
    use super::{Item, MAX_ITEMS, MAX_WIDTH, Placed, Placement, Spec, Zone, clean, layout_row};

    const COMPACT: f32 = 640.0;
    const HOME_MIN: f32 = 128.0;
    const NAME_GAP: f32 = 6.0;
    const TABS_KEPT: f32 = 124.0;
    const TABS_LABEL_ROOM: f32 = 216.0;
    const RIGHT_GROUP: f32 = 60.0;
    const CLUSTER_BUTTONS: f32 = 108.0;
    const REPORT_TEXT_X: f32 = 29.0;
    const SEARCH_GAP: f32 = 4.0;
    const PILL_GAP: f32 = 6.0;

    /// Measurements only; no egui state or registry access enters the engine.
    pub struct Measurements {
        pub origin: f32,
        pub name: f32,
        pub file: f32,
        pub leading: Option<f32>,
        /// Whether each action is omitted on compact bars, in source order.
        pub actions: Vec<bool>,
        pub quick: usize,
        pub report: f32,
        pub report_icon_only: bool,
        pub pills: Vec<f32>,
        /// Full and compact field widths.
        pub search: Option<(f32, f32)>,
        pub assistant: bool,
        pub version: Option<f32>,
        /// Width of the app-owned offer published in the preceding frame.
        pub offered: Option<f32>,
        pub pixels_per_point: f32,
    }

    /// The compatibility choices attached to the measured spec. The ordinary layout policy
    /// always fits one row. This policy retains each zone's historical positions even when a
    /// very long name or many quick buttons already crossed the other zone in the old bar.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Policy {
        origin: f32,
        compact: bool,
        quick: bool,
        leading: Option<f32>,
        action_count: usize,
        report_label: bool,
        report_icon_only: bool,
        search: bool,
        assistant: bool,
        version: Option<f32>,
        pills: Vec<f32>,
        offered: f32,
        pixels_per_point: f32,
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
        pub compact: bool,
        pub mark: f32,
        pub name: f32,
        pub leading: Option<Span>,
        pub home: Span,
        pub brand_separator: f32,
        pub file: Span,
        pub actions: Vec<Option<f32>>,
        pub quick_separator: Option<f32>,
        pub quick: Option<(f32, f32)>,
        pub documents_separator: f32,
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

    fn plain(width: f32, left: f32, right: f32, items: Vec<Item>) -> Spec {
        Spec { width, captions_left: left, captions_right: right, gap: 0.0, overflow_button_width: 0.0, items, application: None }
    }

    fn slot(spec: &Spec, placed: &Placed, id: &str) -> Placement {
        spec.items.iter().position(|i| i.id == id).and_then(|i| placed.placements.get(i)).copied().flatten().unwrap_or(Placement {
            x: 0.0,
            width: 0.0,
            step: 0,
        })
    }

    fn item(id: &str, zone: Zone, width: f32) -> Item {
        Item::new(id, zone, &[clean(width)], u8::MAX).locked()
    }

    /// Build the measured slots, choosing exactly the old bar's compact/label/visibility modes.
    /// Variable-count groups each occupy one slot; their children are projected by layout.
    pub fn spec(width: f32, captions_left: f32, captions_right: f32, m: Measurements) -> Spec {
        let (width, left, right) = (clean(width), clean(captions_left), clean(captions_right));
        let compact = width - left < COMPACT;
        let step = if compact { 22.0 } else { 34.0 };
        let home = if compact { 56.0 } else { HOME_MIN.max(36.0 + clean(m.name) + NAME_GAP + 20.0) };
        let leading = m.leading.map(clean);
        let mut items = vec![item("logo", Zone::Start, home - 20.0)];
        if let Some(w) = leading {
            items.push(item("leading", Zone::Start, w + 8.0));
        }
        items.push(item("home", Zone::Start, 32.0));
        // Home -> separator (8), then File's left edge (3).
        items.push(item("brand", Zone::Start, 11.0));
        // First file-action centre is 17 beyond File; each slot centres on step / 2.
        items.push(item("file", Zone::Start, clean(m.file) + 17.0 - step / 2.0));
        // Keep room under MAX_ITEMS for the fixed slots and the flexible tabs.
        let action_count = m.actions.len().min(MAX_ITEMS.saturating_sub(17));
        for (i, &wide_only) in m.actions.iter().take(action_count).enumerate() {
            if !(compact && wide_only) {
                items.push(item(&format!("action:{i}"), Zone::Start, step));
            }
        }
        let quick = !compact && m.quick > 0;
        if quick {
            items.push(item("quick_gap", Zone::Start, 8.0));
            items.push(item("quick", Zone::Start, m.quick.min(MAX_ITEMS) as f32 * step));
        }
        // The old cursor is half a step beyond these slots. Documents start another
        // 24 (19 compact) - half a step + 10 beyond their right edge.
        items.push(item("documents", Zone::Start, if compact { 19.0 } else { 24.0 } - step / 2.0 + 10.0));
        let starts = plain(width, left, right, items.clone());
        let start_layout = layout_row(&starts, true);
        let tabs_start = slot(&starts, &start_layout, "documents").right();
        items.push(Item::new("tabs", Zone::Centre, &[0.0], u8::MAX).locked().flexible());
        let version = m.version.map(clean).filter(|&w| {
            let need = 12.0 + w + 8.0;
            width >= COMPACT
                && (need <= RIGHT_GROUP || width - right - RIGHT_GROUP.max(need) - 28.0 - CLUSTER_BUTTONS - 8.0 - tabs_start >= TABS_KEPT)
        });
        let version_room = version.map_or(8.0, |w| RIGHT_GROUP.max(12.0 + w + 8.0));
        let report_full = REPORT_TEXT_X + clean(m.report) + 11.0;
        let report_label = !m.report_icon_only && width - right - version_room - CLUSTER_BUTTONS - tabs_start >= TABS_LABEL_ROOM + report_full;
        let report_width = if m.report_icon_only {
            32.0
        } else if report_label {
            report_full
        } else {
            28.0
        };
        // These are widths of packed slots, not offsets from Report's anchor.
        let end_fixed = version_room + report_width + 34.0 + 34.0 + 40.0;
        let search = m.search.map(|(full, compact_width)| {
            let full = clean(full);
            if width >= COMPACT && width - right - end_fixed - tabs_start - full - SEARCH_GAP >= TABS_LABEL_ROOM {
                full
            } else {
                clean(compact_width)
            }
        });
        let controls = width - right - end_fixed - search.map_or(0.0, |w| w + SEARCH_GAP);
        let offered = m.offered.map_or(0.0, |w| clean(w) + 4.0);
        let pills: Vec<f32> = m.pills.into_iter().take(MAX_ITEMS).map(clean).collect();
        let pills_width: f32 = pills.iter().map(|w| w + PILL_GAP).sum();
        let pills = if !pills.is_empty() && controls - offered - pills_width - tabs_start >= TABS_LABEL_ROOM { pills } else { Vec::new() };
        if !pills.is_empty() {
            items.push(item("pills", Zone::End, pills_width));
            items.push(item("offered", Zone::End, offered));
        }
        items.push(item("assistant", Zone::End, 40.0));
        if let Some(w) = search {
            items.push(item("search", Zone::End, w + SEARCH_GAP));
        }
        items.push(item("settings", Zone::End, 34.0));
        items.push(item("help", Zone::End, 34.0));
        items.push(item("report", Zone::End, report_width));
        items.push(item("version", Zone::End, version_room));
        let policy = Policy {
            origin: if m.origin.is_finite() { m.origin.clamp(-MAX_WIDTH, MAX_WIDTH) } else { 0.0 },
            compact,
            quick,
            leading,
            action_count,
            report_label,
            report_icon_only: m.report_icon_only,
            search: search.is_some(),
            assistant: m.assistant,
            version,
            pills,
            offered,
            pixels_per_point: if m.pixels_per_point.is_finite() && m.pixels_per_point > 0.0 { m.pixels_per_point } else { 1.0 },
        };
        Spec { width, captions_left: left, captions_right: right, gap: 0.0, overflow_button_width: 0.0, items, application: Some(policy) }
    }

    pub(super) fn place(spec: &Spec, policy: &Policy) -> Placed {
        // The same packing loop, retaining each zone's historical anchor at narrow widths.
        let mut placed = layout_row(spec, true);
        let at = |id: &str| slot(spec, &placed, id);
        let step = if policy.compact { 22.0 } else { 34.0 };
        let logo = at("logo");
        let home = at("home");
        let brand = at("brand");
        let file = at("file");
        let documents = at("documents");
        let report = at("report");
        let help = at("help");
        let settings = at("settings");
        let assistant = at("assistant");
        let search = at("search");
        let version = at("version");
        let pills = at("pills");
        let controls_left = if policy.pills.is_empty() { assistant.x } else { pills.x };
        let snap = |v: f32| {
            let scaled = (v + policy.origin) * policy.pixels_per_point;
            if scaled.is_finite() { scaled.round() / policy.pixels_per_point - policy.origin } else { v }
        };
        let mut pills_right = assistant.x - 4.0 - policy.offered;
        let mut pill_spans = Vec::new();
        for &width in policy.pills.iter().rev() {
            let scaled_width = width * policy.pixels_per_point;
            let w = if scaled_width.is_finite() { scaled_width.round() / policy.pixels_per_point } else { width };
            let left = snap(pills_right - w);
            pill_spans.push(Span::new(left, w));
            pills_right = left - PILL_GAP;
        }
        pill_spans.reverse();
        let geometry = Geometry {
            compact: policy.compact,
            mark: logo.x + 20.0,
            name: logo.x + 36.0,
            leading: policy.leading.map(|w| Span::new(at("leading").x + 20.0 - w / 2.0, w)),
            home: Span::new(home.x, home.width),
            brand_separator: brand.x + 8.0,
            file: Span::new(file.x, (file.width - 17.0 + step / 2.0).max(0.0)),
            actions: (0..policy.action_count)
                .map(|i| {
                    let id = format!("action:{i}");
                    spec.items.iter().any(|item| item.id == id).then(|| at(&id).x + step / 2.0)
                })
                .collect(),
            quick_separator: policy.quick.then(|| at("quick").x),
            quick: policy.quick.then(|| (at("quick").x + step / 2.0, step)),
            documents_separator: documents.right() - 10.0,
            tabs_start: documents.right(),
            controls_left,
            offer: Span { left: documents.x + step / 2.0, right: assistant.x - 2.0 },
            offer_labeled: policy.report_label || (policy.report_icon_only && clean(spec.width) >= COMPACT),
            report: Span::new(report.x, report.width),
            report_labeled: policy.report_label,
            search: policy.search.then(|| Span::new(search.x, (search.width - SEARCH_GAP).max(0.0))),
            assistant: policy.assistant.then(|| Span::new(assistant.right() - 34.0, 32.0)),
            settings: Span::new(settings.x, 32.0),
            help: help.x + 16.0,
            version: policy.version.map(|w| Span::new(version.right() - 12.0 - w, w)),
            pills: pill_spans,
        };
        placed.application = Some(geometry);
        placed
    }
}

/// Today's default bar as a [`Spec`], reproducing `app_bar::show`'s default arrangement. The
/// widths with a measured source in `app_bar.rs` are exact; label-dependent ones (File, the pills,
/// the search field, Report a bug, the version) are typical widths, and the app measures its own.
pub mod default_bar {
    use super::{Item, Spec, Zone};

    /// The id of the document tabs, the Centre item.
    pub const TABS: &str = "tabs";
    /// The least room the tabs keep (`TABS_KEPT`).
    pub const TABS_MIN: f32 = 124.0;

    /// The default bar's items in source order, left to right as drawn today.
    pub fn items() -> Vec<Item> {
        use Zone::{Centre, End, Start};
        vec![
            // Mark and name end where Home's button starts (HOME_MIN 128, 20 before it); a
            // compact bar keeps the mark only (Home at 56, its button at 36). The name goes first.
            Item::new("logo", Start, &[108.0, 36.0], 45).locked(),
            Item::new("home", Start, &[32.0], 85),
            // 8 after Home, the separator, 16 before File.
            Item::new("separator.brand", Start, &[24.0], 255).locked(),
            // The text button: 16 around its label, and 17 after it.
            Item::new("file", Start, &[62.0], 255).locked(),
            // Each file action steps 34, or 22 on a compact bar; Undo and Redo go first there.
            Item::new("save", Start, &[34.0, 22.0], 90),
            Item::new("undo", Start, &[34.0, 22.0], 50),
            Item::new("redo", Start, &[34.0, 22.0], 50),
            Item::new(TABS, Centre, &[TABS_MIN], 255).locked().flexible(),
            // The offer pill: labelled where Report a bug is, else just its icon.
            Item::new("update", End, &[110.0, 34.0], 70),
            // Donate and Discord: pad, mark, label, x, plus PILL_GAP; no label shrinks them.
            Item::new("donate", End, &[100.0, 51.0], 20),
            Item::new("discord", End, &[106.0, 51.0], 20),
            // The right-hand cluster steps 34 (anchor - 86, - 52, - 18).
            Item::new("assistant", End, &[34.0], 60),
            // The closed field, else its magnifier (CommandSearch::compact_width).
            Item::new("search", End, &[160.0, 42.0], 30),
            Item::new("settings", End, &[34.0], 80),
            Item::new("help", End, &[34.0], 75),
            // Report a bug: REPORT_TEXT_X, the label and 11; the icon alone is a 32 square.
            Item::new("report", End, &[118.0, 32.0], 40),
            // RIGHT_GROUP, the room kept for the version.
            Item::new("version", End, &[60.0], 10),
        ]
    }

    /// The default bar `width` wide with `captions_left`/`captions_right` taken by the window.
    pub fn spec(width: f32, captions_left: f32, captions_right: f32) -> Spec {
        Spec { width, captions_left, captions_right, gap: 0.0, overflow_button_width: 32.0, items: items(), application: None }
    }
}

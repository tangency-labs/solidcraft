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
            for i in 1..ladder.len() {
                ladder[i] = ladder[i].min(ladder[i - 1]);
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
    Placed { placements, overflow, overflow_button, centre }
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
        Spec { width, captions_left, captions_right, gap: 0.0, overflow_button_width: 32.0, items: items() }
    }
}

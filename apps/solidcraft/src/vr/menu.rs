//! The ring menu's tree and what an open one does with the hand (the ring's maths is
//! `solidcraft_vr::menu`; what each slot holds is a setting, Preferences › XR › VR › Ring menu).
//! Held open with its button; the hand moves into a slot; letting go takes it. A group opens a
//! whole ring of its own around the slot the hand reached, leaving free the way back; the ring
//! before turns grey, and moving back the way the hand came returns to it.

use std::sync::Arc;

use solidcraft_ui_egui::vr_pick::Part;
use solidcraft_ui_egui::vr_settings::{GROUP_ICONS, RING_SLOTS, RingAction, VrSettings};
use solidcraft_vr::menu::Ring;

/// What an item does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    ToggleHide,
    Hide,
    Isolate,
    ShowAll,
    Explode,
    Section,
    PutBack,
    Undo,
    Redo,
    Passthrough,
    /// Opens a group's ring (its place among the groups).
    Group(usize),
}

/// One slot: what it does, its words (in the user's language), its pictogram, and whether it can
/// be taken now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub act: Act,
    pub words: String,
    pub icon: Option<&'static str>,
    pub on: bool,
}

/// What the slots depend on when the menu opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// A part is pointed at, or parts are selected: there is something to hide or isolate.
    pub parts: bool,
    /// What Show/Hide acts on is all hidden: it shows.
    pub hidden: bool,
    /// The assembly is shown apart.
    pub exploded: bool,
    /// The section is on.
    pub section: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// The real room can be shown, and whether it is.
    pub passthrough_offered: bool,
    pub passthrough: bool,
}

/// The slot for `action`, as `state` has it.
fn item(action: RingAction, set: &VrSettings, state: State) -> Option<Item> {
    use solidcraft_ui_text::tr;
    let words = |k: &str| tr(k).to_string();
    let (act, text, on) = match action {
        RingAction::Empty => return None,
        RingAction::ToggleHide => (Act::ToggleHide, words(if state.hidden { "vr.menu_show" } else { "vr.menu_toggle_hide" }), state.parts),
        RingAction::Hide => (Act::Hide, words("vr.menu_hide"), state.parts),
        RingAction::Isolate => (Act::Isolate, words("vr.menu_isolate"), state.parts),
        RingAction::ShowAll => (Act::ShowAll, words("vr.menu_show_all"), true),
        RingAction::Explode => (Act::Explode, words(if state.exploded { "vr.menu_assemble" } else { "vr.menu_explode" }), true),
        RingAction::Section => (Act::Section, words(if state.section { "vr.menu_section_off" } else { "vr.menu_section" }), true),
        RingAction::PutBack => (Act::PutBack, words("vr.menu_put_back"), true),
        RingAction::Undo => (Act::Undo, words("vr.menu_undo"), state.can_undo),
        RingAction::Redo => (Act::Redo, words("vr.menu_redo"), state.can_redo),
        RingAction::Passthrough => (Act::Passthrough, words(if state.passthrough { "vr.passthrough_off" } else { "vr.belt_passthrough" }), true),
        RingAction::Group(g) => {
            let g = usize::from(g);
            let group = set.ring_groups.get(g)?;
            let icon = GROUP_ICONS.iter().find(|i| **i == group.icon).copied();
            let name = group.shown_name();
            let name = if name.is_empty() { format!("{} {}", tr("prefs.vr.ring_group"), g + 1) } else { name };
            return Some(Item { act: Act::Group(g), words: name, icon, on: !group.items.is_empty() });
        }
    };
    let icon = match action {
        RingAction::ToggleHide if state.hidden => Some("eye"),
        other => other.icon(),
    };
    Some(Item { act, words: text, icon, on })
}

/// The first ring's slots (`None`) or a group's, as the settings lay them out.
pub fn items(group: Option<usize>, set: &VrSettings, state: State) -> Vec<Item> {
    let actions: &[RingAction] = match group {
        None => &set.ring,
        Some(g) => set.ring_groups.get(g).map_or(&[], |g| g.items.as_slice()),
    };
    actions.iter().filter_map(|a| item(*a, set, state)).take(RING_SLOTS).collect()
}

/// One open ring of the tree: where it stands, its items, and which of its items is marked.
pub struct Level {
    pub ring: Ring,
    pub items: Vec<Item>,
    pub marked: Option<usize>,
}

/// A menu held open.
pub struct Open {
    /// The hand holding it (its index, left first).
    pub hand: usize,
    /// The part the ray pointed at when it opened: with the selection, what the part actions act
    /// on; without one, the selection.
    pub target: Option<Arc<Part>>,
    /// The rings open, the first ring first; each opened from the marked item of the one before.
    pub levels: Vec<Level>,
    /// Each group's slots, as they were when the menu opened.
    groups: Vec<Vec<Item>>,
    /// Since when (seconds, the frame's clock) the hand has been past the marked group: it opens
    /// after the wait asked for.
    on_group: Option<f64>,
}

/// A group's ring stays open while the hand is within this share of its radius of the ring's
/// middle (the group's slot), or on one of its items.
const STAY: f64 = 0.55;

/// Within this share of a ring's radius of an item, the hand is on it and marks it.
const PICK: f64 = 0.6;

/// A group is marked (and opens) only nearer than this share of the radius: the hand has to
/// reach its bubble, so drifting out of the middle opens nothing, and the ring it opens does not
/// close again at once (the hand is then well inside its [`STAY`]).
const GROUP_PICK: f64 = 0.45;

/// A group's ring stands this many times the ring before's radius from its middle.
const GROUP_RADIUS: f64 = 1.1;

/// What moving the hand did to an open menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moved {
    Nothing,
    /// Another item is marked: a tick.
    Marked,
    /// A ring opened or closed: a stronger tick.
    Ring,
}

impl Open {
    /// A menu for `hand` around `at` (the room, metres), its items `radius` from it, facing
    /// `head`, laid out as `set` says; `None` when no ring stands there (the head at the hand).
    pub fn new(hand: usize, at: [f64; 3], head: [f64; 3], radius: f64, target: Option<Arc<Part>>, state: State, set: &VrSettings) -> Option<Open> {
        let ring = Ring::facing(at, head, radius)?;
        let groups = (0..set.ring_groups.len()).map(|g| items(Some(g), set, state)).collect();
        Some(Open { hand, target, levels: vec![Level { ring, items: items(None, set, state), marked: None }], groups, on_group: None })
    }

    /// The hand is at `hand` (the room) at `now` (seconds): mark the item it is in, and a group
    /// as soon as it is marked (after `wait` seconds of it, where asked: 0 at once) opens its ring;
    /// back in the middle of the ring before, the hand goes back to it.
    pub fn moved(&mut self, hand: [f64; 3], now: f64, wait: f64) -> Moved {
        // A group's ring stays open only while the hand is on the group's slot (the ring's
        // middle) or on one of the ring's items; anywhere else it closes, and what the hand is on
        // in the ring before is marked instead (or nothing, until it reaches an item).
        let mut closed = false;
        while self.levels.len() > 1
            && let Some(child) = self.levels.last()
        {
            let (_, nearest) = child.ring.nearest(hand, child.items.len());
            let on_item = nearest.is_some_and(|(i, off)| child.items.get(i).is_some_and(|it| it.on) && off <= child.ring.radius * PICK);
            let on_slot = child.ring.distance(hand) <= child.ring.radius * STAY;
            if on_item || on_slot {
                break;
            }
            self.levels.pop();
            if let Some(last) = self.levels.last_mut() {
                last.marked = None;
            }
            self.on_group = None;
            closed = true;
        }
        let Some(level) = self.levels.last() else { return Moved::Nothing };
        let (reach, nearest) = level.ring.nearest(hand, level.items.len());
        // On the bubble nearest the hand, when it is near enough (a group's nearer still).
        let marked = nearest
            .filter(|(i, off)| {
                level.items.get(*i).is_some_and(|it| {
                    let pick = if matches!(it.act, Act::Group(_)) { GROUP_PICK } else { PICK };
                    it.on && *off <= level.ring.radius * pick
                })
            })
            .map(|(i, _)| i);
        let group = marked.and_then(|i| match level.items.get(i).map(|it| it.act) {
            Some(Act::Group(g)) => Some((i, g)),
            _ => None,
        });
        // A group lit is a group opened: no lit slot that does nothing yet.
        let past = group.filter(|_| reach.is_finite());
        let waited = match past {
            Some(_) => now - *self.on_group.get_or_insert(now) >= wait,
            None => {
                self.on_group = None;
                false
            }
        };
        if waited
            && let Some((i, g)) = past
            && let Some(inner) = self.groups.get(g).filter(|items| !items.is_empty()).cloned()
        {
            let ring = level.ring;
            let count = level.items.len();
            // A fan around the slot reached, pointing on outward (the tree grows away from the
            // ring before), narrow enough to stay clear of that ring's other bubbles; the way
            // back is behind it.
            let out = ring.angle(i, count);
            let n = inner.len() as f64;
            let spread = (0.75 * (n - 1.0)).min(2.2);
            let around = Ring { centre: ring.item(i, count), fan: Some((out, spread)), radius: ring.radius * GROUP_RADIUS, ..ring };
            if let Some(last) = self.levels.last_mut() {
                last.marked = Some(i);
            }
            self.levels.push(Level { ring: around, items: inner, marked: None });
            self.on_group = None;
            return Moved::Ring;
        }
        let Some(level) = self.levels.last_mut() else { return Moved::Nothing };
        if marked != level.marked {
            level.marked = marked;
            return if closed {
                Moved::Ring
            } else if marked.is_some() {
                Moved::Marked
            } else {
                Moved::Nothing
            };
        }
        if closed { Moved::Ring } else { Moved::Nothing }
    }

    /// What letting go takes: the marked item of the last ring, unless it is a group.
    pub fn chosen(&self) -> Option<Act> {
        let level = self.levels.last()?;
        let item = level.items.get(level.marked?)?;
        (item.on && !matches!(item.act, Act::Group(_))).then_some(item.act)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_ui_egui::vr_settings::RingGroup;

    /// Where the menu opens in these tests.
    const AT: [f64; 3] = [0.0, 1.4, -0.5];
    /// The head, level with the menu and behind it: the ring's right is +X, its up +Y.
    const HEAD: [f64; 3] = [0.0, 1.4, 0.0];
    /// The ring's radius.
    const RADIUS: f64 = 0.09;

    /// Everything can be taken: a part is pointed at, undo and redo have steps.
    fn all_on() -> State {
        State { parts: true, can_undo: true, can_redo: true, ..State::default() }
    }

    /// The menu opened at [`AT`] with `set` and `state`.
    fn open_with(set: &VrSettings, state: State) -> Open {
        Open::new(0, AT, HEAD, RADIUS, None, state, set).expect("a ring")
    }

    /// The default menu (Show/Hide, Isolate, Show all, Section, View ▸, Edit ▸ clockwise from
    /// the top), everything on.
    fn default_menu() -> Open {
        open_with(&VrSettings::default(), all_on())
    }

    /// Where item `i` of ring `level` stands.
    fn slot(open: &Open, level: usize, i: usize) -> [f64; 3] {
        let l = &open.levels[level];
        l.ring.item(i, l.items.len())
    }

    /// `at` moved by `x` along the ring's right and `y` up (metres).
    fn off(at: [f64; 3], x: f64, y: f64) -> [f64; 3] {
        [at[0] + x, at[1] + y, at[2]]
    }

    /// What ring `level` holds, by what each item does.
    fn acts(open: &Open, level: usize) -> Vec<Act> {
        open.levels[level].items.iter().map(|i| i.act).collect()
    }

    /// The default ring is laid out as these tests take it.
    #[test]
    fn the_default_ring_holds_four_actions_then_two_groups() {
        let open = default_menu();
        assert_eq!(acts(&open, 0), [Act::ToggleHide, Act::Isolate, Act::ShowAll, Act::Section, Act::Group(0), Act::Group(1)]);
        let bottom = slot(&open, 0, 3);
        assert!((bottom[0] - AT[0]).abs() < 1e-12 && (bottom[1] - (AT[1] - RADIUS)).abs() < 1e-12, "Section at the bottom: {bottom:?}");
    }

    /// Marking a group opens its ring at once (no wait asked for), and it stays open while the
    /// hand is on the group's slot (the new ring's middle) or on one of its items; letting go on
    /// an item takes it, and moving back to the first ring's middle closes the group again.
    #[test]
    fn a_groups_ring_opens_on_marking_it_and_stays_on_its_slot_and_items() {
        let mut open = default_menu();
        assert_eq!(open.moved(slot(&open, 0, 4), 0.0, 0.0), Moved::Ring);
        assert_eq!(open.levels.len(), 2);
        assert_eq!(open.levels[0].marked, Some(4), "the group it came from stays lit");
        assert_eq!(acts(&open, 1), [Act::PutBack, Act::Explode, Act::Passthrough]);
        assert_eq!(open.chosen(), None, "a group is not taken");
        let middle = open.levels[1].ring.centre;
        // On the slot: still open, nothing marked.
        assert_eq!(open.moved(middle, 0.1, 0.0), Moved::Nothing);
        // Around it, within its share of the radius (some of it toward the ring's items): still
        // open.
        let near = RADIUS * GROUP_RADIUS * STAY * 0.9;
        for (x, y) in [(near, 0.0), (-near, 0.0), (0.0, near), (0.0, -near)] {
            assert_ne!(open.moved(off(middle, x, y), 0.1, 0.0), Moved::Ring, "{x} {y}");
            assert_eq!(open.levels.len(), 2);
        }
        // On each of its items: marked, and taken on letting go.
        for (i, act) in [Act::PutBack, Act::Explode, Act::Passthrough].into_iter().enumerate() {
            assert_ne!(open.moved(middle, 0.2, 0.0), Moved::Ring, "back on the slot: still open");
            assert_eq!(open.levels[1].marked, None);
            assert_eq!(open.moved(slot(&open, 1, i), 0.2, 0.0), Moved::Marked);
            assert_eq!((open.levels.len(), open.levels[1].marked, open.chosen()), (2, Some(i), Some(act)));
        }
        // Back in the first ring's middle: the group closes and nothing is marked.
        assert_eq!(open.moved(AT, 0.3, 0.0), Moved::Ring);
        assert_eq!((open.levels.len(), open.levels[0].marked, open.chosen()), (1, None, None));
    }

    /// Moving off a group's ring anywhere but its slot and its items closes it, and the first ring
    /// marks what the hand is then on: the bubble beside the group, or another group, whose ring
    /// opens straight away.
    #[test]
    fn leaving_a_groups_ring_closes_it_and_the_ring_before_marks_what_the_hand_is_on() {
        let mut open = default_menu();
        open.moved(slot(&open, 0, 4), 0.0, 0.0);
        assert_eq!(open.moved(slot(&open, 0, 3), 0.1, 0.0), Moved::Ring, "onto Section, beside View ▸");
        assert_eq!((open.levels.len(), open.levels[0].marked, open.chosen()), (1, Some(3), Some(Act::Section)));

        // From one group straight into the next: the first closes, the second opens.
        let mut open = default_menu();
        open.moved(slot(&open, 0, 4), 0.0, 0.0);
        assert_eq!(open.moved(slot(&open, 0, 5), 0.1, 0.0), Moved::Ring);
        assert_eq!(open.levels.len(), 2);
        assert_eq!(open.levels[0].marked, Some(5));
        assert_eq!(acts(&open, 1), [Act::Undo, Act::Redo]);
        // And back again.
        assert_eq!(open.moved(slot(&open, 0, 4), 0.2, 0.0), Moved::Ring);
        assert_eq!((open.levels.len(), open.levels[0].marked), (2, Some(4)));
        assert_eq!(acts(&open, 1), [Act::PutBack, Act::Explode, Act::Passthrough]);

        // Off past the group's ring, outside everything: it closes, nothing is marked.
        let mut open = default_menu();
        open.moved(slot(&open, 0, 4), 0.0, 0.0);
        assert_eq!(open.moved(off(AT, 0.0, RADIUS * 4.0), 0.1, 0.0), Moved::Ring);
        assert_eq!((open.levels.len(), open.chosen()), (1, None));
    }

    /// The bubble nearest the hand is the one it marks: drifting from the middle toward the bottom
    /// bubble, even well off to the side of the group beside it, never marks that group or opens
    /// its ring.
    #[test]
    fn drifting_toward_the_bottom_bubble_never_picks_the_group_beside_it() {
        let mut open = default_menu();
        for step in 0..=24 {
            let down = RADIUS * 1.2 * f64::from(step) / 24.0;
            for side in [-0.3, -0.15, 0.0, 0.15, 0.3] {
                open.moved(off(AT, RADIUS * side, -down), f64::from(step), 0.0);
                assert_eq!(open.levels.len(), 1, "{down} {side}");
                assert!(!matches!(open.levels[0].marked, Some(4 | 5)), "{down} {side}: {:?}", open.levels[0].marked);
            }
        }
        // Along the ring from the bottom bubble toward the group: Section, then nothing, until
        // the hand is well onto the group's bubble (within 26° of its 60° away).
        for degrees in 180..=212 {
            let (s, c) = f64::from(degrees).to_radians().sin_cos();
            open.moved(off(AT, s * RADIUS, c * RADIUS), 30.0, 0.0);
            assert_eq!(open.levels.len(), 1, "{degrees}°");
            assert!(matches!(open.levels[0].marked, None | Some(3)), "{degrees}°");
        }
        // Between two actions, the nearer is marked.
        let (top, next) = (slot(&open, 0, 0), slot(&open, 0, 1));
        let nearer_next = [0, 1, 2].map(|k| top[k] * 0.4 + next[k] * 0.6);
        open.moved(nearer_next, 31.0, 0.0);
        assert_eq!(open.levels[0].marked, Some(1));
    }

    /// With a wait asked for, a marked group opens once the hand has stayed on it that long;
    /// leaving it starts the wait again.
    #[test]
    fn a_group_opens_after_the_wait_asked_for() {
        let mut open = default_menu();
        let group = slot(&open, 0, 4);
        assert_eq!(open.moved(group, 0.0, 0.5), Moved::Marked);
        assert_eq!(open.chosen(), None, "a group lit is not taken");
        assert_eq!(open.moved(group, 0.3, 0.5), Moved::Nothing);
        assert_eq!(open.levels.len(), 1);
        assert_eq!(open.moved(AT, 0.35, 0.5), Moved::Nothing, "off it: unmarked");
        assert_eq!(open.moved(group, 0.4, 0.5), Moved::Marked);
        assert_eq!(open.moved(group, 0.8, 0.5), Moved::Nothing, "the wait started again");
        assert_eq!(open.moved(group, 0.95, 0.5), Moved::Ring);
        assert_eq!(open.levels.len(), 2);
    }

    /// A slot that cannot be taken now is never marked or taken, a group with nothing in it
    /// never opens, and the settings' rings are kept to what a ring holds: no empty slots, no
    /// groups that are not there, at most [`RING_SLOTS`].
    #[test]
    fn a_slot_that_cannot_be_taken_or_an_empty_group_is_never_marked() {
        let mut open = open_with(&VrSettings::default(), State::default());
        assert_eq!(open.moved(slot(&open, 0, 0), 0.0, 0.0), Moved::Nothing, "Show/Hide with nothing pointed at");
        assert_eq!((open.levels[0].marked, open.chosen()), (None, None));

        let mut set = VrSettings { ring: vec![RingAction::Group(2), RingAction::ShowAll], ..VrSettings::default() };
        set.ring_groups[2] = RingGroup { name: "Empty".into(), icon: "nothing".into(), items: Vec::new() };
        let mut open = open_with(&set, all_on());
        assert!(!open.levels[0].items[0].on, "an empty group is shown off");
        assert_eq!(open.levels[0].items[0].icon, None, "an icon that is not one of the groups' is left out");
        assert_eq!(open.moved(slot(&open, 0, 0), 0.0, 0.0), Moved::Nothing);
        assert_eq!(open.levels.len(), 1);

        let ring = [RingAction::Empty, RingAction::Group(200)].into_iter().chain(std::iter::repeat_n(RingAction::Undo, 40)).collect();
        let mut set = VrSettings { ring, ..VrSettings::default() };
        set.ring_groups[0].items = vec![RingAction::Redo; 1000];
        assert_eq!(items(None, &set, all_on()).len(), RING_SLOTS);
        assert!(items(None, &set, all_on()).iter().all(|i| i.act == Act::Undo));
        assert_eq!(items(Some(0), &set, all_on()).len(), RING_SLOTS);
        assert!(items(Some(99), &set, all_on()).is_empty(), "a group that is not there holds nothing");
        set.ring_groups.clear();
        set.ring = vec![RingAction::Group(0)];
        assert!(items(None, &set, all_on()).is_empty(), "no groups at all");
    }

    /// A hand, a head or a time that is not a number: no ring where the head is at the hand or
    /// lost, a lost hand closes the groups and marks nothing, and a wait that is not a number
    /// never opens a group.
    #[test]
    fn a_hand_or_a_wait_that_is_not_a_number_marks_and_opens_nothing() {
        let set = VrSettings::default();
        assert!(Open::new(0, AT, AT, RADIUS, None, all_on(), &set).is_none(), "the head at the hand");
        assert!(Open::new(0, [f64::NAN; 3], HEAD, RADIUS, None, all_on(), &set).is_none());
        assert!(Open::new(0, AT, [f64::INFINITY; 3], RADIUS, None, all_on(), &set).is_none());
        let odd = Open::new(0, AT, HEAD, f64::NAN, None, all_on(), &set).expect("a ring of the usual size");
        assert_eq!(odd.levels[0].ring.radius, solidcraft_vr::menu::RADIUS);

        let mut open = default_menu();
        open.moved(slot(&open, 0, 4), 0.0, 0.0);
        assert_eq!(open.moved([f64::NAN; 3], 0.1, 0.0), Moved::Ring, "lost: the group closes");
        assert_eq!((open.levels.len(), open.levels[0].marked, open.chosen()), (1, None, None));
        assert_eq!(open.moved([f64::NAN, 0.0, f64::INFINITY], 0.2, 0.0), Moved::Nothing);

        let mut open = default_menu();
        let group = slot(&open, 0, 4);
        for now in [0.0, 1.0, 100.0] {
            open.moved(group, now, f64::NAN);
            assert_eq!(open.levels.len(), 1, "never opens at {now}");
        }
        let mut open = default_menu();
        open.moved(group, f64::NAN, 0.0);
        open.moved(group, f64::NAN, 0.0);
        assert_eq!(open.levels.len(), 1, "a clock that is not a number opens nothing");
    }
}

//! The belt (docs/vr.md): what manages the designs and the room, in one box below and in front of
//! the user. It stays where it is put; Grab on it (or on the handle under it) moves it. Pointing
//! at a group with the ray (or touching it with the controller) and pressing Select opens it, its
//! items branching out forward and up from it as a tree; Select on an item does it. The ring menu
//! stays for what is done to the model.

/// What a belt item does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// Show the open design at this place.
    Switch(usize),
    NewDesign,
    Save,
    Close,
    Passthrough,
    PutBack,
    Undo,
    Redo,
    /// Opens this group's items above it.
    Group(Group),
}

impl Group {
    /// Whether the group opens a window (the Browser) rather than items of its own.
    pub fn is_window(self) -> bool {
        self == Group::Browser
    }
}

/// The belt's groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Designs,
    File,
    Room,
    Edit,
    /// The design's tree, in a window of its own beside the belt.
    Browser,
}

/// The groups, left to right.
pub const GROUPS: [Group; 5] = [Group::Browser, Group::Designs, Group::File, Group::Room, Group::Edit];

/// The most items a group opens (the open designs past it are left out).
pub const MAX_ITEMS: usize = 8;

/// One bubble: what it does, its words (already in the user's language: design names are not in
/// the catalogue), its pictogram, and whether it can be taken now (and is shown lit).
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub act: Act,
    pub words: String,
    pub icon: Option<&'static str>,
    pub on: bool,
    /// Shown as the current one (the design shown, passthrough on).
    pub current: bool,
}

/// What the items depend on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// The open designs' names, and which one is shown.
    pub designs: Vec<String>,
    pub active: usize,
    pub passthrough: bool,
    /// The VR software can show the real room.
    pub passthrough_offered: bool,
    pub can_undo: bool,
    pub can_redo: bool,
}

/// A group's own bubble.
pub fn group_item(group: Group) -> Item {
    use solidcraft_ui_text::tr;
    let (words, icon) = match group {
        Group::Designs => ("vr.belt_designs", "folder"),
        Group::File => ("vr.belt_file", "save"),
        Group::Room => ("vr.belt_room", "eye"),
        Group::Edit => ("vr.group_edit", "edit"),
        Group::Browser => ("vr.belt_browser", "document_browser"),
    };
    Item { act: Act::Group(group), words: tr(words).to_string(), icon: Some(icon), on: true, current: false }
}

/// The items `group` opens, from the bottom up.
pub fn items(group: Group, state: &State) -> Vec<Item> {
    use solidcraft_ui_text::tr;
    let item = |act, words: &str, icon, on| Item { act, words: tr(words).to_string(), icon, on, current: false };
    match group {
        Group::Designs => {
            let mut out: Vec<Item> = state
                .designs
                .iter()
                .enumerate()
                .take(MAX_ITEMS - 1)
                .map(|(i, name)| Item { act: Act::Switch(i), words: name.clone(), icon: None, on: true, current: i == state.active })
                .collect();
            out.push(item(Act::NewDesign, "vr.belt_new", Some("new"), true));
            out
        }
        Group::File => vec![item(Act::Save, "vr.belt_save", Some("save"), true), item(Act::Close, "vr.belt_close", Some("close"), true)],
        Group::Room => vec![
            Item {
                current: state.passthrough && state.passthrough_offered,
                ..item(Act::Passthrough, "vr.belt_passthrough", Some("eye"), state.passthrough_offered)
            },
            item(Act::PutBack, "vr.menu_put_back", Some("home"), true),
        ],
        Group::Edit => {
            vec![item(Act::Undo, "vr.menu_undo", Some("undo"), state.can_undo), item(Act::Redo, "vr.menu_redo", Some("redo"), state.can_redo)]
        }
        Group::Browser => Vec::new(),
    }
}

/// The belt between frames.
#[derive(Default)]
pub struct Belt {
    /// Where it stands: its middle, the way it faces (level, toward the user) and the way its
    /// face looks (toward the head where it was put). `None` until it is first put in front of
    /// the user, and again after [`Belt::put_back`].
    place: Option<([f64; 3], [f64; 3], [f64; 3])>,
    /// The hand moving it, and where the belt is to that hand.
    pub held: Option<Held>,
    /// The group open, by its place in [`GROUPS`].
    pub open: Option<usize>,
    /// The Browser's window is open.
    pub window: bool,
    /// What each hand points at on the belt, left first: the bubble and how far along the ray.
    pub hover: [Option<(Spot, f64)>; 2],
}

/// A hand holding the belt: which, and the belt's middle and the way its face looks in that
/// hand's own frame (its right, up and back), so it goes and turns with the hand as a held part
/// does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Held {
    pub hand: usize,
    offset: [f64; 3],
    looks: [f64; 3],
}

impl Held {
    /// Hand `hand` holding the belt where it is (tests: what letting go lets go of).
    #[cfg(test)]
    pub fn by(hand: usize) -> Held {
        Held { hand, offset: [0.0; 3], looks: [0.0, 0.0, 1.0] }
    }
}

/// Where a hand is and how it is turned: its position and its right, up and back (the room).
pub type HandFrame = ([f64; 3], [[f64; 3]; 3]);

/// `v` (the room) in `axes`' frame.
fn local(axes: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    axes.map(|a| dot(a, v))
}

/// `v` in `axes`' frame back in the room.
fn world(axes: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|k| axes[0][k] * v[0] + axes[1][k] * v[1] + axes[2][k] * v[2])
}

/// A place on the belt: a group (its place), an item of the open group (its place), or the
/// handle it is moved by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    Group(usize),
    Item(usize),
    Handle,
}

/// The curve the belt bends round: an upright cylinder (tilted back with the belt's face) whose
/// axis stands between the belt and the user.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bend {
    /// A point on the axis (room, metres), level with the belt's middle.
    pub axis: [f64; 3],
    /// The axis's direction: the belt's up.
    pub up: [f64; 3],
}

/// A flat area bent onto the belt's curve: its middle and its size (metres).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub middle: [f64; 3],
    pub width: f64,
    pub height: f64,
}

/// Where the belt stands this frame.
#[derive(Clone, Debug)]
pub struct Layout {
    /// Each group's bubble, left to right.
    pub groups: Vec<[f64; 3]>,
    /// The open group's items.
    pub items: Vec<[f64; 3]>,
    /// The bubbles' size (metres).
    pub bubble: f64,
    /// The box around the groups.
    pub panel: Area,
    /// The handle's middle, under the box.
    pub handle: [f64; 3],
    /// The Browser's window, while it is open.
    pub window: Option<Area>,
    /// The curve everything on the belt is bent round.
    pub bend: Bend,
}

/// How many of a group's items stand in a row above it.
const ROW: usize = 4;

/// How far apart the groups stand, in bubbles.
const SPACING: f64 = 1.6;

/// How far the bubbles stand in front of the box (metres), so they are drawn over it.
const LIFT: f64 = 0.004;

/// How wide the Browser's window is for its height.
pub const WINDOW_RATIO: f64 = 0.62;

/// `v` made a unit vector, `None` when it has no length.
fn unit(v: [f64; 3]) -> Option<[f64; 3]> {
    let l = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    (l > 1e-9 && l.is_finite()).then(|| v.map(|x| x / l))
}

/// `a` × `b`.
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// `a` · `b`.
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `v` without its part along unit `up`, and that part.
fn flat(v: [f64; 3], up: [f64; 3]) -> ([f64; 3], f64) {
    let along = dot(v, up);
    ([0, 1, 2].map(|k| v[k] - up[k] * along), along)
}

impl Bend {
    /// The point `along` (metres, along the curve; positive to the user's right) and `rise`
    /// (metres, up the belt) from the belt's middle, `radius` from the axis. `toward` is the
    /// way from the belt's middle to the axis.
    fn at(&self, toward: [f64; 3], radius: f64, along: f64, rise: f64) -> [f64; 3] {
        let right = cross(toward.map(|v| -v), self.up);
        let a = along / radius.max(1e-6);
        let (sin, cos) = a.sin_cos();
        [0, 1, 2].map(|k| self.axis[k] + self.up[k] * rise + radius * (-toward[k] * cos + right[k] * sin))
    }

    /// Where the ray from `origin` along unit `dir` meets the curve `radius` from the axis,
    /// going outward (the user is inside it), and how far along the ray.
    pub fn meet(&self, origin: [f64; 3], dir: [f64; 3], radius: f64) -> Option<([f64; 3], f64)> {
        let (o, _) = flat([0, 1, 2].map(|k| origin[k] - self.axis[k]), self.up);
        let (d, _) = flat(dir, self.up);
        let a = dot(d, d);
        let b = 2.0 * dot(o, d);
        let c = dot(o, o) - radius * radius;
        let disc = b * b - 4.0 * a * c;
        if a < 1e-12 || disc < 0.0 {
            return None;
        }
        let t = (-b + disc.sqrt()) / (2.0 * a);
        (t > 0.0 && t.is_finite()).then(|| ([0, 1, 2].map(|k| origin[k] + dir[k] * t), t))
    }

    /// Where `point` (on the curve) is on `area` (bent onto the same curve): across (0 at the
    /// left edge, 1 at the right) and down (0 at the top, 1 at the bottom), as its picture is.
    pub fn on_area(&self, area: &Area, point: [f64; 3]) -> [f64; 2] {
        let (m, m_up) = flat([0, 1, 2].map(|k| area.middle[k] - self.axis[k]), self.up);
        let (p, p_up) = flat([0, 1, 2].map(|k| point[k] - self.axis[k]), self.up);
        let (Some(out), Some(to)) = (unit(m), unit(p)) else { return [f64::NAN; 2] };
        // Seen from the axis (where the user is), to the right is a turn the other way about up.
        let angle = -dot(cross(out, to), self.up).atan2(dot(out, to));
        let radius = dot(m, m).sqrt();
        [0.5 + angle * radius / area.width, 0.5 - (p_up - m_up) / area.height]
    }

    /// The axis point and up of the curve for drawing: a board at `at` is bent round it.
    pub fn of(&self) -> ([f64; 3], [f64; 3]) {
        (self.axis, self.up)
    }
}

impl Belt {
    /// Put the belt back in front of the user (the next [`Belt::place`]).
    pub fn put_back(&mut self) {
        self.place = None;
        self.held = None;
    }

    /// Held, the belt goes `step` further from the hand holding it (metres; less than nothing:
    /// closer), never nearer than a hand's breadth; a step that is not a number moves nothing.
    pub fn push(&mut self, step: f64) {
        if let Some(held) = self.held.as_mut() {
            let offset = held.offset;
            let length = offset.iter().map(|v| v * v).sum::<f64>().sqrt();
            if step.is_finite() && length > 1e-6 && length + step > 0.08 {
                held.offset = offset.map(|v| v / length * (length + step));
            }
        }
    }

    /// Take the belt with hand `hand`, where `frame` says it is: it keeps its place and its turn
    /// to the hand. A hand whose pose is not a number takes nothing.
    pub fn grab(&mut self, hand: usize, frame: HandFrame) {
        let (at, axes) = frame;
        if !at.iter().chain(axes.iter().flatten()).all(|v| v.is_finite()) {
            return;
        }
        if let Some((centre, _, looks)) = self.place {
            self.held = Some(Held { hand, offset: local(&axes, [0, 1, 2].map(|k| centre[k] - at[k])), looks: local(&axes, looks) });
        }
    }

    /// Where the belt stands this frame. The first time (and after [`Belt::put_back`]) it is put
    /// `drop` below the eyes and `distance` in front of the head at `head`, looking down `back`'s
    /// opposite; held, it goes and turns with the hand (`hands`: where each hand is and how it is
    /// turned) as a held part does, but stays level (it never leans to a side); otherwise it stays
    /// where it is, as it is. It bends round toward the user, `curve`
    /// from the axis; its bubbles `bubble` tall; the open group's items (`count` of them) branch
    /// out from it (at most [`MAX_ITEMS`]); the Browser's window (`window`: its height and side)
    /// stands beside it. A head or a hand whose pose is not a number moves it nowhere.
    #[allow(clippy::too_many_arguments)]
    pub fn place(
        &mut self,
        head: [f64; 3],
        back: [f64; 3],
        drop: f64,
        distance: f64,
        curve: f64,
        bubble: f64,
        count: usize,
        hands: [Option<HandFrame>; 2],
        window: (f64, solidcraft_ui_egui::vr_settings::BrowserSide),
    ) -> Layout {
        let level = |v: [f64; 3]| unit([v[0], 0.0, v[2]]);
        let face = |centre: [f64; 3], facing: [f64; 3]| unit([0, 1, 2].map(|k| head[k] - centre[k])).unwrap_or(facing);
        let (mut centre, mut facing, mut looks) = match self.place {
            Some(place) => place,
            None => {
                let ahead = level([-back[0], 0.0, -back[2]]).unwrap_or([0.0, 0.0, -1.0]);
                let centre = [head[0] + ahead[0] * distance, head[1] - drop, head[2] + ahead[2] * distance];
                let facing = ahead.map(|v| -v);
                (centre, facing, face(centre, facing))
            }
        };
        if let Some(held) = self.held
            && let Some((at, axes)) = hands.get(held.hand).copied().flatten()
        {
            let offset = world(&axes, held.offset);
            let moved = [0, 1, 2].map(|k| at[k] + offset[k]);
            if moved.iter().all(|v| v.is_finite()) {
                centre = moved;
            }
            // Its face turns with the hand; its right stays level (worked out from the facing).
            if let Some(turned) = unit(world(&axes, held.looks))
                && let Some(level_facing) = level(turned)
            {
                facing = level_facing;
                looks = turned;
            }
        }
        if centre.iter().all(|v| v.is_finite()) {
            self.place = Some((centre, facing, looks));
        }
        // The belt's own frame: its face looks toward the head as it was when put down, its
        // right is level, its up square to both.
        let right = unit(cross(facing.map(|v| -v), [0.0, 1.0, 0.0])).unwrap_or([1.0, 0.0, 0.0]);
        let up = unit(cross(looks, right)).unwrap_or([0.0, 1.0, 0.0]);
        let toward = unit(cross(right, up)).unwrap_or(facing);
        let radius = curve.max(0.05);
        let bend = Bend { axis: [0, 1, 2].map(|k| centre[k] + toward[k] * radius), up };
        let n = GROUPS.len() as f64;
        let step = bubble * SPACING;
        let groups: Vec<[f64; 3]> = (0..GROUPS.len()).map(|i| bend.at(toward, radius - LIFT, (i as f64 - (n - 1.0) * 0.5) * step, 0.0)).collect();
        let panel = Area { middle: centre, width: n * step + bubble * 0.4, height: bubble * 1.5 };
        let handle = bend.at(toward, radius - LIFT, 0.0, -panel.height * 0.5 - bubble * 0.3);
        // The open group's items stand in rows above the box, from their group up, in the belt's
        // own curved face (in front of nothing, behind nothing), as far apart as the groups.
        let count = count.min(MAX_ITEMS);
        let items = match self.open {
            Some(g) => {
                let along = (g as f64 - (n - 1.0) * 0.5) * step;
                let first = panel.height * 0.5 + bubble * 0.75;
                (0..count)
                    .map(|j| {
                        let (row, at) = (j / ROW, j % ROW);
                        let in_row = (count - row * ROW).min(ROW) as f64;
                        let across = (at as f64 - (in_row - 1.0) * 0.5) * step;
                        bend.at(toward, radius - LIFT, along + across, first + row as f64 * bubble * 1.2)
                    })
                    .collect()
            }
            None => Vec::new(),
        };
        // The Browser's window, on the belt's curve beside it (or above it), its bottom level
        // with the box's.
        let window = self.window.then(|| {
            use solidcraft_ui_egui::vr_settings::BrowserSide;
            let (height, side) = window;
            let width = height * WINDOW_RATIO;
            let gap = bubble * 0.4;
            let (along, rise) = match side {
                BrowserSide::Left => (-(panel.width + width) * 0.5 - gap, (height - panel.height) * 0.5),
                BrowserSide::Right => ((panel.width + width) * 0.5 + gap, (height - panel.height) * 0.5),
                BrowserSide::Above => (0.0, (height + panel.height) * 0.5 + gap),
            };
            Area { middle: bend.at(toward, radius, along, rise), width, height }
        });
        Layout { groups, items, bubble, panel, handle, window, bend }
    }
}

impl Layout {
    /// The nearest bubble (or the handle) the ray from `origin` along unit `dir` passes through,
    /// and how far along; also one the point `origin` is inside (a touch, at 0).
    pub fn hit(&self, origin: [f64; 3], dir: [f64; 3]) -> Option<(Spot, f64)> {
        let radius = self.bubble * 0.55;
        let spots = self
            .groups
            .iter()
            .enumerate()
            .map(|(i, p)| (Spot::Group(i), *p))
            .chain(self.items.iter().enumerate().map(|(i, p)| (Spot::Item(i), *p)))
            .chain(std::iter::once((Spot::Handle, self.handle)));
        let mut best: Option<(Spot, f64)> = None;
        for (spot, p) in spots {
            let to = [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
            let along = to[0] * dir[0] + to[1] * dir[1] + to[2] * dir[2];
            let gap2 = to.iter().map(|v| v * v).sum::<f64>() - along * along;
            let touch = to.iter().map(|v| v * v).sum::<f64>().sqrt() < radius;
            let t = if touch {
                0.0
            } else if along > 0.0 && gap2 <= radius * radius {
                along
            } else {
                continue;
            };
            if best.is_none_or(|(_, b)| t < b) {
                best = Some((spot, t));
            }
        }
        best
    }

    /// Where the ray from `origin` along unit `dir` meets the Browser's window: across and down
    /// it (0 to 1, as its picture is), and how far along the ray.
    pub fn window_hit(&self, origin: [f64; 3], dir: [f64; 3]) -> Option<([f64; 2], f64)> {
        let area = self.window?;
        let (m, _) = flat([0, 1, 2].map(|k| area.middle[k] - self.bend.axis[k]), self.bend.up);
        let (point, t) = self.bend.meet(origin, dir, dot(m, m).sqrt())?;
        let uv = self.bend.on_area(&area, point);
        (uv.iter().all(|v| (0.0..=1.0).contains(v))).then_some((uv, t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The belt put in front of a head looking down −Z: its groups run left to right across the
    /// user's view, bend toward them, and a ray at the window's middle meets it there.
    #[test]
    fn the_belt_bends_toward_the_user_and_its_window_is_found() {
        let mut belt = Belt { window: true, ..Belt::default() };
        let side = solidcraft_ui_egui::vr_settings::BrowserSide::Left;
        let layout = belt.place([0.0, 1.6, 0.0], [0.0, 0.0, 1.0], 0.4, 0.35, 0.6, 0.045, 0, [None, None], (0.34, side));
        // Held and the hand turned a quarter about the vertical: the belt turns with it and stays
        // level.
        let start = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        belt.grab(1, ([0.2, 1.0, -0.3], start));
        let turned = [[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]];
        let moved = belt.place([0.0, 1.6, 0.0], [0.0, 0.0, 1.0], 0.4, 0.35, 0.6, 0.045, 0, [None, Some(([0.2, 1.0, -0.3], turned))], (0.34, side));
        let ends = (moved.groups[0], moved.groups[GROUPS.len() - 1]);
        assert!((ends.0[1] - ends.1[1]).abs() < 1e-9, "level: {ends:?}");
        assert!((ends.0[0] - ends.1[0]).abs() < (ends.0[2] - ends.1[2]).abs(), "turned a quarter: {ends:?}");
        let (first, last) = (layout.groups[0], layout.groups[GROUPS.len() - 1]);
        assert!(first[0] < last[0], "left to right: {first:?} {last:?}");
        let middle = layout.groups[GROUPS.len() / 2];
        assert!(first[2] > middle[2] && last[2] > middle[2], "the ends come toward the user");
        let window = layout.window.expect("open");
        assert!(window.middle[0] < first[0], "on the left");
        let head = [0.0, 1.6, 0.0];
        let to = unit([0, 1, 2].map(|k| window.middle[k] - head[k])).unwrap();
        let (uv, _) = layout.window_hit(head, to).expect("a ray at the window meets it");
        assert!((uv[0] - 0.5).abs() < 0.02 && (uv[1] - 0.5).abs() < 0.02, "{uv:?}");
    }

    /// Where the head is in these tests.
    const HEAD: [f64; 3] = [0.0, 1.6, 0.0];
    /// The way the head's back points: it looks down −Z.
    const BACK: [f64; 3] = [0.0, 0.0, 1.0];

    /// The Browser's window as the settings have it by default.
    fn window() -> (f64, solidcraft_ui_egui::vr_settings::BrowserSide) {
        (0.42, solidcraft_ui_egui::vr_settings::BrowserSide::Left)
    }

    /// `belt` placed for the head above, with the default drop, distance, curve and bubble, `count`
    /// items open and the hands as given.
    fn put(belt: &mut Belt, count: usize, hands: [Option<HandFrame>; 2]) -> Layout {
        belt.place(HEAD, BACK, 0.4, 0.35, 0.6, 0.045, count, hands, window())
    }

    /// `v` turned by `angle` (radians) about unit `axis` (right-handed).
    fn turn(axis: [f64; 3], angle: f64, v: [f64; 3]) -> [f64; 3] {
        let (sin, cos) = angle.sin_cos();
        let c = cross(axis, v);
        let along = dot(axis, v) * (1.0 - cos);
        [0, 1, 2].map(|k| v[k] * cos + c[k] * sin + axis[k] * along)
    }

    /// The length of `v`.
    fn length(v: [f64; 3]) -> f64 {
        dot(v, v).sqrt()
    }

    /// `a` − `b`.
    fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [0, 1, 2].map(|k| a[k] - b[k])
    }

    /// Whether two bubbles of a layout, `bubble` tall and as wide as their picture, cover each
    /// other where they stand on the belt's face: nearer than their width across and than their
    /// height up the belt.
    fn overlap(layout: &Layout, a: [f64; 3], b: [f64; 3]) -> bool {
        let [w, h] = super::super::sign::BUBBLE_PX;
        let width = layout.bubble * f64::from(w) / f64::from(h);
        let (across, up) = flat(sub(a, b), layout.bend.up);
        length(across) < width && up.abs() < layout.bubble
    }

    /// The bug where a group's items covered each other: for every group and any number of items
    /// (past the most, only the most stand), the items stand in rows of [`ROW`] above the box,
    /// rows 1.2 bubbles apart, no two bubbles covering each other nor their hit spheres touching,
    /// for the default belt and the settings' extremes.
    #[test]
    fn a_groups_items_stand_in_rows_above_the_box_without_covering_each_other() {
        for (curve, bubble) in [(0.6, 0.045), (0.2, 0.045), (5.0, 0.045), (0.6, 0.015), (5.0, 0.015), (0.6, 0.15)] {
            for g in 0..GROUPS.len() {
                for count in (1..=12).chain([MAX_ITEMS]) {
                    let mut belt = Belt { open: Some(g), ..Belt::default() };
                    let layout = belt.place(HEAD, BACK, 0.4, 0.35, curve, bubble, count, [None, None], window());
                    let what = format!("curve {curve}, bubble {bubble}, group {g}, {count} items");
                    assert_eq!(layout.items.len(), count.min(MAX_ITEMS), "{what}");
                    let rise = |p: [f64; 3]| dot(sub(p, layout.bend.axis), layout.bend.up);
                    let top = rise(layout.panel.middle) + layout.panel.height * 0.5;
                    for (j, item) in layout.items.iter().enumerate() {
                        assert!(rise(*item) - bubble * 0.5 > top, "{what}: item {j} stands above the box");
                        for group in &layout.groups {
                            assert!(!overlap(&layout, *item, *group), "{what}: item {j} clear of the groups");
                        }
                        for (k, other) in layout.items.iter().enumerate().skip(j + 1) {
                            assert!(!overlap(&layout, *item, *other), "{what}: items {j} and {k} cover each other");
                            assert!(length(sub(*item, *other)) > 1.1 * bubble, "{what}: items {j} and {k} hit as one");
                            if k == j + ROW {
                                assert!((rise(*other) - rise(*item) - 1.2 * bubble).abs() < 1e-9, "{what}: rows 1.2 bubbles apart");
                            }
                            if k / ROW == j / ROW {
                                assert!((rise(*other) - rise(*item)).abs() < 1e-9, "{what}: one row is level");
                            }
                        }
                    }
                }
            }
        }
    }

    /// No group open, none of its items, or far more than a group holds: nothing stands, or only
    /// the most a group opens (the count is not trusted to size the layout).
    #[test]
    fn a_count_of_nothing_or_far_too_many_items_is_kept_to_what_a_group_holds() {
        let mut belt = Belt { open: Some(1), ..Belt::default() };
        assert!(put(&mut belt, 0, [None, None]).items.is_empty());
        assert_eq!(put(&mut belt, usize::MAX, [None, None]).items.len(), MAX_ITEMS);
        let mut closed = Belt::default();
        assert!(put(&mut closed, 5, [None, None]).items.is_empty(), "no group open");
        let mut past = Belt { open: Some(GROUPS.len() + 3), ..Belt::default() };
        let layout = put(&mut past, MAX_ITEMS, [None, None]);
        assert!(layout.items.iter().flatten().all(|v| v.is_finite()), "a group past the last still stands somewhere");
        // The groups' own items never ask for more than the belt draws.
        let state = State { designs: (0..50).map(|i| format!("Design {i}")).collect(), ..State::default() };
        for group in GROUPS {
            assert!(items(group, &state).len() <= MAX_ITEMS, "{group:?}");
        }
    }

    /// A held belt goes and turns with a hand that is moved, turned, tipped and rolled: its
    /// middle keeps its distance and side to the hand, its face turns with the hand, and its
    /// right stays level (it never leans to a side).
    #[test]
    fn a_held_belt_turns_with_a_tipped_rolled_hand_and_its_right_stays_level() {
        let mut belt = Belt::default();
        let first = put(&mut belt, 0, [None, None]);
        let looks = unit(sub(HEAD, first.panel.middle)).expect("the face looks at the head");
        let hand = [0.15, 1.05, -0.2];
        let start = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        belt.grab(0, (hand, start));
        assert_eq!(belt.held.map(|h| h.hand), Some(0));
        let offset = sub(first.panel.middle, hand);
        // Rolled about the hand's own forward, tipped about its right, turned about the vertical.
        let r = |v: [f64; 3]| turn([0.0, 1.0, 0.0], 1.2, turn([1.0, 0.0, 0.0], 0.6, turn([0.0, 0.0, 1.0], 0.5, v)));
        let to = [0.4, 1.3, 0.1];
        let moved = put(&mut belt, 0, [Some((to, start.map(r))), None]);
        let middle = moved.panel.middle;
        let expected = [0, 1, 2].map(|k| to[k] + r(offset)[k]);
        assert!(length(sub(middle, expected)) < 1e-9, "it goes with the hand: {middle:?} {expected:?}");
        let (left, right) = (moved.groups[0], moved.groups[GROUPS.len() - 1]);
        assert!((left[1] - right[1]).abs() < 1e-9, "level: {left:?} {right:?}");
        // Its right is square to its face turned with the hand, made level.
        let turned = r(looks);
        let facing = unit([turned[0], 0.0, turned[2]]).expect("level");
        let across = unit(sub(right, left)).expect("apart");
        let expected_right = unit(cross(facing.map(|v| -v), [0.0, 1.0, 0.0])).expect("right");
        assert!(dot(across, expected_right) > 0.99, "turned with the hand: {across:?} {expected_right:?}");
        // Its up is square to its right and to the face, tipped with the hand.
        let up = moved.bend.up;
        assert!(dot(up, expected_right).abs() < 1e-9 && dot(up, turned).abs() < 1e-9, "{up:?}");
        // Let go and still: it stays where the hand left it, as it was.
        belt.held = None;
        let left_there = put(&mut belt, 0, [None, None]);
        assert!(length(sub(left_there.panel.middle, middle)) < 1e-12 && left_there.bend == moved.bend);
    }

    /// A hand turned so the belt's face would look straight up: there is no level facing to take,
    /// so the belt keeps its last one and stays level.
    #[test]
    fn a_belt_turned_face_up_keeps_its_last_facing() {
        let mut belt = Belt::default();
        let first = put(&mut belt, 0, [None, None]);
        let looks = unit(sub(HEAD, first.panel.middle)).expect("the face looks at the head");
        let start = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let hand = [0.0, 1.0, -0.2];
        belt.grab(1, (hand, start));
        let axis = unit(cross(looks, [0.0, 1.0, 0.0])).expect("not already up");
        let angle = dot(looks, [0.0, 1.0, 0.0]).acos();
        let up = put(&mut belt, 0, [None, Some((hand, start.map(|v| turn(axis, angle, v))))]);
        let (left, right) = (up.groups[0], up.groups[GROUPS.len() - 1]);
        assert!((left[1] - right[1]).abs() < 1e-9, "level");
        assert!(left[0] < right[0], "still facing the user");
        assert!(up.groups.iter().flatten().all(|v| v.is_finite()));
    }

    /// The stick pushes a held belt away from the hand and pulls it closer along the line between
    /// them, never nearer than a hand's breadth; a belt not held is not pushed.
    #[test]
    fn a_held_belt_is_pushed_along_the_line_from_the_hand_never_into_it() {
        let mut belt = Belt::default();
        let first = put(&mut belt, 0, [None, None]);
        belt.push(0.5);
        assert_eq!(put(&mut belt, 0, [None, None]).panel.middle, first.panel.middle, "not held: not pushed");
        let hand = [0.1, 1.0, -0.1];
        let start = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let hands = [None, Some((hand, start))];
        belt.grab(1, (hand, start));
        let before = sub(first.panel.middle, hand);
        belt.push(0.1);
        let after = sub(put(&mut belt, 0, hands).panel.middle, hand);
        assert!((length(after) - length(before) - 0.1).abs() < 1e-9, "{} → {}", length(before), length(after));
        assert!(dot(unit(after).unwrap(), unit(before).unwrap()) > 1.0 - 1e-12, "along the same line");
        belt.push(-10.0);
        assert!((length(sub(put(&mut belt, 0, hands).panel.middle, hand)) - length(after)).abs() < 1e-12, "not through the hand");
        belt.push(0.09 - length(after));
        let near = length(sub(put(&mut belt, 0, hands).panel.middle, hand));
        assert!((near - 0.09).abs() < 1e-9, "{near}");
        belt.push(-0.02);
        let nearer = length(sub(put(&mut belt, 0, hands).panel.middle, hand));
        assert!((nearer - 0.09).abs() < 1e-9, "never nearer than a hand's breadth: {nearer}");
    }

    /// A head, hand or push that is not a number leaves the belt where it was: it is not taken by
    /// a hand whose pose is not one, does not follow one, and a first place at a head that is
    /// not one is tried again next frame.
    #[test]
    fn a_pose_or_push_that_is_not_a_number_leaves_the_belt_where_it_was() {
        let nan = [f64::NAN; 3];
        let mut belt = Belt::default();
        let lost = belt.place(nan, BACK, 0.4, 0.35, 0.6, 0.045, 0, [None, None], window());
        assert!(lost.hit(HEAD, [0.0, 0.0, -1.0]).is_none(), "nothing to hit");
        let first = put(&mut belt, 0, [None, None]);
        assert!(first.panel.middle.iter().all(|v| v.is_finite()), "placed once the head is known");
        let start = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        belt.grab(0, (nan, start));
        belt.grab(0, ([0.0, 1.0, 0.0], [nan, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]));
        assert!(belt.held.is_none(), "a lost hand takes nothing");
        let hand = [0.0, 1.0, -0.1];
        belt.grab(0, (hand, start));
        for frame in [(nan, start), (hand, [nan; 3]), ([f64::INFINITY, 0.0, 0.0], start)] {
            let layout = put(&mut belt, 0, [Some(frame), None]);
            assert_eq!(layout.panel.middle, first.panel.middle, "{frame:?}");
        }
        for step in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            belt.push(step);
            assert_eq!(put(&mut belt, 0, [Some((hand, start)), None]).panel.middle, first.panel.middle, "{step}");
        }
        // A frame with a lost hand on the other side changes nothing either.
        assert_eq!(put(&mut belt, 0, [Some((hand, start)), Some((nan, [nan; 3]))]).panel.middle, first.panel.middle);
    }

    /// The ray finds the bubble it passes through (a group, an open item, the handle), a hand
    /// inside a bubble touches it, and a ray past them all, away from them or from nowhere finds
    /// nothing.
    #[test]
    fn a_ray_or_a_touch_finds_the_bubble_it_is_on() {
        let mut belt = Belt { open: Some(2), ..Belt::default() };
        let layout = put(&mut belt, 6, [None, None]);
        let at = |p: [f64; 3]| unit(sub(p, HEAD)).expect("apart");
        for (g, p) in layout.groups.iter().enumerate() {
            let (spot, t) = layout.hit(HEAD, at(*p)).expect("hit");
            assert_eq!(spot, Spot::Group(g));
            assert!((t - length(sub(*p, HEAD))).abs() < 1e-9);
        }
        for (j, p) in layout.items.iter().enumerate() {
            assert_eq!(layout.hit(HEAD, at(*p)).map(|h| h.0), Some(Spot::Item(j)));
        }
        assert_eq!(layout.hit(HEAD, at(layout.handle)).map(|h| h.0), Some(Spot::Handle));
        let inside = [layout.groups[3][0] + 0.01, layout.groups[3][1], layout.groups[3][2]];
        assert_eq!(layout.hit(inside, [0.0, 1.0, 0.0]), Some((Spot::Group(3), 0.0)), "a touch");
        assert!(layout.hit(HEAD, [0.0, 1.0, 0.0]).is_none(), "looking up");
        assert!(layout.hit(HEAD, at(layout.groups[2]).map(|v| -v)).is_none(), "looking away");
        assert!(layout.hit([f64::NAN; 3], [0.0, 0.0, -1.0]).is_none());
        assert!(layout.hit(HEAD, [f64::NAN; 3]).is_none());
    }

    /// A point on the curve at a distance along it and up it: the ray from the axis toward it
    /// meets the curve there, and the point is found on an area bent onto the curve as far across
    /// and down as it is from the area's middle.
    #[test]
    fn a_point_on_the_curve_is_met_by_a_ray_and_found_on_its_area() {
        let bend = Bend { axis: [0.0, 1.0, 0.0], up: [0.0, 1.0, 0.0] };
        let toward = [0.0, 0.0, 1.0];
        let radius = 0.6;
        let area = Area { middle: bend.at(toward, radius, 0.0, 0.0), width: 0.4, height: 0.3 };
        assert!(length(sub(area.middle, [0.0, 1.0, -0.6])) < 1e-12, "in front of the axis");
        for along in [-0.3, -0.1, 0.0, 0.05, 0.2] {
            for rise in [-0.15, 0.0, 0.1] {
                let p = bend.at(toward, radius, along, rise);
                let from = [0.0, 1.0 + rise * 0.5, 0.0];
                let (met, t) = bend.meet(from, unit(sub(p, from)).unwrap(), radius).expect("met");
                assert!(length(sub(met, p)) < 1e-9 && (t - length(sub(p, from))).abs() < 1e-9, "{along} {rise}");
                let uv = bend.on_area(&area, p);
                let expected = [0.5 + along / area.width, 0.5 - rise / area.height];
                assert!((uv[0] - expected[0]).abs() < 1e-9 && (uv[1] - expected[1]).abs() < 1e-9, "{uv:?} {expected:?}");
            }
        }
        // Along the axis, or from nowhere, the ray meets nothing; a point on the axis is on no area.
        assert!(bend.meet([0.0, 1.0, 0.0], [0.0, 1.0, 0.0], radius).is_none());
        assert!(bend.meet([f64::NAN; 3], [0.0, 0.0, -1.0], radius).is_none());
        assert!(bend.meet([0.0, 1.0, 0.0], [0.0, 0.0, -1.0], f64::NAN).is_none());
        assert!(bend.on_area(&area, [0.0, 2.0, 0.0]).iter().all(|v| v.is_nan()));
    }

    /// The Browser's window is hit only inside it: near its corners inside, not past its edges,
    /// not while it is closed, and not by a ray that is not one.
    #[test]
    fn the_browsers_window_is_hit_only_inside_it() {
        let mut belt = Belt { window: true, ..Belt::default() };
        let layout = put(&mut belt, 0, [None, None]);
        let area = layout.window.expect("open");
        let strips = super::super::bent(area.middle, &layout.bend, area.width, area.height).expect("a board");
        let (first, last) = (strips[0].0, strips[strips.len() - 1].0);
        // Just inside each corner (top left, top right, bottom right, bottom left), and just past.
        let corners = [first[0], last[1], last[2], first[3]];
        for (corner, uv) in corners.iter().zip([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]) {
            let towards = sub(area.middle, *corner);
            let inside = [0, 1, 2].map(|k| corner[k] + towards[k] * 0.02);
            let outside = [0, 1, 2].map(|k| corner[k] - towards[k] * 0.02);
            let (hit, _) = layout.window_hit(HEAD, unit(sub(inside, HEAD)).unwrap()).expect("inside");
            assert!((hit[0] - uv[0]).abs() < 0.03 && (hit[1] - uv[1]).abs() < 0.03, "{hit:?} {uv:?}");
            assert!(layout.window_hit(HEAD, unit(sub(outside, HEAD)).unwrap()).is_none(), "past {uv:?}");
        }
        let to = unit(sub(area.middle, HEAD)).unwrap();
        assert!(layout.window_hit(HEAD, to.map(|v| -v)).is_none(), "looking away");
        assert!(layout.window_hit(HEAD, [f64::NAN; 3]).is_none());
        assert!(layout.window_hit([f64::NAN; 3], to).is_none());
        belt.window = false;
        assert!(put(&mut belt, 0, [None, None]).window_hit(HEAD, to).is_none(), "closed");
    }
}

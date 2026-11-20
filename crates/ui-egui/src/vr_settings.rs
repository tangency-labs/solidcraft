//! VR settings (Preferences › XR › VR, docs/vr.md): which button does what, and how the grabs, the
//! menu and the signs behave. Each one applies at once, also while the headset is on: what feels
//! right in VR differs from person to person, so a way of working is tried by changing a setting,
//! not by a new build. The defaults are what the viewer did before these were settings, but for
//! the two-hand grab: out of the box both hands turn the model too ([`TwoHands::Equal`], new with
//! layout 4), where the viewer without settings (`solidcraft_vr::grab::Mode::default`) only
//! scales and moves it. Settings saved for an older layout take the newer defaults once, when
//! they are read ([`VrSettings::migrate`]).

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::SolveApp;
use crate::ui_kit;

/// A controller's physical input, as the VR software reports it (tracked hands: a pinch is the
/// trigger, a closed hand the grip).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Input {
    Trigger,
    Grip,
    /// A (X on a left Touch controller).
    Primary,
    /// B (Y on a left Touch controller).
    Secondary,
    StickClick,
    Menu,
}

impl Input {
    /// Every input, in the order the page lists them.
    pub const ALL: [Input; 6] = [Input::Trigger, Input::Grip, Input::Primary, Input::Secondary, Input::StickClick, Input::Menu];

    /// Its name on the page (a catalogue entry).
    fn words(self) -> &'static str {
        match self {
            Input::Trigger => "prefs.vr.input_trigger",
            Input::Grip => "prefs.vr.input_grip",
            Input::Primary => "prefs.vr.input_primary",
            Input::Secondary => "prefs.vr.input_secondary",
            Input::StickClick => "prefs.vr.input_stick_click",
            Input::Menu => "prefs.vr.input_menu",
        }
    }
}

/// The inputs that do an action, on each hand.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Binding {
    pub left: Vec<Input>,
    pub right: Vec<Input>,
}

impl Binding {
    /// `left` on the left hand, `right` on the right.
    fn of(left: &[Input], right: &[Input]) -> Binding {
        Binding { left: left.to_vec(), right: right.to_vec() }
    }

    /// The inputs on hand `i` (0 left, 1 right).
    pub fn on(&self, i: usize) -> &[Input] {
        match i {
            0 => &self.left,
            1 => &self.right,
            _ => &[],
        }
    }

    /// Whether `input` on hand `i` does the action.
    pub fn reads(&self, i: usize, input: Input) -> bool {
        self.on(i).contains(&input)
    }

    /// No input does the action.
    pub fn is_empty(&self) -> bool {
        self.left.is_empty() && self.right.is_empty()
    }

    /// The inputs on hand `i` (0 left, 1 right) to change; `None` past the right hand.
    fn on_mut(&mut self, i: usize) -> Option<&mut Vec<Input>> {
        match i {
            0 => Some(&mut self.left),
            1 => Some(&mut self.right),
            _ => None,
        }
    }

    /// Takes `input` off hand `i` (a chip's Remove); the other hand and the other inputs stay.
    pub fn remove(&mut self, i: usize, input: Input) {
        if let Some(list) = self.on_mut(i) {
            list.retain(|x| *x != input);
        }
    }

    /// Puts `input` on hand `i` (the "+" menu), once: an input the hand has already is not added
    /// again.
    pub fn add(&mut self, i: usize, input: Input) {
        if let Some(list) = self.on_mut(i)
            && !list.contains(&input)
        {
            list.push(input);
        }
    }

    /// The inputs the "+" menu offers for hand `i`: those it does not have yet (none past the
    /// right hand).
    pub fn unused(&self, i: usize) -> Vec<Input> {
        if i > 1 {
            return Vec::new();
        }
        Input::ALL.into_iter().filter(|x| !self.reads(i, *x)).collect()
    }
}

/// The layout of the buttons the defaults are for: saved settings of an older layout get the
/// buttons (and the stick) of this one once, the rest kept.
pub const LAYOUT: u32 = 4;

/// What a ring menu slot does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RingAction {
    /// Nothing: the slot is left out.
    Empty,
    /// Hide what is pointed at or selected, or show it again when it is all hidden.
    ToggleHide,
    Hide,
    Isolate,
    ShowAll,
    Explode,
    Section,
    PutBack,
    Undo,
    Redo,
    /// The real room around the model, on or off.
    Passthrough,
    /// Opens one of [`VrSettings::ring_groups`] (by its place, 0 to 3).
    Group(u8),
}

impl RingAction {
    /// Every action a slot can hold, groups last ([`GROUPS`] of them).
    pub fn all() -> Vec<RingAction> {
        let mut out = vec![
            RingAction::Empty,
            RingAction::ToggleHide,
            RingAction::Hide,
            RingAction::Isolate,
            RingAction::ShowAll,
            RingAction::Explode,
            RingAction::Section,
            RingAction::PutBack,
            RingAction::Undo,
            RingAction::Redo,
            RingAction::Passthrough,
        ];
        out.extend((0..GROUPS as u8).map(RingAction::Group));
        out
    }

    /// Its name (a catalogue entry); a group is named by its own name.
    pub fn words(self) -> &'static str {
        match self {
            RingAction::Empty => "prefs.vr.ring_empty",
            RingAction::ToggleHide => "vr.menu_toggle_hide",
            RingAction::Hide => "vr.menu_hide",
            RingAction::Isolate => "vr.menu_isolate",
            RingAction::ShowAll => "vr.menu_show_all",
            RingAction::Explode => "vr.menu_explode",
            RingAction::Section => "vr.menu_section",
            RingAction::PutBack => "vr.menu_put_back",
            RingAction::Undo => "vr.menu_undo",
            RingAction::Redo => "vr.menu_redo",
            RingAction::Passthrough => "vr.belt_passthrough",
            RingAction::Group(_) => "prefs.vr.ring_group",
        }
    }

    /// Its pictogram (an icon name), where one fits.
    pub fn icon(self) -> Option<&'static str> {
        Some(match self {
            RingAction::Empty | RingAction::Group(_) => return None,
            RingAction::ToggleHide => "eye_off",
            RingAction::Hide => "eye_off",
            RingAction::Isolate => "isolate",
            RingAction::ShowAll => "eye",
            RingAction::Explode => "explode",
            RingAction::Section => "section",
            RingAction::PutBack => "home",
            RingAction::Undo => "undo",
            RingAction::Redo => "redo",
            RingAction::Passthrough => "eye",
        })
    }
}

/// How many groups the ring menu has.
pub const GROUPS: usize = 4;

/// The most slots a ring holds.
pub const RING_SLOTS: usize = 8;

/// The most characters a group's name or icon keeps (a settings file can hold anything).
pub const NAME_CHARS: usize = 64;

/// The icons a group can show.
pub const GROUP_ICONS: [&str; 8] = ["camera", "edit", "eye", "joint", "section", "home", "folder", "explode"];

/// A ring menu group: its name (a catalogue entry for the built-in ones, or the user's words),
/// its icon, and the actions its ring holds (groups inside a group are left out).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RingGroup {
    pub name: String,
    pub icon: String,
    pub items: Vec<RingAction>,
}

impl RingGroup {
    /// The name to show: a built-in name in the user's language, else as typed.
    pub fn shown_name(&self) -> String {
        if self.name.starts_with("vr.") { solidcraft_ui_text::tr(&self.name).to_string() } else { self.name.clone() }
    }
}

/// What the stick does pushed left or right (outside a grab that uses it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StickAction {
    #[default]
    Nothing,
    UndoRedo,
}

/// Which stick does what while one hand holds the model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldSticks {
    /// The right stick moves the model along the holding hand's ray (up and down) and turns it
    /// about the vertical (left and right); the left stick rolls it (left and right) and tips it
    /// (up and down).
    #[default]
    RightMoves,
    /// The holding hand's stick moves and rolls it, the other hand's turns and tips it.
    HoldingMoves,
}

/// What both grabs do together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TwoHands {
    /// Scale about the point between the hands and move with it; no turn.
    ScaleMove,
    /// Both hands equal: scale, move, and turn with the line between the hands (and their roll
    /// about it).
    #[default]
    Equal,
    /// The hand that grabbed first holds and turns the model as one hand does; the other only
    /// scales it, by its distance to the first.
    Lead,
}

/// When the scale (1:1, 1:4, 2:1) is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleReadout {
    /// While both hands scale, fading after they let go.
    #[default]
    WhileScaling,
    Always,
    Never,
}

/// Where the Browser's window stands beside the belt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserSide {
    #[default]
    Left,
    Right,
    Above,
}

/// Every VR setting. Read one setting at a time ([`VrSettings::from_saved`]): one a newer
/// version wrote that this one cannot read takes its default, and the others stay as saved.
/// Read settings are always bounded (as many ring slots, groups and inputs as there can be,
/// names of at most [`NAME_CHARS`]) and of the current layout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, remote = "Self")]
pub struct VrSettings {
    /// Selects, as soon as it is pressed; when [`VrSettings::menu`] has no inputs, it selects when
    /// let go quickly and opens the menu when held.
    pub select: Binding,
    /// Holds the model.
    pub grab: Binding,
    /// Opens the ring menu at once while held (letting go takes the item under the hand); no
    /// inputs: holding Select does, after [`VrSettings::hold_time`].
    pub menu: Binding,
    /// Puts the model back in front.
    pub put_back: Binding,
    /// Takes the section into the hand, or puts it away.
    pub section: Binding,
    pub undo: Binding,
    pub redo: Binding,
    /// The button layout these settings are for ([`LAYOUT`]); missing in older saved settings.
    #[serde(default)]
    pub layout: u32,
    /// The real room is seen around the model, where the headset can show it (the VR software
    /// offers an alpha-blended view).
    pub passthrough: bool,
    /// The ring menu's first ring, slot by slot.
    pub ring: Vec<RingAction>,
    /// The ring menu's groups ([`GROUPS`]).
    pub ring_groups: Vec<RingGroup>,
    /// The belt: groups of what manages the designs and the room, at the hips.
    pub belt: bool,
    /// How far below the eyes the belt is (metres).
    pub belt_drop: f32,
    /// How far in front of the body the belt is (metres).
    pub belt_distance: f32,
    /// How far from the user the belt's curve is centred (metres): it bends round toward them.
    pub belt_curve: f32,
    /// How tall the Browser's window is (metres).
    pub browser_height: f32,
    /// How wide the Browser's window is in the window's points: how much of the Browser fits.
    pub browser_width: f32,
    /// Where the Browser's window stands beside the belt.
    pub browser_side: BrowserSide,
    /// The joints' glyphs are drawn in the room (as the window draws them over the model).
    pub joints: bool,
    /// How strongly the floor's grid shows (0 to 1): the room's reference, against sickness.
    pub grid: f32,
    /// How far the grid reaches before it has faded to a third (metres).
    pub grid_reach: f32,
    /// How much lighter the sky is toward the horizon (0 to 1).
    pub horizon: f32,
    pub stick: StickAction,
    /// While one hand holds the model, its stick pulls it closer or pushes it away, and the other
    /// hand's stick turns it.
    pub hold_stick: bool,
    /// Which stick does what while one hand holds the model.
    pub hold_sticks: HoldSticks,
    pub two_hands: TwoHands,
    pub snap_real_size: bool,
    pub scale_readout: ScaleReadout,
    /// How long Select is held before the menu opens (seconds).
    pub hold_time: f32,
    /// How far the menu's items stand from the hand (metres).
    pub menu_size: f32,
    /// How tall a menu item's bubble is (metres).
    pub menu_bubble: f32,
    /// How much bigger the item under the hand is.
    pub menu_grow: f32,
    /// How long the hand stays on "More…" before its ring opens (seconds; 0: at once).
    pub section_wait: f32,
    /// A factor on every sign's size.
    pub sign_size: f32,
    /// How visible a slot that cannot be taken, or a ring the hand has gone past, stays (0 to 1).
    pub fade: f32,
    /// The stick's jobs written beside it.
    pub stick_labels: bool,
    /// How far beside the stick its labels stand (metres).
    pub stick_label_gap: f32,
    /// How far above the stick its labels stand (metres).
    pub stick_label_lift: f32,
    /// How far the ray reaches (metres).
    pub ray_length: f32,
    /// How strong the buzzes are (0: none).
    pub haptics: f32,
    /// Where the model first appears: this far in front of the head (metres)...
    pub first_distance: f32,
    /// ...this big (its largest side, metres)...
    pub first_size: f32,
    /// ...and this far below the eyes (metres).
    pub first_drop: f32,
    /// How fast the stick pulls or pushes a held model (metres a second, at full push).
    pub push_speed: f32,
    /// How fast the stick turns a held model (degrees a second, at full push).
    pub turn_speed: f32,
}

/// Saved as the derived form ([`VrSettings`] is `remote = "Self"` so that its reading can be
/// lenient).
impl Serialize for VrSettings {
    /// The settings as their fields, by name.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        VrSettings::serialize(self, serializer)
    }
}

/// Read leniently, setting by setting ([`VrSettings::from_saved`]).
impl<'de> Deserialize<'de> for VrSettings {
    /// The saved settings; what is not a map of settings is refused (the preference then takes
    /// its default as a whole).
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let saved = serde_json::Value::deserialize(deserializer)?;
        VrSettings::from_saved(saved).map_err(serde::de::Error::custom)
    }
}

impl VrSettings {
    /// The settings `saved` holds, one at a time: a setting this version cannot read (a newer
    /// version's input, action or choice, a wrong type) is left out and takes its default, and
    /// a missing one does too. At most [`SAVED_FIELDS`] keys are looked at. Not a map: refused.
    /// What is read is bounded (lists and names cut to what the settings can hold, as
    /// [`VrSettings::sane`] does) and, saved for an older layout, migrated
    /// ([`VrSettings::migrate`]), so the page and the headset never see more.
    pub fn from_saved(saved: serde_json::Value) -> Result<VrSettings, serde_json::Error> {
        VrSettings::read_saved(saved).map(|mut s| {
            s.bound();
            s.migrate();
            s
        })
    }

    /// [`VrSettings::from_saved`] before bounding and migrating.
    fn read_saved(saved: serde_json::Value) -> Result<VrSettings, serde_json::Error> {
        let serde_json::Value::Object(saved) = saved else {
            return VrSettings::deserialize(saved);
        };
        let mut kept = serde_json::Map::new();
        for (key, value) in saved.into_iter().take(SAVED_FIELDS) {
            let mut trial = kept.clone();
            trial.insert(key.clone(), value.clone());
            if VrSettings::deserialize(serde_json::Value::Object(trial)).is_ok() {
                kept.insert(key, value);
            } else {
                log::warn!("VR settings: `{key}` is not understood by this version; using its default");
            }
        }
        VrSettings::deserialize(serde_json::Value::Object(kept))
    }
}

/// The most keys [`VrSettings::from_saved`] looks at (the settings have a few dozen; a hostile
/// file may have millions).
const SAVED_FIELDS: usize = 200;

impl Default for VrSettings {
    /// What VR does out of the box (docs/vr.md).
    fn default() -> Self {
        VrSettings {
            // For a controller with a trigger, a grip, a stick and two buttons (most have that):
            // the trigger selects, the grip grabs, the right hand's upper button opens the ring
            // menu (the menu button too, where a controller has one for apps), the left hand's two
            // buttons undo and redo. The stick is left to what the hands are doing.
            select: Binding::of(&[Input::Trigger], &[Input::Trigger]),
            grab: Binding::of(&[Input::Grip], &[Input::Grip]),
            menu: Binding::of(&[Input::Menu], &[Input::Secondary, Input::Menu]),
            put_back: Binding::default(),
            section: Binding::default(),
            undo: Binding::of(&[Input::Primary], &[]),
            redo: Binding::of(&[Input::Secondary], &[]),
            layout: LAYOUT,
            ring: vec![
                RingAction::ToggleHide,
                RingAction::Isolate,
                RingAction::ShowAll,
                RingAction::Section,
                RingAction::Group(0),
                RingAction::Group(1),
            ],
            ring_groups: vec![
                RingGroup {
                    name: "vr.group_view".into(),
                    icon: "camera".into(),
                    items: vec![RingAction::PutBack, RingAction::Explode, RingAction::Passthrough],
                },
                RingGroup { name: "vr.group_edit".into(), icon: "edit".into(), items: vec![RingAction::Undo, RingAction::Redo] },
                RingGroup::default(),
                RingGroup::default(),
            ],
            passthrough: false,
            belt: true,
            belt_drop: 0.4,
            belt_distance: 0.35,
            belt_curve: 0.6,
            browser_height: 0.42,
            browser_width: 300.0,
            browser_side: BrowserSide::Left,
            joints: true,
            grid: 0.8,
            grid_reach: 16.0,
            horizon: 0.35,
            stick: StickAction::Nothing,
            hold_stick: true,
            two_hands: TwoHands::Equal,
            hold_sticks: HoldSticks::RightMoves,
            snap_real_size: true,
            scale_readout: ScaleReadout::WhileScaling,
            hold_time: 0.35,
            menu_size: 0.09,
            menu_bubble: 0.045,
            menu_grow: 1.3,
            section_wait: 0.0,
            sign_size: 1.0,
            fade: 0.2,
            stick_labels: true,
            stick_label_gap: 0.03,
            stick_label_lift: 0.01,
            ray_length: 3.0,
            haptics: 1.0,
            first_distance: 0.6,
            first_size: 0.4,
            first_drop: 0.3,
            push_speed: 1.0,
            turn_speed: 90.0,
        }
    }
}

/// The ranges the numbers are kept in (a settings file can hold anything).
const HOLD_TIME: std::ops::RangeInclusive<f32> = 0.1..=2.0;
const MENU_SIZE: std::ops::RangeInclusive<f32> = 0.04..=0.3;
const MENU_BUBBLE: std::ops::RangeInclusive<f32> = 0.015..=0.15;
const LABEL_GAP: std::ops::RangeInclusive<f32> = 0.0..=0.15;
const LABEL_LIFT: std::ops::RangeInclusive<f32> = -0.05..=0.1;
const MENU_GROW: std::ops::RangeInclusive<f32> = 1.0..=2.0;
const SECTION_WAIT: std::ops::RangeInclusive<f32> = 0.0..=2.0;
const SIGN_SIZE: std::ops::RangeInclusive<f32> = 0.3..=3.0;
const FADE: std::ops::RangeInclusive<f32> = 0.05..=1.0;
const BELT_CURVE: std::ops::RangeInclusive<f32> = 0.2..=5.0;
const SHARE: std::ops::RangeInclusive<f32> = 0.0..=1.0;
const GRID_REACH: std::ops::RangeInclusive<f32> = 1.0..=100.0;
const BROWSER_HEIGHT: std::ops::RangeInclusive<f32> = 0.1..=1.0;
const BROWSER_WIDTH: std::ops::RangeInclusive<f32> = 200.0..=600.0;
const RAY_LENGTH: std::ops::RangeInclusive<f32> = 0.3..=20.0;
const HAPTICS: std::ops::RangeInclusive<f32> = 0.0..=2.0;
const FIRST_DISTANCE: std::ops::RangeInclusive<f32> = 0.2..=5.0;
const FIRST_SIZE: std::ops::RangeInclusive<f32> = 0.05..=5.0;
const FIRST_DROP: std::ops::RangeInclusive<f32> = -1.0..=1.5;
const PUSH_SPEED: std::ops::RangeInclusive<f32> = 0.05..=5.0;
const BELT_DROP: std::ops::RangeInclusive<f32> = 0.2..=1.2;
const BELT_DISTANCE: std::ops::RangeInclusive<f32> = 0.1..=1.0;
const TURN_SPEED: std::ops::RangeInclusive<f32> = 5.0..=360.0;

/// `v` within `range`; a value that is not a number is the default `d`.
fn within(v: f32, range: std::ops::RangeInclusive<f32>, d: f32) -> f32 {
    if v.is_finite() { v.clamp(*range.start(), *range.end()) } else { d }
}

impl VrSettings {
    /// Settings saved for an older button layout take this one's buttons and stick, once; the
    /// rest is kept.
    pub fn migrate(&mut self) {
        if self.layout >= LAYOUT {
            return;
        }
        let d = VrSettings::default();
        // Layout 4: both hands scale, move and turn the model by default.
        if self.layout < 4 {
            self.two_hands = d.two_hands;
        }
        // Layout 3: the belt higher and further out (the first one sat too low).
        if self.layout < 3 {
            self.belt_drop = d.belt_drop;
            self.belt_distance = d.belt_distance;
        }
        if self.layout < 2 {
            self.select = d.select;
            self.grab = d.grab;
            self.menu = d.menu;
            self.put_back = d.put_back;
            self.section = d.section;
            self.undo = d.undo;
            self.redo = d.redo;
            self.stick = d.stick;
            self.hold_stick = d.hold_stick;
        }
        self.layout = self.layout.max(LAYOUT);
    }

    /// Every list cut to what the settings can hold, in place: each input at most once on each
    /// hand of an action (at most the six there are), at most [`RING_SLOTS`] ring slots without
    /// empty ones or groups that do not exist, exactly [`GROUPS`] groups of at most
    /// [`RING_SLOTS`] actions (no empty slots, no groups inside a group), and names and icons of
    /// at most [`NAME_CHARS`] characters.
    fn bound(&mut self) {
        let once = |list: &mut Vec<Input>| {
            let mut out = Vec::new();
            for i in list.iter() {
                if !out.contains(i) {
                    out.push(*i);
                }
            }
            *list = out;
        };
        for b in [&mut self.select, &mut self.grab, &mut self.menu, &mut self.put_back, &mut self.section, &mut self.undo, &mut self.redo] {
            once(&mut b.left);
            once(&mut b.right);
        }
        let ring = |list: &mut Vec<RingAction>, keep: &dyn Fn(RingAction) -> bool| {
            let kept: Vec<RingAction> = list.iter().copied().filter(|a| keep(*a)).take(RING_SLOTS).collect();
            *list = kept;
        };
        ring(&mut self.ring, &|a| a != RingAction::Empty && !matches!(a, RingAction::Group(g) if usize::from(g) >= GROUPS));
        self.ring_groups.truncate(GROUPS);
        self.ring_groups.resize_with(GROUPS, RingGroup::default);
        let cut = |text: &mut String| {
            if let Some((at, _)) = text.char_indices().nth(NAME_CHARS) {
                text.truncate(at);
            }
        };
        for group in &mut self.ring_groups {
            ring(&mut group.items, &|a| !matches!(a, RingAction::Empty | RingAction::Group(_)));
            cut(&mut group.name);
            cut(&mut group.icon);
        }
    }

    /// The settings with every number in its range, and every list cut to what the settings can
    /// hold, as reading them does. Asked every headset frame: what is read is bounded already
    /// ([`VrSettings::from_saved`]), so the copy made here is small.
    pub fn sane(&self) -> VrSettings {
        let d = VrSettings::default();
        let mut out = self.clone();
        out.bound();
        VrSettings {
            belt_drop: within(self.belt_drop, BELT_DROP, d.belt_drop),
            belt_distance: within(self.belt_distance, BELT_DISTANCE, d.belt_distance),
            hold_time: within(self.hold_time, HOLD_TIME, d.hold_time),
            menu_size: within(self.menu_size, MENU_SIZE, d.menu_size),
            menu_bubble: within(self.menu_bubble, MENU_BUBBLE, d.menu_bubble),
            menu_grow: within(self.menu_grow, MENU_GROW, d.menu_grow),
            section_wait: within(self.section_wait, SECTION_WAIT, d.section_wait),
            sign_size: within(self.sign_size, SIGN_SIZE, d.sign_size),
            fade: within(self.fade, FADE, d.fade),
            belt_curve: within(self.belt_curve, BELT_CURVE, d.belt_curve),
            grid: within(self.grid, SHARE, d.grid),
            grid_reach: within(self.grid_reach, GRID_REACH, d.grid_reach),
            horizon: within(self.horizon, SHARE, d.horizon),
            browser_height: within(self.browser_height, BROWSER_HEIGHT, d.browser_height),
            browser_width: within(self.browser_width, BROWSER_WIDTH, d.browser_width).round(),
            stick_label_gap: within(self.stick_label_gap, LABEL_GAP, d.stick_label_gap),
            stick_label_lift: within(self.stick_label_lift, LABEL_LIFT, d.stick_label_lift),
            ray_length: within(self.ray_length, RAY_LENGTH, d.ray_length),
            haptics: within(self.haptics, HAPTICS, d.haptics),
            first_distance: within(self.first_distance, FIRST_DISTANCE, d.first_distance),
            first_size: within(self.first_size, FIRST_SIZE, d.first_size),
            first_drop: within(self.first_drop, FIRST_DROP, d.first_drop),
            push_speed: within(self.push_speed, PUSH_SPEED, d.push_speed),
            turn_speed: within(self.turn_speed, TURN_SPEED, d.turn_speed),
            ..out
        }
    }
}

/// The VR settings on Preferences › XR, in sections: the buttons (one table, the hands side by
/// side), the ring menu, the stick and holding, the belt and the Browser, the look, and the
/// timing and placement. Every one applies at once.
pub(crate) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let tr = |k: &str| solidcraft_ui_text::tr(k).to_string();
    // Older layouts were migrated when the settings were read (`VrSettings::from_saved`).
    ui_kit::text(ui, &tr("prefs.vr.note"), ui_kit::Text::Hint("vr_settings_note"));
    ui_kit::settings_section(ui, "vr_buttons", &tr("prefs.vr.sec_buttons"), |ui| buttons(app, ui));
    ui_kit::settings_section(ui, "vr_ring", &tr("prefs.vr.sec_ring"), |ui| ring_settings(app, ui));
    let passthrough_missing = app_passthrough_missing(&app.services.vr_passthrough);
    let s = &mut app.preferences.vr;
    // Numbers in fields: they are seldom changed, and only while VR is worked out.
    let number = |ui: &mut egui::Ui, words: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, step: f64| {
        ui_kit::row(ui, &tr(words), |ui| ui_kit::settings_number(ui, value, range, step));
    };
    ui_kit::settings_section(ui, "vr_hold", &tr("prefs.vr.sec_hold"), |ui| {
        segments(
            ui,
            &tr("prefs.vr.stick"),
            &[tr("prefs.vr.stick_nothing"), tr("prefs.vr.stick_undo_redo")],
            &mut s.stick,
            &[StickAction::Nothing, StickAction::UndoRedo],
        );
        ui_kit::checkbox(ui, &tr("prefs.vr.hold_stick"), &mut s.hold_stick);
        choose(
            ui,
            &tr("prefs.vr.hold_sticks"),
            "vr_hold_sticks",
            &[tr("prefs.vr.hold_sticks_right"), tr("prefs.vr.hold_sticks_holding")],
            &mut s.hold_sticks,
            &[HoldSticks::RightMoves, HoldSticks::HoldingMoves],
        );
        ui_kit::checkbox(ui, &tr("prefs.vr.stick_labels"), &mut s.stick_labels);
        choose(
            ui,
            &tr("prefs.vr.two_hands"),
            "vr_two_hands",
            &[tr("prefs.vr.two_hands_scale"), tr("prefs.vr.two_hands_equal"), tr("prefs.vr.two_hands_lead")],
            &mut s.two_hands,
            &[TwoHands::ScaleMove, TwoHands::Equal, TwoHands::Lead],
        );
        ui_kit::checkbox(ui, &tr("prefs.vr.snap_real_size"), &mut s.snap_real_size);
        number(ui, "prefs.vr.push_speed", &mut s.push_speed, PUSH_SPEED, 0.05);
        number(ui, "prefs.vr.turn_speed", &mut s.turn_speed, TURN_SPEED, 5.0);
    });
    ui_kit::settings_section(ui, "vr_belt", &tr("prefs.vr.sec_belt"), |ui| {
        ui_kit::checkbox(ui, &tr("prefs.vr.belt"), &mut s.belt);
        number(ui, "prefs.vr.belt_drop", &mut s.belt_drop, BELT_DROP, 0.05);
        number(ui, "prefs.vr.belt_distance", &mut s.belt_distance, BELT_DISTANCE, 0.05);
        number(ui, "prefs.vr.belt_curve", &mut s.belt_curve, BELT_CURVE, 0.05);
        segments(
            ui,
            &tr("prefs.vr.browser_side"),
            &[tr("prefs.vr.side_left"), tr("prefs.vr.side_right"), tr("prefs.vr.side_above")],
            &mut s.browser_side,
            &[BrowserSide::Left, BrowserSide::Right, BrowserSide::Above],
        );
        number(ui, "prefs.vr.browser_height", &mut s.browser_height, BROWSER_HEIGHT, 0.01);
        number(ui, "prefs.vr.browser_width", &mut s.browser_width, BROWSER_WIDTH, 10.0);
    });
    ui_kit::settings_section(ui, "vr_look", &tr("prefs.vr.sec_look"), |ui| {
        ui_kit::checkbox(ui, &tr("prefs.vr.passthrough"), &mut s.passthrough);
        if passthrough_missing {
            ui_kit::text(ui, &tr("prefs.vr.passthrough_missing"), ui_kit::Text::Hint("vr_passthrough_missing"));
        }
        ui_kit::checkbox(ui, &tr("prefs.vr.joints"), &mut s.joints);
        number(ui, "prefs.vr.grid", &mut s.grid, SHARE, 0.05);
        number(ui, "prefs.vr.grid_reach", &mut s.grid_reach, GRID_REACH, 1.0);
        number(ui, "prefs.vr.horizon", &mut s.horizon, SHARE, 0.05);
        segments(
            ui,
            &tr("prefs.vr.scale_readout"),
            &[tr("prefs.vr.readout_while"), tr("prefs.vr.readout_always"), tr("prefs.vr.readout_never")],
            &mut s.scale_readout,
            &[ScaleReadout::WhileScaling, ScaleReadout::Always, ScaleReadout::Never],
        );
        number(ui, "prefs.vr.sign_size", &mut s.sign_size, SIGN_SIZE, 0.1);
        number(ui, "prefs.vr.fade", &mut s.fade, FADE, 0.05);
        number(ui, "prefs.vr.menu_size", &mut s.menu_size, MENU_SIZE, 0.01);
        number(ui, "prefs.vr.menu_bubble", &mut s.menu_bubble, MENU_BUBBLE, 0.005);
        number(ui, "prefs.vr.menu_grow", &mut s.menu_grow, MENU_GROW, 0.05);
        number(ui, "prefs.vr.ray_length", &mut s.ray_length, RAY_LENGTH, 0.1);
        number(ui, "prefs.vr.stick_label_gap", &mut s.stick_label_gap, LABEL_GAP, 0.005);
        number(ui, "prefs.vr.stick_label_lift", &mut s.stick_label_lift, LABEL_LIFT, 0.005);
        number(ui, "prefs.vr.haptics", &mut s.haptics, HAPTICS, 0.1);
    });
    ui_kit::settings_section(ui, "vr_place", &tr("prefs.vr.sec_place"), |ui| {
        number(ui, "prefs.vr.hold_time", &mut s.hold_time, HOLD_TIME, 0.05);
        number(ui, "prefs.vr.section_wait", &mut s.section_wait, SECTION_WAIT, 0.05);
        number(ui, "prefs.vr.first_distance", &mut s.first_distance, FIRST_DISTANCE, 0.05);
        number(ui, "prefs.vr.first_size", &mut s.first_size, FIRST_SIZE, 0.05);
        number(ui, "prefs.vr.first_drop", &mut s.first_drop, FIRST_DROP, 0.05);
    });
    if ui_kit::button(ui, tr("prefs.vr.defaults"), ui_kit::Button::Normal).clicked() {
        *s = VrSettings::default();
    }
}

/// Which inputs do what: an action a row, its inputs as chips ("Trigger L"); "+" adds one, a
/// click on a chip offers to take it out.
fn buttons(app: &mut SolveApp, ui: &mut egui::Ui) {
    let tr = |k: &str| solidcraft_ui_text::tr(k).to_string();
    ui_kit::text(ui, &tr("prefs.vr.inputs"), ui_kit::Text::Hint("vr_inputs"));
    let actions: [(&str, &str, fn(&mut VrSettings) -> &mut Binding); 7] = [
        ("select", "prefs.vr.select", |s| &mut s.select),
        ("grab", "prefs.vr.grab", |s| &mut s.grab),
        ("menu", "prefs.vr.menu", |s| &mut s.menu),
        ("undo", "prefs.vr.undo", |s| &mut s.undo),
        ("redo", "prefs.vr.redo", |s| &mut s.redo),
        ("put_back", "prefs.vr.put_back", |s| &mut s.put_back),
        ("section", "prefs.vr.section_action", |s| &mut s.section),
    ];
    let hands = [tr("prefs.vr.col_left"), tr("prefs.vr.col_right")];
    let chip = |input: Input, side: usize| format!("{} {}", tr(input.words()), hands[side]);
    for (id, words, binding) in actions {
        let b = binding(&mut app.preferences.vr);
        ui_kit::row(ui, &tr(words), |ui| {
            ui.horizontal_wrapped(|ui| {
                let mut remove = None;
                for side in 0..2 {
                    let list = if side == 0 { &b.left } else { &b.right };
                    for input in list {
                        ui_kit::compact_dropdown_menu(ui, format!("vr_input_{id}_{side}_{input:?}"), &chip(*input, side), |ui| {
                            if ui_kit::list_picker(ui, &format!("vr_input_{id}_{side}_{input:?}_remove"), &tr("prefs.vr.remove_input"), false)
                                .clicked()
                            {
                                remove = Some((side, *input));
                            }
                        });
                    }
                }
                let mut add = None;
                ui_kit::compact_dropdown_menu(ui, format!("vr_input_{id}_add"), "+", |ui| {
                    for side in 0..2 {
                        for input in b.unused(side) {
                            if ui_kit::list_picker(ui, &format!("vr_input_{id}_add_{side}_{input:?}"), &chip(input, side), false).clicked() {
                                add = Some((side, input));
                            }
                        }
                    }
                });
                if let Some((side, input)) = remove {
                    b.remove(side, input);
                }
                if let Some((side, input)) = add {
                    b.add(side, input);
                }
            });
        });
    }
    if app.preferences.vr.menu.is_empty() {
        ui_kit::text(ui, &tr("prefs.vr.menu_by_select"), ui_kit::Text::Hint("vr_menu_by_select"));
    }
}

/// The ring menu's layout: the first ring's slots, then each group's name, pictogram and slots,
/// each slot a small chooser in a row (an empty choice takes it out), "+" adding one.
fn ring_settings(app: &mut SolveApp, ui: &mut egui::Ui) {
    let tr = |k: &str| solidcraft_ui_text::tr(k).to_string();
    ui_kit::text(ui, &tr("prefs.vr.ring"), ui_kit::Text::Hint("vr_ring"));
    let s = &mut app.preferences.vr;
    while s.ring_groups.len() < GROUPS {
        s.ring_groups.push(RingGroup::default());
    }
    let group_names: Vec<String> = s.ring_groups.iter().map(|g| if g.name.is_empty() { String::new() } else { g.shown_name() }).collect();
    let name_of = |a: RingAction| match a {
        RingAction::Group(g) => {
            let name = group_names.get(usize::from(g)).cloned().unwrap_or_default();
            format!("{} ▸", if name.is_empty() { format!("{} {}", tr("prefs.vr.ring_group"), g + 1) } else { name })
        }
        RingAction::Empty => tr("prefs.vr.remove_slot"),
        other => tr(other.words()),
    };
    // A ring's slots as choosers in a row, and "+" while there is room for another.
    let slots = |ui: &mut egui::Ui, id: &str, list: &mut Vec<RingAction>, choices: &[RingAction]| {
        ui.horizontal_wrapped(|ui| {
            let mut out: Vec<RingAction> = Vec::new();
            for (k, slot) in list.iter().enumerate() {
                let mut chosen = *slot;
                ui_kit::compact_dropdown_menu(ui, format!("{id}_{k}"), &name_of(*slot), |ui| {
                    for choice in choices.iter().copied().chain([RingAction::Empty]) {
                        if ui_kit::list_picker(ui, &format!("{id}_{k}_{choice:?}"), &name_of(choice), choice == *slot).clicked() {
                            chosen = choice;
                        }
                    }
                });
                if chosen != RingAction::Empty {
                    out.push(chosen);
                }
            }
            if out.len() < RING_SLOTS && ui_kit::icon_action(ui, &tr("prefs.vr.add_slot"), "plus", true, false).clicked() {
                let next = choices.iter().copied().find(|c| !out.contains(c)).unwrap_or(RingAction::ShowAll);
                out.push(next);
            }
            *list = out;
        });
    };
    let all = RingAction::all();
    let actions: Vec<RingAction> = all.iter().copied().filter(|a| !matches!(a, RingAction::Group(_))).collect();
    ui_kit::row(ui, &tr("prefs.vr.ring_first"), |ui| slots(ui, "vr_ring", &mut s.ring, &all));
    for g in 0..GROUPS {
        let label = format!("{} {}", tr("prefs.vr.ring_group"), g + 1);
        let group = &mut s.ring_groups[g];
        ui_kit::row(ui, &label, |ui| {
            ui.horizontal(|ui| {
                let mut name = group.shown_name();
                let width = solidcraft_theme::Tokens::get().form_label_width * 2.2;
                ui_kit::settings_control_width(ui, width, |ui| {
                    if ui_kit::text_field(ui, &mut name, &tr("prefs.vr.group_name")).0 {
                        group.name = name.chars().take(NAME_CHARS).collect();
                    }
                });
                let icons: Vec<(&str, &str)> = GROUP_ICONS.iter().map(|i| (*i, *i)).collect();
                let mut at = GROUP_ICONS.iter().position(|i| *i == group.icon).unwrap_or(0);
                ui_kit::settings_control_width(ui, width, |ui| ui_kit::dropdown_icons(ui, &format!("vr_ring_icon_{g}"), &mut at, &icons));
                group.icon = GROUP_ICONS[at.min(GROUP_ICONS.len() - 1)].to_string();
            });
        });
        ui_kit::row(ui, "", |ui| slots(ui, &format!("vr_ring_group_{g}"), &mut group.items, &actions));
    }
}

/// A row of segments choosing `value` among `values`, named by `names`.
fn segments<T: Copy + PartialEq>(ui: &mut egui::Ui, label: &str, names: &[String], value: &mut T, values: &[T]) {
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    ui_kit::row(ui, label, |ui| {
        let mut at = values.iter().position(|v| v == value).unwrap_or(0);
        if ui_kit::settings_segments(ui, label, &mut at, &names) {
            *value = values.get(at).copied().unwrap_or(*value);
        }
    });
}

/// Whether the running VR session said it cannot show the real room (`Some(false)`).
fn app_passthrough_missing(offered: &Option<bool>) -> bool {
    *offered == Some(false)
}

/// A dropdown row choosing `value` among `values`, named by `names`.
fn choose<T: Copy + PartialEq>(ui: &mut egui::Ui, label: &str, id: &str, names: &[String], value: &mut T, values: &[T]) {
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    ui_kit::row(ui, label, |ui| {
        let mut at = values.iter().position(|v| v == value).unwrap_or(0);
        // The dropdown's own response does not say a choice was made in its list: the choice is
        // read back instead.
        ui_kit::dropdown(ui, id, &mut at, &names);
        if let Some(v) = values.get(at) {
            *value = *v;
        }
    });
}

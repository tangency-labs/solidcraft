//! The controllers: what each hand holds down and where it is, and which button of which
//! controller means what.
//!
//! The VR viewer (docs/vr.md) is driven by the controllers alone. OpenXR does not hand
//! out buttons; an app names its own actions (pick, grab, the stick) and suggests, for each kind
//! of controller it knows ([`PROFILES`]), which input each action reads. The runtime then binds
//! them, and the user can rebind them in the runtime's own settings (SteamVR's controller
//! bindings, for example). A controller the app has no suggestion for is bound by the runtime
//! from one of those it has.
//!
//! This module is plain data, built and tested everywhere; the session reads the actions into
//! [`HandInput`] (`Session::hands`).

use crate::view::Pose;

/// One of the two hands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hand {
    Left,
    Right,
}

impl Hand {
    /// Both hands, left first: the order of every per-hand array.
    pub const BOTH: [Hand; 2] = [Hand::Left, Hand::Right];

    /// Its place in a per-hand array: 0 left, 1 right.
    pub fn index(self) -> usize {
        match self {
            Hand::Left => 0,
            Hand::Right => 1,
        }
    }

    /// The other hand.
    pub fn other(self) -> Hand {
        match self {
            Hand::Left => Hand::Right,
            Hand::Right => Hand::Left,
        }
    }

    /// The hand's OpenXR user path.
    pub fn path(self) -> &'static str {
        match self {
            Hand::Left => "/user/hand/left",
            Hand::Right => "/user/hand/right",
        }
    }
}

/// What the viewer reads from a controller: each physical input on its own, so that the app can
/// give any of them any job (Preferences › XR › VR) without new bindings, and the poses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Control {
    /// Where the hand holds the controller (a pose): what a grab moves with.
    Grip,
    /// Where the controller points (a pose, pointing down its −Z): where the ray starts.
    Aim,
    /// The trigger, 0 to 1 (a pinch of a tracked hand; the select button of a one-button
    /// controller).
    Trigger,
    /// The grip button, 0 to 1 (a grip that only clicks reads 0 or 1; a closed tracked hand).
    Squeeze,
    /// The thumbstick, or the touchpad where there is none: x right, y up, each −1 to 1.
    Stick,
    /// The controller's vibration.
    Haptic,
    /// Pressing the stick (the touchpad).
    StickClick,
    /// The lower face button: A (X on a left Touch controller).
    Primary,
    /// The upper face button: B (Y on a left Touch controller).
    Secondary,
    /// The menu button, where a controller has one for apps.
    Menu,
}

impl Control {
    /// Every control, in the order the session makes its actions.
    pub const ALL: [Control; 10] = [
        Control::Grip,
        Control::Aim,
        Control::Trigger,
        Control::Squeeze,
        Control::Stick,
        Control::Haptic,
        Control::StickClick,
        Control::Primary,
        Control::Secondary,
        Control::Menu,
    ];

    /// The action's name for the runtime: lower case, unique within the viewer's action set.
    pub fn name(self) -> &'static str {
        match self {
            Control::Grip => "grip_pose",
            Control::Aim => "aim_pose",
            Control::Trigger => "trigger",
            Control::Squeeze => "squeeze",
            Control::Stick => "stick",
            Control::Haptic => "haptic",
            Control::StickClick => "stick_click",
            Control::Primary => "primary",
            Control::Secondary => "secondary",
            Control::Menu => "menu",
        }
    }

    /// What the runtime shows for the action in its binding settings: the input it stands for
    /// (what each one does is set in the app). English: OpenXR takes one name, and the runtime's
    /// binding screen is not translated by the app.
    pub fn label(self) -> &'static str {
        match self {
            Control::Grip => "Hand position",
            Control::Aim => "Pointing direction",
            Control::Trigger => "Trigger",
            Control::Squeeze => "Grip",
            Control::Stick => "Stick",
            Control::Haptic => "Vibration",
            Control::StickClick => "Stick click",
            Control::Primary => "A or X button",
            Control::Secondary => "B or Y button",
            Control::Menu => "Menu button",
        }
    }

    /// Whether the control is something the runtime sends (vibration), not something it reads.
    pub fn is_output(self) -> bool {
        matches!(self, Control::Haptic)
    }
}

/// The suggested bindings for one kind of controller: for each control, the input path under
/// each hand's user path (`/user/hand/left` + `/input/trigger/value`).
#[derive(Clone, Copy, Debug)]
pub struct Profile {
    /// The OpenXR interaction profile.
    pub path: &'static str,
    pub bindings: &'static [(Control, &'static str)],
    /// Inputs only one hand has, each a full path: the face buttons of a Touch controller,
    /// named differently on the two hands.
    pub extra: &'static [(Control, &'static str)],
}

impl Profile {
    /// Every binding as the runtime takes it, a full path for each: `bindings` under each hand's
    /// user path (left first), then `extra` as written.
    pub fn paths(&self) -> impl Iterator<Item = (Control, String)> + use<> {
        let (bindings, extra) = (self.bindings, self.extra);
        let both =
            Hand::BOTH.into_iter().flat_map(move |hand| bindings.iter().map(move |(control, input)| (*control, format!("{}{input}", hand.path()))));
        both.chain(extra.iter().map(|(control, path)| (*control, (*path).to_string())))
    }

    /// Whether the session suggests this profile: tracked hands' only where the runtime has
    /// `XR_EXT_hand_interaction` (`hands_tracked`).
    pub fn suggested(&self, hands_tracked: bool) -> bool {
        hands_tracked || self.path != HAND_PROFILE
    }
}

/// Tracked hands' profile (`XR_EXT_hand_interaction`): suggested only where the runtime has the
/// extension.
pub const HAND_PROFILE: &str = "/interaction_profiles/ext/hand_interaction_ext";

/// The poses and the vibration: every OpenXR controller has them, at the same paths.
const POSES: [(Control, &str); 3] = [(Control::Grip, "/input/grip/pose"), (Control::Aim, "/input/aim/pose"), (Control::Haptic, "/output/haptic")];

/// The controllers the viewer suggests bindings for. The runtime binds any other from one of
/// these. Only inputs both hands have are named in `bindings`, because a suggestion with one
/// unknown path is refused whole; one-hand inputs go in `extra`.
pub const PROFILES: &[Profile] = &[
    // Valve Index; SteamVR also binds the Steam Frame's controllers from it.
    Profile {
        path: "/interaction_profiles/valve/index_controller",
        bindings: &[
            POSES[0],
            POSES[1],
            POSES[2],
            (Control::Trigger, "/input/trigger/value"),
            (Control::Squeeze, "/input/squeeze/value"),
            (Control::Stick, "/input/thumbstick"),
            (Control::StickClick, "/input/thumbstick/click"),
            (Control::Primary, "/input/a/click"),
            (Control::Secondary, "/input/b/click"),
        ],
        extra: &[],
    },
    // Meta Quest (Touch): X and Y on the left hand, A and B on the right, the menu button left.
    Profile {
        path: "/interaction_profiles/oculus/touch_controller",
        bindings: &[
            POSES[0],
            POSES[1],
            POSES[2],
            (Control::Trigger, "/input/trigger/value"),
            (Control::Squeeze, "/input/squeeze/value"),
            (Control::Stick, "/input/thumbstick"),
            (Control::StickClick, "/input/thumbstick/click"),
        ],
        extra: &[
            (Control::Primary, "/user/hand/left/input/x/click"),
            (Control::Secondary, "/user/hand/left/input/y/click"),
            (Control::Primary, "/user/hand/right/input/a/click"),
            (Control::Secondary, "/user/hand/right/input/b/click"),
            (Control::Menu, "/user/hand/left/input/menu/click"),
        ],
    },
    // Windows Mixed Reality.
    Profile {
        path: "/interaction_profiles/microsoft/motion_controller",
        bindings: &[
            POSES[0],
            POSES[1],
            POSES[2],
            (Control::Trigger, "/input/trigger/value"),
            (Control::Squeeze, "/input/squeeze/click"),
            (Control::Stick, "/input/thumbstick"),
            (Control::StickClick, "/input/thumbstick/click"),
            (Control::Menu, "/input/menu/click"),
        ],
        extra: &[],
    },
    // HTC Vive wands: the touchpad stands in for the stick.
    Profile {
        path: "/interaction_profiles/htc/vive_controller",
        bindings: &[
            POSES[0],
            POSES[1],
            POSES[2],
            (Control::Trigger, "/input/trigger/value"),
            (Control::Squeeze, "/input/squeeze/click"),
            (Control::Stick, "/input/trackpad"),
            (Control::StickClick, "/input/trackpad/click"),
            (Control::Menu, "/input/menu/click"),
        ],
        extra: &[],
    },
    // Any controller: the select button and the menu button.
    Profile {
        path: "/interaction_profiles/khr/simple_controller",
        bindings: &[POSES[0], POSES[1], POSES[2], (Control::Trigger, "/input/select/click"), (Control::Menu, "/input/menu/click")],
        extra: &[],
    },
    // Tracked hands (`XR_EXT_hand_interaction`, where the runtime has it): a pinch is the
    // trigger, a closed hand the grip. Hands do not vibrate.
    Profile {
        path: HAND_PROFILE,
        bindings: &[POSES[0], POSES[1], (Control::Trigger, "/input/pinch_ext/value"), (Control::Squeeze, "/input/grasp_ext/value")],
        extra: &[],
    },
];

/// A trigger or grip counts as held from this far down (0 to 1)...
pub const PRESS: f32 = 0.6;

/// ...and as let go once back above this, so a value near one threshold does not flicker.
pub const RELEASE: f32 = 0.4;

/// One hand's controller as a frame reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandInput {
    /// Where the hand holds the controller; `None` while it is not tracked.
    pub grip: Option<Pose>,
    /// Where the controller points (down its −Z); `None` while it is not tracked.
    pub aim: Option<Pose>,
    /// The trigger, 0 to 1.
    pub trigger: f32,
    /// The grip button, 0 to 1.
    pub squeeze: f32,
    /// The stick, x right and y up, each −1 to 1.
    pub stick: [f32; 2],
    /// The controller has a stick (or a touchpad standing in for one) bound to the stick action.
    pub has_stick: bool,
    /// The stick (touchpad) pressed in.
    pub stick_click: bool,
    /// The lower face button (A, X) pressed.
    pub primary: bool,
    /// The upper face button (B, Y) pressed.
    pub secondary: bool,
    /// The menu button pressed.
    pub menu: bool,
}

impl HandInput {
    /// A hand with no controller: nothing tracked, nothing held.
    pub const NONE: HandInput = HandInput {
        grip: None,
        aim: None,
        trigger: 0.0,
        squeeze: 0.0,
        stick: [0.0; 2],
        has_stick: false,
        stick_click: false,
        primary: false,
        secondary: false,
        menu: false,
    };

    /// A hand from what the runtime reported, with every number kept to its range: a value that
    /// is not a finite number counts as not pressed, and a pose that is not one as not tracked.
    pub fn new(grip: Option<Pose>, aim: Option<Pose>, trigger: f32, squeeze: f32, stick: [f32; 2]) -> HandInput {
        let amount = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        let axis = |v: f32| if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 };
        let pose = |p: Option<Pose>| p.filter(|p| p.axes().is_some());
        HandInput { grip: pose(grip), aim: pose(aim), trigger: amount(trigger), squeeze: amount(squeeze), stick: stick.map(axis), ..HandInput::NONE }
    }
}

/// A button read as held or let go, with the gap between [`PRESS`] and [`RELEASE`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Button {
    /// Held now.
    pub down: bool,
    /// Pressed this frame.
    pub pressed: bool,
    /// Let go this frame.
    pub released: bool,
}

impl Button {
    /// The button this frame, from how far down it is now and whether it was held last frame.
    pub fn next(self, value: f32) -> Button {
        let down = if self.down { value > RELEASE } else { value >= PRESS };
        Button { down, pressed: down && !self.down, released: !down && self.down }
    }
}

/// A short vibration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pulse {
    /// How strong, 0 to 1.
    pub amplitude: f32,
    /// How long (seconds).
    pub seconds: f32,
}

impl Pulse {
    /// The longest pulse sent: a vibration the app forgets to stop must not run on.
    pub const LONGEST: f32 = 0.5;

    /// A tick: a step, a snap.
    pub const TICK: Pulse = Pulse { amplitude: 0.3, seconds: 0.01 };

    /// Something taken hold of.
    pub const GRAB: Pulse = Pulse { amplitude: 0.5, seconds: 0.03 };

    /// The pulse as sent: strength 0 to 1, length 0 to [`Pulse::LONGEST`], and `None` for one
    /// that would not be felt (or has numbers that are not).
    pub fn bounded(self) -> Option<Pulse> {
        let amplitude = if self.amplitude.is_finite() { self.amplitude.clamp(0.0, 1.0) } else { 0.0 };
        let seconds = if self.seconds.is_finite() { self.seconds.clamp(0.0, Pulse::LONGEST) } else { 0.0 };
        (amplitude > 0.0 && seconds > 0.0).then_some(Pulse { amplitude, seconds })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Every profile binds the poses and the vibration (the session needs them on any
    /// controller), names each control at most once, and uses paths the runtime can take: an
    /// output only for the vibration, inputs for the rest.
    #[test]
    fn every_profile_has_the_poses_and_well_formed_paths() {
        let mut seen = HashSet::new();
        for profile in PROFILES {
            assert!(profile.path.starts_with("/interaction_profiles/"), "{}", profile.path);
            assert!(seen.insert(profile.path), "{} twice", profile.path);
            let controls: Vec<Control> = profile.bindings.iter().map(|(c, _)| *c).collect();
            // Hands do not vibrate.
            let haptic = (profile.path != HAND_PROFILE).then_some(Control::Haptic);
            for needed in [Control::Grip, Control::Aim, Control::Trigger].into_iter().chain(haptic) {
                assert!(controls.contains(&needed), "{} has no {needed:?}", profile.path);
            }
            for (control, path) in profile.extra {
                assert!(!control.is_output(), "{path}");
                assert!(path.starts_with("/user/hand/left/input/") || path.starts_with("/user/hand/right/input/"), "{path}");
            }
            let unique: HashSet<Control> = controls.iter().copied().collect();
            assert_eq!(unique.len(), controls.len(), "{} binds a control twice", profile.path);
            for (control, path) in profile.bindings {
                let prefix = if control.is_output() { "/output/" } else { "/input/" };
                assert!(path.starts_with(prefix), "{}: {control:?} at {path}", profile.path);
                assert!(path.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '/' || c == '_'), "{path}");
                assert!(!path.ends_with('/') && !path.contains("//"), "{path}");
            }
        }
    }

    /// Action names are what OpenXR takes (lower case, digits, `_`, `-`, `.`) and unique; every
    /// action has a label.
    #[test]
    fn action_names_are_valid_and_unique() {
        let names: HashSet<&str> = Control::ALL.iter().map(|c| c.name()).collect();
        assert_eq!(names.len(), Control::ALL.len());
        for control in Control::ALL {
            assert!(control.name().chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-.".contains(c)));
            assert!(!control.label().is_empty());
        }
    }

    /// Both hands, their order and their paths.
    #[test]
    fn hands_have_an_order_and_paths() {
        assert_eq!(Hand::BOTH.map(Hand::index), [0, 1]);
        assert_eq!(Hand::Left.other(), Hand::Right);
        assert_eq!(Hand::Right.other(), Hand::Left);
        assert_eq!(Hand::Left.path(), "/user/hand/left");
        assert_eq!(Hand::Right.path(), "/user/hand/right");
    }

    /// What a runtime reports is kept to its range: numbers that are not finite read as not
    /// pressed, values past the ends are clamped, and a pose with no rotation is not tracked.
    #[test]
    fn hand_input_refuses_numbers_out_of_range() {
        let pose = Pose { position: [0.1, 1.2, -0.3], orientation: [0.0, 0.0, 0.0, 1.0] };
        let broken = Pose { position: [0.0; 3], orientation: [0.0; 4] };
        let far = Pose { position: [f64::NAN, 0.0, 0.0], orientation: [0.0, 0.0, 0.0, 1.0] };
        let hand = HandInput::new(Some(pose), Some(broken), f32::NAN, 7.0, [f32::INFINITY, -3.0]);
        assert_eq!(hand.grip, Some(pose));
        assert_eq!(hand.aim, None);
        assert_eq!(hand.trigger, 0.0);
        assert_eq!(hand.squeeze, 1.0);
        assert_eq!(hand.stick, [0.0, -1.0]);
        assert_eq!(HandInput::new(Some(far), None, -1.0, 0.5, [0.25, 0.5]), HandInput { squeeze: 0.5, stick: [0.25, 0.5], ..HandInput::NONE });
    }

    /// A button is held from [`PRESS`] and let go below [`RELEASE`], and says on which frame
    /// either happened; a value in between keeps what it was.
    #[test]
    fn a_button_has_a_gap_between_press_and_release() {
        let mut button = Button::default();
        let mut events = Vec::new();
        for value in [0.0, 0.5, PRESS, 0.5, 0.45, RELEASE, 0.5, 0.0, 1.0] {
            button = button.next(value);
            events.push((button.down, button.pressed, button.released));
        }
        assert_eq!(
            events,
            [
                (false, false, false),
                (false, false, false),
                (true, true, false),
                (true, false, false),
                (true, false, false),
                (false, false, true),
                (false, false, false),
                (false, false, false),
                (true, true, false),
            ]
        );
        assert!(!Button::default().next(f32::NAN).down);
    }

    /// A pulse is kept short and within strength; one that would not be felt is not sent.
    #[test]
    fn a_pulse_is_bounded() {
        assert_eq!(Pulse::TICK.bounded(), Some(Pulse::TICK));
        assert_eq!(Pulse { amplitude: 3.0, seconds: 60.0 }.bounded(), Some(Pulse { amplitude: 1.0, seconds: Pulse::LONGEST }));
        assert_eq!(Pulse { amplitude: 0.0, seconds: 0.1 }.bounded(), None);
        assert_eq!(Pulse { amplitude: 0.5, seconds: -1.0 }.bounded(), None);
        assert_eq!(Pulse { amplitude: f32::NAN, seconds: 0.1 }.bounded(), None);
        assert_eq!(Pulse { amplitude: 0.5, seconds: f32::INFINITY }.bounded(), None);
    }

    /// The profile at `path`.
    fn profile(path: &str) -> &'static Profile {
        PROFILES.iter().find(|p| p.path == path).unwrap()
    }

    /// Every path a profile suggests is under one hand's user path, and no hand has one control
    /// bound twice; the shared bindings come once for each hand, then the one-hand ones.
    #[test]
    fn each_profile_binds_each_control_once_per_hand() {
        for p in PROFILES {
            let paths: Vec<(Control, String)> = p.paths().collect();
            assert_eq!(paths.len(), 2 * p.bindings.len() + p.extra.len(), "{}", p.path);
            let mut seen = HashSet::new();
            for (control, path) in &paths {
                let hand = Hand::BOTH.into_iter().find(|h| path.starts_with(&format!("{}/", h.path())));
                let hand = hand.unwrap_or_else(|| panic!("{}: {path} is under no hand", p.path));
                assert!(seen.insert((hand, *control)), "{}: {control:?} twice on {hand:?}", p.path);
            }
            let first = p.bindings.first().map(|(c, input)| (*c, format!("/user/hand/left{input}")));
            assert_eq!(paths.first().cloned(), first, "{}: left first", p.path);
        }
    }

    /// The face buttons: on an Index controller A and B on both hands; on a Touch controller X
    /// and Y on the left, A and B on the right, and the menu button only on the left.
    #[test]
    fn face_buttons_are_bound_by_their_names_on_each_hand() {
        let index: Vec<(Control, String)> = profile("/interaction_profiles/valve/index_controller").paths().collect();
        for hand in ["left", "right"] {
            assert!(index.contains(&(Control::Primary, format!("/user/hand/{hand}/input/a/click"))));
            assert!(index.contains(&(Control::Secondary, format!("/user/hand/{hand}/input/b/click"))));
            assert!(index.contains(&(Control::Trigger, format!("/user/hand/{hand}/input/trigger/value"))));
        }
        let touch: Vec<(Control, String)> = profile("/interaction_profiles/oculus/touch_controller").paths().collect();
        let on = |control: Control| touch.iter().filter(|(c, _)| *c == control).map(|(_, p)| p.as_str()).collect::<Vec<_>>();
        assert_eq!(on(Control::Primary), ["/user/hand/left/input/x/click", "/user/hand/right/input/a/click"]);
        assert_eq!(on(Control::Secondary), ["/user/hand/left/input/y/click", "/user/hand/right/input/b/click"]);
        assert_eq!(on(Control::Menu), ["/user/hand/left/input/menu/click"]);
        let simple: Vec<Control> = profile("/interaction_profiles/khr/simple_controller").paths().map(|(c, _)| c).collect();
        assert!(!simple.contains(&Control::Squeeze) && !simple.contains(&Control::Stick), "one button: no grip, no stick");
    }

    /// Every control the session makes an action for is bound by some profile, so none is left
    /// without a suggestion; only the vibration is an output.
    #[test]
    fn every_control_is_bound_somewhere() {
        for control in Control::ALL {
            assert!(PROFILES.iter().any(|p| p.paths().any(|(c, _)| c == control)), "{control:?} is bound nowhere");
            assert_eq!(control.is_output(), control == Control::Haptic);
        }
    }

    /// Tracked hands are suggested only where the runtime has the extension; every controller
    /// always.
    #[test]
    fn tracked_hands_are_suggested_only_with_the_extension() {
        assert!(!profile(HAND_PROFILE).suggested(false));
        assert!(profile(HAND_PROFILE).suggested(true));
        assert_eq!(PROFILES.iter().filter(|p| p.suggested(false)).count(), PROFILES.len() - 1);
        assert!(PROFILES.iter().all(|p| p.suggested(true)));
    }

    /// A hand read from the runtime's numbers has no buttons pressed and no stick until the
    /// session says so; one with nothing reported is [`HandInput::NONE`].
    #[test]
    fn a_new_hand_has_no_buttons_until_the_session_reads_them() {
        assert_eq!(HandInput::new(None, None, 0.0, 0.0, [0.0; 2]), HandInput::NONE);
        let hand = HandInput::new(None, None, 1.0, 1.0, [1.0, -1.0]);
        assert!(!hand.has_stick && !hand.stick_click && !hand.primary && !hand.secondary && !hand.menu);
        assert_eq!((hand.trigger, hand.squeeze, hand.stick), (1.0, 1.0, [1.0, -1.0]));
    }

    /// A held button that reads no number is let go, never stuck down; one that reads endlessly
    /// far down is pressed, endlessly far up is not.
    #[test]
    fn a_button_reading_no_number_is_let_go() {
        let held = Button::default().next(1.0);
        assert!(held.down && held.pressed);
        let nan = held.next(f32::NAN);
        assert!(!nan.down && nan.released);
        assert!(Button::default().next(f32::INFINITY).pressed);
        assert!(!Button::default().next(f32::NEG_INFINITY).down);
        assert!(!held.next(f32::NEG_INFINITY).down);
        // Held across frames: pressed and released only on the frame they happen.
        let still = held.next(0.9);
        assert!(still.down && !still.pressed && !still.released);
    }
}

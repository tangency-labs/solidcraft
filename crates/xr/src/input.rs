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

/// What the viewer reads from a controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Control {
    /// Where the hand holds the controller (a pose): what a grab moves with.
    Grip,
    /// Where the controller points (a pose, pointing down its −Z): where the ray starts.
    Aim,
    /// The trigger, 0 to 1: pick or apply.
    Trigger,
    /// The grip button, 0 to 1 (a grip that only clicks reads 0 or 1): grab.
    Squeeze,
    /// The thumbstick, or the touchpad where there is none: x right, y up, each −1 to 1.
    Stick,
    /// The controller's vibration.
    Haptic,
    /// Pressing the stick (the touchpad where there is none, the menu button on a one-button
    /// controller): put the model back in front.
    StickClick,
}

impl Control {
    /// Every control, in the order the session makes its actions.
    pub const ALL: [Control; 7] =
        [Control::Grip, Control::Aim, Control::Trigger, Control::Squeeze, Control::Stick, Control::Haptic, Control::StickClick];

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
        }
    }

    /// What the runtime shows for the action in its binding settings. English: OpenXR takes one
    /// name, and the runtime's binding screen is not translated by the app.
    pub fn label(self) -> &'static str {
        match self {
            Control::Grip => "Hand position",
            Control::Aim => "Pointing direction",
            Control::Trigger => "Pick or apply",
            Control::Squeeze => "Grab",
            Control::Stick => "Tool ring and numbers",
            Control::Haptic => "Vibration",
            Control::StickClick => "Put the model back",
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
}

/// The poses and the vibration: every OpenXR controller has them, at the same paths.
const POSES: [(Control, &str); 3] = [(Control::Grip, "/input/grip/pose"), (Control::Aim, "/input/aim/pose"), (Control::Haptic, "/output/haptic")];

/// The controllers the viewer suggests bindings for. The runtime binds any other from one of
/// these. Only inputs both hands have are named, because a suggestion with one unknown path is
/// refused whole.
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
        ],
    },
    // Meta Quest (Touch).
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
        ],
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
        ],
    },
    // Any controller: one button. It picks; there is no grab and no stick.
    Profile {
        path: "/interaction_profiles/khr/simple_controller",
        bindings: &[POSES[0], POSES[1], POSES[2], (Control::Trigger, "/input/select/click"), (Control::StickClick, "/input/menu/click")],
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
    /// The stick is pressed in.
    pub stick_click: bool,
}

impl HandInput {
    /// A hand with no controller: nothing tracked, nothing held.
    pub const NONE: HandInput = HandInput { grip: None, aim: None, trigger: 0.0, squeeze: 0.0, stick: [0.0; 2], stick_click: false };

    /// A hand from what the runtime reported, with every number kept to its range: a value that
    /// is not a finite number counts as not pressed, and a pose that is not one as not tracked.
    pub fn new(grip: Option<Pose>, aim: Option<Pose>, trigger: f32, squeeze: f32, stick: [f32; 2]) -> HandInput {
        let amount = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        let axis = |v: f32| if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 };
        let pose = |p: Option<Pose>| p.filter(|p| p.axes().is_some());
        HandInput { grip: pose(grip), aim: pose(aim), trigger: amount(trigger), squeeze: amount(squeeze), stick: stick.map(axis), stick_click: false }
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
            for needed in [Control::Grip, Control::Aim, Control::Trigger, Control::Haptic] {
                assert!(controls.contains(&needed), "{} has no {needed:?}", profile.path);
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
}

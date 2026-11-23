//! The controllers' actions for the runtime (see [`crate::input`]): made, bound and attached
//! once a session asks for them, then read once a frame.

use openxr as xr;

use super::{Error, pose_of};
use crate::input::{Control, Hand, HandInput, PROFILES, Pulse};

/// The viewer's actions, the hands' paths, and a space for each hand's two poses.
pub(super) struct Controls {
    // Dropped in this order: the spaces before the actions they follow, the actions before
    // their set.
    /// Grip and aim space of each hand, left first.
    spaces: [[xr::Space; 2]; 2],
    grip: xr::Action<xr::Posef>,
    aim: xr::Action<xr::Posef>,
    trigger: xr::Action<f32>,
    squeeze: xr::Action<f32>,
    stick: xr::Action<xr::Vector2f>,
    haptic: xr::Action<xr::Haptic>,
    /// Stick click, the two face buttons and the menu button, in that order.
    clicks: [xr::Action<bool>; 4],
    set: xr::ActionSet,
    /// Each hand's user path, left first.
    hands: [xr::Path; 2],
}

impl Controls {
    /// Make the actions, suggest [`PROFILES`] to the runtime, and attach them to `session`. A
    /// profile the runtime refuses is left out (it says so in the log); none taken is an error.
    /// Tracked hands are suggested only with `hands` (the runtime has `XR_EXT_hand_interaction`).
    pub(super) fn new(instance: &xr::Instance, session: &xr::Session<xr::Vulkan>, hands_tracked: bool) -> Result<Controls, Error> {
        let path = |p: &str| instance.string_to_path(p).map_err(|e| Error::xr("naming a controller input", e));
        let hands = [path(Hand::Left.path())?, path(Hand::Right.path())?];
        let set = instance.create_action_set("viewer", "SolidCraft VR viewer", 0).map_err(|e| Error::xr("making the controls", e))?;
        let make_error = |e| Error::xr("making a control", e);
        let grip = set.create_action::<xr::Posef>(Control::Grip.name(), Control::Grip.label(), &hands).map_err(make_error)?;
        let aim = set.create_action::<xr::Posef>(Control::Aim.name(), Control::Aim.label(), &hands).map_err(make_error)?;
        let trigger = set.create_action::<f32>(Control::Trigger.name(), Control::Trigger.label(), &hands).map_err(make_error)?;
        let squeeze = set.create_action::<f32>(Control::Squeeze.name(), Control::Squeeze.label(), &hands).map_err(make_error)?;
        let stick = set.create_action::<xr::Vector2f>(Control::Stick.name(), Control::Stick.label(), &hands).map_err(make_error)?;
        let haptic = set.create_action::<xr::Haptic>(Control::Haptic.name(), Control::Haptic.label(), &hands).map_err(make_error)?;
        let click = |c: Control| set.create_action::<bool>(c.name(), c.label(), &hands).map_err(make_error);
        let clicks = [click(Control::StickClick)?, click(Control::Primary)?, click(Control::Secondary)?, click(Control::Menu)?];

        let mut taken = 0;
        for profile in PROFILES.iter().filter(|p| p.suggested(hands_tracked)) {
            let suggested = (|| -> Result<(), Error> {
                let mut bindings = Vec::with_capacity(2 * profile.bindings.len() + profile.extra.len());
                for (control, at) in profile.paths() {
                    let at = path(&at)?;
                    bindings.push(match control {
                        Control::Grip => xr::Binding::new(&grip, at),
                        Control::Aim => xr::Binding::new(&aim, at),
                        Control::Trigger => xr::Binding::new(&trigger, at),
                        Control::Squeeze => xr::Binding::new(&squeeze, at),
                        Control::Stick => xr::Binding::new(&stick, at),
                        Control::Haptic => xr::Binding::new(&haptic, at),
                        Control::StickClick => xr::Binding::new(&clicks[0], at),
                        Control::Primary => xr::Binding::new(&clicks[1], at),
                        Control::Secondary => xr::Binding::new(&clicks[2], at),
                        Control::Menu => xr::Binding::new(&clicks[3], at),
                    });
                }
                instance.suggest_interaction_profile_bindings(path(profile.path)?, &bindings).map_err(|e| Error::xr("suggesting bindings", e))
            })();
            match suggested {
                Ok(()) => taken += 1,
                Err(e) => log::info!("VR: no bindings for {}: {e}", profile.path),
            }
        }
        if taken == 0 {
            return Err(Error::Unsupported("the VR software took no controller bindings".into()));
        }
        session.attach_action_sets(&[&set]).map_err(|e| Error::xr("attaching the controls", e))?;
        let space = |action: &xr::Action<xr::Posef>, hand: xr::Path| {
            action.create_space(session, hand, xr::Posef::IDENTITY).map_err(|e| Error::xr("following a controller", e))
        };
        let spaces = [[space(&grip, hands[0])?, space(&aim, hands[0])?], [space(&grip, hands[1])?, space(&aim, hands[1])?]];
        Ok(Controls { spaces, grip, aim, trigger, squeeze, stick, haptic, clicks, set, hands })
    }

    /// Both hands at `time`, in `room`, left first. A hand whose controller is off, not tracked
    /// or not read reads as [`HandInput::NONE`] (or with its poses missing); the runtime refusing
    /// the read is logged, not an error: the frame goes on without the controllers.
    pub(super) fn read(&self, session: &xr::Session<xr::Vulkan>, room: &xr::Space, time: xr::Time) -> [HandInput; 2] {
        if let Err(e) = session.sync_actions(&[xr::ActiveActionSet::new(&self.set)]) {
            log::debug!("VR: the controllers were not read: {e}");
            return [HandInput::NONE; 2];
        }
        Hand::BOTH.map(|hand| {
            let path = self.hands[hand.index()];
            let value = |action: &xr::Action<f32>| action.state(session, path).ok().filter(|s| s.is_active).map_or(0.0, |s| s.current_state);
            let stick_state = self.stick.state(session, path).ok().filter(|s| s.is_active);
            let stick = stick_state.map_or([0.0; 2], |s| [s.current_state.x, s.current_state.y]);
            let [grip_space, aim_space] = &self.spaces[hand.index()];
            let locate = |action: &xr::Action<xr::Posef>, space: &xr::Space| {
                if !action.is_active(session, path).unwrap_or(false) {
                    return None;
                }
                let location = space.locate(room, time).ok()?;
                let valid = xr::SpaceLocationFlags::ORIENTATION_VALID | xr::SpaceLocationFlags::POSITION_VALID;
                location.location_flags.contains(valid).then(|| pose_of(location.pose))
            };
            let hand =
                HandInput::new(locate(&self.grip, grip_space), locate(&self.aim, aim_space), value(&self.trigger), value(&self.squeeze), stick);
            let [stick_click, primary, secondary, menu] =
                self.clicks.each_ref().map(|a| a.state(session, path).ok().is_some_and(|s| s.is_active && s.current_state));
            HandInput { has_stick: stick_state.is_some(), stick_click, primary, secondary, menu, ..hand }
        })
    }

    /// Vibrate `hand`'s controller with `pulse`, kept within [`Pulse::bounded`]. A runtime that
    /// refuses is logged: a missed vibration is not worth stopping for.
    pub(super) fn pulse(&self, session: &xr::Session<xr::Vulkan>, hand: Hand, pulse: Pulse) {
        let Some(pulse) = pulse.bounded() else { return };
        let nanos = (f64::from(pulse.seconds) * 1e9) as i64;
        let event =
            xr::HapticVibration::new().amplitude(pulse.amplitude).duration(xr::Duration::from_nanos(nanos)).frequency(xr::FREQUENCY_UNSPECIFIED);
        if let Err(e) = self.haptic.apply_feedback(session, self.hands[hand.index()], &event) {
            log::debug!("VR: the controller did not vibrate: {e}");
        }
    }
}

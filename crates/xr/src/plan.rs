//! What a session decides from what the runtime reports: the picture format, the picture sizes,
//! and what each session state asks of the app. Plain functions over plain values, so they are
//! tested without a headset.

/// A colour format the pictures handed to a runtime can have, as the runtime names it (its
/// Vulkan format number) and as the renderer does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorFormat {
    /// `VK_FORMAT_B8G8R8A8_SRGB`.
    Bgra8Srgb,
    /// `VK_FORMAT_R8G8B8A8_SRGB`.
    Rgba8Srgb,
}

impl ColorFormat {
    /// The formats in the order they are preferred.
    pub const PREFERRED: [ColorFormat; 2] = [ColorFormat::Bgra8Srgb, ColorFormat::Rgba8Srgb];

    /// The Vulkan format number.
    pub fn vulkan(self) -> i64 {
        match self {
            ColorFormat::Bgra8Srgb => 50,
            ColorFormat::Rgba8Srgb => 43,
        }
    }
}

/// The format to ask for among those the runtime `offered`: the first of
/// [`ColorFormat::PREFERRED`] it has. Only sRGB formats are taken: a runtime reads a plain 8-bit
/// format as linear light, which bands in the dark tones a design view is full of. `None` when it
/// offers neither.
pub fn pick_format(offered: &[i64]) -> Option<ColorFormat> {
    ColorFormat::PREFERRED.into_iter().find(|f| offered.contains(&f.vulkan()))
}

/// Most pixels one eye's picture may have: about 4096 × 4096. A runtime set to several times the
/// headset's own resolution would otherwise ask for a multisampled frame of gigabytes.
pub const EYE_PIXELS: u64 = 1 << 24;

/// The size to draw one eye at: the runtime's `recommended` size, within its `largest` size for
/// the view, the device's largest texture side `max_side` and [`EYE_PIXELS`] (made smaller in
/// proportion to fit). `None` when the runtime recommends no size at all.
pub fn eye_size(recommended: [u32; 2], largest: [u32; 2], max_side: u32) -> Option<[u32; 2]> {
    let [w, h] = recommended;
    if w == 0 || h == 0 {
        return None;
    }
    let side = |x: u32, most: u32| x.min(if most == 0 { u32::MAX } else { most }).min(max_side.max(1)).max(1);
    let (mut w, mut h) = (side(w, largest[0]), side(h, largest[1]));
    let pixels = u64::from(w) * u64::from(h);
    if pixels > EYE_PIXELS {
        let f = (EYE_PIXELS as f64 / pixels as f64).sqrt();
        w = ((f64::from(w) * f).floor() as u32).max(1);
        h = ((f64::from(h) * f).floor() as u32).max(1);
    }
    Some([w, h])
}

/// The state a runtime reports for a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Ready,
    Synchronized,
    Visible,
    Focused,
    Stopping,
    LossPending,
    Exiting,
    /// A state this code does not know: taken as "keep going".
    Other,
}

/// What a new session state asks of the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Nothing changes.
    Stay,
    /// The runtime is ready for frames: begin the session.
    Begin,
    /// The runtime wants the frames to stop: end the session (it may become ready again).
    End,
    /// The session is over (the user left, or the runtime is going away): close it, and show the
    /// design on the desktop only.
    Close,
}

/// Whether frames are running, and what the runtime last said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lifecycle {
    /// Between a begun and an ended session: frames are waited for and handed in.
    pub running: bool,
    /// The session is over; no state brings it back.
    pub closed: bool,
}

impl Lifecycle {
    /// A session just created: not running yet.
    pub const NEW: Lifecycle = Lifecycle { running: false, closed: false };

    /// The runtime reports `state`: what to do, and the lifecycle after it is done.
    pub fn on(self, state: State) -> (Step, Lifecycle) {
        if self.closed {
            return (Step::Stay, self);
        }
        match state {
            State::Ready if !self.running => (Step::Begin, Lifecycle { running: true, ..self }),
            State::Stopping if self.running => (Step::End, Lifecycle { running: false, ..self }),
            State::Exiting | State::LossPending => (Step::Close, Lifecycle { running: false, closed: true }),
            _ => (Step::Stay, self),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Quest 3S on the Oculus runtime and the Steam Frame on SteamVR both offer the sRGB
    /// formats among others; BGRA is taken when both are there.
    #[test]
    fn an_srgb_format_is_picked_in_the_preferred_order() {
        assert_eq!(pick_format(&[37, 43, 44, 50, 97, 126]), Some(ColorFormat::Bgra8Srgb));
        assert_eq!(pick_format(&[43, 37]), Some(ColorFormat::Rgba8Srgb));
        assert_eq!(pick_format(&[50]), Some(ColorFormat::Bgra8Srgb));
    }

    /// A runtime with only linear or float formats gets no picture rather than a banded one.
    #[test]
    fn no_format_is_picked_from_a_list_without_srgb() {
        assert_eq!(pick_format(&[]), None);
        assert_eq!(pick_format(&[37, 44, 97, -1, i64::MAX]), None);
    }

    /// The recommended size is taken as it is where it fits (Steam Frame: 2644 × 2644 of 8192).
    #[test]
    fn an_eye_is_drawn_at_the_recommended_size() {
        assert_eq!(eye_size([2644, 2644], [8192, 8192], 16384), Some([2644, 2644]));
        assert_eq!(eye_size([1824, 1872], [3648, 3744], 8192), Some([1824, 1872]));
    }

    /// A recommended size past the view's, the device's or the pixel limit is made smaller; one
    /// with no pixels draws nothing.
    #[test]
    fn an_eye_size_out_of_range_is_brought_into_it() {
        assert_eq!(eye_size([5000, 3000], [4000, 4000], 16384), Some([4000, 3000]));
        assert_eq!(eye_size([5000, 3000], [8192, 8192], 2048), Some([2048, 2048]));
        let [w, h] = eye_size([8192, 8192], [8192, 8192], 16384).unwrap();
        assert!(u64::from(w) * u64::from(h) <= EYE_PIXELS && w == h && w >= 4000, "{w} x {h}");
        let [w, h] = eye_size([u32::MAX, u32::MAX], [0, 0], u32::MAX).unwrap();
        assert!(u64::from(w) * u64::from(h) <= EYE_PIXELS && w > 0 && h > 0, "{w} x {h}");
        assert_eq!(eye_size([0, 2000], [4000, 4000], 8192), None);
        assert_eq!(eye_size([2000, 0], [4000, 4000], 8192), None);
        assert_eq!(eye_size([100, 100], [4000, 4000], 0), Some([1, 1]));
    }

    /// Ready begins the frames, stopping ends them, and ready begins them again (the headset was
    /// taken off and put back on).
    #[test]
    fn frames_run_between_ready_and_stopping() {
        let (step, life) = Lifecycle::NEW.on(State::Idle);
        assert_eq!((step, life.running), (Step::Stay, false));
        let (step, life) = life.on(State::Ready);
        assert_eq!((step, life.running), (Step::Begin, true));
        for state in [State::Synchronized, State::Visible, State::Focused, State::Visible, State::Other, State::Ready] {
            assert_eq!(life.on(state), (Step::Stay, life), "{state:?}");
        }
        let (step, life) = life.on(State::Stopping);
        assert_eq!((step, life.running, life.closed), (Step::End, false, false));
        assert_eq!(life.on(State::Stopping), (Step::Stay, life), "stopping twice ends once");
        let (step, life) = life.on(State::Idle).1.on(State::Ready);
        assert_eq!((step, life.running), (Step::Begin, true));
    }

    /// Exiting and a pending loss close the session whatever it was doing, and nothing reopens it.
    #[test]
    fn an_exit_or_a_loss_closes_the_session_for_good() {
        let running = Lifecycle::NEW.on(State::Ready).1;
        for from in [Lifecycle::NEW, running] {
            for state in [State::Exiting, State::LossPending] {
                let (step, life) = from.on(state);
                assert_eq!((step, life), (Step::Close, Lifecycle { running: false, closed: true }), "{from:?} {state:?}");
                for later in [State::Ready, State::Stopping, State::Exiting, State::Focused] {
                    assert_eq!(life.on(later), (Step::Stay, life), "{later:?} after {state:?}");
                }
            }
        }
    }
}

//! Motions shared by the viewport's handles: a state factor that eases in and out each way on its
//! own terms, a handle stepping back while another works, a colour that comes and goes, the
//! offset a press keeps for its drag, and an offset that slides instead of jumping. egui's own
//! `animate_bool_with_time` is one time both ways and linear; these are what the placement
//! handles needed on top of it (#4384), kept apart so the next manipulator reuses them.

use egui::{Color32, Context, Id, Pos2, Response, Ui, Vec2};

/// A 0 to 1 factor that eases towards 1 while `on` and back to 0 when not, each way with its own
/// time and easing: a hover that opens slowly and leaves fast, for one.
#[derive(Clone, Copy)]
pub struct Transition {
    /// Seconds to reach 1, and to fall back to 0.
    pub on_s: f32,
    pub off_s: f32,
    /// The easing each way (`egui::emath::easing`).
    pub ease_on: fn(f32) -> f32,
    pub ease_off: fn(f32) -> f32,
}

impl Transition {
    /// The same time both ways, linear.
    pub const fn symmetric(s: f32) -> Self {
        Self { on_s: s, off_s: s, ease_on: egui::emath::easing::linear, ease_off: egui::emath::easing::linear }
    }

    /// The factor this frame. Repaints only while it moves.
    pub fn run(&self, ctx: &Context, id: Id, on: bool) -> Factor {
        let (time, ease) = if on { (self.on_s, self.ease_on) } else { (self.off_s, self.ease_off) };
        Factor { t: ctx.animate_bool_with_time_and_easing(id, on, time, ease), rising: on }
    }
}

/// A [`Transition`]'s value: where it is and which way it goes, so a look can morph on the way
/// up and fade on the way down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Factor {
    pub t: f32,
    pub rising: bool,
}

impl Factor {
    /// On the way down and not yet gone.
    pub fn fading(self) -> bool {
        !self.rising && self.t > 0.0
    }
}

/// How much of itself a thing keeps while it steps back: 1 normally, `keep` while `back`,
/// eased over `s` seconds.
pub fn step_back(ctx: &Context, id: Id, back: bool, keep: f32, s: f32) -> f32 {
    1.0 - (1.0 - keep) * ctx.animate_bool_with_time(id, back, s)
}

/// `colour` eased in over `s` seconds from `from`, and eased out again from the last colour given
/// once there is none: the colour it fades out of is the one it was on.
pub fn tint(ctx: &Context, id: Id, colour: Option<Color32>, from: Color32, s: f32) -> Option<Color32> {
    if let Some(c) = colour {
        ctx.data_mut(|d| d.insert_temp(id.with("colour"), c));
    }
    let t = ctx.animate_bool_with_time(id, colour.is_some(), s);
    ctx.data(|d| d.get_temp::<Color32>(id.with("colour"))).filter(|_| t > 0.01).map(|c| from.lerp_to_gamma(c, t))
}

/// The gap a press keeps for its drag: `at_press` measures it from where the button went down
/// (none when it cannot), and every dragged frame gets it back, so what is dragged follows the
/// pointer from where it was grabbed instead of jumping onto it at the first move. Measured from
/// the press, not the pointer when the drag starts: a handle that also takes clicks only knows it
/// is dragged once the pointer has moved a few points.
pub fn grab<T: Clone + Send + Sync + 'static>(ui: &Ui, id: Id, r: &Response, at_press: impl FnOnce(Pos2) -> Option<T>) -> Option<T> {
    if r.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin()).or_else(|| r.interact_pointer_pos())
    {
        match at_press(p) {
            Some(g) => {
                ui.data_mut(|d| d.insert_temp(id, g));
            }
            None => ui.data_mut(|d| d.remove::<T>(id)),
        }
    }
    if r.dragged() { ui.data(|d| d.get_temp::<T>(id)) } else { None }
}

/// An offset that slides to `target` over `s` seconds instead of jumping there; the first call
/// lands at once.
pub fn slide(ctx: &Context, id: Id, target: Vec2, s: f32) -> Vec2 {
    Vec2::new(ctx.animate_value_with_time(id.with("x"), target.x, s), ctx.animate_value_with_time(id.with("y"), target.y, s))
}

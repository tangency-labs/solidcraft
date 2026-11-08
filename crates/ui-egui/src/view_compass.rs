//! The view compass as a component: the six signed world axes drawn as the camera's view axes
//! pose them, a Home chip, and a drag anywhere on it to orbit. It takes plain inputs
//! ([`Compass`], [`ViewAxes`]) and returns the [`ViewAction`] chosen, and [`orbit`] returns a drag's
//! [`OrbitStep`]; the app turns the camera.
//! Every tip publishes its handle (`axis_view:<axis>`) to the widget registry while it is drawn.
use crate::icons::navigation::AXIS_RADIUS;
use crate::ui_kit::BackingTooltipExt as _;
use egui::{Pos2, Rect, Sense, Stroke, pos2, vec2};
use solidcraft_theme::Tokens;

/// How far a tip is from the centre, before [`ART_SCALE`].
const AXIS_LENGTH: f64 = 38.0;
/// The artwork sits inside the pointer reach so the compass keeps breathing room in its circle.
const ART_SCALE: f64 = 0.84;
/// The compass circle's radius: anywhere inside drags the view.
const RADIUS: f32 = 49.5;

/// The six signed world axes, by name.
pub const AXES: [(&str, [f64; 3]); 6] = [
    ("x", [1.0, 0.0, 0.0]),
    ("-x", [-1.0, 0.0, 0.0]),
    ("y", [0.0, 1.0, 0.0]),
    ("-y", [0.0, -1.0, 0.0]),
    ("z", [0.0, 0.0, 1.0]),
    ("-z", [0.0, 0.0, -1.0]),
];

/// The camera's view axes in world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewAxes {
    pub right: [f64; 3],
    pub up: [f64; 3],
    /// Toward the viewer.
    pub back: [f64; 3],
}

impl ViewAxes {
    /// World direction `d` as the camera poses it: its parts along the screen's right and down,
    /// and its depth toward the viewer.
    pub fn pose(&self, d: [f64; 3]) -> ([f64; 2], f64) {
        ([dot(d, self.right), -dot(d, self.up)], dot(d, self.back))
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// One signed axis as drawn: its name (`"x"`, `"-x"`, ... `"-z"`), world direction, tip on screen
/// and depth along the camera's back.
pub type Tip = (&'static str, [f64; 3], Pos2, f64);

/// What the user chose on the compass: a tip or Home.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ViewAction {
    /// Look along an axis: a tip was clicked (`direction`: the tip's world direction).
    Snap { axis: &'static str, direction: [f64; 3] },
    /// The Home chip was clicked.
    Home,
}

/// One frame's step of a drag on a view navigator: `(dx, dy)` screen points (`snap`: Shift was
/// held).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OrbitStep {
    pub dx: f64,
    pub dy: f64,
    pub snap: bool,
}

/// One frame of the compass.
#[derive(Clone, Debug)]
pub struct Compass {
    /// The area it may paint in.
    pub clip: Rect,
    pub centre: Pos2,
    pub axes: ViewAxes,
    /// The pointer, when it may act on the compass (the app leaves out a pointer over a floating
    /// window or with the view disabled).
    pub hover: Option<Pos2>,
    /// A drag on it is turning the view (no tip lights while it does).
    pub orbiting: bool,
    /// The tip's hover text mentions holding Shift to snap.
    pub snap_hint: bool,
    pub home_label: String,
}

/// Where a tip is drawn from the centre, for a direction posed on screen as `[right, down]`
/// ([`ViewAxes::pose`]); a figure of the compass scales it.
pub fn tip_offset([x, y]: [f64; 2]) -> egui::Vec2 {
    vec2((x * AXIS_LENGTH * ART_SCALE) as f32, (y * AXIS_LENGTH * ART_SCALE) as f32)
}

/// The six signed axes as `axes` poses them around `c`, back to front so overlapping tips have
/// one owner.
pub fn tips(axes: &ViewAxes, c: Pos2) -> Vec<Tip> {
    let mut tips: Vec<Tip> = AXES
        .into_iter()
        .map(|(name, d)| {
            let (screen, depth) = axes.pose(d);
            (name, d, c + tip_offset(screen), depth)
        })
        .collect();
    tips.sort_by(|a, b| a.3.total_cmp(&b.3));
    tips
}

/// The front-most tip within reach of `p`.
pub fn hit(tips: &[Tip], p: Pos2) -> Option<usize> {
    tips.iter().enumerate().rev().find(|(_, (_, _, at, _))| at.distance(p) <= AXIS_RADIUS).map(|(i, _)| i)
}

/// Draw the compass; a tip or Home clicked this frame (a drag on it is [`orbit`]'s).
pub fn show(ui: &mut egui::Ui, compass: &Compass) -> Option<ViewAction> {
    let t = Tokens::get();
    let c = compass.centre;
    let tips = tips(&compass.axes, c);
    let hover = compass.hover;
    let hovered = if compass.orbiting { None } else { hover.and_then(|p| hit(&tips, p)) };
    let painter = ui.painter_at(compass.clip);
    let mut action = None;
    // The circle turns fully opaque under the pointer.
    let in_circle = hover.is_some_and(|p| p.distance(c) <= RADIUS);
    let fill = if in_circle || compass.orbiting { 1.0 } else { 0.35 };
    painter.circle(c, RADIUS, t.overlay.gamma_multiply(fill), Stroke::new(1.0, t.border.gamma_multiply(0.75)));
    for (i, (name, d, at, depth)) in tips.iter().enumerate() {
        if i == 3 {
            // The origin dot sits behind the three front axes and over the three back ones.
            painter.circle(c, 3.6, t.text.gamma_multiply(0.9), Stroke::new(1.4, t.panel));
        }
        crate::icons::navigation::axis_tip(&painter, c, name, *d, *at, *depth, hovered == Some(i), t);
        solidcraft_ui_registry::publish_handle(&format!("axis_view:{name}"), *at);
    }
    solidcraft_ui_registry::publish_count("axis_view_highlights", f64::from(u8::from(hovered.is_some())));
    if let Some(i) = hovered
        && let Some((name, d, _, _)) = tips.get(i)
    {
        solidcraft_ui_registry::publish_handle(&format!("axis_view_hover:{name}"), hover.unwrap_or(c));
        ui.interact(Rect::from_center_size(hover.unwrap_or(c), vec2(2.0, 2.0)), ui.scope_id().with("axis_view_tip"), Sense::hover())
            .backing_on_hover_text(format!(
                "View from {}. Click again to reverse. Drag to orbit{}.",
                name.to_uppercase(),
                if compass.snap_hint { "; hold Shift to snap" } else { "" }
            ));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if ui.input(|i| i.pointer.primary_clicked()) {
            action = Some(ViewAction::Snap { axis: name, direction: *d });
        }
    }
    let home = Rect::from_center_size(pos2(c.x - 37.0, c.y - 37.0), vec2(18.0, 18.0));
    solidcraft_ui_registry::publish_target("axis_view:home", home);
    let hr = ui.interact(home, ui.scope_id().with("axis_view_home"), Sense::click());
    // The home button sits in its own small round chip.
    painter.circle(home.center(), 12.0, t.overlay.gamma_multiply(if hr.hovered() { 0.9 } else { 0.5 }), Stroke::new(1.0, t.border));
    crate::icons::paint_icon(&painter, home, "home", f32::from(t.icon.a()) / 255.0);
    // Cursors: a grabbing hand while orbiting, an open hand over the circle, a pointer on the home
    // chip (the axis ends keep their pointing hand).
    if hr.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    } else if compass.orbiting {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if in_circle && hovered.is_none() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if hr.backing_on_hover_text(&compass.home_label).clicked() {
        action = Some(ViewAction::Home);
    }
    action
}

/// Where a drag on a view navigator stands between frames: the pointer at the last step, and
/// where the cursor was warped back to (if it hit the screen's edge).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Drag {
    pub at: Option<Pos2>,
    pub warp: Option<Pos2>,
}

/// One frame of a drag on a view navigator: whether it started or ended, whether it is turning
/// the view, and this frame's step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Orbit {
    /// A press landed on the navigator and holds it.
    pub grabbed: bool,
    /// The button was let go of a held navigator.
    pub released: bool,
    /// The drag is turning the view.
    pub orbiting: bool,
    pub step: Option<OrbitStep>,
}

/// Track a drag on a view navigator. A press may start one (`may_start`: the pointer is over the
/// view and free to act) where `starts_at` says the navigator is; once the drag is decided each
/// frame's motion is an [`OrbitStep`]. The cube and the custom body share it.
pub fn orbit(ui: &egui::Ui, drag: &mut Drag, may_start: bool, starts_at: impl Fn(Pos2) -> bool) -> Orbit {
    let mut out = Orbit::default();
    let (pressed, down, decided, origin, pos, shift) = ui.input(|i| {
        let p = &i.pointer;
        (p.primary_pressed(), p.primary_down(), p.is_decidedly_dragging(), p.press_origin(), p.interact_pos(), i.modifiers.shift)
    });
    if pressed && may_start {
        drag.at = origin.filter(|p| starts_at(*p));
        drag.warp = None;
        out.grabbed = drag.at.is_some();
    }
    if !down && drag.at.take().is_some() {
        drag.warp = None;
        out.released = true;
    }
    let Some(last) = drag.at.filter(|_| decided) else { return out };
    out.orbiting = true;
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    if let Some(p) = pos {
        // While a warp is pending the pointer may still sit at the edge it hit (the OS has not
        // echoed the warp, or cannot), have landed at the centre (the echo), or have moved past
        // it (the echo plus a fast flick): measure each step from whichever reference the
        // pointer is nearer to, so the view never jumps by the warp distance and a platform
        // that ignores warps keeps orbiting from the real pointer instead of freezing.
        let from = match drag.warp {
            Some(w) if p.distance(w) <= p.distance(last) => {
                drag.warp = None;
                w
            }
            _ => last,
        };
        out.step = Some(OrbitStep { dx: f64::from(p.x - from.x), dy: f64::from(p.y - from.y), snap: shift });
        drag.at = Some(p);
        // The OS stops feeding movement once the cursor is pinned at the screen edge, so an
        // orbit toward that edge would stall: warp the cursor back to the window centre and
        // measure the next steps from there, keeping the orbit tracking while the button is held (#9943).
        let screen = ui.ctx().viewport_rect();
        let edge = 6.0;
        if drag.warp.is_none()
            && (p.x <= screen.min.x + edge || p.x >= screen.max.x - edge || p.y <= screen.min.y + edge || p.y >= screen.max.y - edge)
        {
            let home = screen.center();
            drag.warp = Some(home);
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::CursorPosition(home));
        }
        ui.ctx().request_repaint();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, RawInput};

    /// Looking at the front: X to the right, Z up, -Y toward the viewer.
    const FRONT: ViewAxes = ViewAxes { right: [1.0, 0.0, 0.0], up: [0.0, 0.0, 1.0], back: [0.0, -1.0, 0.0] };

    fn run(ctx: &egui::Context, events: Vec<Event>, hover: Option<Pos2>, drag: &mut Drag) -> (Option<ViewAction>, Orbit) {
        let mut out = (None, Orbit::default());
        crate::test_support::frame(
            ctx,
            RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0))), events, focused: true, ..Default::default() },
            |ui| {
                let orbit = super::orbit(ui, drag, true, |p| p.distance(pos2(200.0, 200.0)) <= RADIUS);
                let compass = Compass {
                    clip: ui.max_rect(),
                    centre: pos2(200.0, 200.0),
                    axes: FRONT,
                    hover,
                    orbiting: orbit.orbiting,
                    snap_hint: true,
                    home_label: "Home".into(),
                };
                out = (show(ui, &compass), orbit);
            },
        );
        out
    }

    fn click(ctx: &egui::Context, at: Pos2) -> Option<ViewAction> {
        let mut drag = Drag::default();
        run(ctx, vec![Event::PointerMoved(at)], Some(at), &mut drag);
        let pressed = run(ctx, crate::test_support::pointer(at, true), Some(at), &mut drag).0;
        pressed.or(run(ctx, crate::test_support::pointer(at, false), Some(at), &mut drag).0)
    }

    #[test]
    fn a_click_on_a_tip_or_home_returns_its_view_from_plain_axes() {
        let ctx = crate::fonts::test_context();
        let tips = tips(&FRONT, pos2(200.0, 200.0));
        let x = tips.iter().find(|t| t.0 == "x").map(|t| t.2).unwrap();
        assert_eq!(x, pos2(200.0 + (38.0 * 0.84) as f32, 200.0), "X points right");
        assert_eq!(click(&ctx, x), Some(ViewAction::Snap { axis: "x", direction: [1.0, 0.0, 0.0] }));
        let z = tips.iter().find(|t| t.0 == "z").map(|t| t.2).unwrap();
        assert_eq!(click(&ctx, z), Some(ViewAction::Snap { axis: "z", direction: [0.0, 0.0, 1.0] }));
        assert_eq!(click(&ctx, pos2(163.0, 163.0)), Some(ViewAction::Home));
        assert_eq!(click(&ctx, pos2(20.0, 20.0)), None, "outside the compass");
    }

    #[test]
    fn a_drag_on_the_compass_returns_orbit_steps_and_ends_on_release() {
        let ctx = crate::fonts::test_context();
        let mut drag = Drag::default();
        let start = pos2(180.0, 215.0);
        run(&ctx, vec![Event::PointerMoved(start)], Some(start), &mut drag);
        let (_, pressed) = run(&ctx, crate::test_support::pointer(start, true), Some(start), &mut drag);
        assert!(pressed.grabbed && pressed.step.is_none());
        let mut steps = Vec::new();
        for x in [190.0, 200.0, 230.0] {
            let at = pos2(x, 205.0);
            let (snap, orbit) = run(&ctx, vec![Event::PointerMoved(at)], Some(at), &mut drag);
            assert_eq!(snap, None, "dragging over a tip does not snap");
            steps.extend(orbit.step);
        }
        let travel = steps.iter().fold((0.0, 0.0, false), |(x, y, held), s| (x + s.dx, y + s.dy, held || s.snap));
        assert!(!steps.is_empty());
        assert_eq!(travel, (50.0, -10.0, false), "the steps add up to the pointer's travel once the drag is decided");
        let end = pos2(230.0, 205.0);
        let (_, released) = run(&ctx, crate::test_support::pointer(end, false), Some(end), &mut drag);
        assert!(released.released && drag.at.is_none());
    }
}

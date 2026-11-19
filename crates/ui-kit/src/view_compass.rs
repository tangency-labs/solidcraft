//! The view compass as a component: the six signed world axes drawn as the camera's view axes
//! pose them, a Home chip, and a drag anywhere on it to orbit. It takes plain inputs
//! ([`Compass`], [`ViewAxes`]) and returns the [`ViewAction`] chosen, and [`orbit`] returns a drag's
//! [`OrbitStep`]; the app turns the camera.
//! Every tip publishes its handle (`axis_view:<axis>`) to the widget registry while it is drawn.
use crate::BackingTooltipExt as _;
use egui::{Pos2, Rect, Sense, Stroke, pos2, vec2};
use solidcraft_theme::Tokens;

/// How far a tip is from the centre, before [`ART_SCALE`].
const AXIS_LENGTH: f64 = 38.0;
/// The artwork sits inside the pointer reach so the compass keeps breathing room in its circle.
const ART_SCALE: f64 = 0.84;
/// The compass circle's radius: anywhere inside drags the view.
pub const RADIUS: f32 = 49.5;

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

/// How big a tip is drawn at `depth` (1 toward the viewer, -1 away), as a multiple of its size
/// edge-on: a little larger facing the viewer, much smaller pointing away, so the far ends read as
/// far. Its pointer target keeps its full size ([`hit`]).
pub fn depth_scale(depth: f64) -> f32 {
    let d = if depth.is_finite() { depth.clamp(-1.0, 1.0) } else { 0.0 };
    (if d >= 0.0 { 1.0 + 0.25 * d } else { 1.0 + 0.6 * d }) as f32
}

/// The floor disc: the circle in the world XY plane through the X and Y tips, as `axes` poses it
/// around `c`. A full circle seen from the top or bottom, a flat line at eye level; it follows roll.
pub fn floor(axes: &ViewAxes, c: Pos2) -> Vec<Pos2> {
    (0..64)
        .map(|i| {
            let a = f64::from(i) / 64.0 * std::f64::consts::TAU;
            c + tip_offset(axes.pose([a.cos(), a.sin(), 0.0]).0)
        })
        .collect()
}

/// The view a tip looks from, as the catalog names it.
fn view_name(axis: &str) -> &'static str {
    match axis {
        "x" => "ui.view_compass.right",
        "-x" => "ui.view_compass.left",
        "y" => "ui.view_compass.back",
        "-y" => "ui.view_compass.front",
        "z" => "ui.view_compass.top",
        _ => "ui.view_compass.bottom",
    }
}

/// The tip on the other end of `axis`'s line.
fn opposite(axis: &str) -> &str {
    match axis.strip_prefix('-') {
        Some(positive) => positive,
        None => match axis {
            "x" => "-x",
            "y" => "-y",
            _ => "-z",
        },
    }
}

/// The hovered tip's label: its disc widened into a pill in the axis colour that names the view
/// (and, when the view already looks from it, the view a click turns to). The disc stays where it
/// is; the pill grows away from the centre, or the other way when that would leave `clip`.
fn label(painter: &egui::Painter, clip: Rect, c: Pos2, (name, d, at, depth): Tip, t: Tokens) -> Rect {
    let colour = if d[0] != 0.0 {
        t.axis_x
    } else if d[1] != 0.0 {
        t.axis_y
    } else {
        t.axis_z
    };
    let positive = !name.starts_with('-');
    let text = if depth > 0.999 {
        solidcraft_ui_text::format(
            "ui.view_compass_reverse",
            &[("view", solidcraft_ui_text::tr(view_name(name))), ("opposite", solidcraft_ui_text::tr(view_name(opposite(name))))],
        )
    } else {
        solidcraft_ui_text::tr(view_name(name)).to_owned()
    };
    let font = crate::type_face::bold(t.font_compass);
    let galley = painter.layout_no_wrap(text, font, t.on_accent);
    // The resting disc's radius at edge-on size: a far tip grows to it while it is labelled.
    let r = 11.0;
    // The name starts past the letter on a positive tip, near the disc's edge on a negative one.
    let lead = if positive { r + 3.0 } else { 3.0 - r * 0.4 };
    let (w, pad) = (galley.size().x, 9.0);
    let right = Rect::from_min_max(pos2(at.x - r, at.y - r), pos2(at.x + lead + w + pad, at.y + r));
    let left = Rect::from_min_max(pos2(at.x - lead - w - pad, at.y - r), pos2(at.x + r, at.y + r));
    let rightward =
        if at.x >= c.x { right.max.x <= clip.max.x || left.min.x < clip.min.x } else { left.min.x < clip.min.x && right.max.x <= clip.max.x };
    let pill = if rightward { right } else { left };
    painter.rect_filled(pill, r, colour);
    if positive {
        let letter = painter.layout_no_wrap(name.to_uppercase(), crate::type_face::bold(t.font_compass), t.on_accent);
        painter.galley(at - letter.size() / 2.0, letter, t.on_accent);
    }
    let x = if rightward { at.x + lead } else { at.x - lead - w };
    painter.galley(pos2(x, at.y - galley.size().y / 2.0), galley, t.on_accent);
    pill
}

/// The front-most tip within reach of `p`.
pub fn hit(tips: &[Tip], p: Pos2) -> Option<usize> {
    tips.iter().enumerate().rev().find(|(_, (_, _, at, _))| at.distance(p) <= crate::icons::glyphs::axis_radius()).map(|(i, _)| i)
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
    // The floor disc behind the axes: how far above or below the XY plane the view looks.
    painter.add(egui::Shape::convex_polygon(floor(&compass.axes, c), t.text.gamma_multiply(0.06), Stroke::new(1.2, t.border)));
    for (i, (name, d, at, depth)) in tips.iter().enumerate() {
        if i == 3 {
            // The origin dot sits behind the three front axes and over the three back ones.
            painter.circle(c, 3.6, t.text.gamma_multiply(0.9), Stroke::new(1.4, t.panel));
        }
        // Farther tips are drawn smaller; the hovered one is covered by its label.
        crate::icons::glyphs::axis_tip_scaled(&painter, c, name, *d, *at, *depth, false, t, depth_scale(*depth));
        solidcraft_ui_registry::publish_handle(&format!("axis_view:{name}"), *at);
    }
    if let Some(tip) = hovered.and_then(|i| tips.get(i)) {
        let pill = label(&painter, compass.clip, c, *tip, t);
        solidcraft_ui_registry::publish_target("axis_view:label", pill);
    }
    solidcraft_ui_registry::publish_count("axis_view_highlights", f64::from(u8::from(hovered.is_some())));
    if let Some(i) = hovered
        && let Some((name, d, _, _)) = tips.get(i)
    {
        solidcraft_ui_registry::publish_handle(&format!("axis_view_hover:{name}"), hover.unwrap_or(c));
        ui.interact(Rect::from_center_size(hover.unwrap_or(c), vec2(2.0, 2.0)), ui.scope_id().with("axis_view_tip"), Sense::hover())
            .backing_on_hover_text(solidcraft_ui_text::format(
                if compass.snap_hint { "ui.view_compass_tip_snap" } else { "ui.view_compass_tip" },
                &[("axis", &name.to_uppercase())],
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
/// frame's motion is an [`OrbitStep`]. The custom body uses it.
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
        crate::test_support::raw_frame(
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
        let ctx = crate::test_support::context();
        let tips = tips(&FRONT, pos2(200.0, 200.0));
        let x = tips.iter().find(|t| t.0 == "x").map(|t| t.2).unwrap();
        assert_eq!(x, pos2(200.0 + (38.0 * 0.84) as f32, 200.0), "X points right");
        assert_eq!(click(&ctx, x), Some(ViewAction::Snap { axis: "x", direction: [1.0, 0.0, 0.0] }));
        let z = tips.iter().find(|t| t.0 == "z").map(|t| t.2).unwrap();
        assert_eq!(click(&ctx, z), Some(ViewAction::Snap { axis: "z", direction: [0.0, 0.0, 1.0] }));
        assert_eq!(click(&ctx, pos2(163.0, 163.0)), Some(ViewAction::Home));
        assert_eq!(click(&ctx, pos2(20.0, 20.0)), None, "outside the compass");
    }

    /// Tips shrink with depth, more behind than they grow in front, and are edge-on size at 0.
    #[test]
    fn far_tips_are_drawn_smaller_than_near_ones() {
        assert_eq!(depth_scale(0.0), 1.0);
        assert!(depth_scale(1.0) > 1.0 && depth_scale(-1.0) < 0.5);
        assert!(depth_scale(-0.5) < depth_scale(0.0) && depth_scale(0.0) < depth_scale(0.5));
        assert_eq!(depth_scale(f64::NAN), 1.0);
        assert_eq!(depth_scale(9.0), depth_scale(1.0));
        let tips = tips(&FRONT, pos2(200.0, 200.0));
        let near = tips.iter().find(|t| t.0 == "-y").unwrap();
        assert_eq!(hit(&tips, near.2).map(|i| tips[i].0), Some("-y"), "the near end owns its spot");
    }

    /// The floor disc is a circle through the X and Y tips seen from the top, and a flat line
    /// at eye level.
    #[test]
    fn the_floor_disc_is_round_from_the_top_and_flat_at_eye_level() {
        let c = pos2(200.0, 200.0);
        let reach = (38.0 * 0.84) as f32;
        let top = ViewAxes { right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], back: [0.0, 0.0, 1.0] };
        assert!(floor(&top, c).iter().all(|p| (p.distance(c) - reach).abs() < 1e-3));
        let flat = floor(&FRONT, c);
        assert!(flat.iter().all(|p| (p.y - c.y).abs() < 1e-3));
        assert!(flat.iter().any(|p| (p.x - c.x - reach).abs() < 1e-3), "it reaches the X tip");
    }

    /// The hovered tip's label stays inside the area the compass may paint in, growing toward the
    /// centre when the outward side has no room; the opposite tip of an axis is the other end.
    #[test]
    fn the_label_stays_inside_the_clip() {
        assert_eq!((opposite("x"), opposite("-x"), opposite("z"), opposite("-y")), ("-x", "x", "-z", "y"));
        let ctx = crate::test_support::context();
        crate::test_support::raw_frame(
            &ctx,
            RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0))), ..Default::default() },
            |ui| {
                let c = pos2(350.0, 200.0);
                let clip = Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0));
                let x = tips(&FRONT, c).into_iter().find(|t| t.0 == "x").unwrap();
                let pill = label(ui.painter(), clip, c, x, Tokens::get());
                assert!(clip.contains_rect(pill), "{pill:?} in {clip:?}");
                assert!(pill.max.x <= x.2.x + 12.0, "it grew leftward from the right edge");
                let minus_x = tips(&FRONT, pos2(200.0, 200.0)).into_iter().find(|t| t.0 == "-x").unwrap();
                let pill = label(ui.painter(), clip, pos2(200.0, 200.0), minus_x, Tokens::get());
                assert!(pill.max.x <= minus_x.2.x + 12.0, "a left tip grows leftward");
            },
        );
    }

    #[test]
    fn a_drag_on_the_compass_returns_orbit_steps_and_ends_on_release() {
        let ctx = crate::test_support::context();
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

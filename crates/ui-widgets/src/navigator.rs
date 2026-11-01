//! The signed-axis navigation compass: six world-axis tips, a home chip and drag-to-orbit.

use egui::{Color32, CursorIcon, FontFamily, FontId, Painter, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use solidcraft_render::{Camera, Vec3};

/// Colours the compass draws with, supplied by the caller so the widget needs no theme crate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavigatorColors {
    /// The X, Y and Z colours, in that order.
    pub axis: [Color32; 3],
    pub panel: Color32,
    pub border: Color32,
    pub overlay: Color32,
    pub text: Color32,
    pub on_accent: Color32,
}

/// What the pointer asked the caller to do this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NavAction {
    /// Turn the view to look from this world direction.
    LookFrom(Vec3),
    /// Go to the home view.
    Home,
    /// Orbit by this pointer delta, in screen points.
    Orbit { dx: f32, dy: f32 },
}

/// Caller-owned inputs that keep the widget free of application types.
pub struct NavigatorOptions<'a> {
    /// Size of the bold compass letters (the application's `Tokens::font_compass`).
    pub font_size: f32,
    /// Whether free orbit is on; off appends the Shift-to-snap hint to a tip's tooltip.
    pub free_orbit: bool,
    /// The translated Home tooltip.
    pub home_tooltip: &'a str,
    /// Offset of the compass centre inside `rect`; the application lifts it in its layout.
    pub center_offset: egui::Vec2,
    /// Floating panels and other rectangles that own their input over the compass.
    pub occluders: &'a [Rect],
    /// Draws the Home chip's artwork; the application supplies its icon atlas.
    pub paint_home: &'a dyn Fn(&Painter, Rect),
}

/// The compass box's side in logical pixels.
pub const AXES_SIZE: f32 = 116.0;
/// How far the compass sits above its reserved box's centre.
pub const AXIS_CENTER_LIFT: f32 = 4.0;
/// The pointer target around an axis tip; larger than the drawn tip.
pub const AXIS_RADIUS: f32 = 15.0;

const AXIS_LENGTH: f64 = 38.0;
/// The artwork sits inside the pointer reach so the compass keeps breathing room in its circle.
const AXIS_ART_SCALE: f64 = 0.84;
const CIRCLE_RADIUS: f32 = 49.5;
const HOME_OFFSET: f32 = 37.0;
const HOME_SIZE: f32 = 18.0;
const HOME_RADIUS: f32 = 12.0;
const TIP_RADIUS: f32 = 11.0;
const NEGATIVE_TIP_RADIUS: f32 = 4.8;
const SHAFT_WIDTH: f32 = 3.6;
const ORIGIN_DOT_RADIUS: f32 = 3.6;
const ORIGIN_DOT_STROKE: f32 = 1.4;

/// The signed-axis compass.
pub struct Navigator {
    /// The press that started the current drag, if any. The caller owns this between frames.
    pub drag: Option<Pos2>,
    hovered: Option<&'static str>,
    hover: Option<Pos2>,
    home: Rect,
    centre: Pos2,
    drag_started: bool,
    drag_ended: bool,
}

impl Default for Navigator {
    fn default() -> Self {
        Self { drag: None, hovered: None, hover: None, home: Rect::NOTHING, centre: Pos2::ZERO, drag_started: false, drag_ended: false }
    }
}

impl Navigator {
    /// The compass box's side in logical pixels.
    pub const SIZE: f32 = AXES_SIZE;

    /// A fresh compass, carrying the caller's drag from the previous frame.
    pub fn new() -> Self {
        Self::default()
    }

    /// A fresh compass that continues `drag` from the previous frame.
    pub fn with_drag(drag: Option<Pos2>) -> Self {
        Self { drag, ..Self::default() }
    }

    /// A drag started on this frame's press.
    pub fn drag_started(&self) -> bool {
        self.drag_started
    }

    /// The drag that was active was released on this frame.
    pub fn drag_ended(&self) -> bool {
        self.drag_ended
    }

    /// The tip under the pointer this frame, if any.
    pub fn hovered(&self) -> Option<&'static str> {
        self.hovered
    }

    /// Where the hovered tip's scenario handle belongs: the pointer, else the compass centre.
    pub fn hover_at(&self) -> Option<Pos2> {
        self.hovered.map(|_| self.hover.unwrap_or(self.centre))
    }

    /// The Home chip's rectangle this frame.
    pub fn home_rect(&self) -> Rect {
        self.home
    }

    /// Draws the compass and answers what the pointer asked for; the caller animates and applies it.
    ///
    /// `rect` is the compass box. `options.paint_home` must supply the caller's Home artwork.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        cam: &Camera,
        colours: &NavigatorColors,
        options: &NavigatorOptions<'_>,
    ) -> (Option<NavAction>, Vec<(&'static str, Pos2)>) {
        self.hovered = None;
        self.hover = None;
        self.home = Rect::NOTHING;
        self.centre = rect.center();
        self.drag_started = false;
        self.drag_ended = false;
        if !rect.is_positive() || !rect.is_finite() {
            self.drag = None;
            return (None, Vec::new());
        }

        let c = rect.center() + options.center_offset;
        self.centre = c;
        let covered = |p: Pos2| options.occluders.iter().any(|r| r.contains(p));
        let (pressed, down, decided, origin, pos) = ui.input(|i| {
            let p = &i.pointer;
            (p.primary_pressed(), p.primary_down(), p.is_decidedly_dragging(), p.press_origin(), p.interact_pos())
        });
        if pressed && ui.is_enabled() && ui.rect_contains_pointer(rect) && !pos.is_some_and(&covered) {
            self.drag = origin.filter(|p| rect.contains(*p));
            self.drag_started = self.drag.is_some();
        }
        if !down && self.drag.take().is_some() {
            self.drag_ended = true;
        }

        let mut action = None;
        let orbiting = match self.drag.filter(|_| decided) {
            Some(last) => {
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                if let Some(p) = pos {
                    self.drag = Some(p);
                    ui.ctx().request_repaint();
                    action = Some(NavAction::Orbit { dx: p.x - last.x, dy: p.y - last.y });
                }
                true
            }
            None => false,
        };

        let tips = axis_tips(cam, c);
        let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| ui.is_enabled() && ui.rect_contains_pointer(rect) && !covered(*p));
        let hovered = if orbiting { None } else { hover.and_then(|p| axis_hit(&tips, p)) };
        self.hover = hover;
        self.hovered = hovered.and_then(|i| tips.get(i).map(|tip| tip.0));

        let painter = ui.painter_at(rect);
        // Anywhere in the compass circle drags the view; the circle turns fully opaque under the pointer.
        let in_circle = hover.is_some_and(|p| p.distance(c) <= CIRCLE_RADIUS);
        let fill = if in_circle || orbiting { 1.0 } else { 0.35 };
        painter.circle(c, CIRCLE_RADIUS, colours.overlay.gamma_multiply(fill), Stroke::new(1.0, colours.border.gamma_multiply(0.75)));
        for (i, (name, d, at, depth)) in tips.iter().enumerate() {
            if i == 3 {
                // The origin dot sits behind the three front axes and over the three back ones.
                painter.circle(c, ORIGIN_DOT_RADIUS, colours.text.gamma_multiply(0.9), Stroke::new(ORIGIN_DOT_STROKE, colours.panel));
            }
            axis_tip(&painter, c, name, d, *at, *depth, hovered == Some(i), colours, options.font_size);
        }
        let published = tips.iter().map(|(name, _, at, _)| (*name, *at)).collect();

        if let Some(i) = hovered
            && let Some((name, d, _, _)) = tips.get(i)
        {
            ui.interact(Rect::from_center_size(hover.unwrap_or(c), vec2(2.0, 2.0)), ui.id().with("axis_view_tip"), Sense::hover()).on_hover_text(
                format!(
                    "View from {}. Click again to reverse. Drag to orbit{}.",
                    name.to_uppercase(),
                    if options.free_orbit { "" } else { "; hold Shift to snap" }
                ),
            );
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            if ui.input(|i| i.pointer.primary_clicked()) {
                let dir = if cam.back().dot(*d) > 0.9999 { -*d } else { *d };
                action = Some(NavAction::LookFrom(dir));
            }
        }

        let home = Rect::from_center_size(pos2(c.x - HOME_OFFSET, c.y - HOME_OFFSET), vec2(HOME_SIZE, HOME_SIZE));
        self.home = home;
        let hr = ui.interact(home, ui.id().with("axis_view_home"), Sense::click());
        // The home button sits in its own small round chip.
        painter.circle(
            home.center(),
            HOME_RADIUS,
            colours.overlay.gamma_multiply(if hr.hovered() { 0.9 } else { 0.5 }),
            Stroke::new(1.0, colours.border),
        );
        (options.paint_home)(&painter, home);
        // Cursors: a grabbing hand while orbiting, an open hand over the circle, a pointer on the home
        // chip (the axis ends keep their pointing hand).
        if hr.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        } else if orbiting {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if in_circle && hovered.is_none() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
        if hr.on_hover_text(options.home_tooltip).clicked() {
            action = Some(NavAction::Home);
        }

        (action, published)
    }
}

/// Six signed world axes, sorted back to front so overlapping tips have one owner.
pub fn axis_tips(cam: &Camera, c: Pos2) -> Vec<(&'static str, Vec3, Pos2, f64)> {
    let (r, u, b) = cam.basis();
    let mut tips: Vec<_> = [("x", Vec3::X), ("-x", -Vec3::X), ("y", Vec3::Y), ("-y", -Vec3::Y), ("z", Vec3::Z), ("-z", -Vec3::Z)]
        .into_iter()
        .map(|(name, d)| {
            (name, d, c + vec2((d.dot(r) * AXIS_LENGTH * AXIS_ART_SCALE) as f32, -(d.dot(u) * AXIS_LENGTH * AXIS_ART_SCALE) as f32), d.dot(b))
        })
        .collect();
    tips.sort_by(|a, b| a.3.total_cmp(&b.3));
    tips
}

/// The frontmost tip whose pointer target contains `p`.
pub fn axis_hit(tips: &[(&str, Vec3, Pos2, f64)], p: Pos2) -> Option<usize> {
    tips.iter().enumerate().rev().find(|(_, (_, _, at, _))| at.distance(p) <= AXIS_RADIUS).map(|(i, _)| i)
}

/// The application's bold compass face, with egui's proportional face as a safe fallback when a
/// caller has not registered the `ui-bold` family.
fn compass_font(ctx: &egui::Context, font_size: f32) -> FontId {
    let bold = FontFamily::Name("ui-bold".into());
    let family = if ctx.fonts(|fonts| fonts.definitions().families.contains_key(&bold)) { bold } else { FontFamily::Proportional };
    FontId::new(font_size, family)
}

/// Draws one signed axis tip. Positive tips carry a letter; negative tips are hollow.
pub fn axis_tip(painter: &Painter, c: Pos2, name: &str, d: &Vec3, at: Pos2, depth: f64, hovered: bool, colours: &NavigatorColors, font_size: f32) {
    let col = if d.x != 0.0 {
        colours.axis[0]
    } else if d.y != 0.0 {
        colours.axis[1]
    } else {
        colours.axis[2]
    };
    let positive = !name.starts_with('-');
    let light = colours.panel.r() > 128;
    // Positive shafts are the full axis colour; negative ones are a dimmed tint (and any end
    // pointing away from the viewer dims further).
    let ink = if !positive || depth < -0.01 { col.gamma_multiply(0.55) } else { col };
    let radius = if positive { TIP_RADIUS } else { NEGATIVE_TIP_RADIUS };
    let direction = (at - c).normalized();
    // A negative shaft stops at the outer edge of its ring; a positive one runs under its disc.
    let end = if positive { at - direction * radius } else { at - direction * (radius + 1.2) };
    painter.line_segment([c, end], Stroke::new(SHAFT_WIDTH, ink));
    // The round cap sits at the origin; the other end is covered by the disc or ring.
    painter.circle_filled(c, SHAFT_WIDTH / 2.0, ink);
    if positive {
        painter.circle_filled(at, radius, col);
    }
    // Negative tips are hollow: neither the axis shaft nor a fill covers their centres, so the
    // translucent compass background shows through. A hovered tip of either kind turns fully white.
    if hovered {
        // Same outer size as the resting tip: a ring's outline is half its stroke outside.
        painter.circle_filled(at, if positive { radius } else { radius + 1.2 }, colours.on_accent);
        // On the light theme a white disc vanishes into the compass, so it carries an outline in
        // the axis colour, drawn inside the disc so it does not grow.
        if light && positive {
            painter.circle_stroke(at, radius - 1.0, Stroke::new(2.0, col));
        }
    }
    if positive {
        // On a hovered (white) disc the letter takes the axis colour.
        let letter = if hovered { col } else { colours.on_accent };
        let label = name.to_uppercase();
        let galley = painter.layout_no_wrap(label, compass_font(painter.ctx(), font_size), letter);
        let origin = at - galley.size() / 2.0;
        let ppp = painter.ctx().pixels_per_point().max(0.5);
        let origin = pos2((origin.x * ppp).round() / ppp, (origin.y * ppp).round() / ppp);
        painter.galley(origin, galley, letter);
    }
    if !positive {
        painter.circle_stroke(
            at,
            radius,
            Stroke::new(
                2.4,
                if !hovered {
                    ink
                } else if light {
                    col
                } else {
                    colours.on_accent
                },
            ),
        );
    }
}

/// At a halfway tie, prefer the square heading nearer the standard plan view.
pub fn nearest_quarter_turn(yaw: f64) -> f64 {
    if !yaw.is_finite() {
        return 0.0;
    }
    let q = std::f64::consts::FRAC_PI_2;
    let lo = (yaw / q).floor() * q;
    let hi = lo + q;
    let (dlo, dhi) = (yaw - lo, hi - yaw);
    if (dlo - dhi).abs() < 1e-6 {
        let off_front = |a: f64| ((a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI).abs();
        if off_front(lo) <= off_front(hi) { lo } else { hi }
    } else if dlo < dhi {
        lo
    } else {
        hi
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, FontDefinitions, RawInput, Shape};

    fn test_context() -> egui::Context {
        let ctx = egui::Context::default();
        let mut fonts = FontDefinitions::default();
        let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
        fonts.families.insert(FontFamily::Name("ui-bold".into()), fallback);
        ctx.set_fonts(fonts);
        ctx
    }

    fn input() -> RawInput {
        RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0))), ..Default::default() }
    }

    fn colours(dark: bool) -> NavigatorColors {
        if dark {
            NavigatorColors {
                axis: [Color32::from_rgb(230, 90, 90), Color32::from_rgb(120, 220, 120), Color32::from_rgb(100, 160, 240)],
                panel: Color32::from_rgb(40, 44, 52),
                border: Color32::from_rgb(90, 96, 106),
                overlay: Color32::from_rgb(30, 34, 40),
                text: Color32::from_rgb(230, 230, 230),
                on_accent: Color32::WHITE,
            }
        } else {
            NavigatorColors {
                axis: [Color32::from_rgb(170, 40, 40), Color32::from_rgb(40, 130, 40), Color32::from_rgb(30, 80, 170)],
                panel: Color32::from_rgb(245, 245, 245),
                border: Color32::from_rgb(170, 170, 170),
                overlay: Color32::from_rgb(255, 255, 255),
                text: Color32::from_rgb(30, 30, 30),
                on_accent: Color32::WHITE,
            }
        }
    }

    fn opts<'a>(paint_home: &'a dyn Fn(&Painter, Rect), occluders: &'a [Rect]) -> NavigatorOptions<'a> {
        NavigatorOptions { font_size: 13.5, free_orbit: true, home_tooltip: "Home view", center_offset: egui::Vec2::ZERO, occluders, paint_home }
    }

    #[test]
    fn tips_project_six_signed_axes_and_sort_back_to_front() {
        let cam = Camera { yaw: 0.3, pitch: 0.4, ..Camera::default() };
        let c = pos2(100.0, 100.0);
        let tips = axis_tips(&cam, c);
        assert_eq!(tips.len(), 6);
        for name in ["x", "-x", "y", "-y", "z", "-z"] {
            assert!(tips.iter().any(|tip| tip.0 == name), "{name}");
        }
        assert!(tips.windows(2).all(|pair| pair[0].3 <= pair[1].3), "back to front");
        let (r, u, b) = cam.basis();
        for (name, d, at, depth) in &tips {
            let expected = c + vec2((d.dot(r) * AXIS_LENGTH * AXIS_ART_SCALE) as f32, -(d.dot(u) * AXIS_LENGTH * AXIS_ART_SCALE) as f32);
            assert_eq!(*at, expected, "{name}");
            assert_eq!(*depth, d.dot(b), "{name}");
        }
    }

    #[test]
    fn hit_test_takes_the_frontmost_overlapping_tip_and_stops_at_the_radius() {
        let tips = [("back", Vec3::Z, pos2(10.0, 10.0), 0.0), ("front", Vec3::X, pos2(10.0, 10.0), 1.0)];
        assert_eq!(axis_hit(&tips, pos2(10.0, 10.0)), Some(1), "the later (front) tip owns the overlap");
        assert_eq!(axis_hit(&tips, pos2(10.0 + AXIS_RADIUS, 10.0)), Some(1), "the edge of the target still hits");
        assert_eq!(axis_hit(&tips, pos2(10.0 + AXIS_RADIUS + 0.1, 10.0)), None, "outside every target");
        assert_eq!(axis_hit(&[], pos2(10.0, 10.0)), None);
    }

    #[test]
    fn nearest_quarter_turn_uses_the_nearer_square_and_survives_junk() {
        for (from, to) in [
            (0.0, 0.0),
            (30.0, 0.0),
            (-80.0, -90.0),
            (100.0, 90.0),
            (170.0, 180.0),
            (400.0, 360.0),
            (-400.0, -360.0),
            (-45.0, 0.0),
            (45.0, 0.0),
            (135.0, 90.0),
            (-135.0, -90.0),
            (44.99, 0.0),
            (45.01, 90.0),
        ] {
            let got = nearest_quarter_turn(f64::to_radians(from)).to_degrees();
            assert!((got - to).abs() < 1e-9, "{from}° -> {got}°, expected {to}°");
        }
        assert_eq!(nearest_quarter_turn(f64::NAN), 0.0);
        assert_eq!(nearest_quarter_turn(f64::INFINITY), 0.0);
        assert_eq!(nearest_quarter_turn(f64::NEG_INFINITY), 0.0);
    }

    #[test]
    fn show_publishes_six_tips_and_places_the_home_chip() {
        let ctx = test_context();
        let rect = Rect::from_min_size(pos2(100.0, 100.0), vec2(Navigator::SIZE, Navigator::SIZE));
        let cam = Camera::default();
        let colours = colours(true);
        let paint_home = |_: &Painter, _: Rect| {};
        let mut options = opts(&paint_home, &[]);
        options.center_offset = vec2(0.0, -AXIS_CENTER_LIFT);
        let mut navigator = Navigator::new();
        let mut shown = None;
        let _ = ctx.run_ui(input(), |ui| shown = Some(navigator.show(ui, rect, &cam, &colours, &options)));
        let (action, tips) = shown.expect("the compass draws");
        assert!(action.is_none(), "no pointer, no action");
        assert_eq!(tips.len(), 6);
        assert!(tips.iter().all(|(_, at)| rect.contains(*at)));
        let home =
            Rect::from_center_size(pos2(rect.center().x - HOME_OFFSET, rect.center().y - AXIS_CENTER_LIFT - HOME_OFFSET), vec2(HOME_SIZE, HOME_SIZE));
        assert_eq!(navigator.home_rect(), home);
        assert_eq!(Navigator::SIZE, AXES_SIZE);
        assert_eq!(Navigator::SIZE, 116.0);
        assert_eq!(AXIS_CENTER_LIFT, 4.0);
        assert_eq!(AXIS_RADIUS, 15.0);
    }

    #[test]
    fn hover_follows_the_frontmost_tip_and_occluders_take_it_away() {
        let ctx = test_context();
        let rect = Rect::from_min_size(pos2(100.0, 100.0), vec2(Navigator::SIZE, Navigator::SIZE));
        let cam = Camera::default();
        let colours = colours(true);
        let tips = axis_tips(&cam, rect.center());
        let target = *tips.last().expect("six tips");
        let paint_home = |_: &Painter, _: Rect| {};
        let none = [];
        let options = opts(&paint_home, &none);
        let mut navigator = Navigator::new();
        let moved = RawInput { events: vec![Event::PointerMoved(target.2)], ..input() };
        let output = ctx.run_ui(moved, |ui| {
            let _ = navigator.show(ui, rect, &cam, &colours, &options);
        });
        assert_eq!(navigator.hovered(), Some(target.0));
        assert!(navigator.hover_at().is_some_and(|at| at.distance(target.2) < 1e-6));
        assert_eq!(output.platform_output.cursor_icon, CursorIcon::PointingHand);

        let blocker = [Rect::from_center_size(target.2, vec2(4.0, 4.0))];
        let options = opts(&paint_home, &blocker);
        let moved = RawInput { events: vec![Event::PointerMoved(target.2)], ..input() };
        let output = ctx.run_ui(moved, |ui| {
            let _ = navigator.show(ui, rect, &cam, &colours, &options);
        });
        assert_eq!(navigator.hovered(), None, "an occluding panel owns the pointer");
        assert_ne!(output.platform_output.cursor_icon, CursorIcon::PointingHand);
    }

    #[test]
    fn a_caller_without_the_bold_family_still_draws_the_letters() {
        let ctx = egui::Context::default();
        let colours = colours(true);
        let output =
            ctx.run_ui(input(), |ui| axis_tip(ui.painter(), pos2(100.0, 100.0), "x", &Vec3::X, pos2(131.92, 100.0), 0.5, false, &colours, 13.5));
        let letters = output.shapes.iter().filter(|shape| matches!(&shape.shape, Shape::Text(_))).count();
        assert_eq!(letters, 1, "the proportional fallback keeps the letter");
    }

    #[test]
    fn light_theme_outlines_hovered_positive_tips_and_dark_keeps_the_white_disc() {
        let ctx = test_context();
        let c = pos2(100.0, 100.0);
        let at = c + vec2(38.0 * 0.84, 0.0);
        for dark in [true, false] {
            let colours = colours(dark);
            let output = ctx.run_ui(input(), |ui| axis_tip(ui.painter(), c, "x", &Vec3::X, at, 0.5, true, &colours, 13.5));
            let circles: Vec<_> =
                output.shapes.iter().filter_map(|shape| if let Shape::Circle(circle) = &shape.shape { Some(circle) } else { None }).collect();
            assert!(
                circles.iter().any(|circle| circle.center == at && circle.fill == colours.on_accent),
                "the hovered end is filled white: dark={dark}"
            );
            let outlined = circles.iter().any(|circle| circle.center == at && circle.stroke.color == colours.axis[0] && circle.stroke.width >= 2.0);
            assert_eq!(outlined, !dark, "the light theme keeps the white disc visible: dark={dark}");
        }
    }
}

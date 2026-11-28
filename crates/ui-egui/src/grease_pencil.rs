use std::{
    collections::{BTreeMap, BTreeSet},
    ops::RangeInclusive,
    sync::Arc,
};

use egui::{Color32, Event, PointerButton, Pos2, Response, Shape, Stroke, Ui};
use serde_json::{Value, json};
use solidcore_record::{Plane, Vec3};
use solidcraft_engine::schema::sketch_3d::CopiedInk;
use solidcraft_engine::sketch::MAX_FIT_POINTS;
use solidcraft_engine::{
    Sel,
    doc::{IDENTITY, Mat, ModelState},
    sketch::Wire,
};
use solidcraft_kernel::{affine, points};

use crate::ui_kit::BackingTooltipExt;

use crate::{
    SolveApp,
    tools::Kind,
    viewport::{self, Hit, InkPlacement, Proj, ViewTurn},
};

pub(crate) const COMMAND: &str = "sketch.grease_pencil";
pub(crate) const ERASE: &str = "sketch.erase_ink";
pub(crate) const EDIT: &str = "sketch.edit_ink";
pub(crate) const SCALE: &str = "sketch.scale_ink";

/// The occurrence paths of picked strokes, keyed by design session, sketch and wire.
pub(crate) type InkPickPaths = BTreeMap<(u64, u64, String), Vec<u64>>;

pub(crate) fn is_tool(kind: Kind) -> bool {
    matches!(kind, Kind::Freehand | Kind::InkErase)
}

pub(crate) fn editing(app: &SolveApp) -> bool {
    crate::workspace::focused_sketch(app).is_some_and(|sketch| {
        app.ink_sketches.get(&app.session.uid) == Some(&sketch)
            || app.session.doc.sketch(sketch).is_ok_and(|sk| sk.wires.iter().any(|wire| wire.ink_diameter.is_some()))
    })
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Brush {
    erase: Option<Erase>,
    pub(crate) picking_color: bool,
    stroke: Option<Freehand>,
    depth: Option<Depth>,
}

#[derive(Clone, Debug, Default)]
struct Erase {
    strokes: BTreeSet<String>,
    last: Option<Pos2>,
}

#[derive(Clone, Debug)]
enum Depth {
    Point(Vec3),
    Face { point: Vec3, body: String, face: usize, ray: (Vec3, Vec3) },
}

impl Depth {
    fn point(&self) -> Vec3 {
        match self {
            Self::Point(point) | Self::Face { point, .. } => *point,
        }
    }

    /// Mesh depth is enough for hover and preview. Refine B-rep depth once when saving.
    fn resolve(&self, app: &SolveApp) -> Option<Vec3> {
        let Self::Face { point, body, face, ray: (origin, direction) } = self else { return Some(self.point()) };
        let world = crate::open_meshing::shown_world(app);
        let body = &world.body(body)?.body;
        if body.is_mesh() {
            return Some(*point);
        }
        body.planar_face(*face)
            .and_then(|plane| points::line_plane_point(&plane, *origin, *direction))
            .or_else(|| body.fire_ray(*origin, *direction).ok().flatten().map(|hit| hit.point))
            .filter(|&point| crate::view_pick::section_keeps(&viewport::section_planes(app), point))
    }
}

#[derive(Clone, Debug)]
struct Freehand {
    plane: Plane,
    view: Proj,
    points: Vec<Pos2>,
    tip: Pos2,
    spacing: f32,
    diameter: f64,
    color: Option<[u8; 3]>,
}

pub(crate) fn ink_color(rgb: Option<[u8; 3]>) -> Color32 {
    rgb.map_or_else(|| solidcraft_theme::Tokens::view().sketch, |[r, g, b]| Color32::from_rgb(r, g, b))
}

/// Positive widths are screen points. A negative value is projected at unit eye depth.
pub(crate) fn render_width(proj: &Proj, diameter: f64, extra: f32) -> f32 {
    let height = f64::from(proj.rect.height()).max(1.0);
    let width = diameter * height / (2.0 * proj.cam.half_height()) + f64::from(extra);
    if proj.cam.fov > 0.0 { -(width * proj.cam.distance) as f32 } else { width as f32 }
}

fn ink_width_at(proj: &Proj, width: f32, point: Vec3) -> f32 {
    if width < 0.0 { -width / proj.vp.apply(point)[3].max(1e-6) as f32 } else { width }
}

/// The rail covers strokes half a point to 64 points wide at the view target.
pub(crate) fn width_range(app: &SolveApp) -> RangeInclusive<f64> {
    let mut cam = app.cam;
    cam.fov = app.view_fov();
    let height = app.viewport.rect.map_or(600.0, |rect| f64::from(rect.height()).max(1.0));
    let mm_per_point = 2.0 * cam.half_height() / height;
    (mm_per_point * 0.5).clamp(1e-9, 1e9)..=(mm_per_point * 64.0).clamp(1e-9, 1e9)
}

fn width_label(mm: f64) -> String {
    if !(0.001..1e6).contains(&mm) {
        return format!("{mm:.2e} mm");
    }
    format!("{} mm", egui::emath::format_with_decimals_in_range(mm, 0..=4))
}

pub(crate) fn width_slider(ui: &mut Ui, width: &mut f64, range: RangeInclusive<f64>) -> bool {
    ui.label(solidcraft_ui_text::tr("ui.width"));
    let response = crate::ui_kit::slider(ui, width, range, crate::ui_kit::Slider { format: width_label, logarithmic: true, ..Default::default() });
    crate::panel_runtime::register_content_focus(ui.ctx(), &response);
    response.changed()
}

pub(crate) fn color_picker(ui: &mut Ui, shown: Option<[u8; 3]>, picking: &mut bool) -> Option<[u8; 3]> {
    let mut choice = None;
    ui.label(solidcraft_ui_text::tr("Colour"));
    let t = solidcraft_theme::Tokens::get();
    let spacing = t.space_3;
    let side = ((ui.available_width() - spacing * 7.0) / 8.0).clamp(16.0, t.control_height);
    let size = egui::Vec2::splat(side);
    let color = ink_color(shown);
    let mut rgb = [color.r(), color.g(), color.b()];
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = spacing;
        for (name, swatch) in [
            ("sketch.grease_pencil.white", [255, 255, 255]),
            ("sketch.grease_pencil.black", [0, 0, 0]),
            ("color.red", [235, 64, 65]),
            ("sketch.grease_pencil.yellow", [245, 203, 45]),
            ("color.green", [47, 175, 92]),
            ("color.blue", [59, 130, 246]),
        ] {
            let name = solidcraft_ui_text::tr(name);
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
            let selected = rgb == swatch;
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, name));
            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand).backing_on_hover_text(name);
            ui.painter().rect(
                rect.shrink(1.0),
                t.control_radius,
                ink_color(Some(swatch)),
                Stroke::new(if selected { 2.0 } else { 1.0 }, if selected || response.hovered() { t.form_primary } else { t.control_border }),
                egui::StrokeKind::Inside,
            );
            crate::panel_runtime::register_content_focus(ui.ctx(), &response);
            if response.clicked() {
                rgb = swatch;
                choice = Some(swatch);
                *picking = false;
            }
        }

        let (_, rect) = ui.allocate_space(size);
        let tip = solidcraft_ui_text::tr("sketch.grease_pencil.eyedropper");
        let eyedropper = crate::ui_kit::icon_button_at(ui, rect, "render_pick_up", tip);
        crate::panel_runtime::register_content_focus(ui.ctx(), &eyedropper);
        if eyedropper.clicked() {
            *picking = !*picking;
        }
        if *picking {
            ui.painter().rect_stroke(rect, t.control_radius, Stroke::new(1.5, t.form_primary), egui::StrokeKind::Inside);
        }

        let tip = solidcraft_ui_text::tr("sketch.grease_pencil.custom_color");
        let custom = crate::ui_kit::color_wheel_field(ui, size, &mut rgb, tip);
        crate::panel_runtime::register_content_focus(ui.ctx(), &custom);
        if custom.clicked() {
            *picking = false;
        }
        if custom.changed() {
            choice = Some(rgb);
        }
    });
    choice
}

fn sample_style(app: &SolveApp, ui: &Ui, proj: &Proj, inside: bool) -> Option<([u8; 3], Option<f64>)> {
    if inside {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    brush_press(app, ui, proj, inside)?;
    let world = crate::open_meshing::shown_world(app);
    match app.viewport.hover.as_ref()? {
        Hit::SketchCurve { sketch, id, .. } => {
            let wire = ink_wire(&world, *sketch, id)?;
            let (diameter, color) = stroke_edit(app).and_then(|edit| edit.preview(*sketch, wire)).unwrap_or((wire.ink_diameter?, wire.ink_color));
            let color = ink_color(color);
            Some(([color.r(), color.g(), color.b()], Some(diameter)))
        }
        Hit::Face { body, index, .. } => {
            let body = world.body(body)?;
            let color = solidcraft_theme::Tokens::get().body;
            let rgb = app.session.doc.model_face_look(body, *index).map_or([color.r(), color.g(), color.b()], |look| look.color);
            Some((rgb, None))
        }
        _ => None,
    }
}

pub(crate) fn edit_picking(app: &SolveApp) -> bool {
    stroke_edit(app).is_some_and(|edit| edit.picking_color)
}

fn brush_press(app: &SolveApp, ui: &Ui, proj: &Proj, inside: bool) -> Option<Pos2> {
    ui.input(|i| {
        i.events.iter().find_map(|event| match *event {
            Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, .. } if brush_can_start(app, proj, inside, pos) => Some(pos),
            _ => None,
        })
    })
}

fn brush_can_start(app: &SolveApp, proj: &Proj, inside: bool, pos: Pos2) -> bool {
    inside && proj.rect.contains(pos) && !crate::shell::covered(app, pos)
}

/// Each stroke uses the depth chosen before it starts.
pub(crate) fn input(app: &mut SolveApp, ui: &egui::Ui, proj: &Proj, inside: bool, navigating: bool) {
    if edit_picking(app) {
        if !navigating && ui.input(|i| i.focused) {
            let style = sample_style(app, ui, proj, inside);
            if let Some((color, diameter)) = style
                && let Some(crate::dialogs::Kind::EditInk(edit)) = app.dialog.as_mut().map(|d| &mut d.kind)
            {
                edit.color = Some(color);
                edit.diameter = diameter.or(edit.diameter);
                edit.picking_color = false;
            }
        }
        return;
    }
    if app.tool.as_ref().is_some_and(|tool| tool.kind == Kind::InkErase) {
        erase_input(app, ui, proj, inside, navigating);
        return;
    }
    let Some(tool) = app.tool.as_ref().filter(|tool| tool.kind == Kind::Freehand) else { return };
    if navigating || !ui.input(|i| i.focused) {
        if let Some(tool) = app.tool.as_mut() {
            tool.brush.stroke = None;
        }
        return;
    }
    let diameter = app.ink_diameter;
    let color = app.ink_color;
    if tool.brush.picking_color {
        if let Some((color, diameter)) = sample_style(app, ui, proj, inside)
            && let Some(tool) = app.tool.as_mut()
        {
            app.ink_color = Some(color);
            if let Some(diameter) = diameter {
                app.ink_diameter = diameter;
            }
            tool.brush.picking_color = false;
        }
        return;
    }
    let depth = tool.brush.depth.as_ref().map_or(proj.cam.target, Depth::point);
    if inside {
        ui.ctx().set_cursor_icon(egui::CursorIcon::None);
        if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
            let width = ink_width_at(proj, render_width(proj, diameter, 0.0), depth).min(1024.0);
            let t = solidcraft_theme::Tokens::view();
            ui.painter().circle_stroke(pos, width * 0.5, Stroke::new(2.0, t.sketch_halo));
            ui.painter().circle_stroke(pos, width * 0.5, Stroke::new(1.0, ink_color(color)));
        }
    }
    let Some(tool) = app.tool.as_mut() else { return };
    if tool.brush.stroke.as_ref().is_some_and(|s| s.view.cam != proj.cam || s.view.rect != proj.rect) {
        tool.brush.stroke = None;
    }
    let events = ui.input(|i| i.events.clone());
    for event in events {
        let starts = matches!(event, Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, .. }
            if brush_can_start(app, proj, inside, pos));
        let Some(tool) = app.tool.as_mut().filter(|tool| tool.kind == Kind::Freehand) else { break };
        match event {
            Event::PointerButton { pos, .. } if starts => {
                let origin = tool.brush.depth.as_ref().map_or(proj.cam.target, Depth::point);
                let (x, y, _) = proj.cam.basis();
                let plane = Plane { origin, x, y };
                tool.brush.depth.get_or_insert(Depth::Point(origin));
                tool.brush.stroke = Some(Freehand { plane, view: *proj, points: vec![pos], tip: pos, spacing: 4.0, diameter, color });
            }
            Event::PointerMoved(pos) if let Some(stroke) = &mut tool.brush.stroke => stroke.sample(pos, stroke.spacing),
            Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, .. } if let Some(mut stroke) = tool.brush.stroke.take() => {
                let spacing = if stroke.points.len() == 1 { 3.0 } else { 1.0 };
                stroke.sample(pos, spacing);
                if stroke.points.len() >= 2 {
                    commit_stroke(app, stroke);
                }
            }
            Event::PointerGone => tool.brush.stroke = None,
            _ => {}
        }
    }
}

fn commit_stroke(app: &mut SolveApp, mut stroke: Freehand) {
    if let Some(depth) = app.tool.as_mut().and_then(|tool| tool.brush.depth.take()) {
        let Some(origin) = depth.resolve(app) else {
            app.set_status(solidcraft_ui_text::tr("sketch.grease_pencil.depth_lost"), true);
            return;
        };
        stroke.plane.origin = origin;
    }
    if let Some(tool) = app.tool.as_mut() {
        tool.brush.depth = Some(Depth::Point(stroke.plane.origin));
    }
    if let Some(params) = stroke.finish() {
        match app.run(COMMAND, params) {
            Ok(_) => app.set_tool_hint(crate::tools::hint(COMMAND)),
            Err(error) => app.set_status(error, true),
        }
    }
}

pub(crate) fn pending_erase(app: &SolveApp) -> Option<(u64, &BTreeSet<String>)> {
    let tool = app.tool.as_ref().filter(|tool| tool.kind == Kind::InkErase)?;
    Some((crate::sketch_view::active_sketch(app)?, &tool.brush.erase.as_ref()?.strokes))
}

fn erase_input(app: &mut SolveApp, ui: &Ui, proj: &Proj, inside: bool, navigating: bool) {
    let Some(sketch) = crate::workspace::focused_sketch(app) else { return };
    let (focused, down) = ui.input(|i| (i.focused, i.pointer.primary_down()));
    if navigating || !focused {
        if let Some(tool) = app.tool.as_mut() {
            tool.brush.erase = None;
        }
        return;
    }
    if inside {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    let events = ui.input(|i| i.events.clone());
    for event in events {
        match event {
            Event::PointerButton { pos, button: PointerButton::Primary, pressed, .. } => {
                if pressed {
                    let start = brush_can_start(app, proj, inside, pos);
                    if let Some(tool) = app.tool.as_mut() {
                        tool.brush.erase = start.then(Erase::default);
                    }
                }
                erase_to(app, ui, proj, sketch, pos);
                if !pressed {
                    let erased = app.tool.as_mut().and_then(|tool| tool.brush.erase.take());
                    if let Some(erase) = erased.filter(|erase| !erase.strokes.is_empty()) {
                        let items: Vec<_> = erase.strokes.into_iter().map(|id| Sel::ExternalSketchCurve { sketch, id }).collect();
                        crate::delete::request(app, json!({"items": items}));
                        return;
                    }
                }
            }
            Event::PointerMoved(pos) => erase_to(app, ui, proj, sketch, pos),
            // Cancel when the original pointer disappears.
            Event::PointerGone => {
                if let Some(tool) = app.tool.as_mut() {
                    tool.brush.erase = None;
                }
            }
            _ => {}
        }
    }
    if !down && let Some(tool) = app.tool.as_mut() {
        tool.brush.erase = None;
    }
}

fn erase_to(app: &mut SolveApp, ui: &Ui, proj: &Proj, sketch: u64, pos: Pos2) {
    let visible = proj.rect.contains(pos) && !crate::shell::covered(app, pos);
    let Some(erase) = app.tool.as_mut().and_then(|tool| tool.brush.erase.as_mut()) else { return };
    let last = std::mem::replace(&mut erase.last, visible.then_some(pos));
    if !visible || last == Some(pos) {
        return;
    }
    let from = last.unwrap_or(pos);
    let Some(contacts) = erase_contacts(app, proj, sketch, from, pos) else { return };
    let strokes = contacts
        .into_iter()
        .filter(|&point| !crate::shell::covered(app, point))
        .filter_map(|point| match depth_at(app, proj, point, ui.ctx(), Some(sketch))? {
            Hit::SketchCurve { id, .. } => Some(id),
            _ => None,
        })
        .collect::<Vec<_>>();
    if !strokes.is_empty()
        && let Some(erase) = app.tool.as_mut().and_then(|tool| tool.brush.erase.as_mut())
    {
        erase.strokes.extend(strokes);
        ui.ctx().request_repaint();
    }
}

fn erase_contacts(app: &SolveApp, proj: &Proj, sketch: u64, from: Pos2, to: Pos2) -> Option<Vec<Pos2>> {
    let world = crate::open_meshing::shown_world(app);
    let mut cache = app.viewport.ink_pick.lock().ok()?;
    cache.update(app, &world, proj);
    let path_bounds = egui::Rect::from_two_pos(from, to).expand(BRUSH_PICK_REACH);
    let pending = pending_erase(app).map(|(_, strokes)| strokes);
    let mut contacts = Vec::new();
    for stroke in &cache.strokes {
        if stroke.sketch != sketch || !stroke.bounds.intersects(path_bounds) || pending.is_some_and(|ids| ids.contains(&stroke.wire)) {
            continue;
        }
        contacts.extend(stroke.segments.iter().filter_map(|&[a, b]| sweep_contact(from, to, a, b)));
    }
    contacts.sort_unstable_by(|a, b| a.distance_sq(from).total_cmp(&b.distance_sq(from)));
    contacts.dedup();
    Some(contacts)
}

impl Freehand {
    fn finish(self) -> Option<serde_json::Value> {
        let points: Vec<_> = self
            .points
            .into_iter()
            .filter_map(|pos| {
                let (origin, direction) = self.view.ray(pos);
                points::line_plane_point(&self.plane, origin, direction)
            })
            .collect();
        // Keep the visible shape while avoiding redundant spline spans.
        let points_per_mm = ink_width_at(&self.view, render_width(&self.view, 1.0, 0.0), self.plane.origin);
        let tolerance = 0.75 / f64::from(points_per_mm.max(1e-6));
        let points = points::simplify_polyline(&points, tolerance, 1e-9);
        (points.len() >= 2).then(|| {
            json!({
                "points": points.into_iter().map(|p| [p.x, p.y, p.z]).collect::<Vec<_>>(),
                "diameter": self.diameter, "color": self.color,
            })
        })
    }

    fn sample(&mut self, pos: Pos2, spacing: f32) {
        if !self.view.rect.contains(pos) {
            return;
        }
        self.tip = pos;
        if self.points.last().is_none_or(|last| (pos - *last).length_sq() < spacing * spacing) {
            return;
        }
        if self.points.len() == MAX_FIT_POINTS {
            // Keep both the early and late parts of a very long stroke.
            let mut i = 0;
            self.points.retain(|_| {
                i += 1;
                i % 2 == 1
            });
            self.spacing *= 2.0;
        }
        self.points.push(pos);
    }
}

impl Brush {
    pub(crate) fn paint(&self, painter: &egui::Painter) {
        let Some(stroke) = &self.stroke else { return };
        let color = ink_color(stroke.color);
        let width = ink_width_at(&stroke.view, render_width(&stroke.view, stroke.diameter, 0.0), stroke.plane.origin).min(1024.0);
        let mut screen = stroke.points.clone();
        if screen.last() != Some(&stroke.tip) {
            screen.push(stroke.tip);
        }
        for &end in screen.first().into_iter().chain(screen.last()) {
            painter.circle_filled(end, width * 0.5, color);
        }
        if screen.len() >= 2 {
            painter.add(Shape::line(screen, Stroke::new(width, color)));
        }
    }
}

/// Only a hit updates the depth. Moving into empty space clears the highlight.
pub(crate) fn depth_hover(app: &mut SolveApp, proj: &Proj, pos: Option<Pos2>, ctx: &egui::Context) -> Option<Hit> {
    let sample = pos.filter(|_| app.tool.as_ref().is_none_or(|tool| tool.brush.stroke.is_none()));
    let erase = app.tool.as_ref().filter(|tool| tool.kind == Kind::InkErase).and_then(|_| crate::workspace::focused_sketch(app));
    let pos = sample?;
    let hit = depth_at(app, proj, pos, ctx, erase)?;
    if let Some(tool) = app.tool.as_mut().filter(|tool| tool.kind == Kind::Freehand) {
        tool.brush.depth = match &hit {
            Hit::Face { body, index, point } => Some(Depth::Face { point: *point, body: body.clone(), face: *index, ray: proj.ray(pos) }),
            Hit::SketchCurve { point, .. } => Some(Depth::Point(*point)),
            _ => return None,
        };
    }
    Some(hit)
}

fn depth_at(app: &SolveApp, proj: &Proj, pos: Pos2, ctx: &egui::Context, erase: Option<u64>) -> Option<Hit> {
    let world = crate::open_meshing::shown_world(app);
    let section = viewport::section_planes(app);
    let face = crate::view_pick::brush_face(app, &world, proj, pos, &section, ctx)?;
    let view = proj.cam.view();
    let face_depth = match &face {
        Some(Hit::Face { point, .. }) => Some(view.apply(*point)[2]),
        _ => None,
    };
    if let Some(hit) = nearby_ink(app, &world, proj, pos, BRUSH_PICK_REACH, erase, |point| {
        crate::view_pick::section_keeps(&section, point) && face_depth.is_none_or(|depth| depth - view.apply(point)[2] <= 1e-6)
    }) {
        return Some(hit);
    }
    face.filter(|_| erase.is_none())
}

const BRUSH_PICK_REACH: f32 = 2.0;

#[derive(Default)]
pub(crate) struct InkPickCache {
    key: u64,
    world: std::sync::Weak<ModelState>,
    strokes: Vec<ScreenStroke>,
}

struct ScreenStroke {
    sketch: u64,
    wire: String,
    straight: bool,
    placement: Arc<InkPlacement>,
    bounds: egui::Rect,
    segments: Vec<[ScreenPoint; 2]>,
}

#[derive(Clone, Copy)]
struct ScreenPoint {
    world: Vec3,
    screen: Pos2,
    radius: f32,
}

impl ScreenStroke {
    fn hit(&self, point: Vec3) -> Hit {
        Hit::SketchCurve { sketch: self.sketch, id: self.wire.clone(), straight: self.straight, point, ink_frame: Some(self.placement.clone()) }
    }
}

fn screen_segment_parameter(point: Pos2, a: Pos2, b: Pos2) -> f32 {
    let xy = |p: Pos2| (f64::from(p.x), f64::from(p.y));
    solidcraft_viewport::nearest_on_screen_segment(xy(point), xy(a), xy(b)) as f32
}

/// A point of the pointer path whose pick band intersects the projected ink segment.
fn sweep_contact(from: Pos2, to: Pos2, a: ScreenPoint, b: ScreenPoint) -> Option<Pos2> {
    let bounds = egui::Rect::from_two_pos(a.screen, b.screen).expand(a.radius.max(b.radius) + BRUSH_PICK_REACH);
    if !bounds.intersects(egui::Rect::from_two_pos(from, to)) {
        return None;
    }
    let xy = |p: Pos2| [f64::from(p.x), f64::from(p.y)];
    let radii = [f64::from(a.radius + BRUSH_PICK_REACH), f64::from(b.radius + BRUSH_PICK_REACH)];
    let [x, y] = points::segment_band_contact([xy(from), xy(to)], [xy(a.screen), xy(b.screen)], radii)?;
    Some(Pos2::new(x as f32, y as f32))
}

impl InkPickCache {
    fn update(&mut self, app: &SolveApp, world: &std::sync::Arc<ModelState>, proj: &Proj) {
        use std::hash::{Hash, Hasher};
        let session = &app.session;
        let mut key = std::collections::hash_map::DefaultHasher::new();
        crate::sketch_display::projection_key(proj, 1.0).hash(&mut key);
        session.active_component.hash(&mut key);
        session.active_occurrence_path.hash(&mut key);
        session.active_sketch.hash(&mut key);
        session.visibility.hidden_sketches.hash(&mut key);
        session.visibility.shown_sketches.hash(&mut key);
        session.visibility.hidden_folders.hash(&mut key);
        session.visibility.hidden_occurrences.hash(&mut key);
        let selected: BTreeSet<_> = viewport::selected_ink_sketches(app, world).collect();
        selected.hash(&mut key);
        let edit = stroke_edit(app);
        if let Some(edit) = edit.filter(|edit| edit.diameter.is_some()) {
            edit.strokes.hash(&mut key);
            edit.diameter.map(f64::to_bits).hash(&mut key);
        }
        let key = key.finish();
        if self.key == key && self.world.as_ptr() == std::sync::Arc::as_ptr(world) {
            return;
        }
        self.key = key;
        self.world = std::sync::Arc::downgrade(world);
        self.strokes.clear();
        for ss in &world.sketches {
            if !ss.sketch.wires.iter().any(|wire| wire.ink_diameter.is_some()) {
                continue;
            }
            let sketch = ss.feature;
            for placement in viewport::ink_placements(app, world, sketch, selected.contains(&sketch)) {
                let placement = Arc::new(placement);
                for wire in &ss.sketch.wires {
                    let Some(diameter) = wire.ink_diameter else { continue };
                    let diameter = edit.and_then(|edit| edit.preview(sketch, wire)).map_or(diameter, |(width, _)| width);
                    let width = render_width(proj, diameter, 0.0);
                    let at = |&world: &Vec3| {
                        let world = Vec3::from(affine::apply_point(&placement.frame, world.into()));
                        let screen = proj.to_screen(world)?;
                        Some(ScreenPoint { world, screen, radius: ink_width_at(proj, width, world).min(1024.0) * 0.5 })
                    };
                    let mut bounds = egui::Rect::NOTHING;
                    let mut previous = None;
                    let mut segments = Vec::new();
                    for point in wire.pts.iter().map(at) {
                        if let Some((a, b)) = std::mem::replace(&mut previous, point).zip(point) {
                            bounds = bounds.union(egui::Rect::from_two_pos(a.screen, b.screen).expand(a.radius.max(b.radius)));
                            segments.push([a, b]);
                        }
                    }
                    if !segments.is_empty() {
                        self.strokes.push(ScreenStroke {
                            sketch,
                            wire: wire.id.clone(),
                            straight: wire.fit.len() == 2,
                            placement: placement.clone(),
                            bounds,
                            segments,
                        });
                    }
                }
            }
        }
    }
}

/// The visible stroke nearest the pointer. Intersecting ink takes the frontmost depth;
/// a stroke just outside its width is only eligible within the pick tolerance.
pub(crate) fn nearby_ink(
    app: &SolveApp,
    world: &std::sync::Arc<ModelState>,
    proj: &Proj,
    pos: Pos2,
    tolerance: f32,
    only: Option<u64>,
    visible: impl Fn(Vec3) -> bool,
) -> Option<Hit> {
    let mut hit = None;
    scan_nearby_ink(app, world, proj, InkProbe { pos, tolerance, only }, false, visible, |_, ink| hit = Some(ink));
    hit
}

/// Every displayed stroke near the pointer for Select Other, nearest first.
pub(crate) fn nearby_inks(
    app: &SolveApp,
    world: &Arc<ModelState>,
    proj: &Proj,
    pos: Pos2,
    tolerance: f32,
    visible: impl Fn(Vec3) -> bool,
) -> Vec<Hit> {
    let mut hits = Vec::new();
    scan_nearby_ink(app, world, proj, InkProbe { pos, tolerance, only: None }, true, visible, |rank, hit| hits.push((rank, hit)));
    hits.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    hits.into_iter().map(|(_, hit)| hit).collect()
}

struct InkProbe {
    pos: Pos2,
    tolerance: f32,
    only: Option<u64>,
}

fn scan_nearby_ink(
    app: &SolveApp,
    world: &Arc<ModelState>,
    proj: &Proj,
    probe: InkProbe,
    all: bool,
    visible: impl Fn(Vec3) -> bool,
    mut offer: impl FnMut((f32, f64), Hit),
) {
    let InkProbe { pos, tolerance, only } = probe;
    let Ok(mut cache) = app.viewport.ink_pick.lock() else { return };
    cache.update(app, world, proj);
    let view = proj.cam.view();
    let mut nearest = None;
    let mut nearest_rank = (tolerance, f64::INFINITY);
    let erased = pending_erase(app);
    for stroke in &cache.strokes {
        if only.is_some_and(|sketch| stroke.sketch != sketch)
            || erased.is_some_and(|(sketch, ids)| sketch == stroke.sketch && ids.contains(&stroke.wire))
            || !stroke.bounds.expand(nearest_rank.0).contains(pos)
        {
            continue;
        }
        let mut closest = None;
        let mut rank = nearest_rank;
        for &[a, b] in &stroke.segments {
            let line = b.screen - a.screen;
            let t = screen_segment_parameter(pos, a.screen, b.screen);
            let screen = a.screen + line * t;
            let radius = egui::lerp(a.radius..=b.radius, t);
            let gap = ((pos - screen).length() - radius).max(0.0);
            if gap > rank.0 {
                continue;
            }
            let (o, d) = proj.ray(screen);
            let point = viewport::ray_segment_closest(o, d, a.world, b.world);
            let next = (gap, -view.apply(point)[2]);
            let [_, _, z, w] = proj.vp.apply(point);
            if next < rank && (0.0..=w).contains(&z) && visible(point) {
                closest = Some(point);
                rank = next;
            }
        }
        if let Some(point) = closest {
            if all {
                offer(rank, stroke.hit(point));
            } else {
                nearest = Some((stroke, point));
                nearest_rank = rank;
            }
        }
    }
    if let Some((stroke, point)) = nearest {
        offer(nearest_rank, stroke.hit(point));
    }
}

pub(crate) fn selected_strokes(app: &SolveApp) -> Option<Vec<Value>> {
    let world = crate::open_meshing::shown_world(app);
    Some(stroke_selection(app, &world)?.into_iter().map(|(sketch, wire)| json!({"sketch": sketch, "wire": wire.id})).collect())
}

/// Whether the whole selection can be edited or scaled as strokes.
pub(crate) fn has_stroke_selection(app: &SolveApp) -> bool {
    let selection = &app.session.selection;
    !selection.is_empty() && {
        let world = crate::open_meshing::shown_world(app);
        selection.iter().all(|sel| selection_wires(&world, sel).next().is_some())
    }
}

/// Copy takes directly selected strokes. A Browser sketch still copies as a whole sketch.
pub(crate) fn can_copy_strokes(app: &SolveApp) -> bool {
    app.session.selection.iter().all(|sel| matches!(sel, Sel::ExternalSketchCurve { .. })) && has_stroke_selection(app)
}

/// The owning sketch of a directly picked stroke.
pub(crate) fn stroke_sketch(app: &SolveApp, sel: &Sel) -> Option<u64> {
    let Sel::ExternalSketchCurve { sketch, id } = sel else { return None };
    ink_wire(&crate::open_meshing::shown_world(app), *sketch, id).map(|_| *sketch)
}

fn stroke_selection<'a>(app: &SolveApp, world: &'a ModelState) -> Option<Vec<(u64, &'a Wire)>> {
    let mut strokes = Vec::new();
    let mut seen = BTreeSet::new();
    for sel in &app.session.selection {
        let mut wires = selection_wires(world, sel).peekable();
        wires.peek()?;
        strokes.extend(wires.filter(|(sketch, wire)| seen.insert((*sketch, wire.id.as_str()))));
    }
    (!strokes.is_empty()).then_some(strokes)
}

fn selection_wires<'a>(world: &'a ModelState, sel: &Sel) -> impl Iterator<Item = (u64, &'a Wire)> {
    let (sketch, wires) = match sel {
        Sel::Feature { id } => (*id, world.sketch(*id).map_or(&[][..], |ss| ss.sketch.wires.as_slice())),
        Sel::ExternalSketchCurve { sketch, id } => (*sketch, ink_wire(world, *sketch, id).map_or(&[][..], std::slice::from_ref)),
        _ => (0, &[][..]),
    };
    wires.iter().filter(|wire| wire.ink_diameter.is_some()).map(move |wire| (sketch, wire))
}

/// A stroke under the pointer is part of the selection when its sketch's row is picked.
pub(crate) fn selected_by_sketch(app: &SolveApp, sel: &Sel) -> bool {
    matches!(sel, Sel::ExternalSketchCurve { sketch, id }
        if app.session.selection.contains(&Sel::Feature { id: *sketch })
            && ink_wire(&crate::open_meshing::shown_world(app), *sketch, id).is_some())
}

pub(crate) fn forget_picks(app: &mut SolveApp) {
    if app.viewport.ink_selected_paths.is_empty() {
        return;
    }
    let selected: BTreeSet<_> = app
        .session
        .selection
        .iter()
        .filter_map(|sel| match sel {
            Sel::ExternalSketchCurve { sketch, id } => Some((*sketch, id.as_str())),
            _ => None,
        })
        .collect();
    let other_sessions: BTreeSet<_> = app.docs.background().map(|session| session.uid).collect();
    app.viewport.ink_selected_paths.retain(|(uid, sketch, id), _| {
        if *uid == app.session.uid { selected.contains(&(*sketch, id.as_str())) } else { other_sessions.contains(uid) }
    });
}

/// Record the selected occurrence, so Copy still uses it after the pointer leaves.
pub(crate) fn remember_pick(app: &mut SolveApp, hit: &Hit) {
    let Hit::SketchCurve { sketch, id, ink_frame: Some(placement), .. } = hit else { return };
    remember_ink_placement(app, *sketch, id, placement.path.clone());
}

pub(crate) fn remember_ink_placement(app: &mut SolveApp, sketch: u64, id: &str, path: Vec<u64>) {
    if app.session.selection.iter().any(|sel| matches!(sel, Sel::ExternalSketchCurve { sketch: owner, id: wire } if *owner == sketch && wire == id)) {
        app.viewport.ink_selected_paths.insert((app.session.uid, sketch, id.to_owned()), path);
    }
}

fn copy_frame(app: &SolveApp, world: &ModelState, sketch: u64, id: &str) -> Mat {
    if let Some(path) = app.viewport.ink_selected_paths.get(&(app.session.uid, sketch, id.to_owned())) {
        if app.session.doc.feature(sketch).is_some_and(|feature| app.session.feature_path(feature.component) == *path) {
            return IDENTITY;
        }
        if let Some(copy) = world.sketch_copies.get(&sketch).and_then(|copies| copies.iter().find(|copy| copy.path == *path)) {
            return copy.delta;
        }
    }
    viewport::sketch_frames(app, world, sketch, solidcraft_engine::view::sketch_shown(&app.session, sketch)).first().copied().unwrap_or(IDENTITY)
}

pub(crate) fn copy_strokes(app: &SolveApp) -> Option<Vec<CopiedInk>> {
    app.session.selection.first()?;
    if !app.session.selection.iter().all(|sel| matches!(sel, Sel::ExternalSketchCurve { .. })) {
        return None;
    }
    let world = crate::open_meshing::shown_world(app);
    app.session
        .selection
        .iter()
        .map(|sel| {
            let (sketch, wire) = selected_ink(&world, sel)?;
            let frame = copy_frame(app, &world, sketch, &wire.id);
            Some(CopiedInk {
                points: wire.fit.iter().map(|&point| affine::apply_point(&frame, point.into())).collect(),
                diameter: wire.ink_diameter?,
                color: wire.ink_color,
            })
        })
        .collect()
}

pub(crate) fn paste_strokes(app: &mut SolveApp, at: Option<Pos2>, in_place: bool) -> bool {
    let mut strokes = app.copied_ink.clone();
    if !in_place {
        let Some(transform) = paste_transform(app, at) else { return false };
        for point in strokes.iter_mut().flat_map(|stroke| &mut stroke.points) {
            *point = affine::apply_point(&transform, *point);
        }
    }
    app.run("sketch.paste_ink", json!({"strokes": strokes})).is_ok()
}

fn paste_transform(app: &SolveApp, at: Option<Pos2>) -> Option<Mat> {
    let center = points::point_bounds_center(app.copied_ink.iter().flat_map(|stroke| stroke.points.iter().copied()))?;
    let (x, y, _) = app.cam.basis();
    let half_point = *width_range(app).start();
    let target = at
        .zip(app.viewport.rect)
        .filter(|(pointer, rect)| rect.contains(*pointer) && !crate::shell::covered(app, *pointer))
        .and_then(|(pointer, rect)| {
            let (origin, direction) = viewport::projection(app, rect).ray(pointer);
            points::line_plane_point(&Plane { origin: center.into(), x, y }, origin, direction)
        })
        .filter(|target| points::point_distance(center, (*target).into()).is_ok_and(|distance| distance.value > half_point));
    if let Some(target) = target {
        return affine::translation_between(center, target.into()).ok();
    }
    let offset = 32.0 * half_point;
    affine::translation(affine::parallelogram_point([0.0; 3], x.into(), y.into(), offset, -offset)).ok()
}

pub(crate) fn stroke_edit(app: &SolveApp) -> Option<&EditStroke> {
    let crate::dialogs::Kind::EditInk(edit) = &app.dialog.as_ref()?.kind else { return None };
    Some(edit)
}

#[derive(Clone, Debug)]
pub struct EditStroke {
    strokes: BTreeMap<u64, BTreeSet<String>>,
    initial_width: f64,
    width_range: RangeInclusive<f64>,
    initial_color: Option<[u8; 3]>,
    diameter: Option<f64>,
    color: Option<[u8; 3]>,
    picking_color: bool,
}

impl EditStroke {
    pub(crate) fn selected(app: &SolveApp) -> Option<Self> {
        let world = crate::open_meshing::shown_world(app);
        let selection = stroke_selection(app, &world)?;
        let (_, first) = selection.first().copied()?;
        let mut strokes = BTreeMap::<_, BTreeSet<_>>::new();
        let initial_width = first.ink_diameter?;
        let (mut min, mut max) = (initial_width, initial_width);
        for (sketch, wire) in selection {
            let diameter = wire.ink_diameter?;
            min = min.min(diameter);
            max = max.max(diameter);
            strokes.entry(sketch).or_default().insert(wire.id.clone());
        }
        Some(Self {
            strokes,
            initial_width,
            width_range: min..=max,
            initial_color: first.ink_color,
            diameter: None,
            color: None,
            picking_color: false,
        })
    }

    pub(crate) fn show(&mut self, app: &SolveApp, ui: &mut Ui) {
        let mut width = self.diameter.unwrap_or(self.initial_width);
        let range = width_range(app);
        let range = range.start().min(*self.width_range.start())..=range.end().max(*self.width_range.end());
        if width_slider(ui, &mut width, range) {
            self.diameter = Some(width);
        }
        if let Some(color) = color_picker(ui, self.color.or(self.initial_color), &mut self.picking_color) {
            self.color = Some(color);
        }
    }

    pub(crate) fn preview(&self, sketch: u64, wire: &Wire) -> Option<(f64, Option<[u8; 3]>)> {
        self.strokes.get(&sketch)?.contains(&wire.id).then_some(())?;
        Some((self.diameter.or(wire.ink_diameter)?, self.color.or(wire.ink_color)))
    }

    pub(crate) fn hash_preview(&self, key: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        self.strokes.hash(key);
        self.diameter.map(f64::to_bits).hash(key);
        self.color.hash(key);
    }

    pub(crate) fn params(&self) -> Option<Value> {
        if self.diameter.is_none() && self.color.is_none() {
            return None;
        }
        let strokes: Vec<_> = self.strokes.iter().flat_map(|(sketch, ids)| ids.iter().map(move |id| json!({"sketch": sketch, "wire": id}))).collect();
        Some(json!({"strokes": strokes, "diameter": self.diameter, "color": self.color}))
    }
}

fn selected_ink<'a>(world: &'a ModelState, sel: &Sel) -> Option<(u64, &'a Wire)> {
    let Sel::ExternalSketchCurve { sketch, id } = sel else { return None };
    Some((*sketch, ink_wire(world, *sketch, id)?))
}

pub(crate) fn ink_wire<'a>(world: &'a ModelState, sketch: u64, id: &str) -> Option<&'a Wire> {
    let sk = &world.sketch(sketch)?.sketch;
    sk.wires.get(sk.wire_index(id)?).filter(|wire| wire.ink_diameter.is_some())
}

pub(crate) struct Move {
    pub(crate) strokes: BTreeMap<u64, BTreeSet<String>>,
    pub(crate) transform: Mat,
    plane: Plane,
    turn: Option<ViewTurn>,
    revision: u64,
}

fn plan_move(app: &SolveApp, proj: &Proj, pos: Pos2, rotating: bool) -> Option<(Move, Option<(Sel, Hit)>)> {
    let (hit @ Hit::SketchCurve { point, ink_frame: Some(_), .. }, sel @ Sel::ExternalSketchCurve { .. }) =
        viewport::candidate(app, &viewport::pick(app, proj, pos))?
    else {
        return None;
    };
    let Sel::ExternalSketchCurve { sketch, id } = &sel else { return None };
    let replace = rotating && !app.session.selection.contains(&sel) && !selected_by_sketch(app, &sel);
    let selection = if replace { std::slice::from_ref(&sel) } else { app.session.selection.as_slice() };
    let world = crate::open_meshing::shown_world(app);
    let wires = || selection.iter().flat_map(|sel| selection_wires(&world, sel));
    let mut strokes = BTreeMap::<_, BTreeSet<_>>::new();
    for (sketch, wire) in wires() {
        strokes.entry(sketch).or_default().insert(wire.id.clone());
    }
    if !strokes.get(sketch).is_some_and(|ids| ids.contains(id)) {
        return None;
    }
    let turn = if rotating {
        let center = points::point_bounds_center(wires().flat_map(|(_, wire)| wire.pts.iter().copied().map(Into::into)))?;
        Some(ViewTurn::new(proj, center.into(), pos)?)
    } else {
        None
    };
    let (x, y, _) = proj.cam.basis();
    let mut plane = Plane { origin: point, x, y };
    let (origin, direction) = proj.ray(pos);
    plane.origin = points::line_plane_point(&plane, origin, direction)?;
    let drag = Move { strokes, plane, turn, revision: app.session.revision, transform: IDENTITY };
    Some((drag, replace.then_some((sel, hit))))
}

pub(crate) fn can_rotate_at(app: &SolveApp, proj: &Proj, pos: Pos2) -> bool {
    plan_move(app, proj, pos, true).is_some()
}

/// Preview the selected strokes together and commit one edit on release.
pub(crate) fn move_input(app: &mut SolveApp, ui: &Ui, proj: &Proj, response: &Response, rotating: bool, available: bool) -> bool {
    let (focused, primary, cancelled) = ui.input(|i| (i.focused, i.pointer.primary_down(), i.key_pressed(egui::Key::Escape)));
    if !available || !focused || cancelled {
        return app.viewport.ink_move.take().is_some();
    }
    if response.drag_started_by(PointerButton::Primary)
        && (rotating || app.session.selection.iter().any(|s| matches!(s, Sel::ExternalSketchCurve { .. } | Sel::Feature { .. })))
        && let Some(pos) = ui.input(|i| i.pointer.press_origin()).filter(|p| proj.rect.contains(*p) && !crate::shell::covered(app, *p))
        && let Some((mut drag, selection)) = plan_move(app, proj, pos, rotating)
    {
        if let Some((sel, hit)) = selection {
            viewport::select(app, Some(sel.clone()), false, false);
            if !app.session.selection.contains(&sel) {
                return false;
            }
            remember_pick(app, &hit);
            drag.revision = app.session.revision;
        }
        app.viewport.ink_move = Some(drag);
    }
    let Some(drag) = &mut app.viewport.ink_move else { return false };
    if drag.revision != app.session.revision {
        app.viewport.ink_move = None;
        return true;
    }
    if let Some(p) = ui.input(|i| i.pointer.interact_pos()).filter(|p| proj.rect.contains(*p)) {
        if let Some(turn) = drag.turn {
            if let Some(matrix) = turn.rotation_at(p).and_then(|angle| turn.matrix(angle)) {
                drag.transform = matrix;
            }
        } else {
            let (o, d) = proj.ray(p);
            if let Some(point) = points::line_plane_point(&drag.plane, o, d)
                && let Ok(matrix) = affine::translation_between(drag.plane.origin.into(), point.into())
            {
                drag.transform = matrix;
            }
        }
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    let released = response.drag_stopped_by(PointerButton::Primary);
    if (released || !primary)
        && let Some(drag) = app.viewport.ink_move.take()
        && released
        && drag.transform != IDENTITY
    {
        let strokes: Vec<_> =
            drag.strokes.into_iter().flat_map(|(sketch, ids)| ids.into_iter().map(move |wire| json!({"sketch": sketch, "wire": wire}))).collect();
        let params = json!({"strokes": strokes, "matrix": crate::dialogs_move::matrix_rows(&drag.transform)});
        if let Err(error) = app.run("sketch.move_ink", params) {
            app.set_status(error, true);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    fn projection(h: &mut Harness, target: [f64; 3]) -> Proj {
        h.step(&json!({"call":"ui.view","params":{"view":"top"}})).unwrap();
        h.step(&json!({"call":"ui.camera","params":{"target":target,"distance":120}})).unwrap();
        viewport::projection(&h.app, h.app.viewport.rect.unwrap())
    }

    fn ink(h: &mut Harness) -> (u64, String) {
        let id = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
        let stroke = h.app.run(COMMAND, json!({"points":[[0,0,0],[10,0,0]],"diameter":1,"color":[12,34,56]})).unwrap()["curves"][0]
            .as_str()
            .unwrap()
            .to_owned();
        (id, stroke)
    }

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
    }

    fn brush_frame(h: &mut Harness, proj: &Proj, events: Vec<Event>) {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput { screen_rect: Some(proj.rect), events, ..Default::default() }, |ui| {
            input(&mut h.app, ui, proj, true, false);
        });
    }

    #[test]
    fn paste_in_place_uses_the_copied_occurrence_after_the_pointer_leaves() {
        let mut h = Harness::new();
        let occurrence = h.app.run("component.create", json!({"name":"Ink Part"})).unwrap()["occurrence"].as_u64().unwrap();
        let (sketch, id) = ink(&mut h);
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("component.activate", json!({"component":0})).unwrap();
        h.app.run("occurrence.copy", json!({"occurrence":occurrence,"translate":[100,0,0]})).unwrap();
        let proj = projection(&mut h, [105.0, 0.0, 0.0]);
        let pos = proj.to_screen(Vec3::new(105.0, 0.0, 0.0)).unwrap();
        h.frame_with(vec![Event::PointerMoved(pos), button(pos, true)]);
        h.frame_with(vec![button(pos, false)]);
        let source = Sel::ExternalSketchCurve { sketch, id };
        assert_eq!(h.app.session.selection.as_slice(), std::slice::from_ref(&source));
        h.frame_with(vec![Event::PointerGone]);
        h.app.copied_ink = copy_strokes(&h.app).unwrap();
        h.app.run("select.clear", json!({})).unwrap();
        h.app.run("select.set", json!({"items":[source]})).unwrap();
        assert_eq!(copy_strokes(&h.app).unwrap()[0].points, [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]);
        assert!(paste_strokes(&mut h.app, None, true));
        let [Sel::ExternalSketchCurve { sketch: pasted, id }] = h.app.session.selection.as_slice() else { panic!("pasted stroke must be selected") };
        let world = crate::open_meshing::shown_world(&h.app);
        let pasted = ink_wire(&world, *pasted, id).unwrap();
        assert_eq!(pasted.fit, [Vec3::new(100.0, 0.0, 0.0), Vec3::new(110.0, 0.0, 0.0)]);
        assert_eq!(pasted.ink_color, Some([12, 34, 56]));
    }

    #[test]
    fn eyedropper_hits_the_previewed_width_outside_the_saved_stroke() {
        let mut h = Harness::new();
        let (sketch, id) = ink(&mut h);
        h.app.run("sketch.finish", json!({})).unwrap();
        let proj = projection(&mut h, [5.0, 0.0, 0.0]);
        let pos = proj.to_screen(Vec3::new(5.0, 4.0, 0.0)).unwrap();
        assert!(depth_at(&h.app, &proj, pos, &h.ctx, None).is_none());
        h.app.session.selection = vec![Sel::ExternalSketchCurve { sketch, id: id.clone() }];
        h.app.start(EDIT);
        let Some(crate::dialogs::Kind::EditInk(edit)) = h.app.dialog.as_mut().map(|dialog| &mut dialog.kind) else {
            panic!("Edit Stroke should open");
        };
        edit.diameter = Some(20.0);
        edit.picking_color = true;
        assert!(matches!(depth_at(&h.app, &proj, pos, &h.ctx, None), Some(Hit::SketchCurve { id: hit, .. }) if hit == id));
    }

    #[test]
    fn eraser_catches_a_thin_stroke_between_pointer_events() {
        let mut h = Harness::new();
        let (sketch, id) = ink(&mut h);
        let proj = projection(&mut h, [5.0, 0.0, 0.0]);
        h.app.start(ERASE);
        let from = proj.to_screen(Vec3::new(5.0, -12.0, 0.0)).unwrap();
        let to = proj.to_screen(Vec3::new(5.0, 12.0, 0.0)).unwrap();
        assert!(depth_at(&h.app, &proj, from, &h.ctx, Some(sketch)).is_none());
        assert!(depth_at(&h.app, &proj, to, &h.ctx, Some(sketch)).is_none());
        h.frame_with(vec![Event::PointerMoved(from), button(from, true)]);
        h.frame_with(vec![Event::PointerMoved(to)]);
        assert!(pending_erase(&h.app).is_some_and(|(_, ids)| ids.contains(&id)));
        assert!(h.app.session.doc.sketch(sketch).is_ok(), "the sweep is a preview until release");
        h.frame_with(vec![button(to, false)]);
        assert!(h.app.session.doc.sketch(sketch).is_err(), "the last stroke and empty sketch were removed");
    }

    #[test]
    fn covered_click_does_not_consume_a_later_brush_press_in_the_same_frame() {
        let mut h = Harness::new();
        let sketch = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
        let proj = projection(&mut h, [0.0, 0.0, 0.0]);
        h.app.start(COMMAND);
        let covered = proj.to_screen(Vec3::new(0.0, 20.0, 0.0)).unwrap();
        let from = proj.to_screen(Vec3::new(-10.0, 0.0, 0.0)).unwrap();
        let to = proj.to_screen(Vec3::new(10.0, 0.0, 0.0)).unwrap();
        h.app.shell.cards.push(egui::Rect::from_center_size(covered, egui::Vec2::splat(10.0)));
        let events = vec![button(covered, true), button(covered, false), button(from, true), Event::PointerMoved(to), button(to, false)];
        brush_frame(&mut h, &proj, events);
        let wires = &h.app.session.doc.sketch(sketch).unwrap().wires;
        assert_eq!(wires.len(), 1);
        let end = wires[0].fit[1];
        assert!((end.x - 10.0).abs() < 1e-4 && end.y.abs() < 1e-4, "{end:?}");
    }

    #[test]
    fn brush_saves_drags_after_an_uncovered_empty_click_in_the_same_frame() {
        let mut h = Harness::new();
        let sketch = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
        let proj = projection(&mut h, [0.0, 0.0, 0.0]);
        h.app.start(COMMAND);
        let screen = |x, y| proj.to_screen(Vec3::new(x, y, 0.0)).unwrap();
        let empty = screen(0.0, 10.0);
        let mut events = vec![button(empty, true), button(empty, false)];
        for y in [0.0, -10.0] {
            let from = screen(-10.0, y);
            let to = screen(10.0, y);
            events.extend([button(from, true), Event::PointerMoved(to), button(to, false)]);
        }
        brush_frame(&mut h, &proj, events);
        let wires = &h.app.session.doc.sketch(sketch).unwrap().wires;
        assert_eq!(wires.len(), 2);
        for (wire, y) in wires.iter().zip([0.0, -10.0]) {
            let end = wire.fit[1];
            assert!((end.x - 10.0).abs() < 1e-4 && (end.y - y).abs() < 1e-4, "{end:?}");
        }
    }
}

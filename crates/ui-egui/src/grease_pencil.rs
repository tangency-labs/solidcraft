use std::{
    collections::{BTreeMap, BTreeSet},
    ops::RangeInclusive,
};

use egui::{Color32, Event, PointerButton, Pos2, Response, Shape, Stroke, Ui};
use serde_json::{Value, json};
use solidcore_record::{Plane, Vec3};
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::schema::sketch_3d::CopiedInk;
use solidcraft_engine::sketch::MAX_FIT_POINTS;
use solidcraft_engine::{
    Sel,
    doc::{Mat, ModelState},
    sketch::Wire,
};

use crate::ui_kit::BackingTooltipExt;

use crate::{
    SolveApp,
    tools::Kind,
    viewport::{self, Hit, Proj, ViewTurn},
};

pub(crate) const COMMAND: &str = "sketch.grease_pencil";
pub(crate) const ERASE: &str = "sketch.erase_ink";
pub(crate) const EDIT: &str = "sketch.edit_ink";
pub(crate) const SCALE: &str = "sketch.scale_ink";

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
    erase: Option<BTreeSet<String>>,
    pub(crate) picking_color: bool,
    stroke: Option<Freehand>,
    depth: Option<Depth>,
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
            .and_then(|plane| plane.intersect_ray(*origin, *direction))
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
            Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, .. }
                if inside && proj.rect.contains(pos) && !crate::shell::covered(app, pos) =>
            {
                Some(pos)
            }
            _ => None,
        })
    })
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
    let mut start = brush_press(app, ui, proj, inside).map(|pos| {
        let (x, y, _) = proj.cam.basis();
        let plane = Plane { origin: depth, x, y };
        Freehand { plane, view: *proj, points: vec![pos], tip: pos, spacing: 4.0, diameter, color }
    });
    let Some(tool) = app.tool.as_mut() else { return };
    if tool.brush.stroke.as_ref().is_some_and(|s| s.view.cam != proj.cam || s.view.rect != proj.rect) {
        tool.brush.stroke = None;
    }
    let finished = ui.input(|i| {
        for event in &i.events {
            match *event {
                Event::PointerButton { button: PointerButton::Primary, pressed: true, .. } => {
                    if let Some(stroke) = &start {
                        tool.brush.depth.get_or_insert(Depth::Point(stroke.plane.origin));
                    }
                    tool.brush.stroke = start.take();
                }
                Event::PointerMoved(pos) if let Some(stroke) = &mut tool.brush.stroke => stroke.sample(pos, stroke.spacing),
                Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, .. }
                    if let Some(mut stroke) = tool.brush.stroke.take() =>
                {
                    let spacing = if stroke.points.len() == 1 { 3.0 } else { 1.0 };
                    stroke.sample(pos, spacing);
                    return Some(stroke);
                }
                Event::PointerGone => tool.brush.stroke = None,
                _ => {}
            }
        }
        None
    });
    if let Some(mut stroke) = finished.filter(|stroke| stroke.points.len() >= 2) {
        if let Some(depth) = tool.brush.depth.take() {
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
}

pub(crate) fn pending_erase(app: &SolveApp) -> Option<(u64, &BTreeSet<String>)> {
    let tool = app.tool.as_ref().filter(|tool| tool.kind == Kind::InkErase)?;
    Some((crate::sketch_view::active_sketch(app)?, tool.brush.erase.as_ref()?))
}

fn erase_input(app: &mut SolveApp, ui: &Ui, proj: &Proj, inside: bool, navigating: bool) {
    let Some(sketch) = crate::workspace::focused_sketch(app) else { return };
    let focused = ui.input(|i| i.focused);
    let press = brush_press(app, ui, proj, inside);
    let hit = app.viewport.hover.as_ref().and_then(|hit| match hit {
        Hit::SketchCurve { sketch: owner, id, .. } if inside && *owner == sketch => Some(id.clone()),
        _ => None,
    });
    let Some(tool) = app.tool.as_mut() else { return };
    if navigating || !focused {
        tool.brush.erase = None;
        return;
    }
    if inside {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    let (pressed, released, down) =
        ui.input(|i| (i.pointer.button_pressed(PointerButton::Primary), i.pointer.button_released(PointerButton::Primary), i.pointer.primary_down()));
    if pressed {
        tool.brush.erase = press.map(|_| BTreeSet::new());
    }
    let Some(strokes) = &mut tool.brush.erase else { return };
    if let Some(id) = hit
        && strokes.insert(id)
    {
        ui.ctx().request_repaint();
    }
    if released || !down {
        let strokes = tool.brush.erase.take().unwrap_or_default();
        if released && !strokes.is_empty() {
            let items: Vec<_> = strokes.into_iter().map(|id| Sel::ExternalSketchCurve { sketch, id }).collect();
            crate::delete::request(app, json!({"items": items}));
        }
    }
}

impl Freehand {
    fn finish(self) -> Option<serde_json::Value> {
        let points: Vec<_> = self
            .points
            .into_iter()
            .filter_map(|pos| {
                let (origin, direction) = self.view.ray(pos);
                self.plane.intersect_ray(origin, direction)
            })
            .collect();
        // Keep the visible shape while avoiding redundant spline spans.
        let points_per_mm = ink_width_at(&self.view, render_width(&self.view, 1.0, 0.0), self.plane.origin);
        let tolerance = 0.75 / f64::from(points_per_mm.max(1e-6));
        let mut points = crate::edge_detail::simplify(&points, tolerance);
        points.dedup_by(|a, b| a.dist(*b) < 1e-9);
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
    let target = sample.and_then(|pos| depth_at(app, proj, pos, ctx, erase));
    app.viewport.ink_hover_frame = target.as_ref().and_then(|(_, frame)| *frame);
    let ((hit, _), pos) = target.zip(sample)?;
    if let Some(tool) = app.tool.as_mut().filter(|tool| tool.kind == Kind::Freehand) {
        tool.brush.depth = match &hit {
            Hit::Face { body, index, point } => Some(Depth::Face { point: *point, body: body.clone(), face: *index, ray: proj.ray(pos) }),
            Hit::SketchCurve { point, .. } => Some(Depth::Point(*point)),
            _ => return None,
        };
    }
    Some(hit)
}

fn depth_at(app: &SolveApp, proj: &Proj, pos: Pos2, ctx: &egui::Context, erase: Option<u64>) -> Option<(Hit, Option<Mat>)> {
    let world = crate::open_meshing::shown_world(app);
    let section = viewport::section_planes(app);
    let face = crate::view_pick::brush_face(app, &world, proj, pos, &section, ctx)?;
    let view = proj.cam.view();
    let face_depth = match &face {
        Some(Hit::Face { point, .. }) => Some(view.apply(*point)[2]),
        _ => None,
    };
    if let Some((hit, frame)) = nearby_ink(app, &world, proj, pos, 2.0, erase, |point| {
        crate::view_pick::section_keeps(&section, point) && face_depth.is_none_or(|depth| depth - view.apply(point)[2] <= 1e-6)
    }) {
        return Some((hit, Some(frame)));
    }
    face.filter(|_| erase.is_none()).map(|hit| (hit, None))
}

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
    frame: Mat,
    bounds: egui::Rect,
    segments: Vec<[ScreenPoint; 2]>,
}

#[derive(Clone, Copy)]
struct ScreenPoint {
    world: Vec3,
    screen: Pos2,
    radius: f32,
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
            let frames = viewport::sketch_frames(app, world, sketch, solidcraft_engine::view::sketch_shown(session, sketch));
            for &frame in viewport::ink_frames(&frames, selected.contains(&sketch)) {
                for wire in &ss.sketch.wires {
                    let Some(diameter) = wire.ink_diameter else { continue };
                    let width = render_width(proj, diameter, 0.0);
                    let at = |&world| {
                        let world = solidcraft_engine::doc::apply_point(&frame, world);
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
                        self.strokes.push(ScreenStroke { sketch, wire: wire.id.clone(), straight: wire.fit.len() == 2, frame, bounds, segments });
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
) -> Option<(Hit, Mat)> {
    let mut cache = app.viewport.ink_pick.lock().ok()?;
    cache.update(app, world, proj);
    let view = proj.cam.view();
    let xy = |p: Pos2| (f64::from(p.x), f64::from(p.y));
    let pointer = xy(pos);
    let mut best = None;
    let mut rank = (tolerance, f64::INFINITY);
    let erased = pending_erase(app);
    for stroke in &cache.strokes {
        if only.is_some_and(|sketch| stroke.sketch != sketch)
            || erased.is_some_and(|(sketch, ids)| sketch == stroke.sketch && ids.contains(&stroke.wire))
            || !stroke.bounds.expand(rank.0).contains(pos)
        {
            continue;
        }
        for &[a, b] in &stroke.segments {
            let line = b.screen - a.screen;
            let t = solidcraft_viewport::nearest_on_screen_segment(pointer, xy(a.screen), xy(b.screen)) as f32;
            let screen = a.screen + line * t;
            let radius = egui::lerp(a.radius..=b.radius, t);
            let gap = ((pos - screen).length() - radius).max(0.0);
            if gap > rank.0 {
                continue;
            }
            let (o, d) = proj.ray(screen);
            let point = viewport::ray_segment_closest(o, d, a.world, b.world);
            let next = (gap, -view.apply(point)[2]);
            if next < rank && visible(point) {
                best = Some((stroke, point));
                rank = next;
            }
        }
    }
    best.map(|(stroke, point)| (Hit::SketchCurve { sketch: stroke.sketch, id: stroke.wire.clone(), straight: stroke.straight, point }, stroke.frame))
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
            let (_, wire) = selected_ink(&world, sel)?;
            Some(CopiedInk { points: wire.fit.iter().map(|p| [p.x, p.y, p.z]).collect(), diameter: wire.ink_diameter?, color: wire.ink_color })
        })
        .collect()
}

pub(crate) fn paste_strokes(app: &mut SolveApp, at: Option<Pos2>, in_place: bool) -> bool {
    let by = if in_place { Vec3::ZERO } else { paste_offset(app, at) };
    let strokes: Vec<_> = app
        .copied_ink
        .iter()
        .cloned()
        .map(|mut stroke| {
            for point in &mut stroke.points {
                point[0] += by.x;
                point[1] += by.y;
                point[2] += by.z;
            }
            stroke
        })
        .collect();
    app.run("sketch.paste_ink", json!({"strokes": strokes})).is_ok()
}

fn paste_offset(app: &SolveApp, at: Option<Pos2>) -> Vec3 {
    let center = app
        .copied_ink
        .iter()
        .flat_map(|stroke| stroke.points.iter().map(|&[x, y, z]| Vec3::new(x, y, z)))
        .collect::<solidcraft_engine::geom::Aabb3>()
        .center();
    let (x, y, _) = app.cam.basis();
    let half_point = *width_range(app).start();
    at.zip(app.viewport.rect)
        .and_then(|(point, rect)| {
            if !rect.contains(point) || crate::shell::covered(app, point) {
                return None;
            }
            let proj = viewport::projection(app, rect);
            let (origin, direction) = proj.ray(point);
            let plane = Plane { origin: center, x, y };
            let delta = plane.intersect_ray(origin, direction)? - center;
            (delta.len2() > half_point * half_point).then_some(delta)
        })
        .unwrap_or((x - y) * (32.0 * half_point))
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

pub(crate) fn stroke_at(app: &SolveApp, proj: &Proj, pos: Pos2) -> Option<(Vec3, Sel)> {
    let (Hit::SketchCurve { point, .. }, sel @ Sel::ExternalSketchCurve { .. }) = viewport::candidate(app, &viewport::pick(app, proj, pos))? else {
        return None;
    };
    selected_ink(&crate::open_meshing::shown_world(app), &sel)?;
    Some((point, sel))
}

/// Preview the selected strokes together and commit one edit on release.
pub(crate) fn move_input(app: &mut SolveApp, ui: &Ui, proj: &Proj, response: &Response, rotating: bool, available: bool) -> bool {
    use solidcraft_engine::{
        doc::{IDENTITY, rigid},
        geom::Aabb3,
    };
    let (focused, primary, cancelled) = ui.input(|i| (i.focused, i.pointer.primary_down(), i.key_pressed(egui::Key::Escape)));
    if !available || !focused || cancelled {
        return app.viewport.ink_move.take().is_some();
    }
    if response.drag_started_by(PointerButton::Primary)
        && (rotating || app.session.selection.iter().any(|s| matches!(s, Sel::ExternalSketchCurve { .. } | Sel::Feature { .. })))
        && let Some(pos) = ui.input(|i| i.pointer.press_origin()).filter(|p| proj.rect.contains(*p) && !crate::shell::covered(app, *p))
        && let Some((point, sel)) = stroke_at(app, proj, pos)
        && let Sel::ExternalSketchCurve { sketch, id } = &sel
    {
        if rotating && !app.session.selection.contains(&sel) && !selected_by_sketch(app, &sel) {
            viewport::select(app, Some(sel.clone()), false, false);
        }
        let world = crate::open_meshing::shown_world(app);
        let mut strokes = BTreeMap::<_, BTreeSet<_>>::new();
        let mut bounds = Aabb3::EMPTY;
        for (sketch, wire) in app.session.selection.iter().flat_map(|sel| selection_wires(&world, sel)) {
            if strokes.entry(sketch).or_default().insert(wire.id.clone()) && rotating {
                for &point in &wire.pts {
                    bounds.add(point);
                }
            }
        }
        let (x, y, _) = proj.cam.basis();
        let mut plane = Plane { origin: point, x, y };
        let (o, d) = proj.ray(pos);
        let turn = (rotating && !bounds.is_empty()).then(|| ViewTurn::new(proj, bounds.center(), pos)).flatten();
        if strokes.get(sketch).is_some_and(|ids| ids.contains(id))
            && (!rotating || turn.is_some())
            && let Some(anchor) = plane.intersect_ray(o, d)
        {
            plane.origin = anchor;
            app.viewport.ink_move = Some(Move { strokes, plane, turn, revision: app.session.revision, transform: IDENTITY });
        }
    }
    let Some(drag) = &mut app.viewport.ink_move else { return false };
    if drag.revision != app.session.revision {
        app.viewport.ink_move = None;
        return true;
    }
    if let Some(p) = ui.input(|i| i.pointer.interact_pos()).filter(|p| proj.rect.contains(*p)) {
        if let Some(turn) = drag.turn {
            if let Some(angle) = turn.rotation_at(p) {
                drag.transform = turn.matrix(angle);
            }
        } else {
            let (o, d) = proj.ray(p);
            if let Some(point) = drag.plane.intersect_ray(o, d) {
                drag.transform = rigid(point - drag.plane.origin, Vec3::ZERO, Vec3::Z, 0.0);
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

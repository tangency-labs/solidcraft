//! Small deterministic material previews; no textures, jobs or graphics device.
use crate::{scenario, theme::Tokens};
use egui::{Color32, Mesh, Rect, Response, Sense, Ui, pos2, vec2};
use std::{collections::VecDeque, sync::Arc};

const CACHE_ENTRIES: usize = 64;
const MAX_SIDE: u16 = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinishKind {
    Matte,
    Satin,
    Gloss,
    Metal,
    Glass,
}

/// Colour is document data. All surrounding chrome and checker colours come from Tokens.
#[derive(Clone, Copy, Debug)]
pub struct Sphere {
    pub color: Color32,
    pub kind: FinishKind,
    pub opacity: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    color: Color32,
    kind: FinishKind,
    opacity: u8,
    side: u16,
}

impl Sphere {
    fn key(self, side: f32) -> Key {
        let opacity = self.opacity.filter(|v| v.is_finite()).unwrap_or(if self.kind == FinishKind::Glass { 0.35 } else { 1.0 });
        Key {
            color: self.color,
            kind: self.kind,
            opacity: (opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
            side: if side.is_finite() { side.round().clamp(1.0, f32::from(MAX_SIDE)) as u16 } else { 1 },
        }
    }
}

#[derive(Clone, Default)]
struct SphereCache {
    entries: VecDeque<(Key, Arc<Mesh>)>,
    palette: Option<(Color32, Color32)>,
}

impl SphereCache {
    fn mesh(&mut self, key: Key, t: &Tokens) -> Arc<Mesh> {
        let palette = (t.on_accent, t.tab_on_fill_dark);
        if self.palette != Some(palette) {
            self.entries.clear();
            self.palette = Some(palette);
        }
        if let Some((_, mesh)) = self.entries.iter().find(|(stored, _)| *stored == key) {
            return mesh.clone();
        }
        let mesh = Arc::new(shade(key, t));
        if self.entries.len() >= CACHE_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back((key, mesh.clone()));
        mesh
    }
}

fn shade(key: Key, t: &Tokens) -> Mesh {
    // Fixed topology bounds both work and memory independently of input size.
    const RINGS: u32 = 12;
    const SECTORS: u32 = 48;
    let mut mesh = Mesh::default();
    let radius = f32::from(key.side) * 0.5;
    for ring in 0..=RINGS {
        let r = ring as f32 / RINGS as f32;
        for sector in 0..=SECTORS {
            let angle = std::f32::consts::TAU * sector as f32 / SECTORS as f32;
            let (x, y) = (r * angle.cos(), r * angle.sin());
            let z = (1.0 - r * r).max(0.0).sqrt();
            let light = (-0.4 * x - 0.5 * y + 0.76 * z).max(0.0);
            let specular = (-0.25 * x - 0.35 * y + 0.902 * z).max(0.0);
            let (diffuse, mut shine) = match key.kind {
                FinishKind::Matte => (0.3 + 0.7 * light, 0.04 * specular.powi(8)),
                FinishKind::Satin => (0.25 + 0.75 * light, 0.25 * specular.powi(16)),
                FinishKind::Gloss => (0.2 + 0.8 * light, 0.85 * specular.powi(64)),
                FinishKind::Metal => (0.45 + 0.5 * light, 0.65 * specular.powi(16)),
                FinishKind::Glass => (0.5 + 0.5 * light, (0.85 * specular.powi(64) + 0.4 * (1.0 - z).powi(3)).min(1.0)),
            };
            let mut shaded = key.color.lerp_to_gamma(t.tab_on_fill_dark, 1.0 - diffuse);
            if key.kind == FinishKind::Metal {
                // Original studio sample: broad radial illumination, a curved horizon,
                // a lower reflector and an oval softbox. All are bounded analytic fields.
                let gradient = ((x + 0.28).hypot(y + 0.44) / 1.64).clamp(0.0, 1.0);
                shaded = if gradient < 0.42 {
                    key.color.lerp_to_gamma(t.on_accent, 0.4 * (1.0 - gradient / 0.42))
                } else {
                    key.color.lerp_to_gamma(t.tab_on_fill_dark, 0.66 * (gradient - 0.42) / 0.58)
                };
                shine = 0.0;
            }
            let color = shaded.lerp_to_gamma(t.on_accent, shine).gamma_multiply(f32::from(key.opacity) / 255.0);
            mesh.colored_vertex(pos2(radius + x * radius, radius + y * radius), color);
            if ring < RINGS && sector < SECTORS {
                let a = ring * (SECTORS + 1) + sector;
                let b = a + SECTORS + 1;
                mesh.add_triangle(a, b, b + 1);
                mesh.add_triangle(a, b + 1, a + 1);
            }
        }
    }
    mesh
}

pub(super) fn paint_sphere(ui: &Ui, rect: Rect, sphere: Sphere) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let t = Tokens::get();
    let key = sphere.key(rect.width());
    let mesh = ui.ctx().data_mut(|data| data.get_temp_mut_or_default::<SphereCache>(egui::Id::new("ui_kit:spheres")).mesh(key, &t));
    if sphere.kind == FinishKind::Glass {
        // A bounded four-by-four checker remains visible through the sphere.
        for y in 0..4 {
            for x in 0..4 {
                let cell = Rect::from_min_size(rect.min + vec2(x as f32, y as f32) * rect.size() / 4.0, rect.size() / 4.0);
                ui.painter().rect_filled(cell, 0.0, if (x + y) % 2 == 0 { t.control_fill } else { t.control_border });
            }
        }
    }
    let mut mesh = (*mesh).clone();
    let scale = rect.width() / f32::from(key.side);
    for vertex in &mut mesh.vertices {
        vertex.pos = rect.min + vertex.pos.to_vec2() * scale;
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    if sphere.kind == FinishKind::Metal {
        let radius = rect.width() / 2.0;
        let at = |x: f32, y: f32| rect.center() + vec2(x, y) * radius;
        let opacity = f32::from(key.opacity) / 255.0;
        // Vector reflections stay smooth at small sizes without increasing cached mesh topology.
        ui.painter().add(egui::epaint::QuadraticBezierShape::from_points_stroke(
            [at(-0.965, 0.207), at(0.0, -0.138), at(0.965, 0.207)],
            false,
            Color32::TRANSPARENT,
            egui::Stroke::new(radius * 0.069, t.tab_on_fill_dark.gamma_multiply(0.22 * opacity)),
        ));
        let ellipse = |center: egui::Pos2, radii: egui::Vec2, angle: f32, alpha: f32| {
            let (sin, cos) = angle.sin_cos();
            let points = (0..48)
                .map(|index| {
                    let a = std::f32::consts::TAU * index as f32 / 48.0;
                    let x = radii.x * a.cos();
                    let y = radii.y * a.sin();
                    center + vec2(cos * x - sin * y, sin * x + cos * y)
                })
                .collect();
            ui.painter().add(egui::Shape::convex_polygon(points, t.on_accent.gamma_multiply(alpha * opacity), egui::Stroke::NONE));
        };
        ellipse(at(0.0, 0.48), vec2(0.83, 0.21) * radius, 0.0, 0.16);
        ellipse(at(-0.345, -0.483), vec2(0.38, 0.21) * radius, -32.0_f32.to_radians(), 0.55);
    }
}

/// Clamps size to 1–96 logical points. Cache: 64 FIFO entries per egui context.
/// The caller owns commands and may attach interaction to the returned response.
pub fn sphere_thumbnail(ui: &mut Ui, id: &str, sphere: Sphere, size: f32) -> Response {
    let side = f32::from(sphere.key(size).side);
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
    paint_sphere(ui, rect, sphere);
    scenario::publish_rect(id, rect);
    scenario::publish_handle(id, rect.center());
    response
}

/// Inspector sample card. Content, chips and action reuse the kit's existing
/// controls; dimensions and the bordered card belong here rather than callers.
#[cfg(test)]
pub(crate) fn sphere_preview<R>(ui: &mut Ui, id: &str, sphere: Sphere, name: &str, chips: [&str; 2], action: impl FnOnce(&mut Ui) -> R) -> R {
    sphere_preview_with_source(ui, id, sphere, name, chips[1], |ui| super::text(ui, chips[0], super::Text::Inline), action)
}

/// Sample card with an interactive source tag (for example an inheritance menu).
/// Keeps the standard sphere, two-line caption, usage chip and trailing action.
/// Tokens and dimensions are identical to the plain sample card.
pub(crate) fn sphere_preview_with_source<R>(
    ui: &mut Ui,
    id: &str,
    sphere: Sphere,
    name: &str,
    usage: &str,
    source: impl FnOnce(&mut Ui) -> Response,
    action: impl FnOnce(&mut Ui) -> R,
) -> R {
    let t = Tokens::get();
    super::reports::list_frame_filled(ui, id, t.dialog_footer, t.space_10, |ui| {
        let row = ui.horizontal(|ui| {
            let side = t.control_height * 2.0 - t.space_8;
            sphere_thumbnail(ui, id, sphere, side);
            let text_width = (ui.available_width() - t.control_height - t.space_16).max(0.0);
            ui.allocate_ui_with_layout(vec2(text_width, side), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = t.space_4;
                ui.add_sized(
                    vec2(text_width, t.space_16),
                    egui::Label::new(egui::RichText::new(name).font(crate::fonts::semibold(t.font_label)).color(t.form_ink))
                        .truncate()
                        .halign(egui::Align::Min),
                )
                .on_hover_text(name);
                let (line, _) = ui.allocate_exact_size(vec2(text_width, t.control_height), Sense::hover());
                let usage_width = (ui.painter().layout_no_wrap(usage.to_owned(), egui::FontId::proportional(t.font_caption), t.form_ink).size().x
                    + t.space_12)
                    .min((text_width - t.space_24 - t.space_8).max(0.0));
                let usage_rect = Rect::from_min_max(pos2(line.right() - usage_width, line.top()), line.max);
                let source_rect = Rect::from_min_max(line.min, pos2((usage_rect.left() - t.space_4).max(line.left()), line.bottom()));
                let mut source_ui = ui.new_child(
                    egui::UiBuilder::new().id_salt("sample_source").max_rect(source_rect).layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                source_ui.set_clip_rect(ui.clip_rect().intersect(source_rect));
                let source = source(&mut source_ui);
                scenario::publish_rect(&format!("{id}:chip:0"), source.rect.intersect(source_rect));
                let mut usage_ui = ui.new_child(
                    egui::UiBuilder::new().id_salt("sample_usage").max_rect(usage_rect).layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                usage_ui.set_clip_rect(ui.clip_rect().intersect(usage_rect));
                let response = super::workspace_chip(&mut usage_ui, usage);
                scenario::publish_rect(&format!("{id}:chip:1"), response.rect.intersect(usage_rect));
            });
            let response = ui.vertical(action);
            scenario::publish_target(&format!("{id}:replace"), response.response.rect);
            response.inner
        });
        row.inner
    })
}

/// A catalog sample: a token-sized sphere, two-line caption and optional
/// usage badge. The surrounding chrome follows the active workspace accent.
pub(crate) struct SphereTile<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub sphere: Sphere,
    pub selected: bool,
    pub users: usize,
    pub tip: &'a str,
    pub badge: Option<&'a str>,
}

impl SphereTile<'_> {
    pub(crate) fn show(&self, ui: &mut Ui) -> Response {
        let t = Tokens::get();
        let accent = crate::theme::TabAccent::current(ui.ctx());
        let side = t.control_height + t.space_6;
        let line_height = ui.fonts_mut(|fonts| fonts.row_height(&crate::fonts::semibold(t.font_small)));
        let height = t.space_16 + side + t.space_6 + line_height * 2.0 + t.space_3;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        let response = ui.interact(rect, ui.make_persistent_id(self.id), Sense::click_and_drag());
        let cell = rect;
        ui.painter().rect_filled(cell, t.radius_xl, t.control_fill);
        ui.painter().rect_stroke(cell, t.radius_xl, t.control_border_stroke(t.dialog_rule), egui::StrokeKind::Inside);
        if self.selected || response.hovered() || response.has_focus() {
            ui.painter().rect_stroke(cell, t.radius_xl, egui::Stroke::new(t.space_2, accent.ink), egui::StrokeKind::Inside);
        }
        let preview = Rect::from_min_size(pos2(rect.center().x - side / 2.0, rect.top() + t.space_16), vec2(side, side));
        paint_sphere(ui, preview, self.sphere);
        if let Some(badge) = self.badge {
            ui.painter().text(
                cell.left_top() + vec2(t.space_4, t.space_4),
                egui::Align2::LEFT_TOP,
                badge,
                crate::fonts::semibold(t.font_badge),
                t.form_dim,
            );
        }
        if self.users > 0 {
            let text = if self.users > 99 { "99+".into() } else { self.users.to_string() };
            let galley = ui.painter().layout_no_wrap(text, crate::fonts::semibold(t.font_badge), t.on_accent);
            let width = (galley.size().x + t.space_8).max(t.space_16);
            let badge = Rect::from_min_size(pos2(cell.right() - t.space_4 - width, cell.top() + t.space_2), vec2(width, t.space_16));
            ui.painter().rect_filled(badge, t.radius_lg, accent.ink);
            ui.painter().galley(badge.center() - galley.size() / 2.0, galley, t.on_accent);
        }
        let label = Rect::from_min_max(pos2(rect.left() + t.space_3, preview.bottom() + t.space_6), rect.max - vec2(t.space_3, t.space_3));
        let font = if self.selected { crate::fonts::semibold(t.font_small) } else { egui::FontId::proportional(t.font_small) };
        let mut job = egui::text::LayoutJob::simple(self.name.to_owned(), font, t.form_ink, label.width());
        job.halign = egui::Align::Center;
        job.wrap.max_rows = 2;
        job.wrap.break_anywhere = false;
        let galley = ui.painter().layout_job(job);
        let label_pos = pos2(label.center().x - galley.rect.center().x, label.top() - galley.rect.top());
        scenario::publish_rect(&format!("{}:label", self.id), label);
        scenario::publish_rect(&format!("{}:galley", self.id), galley.rect.translate(label_pos.to_vec2()));
        ui.painter().with_clip_rect(ui.clip_rect().intersect(label)).galley(label_pos, galley, t.form_ink);
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), self.selected, self.name));
        scenario::publish_target(self.id, rect);
        let mut tip = egui::Tooltip::for_enabled(&response);
        tip.popup = tip.popup.frame(super::Container::new(ui.ctx(), super::Surface::Toast).tooltip_frame(ui.ctx()));
        tip.show(|ui| {
            ui.label(self.tip);
        });
        response
    }
}

/// Fixed-column catalog composition. Tile geometry and gaps stay with the kit.
/// Noninteractive cursor sphere, shared by drag-and-drop and paint tools.
pub(crate) fn sphere_cursor(ui: &mut egui::Ui, sphere: Sphere) {
    if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
        let t = Tokens::get();
        let rect = Rect::from_min_size(pointer + vec2(t.space_12, t.space_12), vec2(t.control_height, t.control_height));
        let mut ghost =
            ui.new_child(egui::UiBuilder::new().layer_id(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("sphere_cursor"))).max_rect(rect));
        ghost.set_clip_rect(ui.ctx().content_rect());
        paint_sphere(&ghost, rect, sphere);
    }
}

pub(crate) fn sphere_grid(ui: &mut Ui, tiles: &[SphereTile<'_>]) -> Option<usize> {
    sphere_grid_drag(ui, tiles).0
}

/// Grid click and drag-start events; callers own the drag payload and drop policy.
pub(crate) fn sphere_grid_drag(ui: &mut Ui, tiles: &[SphereTile<'_>]) -> (Option<usize>, Option<usize>) {
    let t = Tokens::get();
    let mut clicked = None;
    let mut dragged = None;
    ui.spacing_mut().item_spacing = vec2(t.space_8, t.space_8);
    for (row, chunk) in tiles.chunks(3).enumerate() {
        ui.columns(3, |columns| {
            for (column, tile) in chunk.iter().enumerate() {
                if let Some(ui) = columns.get_mut(column) {
                    let response = tile.show(ui);
                    if response.clicked() {
                        clicked = Some(row * 3 + column);
                    }
                    if response.drag_started() {
                        dragged = Some(row * 3 + column);
                    }
                }
            }
        });
    }
    (clicked, dragged)
}

#[cfg(test)]
#[path = "sphere_tests.rs"]
mod tests;

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
            let (diffuse, shine) = match key.kind {
                FinishKind::Matte => (0.3 + 0.7 * light, 0.04 * specular.powi(8)),
                FinishKind::Satin => (0.25 + 0.75 * light, 0.25 * specular.powi(16)),
                FinishKind::Gloss => (0.2 + 0.8 * light, 0.85 * specular.powi(64)),
                FinishKind::Metal => (0.15 + 0.65 * light, 0.8 * specular.powi(24)),
                FinishKind::Glass => (0.5 + 0.5 * light, (0.85 * specular.powi(64) + 0.4 * (1.0 - z).powi(3)).min(1.0)),
            };
            let shaded = key.color.lerp_to_gamma(t.tab_on_fill_dark, 1.0 - diffuse);
            let highlight = if key.kind == FinishKind::Metal { key.color.lerp_to_gamma(t.on_accent, 0.65) } else { t.on_accent };
            let color = shaded.lerp_to_gamma(highlight, shine).gamma_multiply(f32::from(key.opacity) / 255.0);
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

#[cfg(test)]
#[path = "sphere_tests.rs"]
mod tests;

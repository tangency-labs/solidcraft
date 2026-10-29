//! Point clouds (#5151): with the GPU viewport their points are dots in the 3D pass (depth-tested
//! with the model); the CPU viewport draws some of them over its image.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use egui::{Painter, Shape};
use solidcraft_engine::doc::point_cloud::PointCloud;

use crate::SolveApp;
use crate::gpu::GpuPoints;
use crate::viewport::Proj;

/// Most points the CPU viewport draws of all clouds together.
const MAX_CPU_POINTS: usize = 100_000;

/// What a cloud's dots are made from: they are made again only when one of these changes.
#[derive(Clone, PartialEq)]
struct Key {
    points: usize,
    colors: usize,
    len: usize,
    origin: [u64; 3],
    transform: [u64; 16],
    size: u64,
    color_by: solidcraft_engine::doc::point_cloud::ColorBy,
    color: Option<[u32; 3]>,
    height_range: Option<[u64; 2]>,
}

impl Key {
    fn of(c: &PointCloud) -> Key {
        Key {
            points: Arc::as_ptr(c.points.values()).cast::<f32>() as usize,
            colors: Arc::as_ptr(&c.colors.0).cast::<u8>() as usize,
            len: c.len(),
            origin: [c.origin.x, c.origin.y, c.origin.z].map(f64::to_bits),
            transform: std::array::from_fn(|i| c.transform[i / 4][i % 4].to_bits()),
            size: c.point_size.to_bits(),
            color_by: c.color_by,
            color: c.color.map(|k| k.map(f32::to_bits)),
            height_range: c.height_range.map(|r| r.map(f64::to_bits)),
        }
    }
}

thread_local! {
    /// Dots by cloud id: what they were made from, their version and bytes.
    static DOTS: RefCell<HashMap<u64, (Key, u64, Arc<Vec<u8>>)>> = RefCell::new(HashMap::new());
    static VERSION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn dots(c: &PointCloud) -> Vec<u8> {
    let mut out = Vec::with_capacity(c.len() * crate::gpu::CLOUD_POINT_SIZE);
    let size = c.point_size as f32;
    let shade = c.shade();
    for (i, p) in c.points.iter().enumerate() {
        // Placed in double precision: the cloud's origin may be far from the design's.
        let local = c.origin + solidcraft_engine::geom::Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
        let w = solidcraft_engine::doc::apply_point(&c.transform, local);
        crate::gpu::cloud_point(&mut out, [w.x as f32, w.y as f32, w.z as f32], shade(i), size);
    }
    out
}

/// The shown point clouds as GPU dots.
pub fn gpu_points(app: &SolveApp) -> Vec<GpuPoints> {
    let doc = &app.session.doc;
    DOTS.with(|m| {
        let mut m = m.borrow_mut();
        m.retain(|id, _| doc.point_clouds.iter().any(|c| c.id == *id));
        solidcraft_engine::view::shown_point_clouds(&app.session)
            .map(|c| {
                let key = Key::of(c);
                if let Some((k, version, bytes)) = m.get(&c.id)
                    && *k == key
                {
                    return GpuPoints { id: c.id, version: *version, bytes: bytes.clone() };
                }
                let version = VERSION.with(|v| {
                    v.set(v.get() + 1);
                    v.get()
                });
                let bytes = Arc::new(dots(c));
                m.insert(c.id, (key, version, bytes.clone()));
                GpuPoints { id: c.id, version, bytes }
            })
            .collect()
    })
}

/// CPU viewport only: some of each shown cloud's points over the rendered model.
pub fn show(app: &SolveApp, painter: &Painter, proj: &Proj) {
    if app.viewport.gpu.is_some() {
        return;
    }
    let clouds: Vec<&PointCloud> = solidcraft_engine::view::shown_point_clouds(&app.session).collect();
    let total: usize = clouds.iter().map(|c| c.len()).sum();
    if total == 0 {
        return;
    }
    let stride = total.div_ceil(MAX_CPU_POINTS).max(1);
    let mut shapes = Vec::new();
    for c in clouds {
        let r = (c.point_size as f32 / painter.ctx().pixels_per_point() * 0.5).max(0.5);
        let shade = c.shade();
        for i in (0..c.len()).step_by(stride) {
            let Some(p) = c.world_point(i) else { continue };
            let Some(q) = proj.to_screen(p) else { continue };
            shapes.push(Shape::rect_filled(egui::Rect::from_center_size(q, egui::vec2(r * 2.0, r * 2.0)), 0.0, crate::theme::model_color(shade(i))));
        }
    }
    painter.extend(shapes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_follow_the_cloud_and_are_made_again_only_when_it_changes() {
        let mut h = crate::scenario::Harness::new();
        let dir = std::env::temp_dir().join(format!("solidcraft-ui-cloud-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("two.xyz");
        std::fs::write(&path, "0 0 0 255 0 0\n10 0 0 0 255 0\n").unwrap();
        let r = h.app.run("pointcloud.insert", serde_json::json!({"path": path.to_string_lossy(), "point_size": 3})).unwrap();
        let id = r["cloud"].as_u64().unwrap();
        h.app.run("browser.collapse_all", serde_json::json!({})).unwrap();
        assert!(!crate::browser::is_open(&h.app, "point_clouds", true));
        h.app.run("browser.expand_all", serde_json::json!({})).unwrap();
        assert!(crate::browser::is_open(&h.app, "point_clouds", true));
        let first = gpu_points(&h.app);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].bytes.len(), 2 * crate::gpu::CLOUD_POINT_SIZE);
        // The second point: at x = 10, green, 3 px (24 eighths).
        assert_eq!(&first[0].bytes[16..20], &10f32.to_le_bytes());
        assert_eq!(&first[0].bytes[28..32], &[0, 255, 0, 24]);
        assert_eq!(gpu_points(&h.app)[0].version, first[0].version, "unchanged: the same dots");
        h.app.run("pointcloud.edit", serde_json::json!({"cloud": id, "color": [0, 0, 1]})).unwrap();
        let blue = gpu_points(&h.app);
        assert_ne!(blue[0].version, first[0].version);
        assert_eq!(&blue[0].bytes[12..15], &[0, 0, 255]);
        // By height across x = 0…10 (the cloud turned so x is up): low end blue, high end red.
        let up = serde_json::json!([0, 0, -1, 0, 0, 1, 0, 0, 1, 0, 0, 0]);
        h.app.run("pointcloud.edit", serde_json::json!({"cloud": id, "color_by": "height", "matrix": up})).unwrap();
        let ramp = gpu_points(&h.app);
        let ramp_of = solidcraft_engine::doc::point_cloud::height_ramp;
        assert_eq!((&ramp[0].bytes[12..15], &ramp[0].bytes[28..31]), (&ramp_of(0.0)[..], &ramp_of(1.0)[..]));
        // Hidden, alone or with its folder: nothing drawn; the view frames it while shown.
        assert!(!solidcraft_engine::view::bounds(&h.app.session).is_empty());
        let first_bounds = crate::viewport::model_bounds(&h.app);
        assert!(!first_bounds.is_empty());
        assert_eq!(crate::viewport::model_bounds(&h.app), first_bounds, "cached sketch bounds keep the point cloud extent");
        h.app.run("pointcloud.edit", serde_json::json!({"cloud": id, "visible": false})).unwrap();
        assert!(gpu_points(&h.app).is_empty());
        assert!(solidcraft_engine::view::bounds(&h.app.session).is_empty());
        assert!(crate::viewport::model_bounds(&h.app).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}

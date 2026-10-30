//! Point clouds (#5151): with the GPU viewport their points are drawn in the 3D pass
//! (depth-tested with the model), coloured and filtered by the shaders; the CPU viewport draws
//! some of them over its image.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use egui::{Painter, Shape};
use solidcraft_engine::doc::point_cloud::Keep;
use solidcraft_engine::doc::point_cloud::{ColorBy, PLAIN, PointCloud, Source, byte};
use solidcraft_engine::doc::point_cloud_distribution::color_range_by;
use solidcraft_gpu::view::{CloudFilter, CloudStyle, ColorMode, Dot, FILTERS, GpuCloud, cloud_dot};

use crate::SolveApp;
use crate::browser::point_cloud_tracks::{Ask, Paint, Tracks};
use crate::viewport::Proj;

/// Most points the CPU viewport draws of all clouds together.
const MAX_CPU_POINTS: usize = 100_000;

/// What a cloud's records are made from: they are made again only when one of these changes.
/// Its colour range, filter ranges and dot size are its style, read by the shaders.
#[derive(Clone, PartialEq)]
struct Key {
    data: Vec<usize>,
    len: usize,
    origin: [u64; 3],
    transform: [u64; 16],
    color_by: ColorBy,
    color: Option<[u32; 3]>,
    value: Option<String>,
    filtered: [Option<String>; FILTERS],
}

impl Key {
    fn of(c: &PointCloud) -> Key {
        let mut data = vec![Arc::as_ptr(c.points.values()).cast::<f32>() as usize, Arc::as_ptr(&c.colors.0).cast::<u8>() as usize];
        data.extend(c.complex.columns.iter().map(|k| Arc::as_ptr(&k.values.0).cast::<f32>() as usize));
        Key {
            data,
            len: c.len(),
            origin: [c.origin.x, c.origin.y, c.origin.z].map(f64::to_bits),
            transform: std::array::from_fn(|i| c.transform[i / 4][i % 4].to_bits()),
            color_by: c.color_by,
            color: c.color.map(|k| k.map(f32::to_bits)),
            value: c.colored_property().map(str::to_string),
            filtered: std::array::from_fn(|k| c.filters.get(k).map(|f| f.property.clone())),
        }
    }
}

thread_local! {
    /// Records by cloud id: what they were made from, their version and bytes.
    static RECORDS: RefCell<HashMap<u64, (Key, u64, Arc<Vec<u8>>)>> = RefCell::new(HashMap::new());
    static VERSION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// By cloud id, the values its coloured property takes over what is kept, worked out off the
    /// frame (one worker a cloud, so that each keeps its own geometry).
    static EXTENTS: RefCell<HashMap<u64, Tracks>> = RefCell::new(HashMap::new());
}

/// The ramp's range of `c` as drawn ([`as_drawn`]): the one set, else, by a property, exactly
/// the values it takes over what the drawn filters keep. Worked out off the frame: until then,
/// the last answer, or the cloud's own bound.
pub(crate) fn color_range(ctx: &egui::Context, c: &PointCloud) -> Option<[f64; 2]> {
    color_range_by(c, |property, axis| {
        let ask = Ask {
            property: property.to_string(),
            span: axis,
            loupe: axis,
            bins: 1,
            paint: Paint::One,
            filters: c.filters.iter().take(FILTERS).cloned().collect(),
            changes_of: None,
        };
        EXTENTS.with(|e| e.borrow_mut().entry(c.id).or_default().answer(ctx, c, "extent", ask)).and_then(|a| a.extent)
    })
}

fn records(c: &PointCloud) -> Arc<Vec<u8>> {
    let plain = PLAIN.map(byte);
    let single = c.color.unwrap_or(PLAIN).map(byte);
    let own = |i: usize| match c.color_by {
        ColorBy::Single => single,
        ColorBy::File => c.colors.get(i).unwrap_or(plain),
        ColorBy::Height | ColorBy::Property => plain,
    };
    // The coloured property, then the filtered ones.
    let sources: [Option<Source>; 1 + FILTERS] = std::array::from_fn(|k| {
        let name = if k == 0 { c.colored_property() } else { c.filters.get(k - 1).map(|f| f.property.as_str()) };
        name.and_then(|n| c.source(n))
    });
    let mut out = Vec::with_capacity(c.len() * solidcraft_gpu::view::DOT_SIZE);
    for i in 0..c.len() {
        // Placed in double precision: the cloud's origin may be far from the design's.
        let w = c.world_point(i).unwrap_or_default();
        let at = [w.x as f32, w.y as f32, w.z as f32];
        let values = sources.map(|s| {
            s.map(|s| match s {
                Source::Coordinate(k) => at.get(k).copied().unwrap_or(0.0),
                Source::Column(v) => v.get(i).copied().unwrap_or(0.0),
            })
        });
        let mut dot = Dot { at, rgb: own(i), ..Default::default() };
        let [value, filtered @ ..] = values;
        if let Some(v) = value {
            dot.value = v;
            dot.has |= 1 << FILTERS;
        }
        for (k, f) in filtered.into_iter().enumerate() {
            if let (Some(f), Some(slot)) = (f, dot.filtered.get_mut(k)) {
                *slot = f;
                dot.has |= 1 << k;
            }
        }
        cloud_dot(&mut out, &dot);
    }
    Arc::new(out)
}

/// The cloud as it is drawn now: a filter `playing` in the Edit Point Cloud card at its range
/// now, so that what follows from the filters (the ramp's automatic range) follows it too.
pub(crate) fn as_drawn(c: &PointCloud, playing: impl Fn(usize) -> Option<[f64; 2]>) -> std::borrow::Cow<'_, PointCloud> {
    let mut c = std::borrow::Cow::Borrowed(c);
    for k in 0..c.filters.len() {
        if let Some(range) = playing(k)
            && let Some(f) = c.to_mut().filters.get_mut(k)
        {
            f.keep = Keep::Range { range };
        }
    }
    c
}

/// How a cloud is coloured and which of its elements are shown, as the shaders read it; a filter
/// playing in the Edit Point Cloud card shows its range now.
fn style(ctx: &egui::Context, c: &PointCloud, playing: impl Fn(usize) -> Option<[f64; 2]>) -> CloudStyle {
    let c = as_drawn(c, playing);
    let property = c.colored_property();
    let mode = match (c.color_by, property) {
        (ColorBy::File | ColorBy::Single, _) | (_, None) => ColorMode::Own,
        (_, Some(name)) if c.complex.integer(name) => ColorMode::Palette,
        _ => ColorMode::Ramp,
    };
    let mut filters = [CloudFilter::Off; FILTERS];
    for (f, slot) in c.filters.iter().zip(filters.iter_mut()) {
        *slot = match &f.keep {
            Keep::Range { range } => CloudFilter::Range(range.map(|v| v as f32)),
            Keep::Values { values } => {
                // Bit `i` is the value `offset + i`, from the label's smallest value.
                let offset = c.complex.names(&f.property).and_then(|n| n.values.iter().map(|(v, _)| *v).min()).unwrap_or(0);
                let mask = values
                    .iter()
                    .filter_map(|v| u32::try_from(i64::from(*v) - i64::from(offset)).ok())
                    .filter(|i| *i < 32)
                    .fold(0u32, |m, i| m | 1 << i);
                CloudFilter::Values { offset, mask }
            }
        };
    }
    let range = color_range(ctx, &c).unwrap_or([0.0, 1.0]).map(|v| v as f32);
    CloudStyle { mode, range, filters, point_size: c.point_size as f32 }
}

/// The shown point clouds as the GPU draws them.
pub fn gpu_clouds(app: &SolveApp, ctx: &egui::Context) -> Vec<GpuCloud> {
    let doc = &app.session.doc;
    EXTENTS.with(|e| e.borrow_mut().retain(|id, _| doc.point_clouds.iter().any(|c| c.id == *id)));
    RECORDS.with(|m| {
        let mut m = m.borrow_mut();
        m.retain(|id, _| doc.point_clouds.iter().any(|c| c.id == *id));
        solidcraft_engine::view::shown_point_clouds(&app.session)
            .map(|c| {
                let key = Key::of(c);
                let (version, dots) = match m.get(&c.id) {
                    Some((k, version, records)) if *k == key => (*version, records.clone()),
                    _ => {
                        let version = VERSION.with(|v| {
                            v.set(v.get() + 1);
                            v.get()
                        });
                        let records = records(c);
                        m.insert(c.id, (key, version, records.clone()));
                        (version, records)
                    }
                };
                let playing = |k: usize| app.tree.cloud_panel.as_ref().and_then(|p| p.playing(c.id, k));
                GpuCloud { id: c.id, version, dots, style: style(ctx, c, playing) }
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
        let shade = c.shade(color_range(painter.ctx(), c));
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
    fn records_are_made_again_only_when_the_cloud_changes_and_its_style_follows_it() {
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
        let first = gpu_clouds(&h.app, &h.ctx);
        assert_eq!(first.len(), 1);
        let size = solidcraft_gpu::view::DOT_SIZE;
        assert_eq!(first[0].dots.len(), 2 * size);
        // The second point: at x = 10, green.
        assert_eq!(&first[0].dots[size..size + 4], &10f32.to_le_bytes());
        assert_eq!(&first[0].dots[2 * size - 4..2 * size - 1], &[0, 255, 0]);
        assert_eq!(first[0].style.point_size, 3.0);
        assert_eq!(gpu_clouds(&h.app, &h.ctx)[0].version, first[0].version, "unchanged: the same records");
        h.app.run("pointcloud.edit", serde_json::json!({"cloud": id, "color": [0, 0, 1]})).unwrap();
        let blue = gpu_clouds(&h.app, &h.ctx);
        assert_ne!(blue[0].version, first[0].version);
        assert_eq!(&blue[0].dots[size - 4..size - 1], &[0, 0, 255]);
        // By height: a ramp over the range, the range a style alone.
        h.app.run("pointcloud.edit", serde_json::json!({"cloud": id, "color_by": "height", "height_range": [0, 1]})).unwrap();
        let ramp = gpu_clouds(&h.app, &h.ctx);
        assert_eq!((ramp[0].style.mode, ramp[0].style.range), (ColorMode::Ramp, [0.0, 1.0]));
        h.app.run("pointcloud.edit", serde_json::json!({"cloud": id, "height_range": [0, 2]})).unwrap();
        let wider = gpu_clouds(&h.app, &h.ctx);
        assert_eq!((wider[0].version, wider[0].style.range), (ramp[0].version, [0.0, 2.0]), "a new range uploads nothing");
        // Hidden, alone or with its folder: nothing drawn; the view frames it while shown.
        assert!(!solidcraft_engine::view::bounds(&h.app.session).is_empty());
        let first_bounds = crate::viewport::model_bounds(&h.app);
        assert!(!first_bounds.is_empty());
        assert_eq!(crate::viewport::model_bounds(&h.app), first_bounds, "cached sketch bounds keep the point cloud extent");
        h.app.run("pointcloud.edit", serde_json::json!({"cloud": id, "visible": false})).unwrap();
        assert!(gpu_clouds(&h.app, &h.ctx).is_empty());
        assert!(solidcraft_engine::view::bounds(&h.app.session).is_empty());
        assert!(crate::viewport::model_bounds(&h.app).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_automatic_range_is_what_is_kept_and_follows_a_playing_filter() {
        let mut h = crate::scenario::Harness::new();
        let dir = std::env::temp_dir().join(format!("solidcraft-ui-cloud-play-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.ply");
        // Eleven points, x and t both running 0 to 10.
        let mut ply = String::from(
            "ply\nformat ascii 1.0\nelement vertex 11\nproperty float x\nproperty float y\nproperty float z\nproperty float t\nend_header\n",
        );
        for i in 0..=10 {
            ply += &format!("{i} 0 0 {i}\n");
        }
        std::fs::write(&path, ply).unwrap();
        let id = h.app.run("pointcloud.insert", serde_json::json!({"path": path.to_string_lossy()})).unwrap()["cloud"].as_u64().unwrap();
        // Filtered by x, not by t: the range is what is kept of t, 2 to 4, once worked out.
        h.app.run("pointcloud.edit", serde_json::json!({"cloud": id, "color_by": "t", "filters": [{"property": "x", "range": [2, 4]}]})).unwrap();
        let range = |h: &crate::scenario::Harness, playing: Option<[f64; 2]>| style(&h.ctx, &h.app.session.doc.point_clouds[0], |_| playing).range;
        let settle = |h: &crate::scenario::Harness, playing: Option<[f64; 2]>, want: [f32; 2]| {
            (0..500).any(|_| {
                let ok = range(h, playing) == want;
                if !ok {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                ok
            })
        };
        assert!(settle(&h, None, [2.0, 4.0]), "{:?}", range(&h, None));
        // Played on to 8, the filter is drawn there, and the range follows it.
        assert!(settle(&h, Some([2.0, 8.0]), [2.0, 8.0]), "{:?}", range(&h, Some([2.0, 8.0])));
        let _ = std::fs::remove_dir_all(dir);
    }
}

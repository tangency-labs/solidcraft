//! Bounded, lazy media for command hints; presentation never executes a command.

use std::collections::{BTreeSet, VecDeque};
use std::io::Cursor;
use std::sync::{Arc, OnceLock};

const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_WIDTH: u32 = 960;
const MAX_HEIGHT: u32 = 600;
const MAX_FRAMES: usize = 32;
const MAX_DECODED_BYTES: usize = 24 * 1024 * 1024;
#[cfg(not(target_arch = "wasm32"))]
const MAX_ANIMATION_SECONDS: f64 = 10.0;
const CACHE_SLOTS: usize = 8;
const REPLAY_PAUSE_SECONDS: f64 = 3.0;

macro_rules! embedded {
    ($($name:literal),+) => {
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/command-hints/exports/", $($name),+))
    };
}

struct Entry {
    command: &'static str,
    posters: [&'static [u8]; 2],
    #[cfg(not(target_arch = "wasm32"))]
    animation: Option<[&'static [u8]; 2]>,
}

macro_rules! entry {
    ($command:literal, $slug:literal) => {
        Entry {
            command: $command,
            posters: [embedded!($slug, "-light.png"), embedded!($slug, "-dark.png")],
            #[cfg(not(target_arch = "wasm32"))]
            animation: None,
        }
    };
    ($command:literal, $slug:literal, animated) => {
        Entry {
            command: $command,
            posters: [embedded!($slug, "-light.png"), embedded!($slug, "-dark.png")],
            #[cfg(not(target_arch = "wasm32"))]
            animation: Some([embedded!($slug, "-light.gif"), embedded!($slug, "-dark.gif")]),
        }
    };
}

const CATALOG: &[Entry] = &[
    entry!("construct.axis.edge", "axis-edge"),
    entry!("construct.axis.normal_to_face", "axis-normal_to_face"),
    entry!("construct.axis.through_cylinder", "axis-through_cylinder"),
    entry!("construct.axis.two_planes", "axis-two_planes"),
    entry!("construct.axis.two_points", "axis-two_points"),
    entry!("construct.plane.along_path", "plane-along_path"),
    entry!("construct.plane.at_angle", "plane-at_angle"),
    entry!("construct.plane.axis_point", "plane-axis_point"),
    entry!("construct.plane.midplane", "plane-midplane"),
    entry!("construct.plane.offset", "plane-offset"),
    entry!("construct.plane.perpendicular", "plane-perpendicular"),
    entry!("construct.plane.tangent", "plane-tangent"),
    entry!("construct.plane.three_points", "plane-three_points"),
    entry!("construct.plane.two_edges", "plane-two_edges"),
    entry!("construct.point.along_path", "point-along_path"),
    entry!("construct.point.center", "point-center"),
    entry!("construct.point.edge_plane", "point-edge_plane"),
    entry!("construct.point.three_planes", "point-three_planes"),
    entry!("construct.point.two_edges", "point-two_edges"),
    entry!("construct.point.vertex", "point-vertex"),
    entry!("explode.create", "explode-create"),
    entry!("face.delete", "delete-face"),
    entry!("inspect.accessibility", "inspect-accessibility"),
    entry!("inspect.center_of_mass", "inspect-center_of_mass"),
    entry!("inspect.colored_components", "inspect-colored_components"),
    entry!("inspect.compare", "inspect-compare"),
    entry!("inspect.continuity", "inspect-continuity"),
    entry!("inspect.curvature_comb", "inspect-curvature_comb"),
    entry!("inspect.curvature_map", "inspect-curvature_map"),
    entry!("inspect.draft", "inspect-draft"),
    entry!("inspect.environment_map", "inspect-environment_map"),
    entry!("inspect.interference", "inspect-interference"),
    entry!("inspect.isocurve", "inspect-isocurve"),
    entry!("inspect.measure", "inspect-measure"),
    entry!("inspect.minimum_radius", "inspect-minimum_radius"),
    entry!("inspect.section", "inspect-section"),
    entry!("inspect.zebra", "inspect-zebra"),
    entry!("joint.as_built", "joint-as_built"),
    entry!("joint.cam", "joint-cam"),
    entry!("joint.create", "joint-create"),
    entry!("joint.drive", "joint-drive"),
    entry!("joint.gear", "joint-gear"),
    entry!("joint.limits", "joint-limits"),
    entry!("joint.motion_link", "joint-motion_link"),
    entry!("joint.origin", "joint-origin"),
    entry!("joint.rack_pinion", "joint-rack_pinion"),
    entry!("joint.reach", "joint-reach"),
    entry!("joint.rigid_group", "joint-rigid_group"),
    entry!("joint.screw", "joint-screw"),
    entry!("motion.study", "motion-study"),
    entry!("motion.trace", "motion-trace"),
    entry!("occurrence.move", "occurrence-move"),
    entry!("plastic.boss", "plastic-boss"),
    entry!("plastic.lip", "plastic-lip"),
    entry!("plastic.rest", "plastic-rest"),
    entry!("plastic.snap_fit", "plastic-snap_fit"),
    entry!("sheet.convert", "convert-sheet-metal"),
    entry!("sheet.flange", "sheet-flange"),
    entry!("sheet.flat_pattern", "sheet-flat_pattern"),
    entry!("sheet.fold", "sheet-fold"),
    entry!("sheet.hem", "sheet-hem"),
    entry!("sheet.path_flange", "sheet-path_flange"),
    entry!("sheet.perforate", "sheet-perforate"),
    entry!("sheet.refold", "sheet-refold"),
    entry!("sheet.unfold", "sheet-unfold"),
    entry!("sketch.arc.center_point", "sketch-arc-center_point", animated),
    entry!("sketch.arc.tangent", "sketch-arc-tangent", animated),
    entry!("sketch.arc.three_point", "sketch-arc-three_point", animated),
    entry!("sketch.arc3d", "arc-3d"),
    entry!("sketch.blend_curve", "sketch-blend_curve"),
    entry!("sketch.break", "sketch-break"),
    entry!("sketch.centerline", "sketch-centerline"),
    entry!("sketch.chamfer.distance_angle", "sketch-chamfer-distance_angle"),
    entry!("sketch.chamfer.equal_distance", "sketch-chamfer-equal_distance"),
    entry!("sketch.chamfer.two_distance", "sketch-chamfer-two_distance"),
    entry!("sketch.circle.center", "sketch-circle-center", animated),
    entry!("sketch.circle.three_point", "sketch-circle-three_point", animated),
    entry!("sketch.circle.three_tangent", "sketch-circle-three_tangent", animated),
    entry!("sketch.circle.two_point", "sketch-circle-two_point", animated),
    entry!("sketch.circle.two_tangent", "sketch-circle-two_tangent", animated),
    entry!("sketch.conic", "sketch-conic"),
    entry!("sketch.constraint.coincident", "sketch-constraint-coincident"),
    entry!("sketch.constraint.collinear", "sketch-constraint-collinear"),
    entry!("sketch.constraint.concentric", "sketch-constraint-concentric"),
    entry!("sketch.constraint.equal", "sketch-constraint-equal"),
    entry!("sketch.constraint.horizontal_vertical", "sketch-constraint-horizontal_vertical"),
    entry!("sketch.constraint.midpoint", "sketch-constraint-midpoint"),
    entry!("sketch.constraint.parallel", "sketch-constraint-parallel"),
    entry!("sketch.constraint.perpendicular", "sketch-constraint-perpendicular"),
    entry!("sketch.constraint.symmetry", "sketch-constraint-symmetry"),
    entry!("sketch.constraint.tangent", "sketch-constraint-tangent"),
    entry!("sketch.construction", "sketch-construction"),
    entry!("sketch.construction_line", "sketch-construction_line"),
    entry!("sketch.construction_line.angle", "sketch-construction_line-angle"),
    entry!("sketch.construction_line.bisector", "sketch-construction_line-bisector"),
    entry!("sketch.construction_line.horizontal", "sketch-construction_line-horizontal"),
    entry!("sketch.construction_line.parallel", "sketch-construction_line-parallel"),
    entry!("sketch.construction_line.perpendicular", "sketch-construction_line-perpendicular"),
    entry!("sketch.construction_line.tangent_at", "sketch-construction_line-tangent_at"),
    entry!("sketch.construction_line.tangent_external", "sketch-construction_line-tangent_external"),
    entry!("sketch.construction_line.tangent_two", "sketch-construction_line-tangent_two"),
    entry!("sketch.construction_line.vertical", "sketch-construction_line-vertical"),
    entry!("sketch.create", "new-sketch"),
    entry!("sketch.dimension", "sketch-dimension"),
    entry!("sketch.ellipse", "sketch-ellipse"),
    entry!("sketch.extend", "sketch-extend"),
    entry!("sketch.fillet", "sketch-fillet"),
    entry!("sketch.fit_curves_to_section", "sketch-fit_curves_to_section"),
    entry!("sketch.include_3d", "sketch-include_3d"),
    entry!("sketch.intersect", "sketch-intersect"),
    entry!("sketch.intersection_curve", "sketch-intersection_curve"),
    entry!("sketch.isoparametric_curve", "sketch-isoparametric_curve"),
    entry!("sketch.line", "sketch-line", animated),
    entry!("sketch.line.midpoint", "sketch-line-midpoint", animated),
    entry!("sketch.line3d", "line-3d"),
    entry!("sketch.mirror", "sketch-mirror"),
    entry!("sketch.move", "sketch-move"),
    entry!("sketch.offset", "sketch-offset"),
    entry!("sketch.pattern.circular", "sketch-pattern-circular"),
    entry!("sketch.pattern.rectangular", "sketch-pattern-rectangular"),
    entry!("sketch.polygon.circumscribed", "sketch-polygon-circumscribed"),
    entry!("sketch.polygon.edge", "sketch-polygon-edge"),
    entry!("sketch.polygon.inscribed", "sketch-polygon-inscribed"),
    entry!("sketch.project", "sketch-project"),
    entry!("sketch.project_to_surface", "sketch-project_to_surface"),
    entry!("sketch.rectangle.center", "sketch-rectangle-center", animated),
    entry!("sketch.rectangle.rounded", "sketch-rectangle-rounded", animated),
    entry!("sketch.rectangle.three_point", "sketch-rectangle-three_point", animated),
    entry!("sketch.rectangle.two_point", "rectangle", animated),
    entry!("sketch.scale", "sketch-scale"),
    entry!("sketch.slot.arc_center", "sketch-slot-arc_center", animated),
    entry!("sketch.slot.arc_three_point", "sketch-slot-arc_three_point", animated),
    entry!("sketch.slot.center_point", "sketch-slot-center_point", animated),
    entry!("sketch.slot.center_to_center", "sketch-slot-center_to_center", animated),
    entry!("sketch.slot.overall", "sketch-slot-overall", animated),
    entry!("sketch.spline.control_point", "sketch-spline-control_point", animated),
    entry!("sketch.spline.control_point_5", "sketch-spline-control_point_5", animated),
    entry!("sketch.spline.fit_point", "sketch-spline-fit_point", animated),
    entry!("sketch.spline3d", "spline-3d"),
    entry!("sketch.spun_profile", "sketch-spun_profile"),
    entry!("sketch.text", "sketch-text"),
    entry!("sketch.transform", "sketch-transform"),
    entry!("sketch.trim", "sketch-trim"),
    entry!("solid.align", "align"),
    entry!("solid.boundary_fill", "boundary_fill"),
    entry!("solid.bounding_solid", "bounding_solid"),
    entry!("solid.box", "box"),
    entry!("solid.chamfer", "chamfer"),
    entry!("solid.coil", "coil"),
    entry!("solid.combine", "combine"),
    entry!("solid.cone", "cone"),
    entry!("solid.cylinder", "cylinder"),
    entry!("solid.draft", "draft"),
    entry!("solid.emboss", "emboss"),
    entry!("solid.extrude", "extrude", animated),
    entry!("solid.fillet", "fillet"),
    entry!("solid.hole", "hole"),
    entry!("solid.loft", "loft"),
    entry!("solid.mirror", "mirror"),
    entry!("solid.move", "move"),
    entry!("solid.offset_face", "offset-face"),
    entry!("solid.pattern.circular", "circular-pattern"),
    entry!("solid.pattern.path", "path-pattern"),
    entry!("solid.pattern.rectangular", "rectangular-pattern"),
    entry!("solid.pipe", "pipe"),
    entry!("solid.press_pull", "press-pull"),
    entry!("solid.replace_face", "replace-face"),
    entry!("solid.revolve", "revolve"),
    entry!("solid.rib", "rib"),
    entry!("solid.scale", "scale"),
    entry!("solid.shell", "shell"),
    entry!("solid.sphere", "sphere"),
    entry!("solid.split_body", "split-body"),
    entry!("solid.split_face", "split-face"),
    entry!("solid.sweep", "sweep"),
    entry!("solid.thread", "thread"),
    entry!("solid.torus", "torus"),
    entry!("solid.web", "web"),
    entry!("surface.blend", "surface-blend"),
    entry!("surface.boundary", "surface-boundary"),
    entry!("surface.extend", "surface-extend"),
    entry!("surface.extrude", "surface-extrude"),
    entry!("surface.loft", "surface-loft"),
    entry!("surface.offset", "surface-offset"),
    entry!("surface.patch", "surface-patch"),
    entry!("surface.revolve", "surface-revolve"),
    entry!("surface.ruled", "surface-ruled"),
    entry!("surface.stitch", "surface-stitch"),
    entry!("surface.sweep", "surface-sweep"),
    entry!("surface.thicken", "surface-thicken"),
    entry!("surface.trim", "surface-trim"),
    entry!("surface.unstitch", "surface-unstitch"),
    entry!("surface.untrim", "surface-untrim"),
];

struct Asset {
    key: usize,
    bytes: &'static [u8],
    animated: bool,
}

fn asset(index: usize, dark: bool, animate: bool) -> Option<Asset> {
    let entry = CATALOG.get(index)?;
    let look = usize::from(dark);
    #[cfg(not(target_arch = "wasm32"))]
    if animate && let Some(animation) = entry.animation {
        return Some(Asset { key: index * 4 + look * 2 + 1, bytes: animation.get(look)?, animated: true });
    }
    #[cfg(target_arch = "wasm32")]
    let _ = animate;
    Some(Asset { key: index * 4 + look * 2, bytes: entry.posters.get(look)?, animated: false })
}

pub(crate) fn available(command: &str) -> bool {
    CATALOG.iter().any(|entry| entry.command == command)
}

#[derive(Clone)]
struct Slot {
    key: usize,
    decoded: DecodeTask,
    texture: Option<egui::TextureHandle>,
    uploaded_frame: Option<usize>,
}

type DecodeTask = Arc<OnceLock<Result<Arc<Media>, String>>>;

#[derive(Clone, Default)]
struct Cache {
    slots: VecDeque<Slot>,
    failed: BTreeSet<usize>,
    playback: Playback,
}

impl Cache {
    fn record_failures(&mut self) {
        self.failed.extend(self.slots.iter().filter(|slot| slot.decoded.get().is_some_and(Result::is_err)).map(|slot| slot.key));
    }

    fn prepare(&mut self, key: usize, start: impl FnOnce() -> DecodeTask) -> Option<usize> {
        if self.failed.contains(&key) {
            return None;
        }
        if let Some(index) = self.slots.iter().position(|slot| slot.key == key) {
            let slot = self.slots.remove(index)?;
            self.slots.push_back(slot);
            return self.slots.len().checked_sub(1);
        }
        if self.slots.len() == CACHE_SLOTS {
            // Pending tasks retain their slots, bounding detached workers as well as memory.
            let oldest_finished = self.slots.iter().position(|slot| slot.decoded.get().is_some())?;
            if let Some(evicted) = self.slots.remove(oldest_finished)
                && evicted.decoded.get().is_some_and(Result::is_err)
            {
                self.failed.insert(evicted.key);
            }
        }
        self.slots.push_back(Slot { key, decoded: start(), texture: None, uploaded_frame: None });
        self.slots.len().checked_sub(1)
    }

    fn prepare_asset(&mut self, ctx: &egui::Context, index: usize, dark: bool, animate: bool) -> Option<(Asset, Option<usize>)> {
        let mut selected = asset(index, dark, animate)?;
        for _ in 0..2 {
            self.record_failures();
            if self.failed.contains(&selected.key) {
                if !selected.animated {
                    return None;
                }
                selected = asset(index, dark, false)?;
                if self.failed.contains(&selected.key) {
                    return None;
                }
            }
            let slot = self.prepare(selected.key, || start_decode(ctx, &selected));
            if let Some(slot) = slot.and_then(|index| self.slots.get(index))
                && slot.decoded.get().is_some_and(Result::is_err)
            {
                self.failed.insert(selected.key);
                continue;
            }
            return Some((selected, slot));
        }
        None
    }
}

#[derive(Clone, Default)]
struct Playback {
    scope: Option<egui::Id>,
    last_pass: u64,
    started: f64,
}

impl Playback {
    fn elapsed(&mut self, scope: egui::Id, pass: u64, now: f64) -> f64 {
        let now = if now.is_finite() { now } else { 0.0 };
        if self.scope != Some(scope) || pass > self.last_pass.saturating_add(1) || pass < self.last_pass || now < self.started {
            self.started = now;
        }
        self.scope = Some(scope);
        self.last_pass = pass;
        (now - self.started).max(0.0)
    }
}

struct Media {
    frames: Vec<Arc<egui::ColorImage>>,
    frame_ends: Vec<f64>,
}

impl Media {
    fn frame_at(&self, elapsed: f64) -> (usize, Option<f64>) {
        if self.frames.len() < 2 {
            return (0, None);
        }
        let last = self.frames.len().saturating_sub(1);
        let result_start = self.frame_ends.get(last.saturating_sub(1)).copied().unwrap_or(0.0);
        let cycle = result_start + REPLAY_PAUSE_SECONDS;
        let elapsed = if elapsed.is_finite() { elapsed.max(0.0) % cycle } else { 0.0 };
        let frame = self.frame_ends.partition_point(|end| *end <= elapsed).min(last);
        let next = if frame < last { self.frame_ends.get(frame).map(|end| (end - elapsed).max(0.0)) } else { Some(cycle - elapsed) };
        (frame, next)
    }
}

/// Returns false for an unknown or failed asset so the caller keeps its text hint.
pub(crate) fn show(ui: &mut egui::Ui, command: &str, dark: bool, animate: bool) -> bool {
    let Some(index) = CATALOG.iter().position(|entry| entry.command == command) else { return false };
    let animate = animate && cfg!(not(target_arch = "wasm32")) && ui.ctx().global_style().animation_time > 0.0;
    let cache_id = egui::Id::new("command_hint_media");
    let mut cache = ui.ctx().data_mut(|data| data.get_temp::<Cache>(cache_id).unwrap_or_default());
    let Some((asset, slot)) = cache.prepare_asset(ui.ctx(), index, dark, animate) else {
        ui.ctx().data_mut(|data| data.insert_temp(cache_id, cache));
        return false;
    };
    let scope = ui.next_auto_id().with((command, dark, asset.animated));
    let size = media_size(ui);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    crate::scenario::publish_rect("command_hint:media", rect);
    crate::scenario::publish_rect(&format!("command_hint:media:{command}"), rect);
    if let Some(slot) = slot.and_then(|index| cache.slots.get_mut(index))
        && let Some(Ok(media)) = slot.decoded.get()
    {
        let elapsed = cache.playback.elapsed(scope, ui.ctx().cumulative_pass_nr(), ui.input(|input| input.time));
        let (frame, next) = media.frame_at(elapsed);
        if let Some(image) = media.frames.get(frame) {
            if slot.uploaded_frame != Some(frame) {
                if let Some(texture) = &mut slot.texture {
                    texture.set(image.clone(), egui::TextureOptions::LINEAR);
                } else {
                    slot.texture = Some(ui.ctx().load_texture(format!("command-hint-{}", asset.key), image.clone(), egui::TextureOptions::LINEAR));
                }
                slot.uploaded_frame = Some(frame);
            }
            if let Some(texture) = &slot.texture {
                egui::Image::new(texture).fit_to_exact_size(size).paint_at(ui, rect);
            }
        }
        if let Some(seconds) = next {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(seconds));
        }
    }
    ui.ctx().data_mut(|data| data.insert_temp(cache_id, cache));
    true
}

fn media_size(ui: &egui::Ui) -> egui::Vec2 {
    let tokens = crate::theme::Tokens::get();
    let size = egui::vec2(tokens.command_hint_media_width, tokens.command_hint_media_height);
    let text_room = ui.text_style_height(&egui::TextStyle::Button) + ui.text_style_height(&egui::TextStyle::Body) * 4.0;
    let chrome_room = tokens.space_24 * 2.0 + tokens.space_8 * 2.0;
    let viewport = ui.ctx().content_rect().size();
    let viewport_room = egui::vec2((viewport.x - tokens.space_24).max(0.0), (viewport.y - text_room - chrome_room).max(0.0));
    let room = ui.available_size().min(viewport_room);
    let scale = (room.x / size.x).min(room.y / size.y).clamp(0.0, 1.0);
    size * scale
}

fn start_decode(ctx: &egui::Context, asset: &Asset) -> DecodeTask {
    let result = Arc::new(OnceLock::new());
    let bytes = asset.bytes;
    let animated = asset.animated;
    let slot = asset.key;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let completed = result.clone();
        let wake = ctx.clone();
        if let Err(error) = std::thread::Builder::new().name(format!("command-hint-{slot}")).spawn(move || {
            finish_decode(&completed, decode(bytes, animated), slot);
            wake.request_repaint();
        }) {
            finish_decode(&result, Err(error.to_string()), slot);
            ctx.request_repaint();
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ctx;
        finish_decode(&result, decode(bytes, animated), slot);
    }
    result
}

fn finish_decode(result: &OnceLock<Result<Arc<Media>, String>>, decoded: Result<Media, String>, slot: usize) {
    if let Err(error) = &decoded {
        log::warn!("command hint media {slot}: {error}");
    }
    let _ = result.set(decoded.map(Arc::new));
}

fn limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_WIDTH);
    limits.max_image_height = Some(MAX_HEIGHT);
    limits.max_alloc = Some(MAX_DECODED_BYTES as u64);
    limits
}

fn checked_image_size(width: u32, height: u32) -> Result<[usize; 2], String> {
    if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
        return Err("image dimensions exceed hint limits".into());
    }
    let width = usize::try_from(width).map_err(|error| error.to_string())?;
    let height = usize::try_from(height).map_err(|error| error.to_string())?;
    Ok([width, height])
}

fn check_budget(frames: usize, bytes: usize) -> Result<(), String> {
    if frames > MAX_FRAMES || bytes > MAX_DECODED_BYTES {
        return Err("decoded hint exceeds frame or memory limits".into());
    }
    Ok(())
}

fn decode(bytes: &[u8], animated: bool) -> Result<Media, String> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err("encoded hint exceeds byte limit".into());
    }
    if animated {
        #[cfg(not(target_arch = "wasm32"))]
        return decode_animation(bytes);
        #[cfg(target_arch = "wasm32")]
        return Err("animated hints are unavailable in the browser".into());
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    reader.limits(limits());
    let image = reader.decode().map_err(|error| error.to_string())?.into_rgba8();
    let size = checked_image_size(image.width(), image.height())?;
    check_budget(1, image.len())?;
    Ok(Media { frames: vec![Arc::new(egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()))], frame_ends: vec![0.0] })
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_animation(bytes: &[u8]) -> Result<Media, String> {
    use image::{AnimationDecoder, ImageDecoder};

    let mut decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(|error| error.to_string())?;
    decoder.set_limits(limits()).map_err(|error| error.to_string())?;
    checked_image_size(decoder.dimensions().0, decoder.dimensions().1)?;
    let mut media = Media { frames: Vec::new(), frame_ends: Vec::new() };
    let mut retained = 0usize;
    let mut end = 0.0;
    for frame in decoder.into_frames().take(MAX_FRAMES + 1) {
        let frame = frame.map_err(|error| error.to_string())?;
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        if numerator == 0 || denominator == 0 {
            return Err("hint frame has an invalid delay".into());
        }
        end += f64::from(numerator) / f64::from(denominator) / 1000.0;
        if !end.is_finite() || end > MAX_ANIMATION_SECONDS {
            return Err("hint animation exceeds duration limit".into());
        }
        let image = frame.into_buffer();
        let size = checked_image_size(image.width(), image.height())?;
        retained = retained.checked_add(image.len()).ok_or("hint byte count overflow")?;
        check_budget(media.frames.len() + 1, retained)?;
        media.frames.push(Arc::new(egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw())));
        media.frame_ends.push(end);
    }
    if media.frames.is_empty() {
        return Err("hint animation has no frames".into());
    }
    Ok(media)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sketch_creators_explain_construction_with_native_animation_and_editing_guides() {
        let source: serde_json::Value =
            serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/command-hints/source/sketch-construction.json")))
                .unwrap();
        assert_eq!(source["frame_delay_cs"], 25, "four native frames per one-second input");
        assert_eq!(source["final_source_delay_cs"], 100);
        let samples = source["samples"].as_array().unwrap();
        assert_eq!(samples.len(), 22);
        let mut commands = BTreeSet::new();
        for sample in samples {
            let command = sample["command"].as_str().unwrap();
            assert!(commands.insert(command), "duplicate construction sequence: {command}");
            let entry = CATALOG.iter().find(|entry| entry.command == command).unwrap();
            assert!(entry.animation.is_some(), "{command} needs its building sequence");
            let events = sample["events"].as_array().unwrap();
            assert!((2..=7).contains(&events.len()), "{command}: bounded input sequence");
            let index = CATALOG.iter().position(|entry| entry.command == command).unwrap();
            for dark in [false, true] {
                let animated = asset(index, dark, true).unwrap();
                let media = decode(animated.bytes, true).unwrap();
                assert!((media.frame_ends.last().unwrap() - (events.len() as f64 + 1.25)).abs() < 0.02, "{command}: one input per second");
            }
        }
        for command in ["sketch.line", "sketch.line.midpoint"] {
            assert!(commands.contains(command), "{command} needs a construction sequence");
        }
        for command in ["sketch.break", "sketch.dimension"] {
            assert!(available(command), "{command} needs a native editing guide");
        }
    }

    #[test]
    fn every_covered_menu_command_has_an_explicit_illustration_decision() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/command-hints/source/coverage.json"))).unwrap();
        let tabs = manifest["tabs"].as_object().unwrap();
        let expected_tabs: BTreeSet<_> = ["ASSEMBLE", "INSPECT", "MODEL", "SHEET METAL", "SKETCH", "SURFACE"].into_iter().collect();
        assert_eq!(tabs.keys().map(String::as_str).collect::<BTreeSet<_>>(), expected_tabs);
        let illustrated: BTreeSet<_> = CATALOG.iter().map(|entry| entry.command).collect();
        assert_eq!(illustrated.len(), CATALOG.len(), "duplicate command illustrations");
        assert!(CATALOG.windows(2).all(|entries| entries[0].command < entries[1].command));
        let mut referenced = BTreeSet::new();
        for (tab, decisions) in tabs {
            let decisions = decisions.as_object().unwrap();
            let actual: BTreeSet<_> = crate::toolbar_custom::panel_lists(tab)
                .iter()
                .flat_map(|(panel, commands, _)| crate::workspace::grouped_commands(tab, panel, commands).into_iter().map(|(_, command)| command.id))
                .collect();
            assert_eq!(decisions.keys().map(String::as_str).collect::<BTreeSet<_>>(), actual, "{tab}: stale or missing decisions");
            for (command, decision) in decisions {
                assert!(solidcraft_engine::find_command(command).is_some(), "{command} is not registered");
                if let Some(slug) = decision["illustration"].as_str() {
                    assert!(decision.get("text_only").is_none(), "{command} has conflicting decisions");
                    assert!(!slug.is_empty() && slug.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_".contains(&b)));
                    let entry = CATALOG.iter().find(|entry| entry.command == command).expect("missing catalog entry");
                    for (index, theme) in ["light", "dark"].into_iter().enumerate() {
                        let path =
                            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../assets/command-hints/exports/{slug}-{theme}.png"));
                        assert!(entry.posters[index] == std::fs::read(path).unwrap().as_slice(), "{command}: mismatched {theme} poster");
                    }
                    referenced.insert(command.as_str());
                } else {
                    assert!(decision["text_only"].as_str().is_some_and(|reason| !reason.is_empty()), "{command}: missing rationale");
                    assert!(!illustrated.contains(command.as_str()), "{command} has conflicting illustration decisions");
                }
            }
        }
        assert_eq!(referenced, illustrated, "unreferenced illustrations");
    }

    #[test]
    fn cache_reuses_recent_media_and_evicts_the_oldest_finished_asset() {
        let mut cache = Cache::default();
        for key in 0..CACHE_SLOTS {
            cache.prepare(key, ready_task);
        }
        cache.prepare(0, || panic!("a cache hit must reuse its decoder"));
        cache.prepare(CACHE_SLOTS, ready_task);
        assert_eq!(cache.slots.len(), CACHE_SLOTS);
        assert!(cache.slots.iter().any(|slot| slot.key == 0));
        assert!(!cache.slots.iter().any(|slot| slot.key == 1));
        assert_eq!(cache.slots.back().unwrap().key, CACHE_SLOTS);
    }

    #[test]
    fn pending_decodes_retain_slots_and_bound_active_workers() {
        let mut cache = Cache::default();
        for key in 0..CACHE_SLOTS {
            cache.prepare(key, || Arc::new(OnceLock::new()));
        }
        assert_eq!(cache.prepare(CACHE_SLOTS, || panic!("a full pending cache must not start another worker")), None);
        let oldest = cache.slots.front().unwrap();
        assert!(oldest.decoded.set(Ok(Arc::new(Media { frames: vec![pixel()], frame_ends: vec![0.0] }))).is_ok());
        assert!(cache.prepare(CACHE_SLOTS, ready_task).is_some());
        assert_eq!(cache.slots.len(), CACHE_SLOTS);
        assert!(!cache.slots.iter().any(|slot| slot.key == 0));
    }

    #[test]
    fn failed_assets_are_remembered_after_eviction_without_redecoding() {
        let mut cache = Cache::default();
        cache.prepare(0, || Arc::new(OnceLock::from(Err("invalid media".into()))));
        for key in 1..CACHE_SLOTS {
            cache.prepare(key, ready_task);
        }
        cache.prepare(CACHE_SLOTS, ready_task);
        assert!(cache.failed.contains(&0));
        assert_eq!(cache.prepare(0, || panic!("a failed asset must not be decoded or logged again")), None);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_failed_animation_uses_its_static_poster() {
        let index = CATALOG.iter().position(|entry| entry.command == "solid.extrude").unwrap();
        let animation = asset(index, false, true).unwrap();
        let poster = asset(index, false, false).unwrap();
        let mut cache = Cache::default();
        cache.failed.insert(animation.key);
        cache.prepare(poster.key, ready_task);
        let (selected, slot) = cache.prepare_asset(&egui::Context::default(), index, false, true).unwrap();
        assert!(!selected.animated);
        assert_eq!(selected.key, poster.key);
        assert!(slot.is_some());
    }

    #[test]
    fn timing_advances_and_replays_after_three_seconds_on_the_result() {
        let media = Media { frames: vec![pixel(), pixel(), pixel()], frame_ends: vec![0.1, 0.3, 0.5] };
        assert_eq!(media.frame_at(0.0), (0, Some(0.1)));
        let (index, next) = media.frame_at(0.1);
        assert_eq!(index, 1);
        assert!((next.unwrap() - 0.2).abs() < f64::EPSILON);
        assert_eq!(media.frame_at(0.3), (2, Some(3.0)));
        let (index, next) = media.frame_at(3.29);
        assert_eq!(index, 2);
        assert!((next.unwrap() - 0.01).abs() < 1e-12);
        assert_eq!(media.frame_at(3.3), (0, Some(0.1)));
        assert_eq!(media.frame_at(6.6), (0, Some(0.1)));
        assert_eq!(media.frame_at(f64::NAN), (0, Some(0.1)));
    }

    #[test]
    fn still_posters_do_not_schedule_replays() {
        let media = Media { frames: vec![pixel()], frame_ends: vec![0.0] };
        assert_eq!(media.frame_at(0.0), (0, None));
        assert_eq!(media.frame_at(50.0), (0, None));
    }

    #[test]
    fn playback_restarts_after_a_gap_or_a_different_hover_scope() {
        let a = egui::Id::new("a");
        let b = egui::Id::new("b");
        let mut playback = Playback::default();
        assert_eq!(playback.elapsed(a, 10, 5.0), 0.0);
        assert!((playback.elapsed(a, 11, 5.2) - 0.2).abs() < f64::EPSILON);
        assert_eq!(playback.elapsed(a, 13, 5.5), 0.0);
        assert_eq!(playback.elapsed(b, 14, 5.7), 0.0);
        assert_eq!(playback.elapsed(b, 14, 5.7), 0.0);
    }

    #[test]
    fn decoder_rejects_hostile_sizes_frames_and_broken_inputs() {
        assert!(decode(&vec![0; MAX_INPUT_BYTES + 1], false).is_err());
        assert!(decode(b"broken", false).is_err());
        assert!(decode(b"GIF89a", true).is_err());
        assert!(checked_image_size(MAX_WIDTH + 1, 1).is_err());
        assert!(checked_image_size(1, MAX_HEIGHT + 1).is_err());
        assert!(checked_image_size(0, 1).is_err());
        assert!(check_budget(MAX_FRAMES + 1, 1).is_err());
        assert!(check_budget(1, MAX_DECODED_BYTES + 1).is_err());
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(MAX_WIDTH + 1, 1).write_to(&mut png, image::ImageFormat::Png).unwrap();
        assert!(decode(png.get_ref(), false).is_err());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn gif_decoder_rejects_excess_frames_and_duration() {
        fn animation(count: usize, delay_ms: u32) -> Vec<u8> {
            let mut bytes = Vec::new();
            {
                let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
                for _ in 0..count {
                    let frame = image::Frame::from_parts(image::RgbaImage::new(1, 1), 0, 0, image::Delay::from_numer_denom_ms(delay_ms, 1));
                    encoder.encode_frame(frame).unwrap();
                }
            }
            bytes
        }
        assert!(decode(&animation(MAX_FRAMES + 1, 100), true).is_err());
        assert!(decode(&animation(1, 11_000), true).is_err());
        assert!(decode(&animation(1, 0), true).is_err());
    }

    #[test]
    fn catalog_has_paired_shipped_posters_and_bounded_animation() {
        assert!(CATALOG.windows(2).all(|entries| entries[0].command < entries[1].command));
        for (index, entry) in CATALOG.iter().enumerate() {
            assert!(available(entry.command));
            for dark in [false, true] {
                let asset = asset(index, dark, false).unwrap();
                let media = decode(asset.bytes, asset.animated).unwrap();
                assert_eq!(media.frames.len(), 1);
                assert_eq!(media.frames[0].size, [960, 600]);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        for (index, entry) in CATALOG.iter().enumerate().filter(|(_, entry)| entry.animation.is_some()) {
            for dark in [false, true] {
                let asset = asset(index, dark, true).unwrap();
                let media = decode(asset.bytes, asset.animated).unwrap();
                assert!((2..=MAX_FRAMES).contains(&media.frames.len()), "{}", entry.command);
                assert!(media.frames.iter().all(|image| image.size == [480, 300]));
                if entry.command == "solid.extrude" {
                    assert_eq!(media.frames.len(), 26);
                    assert!((media.frame_ends.last().copied().unwrap() - 4.0).abs() < 0.02);
                }
            }
        }
        assert!(!available("solid.unknown"));
    }

    fn pixel() -> Arc<egui::ColorImage> {
        Arc::new(egui::ColorImage::filled([1, 1], egui::Color32::TRANSPARENT))
    }

    fn ready_task() -> DecodeTask {
        Arc::new(OnceLock::from(Ok(Arc::new(Media { frames: vec![pixel()], frame_ends: vec![0.0] }))))
    }
}

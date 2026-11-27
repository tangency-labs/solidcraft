//! The one picture of a design: the model cut out over transparent pixels, seen from the
//! isometric home view through an orthographic camera and fitted to its outline. A save keeps it
//! in the file, and the app draws the same picture for Home and the parts library, over a grid in
//! the theme's colours; a mesh export takes a square one.
//!
//! The picture also carries the model's footprint, for the shadow the app draws on its grid: the
//! model dropped onto its lowest level, as a small grey mask in a private PNG chunk. Other
//! readers skip the chunk, and a picture without one simply casts no shadow.
use crate::Session;
use solidcore_record::Vec3;
use solidcraft_geom::{Mesh, Vec3Geom};
use solidcraft_viewport::{Camera, Footprint, Rgb, Scene, SceneLine, SceneMesh, StandardView};
use std::sync::Arc;
use web_time::{Duration, Instant};

/// The picture's width and height: what a save keeps.
pub const SIZE: [usize; 2] = solidcraft_io::THUMBNAIL_SIZE;

/// The side of a mesh export's square thumbnail, as slicers write theirs.
const EXPORT_SIZE: usize = 512;

/// The clear margin around the model, as a part of the picture's side.
const MARGIN: f64 = 0.06;

/// A model with more triangles than this has no picture: drawing it would hold up a save.
const MAX_TRIANGLES: usize = 1_000_000;
const MAX_BODIES: usize = 1024;
/// A design with more occurrences than this is not placed for its picture alone.
const MAX_UNPLACED: usize = 256;
/// How long meshing bodies not yet drawn may take before the picture is given up. Looked at
/// before each body, so one body's mesh can run past it.
const MESHING: Duration = Duration::from_secs(5);

/// The edges' width in the picture's pixels: it is shown at half this size or less.
const EDGE_WIDTH: f64 = 2.0;
const SKETCH_WIDTH: f64 = 3.0;

/// The footprint mask's width and height. It covers more than the picture, since a part's
/// footprint can reach below it: [`SHADOW_AREA`].
pub const SHADOW_SIZE: [usize; 2] = [160, 128];
/// The area the mask covers, in the picture's pixels: left, top, width and height.
pub const SHADOW_AREA: [f64; 4] = [-64.0, 0.0, 640.0, 512.0];
/// The private, ancillary, safe-to-copy PNG chunk that holds the mask.
const SHADOW_CHUNK: &[u8; 4] = b"scSh";
const MAX_SHADOW_BYTES: usize = 32 << 10;
const FOOTPRINT: Footprint = Footprint { picture: SIZE, mask: SHADOW_SIZE, area: SHADOW_AREA };

/// A design's picture: straight RGBA of [`SIZE`], rows top to bottom, and its footprint mask
/// of [`SHADOW_SIZE`].
pub struct Picture {
    pub rgba: Vec<u8>,
    pub shadow: Option<Vec<u8>>,
}

/// The design's picture: the shown bodies, or the shown sketches of a design without bodies.
/// `None` for a design with neither, or one too large to draw.
pub fn render(s: &Session) -> Option<Picture> {
    let (scene, camera) = scene(s, SIZE, None)?;
    let shadow = solidcraft_viewport::footprint(&scene, &camera, FOOTPRINT);
    Some(Picture { rgba: draw(scene, &camera, SIZE)?, shadow })
}

/// The same picture of `meshes` that are not a session's: a file previewed while it loads, from
/// its saved display meshes or as far as it is built. `fits` may refuse the framed scene as too
/// much to draw.
pub fn render_meshes(meshes: Vec<SceneMesh>, fits: impl FnOnce(&Scene, &Camera) -> bool) -> Option<Picture> {
    let mut scene = Scene { meshes, ..Default::default() };
    let (camera, radius) = framed(|| scene.meshes.iter().flat_map(|m| m.mesh.positions.iter().copied()), SIZE)?;
    scene.radius = radius;
    if !fits(&scene, &camera) {
        return None;
    }
    let shadow = solidcraft_viewport::footprint(&scene, &camera, FOOTPRINT);
    Some(Picture { rgba: draw(scene, &camera, SIZE)?, shadow })
}

/// [`render`] as the PNG a save keeps.
pub fn png(s: &Session) -> Option<Vec<u8>> {
    encode(&render(s)?)
}

/// `picture` as PNG, with its footprint in the private chunk.
pub fn encode(picture: &Picture) -> Option<Vec<u8>> {
    let png = solidcraft_viewport::png(SIZE[0], SIZE[1], picture.rgba.clone())?;
    match picture.shadow.as_deref().and_then(grey_png) {
        Some(mask) if mask.len() <= MAX_SHADOW_BYTES => with_chunk(png, SHADOW_CHUNK, &mask),
        _ => Some(png),
    }
}

/// The footprint mask a picture carries, if it has a well formed one. The bytes are hostile: any
/// file can hold a picture.
pub fn shadow(png: &[u8]) -> Option<Vec<u8>> {
    let mut at = 8usize;
    loop {
        let header = png.get(at..at.checked_add(8)?)?;
        let length = u32::from_be_bytes(header.get(..4)?.try_into().ok()?) as usize;
        let data = at.checked_add(8)?;
        if header.get(4..)? == SHADOW_CHUNK {
            return (length <= MAX_SHADOW_BYTES).then(|| read_grey(png.get(data..data.checked_add(length)?)?)).flatten();
        }
        at = data.checked_add(length)?.checked_add(4)?;
    }
}

/// `png` with a chunk `name` holding `data` ahead of its closing chunk.
fn with_chunk(mut png: Vec<u8>, name: &[u8; 4], data: &[u8]) -> Option<Vec<u8>> {
    // The closing chunk is always these twelve bytes.
    let end = png.len().checked_sub(12)?;
    let closing = png.split_off(end);
    png.extend(u32::try_from(data.len()).ok()?.to_be_bytes());
    let mut checksum = crc32fast::Hasher::new();
    for part in [name.as_slice(), data] {
        checksum.update(part);
        png.extend_from_slice(part);
    }
    png.extend(checksum.finalize().to_be_bytes());
    png.extend(closing);
    Some(png)
}

fn grey_png(mask: &[u8]) -> Option<Vec<u8>> {
    let image = image::GrayImage::from_raw(u32::try_from(SHADOW_SIZE[0]).ok()?, u32::try_from(SHADOW_SIZE[1]).ok()?, mask.to_vec())?;
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

fn read_grey(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = u32::try_from(SHADOW_SIZE[0]).ok();
    limits.max_image_height = u32::try_from(SHADOW_SIZE[1]).ok();
    limits.max_alloc = Some((SHADOW_SIZE[0] * SHADOW_SIZE[1] * 8) as u64);
    reader.limits(limits);
    let mask = reader.decode().ok()?.into_luma8();
    (mask.dimensions() == (SHADOW_SIZE[0] as u32, SHADOW_SIZE[1] as u32)).then(|| mask.into_raw())
}

/// A mesh export's square thumbnail of the exported `bodies` (none named, every body).
pub(crate) fn exported(s: &Session, bodies: &[String]) -> Option<Vec<u8>> {
    let size = [EXPORT_SIZE; 2];
    let (scene, camera) = scene(s, size, Some(bodies))?;
    solidcraft_viewport::png(EXPORT_SIZE, EXPORT_SIZE, draw(scene, &camera, size)?)
}

/// `scene` over transparent pixels, by the renderer the host installed (the GPU), or by the CPU
/// rasterizer in a process with none (a headless run), so that every save has its picture.
fn draw(mut scene: Scene, camera: &Camera, [width, height]: [usize; 2]) -> Option<Vec<u8>> {
    let rgba = match crate::capture::installed() {
        Some(renderer) => {
            let (w, h) = (u32::try_from(width).ok()?, u32::try_from(height).ok()?);
            crate::capture::rgba_transparent_with(Some(&*renderer), &mut scene, camera, w, h).ok()?.rgba
        }
        None => solidcraft_viewport::render_transparent(&scene, camera, width, height).rgba,
    };
    rgba.iter().skip(3).step_by(4).any(|alpha| *alpha != 0).then_some(rgba)
}

/// What the picture shows and the camera that frames it. `exported` names the bodies a mesh
/// export holds (none named, every body); without it, the shown bodies.
fn scene(s: &Session, size: [usize; 2], exported: Option<&[String]>) -> Option<(Scene, Camera)> {
    // Not while a joint drag shows a frame: the world placed for it is not the design's.
    if !s.pending_moves.is_empty() || !s.drag_moves().is_empty() {
        return None;
    }
    // The placed model as it stands; a small design nothing has placed yet is placed here.
    let world = match s.cached_world_state() {
        Some(world) => world,
        None if s.doc.occurrences.len() <= MAX_UNPLACED => s.world_state().ok()?,
        None => return None,
    };
    let pictured = |name: &String| match exported {
        Some(bodies) => bodies.is_empty() || bodies.contains(name),
        None => s.body_visible(name),
    };
    let bodies: Vec<_> = world.bodies.iter().filter(|b| pictured(&b.name)).collect();
    if bodies.len() > MAX_BODIES {
        return None;
    }
    let mut scene = Scene::default();
    let began = Instant::now();
    let mut triangles = 0usize;
    for body in bodies {
        if began.elapsed() > MESHING {
            return None;
        }
        // Counted on the definition's own mesh first, so that no placed copy is made of a model
        // too large to draw.
        triangles = triangles.saturating_add(body.mesh_in_place().0.triangles.len());
        if triangles > MAX_TRIANGLES {
            return None;
        }
        let mesh: Arc<Mesh> = body.mesh_uncached();
        let channel = |x: f32| (x.clamp(0., 1.) * 255.).round() as u8;
        let color = s.doc.body_color(body).map_or(crate::view::colors::BODY, |[r, g, b]| Rgb(channel(r), channel(g), channel(b)));
        let face_colors =
            s.doc.face_colors(body).into_iter().map(|(face, look)| (face as u32, Rgb(look.color[0], look.color[1], look.color[2]))).collect();
        let opacity = s.doc.model_body_opacity(body) as f32;
        for (i, edge) in mesh.edges.iter().enumerate() {
            if mesh.shaded_edge_visible(i) {
                scene.lines.push(SceneLine { points: edge.clone(), color: crate::view::colors::EDGE, alpha: 1.0, width: EDGE_WIDTH, on_top: false });
            }
        }
        scene.meshes.push(SceneMesh { mesh, color, face_colors, opacity, closed: body.is_closed(), hatch: Default::default(), values: None });
    }
    if scene.meshes.is_empty() && exported.is_none() {
        scene.lines = sketch_lines(s)?;
    }
    let points = || {
        let drawn = scene.meshes.iter().flat_map(|m| m.mesh.positions.iter().copied());
        drawn.chain(scene.lines.iter().flat_map(|l| l.points.iter().copied()))
    };
    let (camera, radius) = framed(points, size)?;
    scene.radius = radius;
    Some((scene, camera))
}

/// The shown sketches' curves, for a design with no body to show.
fn sketch_lines(s: &Session) -> Option<Vec<SceneLine>> {
    let guides = crate::view::Guides { grid: false, axes: false };
    let mut lines = crate::view::scene_with_guides(s, &Camera::default(), &[], guides).ok()?.lines;
    for line in &mut lines {
        line.width = SKETCH_WIDTH;
        line.on_top = false;
    }
    Some(lines)
}

/// The isometric orthographic camera that centres `points` in a picture of `size` with its
/// margin, and the radius of what it frames. `None` with nothing to frame.
fn framed<I: Iterator<Item = Vec3>>(points: impl Fn() -> I, [width, height]: [usize; 2]) -> Option<(Camera, f64)> {
    let mut camera = Camera::default();
    camera.set_view(StandardView::Iso);
    camera.fov = 0.0;
    let (right, up, _) = camera.basis();
    let dot = |a: Vec3, b: Vec3| a.x * b.x + a.y * b.y + a.z * b.z;
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    let (mut low, mut high) =
        (Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY));
    for p in points().filter(|p| p.is_finite()) {
        for (k, axis) in [right, up].into_iter().enumerate() {
            lo[k] = lo[k].min(dot(p, axis));
            hi[k] = hi[k].max(dot(p, axis));
        }
        low = Vec3::new(low.x.min(p.x), low.y.min(p.y), low.z.min(p.z));
        high = Vec3::new(high.x.max(p.x), high.y.max(p.y), high.z.max(p.z));
    }
    if !(lo[0].is_finite() && lo[1].is_finite() && low.is_finite() && high.is_finite()) {
        return None;
    }
    let centre = (low + high) * 0.5;
    camera.target = centre + right * ((lo[0] + hi[0]) * 0.5 - dot(centre, right)) + up * ((lo[1] + hi[1]) * 0.5 - dot(centre, up));
    // The same margin in pixels on every side: a part of the shorter side.
    let margin = MARGIN * width.min(height) as f64;
    let room = |side: usize| (side as f64 - 2.0 * margin).max(1.0);
    // World units to a pixel, so that the wider of the two extents fills its room.
    let scale = ((hi[0] - lo[0]) / room(width)).max((hi[1] - lo[1]) / room(height));
    // An orthographic camera's half height is its distance times 0.4 (`Camera::half_height`).
    camera.distance = (scale * height as f64 * 0.5).max(1e-6) / 0.4;
    let radius = ((high - low).len2().sqrt() * 0.5).max(camera.half_height()).max(10.0);
    camera.is_valid().then_some((camera, radius))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn boxed(length: u32, width: u32, height: u32) -> Session {
        let mut session = Session::default();
        session.execute("solid.box", &json!({"length":length,"width":width,"height":height})).unwrap();
        session
    }

    /// The columns and rows that hold anything: (left, top, right, bottom), exclusive.
    fn drawn(rgba: &[u8], width: usize) -> (usize, usize, usize, usize) {
        let mut b = (usize::MAX, usize::MAX, 0, 0);
        for (i, px) in rgba.chunks(4).enumerate() {
            if px[3] > 32 {
                let (x, y) = (i % width, i / width);
                b = (b.0.min(x), b.1.min(y), b.2.max(x + 1), b.3.max(y + 1));
            }
        }
        b
    }

    #[test]
    fn a_picture_is_a_centred_cut_out_that_fills_its_frame() {
        let [width, height] = SIZE;
        // Wide, tall and cubic parts: each fills the side that limits it, up to the margin.
        for (l, w, h) in [(200, 14, 8), (10, 10, 120), (30, 30, 30)] {
            let rgba = draw_cpu(&boxed(l, w, h));
            let (left, top, right, bottom) = drawn(&rgba, width);
            assert!(rgba.chunks(4).next().unwrap()[3] == 0, "the corner is clear");
            let margin = (MARGIN * height as f64).round() as usize;
            let tight = |near: usize, far: usize, side: usize| near.abs_diff(margin) <= 4 && (side - far).abs_diff(margin) <= 4;
            assert!(tight(left, right, width) || tight(top, bottom, height), "{l}x{w}x{h}: {left},{top},{right},{bottom}");
            assert!(left.abs_diff(width - right) <= 4 && top.abs_diff(height - bottom) <= 4, "{l}x{w}x{h} is centred");
        }
    }

    /// The CPU picture whatever renderer another test has installed.
    fn draw_cpu(s: &Session) -> Vec<u8> {
        let (scene, camera) = scene(s, SIZE, None).unwrap();
        solidcraft_viewport::render_transparent(&scene, &camera, SIZE[0], SIZE[1]).rgba
    }

    #[test]
    fn a_save_keeps_the_picture_without_a_viewport_having_meshed_the_bodies() {
        let session = boxed(10, 20, 30);
        assert!(session.world_state().unwrap().bodies.iter().all(|b| b.cached_mesh().is_none()));
        let bytes = session.design_container(solidcraft_io::DesignCompression::Fast, false, true).unwrap();
        let png = solidcraft_io::read_design_thumbnail(&bytes).expect("a headless save has a picture");
        assert_eq!(png.get(16..24).unwrap(), [(SIZE[0] as u32).to_be_bytes(), (SIZE[1] as u32).to_be_bytes()].concat());
        let disabled = session.design_container(solidcraft_io::DesignCompression::Fast, false, false).unwrap();
        assert!(solidcraft_io::read_design_thumbnail(&disabled).is_none());
    }

    #[test]
    fn portable_thumbnail_preserves_saved_visibility() {
        let mut session = Session::default();
        session.execute("solid.box", &json!({"length":10,"width":20,"height":30,"body_name":"Visible"})).unwrap();
        session.execute("solid.box", &json!({"length":100,"width":100,"height":100,"body_name":"Hidden"})).unwrap();
        session.execute("browser.visibility", &json!({"items":["Hidden"],"visible":false})).unwrap();
        assert_eq!(scene(&session, SIZE, None).unwrap().0.meshes.len(), 1, "the hidden body is not in the picture");
        let bytes = session.design_container(solidcraft_io::DesignCompression::Fast, false, true).unwrap();
        let doc = solidcraft_io::read_design(&bytes).unwrap();
        assert_eq!(doc.visibility.hidden_bodies, vec!["Hidden"]);
        assert_eq!(doc, *session.design());
        assert!(solidcraft_io::read_design_thumbnail(&bytes).is_some());
    }

    #[test]
    fn a_design_with_only_a_sketch_shows_its_lines_and_an_empty_one_has_no_picture() {
        let mut session = Session::default();
        assert!(scene(&session, SIZE, None).is_none());
        session.execute("sketch.create", &json!({"plane":"XY"})).unwrap();
        session.execute("sketch.rectangle.two_point", &json!({"p0":[-10,-10],"p1":[10,10]})).unwrap();
        let (scene, _) = scene(&session, SIZE, None).unwrap();
        assert!(scene.meshes.is_empty() && !scene.lines.is_empty());
    }

    #[test]
    fn a_picture_carries_the_footprint_under_the_part() {
        // A post: its footprint is a small diamond under it, however tall it stands.
        let png = png(&boxed(10, 10, 60)).unwrap();
        assert!(image::load_from_memory(&png).is_ok(), "any PNG reader opens it");
        let mask = shadow(&png).unwrap();
        let rows: Vec<usize> = (0..SHADOW_SIZE[1]).filter(|y| mask[y * SHADOW_SIZE[0]..(y + 1) * SHADOW_SIZE[0]].iter().any(|v| *v > 0)).collect();
        let to_picture = |row: usize| SHADOW_AREA[1] + row as f64 * SHADOW_AREA[3] / SHADOW_SIZE[1] as f64;
        let (top, bottom) = (to_picture(rows[0]), to_picture(rows[rows.len() - 1] + 1));
        assert!(top > SIZE[1] as f64 * 0.6 && bottom > top && bottom <= SIZE[1] as f64, "{top}..{bottom}");
    }

    #[test]
    fn a_damaged_or_foreign_picture_has_no_shadow() {
        let png = png(&boxed(10, 20, 30)).unwrap();
        for end in [0, 8, 20, 40, png.len() / 2, png.len() - 13] {
            let _ = shadow(&png[..end]);
        }
        assert!(shadow(b"not a picture").is_none());
        let plain = solidcraft_viewport::png(2, 2, vec![255; 16]).unwrap();
        assert!(shadow(&plain).is_none());
        let lying = with_chunk(plain.clone(), SHADOW_CHUNK, b"not a mask").unwrap();
        assert!(shadow(&lying).is_none());
        let mut huge = plain;
        huge[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(shadow(&huge).is_none());
    }

    /// #9449: a selected-body export pictures only those bodies.
    #[test]
    fn an_export_thumbnail_shows_only_the_exported_bodies() {
        let mut session = Session::default();
        session.execute("solid.box", &json!({"length":10,"width":20,"height":30,"body_name":"Exported"})).unwrap();
        session.execute("solid.box", &json!({"length":100,"width":100,"height":100,"body_name":"Omitted"})).unwrap();
        session.execute("browser.visibility", &json!({"items":["Omitted"],"visible":false})).unwrap();
        let exported = ["Exported".to_string()];
        let size = [EXPORT_SIZE; 2];
        assert_eq!(scene(&session, size, Some(&exported)).unwrap().0.meshes.len(), 1, "the omitted body is not in the picture");
        assert_eq!(scene(&session, size, Some(&[])).unwrap().0.meshes.len(), 2, "no selection exports every body, hidden ones too");
    }
}

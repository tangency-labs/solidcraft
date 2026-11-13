//! The surface analysis hooks a surface modeller drives, drawn headless with the viewport's
//! pipelines: the zebra's bars along a direction of the caller's (+Z stands them upright, as the
//! picture always was; +X lays them down, turning the stripes a quarter turn) and its white
//! share; and a mesh coloured by its vertices' values. Skipped, with the reason printed, on a
//! machine without a GPU adapter (`SOLIDCRAFT_REQUIRE_GPU=1` makes that a failure).

use solidcore_record::Vec3;
use solidcraft_color::AnalysisColors;
use solidcraft_engine::geom::Mesh;
use solidcraft_viewport::{AnalysisMode, Camera, Rgb, Scene, SceneAnalysis, SceneMesh, StandardView};
use solidcraft_viewport_gpu::GpuError;
use solidcraft_viewport_gpu::capture::{self, Capturer};
use std::sync::Arc;

fn capturer() -> Option<&'static Capturer> {
    match capture::shared(solidcraft_gpu::env::policy().unwrap_or_else(|e| panic!("{e}"))) {
        Ok(c) => Some(c),
        Err(e @ GpuError::NoAdapter(_)) if std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none() => {
            eprintln!("skipping: {e}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

const SIDE: u32 = 200;

/// A sphere of radius 15 about the origin, 48 rings of 96 quads.
fn sphere() -> Mesh {
    let (rings, around) = (48u32, 96u32);
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    for i in 0..=rings {
        let lat = std::f64::consts::PI * (f64::from(i) / f64::from(rings) - 0.5);
        for j in 0..=around {
            let lon = std::f64::consts::TAU * f64::from(j) / f64::from(around);
            let n = Vec3::new(lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin());
            normals.push(n);
            positions.push(Vec3::new(15.0 * n.x, 15.0 * n.y, 15.0 * n.z));
        }
    }
    let at = |i: u32, j: u32| i * (around + 1) + j;
    let mut triangles = Vec::new();
    for i in 0..rings {
        for j in 0..around {
            triangles.push([at(i, j), at(i, j + 1), at(i + 1, j + 1)]);
            triangles.push([at(i, j), at(i + 1, j + 1), at(i + 1, j)]);
        }
    }
    let tri_face = vec![0; triangles.len()];
    Mesh { positions, normals, triangles, tri_face, ..Default::default() }
}

/// The design's radius the pictures are framed for.
const RADIUS: f64 = 40.0;

/// The front view the pictures are taken from.
fn camera() -> Camera {
    let mut cam = Camera { distance: 62.5, fov: 0.0, ..Default::default() };
    cam.set_view(StandardView::Front);
    cam
}

/// Where world point `p` lands in the picture, in pixels (the camera's own projection: the
/// tests find what they look at where it is drawn, not by an assumed framing).
fn screen(p: Vec3) -> (f64, f64) {
    let (x, y, _) = camera().to_screen(p, f64::from(SIDE), f64::from(SIDE), RADIUS).unwrap();
    (x, y)
}

/// The sphere's centre and radius in the picture, in pixels.
fn disc() -> (f64, f64, f64) {
    let (cx, cy) = screen(Vec3::new(0.0, 0.0, 0.0));
    let (rx, _) = screen(Vec3::new(15.0, 0.0, 0.0));
    (cx, cy, (rx - cx).abs())
}

/// `mesh` (with `values` at its positions) shaded by `mode`, seen from the front.
fn picture(c: &Capturer, mesh: Mesh, values: Option<Vec<f32>>, mode: AnalysisMode) -> Vec<u8> {
    let body = SceneMesh { values: values.map(Arc::new), ..SceneMesh::solid(Arc::new(mesh), Rgb(160, 165, 175)) };
    let scene =
        Scene { meshes: vec![body], radius: RADIUS, analysis: Some(SceneAnalysis { mode, colors: AnalysisColors::DEFAULT }), ..Default::default() };
    c.rgba(&scene, &camera(), SIDE, SIDE).unwrap()
}

fn pixel(img: &[u8], x: u32, y: u32) -> [u8; 4] {
    let at = ((y * SIDE + x) * 4) as usize;
    img.get(at..at + 4).and_then(|p| p.try_into().ok()).unwrap_or_default()
}

/// 1 for a white stripe's pixel, 0 for a black one's, None between.
fn shade(p: [u8; 4]) -> Option<u8> {
    if p[0] > 225 && p[1] > 225 && p[2] > 225 {
        Some(1)
    } else if p[0] < 40 && p[1] < 40 && p[2] < 40 {
        Some(0)
    } else {
        None
    }
}

/// How often the stripes change along the sphere's middle row (`across`) or column, within it.
fn changes(img: &[u8], across: bool) -> usize {
    let (cx, cy, r) = disc();
    let reach = 0.9 * r;
    let mut last = None;
    let mut count = 0;
    for k in 0..(2.0 * reach) as u32 {
        let at = f64::from(k) - reach;
        let (x, y) = if across { (cx + at, cy) } else { (cx, cy + at) };
        let p = pixel(img, x.round() as u32, y.round() as u32);
        if let Some(s) = shade(p) {
            if last.is_some_and(|l| l != s) {
                count += 1;
            }
            last = Some(s);
        }
    }
    count
}

fn zebra(direction: Vec3, width: f64) -> AnalysisMode {
    AnalysisMode::Zebra { stripes: 8.0, direction, width }
}

/// Upright bars (+Z, the picture as it always was) stripe the sphere across the view and not
/// down its middle; bars along X turn the stripes a quarter turn.
#[test]
fn the_bars_direction_turns_the_stripes() {
    let Some(c) = capturer() else { return };
    let upright = picture(c, sphere(), None, zebra(Vec3::new(0.0, 0.0, 1.0), 0.5));
    let (row, column) = (changes(&upright, true), changes(&upright, false));
    assert!(row >= 4 && row > 2 * column, "upright bars: {row} changes across, {column} down");
    let lying = picture(c, sphere(), None, zebra(Vec3::new(1.0, 0.0, 0.0), 0.5));
    let (row, column) = (changes(&lying, true), changes(&lying, false));
    assert!(column >= 4 && column > 2 * row, "bars along X: {row} changes across, {column} down");
}

/// The white share of a period: 0.8 shows more white than 0.2.
#[test]
fn the_white_share_widens_the_white_stripes() {
    let Some(c) = capturer() else { return };
    // White pixels on the sphere only: the background is no stripe (its light part counted
    // as white before).
    let (cx, cy, r) = disc();
    let white = |width: f64| {
        let img = picture(c, sphere(), None, zebra(Vec3::new(0.0, 0.0, 1.0), width));
        let mut count = 0;
        for y in 0..SIDE {
            for x in 0..SIDE {
                let (dx, dy) = (f64::from(x) - cx, f64::from(y) - cy);
                if dx * dx + dy * dy < 0.81 * r * r && shade(pixel(&img, x, y)) == Some(1) {
                    count += 1;
                }
            }
        }
        count
    };
    let (narrow, wide) = (white(0.2), white(0.8));
    assert!(wide > 2 * narrow, "white pixels: {narrow} at 0.2, {wide} at 0.8");
}

/// A 30 mm square facing the view, its values -1 at the left, 1 at the right (0 down the
/// middle), coloured from -1 to 1: flat's colour at the left, the middle colour in the middle,
/// tight's at the right.
#[test]
fn values_ramp_through_the_curvature_colours() {
    let Some(c) = capturer() else { return };
    let quad = Mesh {
        positions: vec![Vec3::new(-15.0, 0.0, -15.0), Vec3::new(15.0, 0.0, -15.0), Vec3::new(15.0, 0.0, 15.0), Vec3::new(-15.0, 0.0, 15.0)],
        normals: vec![Vec3::new(0.0, -1.0, 0.0); 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0, 0],
        ..Default::default()
    };
    let img = picture(c, quad, Some(vec![-1.0, 1.0, 1.0, -1.0]), AnalysisMode::Values { low: -1.0, high: 1.0 });
    let [flat, middle, tight] = AnalysisColors::DEFAULT.curvature.map(|c| c.map(|v| (v * 255.0).round() as u8));
    let near = |p: [u8; 4], c: [u8; 3]| (0..3).all(|k| (i32::from(p[k]) - i32::from(c[k])).abs() < 60);
    // A tenth of the way in from each side (values -0.8 and 0.8: a tenth of the ramp from its
    // ends), and the middle, where the camera draws them.
    let at = |x: f64| {
        let (px, py) = screen(Vec3::new(x, 0.0, 0.0));
        pixel(&img, px.round() as u32, py.round() as u32)
    };
    let (left, mid, right) = (at(-12.0), at(0.0), at(12.0));
    assert!(near(left, flat), "left {left:?}, flat {flat:?}");
    assert!(near(mid, middle), "middle {mid:?}, {middle:?}");
    assert!(near(right, tight), "right {right:?}, tight {tight:?}");
}

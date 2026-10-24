//! CPU rasterizer for headless snapshots: z-buffered, smooth-shaded triangles and depth-tested
//! lines, rendered at 2× and downsampled for anti-aliasing.

use solidcraft_geom::Vec3;

use crate::Camera;
use crate::scene::{Rgb, SECTION_CAP, Scene, png};

/// RGBA pixels plus depth.
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
    depth: Vec<f32>,
}

const SS: usize = 2;
const MAX_DIM: usize = 8192;
/// The rounding the lines' depth test allows for, relative to the depth (a few steps of the
/// `f32` depth buffer).
const DEPTH_EPS: f64 = 4.0 * f32::EPSILON as f64;

impl Canvas {
    fn new(w: usize, h: usize) -> Canvas {
        Canvas { w, h, rgba: vec![255; w * h * 4], depth: vec![f32::INFINITY; w * h] }
    }
    fn put(&mut self, x: usize, y: usize, c: [f64; 3], a: f64) {
        let i = (y * self.w + x) * 4;
        if let Some(px) = self.rgba.get_mut(i..i + 3) {
            for (k, v) in px.iter_mut().enumerate() {
                let old = *v as f64 / 255.0;
                *v = ((old * (1.0 - a) + c[k] * a).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
}

/// The order a scene's bodies are drawn in. Solid bodies come first and own the depth buffer;
/// the see-through ones are then blended over them, far sides before near sides, so a
/// translucent body reads like glass and whatever lies inside it shows through. This is the
/// order the GPU viewport draws its own glass in.
///
/// Splitting each see-through body by facing orders its own two layers, which is what a single
/// translucent body needs. Two see-through bodies that overlap on screen blend in the order the
/// scene lists them, where the GPU sorts its glass triangles back to front: the difference shows
/// only where translucent bodies lie in front of each other, which the CPU rasterizer (a
/// fallback for machines without a GPU) does not try to match.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    Solid,
    GlassFar,
    GlassNear,
}

impl Pass {
    /// Does this pass draw the see-through bodies ([`SceneMesh::glass`])?
    fn glass(self) -> bool {
        self != Pass::Solid
    }

    /// Does it draw a triangle whose facing is `inside` (away from the eye)? A solid body draws
    /// both: its back faces show only where a section opens it.
    fn takes(self, inside: bool) -> bool {
        match self {
            Pass::Solid => true,
            Pass::GlassFar => inside,
            Pass::GlassNear => !inside,
        }
    }
}

pub(crate) fn shade(n: Vec3, view_dir: Vec3, base: [f64; 3]) -> [f64; 3] {
    let n = if n.dot(view_dir) < 0.0 { -n } else { n };
    let key = (view_dir * 0.8 + Vec3::new(-0.3, 0.2, 0.6)).normalized().unwrap_or(view_dir);
    let diff = n.dot(key).max(0.0);
    let fill = n.dot(view_dir).max(0.0);
    let h = (key + view_dir).normalized().unwrap_or(view_dir);
    let spec = n.dot(h).max(0.0).powi(40) * 0.25;
    let k = 0.42 + 0.38 * diff + 0.25 * fill;
    [(base[0] * k + spec).min(1.0), (base[1] * k + spec).min(1.0), (base[2] * k + spec).min(1.0)]
}

/// Render a scene with the camera into an RGBA canvas of `w × h` pixels.
pub fn render(scene: &Scene, cam: &Camera, w: usize, h: usize) -> Canvas {
    let (w, h) = (w.clamp(1, MAX_DIM), h.clamp(1, MAX_DIM));
    let (sw, sh) = (w * SS, h * SS);
    let mut c = Canvas::new(sw, sh);
    let (top, bot) = scene.background.unwrap_or((Rgb(236, 240, 245), Rgb(205, 212, 222)));
    for y in 0..sh {
        let t = y as f64 / sh.max(1) as f64;
        let (a, b) = (top.f(), bot.f());
        let col = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
        for x in 0..sw {
            c.put(x, y, col, 1.0);
        }
    }
    let vp = cam.view_proj(sw as f64 / sh as f64, scene.radius.max(1.0));
    let project = |p: Vec3| -> Option<[f64; 3]> {
        let q = vp.apply(p);
        if q[3] <= 1e-9 {
            return None;
        }
        Some([(q[0] / q[3] + 1.0) * 0.5 * sw as f64, (1.0 - q[1] / q[3]) * 0.5 * sh as f64, q[2] / q[3]])
    };
    let view_dir = cam.back();
    // Where the section cap is the visible surface: the depth of the section plane on that
    // pixel's view ray. Written into the depth buffer once the solids are drawn, so the cut face
    // hides edges inside the body (holes, far corners) instead of showing them like a pane of
    // glass, while holes through the cut stay open (as the viewport does with its stencil).
    let cap_z = scene.clip.and_then(|(n, d)| cap_plane(cam, n, d, scene.radius.max(1.0), &project));
    let mut cap_depth = if cap_z.is_some() { vec![f32::INFINITY; sw * sh] } else { Vec::new() };
    // The screen depth slope (per canvas pixel) of the surface each pixel shows, for the lines'
    // depth test.
    let mut slope = vec![0f32; sw * sh];
    let cap_tint = scene.cap.unwrap_or(SECTION_CAP).f();
    for pass in [Pass::Solid, Pass::GlassFar, Pass::GlassNear] {
        for sm in scene.meshes.iter().filter(|m| m.glass() == pass.glass()) {
            let alpha = sm.alpha();
            let m = &sm.mesh;
            let body = sm.color.f();
            let pts: Vec<Option<[f64; 3]>> = m.positions.iter().map(|p| project(*p)).collect();
            // 1 / clip w per vertex, for perspective-correct interpolation of world positions (so
            // the section cuts along the plane, not a curve, in perspective views).
            let inv_w: Vec<f64> = m.positions.iter().map(|p| 1.0 / vp.apply(*p)[3].max(1e-9)).collect();
            for (ti, t) in m.triangles.iter().enumerate() {
                let face = m.tri_face.get(ti).copied();
                let base = sm.face_colors.iter().find(|(f, _)| Some(*f) == face).map_or(body, |(_, c)| c.f());
                let wp = t.map(|k| m.positions.get(k as usize).copied().unwrap_or_default());
                // Seen from inside: where a section cuts a solid body open, and the far side of
                // a see-through one (drawn in the pass before its near side).
                let to_eye = if cam.fov > 0.0 { cam.eye() - wp[0] } else { view_dir };
                let inside = (wp[1] - wp[0]).cross(wp[2] - wp[0]).dot(to_eye) < 0.0;
                if !pass.takes(inside) {
                    continue;
                }
                let iw = t.map(|k| inv_w.get(k as usize).copied().unwrap_or(1.0));
                let (Some(Some(a)), Some(Some(b)), Some(Some(cc))) = (pts.get(t[0] as usize), pts.get(t[1] as usize), pts.get(t[2] as usize)) else {
                    continue;
                };
                let ns: Vec<Vec3> = t.iter().map(|k| m.normals.get(*k as usize).copied().unwrap_or(Vec3::Z)).collect();
                // A curved surface (its normals turn across the triangle) gets a silhouette line
                // where it turns away from the eye.
                let curved = ns.iter().any(|n| n.dist(ns[0]) > 1e-6);
                let (x0, x1) = (a[0].min(b[0]).min(cc[0]).floor().max(0.0), a[0].max(b[0]).max(cc[0]).ceil().min(sw as f64 - 1.0));
                let (y0, y1) = (a[1].min(b[1]).min(cc[1]).floor().max(0.0), a[1].max(b[1]).max(cc[1]).ceil().min(sh as f64 - 1.0));
                if !(x0 <= x1 && y0 <= y1) {
                    continue;
                }
                let area = (b[0] - a[0]) * (cc[1] - a[1]) - (b[1] - a[1]) * (cc[0] - a[0]);
                if area.abs() < 1e-12 {
                    continue;
                }
                let gx = ((b[2] - a[2]) * (cc[1] - a[1]) - (cc[2] - a[2]) * (b[1] - a[1])) / area;
                let gy = ((cc[2] - a[2]) * (b[0] - a[0]) - (b[2] - a[2]) * (cc[0] - a[0])) / area;
                let tri_slope = gx.hypot(gy) as f32;
                for y in y0 as usize..=y1 as usize {
                    for x in x0 as usize..=x1 as usize {
                        let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                        let w0 = ((b[0] - px) * (cc[1] - py) - (b[1] - py) * (cc[0] - px)) / area;
                        let w1 = ((cc[0] - px) * (a[1] - py) - (cc[1] - py) * (a[0] - px)) / area;
                        let w2 = 1.0 - w0 - w1;
                        if w0 < -1e-9 || w1 < -1e-9 || w2 < -1e-9 {
                            continue;
                        }
                        let (q0, q1, q2) = (w0 * iw[0], w1 * iw[1], w2 * iw[2]);
                        let qs = q0 + q1 + q2;
                        let (p0, p1, p2) = if qs > 1e-12 { (q0 / qs, q1 / qs, q2 / qs) } else { (w0, w1, w2) };
                        if let Some((cn, cd)) = scene.clip
                            && cn.dot(wp[0] * p0 + wp[1] * p1 + wp[2] * p2) > cd
                        {
                            continue;
                        }
                        let z = (w0 * a[2] + w1 * b[2] + w2 * cc[2]) as f32;
                        let i = y * sw + x;
                        if c.depth.get(i).is_some_and(|d| z < *d) {
                            // A see-through body is blended over what is already drawn and
                            // leaves the depth buffer alone, so bodies inside it stay visible
                            // and the lines' depth test still answers for the solids.
                            if !pass.glass() {
                                if let Some(d) = c.depth.get_mut(i) {
                                    *d = z;
                                }
                                if let Some(s) = slope.get_mut(i) {
                                    *s = tri_slope;
                                }
                            }
                            let n = (ns[0] * p0 + ns[1] * p1 + ns[2] * p2).normalized().unwrap_or(Vec3::Z);
                            // A see-through body has no cut face: the interior already shows.
                            let cut = !pass.glass() && scene.clip.is_some() && inside;
                            if let Some(d) = cap_depth.get_mut(i).filter(|_| !pass.glass()) {
                                *d = match cap_z {
                                    Some([a, b, k]) if cut => ((a * px + b * py + k) as f32).min(z),
                                    _ => f32::INFINITY,
                                };
                            }
                            if cut {
                                // The cut: the body's colour tinted toward the cap colour, hatched
                                // at 45 degrees (as the viewport draws it); a plain cap is the
                                // body's own colour.
                                if scene.plain_cap {
                                    c.put(x, y, base, 1.0);
                                    continue;
                                }
                                let tinted: [f64; 3] = std::array::from_fn(|k| base[k] + (cap_tint[k] - base[k]) * 0.35);
                                let k = if ((x + y) / SS) % 9 < 2 { 0.6 } else { 1.0 };
                                c.put(x, y, tinted.map(|v| v * k), 1.0);
                                continue;
                            }
                            let mut col = shade(n, view_dir, base);
                            let ndv = n.dot(view_dir).abs();
                            if curved && ndv < 0.14 {
                                let a = 1.0 - ndv / 0.14;
                                for (k, e) in col.iter_mut().zip([0.07, 0.08, 0.10]) {
                                    *k = *k * (1.0 - a) + e * a;
                                }
                            }
                            c.put(x, y, col, alpha);
                        }
                    }
                }
            }
        }
    }
    let cap_slope = cap_z.map_or(0.0, |[a, b, _]| a.hypot(b) as f32);
    for ((d, s), cz) in c.depth.iter_mut().zip(slope.iter_mut()).zip(&cap_depth) {
        if cz.is_finite() {
            *d = *cz;
            *s = s.max(cap_slope);
        }
    }
    for l in scene.grid.iter().chain(&scene.lines) {
        let col = l.color.f();
        let half = (l.width * SS as f64 * 0.5).max(0.5);
        for seg in l.points.windows(2) {
            let (Some(&p), Some(&q)) = (seg.first(), seg.get(1)) else { continue };
            // The section cuts the segment in 3D (screen space is not linear in depth in
            // perspective views).
            let Some((p, q)) = clip_segment(p, q, scene.clip) else { continue };
            let (Some(a), Some(b)) = (project(p), project(q)) else { continue };
            let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            let steps = (len.ceil() as usize).clamp(1, 20_000);
            for k in 0..=steps {
                let t = k as f64 / steps as f64;
                let (x, y, z) = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t);
                let r = half.ceil() as i64;
                for dy in -r..=r {
                    for dx in -r..=r {
                        let (ix, iy) = (x.floor() as i64 + dx, y.floor() as i64 + dy);
                        if ix < 0 || iy < 0 || ix >= sw as i64 || iy >= sh as i64 {
                            continue;
                        }
                        let d = ((ix as f64 + 0.5 - x).powi(2) + (iy as f64 + 0.5 - y).powi(2)).sqrt();
                        let cov = (half + 0.5 - d).clamp(0.0, 1.0);
                        if cov <= 0.0 {
                            continue;
                        }
                        let i = iy as usize * sw + ix as usize;
                        // A line on a face lies at the face's depth, but the pixel holds the
                        // face's depth at its centre, `d` away: allow what the face's slope
                        // spans over that distance, and the rounding. (A fixed allowance in
                        // screen depth is a share of the scene's depth range, a millimetre or
                        // more on a large design: close views showed the far side of thin
                        // sheet, and parts just behind it, through it.)
                        let visible = l.on_top
                            || c.depth.get(i).zip(slope.get(i)).is_some_and(|(dd, s)| z <= f64::from(*dd) + f64::from(*s) * d + z.abs() * DEPTH_EPS);
                        if visible {
                            c.put(ix as usize, iy as usize, col, cov);
                        }
                    }
                }
            }
        }
    }
    // Downsample.
    let mut out = Canvas::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let i = ((y * SS + sy) * sw + x * SS + sx) * 4;
                    for (k, a) in acc.iter_mut().enumerate() {
                        *a += c.rgba.get(i + k).copied().unwrap_or(0) as u32;
                    }
                }
            }
            let o = (y * w + x) * 4;
            for (k, a) in acc.iter().enumerate() {
                if let Some(v) = out.rgba.get_mut(o + k) {
                    *v = (*a / (SS * SS) as u32) as u8;
                }
            }
        }
    }
    out
}

/// The part of segment `p`–`q` the section keeps (where normal·x <= d).
fn clip_segment(p: Vec3, q: Vec3, clip: Option<(Vec3, f64)>) -> Option<(Vec3, Vec3)> {
    let Some((n, d)) = clip else { return Some((p, q)) };
    let (dp, dq) = (n.dot(p) - d - 1e-6, n.dot(q) - d - 1e-6);
    match (dp > 0.0, dq > 0.0) {
        (true, true) => None,
        (false, false) => Some((p, q)),
        (true, false) => Some((p + (q - p) * (dp / (dp - dq)), q)),
        (false, true) => Some((p, p + (q - p) * (dp / (dp - dq)))),
    }
}

/// Screen depth of the section plane `n·p = d` as `z = a·x + b·y + k` in canvas pixels (a plane
/// stays a plane under projection), set a hair behind the plane so the cut's own outline stays
/// drawn. None when the plane is seen edge-on or lies behind the eye.
fn cap_plane(cam: &Camera, n: Vec3, d: f64, radius: f64, project: &dyn Fn(Vec3) -> Option<[f64; 3]>) -> Option<[f64; 3]> {
    let margin = 1e-3 * (1.0 + radius);
    let o = cam.target - n * (n.dot(cam.target) - d - margin);
    let (u, v) = (n.any_perp(), n.cross(n.any_perp()));
    let s = radius * 0.1;
    let (p0, p1, p2) = (project(o)?, project(o + u * s)?, project(o + v * s)?);
    let (ax, ay, az) = (p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]);
    let (bx, by, bz) = (p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]);
    let det = ax * by - ay * bx;
    if det.abs() < 1e-9 {
        return None;
    }
    let a = (az * by - ay * bz) / det;
    let b = (ax * bz - az * bx) / det;
    let k = p0[2] - a * p0[0] - b * p0[1];
    [a, b, k].iter().all(|x| x.is_finite()).then_some([a, b, k])
}

/// Render to PNG bytes.
pub fn render_png(scene: &Scene, cam: &Camera, w: usize, h: usize) -> Option<Vec<u8>> {
    let c = render(scene, cam, w, h);
    png(c.w, c.h, c.rgba)
}

/// Render on a transparent background: the scene drawn over black and over white, and the
/// alpha recovered from the difference (exact for the anti-aliased edges).
pub fn render_transparent(scene: &mut Scene, cam: &Camera, w: usize, h: usize) -> Canvas {
    let keep = scene.background;
    scene.background = Some((Rgb(0, 0, 0), Rgb(0, 0, 0)));
    let black = render(scene, cam, w, h);
    scene.background = Some((Rgb(255, 255, 255), Rgb(255, 255, 255)));
    let mut out = render(scene, cam, w, h);
    scene.background = keep;
    for (o, b) in out.rgba.as_chunks_mut::<4>().0.iter_mut().zip(black.rgba.as_chunks::<4>().0) {
        // white - black = 255 (1 - alpha) in every channel; average them.
        let d: u32 = (0..3).map(|k| u32::from(o[k].saturating_sub(b[k]))).sum();
        let a = 255.0 - d as f64 / 3.0;
        for k in 0..3 {
            o[k] = if a > 0.5 { (f64::from(b[k]) * 255.0 / a).round().clamp(0.0, 255.0) as u8 } else { 0 };
        }
        o[3] = a.round().clamp(0.0, 255.0) as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StandardView;
    use crate::scene::{SceneLine, SceneMesh};
    use solidcraft_geom::Mesh;
    use std::sync::Arc;

    /// Lines are cut by the section in 3D: the kept piece ends on the plane.
    #[test]
    fn section_clips_lines_at_the_plane() {
        let clip = Some((Vec3::X, 2.0));
        let (p, q) = clip_segment(Vec3::ZERO, Vec3::new(10.0, 4.0, 0.0), clip).unwrap();
        assert_eq!(p, Vec3::ZERO);
        assert!((q.x - 2.0).abs() < 1e-5 && (q.y - 0.8).abs() < 1e-5, "{q:?}");
        assert!(clip_segment(Vec3::new(3.0, 0.0, 0.0), Vec3::new(5.0, 0.0, 0.0), clip).is_none());
        assert!(clip_segment(Vec3::new(3.0, 0.0, 0.0), Vec3::new(5.0, 0.0, 0.0), None).is_some());
    }

    #[test]
    fn renders_a_triangle() {
        let mesh = Mesh {
            positions: vec![Vec3::new(-10.0, 0.0, -10.0), Vec3::new(10.0, 0.0, -10.0), Vec3::new(0.0, 0.0, 10.0)],
            normals: vec![Vec3::new(0.0, -1.0, 0.0); 3],
            triangles: vec![[0, 1, 2]],
            tri_face: vec![0],
            edges: vec![],
            display_only_edges: false,
            seams: vec![],
            smooth_edges: vec![],
            sharp_edges: vec![],
            edge_faces: vec![],
        };
        let scene = Scene {
            meshes: vec![SceneMesh::solid(Arc::new(mesh), Rgb(200, 30, 30))],
            lines: vec![],
            background: None,
            radius: 20.0,
            clip: None,
            cap: None,
            ..Default::default()
        };
        let mut cam = Camera { distance: 50.0, ..Default::default() };
        cam.set_view(StandardView::Front);
        let c = render(&scene, &cam, 64, 64);
        let i = (32 * 64 + 32) * 4;
        assert!(c.rgba[i] > c.rgba[i + 1] + 50, "centre pixel is red: {:?}", &c.rgba[i..i + 4]);
        assert!(render_png(&scene, &cam, 16, 16).unwrap().starts_with(&[0x89, b'P', b'N', b'G']));
        // Hostile sizes are clamped.
        assert_eq!(render(&scene, &cam, 0, usize::MAX).w, 1);
    }

    /// A square panel facing the front camera (which looks along +Y), 20 × 20 about the origin
    /// at `y`: nearer the eye the smaller `y` is.
    fn panel(y: f64) -> Mesh {
        let c = [(-10.0, -10.0), (10.0, -10.0), (10.0, 10.0), (-10.0, 10.0)].map(|(x, z)| Vec3::new(x, y, z));
        Mesh {
            positions: c.to_vec(),
            normals: vec![Vec3::new(0.0, -1.0, 0.0); 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            tri_face: vec![0, 0],
            ..Default::default()
        }
    }

    /// A front panel over a back one, with a line between them, seen from the front.
    fn two_panels(front_opacity: f32) -> (Scene, Camera) {
        let glass = SceneMesh { mesh: Arc::new(panel(-5.0)), color: Rgb(255, 0, 0), face_colors: Vec::new(), opacity: front_opacity };
        let scene = Scene {
            meshes: vec![SceneMesh::solid(Arc::new(panel(5.0)), Rgb(0, 255, 0)), glass],
            lines: vec![SceneLine {
                points: vec![Vec3::new(-8.0, 2.0, 0.0), Vec3::new(8.0, 2.0, 0.0)],
                color: Rgb(0, 0, 255),
                width: 1.4,
                on_top: false,
            }],
            radius: 40.0,
            ..Default::default()
        };
        let mut cam = Camera { distance: 60.0, ..Default::default() };
        cam.set_view(StandardView::Front);
        (scene, cam)
    }

    /// A body the design draws see-through lets what is behind it show: the colour of the body
    /// behind it comes through, and so does a line between the two, which a solid body in front
    /// hides. This is what makes a picture of a cut's tool inside a translucent target readable.
    #[test]
    fn a_see_through_body_shows_what_is_inside_it() {
        let middle = |opacity: f32| {
            let (scene, cam) = two_panels(opacity);
            let c = render(&scene, &cam, 64, 64);
            // Inside both panels and clear of the line across the middle, so this pixel is
            // the two panels alone.
            let i = (24 * 64 + 24) * 4;
            [c.rgba[i], c.rgba[i + 1], c.rgba[i + 2]]
        };
        let behind = |opacity: f32| {
            let (scene, cam) = two_panels(opacity);
            render(&scene, &cam, 64, 64).rgba.chunks(4).filter(|p| p[2] > 150 && p[0] < 120 && p[1] < 120).count()
        };

        let solid = middle(1.0);
        assert!(solid[0] > 150 && solid[1] < 60, "a solid panel hides the one behind it: {solid:?}");
        assert_eq!(behind(1.0), 0, "a solid panel hides the line behind it");

        let glass = middle(0.35);
        assert!(glass[1] > solid[1] + 60, "the panel behind shows through: {glass:?} vs {solid:?}");
        assert!(glass[0] < solid[0], "the see-through panel itself is dimmer: {glass:?} vs {solid:?}");
        assert!(behind(0.35) > 20, "the line between the panels shows through the see-through one");

        // Opacity 0 still leaves a trace of the body rather than an invisible one, and the
        // picture changes all the way down.
        let faint = middle(0.0);
        assert!(faint[0] > 0 && faint[0] < glass[0], "opacity 0 is a trace, not nothing and not glass: {faint:?} vs {glass:?}");
    }

    /// A sheet facing the front camera, turned back by `tilt` about X: the plane y = z·tan(tilt)
    /// over x in ±15, z in ±`h`.
    fn sheet(tilt: f64, h: f64) -> Mesh {
        let t = tilt.tan();
        let c = [(-15.0, -h), (15.0, -h), (15.0, h), (-15.0, h)].map(|(x, z)| Vec3::new(x, z * t, z));
        Mesh {
            positions: c.to_vec(),
            normals: vec![Vec3::new(0.0, -tilt.cos(), tilt.sin()); 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            tri_face: vec![0, 0],
            ..Default::default()
        }
    }

    /// A close front view: 40 mm high on 64 px (128 canvas px, 0.3125 mm each), of a design of
    /// radius `radius`.
    fn close_view(mesh: Mesh, lines: Vec<SceneLine>, radius: f64, fov: f64) -> (Scene, Camera) {
        let scene = Scene { meshes: vec![SceneMesh::solid(Arc::new(mesh), Rgb(160, 165, 175))], lines, radius, ..Default::default() };
        let mut cam = Camera { distance: if fov > 0.0 { 20.0 / (fov * 0.5).tan() } else { 50.0 }, fov, ..Default::default() };
        cam.set_view(StandardView::Front);
        (scene, cam)
    }

    fn red_pixels(scene: &Scene, cam: &Camera) -> usize {
        render(scene, cam, 64, 64).rgba.chunks(4).filter(|p| p[0] > 150 && p[1] < 100 && p[2] < 100).count()
    }

    /// An edge 1 mm behind a sheet is hidden in a close view of a large design. The old test
    /// allowed 2e-4 of the depth range, 2e-4 · (8 · 1200 + 2) = 1.92 mm here (orthographic; far
    /// more in perspective), so it was drawn on the sheet. Now the allowance is what the sheet's
    /// slope spans across the line's width: none facing the camera; tilted 45°, the edge lies
    /// 1 / cos 45° = 1.414 mm behind along the view, and the sheet's depth changes 0.3125 mm
    /// per pixel, at most 2 · 1.7 · 0.3125 = 1.06 mm over a 1.2 px line (1.7 px from its centre
    /// line to the edge of its anti-aliasing).
    #[test]
    fn edges_behind_a_thin_sheet_stay_hidden_in_close_views() {
        for fov in [0.0, 0.6] {
            for tilt in [0.0, 45f64.to_radians()] {
                // 1 mm behind the sheet, along its normal, across the middle.
                let back = Vec3::new(0.0, tilt.cos(), -tilt.sin());
                let edge = SceneLine {
                    points: vec![Vec3::new(-10.0, 0.0, 0.0) + back, Vec3::new(10.0, 0.0, 0.0) + back],
                    color: Rgb(255, 0, 0),
                    width: 1.2,
                    on_top: false,
                };
                let (mut scene, cam) = close_view(sheet(tilt, 12.0), vec![edge], 1200.0, fov);
                assert_eq!(red_pixels(&scene, &cam), 0, "fov {fov}, tilt {tilt}: the edge behind the sheet shows through it");
                scene.lines[0].on_top = true;
                assert!(red_pixels(&scene, &cam) > 20, "fov {fov}, tilt {tilt}: the edge is in the picture");
            }
        }
    }

    /// Edges lying on a face are drawn whole, however steep the face and whatever the design's
    /// size: the outline of a sheet turned 70° away from the camera (its depth changes
    /// tan 70° · 0.3125 = 0.86 mm per pixel) and a line across it draw exactly as they do
    /// drawn over everything.
    #[test]
    fn edges_on_a_steep_face_draw_whole() {
        let tilt = 70f64.to_radians();
        let on = |x: f64, z: f64| Vec3::new(x, z * tilt.tan(), z);
        let line = |points: Vec<Vec3>| SceneLine { points, color: Rgb(255, 0, 0), width: 1.2, on_top: false };
        for fov in [0.0, 0.6] {
            for radius in [30.0, 1200.0] {
                let lines = vec![
                    line(vec![on(-15.0, -5.0), on(15.0, -5.0), on(15.0, 5.0), on(-15.0, 5.0), on(-15.0, -5.0)]),
                    line(vec![on(-10.0, 1.5), on(10.0, 1.5)]),
                ];
                let (mut scene, cam) = close_view(sheet(tilt, 5.0), lines, radius, fov);
                let drawn = red_pixels(&scene, &cam);
                for l in &mut scene.lines {
                    l.on_top = true;
                }
                let whole = red_pixels(&scene, &cam);
                assert!(whole > 100, "fov {fov}, radius {radius}: {whole} line pixels");
                assert_eq!(drawn, whole, "fov {fov}, radius {radius}: parts of the lines on the face are hidden");
            }
        }
    }
}

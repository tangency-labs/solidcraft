//! A z-buffer picture of the mesh, with the edges over it.
//!
//! Orthographic, flat-shaded, drawn at twice the size and averaged down. Faces are pale
//! grey; `--highlight` faces are the accent blue and `--see-through` faces a translucent
//! pale blue that hides nothing. Edges at or below `--edges-above` are thin dark grey
//! lines where they are visible; edges above it are thick black lines drawn over
//! everything.

use crate::body::{EdgeInfo, Mesh};

/// The view direction: the azimuth turns about z, the elevation tilts towards z (degrees).
#[derive(Clone, Copy)]
pub struct View {
    pub azimuth: f64,
    pub elevation: f64,
}

impl View {
    /// `p` as (right, down, depth), depth growing away from the viewer.
    pub fn project(self, p: [f64; 3]) -> [f64; 3] {
        let (sa, ca) = self.azimuth.to_radians().sin_cos();
        let (se, ce) = self.elevation.to_radians().sin_cos();
        let [x, y, z] = p;
        let across = ca * x - sa * y;
        let along = sa * x + ca * y;
        [across, -(ce * z - se * along), ce * along + se * z]
    }
}

/// How faces and edges are drawn.
pub struct Style {
    pub highlight: Vec<usize>,
    pub see_through: Vec<usize>,
    pub edges_above: f64,
}

/// An RGB picture, rows top to bottom.
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
}

/// Where the projected body lands in the picture: pixel = projected × scale + offset.
pub struct Frame {
    pub scale: f64,
    pub offset: [f64; 2],
    pub view: View,
}

impl Frame {
    /// The pixel of the picture a model point projects to.
    pub fn pixel(&self, p: [f64; 3]) -> [f64; 2] {
        let [x, y, _] = self.view.project(p);
        [x * self.scale + self.offset[0], y * self.scale + self.offset[1]]
    }
}

/// Supersampling factor per axis.
const SS: usize = 2;
const BACKGROUND: [f64; 3] = [1.0, 1.0, 1.0];
const FACE: [f64; 3] = [0.94, 0.94, 0.94];
const HIGHLIGHT: [f64; 3] = [0.122, 0.435, 0.698];
const SEE_THROUGH: [f64; 3] = [0.863, 0.914, 0.961];
const SEE_THROUGH_ALPHA: f64 = 0.25;
const EDGE: [f64; 3] = [0.333, 0.333, 0.333];
const LARGE_EDGE: [f64; 3] = [0.0, 0.0, 0.0];

/// Draws the mesh and the edges in a `width` × `height` picture.
pub fn draw(mesh: &Mesh, edges: &[EdgeInfo], view: View, style: &Style, width: usize, height: usize) -> (Picture, Frame) {
    let frame = fit(mesh, edges, view, width, height);
    let mut c = Canvas::new(width * SS, height * SS);
    let fine = Frame { scale: frame.scale * SS as f64, offset: frame.offset.map(|o| o * SS as f64), view };
    let point = |i: u32| mesh.positions.get(i as usize).copied();
    let screen = |p: [f64; 3]| {
        let [x, y] = fine.pixel(p);
        [x, y, view.project(p)[2]]
    };
    // Opaque faces first, so the see-through ones blend over what lies behind them.
    for pass in [false, true] {
        for (t, &face) in mesh.triangles.iter().zip(&mesh.triangle_face) {
            let see_through = style.see_through.contains(&face);
            if see_through != pass {
                continue;
            }
            let (Some(a), Some(b), Some(d)) = (point(t[0]), point(t[1]), point(t[2])) else {
                continue;
            };
            let base = if see_through {
                SEE_THROUGH
            } else if style.highlight.contains(&face) {
                HIGHLIGHT
            } else {
                FACE
            };
            let colour = base.map(|v| v * shade(view, a, b, d));
            let alpha = if see_through { Some(SEE_THROUGH_ALPHA) } else { None };
            c.triangle([screen(a), screen(b), screen(d)], colour, alpha);
        }
    }
    // An edge lies on its faces, which the mesh only approximates: a visible edge may sit
    // up to the mesh's chord tolerance behind them.
    let slack = mesh.size * 3e-3;
    for large in [false, true] {
        for e in edges.iter().filter(|e| (e.tolerance > style.edges_above) == large) {
            let pts: Vec<[f64; 3]> = e.points.iter().map(|&p| screen(p)).collect();
            if large {
                c.polyline(&pts, 2.5 * SS as f64, LARGE_EDGE, None);
            } else {
                c.polyline(&pts, 1.0 * SS as f64, EDGE, Some(slack));
            }
        }
    }
    (c.downsample(), frame)
}

/// The frame that fits the mesh and the edges in the picture with a 4% margin, centred.
fn fit(mesh: &Mesh, edges: &[EdgeInfo], view: View, width: usize, height: usize) -> Frame {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    let all = mesh.positions.iter().chain(edges.iter().flat_map(|e| e.points.iter()));
    for &p in all {
        let [x, y, _] = view.project(p);
        lo = [lo[0].min(x), lo[1].min(y)];
        hi = [hi[0].max(x), hi[1].max(y)];
    }
    let (w, h) = (width as f64, height as f64);
    let margin = 0.04 * w.min(h);
    let extent = [(hi[0] - lo[0]).max(1e-12), (hi[1] - lo[1]).max(1e-12)];
    let scale = ((w - 2.0 * margin) / extent[0]).min((h - 2.0 * margin) / extent[1]);
    let scale = if scale.is_finite() { scale } else { 1.0 };
    let offset = [
        (w - extent[0] * scale) / 2.0 - lo[0] * scale,
        (h - extent[1] * scale) / 2.0 - lo[1] * scale,
    ];
    let offset = if offset.iter().all(|o| o.is_finite()) { offset } else { [0.0, 0.0] };
    Frame { scale, offset, view }
}

/// Lambert shading of a triangle's plane, lit from over the viewer's left shoulder, both
/// sides lit alike.
fn shade(view: View, a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    let [a, b, c] = [a, b, c].map(|p| view.project(p));
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    let light = [-0.40, -0.60, -0.70];
    let light_len = (0.40f64 * 0.40 + 0.60 * 0.60 + 0.70 * 0.70).sqrt();
    let lambert = if len > 0.0 {
        ((n[0] * light[0] + n[1] * light[1] + n[2] * light[2]) / (len * light_len)).abs()
    } else {
        1.0
    };
    0.45 + 0.55 * lambert
}

/// A supersampled colour and depth buffer.
struct Canvas {
    width: usize,
    height: usize,
    depth: Vec<f64>,
    rgb: Vec<[f64; 3]>,
}

impl Canvas {
    fn new(width: usize, height: usize) -> Self {
        Canvas { width, height, depth: vec![f64::INFINITY; width * height], rgb: vec![BACKGROUND; width * height] }
    }

    /// The pixel range `[lo, hi]` covers, clamped to the canvas.
    fn span(lo: f64, hi: f64, n: usize) -> std::ops::Range<usize> {
        let start = lo.floor().max(0.0) as usize;
        let end = (hi.ceil().max(0.0) as usize).min(n);
        start..end.max(start)
    }

    /// Fills a triangle given in (pixel x, pixel y, depth). Opaque fragments write depth;
    /// a translucent one (`alpha`) blends over whatever is behind it and writes none.
    fn triangle(&mut self, [a, b, c]: [[f64; 3]; 3], colour: [f64; 3], alpha: Option<f64>) {
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area.abs() < 1e-12 || !area.is_finite() {
            return;
        }
        let xs = Self::span(a[0].min(b[0]).min(c[0]), a[0].max(b[0]).max(c[0]), self.width);
        let ys = Self::span(a[1].min(b[1]).min(c[1]), a[1].max(b[1]).max(c[1]), self.height);
        for y in ys {
            for x in xs.clone() {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let w0 = ((b[0] - px) * (c[1] - py) - (b[1] - py) * (c[0] - px)) / area;
                let w1 = ((c[0] - px) * (a[1] - py) - (c[1] - py) * (a[0] - px)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * a[2] + w1 * b[2] + w2 * c[2];
                let i = y * self.width + x;
                let (Some(d), Some(p)) = (self.depth.get_mut(i), self.rgb.get_mut(i)) else {
                    continue;
                };
                if z >= *d {
                    continue;
                }
                match alpha {
                    None => {
                        *d = z;
                        *p = colour;
                    }
                    Some(t) => *p = mix(*p, colour, t),
                }
            }
        }
    }

    /// Strokes a polyline in (pixel x, pixel y, depth) with round caps and joins. With a
    /// `slack`, only where the line is no more than `slack` behind the faces.
    fn polyline(&mut self, pts: &[[f64; 3]], width: f64, colour: [f64; 3], slack: Option<f64>) {
        let r = width / 2.0;
        for seg in pts.windows(2) {
            let [a, b] = [seg.first(), seg.get(1)].map(|p| p.copied().unwrap_or([0.0; 3]));
            let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            let steps = (len / (r * 0.5).max(0.5)).ceil().max(1.0) as usize;
            for s in 0..=steps {
                let t = s as f64 / steps as f64;
                let q = mix(a, b, t);
                self.dot(q, r, colour, slack);
            }
        }
    }

    fn dot(&mut self, q: [f64; 3], r: f64, colour: [f64; 3], slack: Option<f64>) {
        for y in Self::span(q[1] - r, q[1] + r, self.height) {
            for x in Self::span(q[0] - r, q[0] + r, self.width) {
                let (dx, dy) = (x as f64 + 0.5 - q[0], y as f64 + 0.5 - q[1]);
                if dx * dx + dy * dy > r * r {
                    continue;
                }
                let i = y * self.width + x;
                let (Some(&d), Some(p)) = (self.depth.get(i), self.rgb.get_mut(i)) else {
                    continue;
                };
                if slack.is_none_or(|s| q[2] <= d + s) {
                    *p = colour;
                }
            }
        }
    }

    /// Averages each `SS` × `SS` block into one 8-bit pixel.
    fn downsample(&self) -> Picture {
        let (width, height) = (self.width / SS, self.height / SS);
        let mut rgb = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            for x in 0..width {
                let mut sum = [0.0; 3];
                for sy in 0..SS {
                    for sx in 0..SS {
                        let i = (y * SS + sy) * self.width + x * SS + sx;
                        let p = self.rgb.get(i).copied().unwrap_or(BACKGROUND);
                        sum = add(sum, p);
                    }
                }
                let n = (SS * SS) as f64;
                rgb.extend(sum.map(|v| (v / n * 255.0).round().clamp(0.0, 255.0) as u8));
            }
        }
        Picture { width, height, rgb }
    }
}

/// `a + (b - a) t`, per component.
fn mix([a0, a1, a2]: [f64; 3], [b0, b1, b2]: [f64; 3], t: f64) -> [f64; 3] {
    [a0 + (b0 - a0) * t, a1 + (b1 - a1) * t, a2 + (b2 - a2) * t]
}

fn add([a0, a1, a2]: [f64; 3], [b0, b1, b2]: [f64; 3]) -> [f64; 3] {
    [a0 + b0, a1 + b1, a2 + b2]
}

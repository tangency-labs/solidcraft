//! The studio the modeling viewport lights finished faces with: model-viewer's default
//! "neutral" environment (google/model-viewer `EnvironmentScene.ts`, Apache-2.0, the same room as
//! three.js `RoomEnvironment`), drawn once on the CPU ([`room`]) and prefiltered for image-based
//! lighting (split-sum): a specular map whose mip `k` is the room seen through a GGX lobe of
//! perceptual roughness `k / (SPECULAR_LEVELS - 1)`, and a small diffuse irradiance map
//! (cosine-weighted), plus the GGX environment-BRDF table (`dfg`) the shader weights them with.
//! The room is used as is (no scale, no turn); the shader applies model-viewer's exposure for it.
//!
//! Both maps are equirectangular in the shader's frame, the view's (x right, y up, z toward the
//! eye): a direction `(x, y, z)` sits at `u = 0.5 + atan2(x, -z) / 2π`, `v = acos(y) / π`. The
//! room's frame is model-viewer's default view (camera on +z looking down -z), so it maps 1:1.

use std::f32::consts::PI;

/// The room's map width, sampled 2 × 2 per texel: twice the specular map's, as `prefilter` reads.
const ROOM_WIDTH: usize = 2 * SPECULAR_WIDTH;

/// The environment-BRDF table: `DFG_SIZE` × `DFG_SIZE` over (n·v, perceptual roughness), at
/// texel centres as the Khronos glTF-IBL-Sampler writes its `LUT`; the size is Filament's (`FILAMENT_DFG_LUT_SIZE`).
const DFG_SIZE: usize = 128;
/// Hammersley samples per table texel: the fewest (of 512, 1024, 2048) within 0.01 of Khronos's table.
const DFG_SAMPLES: u32 = 1024;

/// Mips of the specular map: 256 × 128 (a mirror) down to 8 × 4 (roughness 1).
pub const SPECULAR_LEVELS: u32 = 6;
const SPECULAR_WIDTH: usize = 256;
const IRRADIANCE_WIDTH: usize = 32;
/// GGX samples per texel of each rough level.
const SAMPLES: u32 = 128;

/// An equirectangular image in linear RGB, `width` × `width / 2`.
#[derive(Clone)]
pub(crate) struct Equirect {
    pub width: usize,
    pub rgb: Vec<[f32; 3]>,
}

impl Equirect {
    fn height(&self) -> usize {
        self.width / 2
    }

    fn at(&self, x: usize, y: usize) -> [f32; 3] {
        self.rgb.get(y.min(self.height() - 1) * self.width + x % self.width).copied().unwrap_or([0.0; 3])
    }

    /// Half the size, each texel the mean of four.
    fn halved(&self) -> Equirect {
        let width = (self.width / 2).max(2);
        let mut rgb = Vec::with_capacity(width * width / 2);
        for y in 0..width / 2 {
            for x in 0..width {
                let q = [self.at(2 * x, 2 * y), self.at(2 * x + 1, 2 * y), self.at(2 * x, 2 * y + 1), self.at(2 * x + 1, 2 * y + 1)];
                rgb.push(std::array::from_fn(|c| q.iter().map(|t| t[c]).sum::<f32>() / 4.0));
            }
        }
        Equirect { width, rgb }
    }

    /// Bilinear radiance along the unit direction `d`.
    fn sample(&self, d: [f32; 3]) -> [f32; 3] {
        let (u, v) = uv(d);
        let (w, h) = (self.width as f32, self.height() as f32);
        let (fx, fy) = (u * w - 0.5, (v * h - 0.5).clamp(0.0, h - 1.0));
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let x0 = (x0 as i64).rem_euclid(self.width as i64) as usize;
        let y0 = y0 as usize;
        let (a, b, c, e) = (self.at(x0, y0), self.at(x0 + 1, y0), self.at(x0, y0 + 1), self.at(x0 + 1, y0 + 1));
        std::array::from_fn(|i| (a[i] * (1.0 - tx) + b[i] * tx) * (1.0 - ty) + (c[i] * (1.0 - tx) + e[i] * tx) * ty)
    }
}

/// Where the unit direction `d` lands on an equirectangular map, in 0..1 (see the module).
fn uv(d: [f32; 3]) -> (f32, f32) {
    (0.5 + d[0].atan2(-d[2]) / (2.0 * PI), d[1].clamp(-1.0, 1.0).acos() / PI)
}

/// The direction at the centre of texel (`x`, `y`) of a `width` × `width / 2` map.
fn direction(x: usize, y: usize, width: usize) -> [f32; 3] {
    let phi = ((x as f32 + 0.5) / width as f32 - 0.5) * 2.0 * PI;
    let theta = (y as f32 + 0.5) / (width / 2) as f32 * PI;
    [theta.sin() * phi.sin(), theta.cos(), -theta.sin() * phi.cos()]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalized(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(1e-12);
    a.map(|c| c / l)
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The `i`-th of `n` points of the Hammersley set in the unit square.
fn hammersley(i: u32, n: u32) -> (f32, f32) {
    (i as f32 / n as f32, i.reverse_bits() as f32 * (1.0 / 4_294_967_296.0))
}

/// The environment `source` seen through a GGX lobe of roughness `rough` about each texel's
/// direction (the split-sum's prefiltered radiance, with n = v = r), at `width`.
fn prefiltered(source: &Equirect, rough: f32, width: usize) -> Equirect {
    let a = (rough * rough).max(1e-4);
    let mut rgb = Vec::with_capacity(width * width / 2);
    for y in 0..width / 2 {
        for x in 0..width {
            let n = direction(x, y, width);
            let helper = if n[1].abs() < 0.999 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
            let t = normalized(cross(helper, n));
            let b = cross(n, t);
            let (mut sum, mut weight) = ([0.0f32; 3], 0.0f32);
            for i in 0..SAMPLES {
                let (e1, e2) = hammersley(i, SAMPLES);
                let phi = 2.0 * PI * e1;
                let cos = ((1.0 - e2) / (1.0 + (a * a - 1.0) * e2)).sqrt();
                let sin = (1.0 - cos * cos).max(0.0).sqrt();
                let h: [f32; 3] = std::array::from_fn(|c| t[c] * sin * phi.cos() + b[c] * sin * phi.sin() + n[c] * cos);
                let nh = dot(n, h);
                let l: [f32; 3] = std::array::from_fn(|c| 2.0 * nh * h[c] - n[c]);
                let nl = dot(n, l);
                if nl > 0.0 {
                    let s = source.sample(l);
                    for c in 0..3 {
                        sum[c] += s[c] * nl;
                    }
                    weight += nl;
                }
            }
            rgb.push(sum.map(|s| s / weight.max(1e-6)));
        }
    }
    Equirect { width, rgb }
}

/// `source` read (bilinear) at each texel's direction of a `width` map: a mirror's view.
fn resampled(source: &Equirect, width: usize) -> Equirect {
    let rgb = (0..width / 2).flat_map(|y| (0..width).map(move |x| (x, y))).map(|(x, y)| source.sample(direction(x, y, width))).collect();
    Equirect { width, rgb }
}

/// Cosine-weighted irradiance over π (so a Lambertian face's radiance is albedo × this) at
/// each texel's direction, from `source` (summed over its texels with their solid angles).
fn irradiance(source: &Equirect, width: usize) -> Equirect {
    let sw = source.width;
    let texels: Vec<([f32; 3], f32)> = (0..sw / 2)
        .flat_map(|y| (0..sw).map(move |x| (x, y)))
        .map(|(x, y)| {
            let theta = (y as f32 + 0.5) / (sw / 2) as f32 * PI;
            (direction(x, y, sw), (2.0 * PI / sw as f32) * (PI / (sw / 2) as f32) * theta.sin())
        })
        .collect();
    let mut rgb = Vec::with_capacity(width * width / 2);
    for y in 0..width / 2 {
        for x in 0..width {
            let n = direction(x, y, width);
            let mut sum = [0.0f32; 3];
            for (k, (d, omega)) in texels.iter().enumerate() {
                let c = dot(n, *d);
                if c > 0.0 {
                    let l = source.at(k % sw, k / sw);
                    for i in 0..3 {
                        sum[i] += l[i] * c * omega;
                    }
                }
            }
            rgb.push(sum.map(|s| s / PI));
        }
    }
    Equirect { width, rgb }
}

/// A box of model-viewer's room: centre, size, turn about +y (radians, as three.js
/// `rotation.y`), and its radiance when it is a light panel (`MeshBasicMaterial` colour =
/// intensity) or `None` for a white diffuse surface (`MeshStandardMaterial` defaults).
struct Block {
    centre: [f32; 3],
    size: [f32; 3],
    turn: f32,
    glow: Option<f32>,
}

/// The whole scene sits this far down (`EnvironmentScene`: `this.position.y = -3.5`).
const ROOM_DROP: f32 = -3.5;
/// The `neutral` top light: a three.js `PointLight` of this intensity, range 28, decay 2.
const ROOM_LIGHT: ([f32; 3], f32) = ([0.5, 14.0, 0.5], 400.0);
const ROOM_LIGHT_RANGE: f32 = 28.0;
/// The `neutral` room (seen from inside), boxes and light panels: `EnvironmentScene.ts` verbatim.
const ROOM: ([f32; 3], [f32; 3]) = ([0.0, 13.2, 0.0], [31.5, 28.5, 31.5]);
const ROOM_BOXES: [([f32; 3], f32, [f32; 3]); 6] = [
    ([-10.906, -1.0, 1.846], -0.195, [2.328, 7.905, 4.651]),
    ([-5.607, -0.754, -0.758], 0.994, [1.970, 1.534, 3.955]),
    ([6.167, -0.16, 7.803], 0.561, [3.927, 6.285, 3.687]),
    ([-2.017, 0.018, 6.124], 0.333, [2.002, 4.566, 2.064]),
    ([2.291, -0.756, -2.621], -0.286, [1.546, 1.552, 1.496]),
    ([-2.193, -0.369, -5.547], 0.516, [3.875, 3.487, 2.986]),
];
const ROOM_PANELS: [(f32, [f32; 3], [f32; 3]); 7] = [
    (80.0, [-14.0, 10.0, 8.0], [0.1, 2.5, 2.5]),
    (80.0, [-14.0, 14.0, -4.0], [0.1, 2.5, 2.5]),
    (23.0, [14.0, 12.0, 0.0], [0.1, 5.0, 5.0]),
    (16.0, [0.0, 9.0, 14.0], [5.0, 5.0, 0.1]),
    (80.0, [7.0, 8.0, -14.0], [2.5, 2.5, 0.1]),
    (80.0, [-7.0, 16.0, -14.0], [2.5, 2.5, 0.1]),
    (1.0, [0.0, 20.0, 0.0], [0.1, 0.1, 0.1]),
];

fn dropped(p: [f32; 3]) -> [f32; 3] {
    [p[0], p[1] + ROOM_DROP, p[2]]
}

/// Where the ray from the origin along `d` meets `block` (from inside when `inside`): its
/// distance and the face's normal (toward the ray), or `None`.
fn hit(block: &Block, d: [f32; 3], inside: bool) -> Option<(f32, [f32; 3])> {
    // Into the block's frame: three.js rotation.y maps local (x, z) to (x c + z s, -x s + z c).
    let (s, c) = block.turn.sin_cos();
    let local = |p: [f32; 3]| [p[0] * c - p[2] * s, p[1], p[0] * s + p[2] * c];
    let o = local(block.centre.map(|v| -v));
    let dl = local(d);
    let (mut near, mut far, mut near_axis, mut far_axis) = (f32::NEG_INFINITY, f32::INFINITY, 0, 0);
    for k in 0..3 {
        let h = block.size[k] / 2.0;
        if dl[k].abs() < 1e-9 {
            if o[k].abs() > h {
                return None;
            }
            continue;
        }
        let (a, b) = ((-h - o[k]) / dl[k], (h - o[k]) / dl[k]);
        let (a, b) = (a.min(b), a.max(b));
        if a > near {
            (near, near_axis) = (a, k);
        }
        if b < far {
            (far, far_axis) = (b, k);
        }
    }
    let (t, axis) = if inside { (far, far_axis) } else { (near, near_axis) };
    if near > far || t <= 0.0 {
        return None;
    }
    // The face's normal faces back along the ray; out of the block's frame again.
    let mut n = [0.0f32; 3];
    n[axis] = -dl[axis].signum();
    Some((t, [n[0] * c + n[2] * s, n[1], -n[0] * s + n[2] * c]))
}

/// three.js `getDistanceAttenuation` (physically based lights): inverse square, faded to zero at
/// the light's range.
fn attenuation(distance: f32) -> f32 {
    let fade = (1.0 - (distance / ROOM_LIGHT_RANGE).powi(4)).clamp(0.0, 1.0);
    fade * fade / (distance * distance).max(0.01)
}

/// The radiance the camera at the room's origin sees along `d`: a panel's own, or a white
/// Lambertian surface lit by the top light (three.js `BRDF_Lambert` × the light's irradiance; the
/// light casts no shadow, and the panels light nothing, as in model-viewer's render of the room).
fn room_radiance(blocks: &[Block], d: [f32; 3]) -> f32 {
    let nearest = blocks
        .iter()
        .enumerate()
        .filter_map(|(k, b)| hit(b, d, k == 0).map(|(t, n)| (t, n, b.glow)))
        .fold(None, |best: Option<(f32, [f32; 3], Option<f32>)>, h| if best.is_some_and(|b| b.0 <= h.0) { best } else { Some(h) });
    let Some((t, n, glow)) = nearest else { return 0.0 };
    if let Some(glow) = glow {
        return glow;
    }
    let p = d.map(|c| c * t);
    let to_light: [f32; 3] = std::array::from_fn(|k| dropped(ROOM_LIGHT.0)[k] - p[k]);
    let distance = dot(to_light, to_light).sqrt();
    let cos = (dot(n, to_light) / distance.max(1e-6)).max(0.0);
    ROOM_LIGHT.1 * attenuation(distance) * cos / PI
}

/// model-viewer's neutral room as an equirectangular map in the view's frame (see the module).
fn room() -> Equirect {
    let mut blocks = vec![Block { centre: dropped(ROOM.0), size: ROOM.1, turn: 0.0, glow: None }];
    blocks.extend(ROOM_BOXES.iter().map(|&(p, turn, size)| Block { centre: dropped(p), size, turn, glow: None }));
    blocks.extend(ROOM_PANELS.iter().map(|&(glow, p, size)| Block { centre: dropped(p), size, turn: 0.0, glow: Some(glow) }));
    let fine = 2 * ROOM_WIDTH;
    let mut rgb = Vec::with_capacity(ROOM_WIDTH * ROOM_WIDTH / 2);
    for y in 0..ROOM_WIDTH / 2 {
        for x in 0..ROOM_WIDTH {
            let l = [(0, 0), (1, 0), (0, 1), (1, 1)].iter().map(|(i, j)| room_radiance(&blocks, direction(2 * x + i, 2 * y + j, fine))).sum::<f32>()
                / 4.0;
            rgb.push([l; 3]);
        }
    }
    Equirect { width: ROOM_WIDTH, rgb }
}

/// The specular mips (`SPECULAR_LEVELS`, mip 0 at `SPECULAR_WIDTH`) and the irradiance map of
/// `studio`.
pub(crate) fn prefilter(studio: Equirect) -> (Vec<Equirect>, Equirect) {
    let mut pyramid = vec![studio];
    while pyramid.last().is_some_and(|e| e.width > IRRADIANCE_WIDTH) {
        if let Some(next) = pyramid.last().map(Equirect::halved) {
            pyramid.push(next);
        }
    }
    // The source a level reads: twice its width where there is one, so a lobe narrower than
    // a texel of the level still averages several.
    let source = |width: usize| {
        pyramid
            .iter()
            .rev()
            .find(|e| e.width >= 2 * width)
            .or(pyramid.first())
            .cloned()
            .unwrap_or_else(|| Equirect { width: 4, rgb: vec![[0.0; 3]; 8] })
    };
    let mut levels = Vec::new();
    for k in 0..SPECULAR_LEVELS {
        let width = (SPECULAR_WIDTH >> k).max(4);
        let level =
            if k == 0 { resampled(&source(width), width) } else { prefiltered(&source(width), k as f32 / (SPECULAR_LEVELS - 1) as f32, width) };
        levels.push(level);
    }
    let diffuse = irradiance(&source(IRRADIANCE_WIDTH / 2), IRRADIANCE_WIDTH);
    (levels, diffuse)
}

/// The split-sum environment BRDF at (`nv`, perceptual `roughness`): the scale A and bias B on
/// F0 (Khronos glTF-IBL-Sampler `LUT`, GGX with height-correlated Smith visibility), `samples`
/// Hammersley GGX half vectors.
fn dfg_at(nv: f32, roughness: f32, samples: u32) -> [f32; 2] {
    let v = [(1.0 - nv * nv).max(0.0).sqrt(), 0.0, nv];
    let alpha = roughness * roughness;
    let a2 = alpha * alpha;
    let (mut a, mut b) = (0.0f32, 0.0f32);
    for i in 0..samples {
        let (e1, e2) = hammersley(i, samples);
        let phi = 2.0 * PI * e1;
        let cos = ((1.0 - e2) / (1.0 + (a2 - 1.0) * e2)).sqrt();
        let sin = (1.0 - cos * cos).max(0.0).sqrt();
        let h = [sin * phi.cos(), sin * phi.sin(), cos];
        let vh = dot(v, h);
        let nl = (2.0 * vh * h[2] - v[2]).clamp(0.0, 1.0);
        if nl > 0.0 {
            let visibility = 0.5 / (nl * (nv * nv * (1.0 - a2) + a2).sqrt() + nv * (nl * nl * (1.0 - a2) + a2).sqrt());
            let v_pdf = visibility * vh.clamp(0.0, 1.0) * nl / cos.max(1e-6);
            let fc = (1.0 - vh.clamp(0.0, 1.0)).powi(5);
            a += (1.0 - fc) * v_pdf;
            b += fc * v_pdf;
        }
    }
    [4.0 * a / samples as f32, 4.0 * b / samples as f32]
}

/// The environment-BRDF table: row `y` at roughness `(y + ½) / DFG_SIZE`, column `x` at n·v
/// `(x + ½) / DFG_SIZE`, (A, B, 0) per texel.
fn dfg() -> Vec<[f32; 3]> {
    let at = |i: usize| (i as f32 + 0.5) / DFG_SIZE as f32;
    (0..DFG_SIZE).flat_map(|y| (0..DFG_SIZE).map(move |x| (x, y))).map(|(x, y)| dfg_at(at(x), at(y), DFG_SAMPLES)).map(|[a, b]| [a, b, 0.0]).collect()
}

/// The IEEE half-float bits nearest `f` (non-negative radiance; past the largest half, the
/// largest; below the normal range, zero).
fn half(f: f32) -> u16 {
    let f = f.clamp(0.0, 65504.0);
    let bits = f.to_bits();
    let exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    if exp <= 0 {
        return 0;
    }
    let mantissa = (bits >> 13) & 0x3ff;
    let round = (bits >> 12) & 1;
    ((exp as u32) << 10 | mantissa).saturating_add(round).min(0x7bff) as u16
}

/// The texels as RGBA half floats (alpha 1).
fn half_bytes(rgb: &[[f32; 3]]) -> Vec<u8> {
    rgb.iter().flat_map(|p| [p[0], p[1], p[2], 1.0]).flat_map(|c| half(c).to_le_bytes()).collect()
}

/// What the studio's textures hold: the specular mips, the irradiance map and the BRDF table.
pub(crate) struct Tables {
    levels: Vec<Equirect>,
    diffuse: Equirect,
    dfg: Vec<[f32; 3]>,
}

/// Draw and prefilter the room and build the environment-BRDF table: about 0.23 s of CPU (M-series,
/// release), so the renderer builds them beside its pipelines ([`Environment::set`]).
pub(crate) fn tables() -> Tables {
    let (levels, diffuse) = prefilter(room());
    Tables { levels, diffuse, dfg: dfg() }
}

/// The prefiltered studio and the environment-BRDF table on the GPU, with the sampler that
/// reads them. The textures are made with the viewport (zero until written) and filled once,
/// the first time a queue is at hand ([`Environment::upload`]).
#[derive(Clone)]
pub(crate) struct Environment {
    specular_t: wgpu::Texture,
    diffuse_t: wgpu::Texture,
    dfg_t: wgpu::Texture,
    pub specular: wgpu::TextureView,
    pub diffuse: wgpu::TextureView,
    pub dfg: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    uploaded: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ready: std::sync::Arc<std::sync::Mutex<Option<Tables>>>,
}

impl Environment {
    pub fn new(device: &wgpu::Device) -> Environment {
        let texture = |label, width: usize, height: usize, mips: u32| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: width as u32, height: height as u32, depth_or_array_layers: 1 },
                mip_level_count: mips,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let specular_t = texture("sc_studio_specular", SPECULAR_WIDTH, SPECULAR_WIDTH / 2, SPECULAR_LEVELS);
        let diffuse_t = texture("sc_studio_diffuse", IRRADIANCE_WIDTH, IRRADIANCE_WIDTH / 2, 1);
        let dfg_t = texture("sc_studio_dfg", DFG_SIZE, DFG_SIZE, 1);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sc_studio"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        Environment {
            specular: specular_t.create_view(&Default::default()),
            diffuse: diffuse_t.create_view(&Default::default()),
            dfg: dfg_t.create_view(&Default::default()),
            specular_t,
            diffuse_t,
            dfg_t,
            sampler,
            uploaded: Default::default(),
            ready: Default::default(),
        }
    }

    /// Hand over the tables built ahead ([`tables`]), for [`Environment::upload`] to write.
    pub fn set(&self, tables: Tables) {
        if let Ok(mut ready) = self.ready.lock() {
            *ready = Some(tables);
        }
    }

    /// Write the tables, once (later calls return at once): those [`Environment::set`] handed
    /// over, else built now.
    pub fn upload(&self, queue: &wgpu::Queue) {
        if self.uploaded.swap(true, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let Tables { levels, diffuse, dfg } = self.ready.lock().ok().and_then(|mut r| r.take()).unwrap_or_else(tables);
        let write = |t: &wgpu::Texture, mip: u32, width: usize, height: usize, rgb: &[[f32; 3]]| {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: t, mip_level: mip, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                &half_bytes(rgb),
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(8 * width as u32), rows_per_image: Some(height as u32) },
                wgpu::Extent3d { width: width as u32, height: height as u32, depth_or_array_layers: 1 },
            );
        };
        for (k, level) in levels.iter().enumerate() {
            write(&self.specular_t, k as u32, level.width, level.height(), &level.rgb);
        }
        write(&self.diffuse_t, 0, diffuse.width, diffuse.height(), &diffuse.rgb);
        write(&self.dfg_t, 0, DFG_SIZE, DFG_SIZE, &dfg);
    }

    /// The group-0 layout entries for the studio (bindings 3 to 6).
    pub fn layout_entries() -> [wgpu::BindGroupLayoutEntry; 4] {
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        [
            texture(3),
            texture(4),
            wgpu::BindGroupLayoutEntry {
                binding: 5,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            texture(6),
        ]
    }

    /// The group-0 entries for the studio, matching [`Environment::layout_entries`].
    pub fn entries(&self) -> [wgpu::BindGroupEntry<'_>; 4] {
        [
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.specular) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&self.diffuse) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&self.dfg) },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uv_and_direction_agree() {
        for (x, y) in [(0, 0), (17, 9), (63, 31), (32, 16)] {
            let (u, v) = uv(direction(x, y, 64));
            assert!((u * 64.0 - (x as f32 + 0.5)).abs() < 1e-3 && (v * 32.0 - (y as f32 + 0.5)).abs() < 1e-3, "{x},{y}: {u},{v}");
        }
    }

    #[test]
    fn the_view_frame_sits_where_the_module_says() {
        // Toward the eye on the seam, straight ahead in the middle, right of ahead past it, up on top.
        let close = |(u, v): (f32, f32), want: (f32, f32)| (u.rem_euclid(1.0) - want.0).abs() < 1e-4 && (v - want.1).abs() < 1e-4;
        assert!(close(uv([0.0, 0.0, 1.0]), (0.0, 0.5)));
        assert!(close(uv([0.0, 0.0, -1.0]), (0.5, 0.5)));
        assert!(close(uv([1.0, 0.0, 0.0]), (0.75, 0.5)));
        assert!(uv([0.0, 1.0, 0.0]).1.abs() < 1e-4);
    }

    #[test]
    fn the_shader_knows_the_specular_mip_count() {
        assert!(crate::view::SHADER.contains(&format!("const STUDIO_MIPS: f32 = {SPECULAR_LEVELS}.0;")));
    }

    #[test]
    fn half_floats_round_trip_typical_radiance() {
        for (f, bits) in [(0.0f32, 0u16), (1.0, 0x3c00), (0.5, 0x3800), (2.0, 0x4000), (1e9, 0x7bff)] {
            assert_eq!(half(f), bits, "{f}");
        }
    }

    #[test]
    fn a_furnace_comes_back_white_at_every_mip_and_in_the_irradiance() {
        let (levels, diffuse) = prefilter(Equirect { width: 64, rgb: vec![[1.0; 3]; 64 * 32] });
        assert_eq!(levels.len(), SPECULAR_LEVELS as usize);
        for (k, e) in levels.iter().chain([&diffuse]).enumerate() {
            for p in &e.rgb {
                assert!(p.iter().all(|c| (c - 1.0).abs() < 0.01), "map {k}: {p:?}");
            }
        }
    }

    #[test]
    fn the_room_is_model_viewers() {
        let blocks: Vec<Block> = std::iter::once(Block { centre: dropped(ROOM.0), size: ROOM.1, turn: 0.0, glow: None })
            .chain(ROOM_BOXES.iter().map(|&(p, turn, size)| Block { centre: dropped(p), size, turn, glow: None }))
            .chain(ROOM_PANELS.iter().map(|&(glow, p, size)| Block { centre: dropped(p), size, turn: 0.0, glow: Some(glow) }))
            .collect();
        // Each wall panel is seen at its own radiance, from where the scene puts it.
        for (glow, p, _) in &ROOM_PANELS[..6] {
            assert_eq!(room_radiance(&blocks, normalized(dropped(*p))), *glow, "{p:?}");
        }
        // The floor straight below (4.55 down), lit by the light at (0.5, 10.5, 0.5): 400 / π ×
        // three.js attenuation × cos.
        let (h, d) = (10.5f32 + 4.55, (15.05f32 * 15.05 + 0.5).sqrt());
        let fade = (1.0 - (d / 28.0).powi(4)).powi(2);
        let want = 400.0 / PI * fade / (d * d) * h / d;
        assert!((room_radiance(&blocks, [0.0, -1.0, 0.0]) - want).abs() < 1e-4, "{want}");
        // The box at (2.291, -4.256, -2.621) turned -0.286 is hit on the side facing the eye.
        let (t, n) = hit(
            &Block { centre: [2.291, -4.256, -2.621], size: [1.546, 1.552, 1.496], turn: -0.286, glow: None },
            normalized([2.291, -4.256, -2.621]),
            false,
        )
        .unwrap_or_default();
        assert!(t > 0.0 && t < 5.5 && dot(n, normalized([2.291, -4.256, -2.621])) < 0.0, "{t} {n:?}");
    }

    #[test]
    fn the_rooms_light_reaches_what_an_upright_mirror_face_sees() {
        // In the iso view an upright mirror face reflects about 35° below the horizon; the
        // product-photo studio gave it 0.026 of a white face's light there, black on screen.
        let (levels, diffuse) = prefilter(room());
        let up = diffuse.sample([0.0, 1.0, 0.0])[0];
        let e = (-35f32).to_radians();
        let seen: f32 =
            (0..16).map(|k| k as f32 * PI / 8.0).map(|a| levels[0].sample([e.cos() * a.sin(), e.sin(), e.cos() * a.cos()])[0]).sum::<f32>() / 16.0;
        let side = diffuse.sample([0.0, 0.0, 1.0])[0];
        eprintln!("irradiance/π up {up}, toward the eye {side}; mirror at -35° {seen}");
        assert!(seen > 0.1 * up, "{seen} vs {up}");
    }

    #[test]
    fn the_brdf_table_matches_known_values() {
        // Mirror: Schlick's Fresnel exactly, A + B = 1.
        for nv in [0.1f32, 0.25, 0.5, 1.0] {
            let [a, b] = dfg_at(nv, 0.0, 64);
            assert!((a + b - 1.0).abs() < 1e-3 && (b - (1.0 - nv).powi(5)).abs() < 1e-3, "{nv}: {a} {b}");
        }
        // Roughness 1: A + B = 1 - n·v ln((1 + n·v) / n·v), height-correlated Smith's closed form.
        for nv in [0.25f32, 0.5, 1.0] {
            let [a, b] = dfg_at(nv, 1.0, DFG_SAMPLES);
            let want = 1.0 - nv * ((1.0 + nv) / nv).ln();
            assert!((a + b - want).abs() < 0.01, "{nv}: {} want {want}", a + b);
        }
        // Khronos glTF Sample Renderer v1.0.8 assets/images/lut_ggx.png (8-bit, its texel centres).
        for (nv, r, want) in [
            (0.5f32, 0.5f32, [0.835f32, 0.024]),
            (1.0, 0.5, [0.914, 0.0]),
            (0.5, 1.0, [0.447, 0.004]),
            (1.0, 1.0, [0.306, 0.0]),
            (0.0625, 0.75, [0.827, 0.063]),
        ] {
            let got = dfg_at(nv, r, DFG_SAMPLES);
            assert!((got[0] - want[0]).abs() < 0.01 && (got[1] - want[1]).abs() < 0.01, "({nv}, {r}): {got:?} want {want:?}");
        }
        assert!(dfg().iter().all(|p| p.iter().all(|c| c.is_finite() && *c >= 0.0) && p[0] + p[1] <= 1.001));
    }

    #[test]
    fn the_studio_prefilters_to_finite_levels() {
        let (levels, diffuse) = prefilter(room());
        assert_eq!(levels.first().map(|l| l.width), Some(SPECULAR_WIDTH));
        assert!(levels.iter().chain([&diffuse]).all(|l| l.rgb.iter().flatten().all(|c| c.is_finite() && *c >= 0.0)));
    }
}

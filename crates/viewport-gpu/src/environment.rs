//! The studio the modeling viewport lights finished faces with: dark walls, a lighter floor, a
//! softbox over the part and two upright strip lights ([`studio`]), drawn once on the CPU and
//! prefiltered for image-based lighting (split-sum): a specular map whose mip `k` is the studio
//! seen through a GGX lobe of perceptual roughness `k / (SPECULAR_LEVELS - 1)`, and a small diffuse
//! irradiance map (cosine-weighted), plus the GGX environment-BRDF table (`dfg`) the shader weights
//! them with. A face looking up gets about unit light, upright faces a quarter to a half of it, and
//! a metal reflects bright panels against dark walls, so a part reads by its shading (#19994).
//!
//! Both maps are equirectangular in the shader's frame, the view's (x right, y up, z toward the
//! eye): a direction `(x, y, z)` sits at `u = 0.5 + atan2(x, -z) / 2π`, `v = acos(y) / π`.

use std::f32::consts::PI;

/// The studio's map width, sampled 2 × 2 per texel: twice the specular map's, as `prefilter` reads.
const STUDIO_WIDTH: usize = 2 * SPECULAR_WIDTH;

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

/// The studio's dark walls (above the horizon) and its grey floor, lit by the panels.
const STUDIO_WALL: f32 = 0.05;
const STUDIO_FLOOR: f32 = 0.3;
/// The softbox over the part, on the ceiling plane y = 1 (x right, z toward the eye): x from -0.8
/// to 0.8, z from -0.6 to 0.9, so it leans toward the eye; and its radiance.
const SOFTBOX: ([f32; 2], [f32; 2], f32) = ([-0.8, 0.8], [-0.6, 0.9], 1.58);
/// Upright panels: azimuth centre and half width (degrees from the eye's direction, + to the
/// right), elevation range (degrees) and radiance. A key strip in front on the left, and a wide
/// backdrop behind the part, which flat faces seen from above reflect, and the floor the key
/// strip lights, which gives upright curves a light band.
const PANELS: [(f32, f32, [f32; 2], f32); 3] = [(-71.0, 9.0, [-5.0, 55.0], 2.1), (180.0, 70.0, [8.0, 60.0], 0.5), (-55.0, 30.0, [-60.0, -12.0], 0.3)];
/// Panel edges fade over this much (in the panel's own measure) so the mirror map has no stair steps.
const SOFT: f32 = 0.03;

/// 1 inside `lo..hi`, 0 outside, fading over `soft` at each end.
fn inside(t: f32, [lo, hi]: [f32; 2], soft: f32) -> f32 {
    let ramp = |e: f32| (e / soft + 0.5).clamp(0.0, 1.0);
    ramp(t - lo) * ramp(hi - t)
}

/// The radiance the studio shows along the unit direction `d` (the view's frame, see the module).
fn studio_radiance(d: [f32; 3]) -> f32 {
    let mut l = if d[1] < 0.0 { STUDIO_FLOOR } else { STUDIO_WALL };
    if d[1] > 1e-3 {
        let (x, z) = (d[0] / d[1], d[2] / d[1]);
        l += SOFTBOX.2 * inside(x, SOFTBOX.0, SOFT * 4.0) * inside(z, SOFTBOX.1, SOFT * 4.0);
    }
    let azimuth = d[0].atan2(d[2]).to_degrees();
    let elevation = d[1].clamp(-1.0, 1.0).asin().to_degrees();
    for (centre, half, el, glow) in PANELS {
        let off = (azimuth - centre + 540.0).rem_euclid(360.0) - 180.0;
        l += glow * inside(off, [-half, half], SOFT * 60.0) * inside(elevation, el, SOFT * 60.0);
    }
    l
}

/// The studio as an equirectangular map in the view's frame, each texel the mean of 2 × 2 samples.
fn studio() -> Equirect {
    let fine = 2 * STUDIO_WIDTH;
    let mut rgb = Vec::with_capacity(STUDIO_WIDTH * STUDIO_WIDTH / 2);
    for y in 0..STUDIO_WIDTH / 2 {
        for x in 0..STUDIO_WIDTH {
            let l = [(0, 0), (1, 0), (0, 1), (1, 1)].iter().map(|(i, j)| studio_radiance(direction(2 * x + i, 2 * y + j, fine))).sum::<f32>() / 4.0;
            rgb.push([l; 3]);
        }
    }
    Equirect { width: STUDIO_WIDTH, rgb }
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

/// Draw and prefilter the studio and build the environment-BRDF table: about 0.23 s of CPU (M-series,
/// release), so the renderer builds them beside its pipelines ([`Environment::set`]).
pub(crate) fn tables() -> Tables {
    let (levels, diffuse) = prefilter(studio());
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
    fn the_studio_lights_a_part_from_above_and_leaves_its_sides_darker() {
        // A face looking up gets about unit light, so PBR Neutral shows it in its assigned colour;
        // upright faces the eye can see get a third to a half of that, the bottom less (#19994: the
        // old room lit every side alike, 0.92 to 0.98, and parts looked flat and self-lit).
        let (_, diffuse) = prefilter(studio());
        let at = |d: [f32; 3]| diffuse.sample(normalized(d))[0];
        let up = at([0.0, 1.0, 0.0]);
        let sides = [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]].map(at);
        let down = at([0.0, -1.0, 0.0]);
        eprintln!("STUDIO up {up} sides {sides:?} down {down}");
        assert!((0.9..1.1).contains(&up), "{up}");
        assert!(sides.iter().all(|s| (0.25..0.6).contains(s)), "{sides:?}");
        assert!(down < 0.4, "{down}");
    }

    #[test]
    fn a_mirror_sees_bright_panels_on_dark_walls() {
        // Metals look like metal when what they reflect has contrast: panels many times the walls.
        // An upright mirror in the iso view looks about 35° below the horizon, at the floor, which
        // stays above black so its edges still read.
        let (levels, _) = prefilter(studio());
        let mirror = &levels[0].rgb;
        let (lo, hi) = mirror.iter().fold((f32::MAX, 0.0f32), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])));
        let e = (-35f32).to_radians();
        let seen: f32 =
            (0..16).map(|k| k as f32 * PI / 8.0).map(|a| levels[0].sample([e.cos() * a.sin(), e.sin(), e.cos() * a.cos()])[0]).sum::<f32>() / 16.0;
        let top = levels[0].sample([0.0, (35f32).to_radians().sin(), -(35f32).to_radians().cos()])[0];
        eprintln!("mirror from {lo} to {hi}; at -35° {seen}; a flat top reflects {top}");
        assert!(hi > 20.0 * lo, "{lo} {hi}");
        assert!(seen > 0.05, "{seen}");
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
        let (levels, diffuse) = prefilter(studio());
        assert_eq!(levels.first().map(|l| l.width), Some(SPECULAR_WIDTH));
        assert!(levels.iter().chain([&diffuse]).all(|l| l.rgb.iter().flatten().all(|c| c.is_finite() && *c >= 0.0)));
    }
}

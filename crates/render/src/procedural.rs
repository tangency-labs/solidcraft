//! Procedural material patterns (#2163): a pure, bounded reference that turns a validated recipe
//! and a validated point in the body's definition-local frame (millimetres) into linear colour,
//! roughness and a height with its analytic gradient. Nothing is stored and nothing is sampled
//! from an image; patterns are solid (3D), so they need no UVs and have no seams.
//!
//! The contract the consumers rely on:
//! - **Validation:** recipes and points are validated before evaluation ([`Recipe::validate`],
//!   [`Point::new`]); evaluation itself cannot fail.
//! - **Independent outputs:** colour, roughness and height vary independently. `None` keeps the
//!   material's own value.
//! - **Shading only:** the consumer applies the definition-local frame once, and projects
//!   [`Sample::height_gradient`] onto the shading tangent plane to perturb the *shading* normal.
//!   Geometric normals, silhouettes and shadow tests stay untouched.
//! - **Zero amplitude is the plain material:** [`Sample::PLAIN`] exactly.
//! - **Deterministic:** integer lattice hashing, polynomial fades and IEEE `+ - × ÷ √ floor` only,
//!   with no platform `sin`, `cos` or `exp`. The same inputs give bit-identical outputs everywhere.
//! - **Bounded:** at most [`MAX_OCTAVES`] octaves per noise sum and a fixed number of sums per
//!   family (oak uses two, of 3 and 4 octaves), with no loop that depends on the input.
//! - **Gradients** are exact derivatives of the height, except where the height is not
//!   differentiable: at oak's ring boundaries (a step by design) and within 1e-9 mm of oak's grain
//!   axis, where the radial term is set to zero.
//! - **Unfiltered:** this is a point reference. A renderer that installs it needs its own
//!   footprint/filtering contract for primary and secondary rays; it must not use this as level 0.
//!
//! Not in scope here: persistence, bindings, import, GPU consumption (a WGSL port must be tested
//! against this reference first) and anisotropic optical response.

/// The working capability name a recipe requires (#2163). Provisional: the library and design
/// formats that will carry recipes own the final name and its versioning.
pub const PROVISIONAL_CAPABILITY: &str = "appearance.procedural/draft";

/// Largest absolute value (mm) of each coordinate of a point: 1 km per axis, not a radius.
pub const MAX_COORDINATE_MM: f64 = 1.0e6;

/// Octaves of one noise sum at most: with the fixed number of sums per family, the cost bound of
/// one evaluation.
pub const MAX_OCTAVES: usize = 4;

/// Why a recipe or point was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternError {
    /// A parameter is not finite or outside its documented range; names the parameter.
    Parameter(&'static str),
    /// A coordinate of the point is not finite or exceeds ±[`MAX_COORDINATE_MM`].
    Point,
}

impl std::fmt::Display for PatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatternError::Parameter(name) => write!(f, "procedural pattern parameter `{name}` is not finite or out of range"),
            PatternError::Point => write!(f, "procedural pattern point must be finite, each coordinate within ±{MAX_COORDINATE_MM} mm"),
        }
    }
}

impl std::error::Error for PatternError {}

/// A point in the body's definition-local frame, in millimetres, known to be finite and bounded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point([f64; 3]);

impl Point {
    pub fn new(mm: [f64; 3]) -> Result<Self, PatternError> {
        if mm.iter().all(|c| c.is_finite() && c.abs() <= MAX_COORDINATE_MM) { Ok(Self(mm)) } else { Err(PatternError::Point) }
    }

    pub fn mm(self) -> [f64; 3] {
        self.0
    }
}

/// What one evaluation returns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Linear RGB in 0..=1; `None` keeps the material's own colour.
    pub base_color: Option<[f64; 3]>,
    /// In [`MIN_ROUGHNESS`]..=1; `None` keeps the material's own roughness.
    pub roughness: Option<f64>,
    /// Height above the surface in mm (negative into it).
    pub height_mm: f64,
    /// d(height)/d(point), dimensionless, in the definition-local frame.
    pub height_gradient: [f64; 3],
}

impl Sample {
    /// The plain material: what every recipe returns at zero amplitude.
    pub const PLAIN: Sample = Sample { base_color: None, roughness: None, height_mm: 0.0, height_gradient: [0.0; 3] };
}

/// Smallest roughness a pattern returns (matches the renderers' floor).
pub const MIN_ROUGHNESS: f64 = 0.02;

/// Side-grain oak. The grain runs along local +X; rings are cylinders around the X axis, so
/// faces parallel to X show the familiar figure. **End grain** (faces across X) is not modelled
/// in this slice: such faces show rings without end-grain pores, which consumers must not
/// present as end grain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oak {
    /// Distance between growth rings, 0.5..=20 mm.
    pub ring_spacing_mm: f64,
    /// Ring distortion, 0..=1 (0: perfect cylinders).
    pub figure: f64,
    /// Earlywood and latewood colours, linear RGB 0..=1.
    pub earlywood: [f64; 3],
    pub latewood: [f64; 3],
    /// Roughness of the finish, MIN_ROUGHNESS..=1, raised slightly in pores.
    pub roughness: f64,
    /// Pore depth, 0..=0.2 mm; also how visible pores are in colour (fully from 0.05 mm).
    pub pore_depth_mm: f64,
    /// Strength of the whole pattern, 0..=1; 0 is the plain material.
    pub amplitude: f64,
    pub seed: u32,
}

/// Textured ABS stipple. `grade_mm` is the feature size of the texture (fine to coarse).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TexturedAbs {
    /// Feature size, 0.05..=2 mm.
    pub grade_mm: f64,
    /// Peak-to-valley height, 0..=0.1 mm.
    pub depth_mm: f64,
    /// Roughness in the valleys and on the peaks, MIN_ROUGHNESS..=1.
    pub roughness_valley: f64,
    pub roughness_peak: f64,
    /// 0..=1; 0 is the plain material.
    pub amplitude: f64,
    pub seed: u32,
}

/// Brushed metal: streaks along local +X in colour and roughness, with shallow grooves. This is
/// an isotropic stand-in: no anisotropic optical response is claimed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brushed {
    /// Streak width across the brushing direction, 0.005..=1 mm.
    pub streak_width_mm: f64,
    /// Streak length along it, 1..=1000 mm, and at least 10 × the width.
    pub streak_length_mm: f64,
    /// Metal colour, linear RGB 0..=1, and its relative variation, 0..=0.5.
    pub tint: [f64; 3],
    pub color_variation: f64,
    /// Mean roughness, MIN_ROUGHNESS..=1, and its relative variation, 0..=0.5.
    pub roughness: f64,
    pub roughness_variation: f64,
    /// Groove depth, 0..=0.01 mm.
    pub groove_depth_mm: f64,
    /// 0..=1; 0 is the plain material.
    pub amplitude: f64,
    pub seed: u32,
}

/// One pattern family with its parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Recipe {
    Oak(Oak),
    TexturedAbs(TexturedAbs),
    Brushed(Brushed),
}

/// A recipe whose every parameter passed [`Recipe::validate`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ValidRecipe(Recipe);

fn check(ok: bool, name: &'static str) -> Result<(), PatternError> {
    if ok { Ok(()) } else { Err(PatternError::Parameter(name)) }
}
fn within(v: f64, lo: f64, hi: f64) -> bool {
    v.is_finite() && (lo..=hi).contains(&v)
}
fn colour(c: [f64; 3]) -> bool {
    c.iter().all(|v| within(*v, 0.0, 1.0))
}

impl Recipe {
    pub fn validate(self) -> Result<ValidRecipe, PatternError> {
        match self {
            Recipe::Oak(o) => {
                check(within(o.ring_spacing_mm, 0.5, 20.0), "ring_spacing_mm")?;
                check(within(o.figure, 0.0, 1.0), "figure")?;
                check(colour(o.earlywood), "earlywood")?;
                check(colour(o.latewood), "latewood")?;
                check(within(o.roughness, MIN_ROUGHNESS, 1.0), "roughness")?;
                check(within(o.pore_depth_mm, 0.0, 0.2), "pore_depth_mm")?;
                check(within(o.amplitude, 0.0, 1.0), "amplitude")?;
            }
            Recipe::TexturedAbs(a) => {
                check(within(a.grade_mm, 0.05, 2.0), "grade_mm")?;
                check(within(a.depth_mm, 0.0, 0.1), "depth_mm")?;
                check(within(a.roughness_valley, MIN_ROUGHNESS, 1.0), "roughness_valley")?;
                check(within(a.roughness_peak, MIN_ROUGHNESS, 1.0), "roughness_peak")?;
                check(within(a.amplitude, 0.0, 1.0), "amplitude")?;
            }
            Recipe::Brushed(b) => {
                check(within(b.streak_width_mm, 0.005, 1.0), "streak_width_mm")?;
                check(within(b.streak_length_mm, 1.0, 1000.0) && b.streak_length_mm >= 10.0 * b.streak_width_mm, "streak_length_mm")?;
                check(colour(b.tint), "tint")?;
                check(within(b.color_variation, 0.0, 0.5), "color_variation")?;
                check(within(b.roughness, MIN_ROUGHNESS, 1.0), "roughness")?;
                check(within(b.roughness_variation, 0.0, 0.5), "roughness_variation")?;
                check(within(b.groove_depth_mm, 0.0, 0.01), "groove_depth_mm")?;
                check(within(b.amplitude, 0.0, 1.0), "amplitude")?;
            }
        }
        Ok(ValidRecipe(self))
    }

    /// What this family does not model yet, for consumers to report rather than approximate.
    pub fn limitations(&self) -> &'static [&'static str] {
        match self {
            Recipe::Oak(_) => &[
                "end grain (faces across the grain show rings without end-grain pores)",
                "height gradient at ring boundaries (a step) and within 1e-9 mm of the grain axis (radial term zero)",
            ],
            Recipe::TexturedAbs(_) => &[],
            Recipe::Brushed(_) => &["anisotropic reflection (streaks vary colour and roughness only)"],
        }
    }
}

impl ValidRecipe {
    pub fn recipe(&self) -> &Recipe {
        &self.0
    }
}

/// Evaluate `recipe` at `point`. Infallible: both inputs are validated.
pub fn evaluate(recipe: &ValidRecipe, point: Point) -> Sample {
    match recipe.0 {
        Recipe::Oak(o) if o.amplitude > 0.0 => oak(&o, point.0),
        Recipe::TexturedAbs(a) if a.amplitude > 0.0 => textured_abs(&a, point.0),
        Recipe::Brushed(b) if b.amplitude > 0.0 => brushed(&b, point.0),
        _ => Sample::PLAIN,
    }
}

// ---- families --------------------------------------------------------------------------------

fn oak(o: &Oak, p: [f64; 3]) -> Sample {
    let a = o.amplitude;
    let s = o.ring_spacing_mm;
    // Ring distortion: a low-frequency displacement of the radius around the grain axis (X).
    let warp_scale = 1.0 / (4.0 * s);
    let (warp, dwarp) = fbm(scale(p, [warp_scale; 3]), o.seed, 3);
    let r = (p[1] * p[1] + p[2] * p[2]).sqrt();
    // The radius is not differentiable on the grain axis; within 1e-9 mm of it the radial term of
    // the gradient is zero (documented in the module contract).
    let dr = if r > 1.0e-9 { [0.0, p[1] / r, p[2] / r] } else { [0.0; 3] };
    let radius = r + o.figure * 2.0 * s * warp;
    let dradius = [0, 1, 2].map(|i| dr[i] + o.figure * 2.0 * s * dwarp[i] * warp_scale);
    let t = fract(radius / s);
    // Latewood: the last part of each ring, with a soft rise and a sharp boundary to the next
    // ring's earlywood, as in real growth rings. The boundary is a step; gradients are taken
    // within a ring.
    let (late, dlate_dt) = smoothstep_d(0.7, 0.95, t);
    let dlate = dradius.map(|d| dlate_dt * d / s);
    let ring_colour = mix(o.earlywood, o.latewood, late);
    // Pores: elongated along the grain, in the earlywood.
    let pore_scale = [1.0 / 1.2, 1.0 / 0.12, 1.0 / 0.12];
    let (n, dn) = fbm(scale(p, pore_scale), o.seed.wrapping_add(0x9e37_79b9), MAX_OCTAVES);
    let (mask, dmask) = smoothstep_d(0.15, 0.55, n);
    let earlywood = 1.0 - late;
    let pore = mask * earlywood;
    let dpore = [0, 1, 2].map(|i| dmask * dn[i] * pore_scale[i] * earlywood - mask * dlate[i]);
    // Shallow pores barely show; none at zero depth.
    let visible = pore * (o.pore_depth_mm / 0.05).min(1.0);
    let darker = mix(ring_colour, scale(ring_colour, [0.55; 3]), visible);
    Sample {
        base_color: Some(clamp_colour(mix(ring_colour, darker, a))),
        roughness: Some((o.roughness + 0.15 * a * visible).clamp(MIN_ROUGHNESS, 1.0)),
        height_mm: -o.pore_depth_mm * a * pore,
        height_gradient: dpore.map(|d| -o.pore_depth_mm * a * d),
    }
}

fn textured_abs(t: &TexturedAbs, p: [f64; 3]) -> Sample {
    let a = t.amplitude;
    let k = 1.0 / t.grade_mm;
    let (n, dn) = fbm(scale(p, [k; 3]), t.seed, MAX_OCTAVES);
    // n is in about -1..1; heights are centred on the surface.
    let height = 0.5 * t.depth_mm * a * n;
    let grad = [0, 1, 2].map(|i| 0.5 * t.depth_mm * a * dn[i] * k);
    let peak = (0.5 + 0.5 * n).clamp(0.0, 1.0);
    let roughness = t.roughness_valley + (t.roughness_peak - t.roughness_valley) * peak;
    Sample { base_color: None, roughness: Some(roughness.clamp(MIN_ROUGHNESS, 1.0)), height_mm: height, height_gradient: grad }
}

fn brushed(b: &Brushed, p: [f64; 3]) -> Sample {
    let a = b.amplitude;
    let k = [1.0 / b.streak_length_mm, 1.0 / b.streak_width_mm, 1.0 / b.streak_width_mm];
    let (n, dn) = fbm(scale(p, k), b.seed, MAX_OCTAVES);
    let colour = scale(b.tint, [1.0 + b.color_variation * a * n; 3]);
    let roughness = b.roughness * (1.0 + b.roughness_variation * a * n);
    Sample {
        base_color: Some(clamp_colour(colour)),
        roughness: Some(roughness.clamp(MIN_ROUGHNESS, 1.0)),
        height_mm: b.groove_depth_mm * a * n,
        height_gradient: [0, 1, 2].map(|i| b.groove_depth_mm * a * dn[i] * k[i]),
    }
}

// ---- deterministic noise ----------------------------------------------------------------------

/// Sum of `octaves` value-noise octaves (frequency ×2, amplitude ×½), normalised to about -1..1,
/// with its analytic gradient.
fn fbm(p: [f64; 3], seed: u32, octaves: usize) -> (f64, [f64; 3]) {
    let (mut sum, mut grad, mut weight, mut freq, mut norm) = (0.0, [0.0; 3], 1.0, 1.0, 0.0);
    for octave in 0..octaves.min(MAX_OCTAVES) {
        let (v, d) = value_noise(scale(p, [freq; 3]), seed.wrapping_add((octave as u32).wrapping_mul(0x632b_e5ab)));
        sum += weight * v;
        for (g, d) in grad.iter_mut().zip(d) {
            *g += weight * freq * d;
        }
        norm += weight;
        weight *= 0.5;
        freq *= 2.0;
    }
    (sum / norm, grad.map(|g| g / norm))
}

/// Trilinear value noise in -1..1 with quintic fades (C2), and its analytic gradient.
fn value_noise(p: [f64; 3], seed: u32) -> (f64, [f64; 3]) {
    let cell = p.map(f64::floor);
    let f = [p[0] - cell[0], p[1] - cell[1], p[2] - cell[2]];
    let w = f.map(fade);
    let dw = f.map(fade_d);
    // Points are within 1e6 mm and frequencies at most 1/0.005 × 2^3, so cells fit an i64.
    let c = cell.map(|v| v as i64);
    let mut value = 0.0;
    let mut grad = [0.0; 3];
    for corner in 0..8u32 {
        let o = [corner & 1, (corner >> 1) & 1, (corner >> 2) & 1];
        let h = lattice(c[0] + o[0] as i64, c[1] + o[1] as i64, c[2] + o[2] as i64, seed);
        let wx = if o[0] == 1 { w[0] } else { 1.0 - w[0] };
        let wy = if o[1] == 1 { w[1] } else { 1.0 - w[1] };
        let wz = if o[2] == 1 { w[2] } else { 1.0 - w[2] };
        let sx = if o[0] == 1 { dw[0] } else { -dw[0] };
        let sy = if o[1] == 1 { dw[1] } else { -dw[1] };
        let sz = if o[2] == 1 { dw[2] } else { -dw[2] };
        value += h * wx * wy * wz;
        grad[0] += h * sx * wy * wz;
        grad[1] += h * wx * sy * wz;
        grad[2] += h * wx * wy * sz;
    }
    (value, grad)
}

/// A lattice corner's value in -1..1: splitmix64 of the coordinates and seed.
fn lattice(x: i64, y: i64, z: i64, seed: u32) -> f64 {
    let mut h = (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (y as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f)
        ^ (z as u64).wrapping_mul(0x1656_67b1_9e37_79f9)
        ^ u64::from(seed).wrapping_mul(0x27d4_eb2f_1656_67c5);
    h ^= h >> 30;
    h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94d0_49bb_1331_11eb);
    h ^= h >> 31;
    // The top 53 bits as a fraction in 0..1, mapped to -1..1.
    (h >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
}

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}
fn fade_d(t: f64) -> f64 {
    30.0 * t * t * (t - 1.0) * (t - 1.0)
}

/// Cubic smoothstep and its derivative with respect to `v`.
fn smoothstep_d(lo: f64, hi: f64, v: f64) -> (f64, f64) {
    let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
    let d = if t > 0.0 && t < 1.0 { 6.0 * t * (1.0 - t) / (hi - lo) } else { 0.0 };
    (t * t * (3.0 - 2.0 * t), d)
}

fn fract(v: f64) -> f64 {
    v - v.floor()
}
fn scale(a: [f64; 3], k: [f64; 3]) -> [f64; 3] {
    [a[0] * k[0], a[1] * k[1], a[2] * k[2]]
}
fn mix(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}
fn clamp_colour(c: [f64; 3]) -> [f64; 3] {
    c.map(|v| v.clamp(0.0, 1.0))
}

#[cfg(test)]
#[path = "procedural_tests.rs"]
mod tests;

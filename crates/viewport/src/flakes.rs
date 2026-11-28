//! Original procedural metallic flakes for paint under the clear coat.
//! Space is cut into irregular cells of about one flake size. A hash of a cell decides
//! whether it holds a flake, how far the flake tilts from the surface and how deep it
//! sits in the paint. A point on a flake is shaded as that flake's metal about the
//! tilted normal; the clear coat above keeps the true normal. No texture is read, and
//! the cells do not follow the triangles, so there are no seams. This is a bounded
//! preview model, not a simulation of pigment layers. See docs/render-flakes.md.
use crate::surface::{MIN_ROUGHNESS, SurfaceError, unit_direction};
use crate::vec::Vec3Ops;
use solidcore_record::Vec3;
use std::f64::consts::{FRAC_PI_4, TAU};

/// Smallest and largest admitted flake size (mm).
pub const MIN_FLAKE_SIZE_MM: f64 = 0.005;
pub const MAX_FLAKE_SIZE_MM: f64 = 10.0;
/// The tilt of the most tilted flake at spread 1 (radians).
pub const MAX_FLAKE_TILT: f64 = FRAC_PI_4;
/// The cell pattern repeats after this many cells along each axis.
pub const FLAKE_PERIOD: i64 = 65536;
/// Positions further than this many cells from the origin are refused.
const MAX_CELL: f64 = 1.0e12;

/// Authored flakes for one paint. Private fields keep every instance finite and bounded.
/// Every face and triangle carries its surface, so the layer is stored in 20 bytes:
/// fractions and color in steps of 1/65535, the size in single precision. The GPU reads
/// the same stored values, so both renderers place the same flakes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlakeLayer {
    size_mm: f32,
    coverage: u16,
    spread: u16,
    depth: u16,
    color: [u16; 3],
    metallic: u16,
    roughness: u16,
}

/// The flake under one surface point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flake {
    /// Unit normal of the flake, within [`MAX_FLAKE_TILT`] of the surface normal.
    pub normal: Vec3,
    /// How far the flake sits in the paint: 0 is on top, 1 is fully covered by the base paint.
    pub depth: f64,
}

fn fraction(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

const STEPS: f64 = 65535.0;

/// A checked fraction in steps of 1/65535; 0 and 1 stay exact.
fn quantize(value: f64) -> u16 {
    (value * STEPS).round().clamp(0.0, STEPS) as u16
}

fn level(steps: u16) -> f64 {
    f64::from(steps) / STEPS
}

impl FlakeLayer {
    /// `coverage` is the share of the surface that flakes cover. `spread` is the tilt of the
    /// flakes as a share of [`MAX_FLAKE_TILT`]. `depth` is how deep the deepest flake sits.
    /// `color` is a linear-sRGB reflectance.
    pub fn new(coverage: f64, size_mm: f64, spread: f64, depth: f64, color: Vec3, metallic: f64, roughness: f64) -> Result<Self, SurfaceError> {
        if !fraction(coverage) || !fraction(spread) || !fraction(depth) {
            return Err(SurfaceError::FlakeFraction);
        }
        if !size_mm.is_finite() || !(MIN_FLAKE_SIZE_MM..=MAX_FLAKE_SIZE_MM).contains(&size_mm) {
            return Err(SurfaceError::FlakeSize);
        }
        if !color.is_finite() || ![color.x, color.y, color.z].into_iter().all(|c| (0.0..=1.0).contains(&c)) {
            return Err(SurfaceError::BaseColor);
        }
        if !fraction(metallic) {
            return Err(SurfaceError::Metallic);
        }
        if !roughness.is_finite() || !(MIN_ROUGHNESS..=1.0).contains(&roughness) {
            return Err(SurfaceError::Roughness);
        }
        Ok(Self {
            size_mm: size_mm as f32,
            coverage: quantize(coverage),
            spread: quantize(spread),
            depth: quantize(depth),
            color: [color.x, color.y, color.z].map(quantize),
            metallic: quantize(metallic),
            roughness: quantize(roughness),
        })
    }
    /// The same flakes covering another share of the surface, as when a layer above hides some.
    pub fn with_coverage(mut self, coverage: f64) -> Result<Self, SurfaceError> {
        if !fraction(coverage) {
            return Err(SurfaceError::FlakeFraction);
        }
        self.coverage = quantize(coverage);
        Ok(self)
    }
    // The stored values read back inside the admitted ranges, so `new` accepts them again.
    pub fn coverage(self) -> f64 {
        level(self.coverage)
    }
    pub fn size_mm(self) -> f64 {
        f64::from(self.size_mm).clamp(MIN_FLAKE_SIZE_MM, MAX_FLAKE_SIZE_MM)
    }
    pub fn spread(self) -> f64 {
        level(self.spread)
    }
    pub fn depth(self) -> f64 {
        level(self.depth)
    }
    pub fn color(self) -> Vec3 {
        let [r, g, b] = self.color.map(level);
        Vec3::new(r, g, b)
    }
    pub fn metallic(self) -> f64 {
        level(self.metallic)
    }
    pub fn roughness(self) -> f64 {
        level(self.roughness).max(MIN_ROUGHNESS)
    }

    /// The flake at `position_mm` on a surface with the unit normal `normal`, or `None`
    /// between flakes. The same position always gives the same flake.
    pub fn at(self, position_mm: Vec3, normal: Vec3) -> Result<Option<Flake>, SurfaceError> {
        let n = unit_direction(normal)?;
        let q = position_mm / self.size_mm();
        if !q.is_finite() || q.x.abs().max(q.y.abs()).max(q.z.abs()) > MAX_CELL {
            return Err(SurfaceError::Position);
        }
        let coverage = self.coverage();
        if coverage == 0.0 {
            return Ok(None);
        }
        let cell = nearest_cell(q);
        if coverage < 1.0 && unit(hash(cell, 3)) >= coverage {
            return Ok(None);
        }
        let depth = self.depth() * unit(hash(cell, 6));
        if self.spread == 0 {
            return Ok(Some(Flake { normal: n, depth }));
        }
        // The square root spreads the tilted normals evenly over their cone's disk.
        let tilt = self.spread() * MAX_FLAKE_TILT * unit(hash(cell, 4)).sqrt();
        let (turn_sine, turn_cosine) = (TAU * unit(hash(cell, 5))).sin_cos();
        let (tilt_sine, tilt_cosine) = tilt.sin_cos();
        let tangent = flake_tangent(n)?;
        let bitangent = n.cross(tangent);
        let normal = unit_direction(tangent * (tilt_sine * turn_cosine) + bitangent * (tilt_sine * turn_sine) + n * tilt_cosine)?;
        Ok(Some(Flake { normal, depth }))
    }
}

/// A unit tangent of `n` that turns continuously with `n`. Each world axis is projected onto the
/// tangent plane and weighted by its own length, so the sum has no switch between frames. The sum
/// vanishes only at isolated normals; there one fixed cross product stands in.
fn flake_tangent(n: Vec3) -> Result<Vec3, SurfaceError> {
    let mut sum = Vec3::ZERO;
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        let projected = axis - n * axis.dot(n);
        sum += projected * projected.len();
    }
    match sum.normalized() {
        Some(tangent) => Ok(tangent),
        None if n.x.abs() < 0.9 => unit_direction(Vec3::X.cross(n)),
        None => unit_direction(Vec3::Y.cross(n)),
    }
}

/// The cell whose jittered centre is nearest to `q`. Each lattice cell holds one centre,
/// so the nearest one is always in the 27 cells around `q`.
fn nearest_cell(q: Vec3) -> [i64; 3] {
    let floor = [q.x.floor(), q.y.floor(), q.z.floor()];
    let base = floor.map(|value| value as i64);
    let mut nearest = base;
    let mut least = f64::INFINITY;
    for dz in -1..=1_i64 {
        for dy in -1..=1_i64 {
            for dx in -1..=1_i64 {
                let cell = [base[0] + dx, base[1] + dy, base[2] + dz];
                let offset = [dx, dy, dz];
                let mut distance = 0.0;
                for axis in 0..3 {
                    // Subtract the whole cells first: the difference keeps its precision far from the origin.
                    let centre = offset[axis] as f64 + unit(hash(cell, axis as u32));
                    let along = centre - (q_axis(q, axis) - floor[axis]);
                    distance += along * along;
                }
                if distance < least {
                    least = distance;
                    nearest = cell;
                }
            }
        }
    }
    nearest
}

fn q_axis(q: Vec3, axis: usize) -> f64 {
    match axis {
        0 => q.x,
        1 => q.y,
        _ => q.z,
    }
}

/// An integer hash of a cell, the same on every platform: the GPU shader repeats it in `u32`.
fn hash(cell: [i64; 3], salt: u32) -> u32 {
    let [x, y, z] = cell.map(|n| n.rem_euclid(FLAKE_PERIOD) as u32);
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841) ^ z.wrapping_mul(0xcb1a_b31f) ^ salt.wrapping_mul(0x9e37_79b9);
    h = (h ^ (h >> 16)).wrapping_mul(0x7feb_352d);
    h = (h ^ (h >> 15)).wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

/// The top 24 bits of a hash as a number in [0, 1).
fn unit(hash: u32) -> f64 {
    f64::from(hash >> 8) / 16_777_216.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clear_coat::{ClearCoat, lobe_probabilities, surface_pdf};
    use crate::surface::{OpaqueSurface, visible_normal};
    use std::f64::consts::PI;

    fn layer(coverage: f64, spread: f64, depth: f64) -> FlakeLayer {
        FlakeLayer::new(coverage, 0.05, spread, depth, Vec3::new(1.0, 1.0, 1.0), 1.0, 0.2).unwrap()
    }
    /// Points on a tilted plane, a fraction of a flake apart.
    fn plane_point(i: u32, j: u32) -> Vec3 {
        let (u, v) = (f64::from(i) * 0.0173, f64::from(j) * 0.0191);
        Vec3::new(3.0 + u, -2.0 + v, 0.3 * u - 0.2 * v)
    }

    #[test]
    fn admission_rejects_values_out_of_range() {
        let white = Vec3::new(1.0, 1.0, 1.0);
        for bad in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
            assert_eq!(FlakeLayer::new(bad, 0.05, 0.5, 0.5, white, 1.0, 0.2), Err(SurfaceError::FlakeFraction));
            assert_eq!(FlakeLayer::new(0.5, 0.05, bad, 0.5, white, 1.0, 0.2), Err(SurfaceError::FlakeFraction));
            assert_eq!(FlakeLayer::new(0.5, 0.05, 0.5, bad, white, 1.0, 0.2), Err(SurfaceError::FlakeFraction));
            assert_eq!(FlakeLayer::new(0.5, 0.05, 0.5, 0.5, white, bad, 0.2), Err(SurfaceError::Metallic));
            assert_eq!(FlakeLayer::new(0.5, 0.05, 0.5, 0.5, Vec3::new(bad, 0.0, 0.0), 1.0, 0.2), Err(SurfaceError::BaseColor));
        }
        for bad in [f64::NAN, 0.0, 0.0049, 10.01, f64::INFINITY] {
            assert_eq!(FlakeLayer::new(0.5, bad, 0.5, 0.5, white, 1.0, 0.2), Err(SurfaceError::FlakeSize));
        }
        for bad in [f64::NAN, 0.0, 0.044, 1.01] {
            assert_eq!(FlakeLayer::new(0.5, 0.05, 0.5, 0.5, white, 1.0, bad), Err(SurfaceError::Roughness));
        }
        let flakes = layer(0.5, 0.5, 0.5);
        for bad in [Vec3::new(f64::NAN, 0.0, 0.0), Vec3::new(1.0e12, 0.0, 0.0), Vec3::new(0.0, f64::INFINITY, 0.0)] {
            assert_eq!(flakes.at(bad, Vec3::Z), Err(SurfaceError::Position));
        }
        assert_eq!(flakes.at(Vec3::ZERO, Vec3::ZERO), Err(SurfaceError::Direction));
    }

    #[test]
    fn stored_values_stay_admitted_and_the_surface_stays_small() {
        // Every face and triangle holds a surface; the render and GPU memory estimates count it.
        assert!(std::mem::size_of::<FlakeLayer>() <= 20);
        assert!(std::mem::size_of::<OpaqueSurface>() <= 112, "{}", std::mem::size_of::<OpaqueSurface>());
        let low = FlakeLayer::new(0.0, MIN_FLAKE_SIZE_MM, 0.0, 0.0, Vec3::ZERO, 0.0, MIN_ROUGHNESS).unwrap();
        let high = FlakeLayer::new(1.0, MAX_FLAKE_SIZE_MM, 1.0, 1.0, Vec3::new(1.0, 1.0, 1.0), 1.0, 1.0).unwrap();
        for stored in [low, high, layer(0.37, 0.61, 0.29)] {
            let again = FlakeLayer::new(
                stored.coverage(),
                stored.size_mm(),
                stored.spread(),
                stored.depth(),
                stored.color(),
                stored.metallic(),
                stored.roughness(),
            );
            assert_eq!(again, Ok(stored));
        }
        assert_eq!((high.coverage(), high.spread(), high.depth(), low.coverage()), (1.0, 1.0, 1.0, 0.0));
        assert!((layer(0.37, 0.5, 0.5).coverage() - 0.37).abs() <= 0.5 / 65535.0);
    }

    #[test]
    fn coverage_is_the_share_of_the_surface_under_flakes() {
        let n = unit_direction(Vec3::new(-0.3, 0.2, 1.0)).unwrap();
        for coverage in [0.0, 0.1, 0.5, 0.9, 1.0] {
            let flakes = layer(coverage, 0.5, 0.5);
            let mut covered = 0_u32;
            for i in 0..200 {
                for j in 0..200 {
                    covered += u32::from(flakes.at(plane_point(i, j), n).unwrap().is_some());
                }
            }
            let share = f64::from(covered) / 40_000.0;
            assert!((share - coverage).abs() < 0.03, "coverage {coverage}: {share}");
            if coverage == 0.0 || coverage == 1.0 {
                assert_eq!(share, coverage);
            }
        }
    }

    #[test]
    fn the_tilt_frame_turns_smoothly_across_the_old_frame_switch() {
        // The former frame switched at |n.z| = 0.9 on this circle, flipping the tangent from +Y to -Y.
        let steps = 3000;
        let mut previous: Option<Vec3> = None;
        for i in 0..=steps {
            let t = std::f64::consts::PI * f64::from(i) / f64::from(steps);
            let n = Vec3::new(t.sin(), 0.0, t.cos());
            let tangent = flake_tangent(n).unwrap();
            assert!((tangent.len() - 1.0).abs() < 1e-12 && tangent.dot(n).abs() < 1e-12, "step {i}: not a unit tangent");
            if let Some(last) = previous {
                assert!(tangent.dot(last) > 0.999, "step {i}: the tangent jumped");
            }
            previous = Some(tangent);
        }
    }

    #[test]
    fn the_tangent_is_finite_where_the_axis_projections_cancel() {
        // (1, 1, 1) / sqrt(3): the three projections sum to zero.
        let n = unit_direction(Vec3::new(1.0, 1.0, 1.0)).unwrap();
        let tangent = flake_tangent(n).unwrap();
        assert!((tangent.len() - 1.0).abs() < 1e-12 && tangent.dot(n).abs() < 1e-12);
    }

    #[test]
    fn a_flake_is_stable_tilts_within_its_spread_and_spans_several_points() {
        let n = unit_direction(Vec3::new(-0.3, 0.2, 1.0)).unwrap();
        for spread in [0.0, 0.3, 1.0] {
            let flakes = layer(1.0, spread, 0.6);
            let (mut shared, mut widest, mut deepest) = (0_u32, 0.0_f64, 0.0_f64);
            let mut previous = None;
            for i in 0..4000 {
                // Steps of a twentieth of a flake: neighbours are mostly on one flake.
                let p = Vec3::new(1.0 + f64::from(i) * 0.0025, 0.4, 0.3);
                let flake = flakes.at(p, n).unwrap().unwrap();
                assert_eq!(flakes.at(p, n).unwrap(), Some(flake));
                assert!((flake.normal.len() - 1.0).abs() < 1e-12);
                let tilt = flake.normal.dot(n).clamp(-1.0, 1.0).acos();
                assert!(tilt <= spread * MAX_FLAKE_TILT + 1e-9, "spread {spread}: tilt {tilt}");
                assert!((0.0..=0.6).contains(&flake.depth));
                widest = widest.max(tilt);
                deepest = deepest.max(flake.depth);
                shared += u32::from(previous == Some(flake));
                previous = Some(flake);
            }
            assert!(shared > 3000, "spread {spread}: only {shared} neighbours share a flake");
            assert!(deepest > 0.5);
            if spread == 0.0 {
                assert_eq!(widest, 0.0);
            } else {
                assert!(widest > 0.8 * spread * MAX_FLAKE_TILT, "spread {spread}: widest tilt {widest}");
            }
        }
    }

    #[test]
    fn the_pattern_has_no_seam_far_from_the_origin_or_across_a_cell_period() {
        let flakes = layer(0.5, 1.0, 1.0);
        let period = FLAKE_PERIOD as f64 * flakes.size_mm();
        for p in [Vec3::new(0.013, 0.021, 0.034), Vec3::new(-7.31, 2.2, 0.9), Vec3::new(120.4, -55.5, 9.1)] {
            let near = flakes.at(p, Vec3::Z).unwrap();
            // Whole periods away, the same cell decides: only rounding of the position differs.
            let far = flakes.at(p + Vec3::new(period, -period, 2.0 * period), Vec3::Z).unwrap();
            assert_eq!(near.is_some(), far.is_some());
            if let (Some(a), Some(b)) = (near, far) {
                assert!((a.normal - b.normal).len() < 1e-9 && (a.depth - b.depth).abs() < 1e-12);
            }
        }
    }

    fn painted(flake_normal: Vec3, depth: f64) -> OpaqueSurface {
        let flakes = FlakeLayer::new(1.0, 0.05, 1.0, 1.0, Vec3::new(1.0, 1.0, 1.0), 1.0, 0.3).unwrap();
        OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 0.0, 0.5, 1.5)
            .unwrap()
            .with_flakes(flakes)
            .on_flake(Flake { normal: flake_normal, depth })
            .unwrap()
    }

    #[test]
    fn a_surface_takes_the_flake_metal_by_depth_and_no_flake_changes_nothing() {
        let base = OpaqueSurface::new(Vec3::new(0.1, 0.2, 0.6), 0.0, 0.5, 1.5).unwrap();
        let flakes = FlakeLayer::new(0.5, 0.05, 0.5, 1.0, Vec3::new(0.9, 0.8, 0.7), 1.0, 0.2).unwrap();
        let paint = base.with_flakes(flakes);
        assert_eq!(base.with_flakes(layer(0.0, 0.5, 0.5)), base);
        assert_eq!(paint.flakes(), Some(flakes));
        assert_eq!(paint.base_normal(Vec3::Z), Vec3::Z);
        let (v, l) = (Vec3::new(0.5, 0.0, 0.8), Vec3::new(-0.3, 0.2, 0.6));
        // Until a hit puts the point on a flake, the paint shades as its base.
        assert_eq!(paint.brdf(Vec3::Z, v, l), base.brdf(Vec3::Z, v, l));
        assert_eq!(surface_pdf(paint, Vec3::Z, v, l), surface_pdf(base, Vec3::Z, v, l));
        let tilted = unit_direction(Vec3::new(0.2, 0.1, 1.0)).unwrap();
        let top = paint.on_flake(Flake { normal: tilted, depth: 0.0 }).unwrap();
        assert_eq!((top.base_color(), top.metallic(), top.roughness()), (flakes.color(), 1.0, 0.2));
        // The flake's normal is kept in single precision, like the GPU's.
        assert!((top.base_normal(Vec3::Z) - tilted).len() < 1e-7);
        assert_eq!(top.flakes(), None, "a point on a flake is resolved once");
        let buried = paint.on_flake(Flake { normal: tilted, depth: 1.0 }).unwrap();
        assert_eq!((buried.base_color(), buried.metallic(), buried.roughness()), (base.base_color(), 0.0, 0.5));
        let half = paint.on_flake(Flake { normal: tilted, depth: 0.5 }).unwrap();
        assert!((half.metallic() - 0.5).abs() < 1e-15 && (half.roughness() - 0.35).abs() < 1e-15);
        assert!(base.on_flake(Flake { normal: tilted, depth: 0.0 }).is_err(), "a surface without flakes has no flake to stand on");
        for bad in [Flake { normal: Vec3::ZERO, depth: 0.0 }, Flake { normal: tilted, depth: 1.5 }, Flake { normal: tilted, depth: f64::NAN }] {
            assert!(paint.on_flake(bad).is_err());
        }
    }

    #[test]
    fn a_tilted_flake_is_reciprocal_and_dark_from_behind() {
        let tilted = unit_direction(Vec3::new(0.6, 0.0, 0.8)).unwrap();
        for coat in [0.0, 1.0] {
            let s = painted(tilted, 0.0).with_clear_coat(ClearCoat::new(coat, 0.12, 1.5).unwrap());
            for (v, l) in [
                (Vec3::new(0.5, 0.0, 0.8), Vec3::new(-0.3, 0.2, 0.6)),
                (Vec3::new(0.9, 0.1, 0.2), Vec3::new(0.1, -0.7, 0.4)),
                (Vec3::new(-0.2, 0.3, 0.9), Vec3::new(0.7, 0.2, 0.3)),
            ] {
                let a = s.brdf(Vec3::Z, v, l).unwrap();
                let b = s.brdf(Vec3::Z, l, v).unwrap();
                assert!(a.is_finite() && a.x >= 0.0 && (a - b).len() <= 1e-12 * (1.0 + a.len()));
            }
            // A view above the surface but behind the flake sees no flake metal, only the coat.
            let behind = unit_direction(Vec3::new(-0.9, 0.0, 0.3)).unwrap();
            assert!(behind.dot(tilted) < 0.0);
            let seen = s.brdf(Vec3::Z, behind, Vec3::Z).unwrap();
            let coat_only = OpaqueSurface::new(Vec3::ZERO, 0.0, 0.5, 1.0).unwrap().with_clear_coat(ClearCoat::new(coat, 0.12, 1.5).unwrap());
            assert!((seen - coat_only.brdf(Vec3::Z, behind, Vec3::Z).unwrap()).len() < 1e-12);
        }
    }

    /// A white flake under a white light returns no more light than it receives, at any tilt.
    /// Each lobe is integrated with its own samples, as the path tracer draws them.
    #[test]
    fn a_flake_does_not_create_energy_and_its_density_matches_the_sampler() {
        const COUNT: u32 = 1 << 14;
        let mut maximum = 0.0_f64;
        for tilt in [0.0_f64, 0.3, MAX_FLAKE_TILT] {
            for turn in [0.0_f64, 2.0, 4.0] {
                let m = Vec3::new(tilt.sin() * turn.cos(), tilt.sin() * turn.sin(), tilt.cos());
                for weight in [0.0, 1.0] {
                    for cosine in [0.05_f64, 0.2, 0.6, 1.0] {
                        let s = painted(m, 0.0).with_clear_coat(ClearCoat::new(weight, 0.3, 1.5).unwrap());
                        let v = Vec3::new((1.0 - cosine * cosine).sqrt(), 0.0, cosine);
                        let (mut energy, mut above) = (0.0, 0.0);
                        for (component, probability) in lobe_probabilities(s).into_iter().enumerate() {
                            // A lobe that is never chosen draws nothing.
                            if probability == 0.0 {
                                continue;
                            }
                            for i in 0..COUNT {
                                let x = (f64::from(i) + 0.5) / f64::from(COUNT);
                                let y = f64::from(i.reverse_bits()) / 4_294_967_296.0;
                                // The base lobes turn with the flake; the coat keeps the surface normal.
                                let about = if component == 2 { Vec3::Z } else { m };
                                let roughness = 0.3_f64;
                                if about.dot(v) <= 0.0 {
                                    continue;
                                }
                                let l = if component == 0 {
                                    let tangent =
                                        unit_direction(if about.z.abs() < 0.9 { Vec3::Z.cross(about) } else { Vec3::X.cross(about) }).unwrap();
                                    let phi = 2.0 * PI * y;
                                    tangent * (x.sqrt() * phi.cos()) + about.cross(tangent) * (x.sqrt() * phi.sin()) + about * (1.0 - x).sqrt()
                                } else {
                                    let h = visible_normal(v, about, roughness * roughness, [x, y]).unwrap();
                                    h * (2.0 * v.dot(h)) - v
                                };
                                if l.z <= 0.0 {
                                    continue;
                                }
                                above += probability / f64::from(COUNT);
                                let pdf = surface_pdf(s, Vec3::Z, v, l).unwrap();
                                assert!(pdf > 0.0, "a drawn direction has no density: tilt {tilt}, component {component}");
                                energy += probability * s.brdf(Vec3::Z, v, l).unwrap().x * l.z / pdf / f64::from(COUNT);
                            }
                        }
                        maximum = maximum.max(energy);
                        assert!(energy.is_finite() && energy <= 1.003, "tilt={tilt} turn={turn} coat={weight} nv={cosine}: {energy}");
                        // The density integrates to the share of samples that land above the surface.
                        // The grid resolves these lobes from a cosine of 0.2 up, as the coat's own test.
                        if cosine < 0.2 {
                            continue;
                        }
                        let mut integrated = 0.0;
                        const GRID: u32 = 384;
                        for i in 0..GRID {
                            let mu = (f64::from(i) + 0.5) / f64::from(GRID);
                            let r = (1.0 - mu * mu).sqrt();
                            for j in 0..GRID {
                                let phi = 2.0 * PI * (f64::from(j) + 0.5) / f64::from(GRID);
                                integrated += surface_pdf(s, Vec3::Z, v, Vec3::new(r * phi.cos(), r * phi.sin(), mu)).unwrap();
                            }
                        }
                        integrated *= 2.0 * PI / f64::from(GRID * GRID);
                        assert!(
                            (above - integrated).abs() < 0.01,
                            "tilt={tilt} turn={turn} coat={weight} nv={cosine}: drew {above}, density {integrated}"
                        );
                    }
                }
            }
        }
        eprintln!("maximum flake furnace reflectance: {maximum}");
    }
}

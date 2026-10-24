//! Visible surface radiance only: no mesh-light sampling or neighboring illumination.
use crate::job::{EmissionSides, SurfaceEmission};
use solidcraft_geom::Vec3;

#[derive(Clone, Copy, Debug)]
pub struct Emission {
    radiance: Vec3,
    sides: EmissionSides,
}

impl Emission {
    pub fn new(input: &SurfaceEmission) -> crate::job::Result<Self> {
        input.validate()?;
        let [r, g, b] = input.color;
        Ok(Self { radiance: Vec3::new(r, g, b) * input.strength, sides: input.sides })
    }

    /// Front means strictly positive dot(outward geometric normal, outgoing view).
    /// Shading normals never decide emission sidedness. Tangency emits only in Both.
    pub fn radiance(self, front: bool) -> Vec3 {
        if front || self.sides == EmissionSides::Both { self.radiance } else { Vec3::ZERO }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> SurfaceEmission {
        SurfaceEmission { body_index: 0, body: "emitter".into(), color: [0.25, 0.5, 1.0], strength: 4.0, sides: EmissionSides::Front }
    }
    #[test]
    fn linear_radiance_and_explicit_sides() {
        let e = Emission::new(&input()).unwrap();
        assert_eq!(e.radiance(true), Vec3::new(1.0, 2.0, 4.0));
        assert_eq!(e.radiance(false), Vec3::ZERO);
        let both = Emission::new(&SurfaceEmission { sides: EmissionSides::Both, ..input() }).unwrap();
        assert_eq!(both.radiance(false), e.radiance(true));
    }
    #[test]
    fn zero_is_exact_and_hostile_values_refuse() {
        assert_eq!(Emission::new(&SurfaceEmission { strength: 0.0, ..input() }).unwrap().radiance(true), Vec3::ZERO);
        for strength in [f64::NAN, f64::INFINITY, -1.0, 64.0001] {
            assert!(Emission::new(&SurfaceEmission { strength, ..input() }).is_err());
        }
        for color in [[f64::NAN, 0.0, 0.0], [-0.1; 3], [1.1; 3]] {
            assert!(Emission::new(&SurfaceEmission { color, ..input() }).is_err());
        }
    }
}

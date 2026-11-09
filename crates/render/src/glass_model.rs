//! Original bounded rough-dielectric, Cauchy dispersion and lossless thin-film math.
//!
//! The representative RGB wavelengths below define a three-band approximation,
//! not a sampled spectrum or a colour-matching-function integration. Equations:
//! Heitz (2018), https://jcgt.org/published/0007/04/01/paper.pdf (GGX VNDF);
//! Walter et al. (2007), https://www.graphics.cornell.edu/~bjw/microfacetbsdf.pdf
//! (refraction half-vector and Jacobian); Belcour and Barla (2017),
//! https://hal.science/hal-01518344/document (Airy interference). No source code
//! from those publications is incorporated here.
//! Shading-normal measures follow Veach (1997), sections 5.3.3--5.3.4:
//! https://graphics.stanford.edu/papers/veach_thesis/thesis.pdf.
use crate::{surface::unit_direction, transmission};
use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::Vec3Geom;
use std::f64::consts::{PI, TAU};

/// Fixed R, G, B representative bands. Thin films can alias in this approximation.
pub const WAVELENGTHS_NM: [f64; 3] = [610.0, 550.0, 460.0];
pub const MIN_WAVELENGTH_NM: f64 = 380.0;
pub const MAX_WAVELENGTH_NM: f64 = 780.0;
/// Fraunhofer wavelengths used for nd and vd; SCHOTT TIE-29, Table 1.
pub const D_LINE_NM: f64 = 587.5618;
pub const F_LINE_NM: f64 = 486.1327;
pub const C_LINE_NM: f64 = 656.2725;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dispersion {
    pub abbe_number: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThinFilm {
    pub ior: f64,
    pub thickness_nm: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GlassModel {
    pub roughness: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispersion: Option<Dispersion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thin_film: Option<ThinFilm>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlassModelError {
    Roughness,
    Dispersion,
    Film,
    Ior,
    Wavelength,
    Angle,
    Direction,
    Sample,
    Numerical,
}
impl std::fmt::Display for GlassModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Roughness => "Glass roughness must be finite in [0, 1] with representable GGX width and peak",
            Self::Dispersion => "Glass dispersion requires a finite Abbe number in [5, 200] and IOR in [1, 3] over 380..780 nm",
            Self::Film => "Glass thin film requires finite IOR in [1, 3] and thickness in [0, 100000] nm",
            Self::Ior => "Glass interface IOR must be finite in [1, 3]",
            Self::Wavelength => "Glass wavelength must be finite in [380, 780] nm",
            Self::Angle => "Glass incident cosine must be finite in [0, 1]",
            Self::Direction => "Glass directions must be finite and nonzero; the normal must face the incident medium",
            Self::Sample => "Glass sample coordinates must be finite in [0, 1)",
            Self::Numerical => "Glass interface cannot be represented with finite nonnegative transport and density",
        })
    }
}
impl std::error::Error for GlassModelError {}
pub type Result<T> = std::result::Result<T, GlassModelError>;

/// One sampled radiance-transport continuation. `weight` is f*|cos(out)|/pdf,
/// including Fresnel branch selection and the transmitted radiance IOR ratio.
/// For delta events `pdf` is a discrete branch probability, not sr^-1. For the
/// index-matched coated transmission singularity it is conditional on the sampled
/// visible facet; delta paths must not use it as a continuous MIS density.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InterfaceSample {
    pub direction: Vec3,
    pub weight: f64,
    pub pdf: f64,
    pub crosses: bool,
    pub delta: bool,
}

/// Continuous interface value and matching full mixture density per solid angle.
/// `evaluate` returns the BSDF; `evaluate_projected` includes outgoing projected
/// cosine in `value`. Delta lobes return zero in this measure.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InterfaceEvaluation {
    pub value: f64,
    pub pdf: f64,
}

fn valid_ior(ior: f64) -> bool {
    ior.is_finite() && (1.0..=3.0).contains(&ior)
}
fn wavelength(value: f64) -> Result<()> {
    if value.is_finite() && (MIN_WAVELENGTH_NM..=MAX_WAVELENGTH_NM).contains(&value) { Ok(()) } else { Err(GlassModelError::Wavelength) }
}
fn direction(value: Vec3) -> Result<Vec3> {
    unit_direction(value).map_err(|_| GlassModelError::Direction)
}

// Compute positive products as a mantissa and a separate binary exponent. This
// preserves finite ratios when numerator and denominator products would both
// underflow, and refuses a genuinely unrepresentable positive result.
fn positive_ratio(numerator: &[f64], denominator: &[f64]) -> Result<f64> {
    let mut mantissa = 1.0;
    let mut exponent = 0_i32;
    for (values, divide) in [(numerator, false), (denominator, true)] {
        for &value in values {
            if !value.is_finite() || value <= 0.0 {
                return Err(GlassModelError::Numerical);
            }
            let (scaled, adjustment) = if value < f64::MIN_POSITIVE { (value * 4503599627370496.0, -52) } else { (value, 0) };
            let bits = scaled.to_bits();
            let power = ((bits >> 52) & 0x7ff) as i32 - 1023 + adjustment;
            let fraction = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
            if divide {
                mantissa /= fraction;
                exponent -= power;
            } else {
                mantissa *= fraction;
                exponent += power;
            }
            while mantissa >= 2.0 {
                mantissa *= 0.5;
                exponent += 1;
            }
            while mantissa < 1.0 {
                mantissa *= 2.0;
                exponent -= 1;
            }
        }
    }
    let value = if exponent < -1022 {
        (mantissa * 2.0_f64.powi(exponent + 1074)) * f64::from_bits(1)
    } else if exponent <= 1023 {
        mantissa * 2.0_f64.powi(exponent)
    } else {
        return Err(GlassModelError::Numerical);
    };
    if value.is_finite() && value > 0.0 { Ok(value) } else { Err(GlassModelError::Numerical) }
}

/// Adjoint shading-normal correction for a photon continuation. `incident`
/// travels toward the interface, `outgoing` travels away; normals describe a
/// fixed facet model. Remove the radiance transmission (ni/nt)^2 first. This is
/// |wo.ns| |wi.ng| / (|wo.ng| |wi.ns|), with wo=-incident and wi=outgoing.
/// Exact tangent projections have zero measure and return zero; invalid vectors
/// and a nonrepresentable positive ratio are errors, never epsilon-clamped.
pub fn importance_normal_correction(incident: Vec3, outgoing: Vec3, shading_normal: Vec3, geometric_normal: Vec3) -> Result<f64> {
    let wo = -direction(incident)?;
    let wi = direction(outgoing)?;
    let ns = direction(shading_normal)?;
    let ng = direction(geometric_normal)?;
    let a = wo.dot(ns).abs();
    let b = wi.dot(ng).abs();
    let c = wo.dot(ng).abs();
    let d = wi.dot(ns).abs();
    if a == 0.0 || b == 0.0 || c == 0.0 || d == 0.0 {
        return Ok(0.0);
    }
    positive_ratio(&[a, b], &[c, d])
}

/// Convert photon flux per geometric area to a shading-normal BSDF measure.
/// `incoming_light` points from the receiver toward the photon's previous point.
/// Multiply the surface density estimate by f * this ratio, without an extra
/// cosine: the deposited flux already includes geometric projected incidence.
pub fn surface_normal_measure(incoming_light: Vec3, shading_normal: Vec3, geometric_normal: Vec3) -> Result<f64> {
    let incoming = direction(incoming_light)?;
    let ns = direction(shading_normal)?;
    let ng = direction(geometric_normal)?;
    let numerator = incoming.dot(ns).abs();
    let denominator = incoming.dot(ng).abs();
    if numerator == 0.0 || denominator == 0.0 {
        return Ok(0.0);
    }
    positive_ratio(&[numerator], &[denominator])
}

/// Uniform sphere kernel for a photon map gathered at an analog camera
/// scattering collision: 3/(4*pi*r^3*sigma_s), in inverse mm^2. Light collision
/// density is sigma_s times fluence; camera free-flight sampling already
/// includes sigma_s, so it must be divided out here. Multiply by the phase
/// function and photon flux. A ray-marched volume estimator has another measure.
pub fn volume_photon_kernel(radius_mm: f64, sigma_s_per_mm: f64) -> Result<f64> {
    if !radius_mm.is_finite() || radius_mm <= 0.0 || !sigma_s_per_mm.is_finite() || sigma_s_per_mm <= 0.0 {
        return Err(GlassModelError::Numerical);
    }
    positive_ratio(&[3.0], &[4.0 * PI, radius_mm, radius_mm, radius_mm, sigma_s_per_mm])
}

impl GlassModel {
    fn controls(self) -> Result<()> {
        let alpha = self.roughness * self.roughness;
        let peak_root = 1.0 / (alpha * PI.sqrt());
        if !self.roughness.is_finite()
            || !(0.0..=1.0).contains(&self.roughness)
            || (self.roughness > 0.0 && (alpha == 0.0 || alpha * alpha == 0.0 || !(peak_root * peak_root).is_finite()))
        {
            return Err(GlassModelError::Roughness);
        }
        if self.dispersion.is_some_and(|d| !d.abbe_number.is_finite() || !(5.0..=200.0).contains(&d.abbe_number)) {
            return Err(GlassModelError::Dispersion);
        }
        if self.thin_film.is_some_and(|f| !valid_ior(f.ior) || !f.thickness_nm.is_finite() || !(0.0..=100000.0).contains(&f.thickness_nm)) {
            return Err(GlassModelError::Film);
        }
        Ok(())
    }

    pub fn validate(self, base_ior: f64) -> Result<()> {
        self.controls()?;
        if !valid_ior(base_ior) {
            return Err(GlassModelError::Ior);
        }
        // Two-term normal Cauchy dispersion is monotone, so endpoints prove the
        // complete bounded wavelength domain without silently clamping an IOR.
        for nm in [MIN_WAVELENGTH_NM, MAX_WAVELENGTH_NM] {
            if !valid_ior(self.cauchy(base_ior, nm)) {
                return Err(GlassModelError::Dispersion);
            }
        }
        Ok(())
    }

    pub fn spectral(self) -> bool {
        self.dispersion.is_some() || self.thin_film.is_some_and(|film| film.thickness_nm > 0.0)
    }

    pub fn is_identity(self) -> bool {
        self.roughness == 0.0 && self.dispersion.is_none() && self.thin_film.is_none_or(|film| film.thickness_nm == 0.0)
    }

    fn cauchy(self, base_ior: f64, nm: f64) -> f64 {
        let Some(dispersion) = self.dispersion else { return base_ior };
        let inverse_square = |nm: f64| 1.0 / (nm * nm);
        let b = ((base_ior - 1.0) / dispersion.abbe_number) / (inverse_square(F_LINE_NM) - inverse_square(C_LINE_NM));
        base_ior + b * (inverse_square(nm) - inverse_square(D_LINE_NM))
    }

    /// Two-term Cauchy approximation anchored at nd, with vd=(nd-1)/(nF-nC).
    /// Abbe number alone is not a measured Sellmeier model for a named glass.
    pub fn ior(self, base_ior: f64, wavelength_nm: f64) -> Result<f64> {
        wavelength(wavelength_nm)?;
        self.validate(base_ior)?;
        Ok(self.cauchy(base_ior, wavelength_nm))
    }

    /// Lossless single-layer coherent Airy response. ni/nt are the actual IORs
    /// at the supplied wavelength; dispersion is resolved by the caller. The
    /// substrate's Snell cosine is preserved even when the film is evanescent.
    pub fn fresnel(self, cosine: f64, ni: f64, nt: f64, wavelength_nm: f64) -> Result<transmission::Fresnel> {
        self.controls()?;
        wavelength(wavelength_nm)?;
        if !cosine.is_finite() || !(0.0..=1.0).contains(&cosine) {
            return Err(GlassModelError::Angle);
        }
        if !valid_ior(ni) || !valid_ior(nt) {
            return Err(GlassModelError::Ior);
        }
        let bare = transmission::fresnel(cosine, ni, nt).map_err(|_| GlassModelError::Numerical)?;
        let Some(film) = self.thin_film.filter(|film| film.thickness_nm > 0.0) else { return Ok(bare) };
        if film.ior == ni || film.ior == nt || bare.transmitted_cosine.is_none() {
            return Ok(bare);
        }
        if cosine == 0.0 {
            return Ok(transmission::Fresnel { s: 1.0, p: 1.0, transmitted_cosine: bare.transmitted_cosine });
        }
        let transverse_square = ni * ni * (1.0 - cosine * cosine).max(0.0);
        let film_longitudinal_square = film.ior * film.ior - transverse_square;
        let qf = if film_longitudinal_square >= 0.0 {
            Complex::real(film_longitudinal_square.sqrt())
        } else {
            Complex { re: 0.0, im: (-film_longitudinal_square).sqrt() }
        };
        let qi = ni * cosine;
        let qt = nt * bare.transmitted_cosine.ok_or(GlassModelError::Numerical)?;
        let k = TAU * film.thickness_nm / wavelength_nm;
        let polarized = |p: bool| -> Result<f64> {
            let (yi, yt, factor) = if p { (qi / (ni * ni), qt / (nt * nt), film.ior * film.ior) } else { (qi, qt, 1.0) };
            let phase = qf.scale(k);
            let amplitude = if phase.norm_square() < 1e-6 {
                // Transfer matrix written with sin(delta)/qf. Its power series
                // remains finite at the exact film critical angle qf=0, where
                // the direct Airy quotient otherwise becomes 0/0.
                let z2 = phase.mul(phase);
                let z4 = z2.mul(z2);
                let z6 = z4.mul(z2);
                let a = Complex::real(1.0).sub(z2.scale(0.5)).add(z4.scale(1.0 / 24.0)).sub(z6.scale(1.0 / 720.0));
                let sinc = Complex::real(1.0).sub(z2.scale(1.0 / 6.0)).add(z4.scale(1.0 / 120.0)).sub(z6.scale(1.0 / 5040.0));
                let b = sinc.minus_i().scale(k * factor);
                let c = sinc.minus_i().scale(k * film_longitudinal_square / factor);
                let common = b.scale(yi * yt);
                a.scale(yi - yt).add(common).sub(c).div(a.scale(yi + yt).add(common).add(c))?
            } else {
                let yf = qf.scale(1.0 / factor);
                let r01 = Complex::real(yi).sub(yf).div(Complex::real(yi).add(yf))?;
                let r12 = yf.sub(Complex::real(yt)).div(yf.add(Complex::real(yt)))?;
                let decay = (-2.0 * phase.im).exp();
                let (sine, cosine) = (2.0 * phase.re).sin_cos();
                let propagation = Complex { re: decay * cosine, im: decay * sine };
                let second = r12.mul(propagation);
                r01.add(second).div(Complex::real(1.0).add(r01.mul(second)))?
            };
            let power = amplitude.norm_square();
            if !power.is_finite() || !(0.0..=1.0 + 1e-10).contains(&power) {
                return Err(GlassModelError::Numerical);
            }
            // Only the roundoff of a lossless power sum can exceed one here.
            Ok(power.min(1.0))
        };
        Ok(transmission::Fresnel { s: polarized(false)?, p: polarized(true)?, transmitted_cosine: bare.transmitted_cosine })
    }

    /// Isotropic GGX visible-normal sampling. Incident points toward the surface;
    /// macro_normal points into its incident medium. uniforms are facet-u,
    /// facet-v, Fresnel branch. A returned None is an actual zero contribution;
    /// retrying until a sample passes would change the estimator.
    pub fn sample(
        self,
        incident: Vec3,
        macro_normal: Vec3,
        ni: f64,
        nt: f64,
        wavelength_nm: f64,
        uniforms: [f64; 3],
    ) -> Result<Option<InterfaceSample>> {
        self.controls()?;
        wavelength(wavelength_nm)?;
        if !valid_ior(ni) || !valid_ior(nt) {
            return Err(GlassModelError::Ior);
        }
        if uniforms.into_iter().any(|u| !u.is_finite() || !(0.0..1.0).contains(&u)) {
            return Err(GlassModelError::Sample);
        }
        let d = direction(incident)?;
        let n = direction(macro_normal)?;
        let v = -d;
        let cv = v.dot(n);
        if cv < 0.0 {
            return Err(GlassModelError::Direction);
        }
        let smooth = self.roughness == 0.0 || (ni == nt && self.thin_film.is_none_or(|f| f.thickness_nm == 0.0 || f.ior == ni));
        let m = if smooth {
            n
        } else {
            if cv == 0.0 {
                return Ok(None);
            }
            visible_normal(v, n, self.roughness * self.roughness, [uniforms[0], uniforms[1]])?
        };
        let vm = v.dot(m).clamp(0.0, 1.0);
        let fresnel = self.fresnel(vm, ni, nt, wavelength_nm)?;
        let reflected = uniforms[2] < fresnel.reflectance() || fresnel.transmitted_cosine.is_none();
        let (out, probability, crosses) = if reflected {
            (direction(d + m * (2.0 * vm))?, fresnel.reflectance(), false)
        } else {
            let out = transmission::refract(d, m, ni, nt).map_err(|_| GlassModelError::Numerical)?.ok_or(GlassModelError::Numerical)?;
            (direction(out)?, 1.0 - fresnel.reflectance(), true)
        };
        let cl = out.dot(n);
        if !smooth && ((!crosses && cl <= 0.0) || (crosses && cl >= 0.0)) {
            return Ok(None);
        }
        let radiance = if crosses { (ni / nt).powi(2) } else { 1.0 };
        let delta = smooth || (crosses && ni == nt);
        let weight = if smooth { radiance } else { smith_ratio(cv, cl.abs(), self.roughness * self.roughness) * radiance };
        let pdf = if delta { probability } else { self.evaluate(d, out, n, ni, nt, wavelength_nm)?.pdf };
        if !weight.is_finite() || weight < 0.0 || !pdf.is_finite() || pdf <= 0.0 {
            return Err(GlassModelError::Numerical);
        }
        Ok(Some(InterfaceSample { direction: out, weight, pdf, crosses, delta }))
    }

    /// Continuous radiance BSDF and complete reflection/transmission PDF for the
    /// same VNDF/Fresnel mixture as sample. Rough transmission uses Walter's
    /// half-vector Jacobian and the radiance (ni/nt)^2 transport conversion.
    pub fn evaluate(self, incident: Vec3, outgoing: Vec3, macro_normal: Vec3, ni: f64, nt: f64, wavelength_nm: f64) -> Result<InterfaceEvaluation> {
        self.evaluate_measure(incident, outgoing, macro_normal, ni, nt, wavelength_nm, false)
    }

    /// Continuous radiance BSDF times the outgoing projected cosine, with the
    /// same complete mixture PDF as `evaluate`. This is the direct-light
    /// integrand and avoids dividing by a tiny outgoing cosine only to multiply
    /// it back. Discrete lobes remain zero in this solid-angle measure.
    pub fn evaluate_projected(
        self,
        incident: Vec3,
        outgoing: Vec3,
        macro_normal: Vec3,
        ni: f64,
        nt: f64,
        wavelength_nm: f64,
    ) -> Result<InterfaceEvaluation> {
        self.evaluate_measure(incident, outgoing, macro_normal, ni, nt, wavelength_nm, true)
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_measure(
        self,
        incident: Vec3,
        outgoing: Vec3,
        macro_normal: Vec3,
        ni: f64,
        nt: f64,
        wavelength_nm: f64,
        projected: bool,
    ) -> Result<InterfaceEvaluation> {
        self.controls()?;
        // This also validates the actual wavelength/IORs on all zero-lobe paths.
        self.fresnel(1.0, ni, nt, wavelength_nm)?;
        let v = -direction(incident)?;
        let l = direction(outgoing)?;
        let n = direction(macro_normal)?;
        let cv = v.dot(n);
        let cl = l.dot(n);
        if cv < 0.0 {
            return Err(GlassModelError::Direction);
        }
        if self.roughness == 0.0 || cv == 0.0 || cl == 0.0 || (cl < 0.0 && ni == nt) {
            return Ok(InterfaceEvaluation::default());
        }
        let sum = if cl > 0.0 { v + l } else { v * ni + l * nt };
        let mut m = direction(sum)?;
        if m.dot(n) < 0.0 {
            m = -m;
        }
        let vm = v.dot(m);
        let lm = l.dot(m);
        if vm <= 0.0 || (cl > 0.0 && lm <= 0.0) || (cl < 0.0 && lm >= 0.0) {
            return Ok(InterfaceEvaluation::default());
        }
        let alpha = self.roughness * self.roughness;
        let d = ggx_distribution(m, n, alpha);
        let g1 = smith_g1(cv, alpha);
        let g2 = g1 * smith_ratio(cv, cl.abs(), alpha);
        let visible_pdf = d * (2.0 * vm / (cv + smith_root(cv, alpha)));
        let f = self.fresnel(vm.min(1.0), ni, nt, wavelength_nm)?.reflectance();
        let outgoing_measure = if projected { 1.0 } else { cl.abs() };
        let result = if cl > 0.0 {
            InterfaceEvaluation { value: d * (f * g2 / (4.0 * cv * outgoing_measure)), pdf: visible_pdf * (f / (4.0 * vm)) }
        } else {
            let denominator = ni * vm + nt * lm;
            if denominator == 0.0 {
                return Ok(InterfaceEvaluation::default());
            }
            let jacobian = nt * nt * lm.abs() / (denominator * denominator);
            InterfaceEvaluation {
                value: d * ((1.0 - f) * g2 * (vm * lm).abs() * ni * ni / (cv * outgoing_measure * denominator * denominator)),
                pdf: (1.0 - f) * visible_pdf * jacobian,
            }
        };
        if !result.value.is_finite() || !result.pdf.is_finite() || result.value < 0.0 || result.pdf < 0.0 {
            return Err(GlassModelError::Numerical);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: f64, expected: f64, tolerance: f64) {
        assert!((actual - expected).abs() <= tolerance, "{actual} differs from {expected} by more than {tolerance}");
    }
    fn film(ior: f64, thickness_nm: f64) -> GlassModel {
        GlassModel { thin_film: Some(ThinFilm { ior, thickness_nm }), ..Default::default() }
    }
    fn radical_inverse(mut bits: u32) -> f64 {
        bits = bits.reverse_bits();
        f64::from(bits) / 4294967296.0
    }

    #[test]
    fn hard_directional_normal_ggx_response_matches_independent_peak_and_projected_measure() {
        for roughness in [0.05_f64, 0.2, 0.7, 1.0] {
            let model = GlassModel { roughness, ..Default::default() };
            // At normal reflection h=n, G=1, D=1/(pi*r^4), F=((1-1.5)/(1+1.5))^2.
            let expected = 0.04 / (4.0 * PI * roughness.powi(4));
            let value = model.evaluate_projected(-Vec3::Z, Vec3::Z, Vec3::Z, 1.0, 1.5, 550.0).unwrap();
            close(value.value / expected, 1.0, 2e-15);
            close(value.pdf / expected, 1.0, 2e-15);
            for uniforms in [[0.01, 0.3, 0.0], [0.1, 0.4, 0.9], [0.4, 0.7, 0.5]] {
                if let Some(sample) = model.sample(-Vec3::Z, Vec3::Z, 1.0, 1.5, 550.0, uniforms).unwrap() {
                    let projected = model.evaluate_projected(-Vec3::Z, sample.direction, Vec3::Z, 1.0, 1.5, 550.0).unwrap();
                    let unprojected = model.evaluate(-Vec3::Z, sample.direction, Vec3::Z, 1.0, 1.5, 550.0).unwrap();
                    close(projected.value / projected.pdf, sample.weight, 3e-13);
                    close(projected.value / (unprojected.value * sample.direction.z.abs()), 1.0, 3e-13);
                    assert_eq!(projected.pdf, unprojected.pdf);
                }
            }
        }
        // This peak fits in f64 even though squaring the GGX denominator does not.
        let tiny = GlassModel { roughness: 1e-50, ..Default::default() };
        let value = tiny.evaluate_projected(-Vec3::Z, Vec3::Z, Vec3::Z, 1.0, 1.5, 550.0).unwrap();
        close(value.value / (0.01 / PI * 1e200), 1.0, 2e-15);
        assert_eq!(GlassModel { roughness: 1e-80, ..Default::default() }.validate(1.5), Err(GlassModelError::Roughness));
    }

    #[test]
    fn adjoint_normal_and_flux_area_measures_match_fixed_facet_geometry() {
        let ng = Vec3::Z;
        let ns = Vec3::new(0.6, 0.0, 0.8);
        let wo = Vec3::new(0.0, 0.6, 0.8);
        let wi = Vec3::new(-0.8, 0.0, 0.6);
        // Independent scalar dot products: wo.ns=.64, wi.ng=.6,
        // wo.ng=.8, wi.ns=0. Exact cancellation must survive near grazing too.
        assert_eq!(importance_normal_correction(-wo, wi, ns, ng).unwrap(), 0.0);
        let wi = Vec3::new(0.8, 0.0, 0.6);
        let correction = importance_normal_correction(-wo, wi, ns, ng).unwrap();
        close(correction, (0.64 * 0.6) / (0.8 * 0.96), 2e-16);
        let reverse = importance_normal_correction(-wi, wo, ns, ng).unwrap();
        close(correction * reverse, 1.0, 3e-16);
        close(surface_normal_measure(wi, ns, ng).unwrap(), 0.96 / 0.6, 3e-16);
        // A planar photon count already measures wi.ng; the ratio converts it
        // into wi.ns once, rather than adding a second cosine to deposited flux.
        let geometric_irradiance = 7.0 * 0.6;
        close(geometric_irradiance * surface_normal_measure(wi, ns, ng).unwrap(), 7.0 * 0.96, 2e-15);
        for normal in [Vec3::Z, -Vec3::Z] {
            close(importance_normal_correction(-wo, wi, normal, ng).unwrap(), 1.0, 0.0);
        }
        let grazing = Vec3::new(1.0, 0.0, 1e-300);
        close(importance_normal_correction(-grazing, grazing, Vec3::X, Vec3::Z).unwrap(), 1.0, 0.0);
        close(importance_normal_correction(-grazing, grazing, Vec3::Z, Vec3::Z).unwrap(), 1.0, 0.0);
        let opposite_grazing = Vec3::new(1e-300, 0.0, 1.0);
        assert_eq!(importance_normal_correction(-grazing, opposite_grazing, Vec3::X, Vec3::Z), Err(GlassModelError::Numerical));
        close(importance_normal_correction(-wo * 1e300, wi * 1e300, ns * 1e300, ng * 1e300).unwrap(), correction, 3e-16);
        assert_eq!(surface_normal_measure(Vec3::X, Vec3::Z, Vec3::Z).unwrap(), 0.0);
        assert_eq!(surface_normal_measure(Vec3::ZERO, ns, ng), Err(GlassModelError::Direction));
        assert_eq!(importance_normal_correction(Vec3::new(f64::NAN, 0.0, 0.0), wi, ns, ng), Err(GlassModelError::Direction));
    }

    #[test]
    fn analog_volume_photon_measure_has_correct_density_units_and_sigma_cancellation() {
        let radius = 2.5_f64;
        let sigma = 0.2;
        let ball_volume = 4.0 * PI * radius.powi(3) / 3.0;
        let kernel = volume_photon_kernel(radius, sigma).unwrap();
        close(kernel * ball_volume * sigma, 1.0, 2e-16);
        // A homogeneous field with fluence Phi deposits sigma*Phi collisions per
        // volume; an isotropic analog camera collision must recover Phi/(4*pi).
        let fluence = 11.0;
        let isotropic_phase = 1.0 / (4.0 * PI);
        close(kernel * (sigma * fluence * ball_volume) * isotropic_phase, fluence * isotropic_phase, 3e-16);
        close(volume_photon_kernel(radius * 2.0, sigma).unwrap() * 8.0, kernel, 0.0);
        close(volume_photon_kernel(radius, sigma * 2.0).unwrap() * 2.0, kernel, 0.0);
        close(volume_photon_kernel(1e100, 1e-300).unwrap(), 3.0 / (4.0 * PI), 2e-16);
        // Unit conversion mm -> metre: radius /1000 and sigma *1000; an
        // inverse-area estimator consequently gains 10^6, without changing flux.
        close(volume_photon_kernel(radius / 1000.0, sigma * 1000.0).unwrap() / kernel, 1e6, 3e-10);
        for (r, s) in [(0.0, sigma), (-1.0, sigma), (f64::INFINITY, sigma), (radius, 0.0), (radius, f64::NAN), (1e-200, sigma)] {
            assert_eq!(volume_photon_kernel(r, s), Err(GlassModelError::Numerical));
        }
    }

    #[test]
    fn photon_hg_receiver_angle_uses_physical_forward_propagation() {
        let medium = crate::transmission::subsurface::Subsurface { scattering_per_mm: 0.2, anisotropy: 0.6 };
        let incoming_light = Vec3::Z; // The photon's physical travel is -Z.
        let camera_incident = Vec3::Z; // The scattered travel toward the eye is -Z.
        let forward_cosine = camera_incident.dot(incoming_light);
        let backward_cosine = (-camera_incident).dot(incoming_light);
        let forward = medium.phase(forward_cosine).unwrap();
        let backward = medium.phase(backward_cosine).unwrap();
        let g = medium.anisotropy;
        close(forward, (1.0 + g) / (4.0 * PI * (1.0 - g).powi(2)), 2e-15);
        close(backward, (1.0 - g) / (4.0 * PI * (1.0 + g).powi(2)), 2e-17);
        close(forward / backward, ((1.0 + g) / (1.0 - g)).powi(3), 2e-13);
        assert!(forward > backward);
    }

    #[test]
    fn one_record_uniform_receiver_reservoir_is_unbiased_for_every_bounded_path_length() {
        // Count all replacement decisions exactly, without Monte Carlo or a
        // production reservoir helper: stage k has k equally likely outcomes,
        // one replaces the record and k-1 retain it. At k<=16, k! fits in u64.
        let mut histories = vec![1_u64];
        let mut total = 1_u64;
        for k in 1..=16_usize {
            if k > 1 {
                for count in &mut histories {
                    *count *= (k - 1) as u64;
                }
                histories.push(total);
                total *= k as u64;
            }
            assert_eq!(histories.iter().copied().sum::<u64>(), total);
            let mut expected_numerator = 0_u128;
            let mut reference_sum = 0_u128;
            for (index, &count) in histories.iter().enumerate() {
                assert_eq!(count * k as u64, total);
                let flux = (index + 2).pow(2) as u128;
                let receiver_response = (3 * index + 1) as u128;
                reference_sum += flux * receiver_response;
                expected_numerator += u128::from(count) * k as u128 * flux * receiver_response;
            }
            assert_eq!(expected_numerator, u128::from(total) * reference_sum);
            // Dividing by launched paths rather than retained records also
            // preserves zero-deposit paths: append one empty launch explicitly.
            close(expected_numerator as f64 / total as f64 / 2.0, reference_sum as f64 / 2.0, 0.0);
        }
    }

    #[test]
    fn bounded_models_round_trip_and_refuse_hostile_values() {
        let model = GlassModel {
            roughness: 0.3,
            dispersion: Some(Dispersion { abbe_number: 60.0 }),
            thin_film: Some(ThinFilm { ior: 1.38, thickness_nm: 285.0 }),
        };
        model.validate(1.5).unwrap();
        assert_eq!(serde_json::from_value::<GlassModel>(serde_json::to_value(model).unwrap()).unwrap(), model);
        assert_eq!(serde_json::from_str::<GlassModel>("{}").unwrap(), GlassModel::default());
        assert!(serde_json::from_str::<GlassModel>(r#"{"roughnes":0.5}"#).is_err());
        assert!(serde_json::from_str::<GlassModel>(r#"{"dispersion":{"abbe_number":40,"unknown":0}}"#).is_err());
        for roughness in [-1.0, f64::NAN, f64::INFINITY, 1.1, f64::MIN_POSITIVE] {
            assert_eq!(GlassModel { roughness, ..Default::default() }.validate(1.5), Err(GlassModelError::Roughness));
        }
        for abbe_number in [0.0, 4.99, 200.1, f64::NAN, f64::INFINITY] {
            assert_eq!(
                GlassModel { dispersion: Some(Dispersion { abbe_number }), ..Default::default() }.validate(1.5),
                Err(GlassModelError::Dispersion)
            );
        }
        for bad in [
            ThinFilm { ior: 0.99, thickness_nm: 10.0 },
            ThinFilm { ior: 1.4, thickness_nm: -1.0 },
            ThinFilm { ior: 1.4, thickness_nm: f64::NAN },
            ThinFilm { ior: 1.4, thickness_nm: 100001.0 },
        ] {
            assert_eq!(GlassModel { thin_film: Some(bad), ..Default::default() }.validate(1.5), Err(GlassModelError::Film));
        }
        assert!(!film(1.4, 0.0).spectral());
        assert!(film(1.4, 1.0).spectral());
        assert!(film(1.4, 0.0).is_identity());
        assert!(GlassModel { dispersion: Some(Dispersion { abbe_number: 100.0 }), ..Default::default() }.spectral());
    }

    #[test]
    fn cauchy_fit_matches_independent_abbe_definition_and_visible_ordering() {
        for (nd, vd) in [(1.33, 55.0), (1.5, 60.0), (1.8, 25.0), (2.0, 12.0)] {
            let model = GlassModel { dispersion: Some(Dispersion { abbe_number: vd }), ..Default::default() };
            close(model.ior(nd, D_LINE_NM).unwrap(), nd, 1e-15);
            let f = model.ior(nd, F_LINE_NM).unwrap();
            let c = model.ior(nd, C_LINE_NM).unwrap();
            close((nd - 1.0) / (f - c), vd, 2e-11);
            let [r, g, b] = WAVELENGTHS_NM.map(|nm| model.ior(nd, nm).unwrap());
            assert!(b > g && g > r);
            for nm in [MIN_WAVELENGTH_NM, MAX_WAVELENGTH_NM] {
                close(model.ior(1.0, nm).unwrap(), 1.0, 0.0);
            }
        }
        let strong = GlassModel { dispersion: Some(Dispersion { abbe_number: 5.0 }), ..Default::default() };
        assert_eq!(strong.validate(3.0), Err(GlassModelError::Dispersion));
        assert_eq!(strong.ior(1.5, 1000.0), Err(GlassModelError::Wavelength));
    }

    #[test]
    fn no_film_and_zero_thickness_preserve_bare_fresnel_exactly() {
        for (ni, nt) in [(1.0, 1.5), (1.5, 1.0), (1.5, 1.5)] {
            for cosine in [0.0, 0.001, 0.3, 0.8, 1.0] {
                let bare = transmission::fresnel(cosine, ni, nt).unwrap();
                for model in [GlassModel::default(), film(1.38, 0.0), film(ni, 500.0), film(nt, 500.0)] {
                    assert_eq!(model.fresnel(cosine, ni, nt, 550.0).unwrap(), bare);
                }
            }
        }
        let brewster_cosine = (1.5_f64).atan().cos();
        let brewster = GlassModel::default().fresnel(brewster_cosine, 1.0, 1.5, 550.0).unwrap();
        assert!(brewster.p < 1e-30 && brewster.s > 0.1);
        assert_eq!(film(1.38, 500.0).fresnel(0.3, 1.5, 1.0, 550.0).unwrap().transmitted_cosine, None);
    }

    #[test]
    fn airy_quarter_wave_antireflection_and_periodicity_match_analytic_limits() {
        let nf = 1.5_f64.sqrt();
        let quarter = film(nf, 550.0 / (4.0 * nf));
        let response = quarter.fresnel(1.0, 1.0, 1.5, 550.0).unwrap();
        assert!(response.s < 1e-28 && response.p < 1e-28);
        for cosine in [0.2, 0.6, 1.0] {
            let cosine_film = (1.0 - (1.0 - cosine * cosine) / 1.38_f64.powi(2)).sqrt();
            let period = 550.0 / (2.0 * 1.38 * cosine_film);
            let a = film(1.38, 285.0).fresnel(cosine, 1.0, 1.5, 550.0).unwrap();
            let b = film(1.38, 285.0 + period).fresnel(cosine, 1.0, 1.5, 550.0).unwrap();
            close(a.s, b.s, 2e-14);
            close(a.p, b.p, 2e-14);
            assert_eq!(a.transmitted_cosine, transmission::fresnel(cosine, 1.0, 1.5).unwrap().transmitted_cosine);
        }
    }

    #[test]
    fn film_evanescence_tunnels_and_critical_angle_is_continuous() {
        let thin = film(1.0, 1e-5).fresnel(0.5, 1.5, 1.5, 550.0).unwrap();
        let medium = film(1.0, 100.0).fresnel(0.5, 1.5, 1.5, 550.0).unwrap();
        let thick = film(1.0, 100000.0).fresnel(0.5, 1.5, 1.5, 550.0).unwrap();
        assert!(thin.reflectance() < 1e-12);
        assert!(medium.reflectance() > 0.1 && medium.reflectance() < 1.0);
        close(thick.s, 1.0, 1e-14);
        close(thick.p, 1.0, 1e-14);
        let critical_cosine = 5.0_f64.sqrt() / 3.0;
        let at = film(1.0, 200.0).fresnel(critical_cosine, 1.5, 1.5, 550.0).unwrap();
        for offset in [-1e-9, 1e-9] {
            let near = film(1.0, 200.0).fresnel(critical_cosine + offset, 1.5, 1.5, 550.0).unwrap();
            close(at.s, near.s, 1e-7);
            close(at.p, near.p, 1e-7);
        }
        assert!(at.s.is_finite() && at.p.is_finite());
    }

    #[test]
    fn film_power_is_bounded_and_reciprocal_at_matching_optical_angles() {
        for thickness in [1e-8, 50.0, 285.0, 10000.0, 100000.0] {
            for nm in WAVELENGTHS_NM {
                for cosine in [0.01, 0.2, 0.7, 1.0] {
                    let model = film(1.1, thickness);
                    let forward = model.fresnel(cosine, 1.0, 1.5, nm).unwrap();
                    assert!((0.0..=1.0).contains(&forward.s) && (0.0..=1.0).contains(&forward.p));
                    let reverse = model.fresnel(forward.transmitted_cosine.unwrap(), 1.5, 1.0, nm).unwrap();
                    close(forward.s, reverse.s, 1e-10);
                    close(forward.p, reverse.p, 1e-10);
                }
            }
        }
    }

    #[test]
    fn rough_sampling_weights_equal_continuous_bsdf_over_full_pdf() {
        for roughness in [0.12, 0.5, 1.0] {
            for (ni, nt) in [(1.0, 1.5), (1.5, 1.0)] {
                for cosine in [0.2, 0.7, 1.0] {
                    let incident = Vec3::new((1.0_f64 - cosine * cosine).sqrt(), 0.0, -cosine);
                    let model = GlassModel { roughness, thin_film: Some(ThinFilm { ior: 1.38, thickness_nm: 270.0 }), ..Default::default() };
                    let mut accepted = 0;
                    for i in 0..2048_u32 {
                        let random =
                            [(f64::from(i) + 0.5) / 2048.0, radical_inverse(i), radical_inverse(i.wrapping_mul(1664525).wrapping_add(1013904223))];
                        if let Some(sample) = model.sample(incident, Vec3::Z, ni, nt, 550.0, random).unwrap() {
                            assert!(!sample.delta);
                            assert_eq!(sample.crosses, sample.direction.z < 0.0);
                            let eval = model.evaluate(incident, sample.direction, Vec3::Z, ni, nt, 550.0).unwrap();
                            close(sample.pdf, eval.pdf, sample.pdf * 1e-12 + 1e-14);
                            close(sample.weight, eval.value * sample.direction.z.abs() / eval.pdf, sample.weight * 2e-8 + 1e-10);
                            assert!(sample.weight >= 0.0 && sample.weight <= (ni / nt).powi(2).max(1.0) * (1.0 + 1e-12));
                            accepted += 1;
                        }
                    }
                    assert!(accepted > 200);
                }
            }
        }
    }

    #[test]
    fn visible_normal_density_integrates_to_one_and_rough_furnace_cannot_gain_energy() {
        let nz = 256_u32;
        let np = 512_u32;
        let solid_angle = TAU / f64::from(nz * np);
        for alpha in [0.3, 0.7, 1.0] {
            for cosine in [0.2, 0.7, 1.0] {
                let v = Vec3::new((1.0_f64 - cosine * cosine).sqrt(), 0.0, cosine);
                let mut integral = 0.0;
                for z in 0..nz {
                    let z = (f64::from(z) + 0.5) / f64::from(nz);
                    let radius = (1.0 - z * z).sqrt();
                    for phi in 0..np {
                        let (s, c) = (TAU * (f64::from(phi) + 0.5) / f64::from(np)).sin_cos();
                        let m = Vec3::new(radius * c, radius * s, z);
                        integral += ggx_distribution(m, Vec3::Z, alpha) * smith_g1(cosine, alpha) * v.dot(m).max(0.0) / cosine;
                    }
                }
                close(integral * solid_angle, 1.0, 0.002);
                let model = GlassModel { roughness: alpha.sqrt(), ..Default::default() };
                let mut furnace = 0.0;
                for i in 0..8192_u32 {
                    let random =
                        [(f64::from(i) + 0.5) / 8192.0, radical_inverse(i), radical_inverse(i.wrapping_mul(1664525).wrapping_add(1013904223))];
                    if let Some(sample) = model.sample(-v, Vec3::Z, 1.0, 1.5, 550.0, random).unwrap() {
                        furnace += sample.weight * if sample.crosses { 1.5 * 1.5 } else { 1.0 };
                    }
                }
                assert!((0.45..=1.000000001).contains(&(furnace / 8192.0)), "rough single-scattering furnace={}", furnace / 8192.0);
            }
        }
    }

    #[test]
    fn matched_indices_and_zero_roughness_retain_discrete_physical_limits() {
        let incident = direction(Vec3::new(0.8, 0.0, -0.6)).unwrap();
        for roughness in [0.0, 0.5, 1.0] {
            let model = GlassModel { roughness, ..Default::default() };
            let sample = model.sample(incident, Vec3::Z, 1.5, 1.5, 550.0, [0.7, 0.2, 0.1]).unwrap().unwrap();
            assert!(sample.delta && sample.crosses);
            close(sample.weight, 1.0, 0.0);
            close((sample.direction - incident).len(), 0.0, 1e-15);
        }
        let model = GlassModel::default();
        let reflected = model.sample(incident, Vec3::Z, 1.0, 1.5, 550.0, [0.0; 3]).unwrap().unwrap();
        assert!(reflected.delta && !reflected.crosses);
        close(reflected.weight, 1.0, 0.0);
        let transmitted = model.sample(incident, Vec3::Z, 1.0, 1.5, 550.0, [0.9; 3]).unwrap().unwrap();
        assert!(transmitted.delta && transmitted.crosses);
        close(transmitted.weight, 1.0 / 2.25, 1e-15);
        assert_eq!(model.sample(incident, Vec3::Z, 1.0, 1.5, 550.0, [1.0, 0.0, 0.0]), Err(GlassModelError::Sample));
    }
}

fn visible_normal(v: Vec3, n: Vec3, alpha: f64, u: [f64; 2]) -> Result<Vec3> {
    let tangent = direction(if n.z.abs() < 0.9 { Vec3::Z.cross(n) } else { Vec3::X.cross(n) })?;
    let bitangent = n.cross(tangent);
    let stretched = direction(Vec3::new(alpha * v.dot(tangent), alpha * v.dot(bitangent), v.dot(n)))?;
    let side = if stretched.x == 0.0 && stretched.y == 0.0 { Vec3::X } else { direction(Vec3::new(-stretched.y, stretched.x, 0.0))? };
    let vertical = stretched.cross(side);
    let radius = u[0].sqrt();
    let (sine, cosine) = (TAU * u[1]).sin_cos();
    let x = radius * cosine;
    let mix = 0.5 * (1.0 + stretched.z);
    let y = (1.0 - mix) * (1.0 - x * x).max(0.0).sqrt() + mix * radius * sine;
    let height = (1.0 - x * x - y * y).max(0.0).sqrt();
    let hemisphere = side * x + vertical * y + stretched * height;
    let local = direction(Vec3::new(alpha * hemisphere.x, alpha * hemisphere.y, hemisphere.z.max(0.0)))?;
    direction(tangent * local.x + bitangent * local.y + n * local.z)
}
fn ggx_distribution(m: Vec3, n: Vec3, alpha: f64) -> f64 {
    let cosine = m.dot(n);
    if cosine <= 0.0 {
        return 0.0;
    }
    let sine = m.cross(n);
    let denominator = sine.dot(sine) + alpha * alpha * cosine * cosine;
    let root = (alpha / denominator) / PI.sqrt();
    root * root
}
fn smith_root(cosine: f64, alpha: f64) -> f64 {
    (cosine * cosine + alpha * alpha * (1.0 - cosine * cosine).max(0.0)).sqrt()
}
fn smith_g1(cosine: f64, alpha: f64) -> f64 {
    2.0 * cosine / (cosine + smith_root(cosine, alpha))
}
fn smith_ratio(cv: f64, cl: f64, alpha: f64) -> f64 {
    if cv <= 0.0 || cl <= 0.0 {
        return 0.0;
    }
    let sv = smith_root(cv, alpha);
    let sl = smith_root(cl, alpha);
    cl * (cv + sv) / (sv * cl + sl * cv)
}

#[derive(Clone, Copy)]
struct Complex {
    re: f64,
    im: f64,
}
impl Complex {
    fn real(re: f64) -> Self {
        Self { re, im: 0.0 }
    }
    fn add(self, b: Self) -> Self {
        Self { re: self.re + b.re, im: self.im + b.im }
    }
    fn sub(self, b: Self) -> Self {
        Self { re: self.re - b.re, im: self.im - b.im }
    }
    fn scale(self, b: f64) -> Self {
        Self { re: self.re * b, im: self.im * b }
    }
    fn mul(self, b: Self) -> Self {
        Self { re: self.re * b.re - self.im * b.im, im: self.re * b.im + self.im * b.re }
    }
    fn minus_i(self) -> Self {
        Self { re: self.im, im: -self.re }
    }
    fn norm_square(self) -> f64 {
        self.re * self.re + self.im * self.im
    }
    fn div(self, b: Self) -> Result<Self> {
        let denominator = b.norm_square();
        if !denominator.is_finite() || denominator <= 0.0 {
            return Err(GlassModelError::Numerical);
        }
        let value = Self { re: (self.re * b.re + self.im * b.im) / denominator, im: (self.im * b.re - self.re * b.im) / denominator };
        if !value.re.is_finite() || !value.im.is_finite() {
            return Err(GlassModelError::Numerical);
        }
        Ok(value)
    }
}

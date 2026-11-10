//! Exclusive ownership of smooth-glass caustics at opaque receivers.
//!
//! The opt-in policy transfers only the area/environment path family already
//! tagged by photon storage. It neither mixes a finite-radius density estimate
//! with camera continuation nor changes the receiver BSDF or direct lighting.
//! Callers use the actual mapped and aged base roughness at both the gather and
//! the continuation boundary. The existing full opaque base/coat BSDF and its
//! matched mixture proposal remain unchanged. A coat has its own roughness;
//! this threshold does not describe that lobe. Every opaque receiver establishes
//! its own policy.

/// Whether this opaque receiver leaves smooth-glass area/environment paths to
/// camera continuation. Invalid inputs fail closed to the existing gather policy.
/// The shader mirror is photon_continues_smooth_caustics in photons.wgsl.
pub fn continues_smooth_caustics(maximum_roughness: Option<f64>, roughness: f64) -> bool {
    maximum_roughness.is_some_and(|maximum| maximum.is_finite() && (0.0..=1.0).contains(&maximum) && (0.0..=maximum).contains(&roughness))
}

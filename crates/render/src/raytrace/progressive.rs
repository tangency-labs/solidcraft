//! Deterministic counter sampling and owned camera-film continuation.
//! No target sample count, work quantum or preview cadence enters the sampler.

use crate::{film::primary::PrimaryFilm, job::CpuProgressiveOptions};

pub(super) struct Progressive {
    pub film: PrimaryFilm,
    pub options: CpuProgressiveOptions,
    pub resolving: bool,
    pub resolve_pixel: usize,
}

/// Integer-only mixing before conversion to exactly representable 53-bit unit
/// variates. Domains separate camera, BSDF and environment choices at each bounce.
pub(super) fn variates(seed: u32, pixel: u64, ordinal: u32, domain: u64) -> [f64; 2] {
    fn mix(mut value: u64) -> u64 {
        value ^= value >> 30;
        value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value ^= value >> 27;
        value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }
    let key = mix(u64::from(seed) ^ 0x4350_5546_494c_4d00)
        ^ mix(pixel.wrapping_add(0x9e37_79b9_7f4a_7c15))
        ^ mix(u64::from(ordinal).wrapping_add(0xd1b5_4a32_d192_ed03))
        ^ mix(domain.wrapping_add(0x94d0_49bb_1331_11eb));
    [0_u64, 1].map(|axis| (mix(key ^ mix(axis.wrapping_add(0x632b_e59b_d9b4_e019))) >> 11) as f64 / 9_007_199_254_740_992.0)
}

//! Stable semantic sample domains for independent and randomized Sobol pairs.
//!
//! Each domain owns an independently scrambled two-dimensional digital net.
//! This avoids consuming dimensions conditionally on path length. It is not a
//! claim that the concatenation is an optimized high-dimensional Sobol net.
//! Pilots remain independent; dependent production samples do not provide an
//! IID standard-error estimate. See docs/render-camera-sampling.md.
use serde::{Deserialize, Serialize};

/// Explicit camera/path sampling choice. Existing setups retain their stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sampler {
    #[default]
    Independent,
    ScrambledSobol,
}

impl Sampler {
    pub fn is_independent(&self) -> bool {
        *self == Self::Independent
    }

    /// Pure counter sampling: targets, work quanta, tiles and scheduling never
    /// enter the key. A domain identifies a physical decision, including bounce
    /// and branch where applicable, rather than a mutable random-call counter.
    pub fn pair(self, seed: u32, pixel: u64, ordinal: u32, domain: u64) -> [f64; 2] {
        match self {
            Self::Independent => independent_pair(seed, pixel, ordinal, domain),
            Self::ScrambledSobol => scrambled_sobol_pair(seed, pixel, ordinal, domain),
        }
    }
}

/// The established CPU 53-bit counter stream, preserved bit for bit.
pub fn independent_pair(seed: u32, pixel: u64, ordinal: u32, domain: u64) -> [f64; 2] {
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

// Odd multiplication and xor shifts permute u32. The same wrapping operations
// appear in sampling.wgsl; no external tables or code are required.
fn mix32(mut value: u32) -> u32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^ (value >> 16)
}

/// The first two Sobol generator columns. The second dimension uses the degree
/// one primitive polynomial x + 1: v[j+1] = v[j] xor (v[j] >> 1).
fn sobol_bits(ordinal: u32) -> [u32; 2] {
    let mut gray = ordinal ^ (ordinal >> 1);
    let mut x = 0;
    let mut y = 0;
    let mut direction_x = 1_u32 << 31;
    let mut direction_y = direction_x;
    while gray != 0 {
        if gray & 1 != 0 {
            x ^= direction_x;
            y ^= direction_y;
        }
        gray >>= 1;
        direction_x >>= 1;
        direction_y ^= direction_y >> 1;
    }
    [x, y]
}

/// Hash-seeded nested binary permutations. Each output bit uses only earlier
/// input bits, the digit index and the scramble key. The map is therefore
/// bijective on every dyadic prefix. Hashing supplies pseudorandom permutations;
/// it does not promise mathematical independence of a finite hash family.
fn nested_scramble(bits: u32, key: u32) -> u32 {
    let mut prefix = 0_u32;
    let mut result = 0_u32;
    for digit in 0..23_u32 {
        let bit = (bits >> (31 - digit)) & 1;
        let flip = mix32(key ^ mix32(prefix ^ (digit + 1).wrapping_mul(0x9e37_79b9))) & 1;
        result = (result << 1) | (bit ^ flip);
        prefix = (prefix << 1) | bit;
    }
    result
}

/// Randomized two-dimensional Sobol pair on a 23-bit midpoint grid, strictly
/// inside (0, 1) and exactly representable in f32. The extra midpoint bit keeps
/// inverse free-flight sampling away from both endpoints. Rust and WGSL agree
/// for u32 pixel/domain inputs; CPU high words also participate in the key.
pub fn scrambled_sobol_pair(seed: u32, pixel: u64, ordinal: u32, domain: u64) -> [f64; 2] {
    let key = mix32(seed ^ 0x534f_424f);
    let key = mix32(key ^ pixel as u32);
    let key = mix32(key ^ (pixel >> 32) as u32);
    let key = mix32(key ^ domain as u32);
    let key = mix32(key ^ (domain >> 32) as u32);
    let [x, y] = sobol_bits(ordinal);
    [
        f64::from(2 * nested_scramble(x, mix32(key ^ 0x585f_4158)) + 1) / 16_777_216.0,
        f64::from(2 * nested_scramble(y, mix32(key ^ 0x595f_4158)) + 1) / 16_777_216.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_two_generator_columns_match_the_small_exact_net() {
        let expected = [[0, 0], [4, 4], [6, 2], [2, 6], [3, 3], [7, 7], [5, 1], [1, 5]];
        for (ordinal, expected) in expected.into_iter().enumerate() {
            assert_eq!(sobol_bits(ordinal as u32).map(|bits| bits >> 29), expected);
        }
    }

    #[test]
    fn scrambled_pairs_cover_all_dyadic_rectangles_once() {
        // This checks the net's mathematical property, including non-square
        // elementary intervals, instead of reproducing the implementation.
        for seed in [0, 1, 81, u32::MAX] {
            for power in 1..=10 {
                let count = 1_u32 << power;
                for x_power in 0..=power {
                    let width = 1_u32 << x_power;
                    let height = count / width;
                    let mut seen = vec![false; count as usize];
                    for ordinal in 0..count {
                        let [x, y] = scrambled_sobol_pair(seed, 37, ordinal, 0x4c45_4e53);
                        let cell = (x * f64::from(width)) as u32 + width * (y * f64::from(height)) as u32;
                        assert!(!seen[cell as usize], "duplicate dyadic cell at seed {seed}, power {power}, split {x_power}");
                        seen[cell as usize] = true;
                    }
                    assert!(seen.into_iter().all(|value| value));
                }
            }
        }
    }

    #[test]
    fn all_counters_are_bounded_and_domains_are_stable() {
        for seed in [0, 0xa511_e9b3, u32::MAX] {
            for ordinal in [0, 1, 16, 65_535, u32::MAX] {
                for pixel in [0, 1, u64::MAX] {
                    for domain in [0, 0x4c45_4e53, u64::MAX] {
                        for sampler in [Sampler::Independent, Sampler::ScrambledSobol] {
                            let pair = sampler.pair(seed, pixel, ordinal, domain);
                            assert!(pair.into_iter().all(|value| value.is_finite() && (0.0..1.0).contains(&value)));
                            assert_eq!(pair, sampler.pair(seed, pixel, ordinal, domain));
                            if sampler == Sampler::ScrambledSobol {
                                for value in pair {
                                    assert!(value > 0.0 && value < 1.0);
                                    assert_eq!(value, f64::from(value as f32));
                                    assert_eq!((value * 16_777_216.0) as u32 & 1, 1, "a midpoint has one odd low fractional bit");
                                }
                            }
                        }
                    }
                }
            }
        }
        let base = scrambled_sobol_pair(81, 37, 19, 0);
        assert_ne!(base, scrambled_sobol_pair(81 ^ 0xa511_e9b3, 37, 19, 0));
        assert_ne!(base, scrambled_sobol_pair(81, 37, 19, 0x4c45_4e53));
        assert_ne!(base, scrambled_sobol_pair(81, 37 + (1_u64 << 32), 19, 0));
    }

    #[test]
    fn midpoint_grid_removes_a_reproducible_zero_free_flight() {
        // This valid camera key previously rounded its first interior flight
        // to the source boundary: raw 24-bit output was exactly zero.
        let [uniform, _] = scrambled_sobol_pair(453_936, 0, 43, 0x5041_5449_5000);
        assert_eq!(uniform, 1.0 / 16_777_216.0);
        let distance = -(-uniform).ln_1p();
        assert!(1.0 - distance < 1.0 && 1.0 - distance > 0.0);
    }

    #[test]
    fn sampler_parsing_preserves_the_default_and_rejects_unknown_modes() {
        assert_eq!(Sampler::default(), Sampler::Independent);
        assert!(Sampler::default().is_independent());
        assert_eq!(serde_json::from_str::<Sampler>(r#""scrambled_sobol""#).unwrap(), Sampler::ScrambledSobol);
        assert!(serde_json::from_str::<Sampler>(r#""noise_free""#).is_err());
    }

    #[test]
    fn randomized_net_integrates_linear_functions_without_a_false_noise_claim() {
        for seed in 0..8 {
            let count = 1024;
            let integral: f64 = (0..count)
                .map(|ordinal| {
                    let [x, y] = scrambled_sobol_pair(seed, 0, ordinal, 0);
                    x + 2.0 * y
                })
                .sum::<f64>()
                / f64::from(count);
            // Every marginal stratum has width 1/N, so this bound follows from
            // stratification, independent of the details of the scramble.
            assert!((integral - 1.5).abs() <= 3.0 / f64::from(count));
        }
    }
}

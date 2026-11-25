//! A quick, fixed hasher for the maps keyed by integers and floats' bits (`simplify`'s welding
//! index, the mesher's lookups), in place of std's `SipHash`, seeded per process. Every map that
//! takes it is only looked up, so no result depends on the hash: a walk of one is sorted first.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// A map hashed by [`BitsHasher`].
pub(crate) type BitsMap<K, V> = HashMap<K, V, BuildHasherDefault<BitsHasher>>;

/// Each word folded in by a rotate, an xor and one multiply, a word at a time (as `simplify`'s
/// welding hasher did). The product's low bits see only its factors' low bits, and a round float's
/// bits end in zeros, so the words leave their differences in the high bits: `finish` mixes them
/// all down (`SplitMix64`'s finaliser) to the low bits a table takes its bucket from.
#[derive(Default)]
pub(crate) struct BitsHasher(u64);

impl Hasher for BitsHasher {
    fn finish(&self) -> u64 {
        let mut x = self.0;
        x ^= x >> 30;
        x = x.wrapping_mul(0xbf_58_47_6d_1c_e4_e5_b9);
        x ^= x >> 27;
        x = x.wrapping_mul(0x94_d0_49_bb_13_31_11_eb);
        x ^ (x >> 31)
    }
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            for (w, b) in word.iter_mut().zip(chunk) {
                *w = *b;
            }
            self.write_u64(u64::from_le_bytes(word));
        }
    }
    fn write_u64(&mut self, x: u64) {
        self.0 = (self.0.rotate_left(5) ^ x).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    fn write_u32(&mut self, x: u32) {
        self.write_u64(u64::from(x));
    }
    fn write_usize(&mut self, x: usize) {
        self.write(&x.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::{BuildHasher, Hash};

    fn hash<T: Hash>(t: &T) -> u64 {
        BuildHasherDefault::<BitsHasher>::default().hash_one(t)
    }

    /// Fixed: the same key hashes the same in every process (no seed), keys differing in one
    /// word differ, and round floats' bits (zeros at the bottom) still spread over the low bits
    /// a table takes its bucket from.
    #[test]
    fn fixed_and_spread() {
        assert_eq!(hash(&(3u32, 4u32)), hash(&(3u32, 4u32)));
        assert_ne!(hash(&(3u32, 4u32)), hash(&(4u32, 3u32)));
        let low: std::collections::BTreeSet<u64> = (0..64)
            .map(|i| hash(&[f64::from(i) * 0.25, 0.5].map(f64::to_bits)) & 0x3f)
            .collect();
        assert!(low.len() > 32, "{} of 64 low-bit patterns", low.len());
    }
}

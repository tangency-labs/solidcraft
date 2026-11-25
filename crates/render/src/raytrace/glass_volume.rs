//! Exact dyadic sign of six times the captured closed mesh volume.
//! Certificate coordinates are zero or normal f64 values with magnitude2^-128..1e12.
//! A coordinate is an integer mantissa times2^e with -180<=e<=-13.
//! Triple products share2^-540; their magnitudes occupy at most660 bits. Even
//! six terms for usize::MAX faces need fewer than728 bits, so12 limbs suffice.
//! This fixed continuation state has no heap, tolerance or floating accumulation.
use crate::job::{self, ErrorCode, JobError};
use solidcore_record::Vec3;
fn invalid() -> JobError {
    JobError::new(ErrorCode::InvalidScene, "CPU exact glass volume exceeds its dyadic certificate domain")
}
#[derive(Default)]
pub(super) struct SignedVolume {
    limbs: [u64; 12],
    negative: bool,
}
fn magnitude_compare(a: &[u64; 12], b: &[u64; 12]) -> std::cmp::Ordering {
    a.iter().rev().cmp(b.iter().rev())
}
fn subtract(a: &mut [u64; 12], b: &[u64; 12]) -> job::Result<()> {
    let mut borrow = false;
    for (a, b) in a.iter_mut().zip(b) {
        let (value, first) = a.overflowing_sub(*b);
        let (value, second) = value.overflowing_sub(u64::from(borrow));
        *a = value;
        borrow = first || second;
    }
    if borrow {
        return Err(invalid());
    }
    Ok(())
}
fn parts(value: f64) -> job::Result<Option<(u64, i32, bool)>> {
    if value == 0.0 {
        return Ok(None);
    }
    let bits = value.to_bits();
    let exponent = ((bits >> 52) & 2047) as i32;
    if !value.is_finite() || value.abs() < f64::from_bits((1023 - 128) << 52) || value.abs() > super::MAX_COORDINATE_MM {
        return Err(invalid());
    }
    Ok(Some(((bits & ((1_u64 << 52) - 1)) | (1_u64 << 52), exponent - 1023 - 52, bits >> 63 != 0)))
}
impl SignedVolume {
    pub(super) fn positive(&self) -> bool {
        !self.negative && self.limbs.iter().any(|v| *v != 0)
    }
    /// Exactly one of the six determinant products per charged caller transition.
    pub(super) fn add_term(&mut self, p: [Vec3; 3], term: usize) -> job::Result<()> {
        let [a, b, c] = p;
        let (x, y, z, minus) = match term {
            0 => (a.x, b.y, c.z, false),
            1 => (a.y, b.z, c.x, false),
            2 => (a.z, b.x, c.y, false),
            3 => (a.x, b.z, c.y, true),
            4 => (a.y, b.x, c.z, true),
            5 => (a.z, b.y, c.x, true),
            _ => return Err(invalid()),
        };
        let [Some((x, ex, sx)), Some((y, ey, sy)), Some((z, ez, sz))] = [parts(x)?, parts(y)?, parts(z)?] else {
            return Ok(());
        };
        let xy = u128::from(x) * u128::from(y);
        let low = u128::from(xy as u64) * u128::from(z);
        let high = (xy >> 64) * u128::from(z) + (low >> 64);
        let product = [low as u64, high as u64, (high >> 64) as u64];
        let shift = usize::try_from(ex + ey + ez + 540).map_err(|_| invalid())?;
        let mut value = [0_u64; 12];
        let bit = shift % 64;
        for (i, word) in product.into_iter().enumerate() {
            let at = shift / 64 + i;
            *value.get_mut(at).ok_or_else(invalid)? |= word << bit;
            if bit != 0 {
                *value.get_mut(at + 1).ok_or_else(invalid)? |= word >> (64 - bit);
            }
        }
        let negative = sx ^ sy ^ sz ^ minus;
        if self.negative == negative {
            let mut carry = 0_u128;
            for (a, b) in self.limbs.iter_mut().zip(value) {
                let sum = u128::from(*a) + u128::from(b) + carry;
                *a = sum as u64;
                carry = sum >> 64;
            }
            if carry != 0 {
                return Err(invalid());
            }
        } else if magnitude_compare(&self.limbs, &value).is_lt() {
            subtract(&mut value, &self.limbs)?;
            self.limbs = value;
            self.negative = negative;
        } else {
            subtract(&mut self.limbs, &value)?;
        }
        if self.limbs.iter().all(|v| *v == 0) {
            self.negative = false;
        }
        Ok(())
    }
}

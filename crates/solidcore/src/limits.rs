//! A documented ceiling on allocations whose size comes from caller input, so an
//! absurd count fails with an error on every machine instead of depending on how much
//! memory the operating system is willing to promise.
//!
//! Upstream: no counterpart. Upstream sizes these arrays from the caller's argument
//! (a degree, count, index or voxel count) and fails softly when `malloc` /
//! `smos_Calloc` returns `NULL`. A fallible Rust reservation is not equivalent: with
//! memory overcommit a reservation of tens of GiB succeeds, and the zero-fill that
//! follows then pages it all in (issue #505). Each such site therefore refuses a
//! request above [`MAX_ALLOC_BYTES`] up front and reports it as it reports a failed
//! reservation.
//!
//! | Rust                 | Upstream                                     |
//! |----------------------|----------------------------------------------|
//! | [`MAX_ALLOC_BYTES`]  | none (the machine's memory)                  |
//! | [`fits`]             | none                                         |
//! | [`try_filled`]       | `malloc`/`smos_Calloc` + fill, `NULL` on failure |
//! | [`try_reserve_total`] | `smos_Realloc` growing an array, `NULL` on failure |
//!
//! Deviations: a request above [`MAX_ALLOC_BYTES`] fails even where the machine could
//! have backed it. No upstream test or caller comes near the bound (the largest arrays
//! the difftest fixtures build are a few MiB).

/// Largest single allocation, in bytes, that a caller-sized array may take (1 GiB).
pub const MAX_ALLOC_BYTES: usize = 1 << 30;

/// Whether `len` values of `T` stay within [`MAX_ALLOC_BYTES`].
#[must_use]
pub const fn fits<T>(len: usize) -> bool {
    let size = if size_of::<T>() == 0 {
        1
    } else {
        size_of::<T>()
    };
    match len.checked_mul(size) {
        Some(bytes) => bytes <= MAX_ALLOC_BYTES,
        None => false,
    }
}

/// `len` copies of `value`, or `None` when that exceeds [`MAX_ALLOC_BYTES`] or cannot
/// be reserved.
#[must_use]
pub fn try_filled<T: Clone>(len: usize, value: T) -> Option<Vec<T>> {
    if !fits::<T>(len) {
        return None;
    }
    let mut v = Vec::new();
    v.try_reserve_exact(len).ok()?;
    v.resize(len, value);
    Some(v)
}

/// Makes room in `v` for a total of `len` values; `false` (and `v` unchanged) when
/// that exceeds [`MAX_ALLOC_BYTES`] or cannot be reserved.
#[must_use]
pub fn try_reserve_total<T>(v: &mut Vec<T>, len: usize) -> bool {
    fits::<T>(len) && v.try_reserve_exact(len.saturating_sub(v.len())).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound() {
        assert!(fits::<f64>(MAX_ALLOC_BYTES / 8));
        assert!(!fits::<f64>(MAX_ALLOC_BYTES / 8 + 1));
        assert!(!fits::<f64>(usize::MAX));
        assert!(fits::<()>(MAX_ALLOC_BYTES));
        assert!(!fits::<()>(MAX_ALLOC_BYTES + 1));
        assert_eq!(try_filled(3, 1.5), Some(vec![1.5; 3]));
        assert_eq!(try_filled(1 << 31, 0.0_f64), None);
        let mut v = vec![1_u8];
        assert!(try_reserve_total(&mut v, 10));
        assert!(v.capacity() >= 10);
        assert!(!try_reserve_total(&mut v, MAX_ALLOC_BYTES + 1));
        assert_eq!(v, [1]);
    }
}

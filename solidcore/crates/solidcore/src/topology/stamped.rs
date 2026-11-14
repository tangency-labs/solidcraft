//! Write stamps on a [`super::Brep`]'s fields (not upstream).
//!
//! Every field of a B-rep is a [`Stamped`] value: reads go through `Deref`, and every
//! mutable access (`DerefMut`: a `*_mut` accessor, an arena insert, a list push, a
//! whole-value assignment through `*`) counts as a write. A value made by `new`,
//! `default` or `clone` takes an origin no other value has, so two equal [`Stamp`]s
//! name the same value with no mutable access between them. The B-rep check uses this
//! to tell that a pass left the body as it found it
//! ([`super::Brep::write_stamps`]).
//!
//! A stamp is conservative: a mutable access that changes nothing still counts. Interior
//! caches that geometry fills through `&self` are not writes; they never change a result.

use core::sync::atomic::{AtomicU64, Ordering};

/// The next origin to hand out. 0 is never handed out.
static ORIGINS: AtomicU64 = AtomicU64::new(1);

fn fresh_origin() -> u64 {
    ORIGINS.fetch_add(1, Ordering::Relaxed)
}

/// Which value a [`Stamped`] is and how many mutable accesses it has had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stamp {
    origin: u64,
    writes: u64,
}

/// A value that counts its mutable accesses (see the module docs).
pub(crate) struct Stamped<T> {
    value: T,
    origin: u64,
    writes: u64,
}

impl<T> Stamped<T> {
    /// `value` under an origin of its own, with no writes yet.
    pub(crate) fn new(value: T) -> Self {
        Self {
            value,
            origin: fresh_origin(),
            writes: 0,
        }
    }

    /// The value, for `const` readers (`Deref` is not `const`).
    pub(crate) const fn inner(&self) -> &T {
        &self.value
    }

    /// The value's stamp now.
    pub(crate) fn stamp(&self) -> Stamp {
        Stamp {
            origin: self.origin,
            writes: self.writes,
        }
    }
}

impl<T> core::ops::Deref for Stamped<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> core::ops::DerefMut for Stamped<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.writes = self.writes.wrapping_add(1);
        &mut self.value
    }
}

impl<T: Clone> Clone for Stamped<T> {
    /// A copy is another value: it takes its own origin.
    fn clone(&self) -> Self {
        Self::new(self.value.clone())
    }
}

impl<T: Default> Default for Stamped<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for Stamped<T> {
    /// The value alone, as without the stamp.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.value.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_keeps_the_stamp_and_a_mutable_access_moves_it() {
        let mut v = Stamped::new(vec![1, 2]);
        let before = v.stamp();
        assert_eq!(v.len(), 2);
        assert_eq!(v.stamp(), before);
        v.push(3);
        assert_ne!(v.stamp(), before);
        let after = v.stamp();
        // a mutable access that changes nothing still counts
        let _ = v.first_mut();
        assert_ne!(v.stamp(), after);
    }

    #[test]
    fn a_copy_or_a_new_value_never_shares_a_stamp() {
        let v = Stamped::new(7_u32);
        let copy = v.clone();
        assert_eq!(*copy, 7);
        assert_ne!(copy.stamp(), v.stamp());
        let mut a = Stamped::new(0_u32);
        let b = Stamped::new(0_u32);
        assert_ne!(a.stamp(), b.stamp());
        // a value put in place of another, even with as many writes, is told apart
        let mut c = Stamped::new(0_u32);
        *a = 1;
        *c = 1;
        assert_ne!(a.stamp(), c.stamp());
        let a_stamp = a.stamp();
        core::mem::swap(&mut a, &mut c);
        assert_ne!(a.stamp(), a_stamp);
    }

    #[test]
    fn debug_shows_the_value_alone() {
        assert_eq!(format!("{:?}", Stamped::new(vec![1])), "[1]");
    }
}

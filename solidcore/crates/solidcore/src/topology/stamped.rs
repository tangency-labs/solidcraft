//! Write stamps on a [`super::Brep`]'s fields (not upstream).
//!
//! Every field of a B-rep is a [`Stamped`] value: reads go through `Deref`, and every
//! mutable access (`DerefMut`: a `*_mut` accessor, an arena insert, a list push, a
//! whole-value assignment through `*`) counts as a write. The fields of one B-rep
//! value share an [`Origin`]: a fresh allocation made when the value is (`Default`, or
//! the B-rep's copy), compared by address, as the B-rep's progress and removal tags are.
//! A [`Stamp`] keeps its origin alive, so the address cannot be reused while a reading is
//! held. Two equal stamps therefore name the same field of the same value with no
//! mutable access between them, with no global state. The B-rep check uses this to tell
//! that a pass left the body as it found it ([`super::Brep::write_stamps`]).
//!
//! A stamp is conservative: a mutable access that changes nothing still counts. Interior
//! caches that geometry fills through `&self` are not writes; they never change a result.

use std::sync::Arc;

/// The identity of one B-rep value, shared by its fields: an allocation of its own,
/// compared by address.
#[derive(Debug, Clone)]
pub(crate) struct Origin(Arc<()>);

impl Origin {
    /// An origin no other live value has.
    pub(crate) fn new() -> Self {
        Self(Arc::new(()))
    }
}

impl PartialEq for Origin {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Origin {}

/// Which value a [`Stamped`] is and how many mutable accesses it has had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stamp {
    origin: Origin,
    writes: u64,
}

/// A value that counts its mutable accesses (see the module docs).
pub(crate) struct Stamped<T> {
    value: T,
    origin: Origin,
    writes: u64,
}

impl<T> Stamped<T> {
    /// `value` under `origin`, with no writes yet.
    pub(crate) fn new(value: T, origin: &Origin) -> Self {
        Self {
            value,
            origin: origin.clone(),
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
            origin: self.origin.clone(),
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
        let mut v = Stamped::new(vec![1, 2], &Origin::new());
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
    fn values_of_other_origins_never_share_a_stamp() {
        let (one, other) = (Origin::new(), Origin::new());
        assert_ne!(one, other);
        assert_eq!(one, one.clone());
        let mut a = Stamped::new(0_u32, &one);
        let mut c = Stamped::new(0_u32, &other);
        assert_ne!(a.stamp(), c.stamp());
        // a value put in place of another, even with as many writes, is told apart
        *a = 1;
        *c = 1;
        assert_ne!(a.stamp(), c.stamp());
        let a_stamp = a.stamp();
        core::mem::swap(&mut a, &mut c);
        assert_ne!(a.stamp(), a_stamp);
        // a reading keeps its origin alive, so a later origin cannot take its address
        drop((a, c, one));
        assert_ne!(Stamped::new(1_u32, &Origin::new()).stamp(), a_stamp);
    }

    #[test]
    fn debug_shows_the_value_alone() {
        assert_eq!(
            format!("{:?}", Stamped::new(vec![1], &Origin::new())),
            "[1]"
        );
    }
}

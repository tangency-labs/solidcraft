//! Actual operation-owned curve copies. Handles never impersonate topology.

use crate::curve::curve::Curve;
use crate::geom::context::Context;
use crate::geom::object::ObjectRef;
use crate::{Result, SolidError};

/// An issued identity and its live operation-owned Curve slot.
/// Valid only while the issuing store and object remain alive.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CurveBinding {
    slot: usize,
    identity: ObjectRef,
    // Last only to keep valid-store ordering determined by slot and issued identity.
    // Equality checks the actual live object; expired-store handles are invalid.
    instance: usize,
}

impl std::fmt::Debug for CurveBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CurveBinding")
            .field("slot", &self.slot)
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

/// Curves retained for the whole insertion operation, including partial failures.
#[derive(Debug)]
struct OwnedCurve {
    // Stable instance identity while the operation's storage vector grows.
    curve: Box<Curve>,
}
/// Curves retained for the whole insertion operation, including partial failures.
#[derive(Debug, Default)]
pub struct InsertionCurves {
    curves: Vec<OwnedCurve>,
}
impl InsertionCurves {
    /// Move an actual curve copy into operation ownership and issue its identity.
    /// # Errors
    /// Identity-domain issuance failure; the curve is retained on this failure too.
    pub fn insert(&mut self, context: &mut Context, curve: Curve) -> Result<CurveBinding> {
        let slot = self.curves.len();
        self.curves.push(OwnedCurve {
            curve: Box::new(curve),
        });
        let curve = self.curves.get_mut(slot).ok_or_else(|| {
            SolidError::invalid_input(
                "blend::fillet_executive::InsertionCurves",
                "inserted curve slot is absent",
            )
        })?;
        let curve = curve.curve.as_mut();
        let type_code = curve.type_code();
        let key = context.geometry_key(curve.notify_identity_slot())?;
        Ok(CurveBinding {
            slot,
            identity: ObjectRef { type_code, key },
            instance: std::ptr::from_ref(curve).addr(),
        })
    }
    /// Borrow the actual retained object, rejecting a foreign/replaced binding.
    #[must_use]
    pub fn get(&self, binding: CurveBinding) -> Option<&Curve> {
        let curve = self.curves.get(binding.slot)?.curve.as_ref();
        (std::ptr::from_ref(curve).addr() == binding.instance
            && curve.notify_key() == Some(binding.identity.key)
            && curve.type_code() == binding.identity.type_code)
            .then_some(curve)
    }
    /// Mutably borrow the same retained object after identity validation.
    pub fn get_mut(&mut self, binding: CurveBinding) -> Option<&mut Curve> {
        self.get(binding)?;
        Some(self.curves.get_mut(binding.slot)?.curve.as_mut())
    }
    /// Resolve a binding with an explicit failure at a provider capability boundary.
    /// # Errors
    /// A binding from a different store or a replaced object.
    pub fn checked(&self, binding: CurveBinding) -> Result<&Curve> {
        self.get(binding).ok_or_else(|| {
            SolidError::invalid_input(
                "blend::fillet_executive::InsertionCurves",
                "foreign or replaced curve binding",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::Line;
    use crate::math::Point3;
    use crate::Kernel;

    #[test]
    fn independent_context_numeric_identity_collision_does_not_alias_live_objects() {
        let mut a = Kernel::default();
        let mut b = Kernel::default();
        let mut first = InsertionCurves::default();
        let mut second = InsertionCurves::default();
        let curve = || {
            Curve::from(Line::from_points(
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                3,
            ))
        };
        let one = first.insert(&mut a.context, curve()).unwrap();
        let two = second.insert(&mut b.context, curve()).unwrap();
        assert_eq!(one.slot, two.slot);
        assert_eq!(one.identity, two.identity);
        assert_ne!(one.instance, two.instance);
        assert!(first.get(two).is_none());
        assert!(second.get(one).is_none());
        assert!(first.get(one).is_some());
        assert!(second.get(two).is_some());
        // Growing the owner vector must not invalidate an existing live object.
        for _ in 0..64 {
            first.insert(&mut a.context, curve()).unwrap();
        }
        assert!(first.get(one).is_some());
    }
}

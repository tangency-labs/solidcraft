//! Bidirectional topology mate storage for the Boolean intersector.
//!
//! Upstream: `source/SMLib/inc/SmTopologyIntersector.h`,
//! `source/SMLib/src/SmTopologyIntersector.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`MateMap`], [`Relations`] | `m_pBToO`, `m_pOToB` (edge/vertex subset) |
//! | [`MateMap::other`], [`MateMap::primary`] | `GetOtherMate`, `GetBrepMate` |
//!
//! Deviations: typed ids and ordered maps replace object pointers. `record` stores
//! an already-established relationship, checking the conflicts that `Relate`
//! rejects. It is not the full `Relate`: tolerance updates and its other entity
//! kinds remain owned by this unit. Merge callers must not bypass that operation
//! for newly created geometry. No input-dependent recursion.

use std::collections::BTreeMap;

use crate::error::{Result, SolidError};
use crate::topology::{EdgeId, VertexId};

/// The same mate relationship viewed in both operand directions.
#[derive(Debug, Clone)]
pub struct MateMap<I> {
    to_other: BTreeMap<I, I>,
    to_primary: BTreeMap<I, I>,
}

impl<I> Default for MateMap<I> {
    fn default() -> Self {
        Self {
            to_other: BTreeMap::new(),
            to_primary: BTreeMap::new(),
        }
    }
}

impl<I: Ord + Copy> MateMap<I> {
    /// `GetOtherMate`: the mate of a primary operand id.
    pub fn other(&self, primary: I) -> Option<I> {
        self.to_other.get(&primary).copied()
    }

    /// `GetBrepMate`: the mate of an other operand id.
    pub fn primary(&self, other: I) -> Option<I> {
        self.to_primary.get(&other).copied()
    }

    /// Record an established pair. Conflicting pairs fail before either map changes.
    /// Call the owning intersector's `Relate` for new geometry, including its
    /// tolerance updates, before recording that relationship here.
    pub fn record(&mut self, primary: I, other: I) -> Result<()> {
        if self.other(primary).is_some_and(|v| v != other)
            || self.primary(other).is_some_and(|v| v != primary)
        {
            return Err(SolidError::sm_err(
                "boolean::topology_intersector::p1::record",
            ));
        }
        self.to_other.insert(primary, other);
        self.to_primary.insert(other, primary);
        Ok(())
    }
}

/// Edge and vertex mates shared by the merge stages; every id is operand-local.
#[derive(Debug, Default, Clone)]
pub struct Relations {
    /// Edge mates in both directions.
    pub edges: MateMap<EdgeId>,
    /// Vertex mates in both directions.
    pub vertices: MateMap<VertexId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conflicts_leave_both_directions_unchanged() {
        let mut m = MateMap::default();
        m.record(1, 8).unwrap();
        m.record(1, 8).unwrap();
        assert!(m.record(1, 9).is_err());
        assert!(m.record(2, 8).is_err());
        assert_eq!(m.other(1), Some(8));
        assert_eq!(m.primary(8), Some(1));
        assert_eq!(m.other(2), None);
        assert_eq!(m.primary(9), None);
    }
}

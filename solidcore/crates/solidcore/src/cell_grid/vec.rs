//! The few plane-vector helpers the cell grid planner needs beyond [`Vec2`]'s own methods, and
//! the ring indices of a closed polygon. (The cross product is [`crate::prism::profile::cross2`].)

use crate::math::Vec2;

/// Plane-vector operations the planner uses that [`Vec2`] lacks.
pub(crate) trait Vec2Ext: Sized {
    /// The unit vector, `None` when the length is not a finite number above 1e-9.
    fn normalized(self) -> Option<Vec2>;
    /// Whether both components are finite.
    fn is_finite(&self) -> bool;
}

impl Vec2Ext for Vec2 {
    fn normalized(self) -> Option<Vec2> {
        let l = self.length();
        (l > 1e-9 && l.is_finite()).then(|| self / l)
    }
    fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

/// The index after `k` in a ring of `n` (0 after the last).
pub(crate) fn ring_next(k: usize, n: usize) -> usize {
    if k + 1 >= n {
        0
    } else {
        k + 1
    }
}

/// The index before `k` in a ring of `n` (the last before 0).
pub(crate) fn ring_prev(k: usize, n: usize) -> usize {
    if k == 0 {
        n.saturating_sub(1)
    } else {
        k - 1
    }
}

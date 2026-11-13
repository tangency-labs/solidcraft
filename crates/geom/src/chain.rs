//! Chains of items in order: a [`Ring`] closes back on its first item (a polygon's corners, a
//! loop of segments, a fan of rays), a [`Path`] has two ends (a polyline, a sweep path). Which one
//! a chain is lives in its type, so code that walks its edges never wraps an index by hand, and
//! what only a closed chain has (an area, an inside) is only asked of a ring.

use crate::prelude::*;
use std::marker::PhantomData;
use std::ops::Deref;

use crate::Aabb2;
use solidcore_record::{Vec2, Vec3};

/// Whether a chain's last item joins its first.
pub trait Ends: sealed::Sealed {
    const CLOSED: bool;
}

/// The last item joins the first: a [`Ring`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Closed;

/// The first and last items are ends: a [`Path`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Open;

impl Ends for Closed {
    const CLOSED: bool = true;
}

impl Ends for Open {
    const CLOSED: bool = false;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Closed {}
    impl Sealed for super::Open {}
}

/// Items in order, closed or open by `E`; as a slice it is the items once each.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Chain<T, E> {
    items: Vec<T>,
    ends: PhantomData<E>,
}

/// A closed chain: the last item joins the first.
pub type Ring<T> = Chain<T, Closed>;

/// An open chain from its first item to its last.
pub type Path<T> = Chain<T, Open>;

impl<T, E: Ends> Chain<T, E> {
    pub fn new(items: Vec<T>) -> Chain<T, E> {
        Chain { items, ends: PhantomData }
    }

    /// Each item with the next: as many edges as items around a ring (the last with the first;
    /// a lone item pairs with itself), one fewer along a path.
    pub fn edges(&self) -> impl Iterator<Item = (&T, &T)> + Clone {
        let n = if E::CLOSED { self.items.len() } else { self.items.len().saturating_sub(1) };
        self.items.iter().zip(self.items.iter().cycle().skip(1)).take(n)
    }

    pub fn into_vec(self) -> Vec<T> {
        self.items
    }
}

impl<T> Ring<T> {
    /// Each item with the one before and the one after it: `(previous, item, next)`, once per
    /// item.
    pub fn corners(&self) -> impl Iterator<Item = (&T, &T, &T)> + Clone {
        let before = self.items.iter().cycle().skip(self.items.len().saturating_sub(1));
        before.zip(self.edges()).map(|(p, (c, x))| (p, c, x))
    }
}

impl<T> Path<T> {
    /// The ring through the same items: the last joined back to the first.
    pub fn close(self) -> Ring<T> {
        Chain::new(self.items)
    }
}

impl<E: Ends> Chain<Vec2, E> {
    /// The smallest box holding every item (empty for an empty chain).
    pub fn bounds(&self) -> Aabb2 {
        self.items.iter().copied().collect()
    }
}

/// A point in a plane, whichever type holds it.
pub trait Planar: Copy {
    fn xy(self) -> (f64, f64);
}

impl Planar for Vec2 {
    fn xy(self) -> (f64, f64) {
        (self.x, self.y)
    }
}

impl Planar for [f64; 2] {
    fn xy(self) -> (f64, f64) {
        (self[0], self[1])
    }
}

impl<P: Planar> Ring<P> {
    /// The area the ring encloses, positive when it runs counter-clockwise.
    pub fn signed_area(&self) -> f64 {
        0.5 * self
            .edges()
            .map(|(a, b)| {
                let ((ax, ay), (bx, by)) = (a.xy(), b.xy());
                ax * by - ay * bx
            })
            .sum::<f64>()
    }

    /// Even-odd containment: whether a ray from `p` along +x crosses the ring an odd number of
    /// times.
    pub fn contains(&self, p: P) -> bool {
        let (px, py) = p.xy();
        let mut inside = false;
        for (a, b) in self.edges() {
            let ((ax, ay), (bx, by)) = (a.xy(), b.xy());
            if (ay > py) != (by > py) && px < ax + (py - ay) / (by - ay) * (bx - ax) {
                inside = !inside;
            }
        }
        inside
    }
}

impl Ring<Vec3> {
    /// The ring's vector area (Newell's method): its direction the normal the ring turns about,
    /// its length the area it encloses when flat; `area_vector().dot(n)` is the area seen along
    /// the unit vector `n`, positive when the ring runs counter-clockwise about it.
    pub fn area_vector(&self) -> Vec3 {
        self.edges().fold(Vec3::ZERO, |s, (a, b)| s + a.cross(*b)) * 0.5
    }
}

/// [`Ring::edges`] of items held elsewhere, for a type that keeps a closed ring in a field of its
/// own ([`solidcore_record::Loop2::joints`]).
pub(crate) fn around<T>(items: &[T]) -> impl Iterator<Item = (&T, &T)> + Clone {
    items.iter().zip(items.iter().cycle().skip(1))
}

impl<T, E> Deref for Chain<T, E> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.items
    }
}

impl<T, E> IntoIterator for Chain<T, E> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> std::vec::IntoIter<T> {
        self.items.into_iter()
    }
}

impl<'a, T, E> IntoIterator for &'a Chain<T, E> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> std::slice::Iter<'a, T> {
        self.items.iter()
    }
}

impl<T, E: Ends> From<Vec<T>> for Chain<T, E> {
    fn from(items: Vec<T>) -> Chain<T, E> {
        Chain::new(items)
    }
}

impl<T, E: Ends, const N: usize> From<[T; N]> for Chain<T, E> {
    fn from(items: [T; N]) -> Chain<T, E> {
        Chain::new(items.into())
    }
}

impl<T, E: Ends> FromIterator<T> for Chain<T, E> {
    fn from_iter<I: IntoIterator<Item = T>>(items: I) -> Chain<T, E> {
        Chain::new(items.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ring_closes_and_a_path_does_not() {
        assert_eq!(Ring::from([1, 2, 3]).edges().collect::<Vec<_>>(), vec![(&1, &2), (&2, &3), (&3, &1)]);
        assert_eq!(Path::from([1, 2, 3]).edges().collect::<Vec<_>>(), vec![(&1, &2), (&2, &3)]);
        assert_eq!(Path::from([1, 2, 3]).close().edges().count(), 3);
        assert_eq!(Ring::from([7]).edges().collect::<Vec<_>>(), vec![(&7, &7)]);
        assert_eq!(Path::from([7]).edges().count(), 0);
        assert_eq!(Ring::<u8>::default().edges().count(), 0);
        assert_eq!(Path::<u8>::default().edges().count(), 0);
        assert_eq!(Ring::from([1, 2, 3]).corners().collect::<Vec<_>>(), vec![(&3, &1, &2), (&1, &2, &3), (&2, &3, &1)]);
        assert_eq!(Ring::<u8>::default().corners().count(), 0);
    }

    #[test]
    fn a_square_encloses_its_area_and_its_inside() {
        let square = Ring::from([Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0), Vec2::new(2.0, 2.0), Vec2::new(0.0, 2.0)]);
        assert_eq!(square.signed_area(), 4.0);
        assert_eq!(square.iter().rev().copied().collect::<Ring<_>>().signed_area(), -4.0);
        assert!(square.contains(Vec2::new(1.0, 1.0)));
        assert!(!square.contains(Vec2::new(3.0, 1.0)));
        let uv = Ring::from([[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]]);
        assert_eq!(uv.signed_area(), 4.0);
        let tilted = Ring::from([Vec3::new(0.0, 0.0, 1.0), Vec3::new(2.0, 0.0, 1.0), Vec3::new(2.0, 2.0, 1.0), Vec3::new(0.0, 2.0, 1.0)]);
        assert_eq!(tilted.area_vector(), Vec3::new(0.0, 0.0, 4.0));
        assert!(uv.contains([1.0, 1.0]) && !uv.contains([3.0, 1.0]));
        assert_eq!(square.bounds(), Aabb2 { min: Vec2::new(0.0, 0.0), max: Vec2::new(2.0, 2.0) });
        assert!(Path::<Vec2>::default().bounds().is_empty());
    }
}

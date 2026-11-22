//! A closed chain of items: the last joins the first (a polygon's corners in a face's (u, v)).

use std::ops::Deref;

/// Items in order, the last joined back to the first; as a slice it is the items once each.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Ring<T> {
    items: Vec<T>,
}

impl<T> Ring<T> {
    pub(crate) fn new(items: Vec<T>) -> Ring<T> {
        Ring { items }
    }

    /// Each item with the next, as many edges as items (the last with the first; a lone item
    /// pairs with itself).
    pub(crate) fn edges(&self) -> impl Iterator<Item = (&T, &T)> + Clone {
        self.items
            .iter()
            .zip(self.items.iter().cycle().skip(1))
            .take(self.items.len())
    }
}

impl<T> Ring<T> {
    /// Each item with the one before and the one after it: `(previous, item, next)`, once per
    /// item.
    pub(crate) fn corners(&self) -> impl Iterator<Item = (&T, &T, &T)> + Clone {
        let before = self
            .items
            .iter()
            .cycle()
            .skip(self.items.len().saturating_sub(1));
        before.zip(self.edges()).map(|(p, (c, x))| (p, c, x))
    }
}

impl Ring<[f64; 2]> {
    /// The area the ring encloses, positive when it runs counter-clockwise.
    pub(crate) fn signed_area(&self) -> f64 {
        0.5 * self
            .edges()
            .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
            .sum::<f64>()
    }
}

impl<T> Deref for Ring<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.items
    }
}

impl<T> FromIterator<T> for Ring<T> {
    fn from_iter<I: IntoIterator<Item = T>>(items: I) -> Ring<T> {
        Ring::new(items.into_iter().collect())
    }
}

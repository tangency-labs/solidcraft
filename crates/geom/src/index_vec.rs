//! A `Vec` indexed by a typed index, so an index into one list can't be used on another.

use std::fmt;
use std::marker::PhantomData;
use std::ops::{Index, IndexMut};

use serde::{Deserialize, Serialize};

/// A typed index into an [`IndexVec`].
pub trait Idx: Copy {
    fn new(i: usize) -> Self;
    fn index(self) -> usize;
}

/// A `Vec<T>` indexed by `I`. Saved as the bare list.
#[derive(Serialize, Deserialize)]
#[serde(transparent, bound(serialize = "T: Serialize", deserialize = "T: Deserialize<'de>"))]
pub struct IndexVec<I, T> {
    items: Vec<T>,
    #[serde(skip)]
    marker: PhantomData<fn(I)>,
}

impl<I: Idx, T> IndexVec<I, T> {
    pub fn new() -> Self {
        IndexVec { items: Vec::new(), marker: PhantomData }
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn get(&self, i: I) -> Option<&T> {
        self.items.get(i.index())
    }
    pub fn get_mut(&mut self, i: I) -> Option<&mut T> {
        self.items.get_mut(i.index())
    }
    /// Appends `t` and returns its index.
    pub fn push(&mut self, t: T) -> I {
        let i = I::new(self.items.len());
        self.items.push(t);
        i
    }
    pub fn pop(&mut self) -> Option<T> {
        self.items.pop()
    }
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.items.iter()
    }
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.items.iter_mut()
    }
    /// Every index, in order.
    pub fn indices(&self) -> impl DoubleEndedIterator<Item = I> + ExactSizeIterator + Clone + use<I, T> {
        (0..self.items.len()).map(I::new)
    }
    pub fn iter_enumerated(&self) -> impl DoubleEndedIterator<Item = (I, &T)> + ExactSizeIterator + Clone {
        self.items.iter().enumerate().map(|(i, t)| (I::new(i), t))
    }
    pub fn iter_enumerated_mut(&mut self) -> impl DoubleEndedIterator<Item = (I, &mut T)> + ExactSizeIterator {
        self.items.iter_mut().enumerate().map(|(i, t)| (I::new(i), t))
    }
    pub fn first(&self) -> Option<&T> {
        self.items.first()
    }
    pub fn first_mut(&mut self) -> Option<&mut T> {
        self.items.first_mut()
    }
    pub fn last(&self) -> Option<&T> {
        self.items.last()
    }
    /// The items as a plain slice, for the rare untyped use.
    pub fn as_slice(&self) -> &[T] {
        &self.items
    }
    pub fn retain(&mut self, f: impl FnMut(&T) -> bool) {
        self.items.retain(f);
    }
    pub fn truncate(&mut self, len: usize) {
        self.items.truncate(len);
    }
}

impl<I, T> Extend<T> for IndexVec<I, T> {
    fn extend<It: IntoIterator<Item = T>>(&mut self, iter: It) {
        self.items.extend(iter);
    }
}

impl<I, T> Default for IndexVec<I, T> {
    fn default() -> Self {
        IndexVec { items: Vec::new(), marker: PhantomData }
    }
}

impl<I, T: Clone> Clone for IndexVec<I, T> {
    fn clone(&self) -> Self {
        IndexVec { items: self.items.clone(), marker: PhantomData }
    }
}

impl<I, T: fmt::Debug> fmt::Debug for IndexVec<I, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.items.fmt(f)
    }
}

impl<I, T: PartialEq> PartialEq for IndexVec<I, T> {
    fn eq(&self, other: &Self) -> bool {
        self.items == other.items
    }
}

impl<I, T> From<Vec<T>> for IndexVec<I, T> {
    fn from(items: Vec<T>) -> Self {
        IndexVec { items, marker: PhantomData }
    }
}

impl<I, T> FromIterator<T> for IndexVec<I, T> {
    fn from_iter<It: IntoIterator<Item = T>>(iter: It) -> Self {
        IndexVec { items: iter.into_iter().collect(), marker: PhantomData }
    }
}

impl<I, T> IntoIterator for IndexVec<I, T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a, I, T> IntoIterator for &'a IndexVec<I, T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<'a, I, T> IntoIterator for &'a mut IndexVec<I, T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.iter_mut()
    }
}

/// Panics out of range, like `Vec`; non-test code uses [`IndexVec::get`].
impl<I: Idx, T> Index<I> for IndexVec<I, T> {
    type Output = T;
    fn index(&self, i: I) -> &T {
        &self.items[i.index()]
    }
}

impl<I: Idx, T> IndexMut<I> for IndexVec<I, T> {
    fn index_mut(&mut self, i: I) -> &mut T {
        &mut self.items[i.index()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Ix(usize);

    impl Idx for Ix {
        fn new(i: usize) -> Self {
            Ix(i)
        }
        fn index(self) -> usize {
            self.0
        }
    }

    #[test]
    fn push_returns_the_index_and_saves_as_a_list() {
        let mut v: IndexVec<Ix, &str> = IndexVec::new();
        assert_eq!(v.push("a"), Ix(0));
        assert_eq!(v.push("b"), Ix(1));
        assert_eq!(v.get(Ix(1)), Some(&"b"));
        assert_eq!(v.get(Ix(2)), None);
        assert_eq!(v.indices().collect::<Vec<_>>(), [Ix(0), Ix(1)]);
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, r#"["a","b"]"#);
        let back: IndexVec<Ix, String> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.as_slice(), ["a", "b"]);
    }
}

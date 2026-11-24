//! A drawing's list of one kind of record: each behind its own `Arc`, so an undo step shares what it
//! didn't change, and by id, whatever order they came in. It holds what every such list does, so a
//! kind's own file holds only its own rules.

use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::require;
use crate::{DocError, Result, ids};

/// A record a drawing lists by its id.
pub trait Record {
    fn id(&self) -> u64;
}

/// The records of one kind, by id. It reads as the `Vec` it is, and a file holds them as one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ById<T>(Vec<Arc<T>>);

impl<T> Default for ById<T> {
    fn default() -> Self {
        ById(Vec::new())
    }
}

impl<T> Deref for ById<T> {
    type Target = Vec<Arc<T>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for ById<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<'a, T> IntoIterator for &'a ById<T> {
    type Item = &'a Arc<T>;
    type IntoIter = std::slice::Iter<'a, Arc<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<T> ById<T> {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<T: Record> ById<T> {
    /// Adds `more`, each where its id puts it.
    pub fn add(&mut self, more: impl IntoIterator<Item = Arc<T>>) {
        self.0.extend(more);
        self.put_in_order();
    }

    /// Puts what a file listed in another order where each id puts it.
    pub(super) fn put_in_order(&mut self) {
        self.0.sort_by_key(|record| record.id());
    }

    /// Where record `id` is in the list, found by halves.
    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.0.binary_search_by_key(&id, |record| record.id()).ok()
    }

    /// Record `id`.
    pub fn by_id(&self, id: u64) -> Option<&T> {
        self.index_of(id).and_then(|at| self.0.get(at)).map(|record| &**record)
    }

    /// The list's rules, each named by `word`: at most `most` records, by id, each keeping `rule`.
    pub(super) fn check(&self, most: usize, word: &str, mut rule: impl FnMut(&T) -> Result<()>) -> Result<()> {
        require(self.0.len() <= most, || format!("a drawing has at most {most} {word}s"))?;
        require(self.0.is_sorted_by_key(|record| record.id()), || format!("a drawing lists its {word}s by id"))?;
        self.0.iter().try_for_each(|record| rule(record).map_err(|e| DocError::Invalid(format!("{word} {}: {e}", ids::encode(record.id())))))
    }
}

/// A sheet or sheet set is named by id first, else by a name only one item has.
pub(super) fn named_index<T>(items: &[T], named: &str, word: &str, id: impl Fn(&T) -> u64, name: impl Fn(&T) -> &str) -> Result<usize> {
    if let Some(i) = ids::decode(named).and_then(|wanted| items.iter().position(|item| id(item) == wanted)) {
        return Ok(i);
    }
    let mut called = items.iter().enumerate().filter(|(_, item)| name(item) == named).map(|(i, _)| i);
    match (called.next(), called.next()) {
        (Some(i), None) => Ok(i),
        (Some(_), Some(_)) => Err(DocError::Invalid(format!("more than one {word} is called `{named}`: give its id"))),
        (None, _) => Err(DocError::Invalid(format!("no {word} `{named}`"))),
    }
}

#[cfg(test)]
#[path = "by_id_tests.rs"]
mod tests;

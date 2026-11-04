// SPDX-License-Identifier: Apache-2.0
//! Borrowed parallel wire columns over stored record rows.

use serde::Serialize;

pub(crate) struct SliceColumn<'a, T, U> {
    items: &'a [T],
    value: fn(&'a T) -> U,
}

impl<'a, T, U> SliceColumn<'a, T, U> {
    pub(crate) fn new(items: &'a [T], value: fn(&'a T) -> U) -> Self {
        Self { items, value }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl<T, U: Serialize> Serialize for SliceColumn<'_, T, U> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.items.iter().map(self.value))
    }
}

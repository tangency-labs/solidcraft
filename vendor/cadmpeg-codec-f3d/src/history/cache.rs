// SPDX-License-Identifier: Apache-2.0
//! Reuse indexes over immutable inputs within one projection pass.

use std::collections::HashMap;
use std::marker::PhantomData;

pub(super) struct SnapshotCache<'a, T, I> {
    entries: HashMap<*const T, I>,
    inputs: PhantomData<&'a T>,
}

impl<T, I> Default for SnapshotCache<'_, T, I> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            inputs: PhantomData,
        }
    }
}

impl<'a, T, I> SnapshotCache<'a, T, I> {
    pub(super) fn get(&mut self, input: &'a T, build: impl FnOnce(&'a T) -> I) -> &I {
        self.entries
            .entry(std::ptr::from_ref(input))
            .or_insert_with(|| build(input))
    }
}

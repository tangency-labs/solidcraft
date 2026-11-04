// SPDX-License-Identifier: Apache-2.0
//! Encode source-less F3D archives and apply supported edits to retained source
//! archives.

pub(crate) mod generate;
pub(crate) mod patch;
pub(crate) mod primitives;
pub(crate) mod target;

#[cfg(test)]
mod tests;

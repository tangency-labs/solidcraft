// SPDX-License-Identifier: Apache-2.0
//! Select graph rows in source order through admitted identity indexes.

use cadmpeg_ir::attributes::AttributeTarget;
use std::collections::HashSet;

pub(super) fn select_rows<T>(
    rows: Vec<T>,
    reachable: &HashSet<String>,
    owner: impl Fn(&T) -> &str,
) -> Vec<T> {
    let mut retained = Vec::new();
    for row in rows {
        if reachable.contains(owner(&row)) {
            retained.push(row);
        }
    }
    retained
}

fn target_selected(target: &AttributeTarget, reachable: &HashSet<String>) -> bool {
    let id = match target {
        AttributeTarget::Document => return true,
        AttributeTarget::Body(id) => id.as_str(),
        AttributeTarget::Face(id) => id.as_str(),
        AttributeTarget::Shell(id) => id.as_str(),
        AttributeTarget::Loop(id) => id.as_str(),
        AttributeTarget::Coedge(id) => id.as_str(),
        AttributeTarget::Edge(id) => id.as_str(),
        AttributeTarget::Vertex(id) => id.as_str(),
    };
    reachable.contains(id)
}

pub(super) fn select_links<T>(
    rows: Vec<T>,
    reachable: &HashSet<String>,
    target: impl Fn(&T) -> &AttributeTarget,
) -> Vec<T> {
    let mut retained = Vec::new();
    for row in rows {
        if target_selected(target(&row), reachable) {
            retained.push(row);
        }
    }
    retained
}

#[cfg(test)]
mod tests;

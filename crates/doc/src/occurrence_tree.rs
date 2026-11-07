//! The occurrence tree of a document: each component's child occurrences, gathered in one pass,
//! and the walk of every placement from the root over them. Walking it costs the placements,
//! where scanning every occurrence for each component visited cost placements times occurrences.

use std::collections::HashMap;

use crate::{Document, IDENTITY, MAX_PLACEMENT_DEPTH, MAX_PLACEMENTS, Mat, Occurrence, mat_mul};

/// Each component's child occurrences, in document order, as a document held them when made.
pub(crate) struct OccurrenceTree<'a> {
    children: HashMap<u64, Vec<&'a Occurrence>>,
}

impl<'a> OccurrenceTree<'a> {
    pub(crate) fn new(doc: &'a Document) -> Self {
        let mut children: HashMap<u64, Vec<&Occurrence>> = HashMap::new();
        for o in &doc.occurrences {
            children.entry(o.parent).or_default().push(o);
        }
        OccurrenceTree { children }
    }

    /// The occurrences placed in `component`, in document order.
    pub(crate) fn children(&self, component: u64) -> &[&'a Occurrence] {
        self.children.get(&component).map_or(&[], Vec::as_slice)
    }

    /// Every placement from the root: (component, occurrence path ids, world transform), with
    /// each occurrence placed in its parent by `transform`. Depth first, children in document
    /// order. A branch stops past [`MAX_PLACEMENT_DEPTH`] levels, the walk past [`MAX_PLACEMENTS`]
    /// placements.
    pub(crate) fn placements_with(&self, transform: impl Fn(&Occurrence) -> Mat) -> Vec<(u64, Vec<u64>, Mat)> {
        let mut out = vec![(0, Vec::new(), IDENTITY)];
        let mut stack: Vec<(u64, Vec<u64>, Mat)> = vec![(0, Vec::new(), IDENTITY)];
        while let Some((comp, path, m)) = stack.pop() {
            if out.len() > MAX_PLACEMENTS {
                break;
            }
            // Only this branch stops past the depth limit; the others are still walked.
            if path.len() > MAX_PLACEMENT_DEPTH {
                continue;
            }
            for o in self.children(comp) {
                let mut p = path.clone();
                p.push(o.id);
                let w = mat_mul(&m, &transform(o));
                out.push((o.component, p.clone(), w));
                stack.push((o.component, p, w));
            }
        }
        out
    }
}

#[cfg(test)]
#[path = "occurrence_tree_tests.rs"]
mod tests;

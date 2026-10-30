//! The STEP imports a model has read, kept by the SHA-256 of their text, so a design that is
//! evaluated again need not read its STEP files again (#9456).
//!
//! Reading a STEP file (converting each of its solids) takes seconds to minutes and depends only
//! on the text and the importer. A design opened by another release than the one that saved it
//! evaluates its timeline again, since the timeline's code changed; its imports were read by the
//! same importer more often than not, and the design file keeps them for that case
//! ([`ImportStore`], read by `solidcraft-io` only when the importer is the one that wrote them).
//! The Import feature still turns the import into bodies as the running release does.
use crate::kernel::{self, StepImport};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Imports a design file keeps, read from the file only when one is asked for.
pub trait ImportStore: Send + Sync + std::fmt::Debug {
    /// Whether it keeps the import of the text with this digest.
    fn has(&self, digest: &str) -> bool;
    /// The import of the text with this digest; `None` when it keeps none or cannot read it.
    fn import(&self, digest: &str) -> Option<StepImport>;
    /// For the writer that stored it: copies what it keeps without reading it.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// The imports a save keeps: those of the texts the design imports, in the order it imports
/// them, each read in this session (`read`) or still in the file it was opened from (`stored`).
#[derive(Clone, Debug, Default)]
pub struct SavedImports {
    /// Every imported text's digest, once, in the design's order.
    pub digests: Vec<String>,
    /// The imports read in this session, by digest.
    pub read: HashMap<String, Arc<StepImport>>,
    /// The imports of the file the design was opened from.
    pub stored: Option<Arc<dyn ImportStore>>,
}

impl SavedImports {
    pub fn is_empty(&self) -> bool {
        self.digests.is_empty()
    }
}

/// A model's imports: shared by its copies, as its evaluation cache is.
#[derive(Debug, Default)]
pub struct ImportResults {
    read: Mutex<HashMap<String, Arc<StepImport>>>,
    stored: Mutex<Option<Arc<dyn ImportStore>>>,
    /// Imports taken from `stored` instead of being read again.
    reused: AtomicUsize,
}

impl ImportResults {
    /// The import of `step`, whose digest is `digest`: one this model has, else the one the
    /// design file kept, else the text read now. The model keeps it for its next save, so a
    /// feature evaluated again on the same text (after an edit before it) does not read the text
    /// again either.
    pub fn import(&self, digest: &str, step: &str) -> kernel::Result<Arc<StepImport>> {
        if let Some(imp) = self.read.lock().unwrap_or_else(|e| e.into_inner()).get(digest) {
            return Ok(imp.clone());
        }
        // Not held while reading: two components may read two files at once.
        let stored = self.stored.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let imp = match stored.and_then(|s| s.import(digest)) {
            Some(imp) => {
                self.reused.fetch_add(1, Ordering::Relaxed);
                Arc::new(imp)
            }
            None => kernel::step_import_shared(step)?,
        };
        self.read.lock().unwrap_or_else(|e| e.into_inner()).insert(digest.to_string(), imp.clone());
        Ok(imp)
    }

    /// Take the imports a design file kept (opening it).
    pub fn take(&self, saved: &SavedImports) {
        *self.stored.lock().unwrap_or_else(|e| e.into_inner()) = saved.stored.clone();
        let mut read = self.read.lock().unwrap_or_else(|e| e.into_inner());
        for (digest, imp) in &saved.read {
            read.entry(digest.clone()).or_insert_with(|| imp.clone());
        }
    }

    /// What a save keeps of the imports of texts with these digests (the texts the design
    /// imports): the imports of other texts (a feature deleted since) are dropped.
    pub(crate) fn saved(&self, digests: Vec<String>) -> SavedImports {
        let wanted: HashSet<&str> = digests.iter().map(String::as_str).collect();
        let mut read = self.read.lock().unwrap_or_else(|e| e.into_inner());
        read.retain(|d, _| wanted.contains(d.as_str()));
        let read = read.clone();
        let stored = self.stored.lock().unwrap_or_else(|e| e.into_inner()).clone();
        SavedImports { digests, read, stored }
    }

    /// Imports taken from the design file instead of being read again, so far.
    pub fn reused(&self) -> usize {
        self.reused.load(Ordering::Relaxed)
    }
}

//! Opening a design a feature at a time, for a caller that must keep doing other work while a
//! large design is read: the web app has one thread, which has to keep drawing frames (and offer
//! Cancel) meanwhile (#10198). [`OpenSteps::start`] reads the file, [`OpenSteps::step`] evaluates
//! the next feature of its timeline, and [`OpenSteps::finish`] puts the design in the session's
//! place as `doc.open` does. The result is the same design, evaluated the same way: each step
//! moves a copy's history marker one feature on, so the evaluation reuses every result before it.

use std::sync::Arc;

use serde_json::Value;
use solidcraft_doc::{Document, FeatureKind, Model, Visibility};

use super::{opened_design, read_design};
use crate::params::bool_;
use crate::{Result, Session};

/// A design file being opened ([`OpenSteps::start`]).
pub struct OpenSteps {
    path: String,
    read_only: bool,
    disk: Option<solidcraft_io::design_lock::FileStamp>,
    /// The design as the session will hold it, its history marker moved to `next` meanwhile.
    doc: Arc<Document>,
    /// The history marker the design was saved with.
    marker: Option<usize>,
    visibility: Visibility,
    model: Model,
    /// Features evaluated so far, and where evaluation stops (the saved marker, or the end).
    next: usize,
    stop: usize,
    /// The model came from the file and needs no evaluation.
    adopted: bool,
}

impl OpenSteps {
    /// Read the design file `doc.open` would open with parameters `p`, without evaluating it.
    /// `None` when `p` names a file `doc.open` reads in another way (a CAD, mesh or drawing file,
    /// or a point cloud), or asks for something only those take: run `doc.open` for it.
    pub fn start(p: &Value) -> Result<Option<OpenSteps>> {
        let Some(path) = p.get("path").and_then(Value::as_str) else { return Ok(None) };
        let other = solidcraft_io::is_step_path(path)
            || solidcraft_io::is_mesh_path(path)
            || solidcraft_io::is_point_cloud_path(path)
            || super::super::drawing_import_command(path).is_some()
            || ["names", "up"].iter().any(|k| p.get(k).is_some_and(|v| !v.is_null()));
        if other || path.trim().is_empty() || path.len() >= 4096 {
            return Ok(None);
        }
        let read_only = bool_(p, "read_only").unwrap_or(false);
        // Stamped before it is read, as `doc.open` does.
        let disk = solidcraft_io::design_lock::stamp(path);
        let (mut doc, saved) = read_design(&mut Session::default(), path)?;
        super::super::component_link::resolve_design_links(&mut doc, Some(path));
        // What `Session::opened` does before it evaluates.
        solidcraft_io::vfs::install_checks();
        let visibility = std::mem::take(&mut doc.visibility);
        let doc = Arc::new(doc);
        let marker = doc.marker;
        let stop = marker.unwrap_or(usize::MAX).min(doc.features.len());
        // A model saved under another check mode holds that mode's results: evaluated again.
        let adopted = Session::saved_model(&doc, saved.as_ref()).filter(|model| model.validation() == solidcraft_kernel::Validation::default());
        let steps = OpenSteps {
            path: path.to_string(),
            read_only,
            disk,
            doc,
            marker,
            visibility,
            next: if adopted.is_some() { stop } else { 0 },
            stop,
            adopted: adopted.is_some(),
            model: adopted.unwrap_or_default(),
        };
        Ok(Some(steps))
    }

    /// The file being opened.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Features evaluated so far, and how many there are to evaluate.
    pub fn progress(&self) -> (usize, usize) {
        (self.next, self.stop)
    }

    /// Whether every feature has been evaluated ([`Self::finish`] is next).
    pub fn done(&self) -> bool {
        self.next >= self.stop
    }

    /// The STEP text the next feature imports, when it is an import that is not suppressed: a
    /// caller can read it in parts first (`kernel::StepReading`), and while it holds that
    /// reading's result the step finds the file read.
    pub fn next_import(&self) -> Option<Arc<str>> {
        let f = self.doc.features.get(self.next).filter(|_| !self.done())?;
        match &f.kind {
            FeatureKind::Import { step, .. } if !f.suppressed => Some(step.clone()),
            _ => None,
        }
    }

    /// Evaluate the next feature. False once there is none left.
    pub fn step(&mut self) -> bool {
        if self.done() {
            return false;
        }
        self.next += 1;
        Arc::make_mut(&mut self.doc).marker = Some(self.next);
        self.model.evaluate(&self.doc);
        !self.done()
    }

    /// Put the design in `s`'s place as `doc.open` does, and answer as it does. Features not
    /// yet evaluated are evaluated now.
    pub fn finish(mut self, s: &mut Session) -> Result<Value> {
        Arc::make_mut(&mut self.doc).marker = self.marker;
        if self.model.validation() != s.validation() {
            self.model.set_validation(s.validation());
            self.adopted = false;
        }
        if !self.adopted {
            // Every result is current: this takes them all and keeps the placement proof
            // `Session::opened` keeps.
            self.model.evaluate_shared(&self.doc);
        }
        let opened = Session::with_model(self.doc, self.visibility, self.model).located(Some(&self.path));
        opened_design(s, opened, &self.path, self.read_only, self.disk)
    }
}

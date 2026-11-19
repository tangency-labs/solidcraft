//! Captured target edits sent to the shell's single Form CAD worker.
use solidcraft_engine::forms_prototype::{FormTransformJob, PreparedFormTransform};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

pub type BrowserTransformOutcome = Result<Vec<u8>, String>;

/// The Form worker retains capacity until a cancelled computation replies or stops.
pub trait BrowserTransformHost {
    fn transform_busy(&self) -> bool;
    fn submit_transform(
        &self,
        request: Vec<u8>,
        cancelled: Rc<Cell<bool>>,
        finished: Sender<BrowserTransformOutcome>,
        ctx: &egui::Context,
    ) -> Result<(), String>;
}

pub(super) struct Pending {
    job: Option<FormTransformJob>,
    done: Receiver<BrowserTransformOutcome>,
    cancelled: Rc<Cell<bool>>,
}

impl Pending {
    pub(super) fn submit(job: FormTransformJob, host: &dyn BrowserTransformHost, ctx: &egui::Context) -> Result<Self, String> {
        if host.transform_busy() {
            return Err(solidcraft_ui_text::tr("forms.prototype.command_worker_busy").to_owned());
        }
        let request = job.browser_request().map_err(|error| error.to_string())?;
        let (finished, done) = std::sync::mpsc::channel();
        let cancelled = Rc::new(Cell::new(false));
        host.submit_transform(request, cancelled.clone(), finished, ctx)?;
        Ok(Self { job: Some(job), done, cancelled })
    }

    pub(super) fn poll(&mut self, current: bool) -> Option<Result<PreparedFormTransform, String>> {
        let result = match self.done.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(solidcraft_ui_text::tr("forms.prototype.preview_stopped").to_owned()),
        };
        // An obsolete sample only drains its slot. It never decodes or adopts
        // old authored points while the latest absolute target is waiting.
        if !current {
            return Some(Err("Form transform sample superseded".into()));
        }
        Some(result.and_then(|bytes| {
            self.job
                .take()
                .ok_or_else(|| "Form transform source is missing".to_owned())?
                .with_browser_reply(&bytes)
                .map_err(|error| error.to_string())
        }))
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.cancelled.set(true);
    }
}

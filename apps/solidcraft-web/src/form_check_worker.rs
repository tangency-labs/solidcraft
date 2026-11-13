//! One lazy dedicated Worker for the browser shell's Form checks and combined previews.
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::time::Duration;

use solidcraft_engine::forms_prototype::validation::BrowserCheck;
use solidcraft_ui_egui::forms_prototype::{BrowserCheckHost, BrowserCheckOutcome, BrowserPreview, BrowserPreviewHost, BrowserPreviewOutcome};
use solidcraft_web::{Kind, frame, frame_bytes, read};
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{Event, MessageEvent, Worker, WorkerOptions, WorkerType};

const TIMEOUT_MS: f64 = 60_000.0;
type Inbox = Rc<RefCell<Option<Result<Vec<u8>, &'static str>>>>;

struct Port {
    worker: Worker,
    inbox: Inbox,
    _receive: Closure<dyn FnMut(MessageEvent)>,
    _failure: Closure<dyn FnMut(Event)>,
}

fn now() -> Result<f64, String> {
    web_sys::window().and_then(|w| w.performance()).map(|p| p.now()).filter(|t| t.is_finite()).ok_or("browser worker clock unavailable".into())
}

impl Port {
    fn open(ctx: &egui::Context) -> Result<Self, String> {
        let path = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.query_selector("meta[name='solidcraft-compute-worker']").ok().flatten())
            .and_then(|element| element.get_attribute("content"))
            .ok_or("Form CAD worker bundle unavailable")?;
        let digest = path.strip_prefix("./workers/").and_then(|p| p.strip_suffix("/solidcraft-compute-worker_loader.js"));
        if !digest.is_some_and(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))) {
            return Err("invalid Form CAD worker bundle identity".into());
        }
        let options = WorkerOptions::new();
        options.set_type(WorkerType::Module);
        let worker = Worker::new_with_options(&path, &options).map_err(|e| format!("could not start Form CAD worker: {e:?}"))?;
        let inbox: Inbox = Rc::default();
        let messages = inbox.clone();
        let repaint = ctx.clone();
        let receive = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            let mut slot = messages.borrow_mut();
            *slot = Some(if slot.is_some() { Err("Form CAD worker sent excess replies") } else { frame_bytes(event.data()) });
            repaint.request_repaint();
        });
        let errors = inbox.clone();
        let repaint = ctx.clone();
        let failure = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
            event.prevent_default();
            *errors.borrow_mut() = Some(Err("Form CAD worker could not start or continue"));
            repaint.request_repaint();
        });
        worker.set_onmessage(Some(receive.as_ref().unchecked_ref()));
        worker.set_onmessageerror(Some(failure.as_ref().unchecked_ref()));
        worker.set_onerror(Some(failure.as_ref().unchecked_ref()));
        Ok(Self { worker, inbox, _receive: receive, _failure: failure })
    }
}

impl Drop for Port {
    fn drop(&mut self) {
        self.worker.set_onmessage(None);
        self.worker.set_onmessageerror(None);
        self.worker.set_onerror(None);
        self.worker.terminate();
    }
}

struct Job {
    id: u64,
    /// A job with no bytes is running. It keeps the slot until the reply or
    /// termination even if the editor cancels and drops its receiver.
    request: Option<Vec<u8>>,
    cancelled: Rc<Cell<bool>>,
    done: Finished,
}

enum Finished {
    Check(Sender<BrowserCheckOutcome>),
    Preview(Sender<BrowserPreviewOutcome>),
}

impl Finished {
    fn response(&self) -> Kind {
        match self {
            Self::Check(_) => Kind::Checked,
            Self::Preview(_) => Kind::Previewed,
        }
    }

    fn finish(self, result: Result<&[u8], String>) {
        match self {
            Self::Check(done) => {
                let _ = done.send(result.and_then(BrowserCheck::decode_reply));
            }
            Self::Preview(done) => {
                let _ = done.send(result.and_then(BrowserPreview::decode_reply));
            }
        }
    }
}

#[derive(Default)]
struct State {
    port: Option<Port>,
    ready: bool,
    deadline: Option<f64>,
    job: Option<Job>,
    last_id: u64,
}

#[derive(Default)]
pub(super) struct FormCheckWorker(RefCell<State>);

impl BrowserCheckHost for FormCheckWorker {
    fn busy(&self) -> bool {
        self.0.borrow().job.is_some()
    }

    fn submit(&self, request: Vec<u8>, cancelled: Rc<Cell<bool>>, done: Sender<BrowserCheckOutcome>, ctx: &egui::Context) -> Result<(), String> {
        self.submit_job(Kind::Check, request, cancelled, Finished::Check(done), ctx)
    }
}

impl BrowserPreviewHost for FormCheckWorker {
    fn preview_busy(&self) -> bool {
        self.busy()
    }

    fn submit_preview(
        &self,
        request: Vec<u8>,
        cancelled: Rc<Cell<bool>>,
        done: Sender<BrowserPreviewOutcome>,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        self.submit_job(Kind::Preview, request, cancelled, Finished::Preview(done), ctx)
    }
}

impl FormCheckWorker {
    fn submit_job(&self, kind: Kind, request: Vec<u8>, cancelled: Rc<Cell<bool>>, done: Finished, ctx: &egui::Context) -> Result<(), String> {
        let mut state = self.0.borrow_mut();
        if state.job.is_some() {
            return Err("another Form CAD computation is running".into());
        }
        let id = state.last_id.checked_add(1).ok_or("Form CAD worker job ID exhausted")?;
        let request = frame(kind, id, &request)?;
        if state.port.is_none() {
            let clock = now()?;
            state.port = Some(Port::open(ctx)?);
            state.deadline = Some(clock + TIMEOUT_MS);
        }
        state.last_id = id;
        state.job = Some(Job { id, request: Some(request), cancelled, done });
        ctx.request_repaint();
        Ok(())
    }

    pub(super) fn poll(&self, ctx: &egui::Context) {
        let mut state = self.0.borrow_mut();
        if state.port.is_none() {
            return;
        }
        if let Err(error) = state.progress() {
            state.port = None;
            state.ready = false;
            state.deadline = None;
            if let Some(job) = state.job.take()
                && !job.cancelled.get()
            {
                job.done.finish(Err(error));
            }
            ctx.request_repaint();
        }
        if state.deadline.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }
}

impl State {
    fn progress(&mut self) -> Result<(), String> {
        let clock = now()?;
        let port = self.port.as_ref().ok_or("Form CAD worker disconnected")?;
        let incoming = port.inbox.borrow_mut().take();
        if let Some(message) = incoming {
            let message = message?;
            match read(&message)? {
                (Kind::Ready, _, _) if !self.ready => self.ready = true,
                (kind @ (Kind::Checked | Kind::Previewed), id, payload) if self.ready => {
                    if !self.job.as_ref().is_some_and(|job| job.id == id && job.request.is_none() && job.done.response() == kind) {
                        return Err("unexpected Form CAD worker job reply".into());
                    }
                    let job = self.job.take().ok_or("Form CAD worker job missing")?;
                    if !job.cancelled.get() {
                        job.done.finish(Ok(payload));
                    }
                }
                _ => return Err("Form CAD worker rejected its request or protocol".into()),
            }
            self.deadline = None;
        }
        if self.deadline.is_some_and(|deadline| clock >= deadline) {
            return Err("Form CAD worker timed out".into());
        }
        if self.job.as_ref().is_some_and(|job| job.request.is_some() && job.cancelled.get()) {
            self.job = None;
        }
        if self.ready
            && let Some(request) = self.job.as_mut().and_then(|job| job.request.take())
        {
            port.worker
                .post_message(&js_sys::Uint8Array::from(request.as_slice()))
                .map_err(|e| format!("could not send Form CAD computation: {e:?}"))?;
            self.deadline = Some(clock + TIMEOUT_MS);
        }
        Ok(())
    }
}

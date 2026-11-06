//! Prepare the original and rollback models when transferring a sketch's history marker.

use solidcraft_engine::Session;

use crate::SolveApp;

pub(crate) struct PendingFeatureEdit {
    source: Session,
    done: std::sync::mpsc::Receiver<Result<(Session, Session), String>>,
    feature: Option<u64>,
    next_command: Option<String>,
    pub(crate) marker: Option<usize>,
    pub(crate) started: f64,
}

impl PendingFeatureEdit {
    pub(crate) fn cancel(&mut self) {
        self.feature = None;
        self.next_command = None;
    }

    pub(crate) fn then_start(&mut self, command: &str) {
        self.cancel();
        self.next_command = Some(command.to_owned());
    }

    pub(crate) fn view_changed(&mut self, before: u64, after: u64) {
        if self.source.revision == before {
            self.source.revision = after;
        }
    }

    fn result(&self) -> Option<Result<(Session, Session), String>> {
        match self.done.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err("feature editor preparation ended without a result".into())),
        }
    }
}

impl SolveApp {
    pub(crate) fn prepare_feature_edit(&mut self, feature: u64, index: usize, marker: Option<usize>) {
        let source = self.session.scratch();
        let mut original = source.scratch();
        let quality = solidcraft_engine::doc::display_quality::DisplayQuality::parse(&self.preferences.display_quality).unwrap_or_default();
        let compute = move || {
            let params = marker.map(|position| serde_json::json!({"position": position})).unwrap_or_else(|| serde_json::json!({}));
            original.execute("timeline.roll_to", &params).map_err(|error| error.to_string())?;
            let mut rolled = original.scratch();
            rolled.execute("timeline.roll_to", &serde_json::json!({"position": index})).map_err(|error| error.to_string())?;
            original.world_state().map_err(|error| error.to_string())?.warm_display_at(quality, || true);
            rolled.world_state().map_err(|error| error.to_string())?.warm_display_at(quality, || true);
            Ok((original, rolled))
        };
        let (tx, done) = std::sync::mpsc::channel();
        #[cfg(not(target_arch = "wasm32"))]
        if let Err(error) = std::thread::Builder::new().name("feature edit".into()).spawn(move || {
            let _ = tx.send(compute());
        }) {
            self.set_status(format!("could not prepare feature editor: {error}"), true);
            return;
        }
        #[cfg(target_arch = "wasm32")]
        let _ = tx.send(compute());
        self.pending_feature_edit =
            Some(PendingFeatureEdit { source, done, feature: Some(feature), next_command: None, marker, started: crate::now_ms() });
    }

    pub(crate) fn poll_feature_edit(&mut self, ctx: &egui::Context) {
        // A tab switch cancels opening the editor, but its original model still belongs
        // to that design. Finish its guarded restoration in its own inactive slot.
        for slot in self.docs.background_mut() {
            let Some(pending) = slot.pending_feature_edit.take() else { continue };
            match pending.result() {
                None => {
                    slot.pending_feature_edit = Some(pending);
                    ctx.request_repaint_after(std::time::Duration::from_millis(16));
                }
                Some(Ok((original, _))) => {
                    let _ = slot.session.restore_editor(&pending.source, original);
                }
                Some(Err(_)) => {}
            }
        }
        let Some(mut pending) = self.pending_feature_edit.take() else { return };
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.esc_handled = true;
            pending.cancel();
        }
        let Some(result) = pending.result() else {
            self.pending_feature_edit = Some(pending);
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
            return;
        };
        match result {
            Ok((original, rolled)) => {
                // A stale result is discarded, never restarted against a newer design.
                // Adoption preserves the live history stacks and current selection.
                let restore = if pending.feature.is_some() { rolled } else { original.scratch() };
                if let Err(error) = self.session.restore_editor(&pending.source, restore) {
                    self.set_status(error.to_string(), true);
                    return;
                }
                if let Some(feature) = pending.feature {
                    self.open_feature_edit(feature, pending.marker, original);
                } else if let Some(command) = pending.next_command {
                    self.start(&command);
                }
            }
            Err(error) => self.set_status(error, true),
        }
    }
}

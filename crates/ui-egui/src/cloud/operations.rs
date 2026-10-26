//! Button lifetimes follow command operation IDs, including work started in another tab.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use solidcraft_cloud::jobs::{JobStatus, Phase};

use crate::SolveApp;

#[derive(Default)]
pub(crate) struct Operations {
    running: BTreeMap<String, (String, String)>,
    cancelling: BTreeSet<String>,
}

impl Operations {
    pub(super) fn tracked(&self, operation: &str) -> bool {
        self.running.contains_key(operation)
    }

    pub(super) fn update(&mut self, statuses: &[JobStatus]) {
        let active: BTreeSet<_> =
            statuses.iter().filter(|s| matches!(s.phase, Phase::Working | Phase::Syncing)).map(|s| s.operation.as_str()).collect();
        self.running.retain(|id, _| active.contains(id.as_str()));
        self.cancelling.retain(|id| active.contains(id.as_str()));
    }

    pub(crate) fn busy(&self, command: &str, connection: &str) -> bool {
        self.running.values().any(|(kind, target)| kind == command && target == connection)
    }

    pub(crate) fn cancel_requested(&mut self, operation: &str) {
        self.cancelling.insert(operation.into());
    }

    pub(crate) fn cancelling(&self, operation: &str) -> bool {
        self.cancelling.contains(operation)
    }
}

pub(crate) fn submitted(app: &mut SolveApp, command: &str, params: &Value, result: &Value) {
    // Editing a setup field does not test or acknowledge the saved connection's push.
    if command == "cloud.test" && params.get("metadata_only").and_then(Value::as_bool) == Some(true) {
        return;
    }
    if command == "cloud.cancel" {
        if let Some(id) = params.get("operation").and_then(Value::as_str) {
            app.cloud.operations.cancelling.insert(id.into());
        }
        return;
    }
    let Some(id) = result.get("operation").and_then(Value::as_str) else { return };
    let command = if command == "file.save" && app.session.cloud_binding().is_some() { "cloud.save" } else { command };
    if !matches!(command, "cloud.save" | "cloud.sync" | "cloud.test" | "cloud.rename" | "cloud.delete" | "cloud.restore" | "cloud.discard") {
        return;
    }
    // The worker returns its resolved connection. Polling status must never replace
    // the original action kind, and the current tab's binding need not be this target.
    let connection = result.get("connection").and_then(Value::as_str).unwrap_or_default();
    app.cloud.operations.running.insert(id.into(), (command.into(), connection.into()));
}

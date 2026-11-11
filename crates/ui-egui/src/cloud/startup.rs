//! One startup synchronization per repository. Backoff belongs to failed attempts only.
use super::*;
use solidcraft_cloud::{Connection, Settings};

#[derive(Default)]
pub(super) struct State {
    entries: BTreeMap<String, Attempt>,
}
struct Attempt {
    connection: Connection,
    operation: Option<String>,
    failures: u8,
    next: f64,
    complete: bool,
}
impl Attempt {
    fn new(connection: Connection) -> Self {
        Self { connection, operation: None, failures: 0, next: 0.0, complete: false }
    }
    fn failed(&mut self, now: f64) {
        self.operation = None;
        let delay = [2.0, 5.0, 15.0, 60.0, 300.0].get(usize::from(self.failures)).copied().unwrap_or(300.0);
        self.failures = self.failures.saturating_add(1);
        self.next = now + delay;
    }
}
impl State {
    fn requests(&mut self, settings: &Settings, statuses: &[JobStatus], now: f64) -> Vec<String> {
        self.entries.retain(|id, _| settings.connection(id).is_some());
        let mut ready = Vec::new();
        for connection in &settings.connections {
            let attempt = self.entries.entry(connection.id.clone()).or_insert_with(|| Attempt::new(connection.clone()));
            if !attempt.connection.same_repository(connection) {
                *attempt = Attempt::new(connection.clone());
            }
            if let Some(id) = &attempt.operation {
                match statuses.iter().find(|s| &s.operation == id).map(|s| s.phase) {
                    Some(Phase::Ready | Phase::Synced | Phase::Cancelled) => {
                        attempt.complete = true;
                        attempt.operation = None;
                    }
                    Some(Phase::Failed | Phase::SavedLocally) | None => attempt.failed(now),
                    _ => {}
                }
            }
            if settings.retry_pending
                && !attempt.complete
                && attempt.operation.is_none()
                && now >= attempt.next
                && !statuses.iter().any(|s| s.connection == connection.id && matches!(s.phase, Phase::Working | Phase::Syncing))
            {
                ready.push(connection.id.clone());
            }
        }
        ready
    }
}

pub(super) fn tick(app: &mut SolveApp, ctx: &egui::Context, statuses: &[JobStatus]) {
    if app.session.cloud_configuring() || app.session.cloud_save_pending() {
        return;
    }
    let now = ctx.input(|i| i.time);
    let now = if now.is_finite() { now.max(0.0) } else { 0.0 };
    let settings = app.session.cloud_settings().clone();
    let requests = app.cloud.startup.requests(&settings, statuses, now);
    for connection in requests {
        let result = app.run("cloud.sync", serde_json::json!({"connection":connection}));
        if let Some(attempt) = app.cloud.startup.entries.get_mut(&connection) {
            match result.ok().and_then(|value| value.get("operation").and_then(serde_json::Value::as_str).map(str::to_owned)) {
                Some(id) => attempt.operation = Some(id),
                None => attempt.failed(now),
            }
        }
    }
    if settings.retry_pending {
        let next = app.cloud.startup.entries.values().filter(|a| !a.complete && a.operation.is_none()).map(|a| a.next).min_by(f64::total_cmp);
        if let Some(next) = next {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64((next - now).clamp(0.1, 300.0)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_cloud::jobs::Job;
    #[test]
    fn successful_startup_never_polls_again_and_failed_attempts_back_off() {
        let connection = Connection::default();
        let settings = Settings { connections: vec![connection.clone()], ..Default::default() };
        let mut state = State::default();
        assert_eq!(state.requests(&settings, &[], 0.0).as_slice(), std::slice::from_ref(&connection.id));
        let job = Job::new(connection.id.clone()).unwrap();
        state.entries.get_mut(&connection.id).unwrap().operation = Some(job.status().unwrap().operation);
        job.fail(solidcraft_cloud::Error::Storage("offline".into())).unwrap();
        assert!(state.requests(&settings, &[job.status().unwrap()], 0.1).is_empty());
        assert!(state.requests(&settings, &[], 2.0).is_empty());
        assert_eq!(state.requests(&settings, &[], 2.1).as_slice(), std::slice::from_ref(&connection.id));
        let retry = Job::new(connection.id.clone()).unwrap();
        state.entries.get_mut(&connection.id).unwrap().operation = Some(retry.status().unwrap().operation);
        retry.update(|s| s.phase = Phase::Synced).unwrap();
        for now in [3.0, 60.0, 600.0, 86_400.0] {
            assert!(state.requests(&settings, &[retry.status().unwrap()], now).is_empty());
        }
        let mut changed = settings;
        changed.connections.push(Connection { id: "second".into(), credential: "second".into(), ..connection });
        assert_eq!(state.requests(&changed, &[], 86_401.0), ["second"]);
    }
}

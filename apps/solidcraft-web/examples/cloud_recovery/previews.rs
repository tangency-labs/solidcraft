//! Real engine Save and preview commands over the browser's durable adapter.
use crate::cloud_host::BrowserHost;
use serde_json::{Value, json};
use solidcraft_cloud::{
    Connection, Destination, Error, Result, Settings, Thumbnail,
    jobs::{Job, Operation, Phase},
};
use solidcraft_engine::Session;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
struct Worker {
    job: Mutex<Option<Job>>,
    captured: AtomicBool,
}
fn storage(error: impl std::fmt::Display) -> Error {
    Error::Storage(error.to_string())
}
fn require(value: bool, message: &str) -> Result<()> {
    if value { Ok(()) } else { Err(storage(message)) }
}
impl solidcraft_cloud::jobs::Host for Worker {
    fn submit(&self, operation: Operation, executable: String) -> Result<Job> {
        if let Operation::Save { save, .. } = &operation {
            self.captured.store(Thumbnail::read(&save.record.manifest)?.is_some(), Ordering::Relaxed);
        }
        let job = BrowserHost.submit(operation, executable)?;
        *self.job.lock().map_err(storage)? = Some(job.clone());
        Ok(job)
    }
}
fn session(identity: String) -> Result<(Session, Arc<Worker>, Destination)> {
    let connection = Connection { id: identity.clone(), repository: "fixture/previews".into(), ..Default::default() };
    let destination = Destination { connection: identity.clone(), project: "Robot".into(), file: "parts/preview".into() };
    let mut session = Session::new(solidcraft_engine::doc::Document::new("Browser preview bracket"));
    let settings = Settings {
        connections: vec![connection],
        default_connection: Some(identity),
        sync_after_save: false,
        retry_pending: false,
        ..Default::default()
    };
    session.execute("cloud.configure", &json!({"settings":settings})).map_err(storage)?;
    let worker = Arc::new(Worker::default());
    session.set_cloud_host(Some(worker.clone()));
    Ok((session, worker, destination))
}
fn submit(session: &mut Session, worker: &Worker, command: &str, params: Value) -> Result<Job> {
    let reply = session.execute(command, &params).map_err(storage)?;
    let job = worker.job.lock().map_err(storage)?.clone().ok_or_else(|| storage("preview operation missing"))?;
    require(reply.get("operation").and_then(Value::as_str) == Some(job.status()?.operation.as_str()), "registered command submitted the operation")?;
    Ok(job)
}
pub(super) async fn phase(name: &str, identity: String) -> Result<Value> {
    let (mut session, worker, destination) = session(identity)?;
    match name {
        "preview-write" => {
            // Exactly the picture renderer installed by the browser app.
            solidcraft_ui_egui::gpu::install_cpu_picture_renderer();
            session.execute("solid.box", &json!({"length":24,"width":16,"height":5})).map_err(storage)?;
            for body in &session.world_state().bodies {
                body.mesh();
            }
            let job = submit(&mut session, &worker, "cloud.save", json!({"destination":destination}))?;
            require(super::worker::complete(&job).await?.phase == Phase::SavedLocally, "browser Save persisted the preview")?;
            require(worker.captured.load(Ordering::Relaxed), "normal engine Save captured a PNG in WASM")?;
        }
        "preview-read" => {
            let job = submit(&mut session, &worker, "cloud.thumbnail", json!({"destination":destination}))?;
            require(super::worker::complete(&job).await?.phase == Phase::Ready, "preview survived browser restart")?;
            let reply = session.execute("cloud.thumbnail.finish", &json!({"operation":job.status()?.operation})).map_err(storage)?;
            let png = Thumbnail::decode_response(&reply)?.ok_or_else(|| storage("browser preview missing"))?;
            require(png.get(..8) == Some(b"\x89PNG\r\n\x1a\n"), "returned PNG bytes")?;
            require(job.status()?.result.is_none(), "transient image result was consumed")?;
            let opening = submit(&mut session, &worker, "cloud.open", json!({"destination":destination}))?;
            require(super::worker::complete(&opening).await?.phase == Phase::Ready, "design remains editable")?;
            session.execute("cloud.open.finish", &json!({"operation":opening.status()?.operation})).map_err(storage)?;
            require(session.doc.name == "Browser preview bracket", "preview belongs to the saved CAD document")?;
        }
        _ => return Err(storage("unknown preview fixture phase")),
    }
    Ok(json!({"phase":name}))
}

//! Opt-in authenticated interoperability acceptance, restricted to disposable branches.
use crate::{cloud_host::BrowserHost, cloud_settings::LocalSettings};
use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_cloud::{
    Connection, Destination, Error, Result, Settings,
    jobs::{Host, Job, Operation, Phase},
};
use solidcraft_engine::Session;
use std::sync::{Arc, Mutex};
use wasm_bindgen::prelude::*;

#[derive(Deserialize)]
struct Config {
    connection: Connection,
    token: Option<String>,
}
#[derive(Default)]
struct Worker(Mutex<Option<Job>>);
impl Host for Worker {
    fn submit(&self, operation: Operation, executable: String) -> Result<Job> {
        let job = BrowserHost.submit(operation, executable)?;
        *self.0.lock().map_err(storage)? = Some(job.clone());
        Ok(job)
    }
}
fn storage(error: impl std::fmt::Display) -> Error {
    Error::Storage(error.to_string())
}
fn require(condition: bool, message: &str) -> Result<()> {
    if condition { Ok(()) } else { Err(storage(message)) }
}
async fn command(session: &mut Session, worker: &Worker, name: &str, params: Value) -> Result<Job> {
    session.execute(name, &params).map_err(storage)?;
    let job = worker.0.lock().map_err(storage)?.clone().ok_or_else(|| storage("live operation missing"))?;
    let status = super::worker::complete_for(&job, 6000).await?;
    require(matches!(status.phase, Phase::Ready | Phase::Synced), status.message.as_deref().unwrap_or("live operation failed"))?;
    Ok(job)
}
async fn phase(name: &str, config: &str) -> Result<Value> {
    let mut config: Config = serde_json::from_str(config).map_err(|_| storage("invalid live-test configuration"))?;
    config.connection.validate()?;
    require(config.connection.branch.starts_with("cloud-acceptance-"), "live acceptance requires a disposable cloud-acceptance- branch")?;
    let destination = Destination { connection: config.connection.id.clone(), project: "Acceptance".into(), file: "parts/bracket".into() };
    let worker = Arc::new(Worker::default());
    let mut session = Session::default();
    session.set_cloud_host(Some(worker.clone()));
    match name {
        "online" => {
            let token = config.token.take().ok_or_else(|| storage("live-test credential missing"))?;
            // Test-only input, outside engine JSON, command recordings and repository files.
            LocalSettings::open().await?.store_credential(config.connection.credential.clone(), Some(token)).await?;
            let settings = Settings {
                connections: vec![config.connection.clone()],
                default_connection: Some(config.connection.id.clone()),
                retry_pending: false,
                ..Default::default()
            };
            command(&mut session, &worker, "cloud.configure", json!({"settings":settings})).await?;
            command(&mut session, &worker, "cloud.test", json!({"connection":config.connection.id})).await?;
        }
        "offline" => {
            require(config.token.is_none(), "restart must use persisted credentials")?;
            command(&mut session, &worker, "cloud.settings", json!({"reload":true})).await?;
            session.poll_cloud().map_err(storage)?;
            require(session.cloud_settings().connection(&config.connection.id) == Some(&config.connection), "connection survived browser restart")?;
            require(LocalSettings::open().await?.credential(&config.connection.credential).await?.is_some(), "credential survived browser restart")?;
        }
        _ => return Err(storage("unknown live-test phase")),
    }
    let opening = command(&mut session, &worker, "cloud.open", json!({"destination":destination})).await?;
    session.execute("cloud.open.finish", &json!({"operation":opening.status()?.operation})).map_err(storage)?;
    require(session.doc.name == "Acceptance bracket", "native design opened in browser")?;
    if name == "offline" {
        require(session.doc.features.len() == 2 && !session.is_dirty(), "published editable CAD survived offline browser restart")?;
        return Ok(json!({"phase":name,"features":session.doc.features.len()}));
    }
    require(session.doc.features.len() == 1, "native parametric feature restored")?;
    session.execute("solid.box", &json!({"length":8,"width":8,"height":12,"corner":[32,0,0]})).map_err(storage)?;
    solidcraft_ui_egui::gpu::install_cpu_picture_renderer();
    for body in &session.world_state().bodies {
        body.mesh();
    }
    let saving = command(&mut session, &worker, "cloud.save", json!({})).await?;
    let status = saving.status()?;
    require(status.phase == Phase::Synced, "GitHub accepted the browser save")?;
    let published = status.publications.last().ok_or_else(|| storage("publication confirmation missing"))?;
    require(!published.conflict && published.destination == destination, "browser advanced the native revision")?;
    Ok(json!({"phase":name,"published":published}))
}

#[wasm_bindgen]
pub async fn cloud_live_phase(name: String, config: String) -> std::result::Result<String, JsValue> {
    let value = phase(&name, &config).await.map_err(|e| JsValue::from(js_sys::Error::new(&e.to_string())))?;
    serde_json::to_string(&value).map_err(|_| JsValue::from_str("live-test result encoding failed"))
}

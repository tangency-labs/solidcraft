//! The actual browser worker, exercised before and after process termination.
use std::time::Duration;

use serde_json::{Value, json};
use solidcraft_cloud::{
    Connection, Destination, Error, PendingSave, Published, Result, Settings,
    jobs::{Host, Job, JobStatus, Operation, Phase},
};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_futures::JsFuture;

use crate::{cloud_host::BrowserHost, cloud_settings::LocalSettings, cloud_storage::Store};

thread_local! {
    static CANCELLING: std::cell::RefCell<Option<Job>> = const {std::cell::RefCell::new(None)};
}

fn require(condition: bool, message: &str) -> Result<()> {
    if condition { Ok(()) } else { Err(Error::Storage(format!("browser worker test: {message}"))) }
}

async fn complete(job: &Job) -> Result<JobStatus> {
    for _ in 0..1000 {
        let status = job.status()?;
        if !matches!(status.phase, Phase::Working | Phase::Syncing) {
            return Ok(status);
        }
        let timer = js_sys::Promise::new(&mut |resolve, reject| {
            if let Some(window) = web_sys::window() {
                if let Err(error) = window
                    .set_timeout_with_callback_and_timeout_and_arguments_0(resolve.unchecked_ref(), Duration::from_millis(10).as_millis() as i32)
                {
                    let _ = reject.call1(&wasm_bindgen::JsValue::UNDEFINED, &error);
                }
            } else {
                let _ = reject.call1(&wasm_bindgen::JsValue::UNDEFINED, &"window missing".into());
            }
        });
        JsFuture::from(timer).await.map_err(|_| Error::Storage("browser worker timer failed".into()))?;
    }
    Err(Error::Storage("browser worker timed out".into()))
}

async fn operation(operation: Operation) -> Result<JobStatus> {
    complete(&BrowserHost.submit(operation, String::new())?).await
}

pub async fn phase(name: &str, identity: String) -> Result<Value> {
    let connection =
        Connection { id: identity.clone(), repository: "fixture/offline".into(), credential: format!("cloud:{identity}"), ..Default::default() };
    let destination = Destination { connection: identity.clone(), project: "Robot".into(), file: "parts/bracket".into() };
    match name {
        "worker-future-document" => {
            let snapshot = solidcraft_cloud::Snapshot::prepare(
                br#"{"format":"solidcraft/4294967295"}"#.to_vec(),
                std::collections::BTreeMap::from([("blob".into(), vec![42; 100])]),
            )?;
            let capture = PendingSave::stage(&connection, &destination.project, &destination.file, None, snapshot)?;
            let object = capture
                .objects
                .keys()
                .find(|path| path.starts_with("objects/"))
                .cloned()
                .ok_or_else(|| Error::Storage("fixture object missing".into()))?;
            let store = Store::open(connection.clone()).await?;
            store.stage(capture).await?;
            // Remove a payload: schema recognition must win before attempting to read it.
            let root = crate::files::root().await.map_err(Error::Storage)?;
            let path = format!("cloud/{}/{object}", solidcraft_cloud::digest(connection.id.as_bytes()));
            let (parents, name) = path.rsplit_once('/').ok_or_else(|| Error::Storage("fixture path missing".into()))?;
            let mut directory = root;
            for part in parents.split('/') {
                directory = JsFuture::from(directory.get_directory_handle(part))
                    .await
                    .map_err(|_| Error::Storage("fixture directory missing".into()))?
                    .dyn_into()
                    .map_err(|_| Error::Storage("fixture directory invalid".into()))?;
            }
            JsFuture::from(directory.remove_entry(name)).await.map_err(|_| Error::Storage("fixture removal failed".into()))?;
            for request in [Operation::Open { connection: connection.clone(), destination }, Operation::Synchronize(connection)] {
                let status = operation(request).await?;
                require(
                    status.phase == Phase::Failed && status.message.as_deref().is_some_and(|message| message.contains("Upgrade SolidCraft")),
                    "resumed open and sync reject future schemas before asset reads or network writes",
                )?;
            }
            Ok(json!({"phase":name}))
        }
        "worker-cancel-start" => {
            let job = BrowserHost.submit(Operation::Synchronize(connection), String::new())?;
            let status = job.status()?;
            CANCELLING.with(|active| *active.borrow_mut() = Some(job));
            Ok(json!({"phase":name,"operation":status.operation}))
        }
        "worker-cancel-finish" => {
            let job = CANCELLING.with(|active| active.borrow_mut().take()).ok_or_else(|| Error::Storage("cancelling worker missing".into()))?;
            job.cancel()?;
            require(complete(&job).await?.phase == Phase::Cancelled, "sync cancellation is reported")?;
            require(Store::open(connection).await?.pending().await?.len() == 1, "cancellation keeps the captured save")?;
            Ok(json!({"phase":name}))
        }
        "worker-write" => {
            let other = Connection {
                id: format!("{identity}_other"),
                repository: "fixture/other".into(),
                credential: format!("cloud:{identity}_other"),
                ..Default::default()
            };
            LocalSettings::open().await?.store_credential(connection.credential.clone(), Some("fixture-primary-token".into())).await?;
            LocalSettings::open().await?.store_credential(other.credential.clone(), Some("fixture-secondary-token".into())).await?;
            let settings = Settings {
                connections: vec![connection.clone(), other],
                default_connection: Some(identity),
                sync_after_save: false,
                retry_pending: false,
                ..Default::default()
            };
            require(operation(Operation::Configure(settings.clone())).await?.phase == Phase::Synced, "configuration was durably stored")?;
            let snapshot = solidcraft_engine::io::cloud::prepare_document(&solidcraft_engine::doc::Document::new("Browser offline capture"))?;
            let save = PendingSave::stage(&connection, &destination.project, &destination.file, None, snapshot)?;
            let status = operation(Operation::Save { connection, save: Box::new(save), synchronize: false }).await?;
            require(status.phase == Phase::SavedLocally && status.saved_locally, "save acknowledged local durability")?;
            Ok(json!({"phase":name,"revision":status.revision,"persistent":status.storage_persistent}))
        }
        "worker-read" => {
            let loaded = operation(Operation::LoadSettings).await?;
            require(loaded.phase == Phase::Ready, "configuration reloaded")?;
            let settings: Settings = serde_json::from_value(loaded.result.ok_or_else(|| Error::Storage("settings missing".into()))?)
                .map_err(|e| Error::Storage(e.to_string()))?;
            require(
                settings.connections.len() == 2 && settings.default_connection.as_deref() == Some(identity.as_str()),
                "both connections and default survived restart",
            )?;
            let credentials = LocalSettings::open().await?;
            require(
                credentials.credential(&connection.credential).await?.as_deref() == Some("fixture-primary-token"),
                "first credential survived restart",
            )?;
            require(
                credentials.credential(&format!("cloud:{identity}_other")).await?.as_deref() == Some("fixture-secondary-token"),
                "credentials stayed independent",
            )?;
            let sync = operation(Operation::Synchronize(connection.clone())).await?;
            require(sync.phase == Phase::Failed, "network failure is reported")?;
            require(
                !serde_json::to_string(&sync).map_err(|e| Error::Storage(e.to_string()))?.contains("fixture-primary-token"),
                "credentials never enter status/errors",
            )?;
            require(Store::open(connection.clone()).await?.pending().await?.len() == 1, "failed sync retained the capture")?;
            open_and_list(connection, destination).await?;
            Ok(json!({"phase":name,"connections":settings.connections.len(),"offline_open":true}))
        }
        "worker-confirm" => {
            let store = Store::open(connection).await?;
            let saves = store.pending().await?;
            let save = saves.first().ok_or_else(|| Error::Storage("worker capture missing".into()))?;
            // Only the storage acknowledgement is stubbed, as in the existing fixture.
            store.confirm(save.clone(), Published { destination, revision: save.id.clone(), commit: "a".repeat(40), conflict: false }).await?;
            Ok(json!({"phase":name}))
        }
        "worker-cached-read" => {
            require(Store::open(connection.clone()).await?.pending().await?.is_empty(), "receipt survived restart")?;
            open_and_list(connection.clone(), destination).await?;
            require(operation(Operation::Synchronize(connection)).await?.phase == Phase::Synced, "empty queue needs no connectivity")?;
            Ok(json!({"phase":name,"cached_open":true}))
        }
        "worker-future-settings" => {
            let loaded = operation(Operation::LoadSettings).await?;
            require(
                loaded.phase == Phase::Failed && loaded.message.as_deref().is_some_and(|message| message.contains("Upgrade SolidCraft")),
                "future settings ask for upgrade",
            )?;
            let replacement = operation(Operation::Configure(Settings::default())).await?;
            require(
                replacement.phase == Phase::Failed && replacement.message.as_deref().is_some_and(|message| message.contains("Upgrade SolidCraft")),
                "future settings cannot be replaced",
            )?;
            Ok(json!({"phase":name}))
        }
        _ => Err(Error::Storage("unknown browser worker phase".into())),
    }
}

async fn open_and_list(connection: Connection, destination: Destination) -> Result<()> {
    let listed = operation(Operation::List(connection.clone())).await?;
    require(listed.phase == Phase::Ready && listed.result == Some(json!([destination])), "offline listing includes the cached design")?;
    let opening = BrowserHost.submit(Operation::Open { connection, destination: destination.clone() }, String::new())?;
    let status = complete(&opening).await?;
    require(status.phase == Phase::Ready && status.destination == Some(destination), "offline opening completed")?;
    let snapshot = opening.take_opened()?;
    let document = solidcraft_engine::io::cloud::open_document(&snapshot.manifest, &snapshot.document, |path| {
        snapshot.objects.get(path).cloned().ok_or_else(|| Error::Storage("missing worker asset".into()))
    })?;
    require(document.name == "Browser offline capture", "worker opened the captured CAD document")
}

use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_cloud::{Destination, PendingSave, Settings, jobs::Operation};

use super::CommandSpec;
use crate::{
    Result, Session,
    cloud::{Binding, Capture, error},
    params::bad,
};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cloud.configure", "Configure Cloud Saves", configure)
        .icon("settings")
        .noundo()
        .doc_only()
        .params("settings: local repository connections and defaults; contains credential identifiers only"),
    CommandSpec::new("cloud.settings", "Cloud Saves Settings", settings)
        .icon("settings")
        .noundo()
        .doc_only()
        .params("Returns local connection settings and current destination"),
    CommandSpec::new("cloud.test", "Test Repository Connection", test)
        .icon("inspect")
        .noundo()
        .doc_only()
        .params("connection?: connection id, defaults to selected destination; returns operation id"),
    CommandSpec::new("cloud.list", "Open Repository", list)
        .icon("open")
        .noundo()
        .doc_only()
        .params("connection?: connection id; returns operation id, then project design destinations"),
    CommandSpec::new("cloud.open", "Open Cloud Design", open)
        .icon("open")
        .noundo()
        .doc_only()
        .params("destination: {connection,project,file}; returns operation id; finish with cloud.open.finish"),
    CommandSpec::new("cloud.open.finish", "Finish Opening Cloud Design", finish)
        .icon("open")
        .noundo()
        .doc_only()
        .params("operation: completed cloud.open operation id; adopts the verified snapshot"),
    CommandSpec::new("cloud.save", "Save to Cloud Project", save)
        .icon("save")
        .noundo()
        .doc_only()
        .params("destination?: {connection,project,file}, otherwise current binding; returns durable-save operation id"),
    CommandSpec::new("cloud.sync", "Synchronize Repository", sync)
        .icon("send")
        .noundo()
        .doc_only()
        .params("connection?: connection id; retries durable captured saves; returns operation id"),
    CommandSpec::new("cloud.status", "Cloud Save Status", status)
        .icon("inspect")
        .noundo()
        .doc_only()
        .params("operation?: operation id; otherwise all retained operation statuses"),
    CommandSpec::new("cloud.cancel", "Cancel Cloud Operation", cancel)
        .icon("stop")
        .noundo()
        .doc_only()
        .params("operation: operation id; pending local saves are retained"),
];

fn settings(session: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!({"settings":session.cloud.settings,"binding":session.cloud.binding}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Configure {
    settings: Settings,
}

fn configure(session: &mut Session, params: &Value) -> Result<Value> {
    let input: Configure = serde_json::from_value(params.clone()).map_err(|e| bad("cloud.configure", e.to_string()))?;
    input.settings.validate().map_err(error)?;
    session.cloud.settings = input.settings;
    settings(session, &Value::Null)
}

fn submit(session: &mut Session, operation: Operation) -> Result<Value> {
    // Retain active saves; bound terminal history to avoid unbounded state on long sessions.
    if session.cloud.jobs.len() >= 128 {
        let removable = session.cloud.jobs.iter().find_map(|(id, job)| {
            if session.cloud.captures.contains_key(id) {
                return None;
            }
            job.status()
                .ok()
                .filter(|s| {
                    matches!(
                        s.phase,
                        solidcraft_cloud::jobs::Phase::SavedLocally
                            | solidcraft_cloud::jobs::Phase::Ready
                            | solidcraft_cloud::jobs::Phase::Synced
                            | solidcraft_cloud::jobs::Phase::Cancelled
                            | solidcraft_cloud::jobs::Phase::Failed
                    )
                })
                .map(|_| id.clone())
        });
        if let Some(id) = removable {
            session.cloud.jobs.remove(&id);
        } else {
            return Err(bad("cloud", "too many cloud operations; finish or cancel a pending operation"));
        }
    }
    let host = session.cloud.host.as_ref().ok_or_else(|| bad("cloud", "cloud storage is unavailable in this host"))?;
    let job = host.submit(operation, session.cloud.settings.git_executable.clone()).map_err(error)?;
    let status = job.status().map_err(error)?;
    session.cloud.jobs.insert(status.operation.clone(), job);
    Ok(json!(status))
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ConnectionInput {
    connection: Option<String>,
}

fn connection(session: &Session, params: &Value) -> Result<solidcraft_cloud::Connection> {
    let input: ConnectionInput = serde_json::from_value(params.clone()).map_err(|e| bad("cloud", e.to_string()))?;
    session.cloud.settings.destination(input.connection.as_deref()).cloned().map_err(error)
}

fn test(session: &mut Session, params: &Value) -> Result<Value> {
    submit(session, Operation::Test(connection(session, params)?))
}
fn list(session: &mut Session, params: &Value) -> Result<Value> {
    submit(session, Operation::List(connection(session, params)?))
}
fn sync(session: &mut Session, params: &Value) -> Result<Value> {
    submit(session, Operation::Synchronize(connection(session, params)?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    destination: Destination,
}

fn open(session: &mut Session, params: &Value) -> Result<Value> {
    let input: Open = serde_json::from_value(params.clone()).map_err(|e| bad("cloud.open", e.to_string()))?;
    input.destination.validate().map_err(error)?;
    let connection = session.cloud.settings.destination(Some(&input.destination.connection)).cloned().map_err(error)?;
    submit(session, Operation::Open { connection, destination: input.destination })
}

fn operation_id(params: &Value) -> Result<&str> {
    params.get("operation").and_then(Value::as_str).ok_or_else(|| bad("cloud", "operation id required"))
}

fn finish(session: &mut Session, params: &Value) -> Result<Value> {
    let id = operation_id(params)?;
    let job = session.cloud.jobs.get(id).ok_or_else(|| bad("cloud.open.finish", "unknown operation"))?;
    let status = job.status().map_err(error)?;
    let snapshot = job.take_opened().map_err(error)?;
    let document = solidcraft_io::cloud::open_document(&snapshot.manifest, &snapshot.document, |path| {
        snapshot.objects.get(path).cloned().ok_or_else(|| solidcraft_cloud::Error::Invalid("missing asset".into()))
    })
    .map_err(error)?;
    let mut fresh = Session::new(document);
    fresh.cloud.binding =
        Some(Binding { destination: status.destination.ok_or_else(|| bad("cloud.open.finish", "missing destination"))?, revision: status.revision });
    session.replace_design(fresh);
    Ok(json!({"opened":true,"binding":session.cloud.binding}))
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Save {
    destination: Option<Destination>,
}

pub(super) fn save(session: &mut Session, params: &Value) -> Result<Value> {
    session.poll_cloud()?;
    if !session.cloud.captures.is_empty() {
        return Err(bad("cloud.save", "the previous save is still being persisted locally"));
    }
    let input: Save = serde_json::from_value(params.clone()).map_err(|e| bad("cloud.save", e.to_string()))?;
    let destination = input
        .destination
        .or_else(|| session.cloud.binding.as_ref().map(|b| b.destination.clone()))
        .ok_or_else(|| bad("cloud.save", "choose a repository, project folder, and file name"))?;
    let connection = session.cloud.settings.destination(Some(&destination.connection)).cloned().map_err(error)?;
    let parent = session.cloud.binding.as_ref().filter(|binding| binding.destination == destination).and_then(|binding| binding.revision.clone());
    let snapshot = solidcraft_io::cloud::prepare_document(&session.design()).map_err(error)?;
    let save = PendingSave::stage(&connection, &destination.project, &destination.file, parent, snapshot).map_err(error)?;
    let binding = Binding { destination, revision: Some(save.record.id.clone()) };
    let reply = submit(session, Operation::Save { connection, save: Box::new(save), synchronize: session.cloud.settings.sync_after_save })?;
    let id = operation_id(&reply)?.to_string();
    session.cloud.captures.insert(id, Capture { doc: session.doc.clone(), visibility: session.visibility.clone(), binding });
    Ok(reply)
}

fn status(session: &mut Session, params: &Value) -> Result<Value> {
    session.poll_cloud()?;
    if let Some(id) = params.get("operation").and_then(Value::as_str) {
        let job = session.cloud.jobs.get(id).ok_or_else(|| bad("cloud.status", "unknown operation"))?;
        Ok(json!(job.status().map_err(error)?))
    } else {
        Ok(json!(session.cloud_operations()?))
    }
}

fn cancel(session: &mut Session, params: &Value) -> Result<Value> {
    let id = operation_id(params)?;
    let job = session.cloud.jobs.get(id).ok_or_else(|| bad("cloud.cancel", "unknown operation"))?;
    job.cancel().map_err(error)?;
    Ok(json!(job.status().map_err(error)?))
}

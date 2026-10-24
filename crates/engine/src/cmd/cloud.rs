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
    CommandSpec::new("cloud.ssh.generate", "Generate SSH Key", generate_key)
        .icon("settings")
        .noundo()
        .doc_only()
        .params("no parameters; creates a local Ed25519 pair asynchronously; optional secret input supplied through Session::set_cloud_ssh_key_input; returns operation id then public key and private-key path"),
    CommandSpec::new("cloud.connect", "Connect GitHub", connect)
        .icon("settings")
        .noundo()
        .doc_only()
        .params("connection: proposed connection with opaque credential id; verify provider and prepare checkout asynchronously; configure returned connection separately"),
    CommandSpec::new("cloud.configure", "Configure Cloud Saves", configure)
        .icon("settings")
        .noundo()
        .doc_only()
        .params("settings: local connections/defaults, credential identifiers only; returns operation id; settings apply after persistence"),
    CommandSpec::new("cloud.settings", "Cloud Saves Settings", settings)
        .icon("settings")
        .noundo()
        .doc_only()
        .params("reload?: load host settings asynchronously; otherwise returns settings, configuring flag, and destination"),
    CommandSpec::new("cloud.test", "Test Repository Connection", test)
        .icon("inspect")
        .noundo()
        .doc_only()
        .params("connection?: saved connection id, or proposed?: connection to verify before configuring; git_executable?: override for this test; returns operation id and verified host metadata"),
    CommandSpec::new("cloud.thumbnail", "Load Design Preview", thumbnail)
        .icon("inspect")
        .noundo()
        .doc_only()
        .params("destination: {connection,project,file}; asynchronously reads only a bounded optional PNG; result png is base64 or null; consume with cloud.thumbnail.finish"),
    CommandSpec::new("cloud.thumbnail.finish", "Finish Loading Design Preview", finish_thumbnail)
        .icon("inspect")
        .noundo()
        .doc_only()
        .params("operation: completed cloud.thumbnail operation id; returns and consumes the transient base64 PNG reply"),
    CommandSpec::new("cloud.list", "Open Repository", list)
        .icon("open")
        .noundo()
        .doc_only()
        .params("connection?: connection id; returns operation id, then project design destinations"),
    CommandSpec::new("cloud.open", "Open Cloud Design", open)
        .icon("open")
        .noundo()
        .doc_only()
        .params("destination: {connection,project,file}; local_revision?: exact queued capture to inspect offline; returns operation id; finish with cloud.open.finish"),
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

fn generate_key(session: &mut Session, params: &Value) -> Result<Value> {
    let input = session.cloud.ssh_key_input.take().unwrap_or_default();
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad("cloud.ssh.generate", "pass secret input directly; this command takes no parameters"));
    }
    input.validate().map_err(error)?;
    submit(session, Operation::GenerateSshKey(input))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Connect {
    connection: solidcraft_cloud::Connection,
}

fn connect(session: &mut Session, params: &Value) -> Result<Value> {
    let input: Connect = serde_json::from_value(params.clone()).map_err(|e| bad("cloud.connect", e.to_string()))?;
    input.connection.validate().map_err(error)?;
    if input.connection.provider != "github" {
        return Err(bad("cloud.connect", "unsupported setup provider"));
    }
    session.poll_cloud()?;
    if session.cloud_configuring() {
        return Err(bad("cloud.connect", "connection settings are still being stored or loaded"));
    }
    submit(session, Operation::Setup(input.connection))
}

fn settings(session: &mut Session, params: &Value) -> Result<Value> {
    session.poll_cloud()?;
    if params.get("reload").and_then(Value::as_bool) == Some(true) {
        return configure_operation(session, Operation::LoadSettings);
    }
    Ok(json!({"settings":session.cloud.settings,"binding":session.cloud.binding,"configuring":session.cloud_configuring()}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Configure {
    settings: Settings,
}

fn configure(session: &mut Session, params: &Value) -> Result<Value> {
    let input: Configure = serde_json::from_value(params.clone()).map_err(|e| bad("cloud.configure", e.to_string()))?;
    input.settings.validate().map_err(error)?;
    session.poll_cloud()?;
    if session.cloud.host.is_none() {
        session.cloud.settings = input.settings;
        session.cloud.settings_changed = true;
        return settings(session, &Value::Null);
    }
    configure_operation(session, Operation::Configure(input.settings))
}

fn configure_operation(session: &mut Session, operation: Operation) -> Result<Value> {
    if session.cloud_configuring() {
        return Err(bad("cloud.configure", "connection settings are still being stored or loaded"));
    }
    let reply = submit(session, operation)?;
    session.cloud.configuration = Some(operation_id(&reply)?.to_owned());
    session.poll_cloud()?;
    Ok(reply)
}

fn submit(session: &mut Session, operation: Operation) -> Result<Value> {
    let executable = session.cloud.settings.git_executable.clone();
    submit_using(session, operation, executable)
}

fn submit_using(session: &mut Session, operation: Operation, executable: String) -> Result<Value> {
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
    let job = host.submit(operation, executable).map_err(error)?;
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
    if session.cloud_configuring() {
        return Err(bad("cloud", "connection settings are still being stored or loaded"));
    }
    let input: ConnectionInput = serde_json::from_value(params.clone()).map_err(|e| bad("cloud", e.to_string()))?;
    session.cloud.settings.destination(input.connection.as_deref()).cloned().map_err(error)
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Test {
    connection: Option<String>,
    proposed: Option<solidcraft_cloud::Connection>,
    git_executable: Option<String>,
}

fn test(session: &mut Session, params: &Value) -> Result<Value> {
    session.poll_cloud()?;
    if session.cloud_configuring() {
        return Err(bad("cloud.test", "connection settings are still being stored or loaded"));
    }
    let input: Test = serde_json::from_value(params.clone()).map_err(|e| bad("cloud.test", e.to_string()))?;
    let candidate = match input.proposed {
        Some(connection) => {
            if input.connection.is_some() {
                return Err(bad("cloud.test", "choose a saved or proposed connection"));
            }
            connection.validate().map_err(error)?;
            connection
        }
        None => session.cloud.settings.destination(input.connection.as_deref()).cloned().map_err(error)?,
    };
    let executable = input.git_executable.unwrap_or_else(|| session.cloud.settings.git_executable.clone());
    let settings = Settings { git_executable: executable.clone(), ..Default::default() };
    settings.validate().map_err(error)?;
    submit_using(session, Operation::Test(candidate), executable)
}
fn list(session: &mut Session, params: &Value) -> Result<Value> {
    session.poll_cloud()?;
    submit(session, Operation::List(connection(session, params)?))
}
fn sync(session: &mut Session, params: &Value) -> Result<Value> {
    session.poll_cloud()?;
    submit(session, Operation::Synchronize(connection(session, params)?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    destination: Destination,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenRevision {
    destination: Destination,
    local_revision: Option<String>,
}

fn thumbnail(session: &mut Session, params: &Value) -> Result<Value> {
    session.poll_cloud()?;
    if session.cloud_configuring() {
        return Err(bad("cloud.thumbnail", "connection settings are still being stored or loaded"));
    }
    let input: Open = serde_json::from_value(params.clone()).map_err(|e| bad("cloud.thumbnail", e.to_string()))?;
    input.destination.validate().map_err(error)?;
    let connection = session.cloud.settings.destination(Some(&input.destination.connection)).cloned().map_err(error)?;
    submit(session, Operation::Thumbnail { connection, destination: input.destination })
}

fn finish_thumbnail(session: &mut Session, params: &Value) -> Result<Value> {
    let id = operation_id(params)?;
    let job = session.cloud.jobs.get(id).ok_or_else(|| bad("cloud.thumbnail.finish", "unknown operation"))?;
    let status = job.status().map_err(error)?;
    solidcraft_cloud::Thumbnail::decode_response(status.result.as_ref().ok_or_else(|| bad("cloud.thumbnail.finish", "preview missing"))?)
        .map_err(error)?;
    job.take_result().map_err(error)
}

fn open(session: &mut Session, params: &Value) -> Result<Value> {
    session.poll_cloud()?;
    if session.cloud_configuring() {
        return Err(bad("cloud.open", "connection settings are still being stored or loaded"));
    }
    let input: OpenRevision = serde_json::from_value(params.clone()).map_err(|e| bad("cloud.open", e.to_string()))?;
    input.destination.validate().map_err(error)?;
    let connection = session.cloud.settings.destination(Some(&input.destination.connection)).cloned().map_err(error)?;
    if input.local_revision.as_ref().is_some_and(|id| id.len() != 64 || !id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))) {
        return Err(bad("cloud.open", "invalid local revision"));
    }
    submit(session, Operation::Open { connection, destination: input.destination, local_revision: input.local_revision })
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
    if session.cloud_configuring() {
        return Err(bad("cloud.save", "connection settings are still being stored or loaded"));
    }
    if !session.cloud.captures.is_empty() {
        return Err(bad("cloud.save", "the previous save is still being persisted locally"));
    }
    let input: Save = serde_json::from_value(params.clone()).map_err(|e| bad("cloud.save", e.to_string()))?;
    let destination = input
        .destination
        .or_else(|| session.cloud.binding.as_ref().map(|b| b.destination.clone()))
        .ok_or_else(|| bad("cloud.save", "choose a repository, project folder, and file name"))?;
    let connection = session.cloud.settings.destination(Some(&destination.connection)).cloned().map_err(error)?;
    destination.validate().map_err(error)?;
    // Reusing the saved baseline must not capture a new parent-linked revision or
    // start an upload. A different destination is still an explicit cloud copy.
    if session.cloud.binding.as_ref().is_some_and(|binding| binding.destination == destination) && !session.is_dirty() {
        return Ok(json!({"unchanged":true,"binding":session.cloud.binding}));
    }
    // Like native Save, an unnamed document takes the name chosen for its destination.
    // Cloud design paths are directories, so dots in their names are not extensions.
    let renamed = (session.doc.name.trim().is_empty() || session.doc.name == "Untitled")
        .then(|| (session.doc.name.clone(), destination.file.rsplit('/').next().unwrap_or(&destination.file).to_owned()));
    if let Some((from, to)) = &renamed {
        super::file::rename_design(session, from, to);
    }
    let result = capture(session, connection, destination);
    if result.is_err()
        && let Some((from, to)) = &renamed
    {
        super::file::rename_design(session, to, from);
    }
    result
}

fn capture(session: &mut Session, connection: solidcraft_cloud::Connection, destination: Destination) -> Result<Value> {
    let parent = session.cloud.binding.as_ref().filter(|binding| binding.destination == destination).and_then(|binding| binding.revision.clone());
    let mut snapshot = solidcraft_io::cloud::prepare_document(&session.design()).map_err(error)?;
    if let Some(png) = crate::thumbnail::saved(session) {
        let _ = solidcraft_cloud::attach_thumbnail(&mut snapshot, png);
    }
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

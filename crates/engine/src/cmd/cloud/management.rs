//! Repository management shares the existing asynchronous command and durability contract.
use super::*;
use solidcraft_cloud::management::Change;

pub(crate) static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("cloud.discard", "Discard Queued Cloud Action", discard).icon("stop").noundo().doc_only()
        .params("destination, revision: exact queued management id; verify repository state before discarding only that intent; saved designs and assets are preserved"),
    CommandSpec::new("cloud.website", "Open on GitHub", website).icon("open").noundo().doc_only()
        .params("connection?: selected GitHub connection id; returns canonical repository URL"),
    CommandSpec::new("cloud.history", "Cloud Design History", history).icon("inspect").noundo().doc_only()
        .params("destination: {connection,project,file}; returns operation id, then revision and up to 50 historical versions"),
    CommandSpec::new("cloud.trash", "Cloud Trash", trash).icon("open").noundo().doc_only()
        .params("connection?: connection id; returns operation id, then recoverable deleted designs"),
    CommandSpec::new("cloud.rename", "Rename Cloud Design", rename).icon("settings").noundo().doc_only()
        .params("destination, revision: observed revision or null, file: new project-relative design path; durable rename with collision checks"),
    CommandSpec::new("cloud.delete", "Move Cloud Design to Trash", delete).icon("delete").noundo().doc_only()
        .params("destination, revision: observed revision or null; durably queue recoverable deletion"),
    CommandSpec::new("cloud.restore", "Restore Cloud Revision", restore).icon("save").noundo().doc_only()
        .params("destination, revision: observed current revision or null, commit: full Git SHA, file: historical project-relative path; publish old contents as a new revision"),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Discard {
    destination: Destination,
    revision: String,
}
fn discard(session: &mut Session, params: &Value) -> Result<Value> {
    session.poll_cloud()?;
    let input: Discard = serde_json::from_value(params.clone()).map_err(|e| bad("cloud.discard", e.to_string()))?;
    input.destination.validate().map_err(error)?;
    if input.revision.len() != 64 || !input.revision.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad("cloud.discard", "exact queued revision required"));
    }
    let connection = super::connection(session, &json!({"connection":input.destination.connection}))?;
    submit(session, Operation::Discard { connection, destination: input.destination, revision: input.revision })
}

fn website(session: &mut Session, params: &Value) -> Result<Value> {
    let connection = super::connection(session, params)?;
    Ok(json!({"url":solidcraft_cloud::setup::github_website(&connection).map_err(error)?}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    destination: Destination,
}

fn history(session: &mut Session, params: &Value) -> Result<Value> {
    session.poll_cloud()?;
    let input: Target = serde_json::from_value(params.clone()).map_err(|e| bad("cloud.history", e.to_string()))?;
    input.destination.validate().map_err(error)?;
    let connection = super::connection(session, &json!({"connection":input.destination.connection}))?;
    submit(session, Operation::History { connection, destination: input.destination })
}

fn trash(session: &mut Session, params: &Value) -> Result<Value> {
    session.poll_cloud()?;
    submit(session, Operation::Trash(super::connection(session, params)?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    destination: Destination,
    revision: Option<String>,
    file: Option<String>,
    commit: Option<String>,
}

fn manage(session: &mut Session, params: &Value, command: &str) -> Result<Value> {
    session.poll_cloud()?;
    let input: Input = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    if input.revision.as_ref().is_some_and(|r| r.is_empty() || r.len() > 128 || r.chars().any(char::is_control)) {
        return Err(bad(command, "invalid observed revision"));
    }
    let change = match command {
        "cloud.rename" if input.commit.is_none() => Change::Rename { file: input.file.ok_or_else(|| bad(command, "new design path required"))? },
        "cloud.delete" if input.file.is_none() && input.commit.is_none() => Change::Delete,
        "cloud.restore" => Change::Restore {
            commit: input.commit.ok_or_else(|| bad(command, "historical commit required"))?,
            file: input.file.ok_or_else(|| bad(command, "historical design path required"))?,
        },
        _ => return Err(bad(command, "unexpected management parameters")),
    };
    change.validate(&input.destination).map_err(error)?;
    let connection = super::connection(session, &json!({"connection":input.destination.connection}))?;
    submit(session, Operation::Manage { connection, destination: input.destination, expected_revision: input.revision, change })
}

fn rename(session: &mut Session, params: &Value) -> Result<Value> {
    manage(session, params, "cloud.rename")
}
fn delete(session: &mut Session, params: &Value) -> Result<Value> {
    manage(session, params, "cloud.delete")
}
fn restore(session: &mut Session, params: &Value) -> Result<Value> {
    manage(session, params, "cloud.restore")
}

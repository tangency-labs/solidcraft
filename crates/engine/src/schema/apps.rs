//! Contracts and result types for Send to App: `file.send_to_app` and `file.apps` (RFC 0018).
//!
//! No command adds or removes a target: the user does that in the Send to App window, so a script or
//! an agent can start only a program the user chose.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::types::StringList;

/// Send the open design, or some of its bodies, to an app the user added under Send to App.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SendToAppParams {
    /// The app, by its id or name from `file.apps`. A program path is refused: only an app the
    /// user added can be started.
    pub target: String,
    /// The format to send, in any case: `3mf`, `3mf-tagged`, `step`, `stl` or `sc` (the SolidCraft
    /// design file, the whole design as a copy, never the file you have open). It must be one the
    /// app takes. Default: the app's default format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// The bodies to send, by name (not read for `sc`, which sends the whole design). Default: the
    /// selected bodies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bodies: Option<StringList>,
    /// A folder to keep the file in instead of this user's send folder (made if missing): the file
    /// is named after the design with the next free `-v<N>`, and is never replaced. The reply has
    /// `version` and `copy`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_to: Option<String>,
    /// With `copy_to`: true keeps one file named after the design and replaces it each time.
    /// Default: false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_overwrite: Option<bool>,
    /// Milliseconds to wait for the app to end, 0 to 10000. An app that ends within that time has
    /// its exit code in the reply, and a non-zero one in `app_error`; one that is still running is
    /// left running. Default: 0 (do not wait).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 0, max = 10000))]
    pub wait: Option<u32>,
}

/// What `file.send_to_app` returns: the file that was written, and what happened when the app was
/// started with it. The file is saved whether or not the app started.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SendToAppResult {
    /// The file written.
    pub path: String,
    /// Its size in bytes.
    pub bytes: usize,
    /// The format written: `3mf`, `stl`, `step` or `sc`.
    pub format: String,
    /// The file name had to be numbered because the name was in use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numbered: Option<bool>,
    /// With `copy_to` (without `copy_overwrite`), and for `sc`: the `-v<N>` of the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// With `copy_to`: the file is the user's kept copy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<bool>,
    /// A 3MF was asked for and could not be written, so the file is an STL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stl_fallback: Option<bool>,
    /// The bodies written laid on their print face.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub print_faces: Vec<String>,
    /// Warnings of the export.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// The command that was started (or would have been, when it did not start), with its real
    /// values: the program, then each argument as it was handed over. No shell reads it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<String>,
    /// The app's name: it was started with the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// The process id of the app as started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// With `wait`: true when the app ended within the wait.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exited: Option<bool>,
    /// With `wait`: the app's exit code, when it ended with one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Why the app did not take the file: it would not start, or it ended with a non-zero code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_error: Option<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `file.apps` takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AppsParams {}

/// One app the user added.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AppInfo {
    /// What `file.send_to_app` takes as `target`.
    pub id: String,
    /// The name shown to the user.
    pub name: String,
    /// `app`: the kind of target (a slicer is listed by `file.slicers`).
    pub kind: String,
    /// The program that is started, as the user chose it.
    pub program: String,
    /// The argument template: `{file}`, `{name}`, `{dir}` and `{format}` are replaced inside an argument.
    pub args: Vec<String>,
    /// The formats it takes.
    pub formats: Vec<String>,
    /// The format sent when none is asked for.
    pub default_format: String,
    /// Whether the program was found on this computer and can be started.
    pub found: bool,
}

/// What `file.apps` returns: the apps the user added, in the order they are listed (empty in the
/// browser).
#[derive(Debug, Serialize, JsonSchema)]
pub struct AppsResult {
    /// The apps.
    pub apps: Vec<AppInfo>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "apps_tests.rs"]
mod tests;

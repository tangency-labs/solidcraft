//! Contracts and result types for the commands in `cmd/edit.rs`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `edit.repeat` takes no parameters: it starts the last tool with its defaults.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RepeatParams {}

/// What `edit.repeat` returns: nothing. Called through the engine it is always refused, since
/// only the app has a tool to start.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RepeatResult {}

/// Display-only timeline playback requests. The connected UI owns the animation clock.
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackAction {
    #[default]
    Toggle,
    Play,
    Stop,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TimelinePlayParams {
    /// Omit to toggle; explicit play/stop requests are idempotent.
    #[serde(default)]
    pub action: PlaybackAction,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TimelinePlayResult {
    /// Validated display request. A connected UI applies it; a headless engine has no clock.
    pub action: PlaybackAction,
}

#[cfg(test)]
mod tests {
    use super::RepeatParams;
    use crate::schema::input_schema_for;
    use crate::schema::tests::assert_keys;

    #[test]
    fn schemas_and_the_key_check_agree() {
        assert_keys(input_schema_for::<RepeatParams>(), &[]);
    }
}

//! What a chamfer mill is refused for: its pointed end leaves ridges between roughing passes a
//! diameter apart, so it does not rough.

use crate::tool::{Tool, ToolKind};
use crate::{Result, invalid};

/// Refuses `tool` for an operation that roughs (`what`): it plunges straight down into the
/// stock and takes everything between passes up to a diameter apart as cut down to the level,
/// which a chamfer mill's pointed end does not do.
pub(crate) fn roughs(tool: &Tool, what: &str) -> Result<()> {
    match tool.kind {
        ToolKind::Chamfer => invalid(format!(
            "{what} needs a tool that cuts flat between its passes: a chamfer mill's pointed end leaves ridges up to its cone's height; rough with an end mill"
        )),
        _ => Ok(()),
    }
}

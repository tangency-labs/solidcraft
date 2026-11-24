//! What the user had selected in the model, as context for a message ("Attach selection", #2286).
//! The describing lives in `solidcraft_engine::selection_context`, shared with the MCP server's
//! `selection` block; this is the chat's view of it.

pub use solidcraft_engine::selection_context::{
    Context, Entry, Kind, MAX_ITEMS, NameCache, clicked, current_sels, describe, describe_groups, describe_selection, is_gone, selection_count,
};

/// The message as the agent gets it: the text, then what was selected.
pub fn with_context(text: &str, context: &Context) -> String {
    let block = context.prompt_block();
    if block.is_empty() { text.to_string() } else { format!("{text}\n\n{block}") }
}

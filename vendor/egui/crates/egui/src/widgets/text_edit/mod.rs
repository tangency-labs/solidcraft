mod builder;
mod completion;
mod output;
mod state;
mod text_buffer;

pub use {
    crate::text_selection::TextCursorState,
    builder::{TextEdit, cap_ink_rise, galley_cap_ink_rise},
    completion::{CompletionOutput, CompletionPopup, CompletionQuery, Suggestion},
    output::TextEditOutput,
    state::TextEditState,
    text_buffer::TextBuffer,
};

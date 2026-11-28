//! Split Body saves `tool_bodies` when a whole surface body cuts (`tool_sheet` `body`) and more
//! surface bodies cut with it, each whole and as it is (#13303). Older designs omit it and cut
//! with the tool's body alone. The last release opens a design that has it with a warning, keeps
//! the field it cannot read, and cuts with the tool's body alone.

//! Split Body saves `tool_sheet` when a face tool cuts with the picked faces exactly as they are
//! (`faces`, with the other picked faces' points in `tool_faces`) or with every face of the tool's
//! surface body (`body`), never extended (#9305). Older designs omit both and keep the picked
//! face's surface extended. The last release opens a design that has them with a warning, keeps
//! the fields it cannot read, and splits with the picked face extended.

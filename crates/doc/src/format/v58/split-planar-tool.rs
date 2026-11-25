//! Split Body saves `tool_plane` when a selected face supplies its infinite plane. The tool's
//! existing body, point and persistent face name keep that plane attached after edits. Older
//! designs omit this flag and retain their fixed-plane or whole-surface tool behavior. The
//! last release opens newer designs with a warning and preserves what it cannot read.

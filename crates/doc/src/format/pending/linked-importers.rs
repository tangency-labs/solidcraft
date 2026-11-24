//! A component linked to a file may name the importer that read it (an importer plugin's id,
//! version and options, or another built-in reader), so Update Links reads the file again with the
//! same one. Older designs have no such field and keep their links as they are. The last release
//! asks for an update instead of opening a design carrying this format change.

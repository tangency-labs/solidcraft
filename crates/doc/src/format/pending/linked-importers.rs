//! A component linked to a file may name the importer that read it (an importer plugin's id,
//! version and options), so Update Links reads the file again with the same one. Older designs
//! have no such field and keep their links as they are. The last release warns that the design is
//! from a newer format, opens it and ignores the field: the component keeps its bodies and the
//! file it names, but that release reads the file as a design, so Update Links fails for it
//! there, and saving the design there drops the importer.

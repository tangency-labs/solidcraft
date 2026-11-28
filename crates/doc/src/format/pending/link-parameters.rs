//! A linked component (`links` → `parameters`) can set user parameters of its linked design, each
//! an expression over this design's parameters (`component.link_parameters`), set every parameter
//! that shares a name with one of this design's (`parameters_by_name`), and keep some of those
//! at the linked design's own value (`parameters_kept`). The link keeps the values it last passed
//! (`parameter_values`) and the linked design's number user parameters with their units as it
//! last read them (`parameter_units`), so a by-name match is found without the file. The Import feature still holds the linked
//! bodies, worked out with those values, so the design opens without the linked file: as STEP
//! text, or, for a link driven again with the same bodies and faces, as B-rep records
//! (`FeatureKind::Import::records`, the form cadmpeg imports use since 0.45). An older design has
//! none of these keys and its links show the linked design's own values, as before. The last
//! release warns about the newer format number, opens the design with the bodies it holds and
//! keeps the keys it can't read; 0.44 and older read no records, so a link held as records shows
//! no bodies there until Update Links reads the linked design again (with its own values).

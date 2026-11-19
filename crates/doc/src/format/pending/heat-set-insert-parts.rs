//! A Heat-Set Insert's `insert` can save two more fields: `part`, whether the feature places the
//! brass inserts themselves in its holes, and `component`, the component of those parts, which
//! the feature replaces when it changes. Older designs have neither, so they place no parts and
//! no step is needed. The last release opens a design that has them with a newer-format warning:
//! the parts already placed stay as ordinary components, and editing the feature does not
//! replace them.

//! A sketch link to a model vertex (`LinkSource::Vertex`) can save the vertex's persistent
//! topological name (`vertex_name`) alongside its point (`at`), formed from the persistent names
//! of its incident faces (`faceA|faceB|…`, disambiguated with `~k` when repeated) so upstream
//! edits that move a vertex farther or past a neighbouring vertex keep resolving the right vertex.
//! Older designs lack `vertex_name` (`None`): they continue resolving by nearest point as before
//! and backfill `vertex_name` lazily when the sketch is edited. The last release ignores the
//! unknown field after warning about the newer format number.

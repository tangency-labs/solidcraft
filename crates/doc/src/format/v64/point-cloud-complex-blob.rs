//! A point cloud's segments, triangles and properties (`complex`) are saved as a binary part of the
//! container of their own (`complex_blob` beside `points_blob`), not as base64 text inside
//! `document.json`: the record keeps their shape (the properties' names, what they are on, the
//! named values) with empty numbers. An older design has its numbers inline and reads as before;
//! nothing is rewritten as it opens. The last release warns about the newer format number, opens
//! the design and its points, and keeps what it can't read when it saves.

//! Design containers may save mesh blobs up to 512 MiB uncompressed, using Deflate when
//! a blob exceeds the 256 MiB file limit. The complete saved file still fits that limit.
//! Older designs keep their mesh coordinates, indices and compression without migration.
//! New saves carry the next format number because released readers only accept mesh
//! blobs up to 256 MiB; they must be updated to open these larger payloads.

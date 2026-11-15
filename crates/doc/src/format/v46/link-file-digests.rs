//! A linked component's link saves `file_sha256`, the SHA-256 of the linked design file's bytes
//! as the assembly last read them, so an assembly opened again knows which linked designs changed
//! while it was closed (#9739). Older designs read unchanged: their links have no digest and are
//! taken as up to date until Update Links reads them. The last release refuses the newer format
//! and asks for an update instead of dropping the digests.

//! Mounting plates save their persistent supporting face, optional frame feature/line identity,
//! outline, bolt-hole pattern and thickness/placement expressions as `frame_plate`. Older
//! designs contain no plates and need no migration. The last release warns about the newer
//! format, opens what it can read and keeps the plate for a later save.

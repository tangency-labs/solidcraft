//! A sketch can be saved on a curved face: a `SurfaceRef` (the face certificate and the chart:
//! origin, u axis, v axis and tolerance in the face's (u, v)) with the sketch itself. Older
//! designs have no such feature; a design with one is a newer file, so the last release warns
//! and keeps the feature it cannot read. The feature kind, the attachment resolver and the
//! surface solver follow in later changes.
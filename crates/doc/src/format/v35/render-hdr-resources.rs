//! Render settings may carry immutable Radiance HDR payloads keyed by SHA-256, shared by
//! typed CPU/GPU jobs. JSON carries canonical base64; the design container stores verified
//! required blobs. HDR controls preserve backend-specific primaries policy and units.
//! Older designs omit resources and job HDR references and need no transformation. The
//! last released reader refuses the pending format instead of discarding required sources.

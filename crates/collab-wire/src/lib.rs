//! The co-editing wire (#5184): what travels between collaborators, as bytes, with every decoder
//! bounded. Pure: no I/O, no network, builds for wasm. Payload schemas (presence, pings, claims,
//! batches) live with the co-editing module in the engine and read their numbers the same way.
//!
//! So far: the mesh codec ([`Shape`], [`FaceChunk`], [`Body`]). A display mesh travels as one
//! chunk per B-rep face, keyed by the face's persistent name and a content hash, so a changed
//! body sends only the faces that changed; a mesh is sent once per shape and a body is a shape
//! hash plus a placement, so repeated parts and moves cost almost nothing.
//!
//! The codec doesn't know which mesh it carries; the sender picks it by audience. Collaborators
//! who hold the design (Creators and Workers) get the reference mesh for the live ghost: the
//! same mesh their own model reads. Viewers get the display plan's mesh at Normal quality and
//! never the reference mesh: a meshes-only role exists to show the model, not to hand over
//! export-quality geometry.
//!
//! A body imported as a mesh (STL and the like) goes as one face, the way its mesh already comes
//! (every triangle on face 0). The sender keeps it so: a chunk per triangle would pay for a name,
//! counts and vertices every time (183 KB against 108 KB for 10,000 triangles).
//!
//! The content hashes are the receiver's cache keys, never the ids a relay serves: those are keyed
//! per audience above the codec, so nobody can ask a relay whether a guessable mesh is there.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod bytes;
mod mesh;

pub use mesh::{Body, FaceChunk, Hash, Held, Placement, Shape, declared_size};

/// Bytes the wire refuses, or a mesh it can't send.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("collaboration wire: {0}")]
pub struct WireError(pub &'static str);

fn require(condition: bool, message: &'static str) -> Result<(), WireError> {
    if condition { Ok(()) } else { Err(WireError(message)) }
}

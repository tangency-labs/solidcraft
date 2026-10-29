//! wgpu calls that wgpu marks `unsafe` although SolidCraft can meet their contract, kept out of
//! `solidcraft-gpu`, which forbids unsafe code (#3236). Native only: nothing here builds for the
//! browser. Each call lives in one module that allows `unsafe_code` and states its contract.
#![deny(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod pipeline_cache;

pub use pipeline_cache::pipeline_cache;

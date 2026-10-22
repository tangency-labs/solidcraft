//! # solidcore
//!
//! A boundary-representation (B-rep) solid modeling kernel in safe Rust: a port of
//! NVIDIA's SMLib (`source/SMLib` and `source/SM_API` of
//! <https://github.com/NVIDIA-Omniverse/usd-brep>, commit `c9c5979`, Apache-2.0).
//!
//! Solids are exact: topology (shells, faces, loops, edges, vertices) is bound to
//! analytic and NURBS geometry. Every ported unit is checked side by side against the
//! upstream C++ through `solidcore-ffi`: identical topology and values within `1e-9`
//! relative.
//!
//! ## Design
//!
//! - **No global state.** All tolerances, the deterministic RNG and caches live in a
//!   [`Kernel`] value that is passed to operations (upstream's `SmContext`).
//! - **Arena topology.** A [`Brep`] owns flat arenas of records addressed by typed ids
//!   ([`FaceId`], [`EdgeId`], ...), never pointers.
//! - **Errors are values.** Every fallible operation returns [`Result`] with a
//!   [`SolidError`] naming the operation and entity involved; library code never panics
//!   on input-derived data.
//! - **Deterministic and portable.** No clock, no address-dependent ordering, no
//!   `unsafe`; builds for `wasm32-unknown-unknown`.
//!
//! ## Modules
//!
//! Foundation: [`math`], [`error`], [`kernel`], [`limits`], [`topology`], [`geom`]. Algorithm layers (filled
//! in as units are ported, see `plan/modules.toml`): [`nurbs`], [`curve`],
//! [`surface`], [`intersect`], [`boolean`], [`blend`], [`offset`], [`heal`], [`mass`],
//! [`query`], [`sweep`], [`primitive`].
#![forbid(unsafe_code)]

pub mod error;
pub mod kernel;
pub mod limits;
pub mod math;
pub mod topology;

pub mod blend;
pub mod boolean;
pub mod curve;
pub mod geom;
pub mod heal;
pub mod intersect;
pub mod mass;
pub mod nurbs;
pub mod offset;
pub mod primitive;
pub mod query;
pub mod surface;
pub mod sweep;

pub use error::{EntityRef, NlibCode, Result, SolidError};
pub use kernel::{Caches, Kernel, KernelOptions, Rng, Tolerances};
pub use math::{Point2, Point3, Transform, Vec2, Vec3};
pub use topology::{Brep, BrepId, EdgeId, FaceId, LoopId, ShellId, VertexId};

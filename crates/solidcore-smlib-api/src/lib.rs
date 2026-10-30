//! The SolidCraft fork's C API to SMLib (tangency-labs/smlib `capi/include/smlib.h`) as a
//! Rust API over solidcore, so a backend written against the `smlib_*` functions moves to
//! solidcore one call at a time.
//!
//! Each `smlib_*` function is a method of [`Smlib`] with the same name less the prefix,
//! the same arguments as Rust values (a `&Brep` for a `const smlib_brep *`, an owned
//! [`Brep`] for an `smlib_brep **out`), the same checks and the same result: `Ok` where the
//! C function returns `SMLIB_OK`, otherwise the [`Status`] it returns (`SMLIB_ERR_*`, or an
//! SMLib `SM_ERR*` status passed through). As in the C API, inputs are never changed: an
//! operation SMLib performs in place runs on a copy.
//!
//! The glue is ported from the fork's `capi/src` (Apache-2.0, Copyright SolidCraft
//! contributors); each module names its file and the fork commit it follows. The kernel
//! calls are solidcore's ports of `SM_API` and SMLib. Differences from the C API:
//! - no handles, no `smlib_brep_free` (a [`Brep`] is dropped), no output buffers: a result
//!   that the C API copies into a caller buffer (`SMLIB_ERR_BUFFER` when too small) is
//!   returned as a value;
//! - no library lock: a [`Smlib`] is used through `&mut self`, so calls on one value are
//!   serialised by the borrow checker; separate values are independent;
//! - no clock: the log line has no duration (`std::time` is unavailable on wasm32).
//!
//! [`Smlib::default`] runs on [`fork_kernel`]: solidcore with the SolidCraft preset of
//! fixes (`UpstreamFixes::solidcraft`), so results match the C API's builds of the fork.
//! [`Smlib::new`] takes any kernel (solidcore's default is upstream SMLib 1.0.1).
//!
//! # Rules for every family
//!
//! The families (F1 core and F1b flat arrays, F2 blends, F3 sweeps, F4 offsets, F5
//! surface tools) each add their own modules (`mod <name>;` here, one sorted line each)
//! and follow these rules:
//! - library code as solidcore's: `#![forbid(unsafe_code)]`, the workspace lints (no
//!   `unwrap`/`expect`/`panic!`/unchecked indexing outside tests), deterministic (no clock,
//!   no `HashMap` order in results), no global state, builds for
//!   `wasm32-unknown-unknown` (CI checks it);
//! - every function is a method of [`Smlib`] whose doc starts with the `smlib_*` function
//!   it mirrors; the module header names the capi file(s) and the fork commit
//!   (tangency-labs/smlib, branch and hash) the port follows, with a Rust/C table and
//!   its deviations;
//! - the same argument checks, in the same order, and the same statuses: [`Status`] holds
//!   the exact `SMLIB_*` values of `smlib.h`; a new code or mapping goes into `status.rs`
//!   (one owner);
//! - a body goes through `Smlib::run` (log line, dump, error mapping); inputs are never
//!   changed (in-place work runs on `Smlib::copy`); a result is handed back with `take` or
//!   `finish`; selections by point use `pick`;
//! - tests come from the C API's own tests (`capi/tests/*`), in `tests/<family>.rs`;
//! - each family PR flips its functions' `plan/smlib-api/<smlib_fn>.toml` status files.
//!
//! Ported so far: the B-rep basics, measures and validity ([`Smlib::brep_read`] to
//! [`Smlib::brep_check`]), the primitives and the booleans.

#![forbid(unsafe_code)]

mod body;
mod boolean;
mod check;
mod measure;
mod pick;
mod primitives;
mod session;
mod status;

pub use boolean::{
    BOOLEAN_INTERSECT, BOOLEAN_MAX_BREPS, BOOLEAN_SUBTRACT, BOOLEAN_UNION, RESULT_NONMANIFOLD,
};
pub use session::{fork_kernel, LogFn, Smlib, ABI_VERSION};
pub use solidcore::topology::Brep;
pub use status::{Result, Status};

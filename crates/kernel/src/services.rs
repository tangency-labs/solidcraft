//! The solidcore services that stay outside the kernel interface's families (`crate::route`), and
//! the midsurface helpers that are still on their way into one:
//!
//! - `constraint`, an exception by decision: the sketch solver's forward-mode dual numbers and the
//!   residuals it evaluates on every iteration, generic over the number type. A copy in the
//!   interface would be a second implementation of one job; they move when the solver's residuals
//!   move into solidcore.
//! - `tess`, the display-format exception: the display plan and meshes
//!   (`docs/design/kernel-layer.md` §10 and §13, question 2).
//! - `Error`, `face_pair`, `mid_joint` and `mid_sheet`: the midsurface feature's helpers, until a
//!   Midsurface family takes them.
//!
//! They read records and data; none makes a body, so none is routed (§2). The rest of SolidCraft
//! takes them from here, never from solidcore's crates directly: this crate is solidcore's adapter,
//! and `cargo xtask layers` keeps the other crates off solidcore. Grouped as `solidcore-ops` groups
//! them, under the same module names, so a call keeps its path below `services`.

pub use solidcore_ops::Error;
pub use solidcore_ops::constraint;
pub use solidcore_ops::face_pair;
pub use solidcore_ops::finger_joint;
pub use solidcore_ops::mid_joint;
pub use solidcore_ops::mid_sheet;

/// The display plan and meshes (`solidcore-tess`).
pub mod tess {
    pub use solidcore_tess::EvalCurve;
    pub use solidcore_tess::EvalSurface;
    pub use solidcore_tess::FacePlan;
    pub use solidcore_tess::Mesh;
    pub use solidcore_tess::PoleNode;
    pub use solidcore_tess::PoleRule;
    pub use solidcore_tess::TessPlan;
    pub use solidcore_tess::mesh;
    pub use solidcore_tess::plan;
    pub use solidcore_tess::simplify;
}

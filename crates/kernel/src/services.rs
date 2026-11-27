//! solidcore's services that the application still uses beside the kernel interface's families
//! (`crate::route`), until each moves behind it: helpers and the display plan.
//! They read records and data; none
//! makes a body, so none is routed (`docs/design/kernel-layer.md` §2).
//!
//! The rest of SolidCraft takes them from here, never from solidcore's crates directly: this
//! crate is solidcore's adapter, and `cargo xtask layers` keeps the other crates off solidcore.
//! Grouped as `solidcore-ops` groups them, under the same module names, so a call keeps its
//! path below `services`.

pub use solidcore_ops::Error;
pub use solidcore_ops::constraint;
pub use solidcore_ops::face_pair;
pub use solidcore_ops::mid_joint;
pub use solidcore_ops::mid_sheet;
pub use solidcore_ops::stroke_runs;
pub use solidcore_ops::sweep;
pub use solidcore_ops::{Body as ExactBody, CheckLevel, Order, Result, boolean, make_box, make_sphere, measure};

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

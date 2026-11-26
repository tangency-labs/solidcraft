//! Kernel adapter services used by the geometry foundation. These operate on portable
//! values and have no dependency on SolidCraft bodies or the operation router.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub use solidcore_ops::form_cage;
pub use solidcore_ops::form_curves;
pub use solidcore_ops::form_fill;
pub use solidcore_ops::form_match;
pub use solidcore_ops::form_pipe;
pub use solidcore_ops::form_subdivision;
pub use solidcore_ops::forms_view_queries;
pub use solidcore_ops::ordered_solve;
pub use solidcore_ops::parameter_line;
pub use solidcore_ops::rational_rim;
pub use solidcore_ops::ray_spline;
pub use solidcore_ops::ray_tensor;
pub use solidcore_ops::spline_space;

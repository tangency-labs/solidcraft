//! The router: which kernel runs each operation (`docs/design/kernel-layer.md`), one module per
//! family of the interface ([`modelling`], [`cell_grid`], [`exchange`], [`frames`]).
//!
//! These are the application's entry points for the operations of the kernel interface
//! ([`OpKind`]). Each asks [`which`] kernel runs it and calls that kernel's adapter. Which kernels
//! a build holds is decided at compile time; this build holds solidcore only, so the choice is a
//! constant and every call is exactly the adapter's, which is the call made before the router.

use solidcraft_kernel_api::OpKind;

mod cell_grid;
mod exchange;
mod frames;
mod modelling;

pub use cell_grid::*;
pub use exchange::*;
pub use frames::*;
pub use modelling::*;

/// A kernel compiled into this build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    /// solidcore, through its adapter (`crate::backend::Solidcore`): the kernel SolidCraft
    /// maintains, and the default.
    Solidcore,
}

/// The kernel that runs `op`. Only solidcore is compiled in, so it runs every operation.
pub const fn which(_op: OpKind) -> Which {
    Which::Solidcore
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::Vec3;

    #[test]
    fn this_build_runs_every_operation_on_solidcore() {
        for &op in OpKind::ALL {
            assert_eq!(which(op), Which::Solidcore, "{}", op.name());
        }
    }

    #[test]
    fn a_routed_box_is_the_adapters_box() {
        let (a, b) = (Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 3.0, 4.0));
        let routed = box_solid(a, b).unwrap();
        let direct = crate::build::box_solid(a, b).unwrap();
        assert_eq!(routed.record(), direct.record());
    }
}

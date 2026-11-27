//! The router: which kernel runs each operation (`docs/design/kernel-layer.md`), one module per
//! family of the interface ([`modelling`], [`cell_grid`], [`exchange`], [`frames`], [`projection`], [`affine`], [`curves`], [`develop`], [`points`], [`query`], [`holes`], [`planar`], [`hatch`], [`mesh`], [`tube_cope`], [`shaft_spline`], [`dxf`], [`sections`], [`tessellate`], [`runtime`], [`profiles`]).
//!
//! These are the application's entry points for the operations of the kernel interface
//! ([`OpKind`]). Each asks [`which`] kernel runs it and calls that kernel's adapter. Which kernels
//! a build holds is decided at compile time; this build holds solidcore only, so the choice is a
//! constant and every call is exactly the adapter's, which is the call made before the router.

use solidcraft_kernel_api::OpKind;

pub mod affine;
pub mod curves;
pub mod develop;
pub mod dxf;
pub mod hatch;
pub mod mesh;
pub mod planar;
pub mod points;
pub mod profiles;
pub mod projection;
pub mod query;
pub mod runtime;
pub mod sections;
pub mod shaft_spline;
pub mod tessellate;
pub mod tube_cope;

mod cell_grid;
mod exchange;
#[cfg(feature = "backend-occt")]
pub mod foreign;
mod frames;
mod holes;
mod modelling;

pub use cell_grid::*;
pub use exchange::*;
pub use frames::*;
pub use holes::*;
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

    fn records(bodies: &[crate::body::Body]) -> Vec<&solidcore_record::brep::Brep> {
        bodies.iter().map(|b| b.record()).collect()
    }

    /// Every extrude and revolve entry point is the adapter's call: the same record as calling
    /// solidcore's builder directly.
    #[test]
    fn routed_extrudes_and_revolves_are_the_adapters() {
        use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2};
        let poly = |pts: &[(f64, f64)]| Loop2::polygon(&pts.iter().map(|&(x, y)| Vec2::new(x, y)).collect::<Vec<_>>());
        // A side drawn in two pieces, so whole sides join them; set off from the y axis to turn.
        let r = Region2 { outer: poly(&[(2.0, 0.0), (4.0, 0.0), (6.0, 0.0), (6.0, 3.0), (2.0, 3.0)]), holes: vec![] };
        let disc = Region2 { outer: Loop2::circle(Vec2::new(5.0, 8.0), 2.0), holes: vec![] };
        let pl = Plane::XY;
        let one: &dyn Fn(&Seg2, &Seg2) -> bool = &|_, _| true;
        let regions = [r.clone()];
        assert_eq!(records(&extrude(&pl, &regions, 0.0, 5.0).unwrap()), records(&crate::build::extrude(&pl, &regions, 0.0, 5.0).unwrap()));
        assert_eq!(
            records(&extrude_whole_sides(&pl, &regions, -1.0, 5.0, one).unwrap()),
            records(&crate::build::extrude_whole_sides(&pl, &regions, -1.0, 5.0, one).unwrap())
        );
        let both = [r.clone(), disc.clone()];
        assert_eq!(
            extrude_planar_regions(&pl, &both, 0.0, 2.0).unwrap().record(),
            crate::prism::extrude_planar_regions(&pl, &both, 0.0, 2.0).unwrap().record()
        );
        let corner = crate::DraftCorner::Round;
        assert_eq!(
            extrude_tapered(&pl, &r, 4.0, 1.0, 0.1, corner).unwrap().record(),
            crate::build::extrude_tapered_between(&pl, &r, 0.0, 4.0, 0.0, 1.0, 0.1, corner).unwrap().record()
        );
        assert_eq!(
            extrude_tapered_between(&pl, &disc, -1.0, 4.0, 1.0, -1.0, -0.2, corner).unwrap().record(),
            crate::build::extrude_tapered_between(&pl, &disc, -1.0, 4.0, 1.0, -1.0, -0.2, corner).unwrap().record()
        );
        let (o, d, a) = (Vec2::ZERO, Vec2::new(0.0, 1.0), 1.5);
        assert_eq!(records(&revolve(&pl, &regions, o, d, a).unwrap()), records(&crate::build::revolve(&pl, &regions, o, d, a).unwrap()));
        assert_eq!(
            records(&revolve_whole(&pl, &regions, o, d, std::f64::consts::TAU).unwrap()),
            records(&crate::build::revolve_whole(&pl, &regions, o, d, std::f64::consts::TAU).unwrap())
        );
        assert_eq!(
            records(&revolve_joining_arcs(&pl, &regions, o, d, a).unwrap()),
            records(&crate::build::revolve_joining_arcs(&pl, &regions, o, d, a).unwrap())
        );
        assert_eq!(
            records(&revolve_whole_sides(&pl, &regions, o, d, a, one).unwrap()),
            records(&crate::build::revolve_whole_sides(&pl, &regions, o, d, a, one).unwrap())
        );
    }
}

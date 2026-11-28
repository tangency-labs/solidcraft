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

    /// Every fillet and chamfer entry point is the adapter's call: the same record (and, for
    /// [`fillet_removing`], the same picks left out) as calling solidcore's blend directly.
    #[test]
    fn routed_fillets_and_chamfers_are_the_adapters() {
        use crate::{ChamferSide, FilletTurn};
        let b = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(60.0, 40.0, 20.0)).unwrap();
        let edges = [Vec3::new(30.0, 40.0, 20.0), Vec3::new(60.0, 20.0, 20.0), Vec3::new(60.0, 40.0, 10.0)];
        let (routed, gone) = fillet_removing(&b, &edges, 3.0, FilletTurn::Mitre).unwrap();
        let (direct, gone_direct) = crate::blend::fillet_removing(&b, &edges, 3.0, FilletTurn::Mitre).unwrap();
        assert_eq!((routed.record(), gone), (direct.record(), gone_direct));
        let radii = [2.0, 3.0, 4.0];
        assert_eq!(
            fillet_each(&b, &edges, &radii, FilletTurn::Mitre).unwrap().record(),
            crate::blend::fillet_each(&b, &edges, &radii, FilletTurn::Mitre).unwrap().record()
        );
        let corner = [(Vec3::new(60.0, 40.0, 20.0), 8.0)];
        assert_eq!(
            fillet_setback(&b, &edges, 5.0, &corner).unwrap().record(),
            crate::blend::fillet_setback(&b, &edges, 5.0, &corner).unwrap().record()
        );
        assert_eq!(
            chamfer_carrying_face(&b, &edges[..1], 2.0).unwrap().record(),
            crate::blend::chamfer_carrying_face(&b, &edges[..1], 2.0).unwrap().record()
        );
        let side = ChamferSide::Distance(3.0);
        assert_eq!(
            chamfer_sides_carrying_face(&b, &edges[..1], 2.0, side, true).unwrap().record(),
            crate::blend::chamfer_sides_carrying_face(&b, &edges[..1], 2.0, side, true).unwrap().record()
        );
    }

    /// The draft and split entry points are the adapter's calls: the same records as calling
    /// solidcore's directly.
    #[test]
    fn routed_drafts_and_splits_are_the_adapters() {
        use solidcore_record::Plane;
        let b = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(20.0, 10.0, 10.0)).unwrap();
        let side = [Vec3::new(20.0, 5.0, 5.0)];
        assert_eq!(
            draft(&b, &side, &Plane::XY, Vec3::Z, 0.1).unwrap().record(),
            crate::polyhedron::draft(&b, &side, &Plane::XY, Vec3::Z, 0.1).unwrap().record()
        );
        let tool = crate::SplitTool::Plane(Plane { origin: Vec3::new(7.0, 0.0, 0.0), x: Vec3::Y, y: Vec3::Z });
        let routed: Vec<_> = split_body(&b, &tool).unwrap();
        let direct: Vec<_> = crate::splitface::split_body(&b, &tool).unwrap();
        assert_eq!(routed.len(), 2);
        assert_eq!(routed.iter().map(|p| p.record()).collect::<Vec<_>>(), direct.iter().map(|p| p.record()).collect::<Vec<_>>());
        let top = b.face_at(Vec3::new(10.0, 5.0, 10.0), 1e-6).unwrap();
        let (r, rm) = split_faces_with_map(&b, &[top], &tool).unwrap();
        let (d, dm) = crate::splitface::split_faces_with_map(&b, &[top], &tool).unwrap();
        assert_eq!((r.record(), rm), (d.record(), dm));
    }

    /// Every sweep and loft entry point is the adapter's call, its path and options copied to the
    /// interface's plain data and back: the same records as calling solidcore's builders directly.
    #[test]
    fn routed_sweeps_and_lofts_are_the_adapters() {
        use crate::{PathSeg, PointPath, RailFit, SweepControl, SweepOrientation};
        use solidcore_record::{Loop2, Plane, Region2, Vec2};
        use solidcraft_geom::prelude::*;
        let sq = |h: f64| Loop2::polygon(&[Vec2::new(-h, -h), Vec2::new(h, -h), Vec2::new(h, h), Vec2::new(-h, h)]);
        let region = Region2 { outer: sq(1.0), holes: vec![] };
        let path = [
            PathSeg::Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(0.0, 0.0, 10.0) },
            PathSeg::Arc { a: Vec3::new(0.0, 0.0, 10.0), center: Vec3::new(5.0, 0.0, 10.0), axis: Vec3::new(0.0, 1.0, 0.0), angle: 1.0 },
        ];
        let pl = Plane::XY;
        assert_eq!(
            sweep(&pl, &region, &path).unwrap().record(),
            crate::build::sweep_with(&pl, &region, &path, &SweepControl::default()).unwrap().record()
        );
        let ctl = SweepControl { twist: 0.5, scale: 1.5, orientation: SweepOrientation::Perpendicular, ..SweepControl::default() };
        assert_eq!(sweep_with(&pl, &region, &path, &ctl).unwrap().record(), crate::build::sweep_with(&pl, &region, &path, &ctl).unwrap().record());
        let points = PointPath::polyline(vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 5.0), Vec3::new(3.0, 0.0, 9.0)]);
        let plain = SweepControl::default();
        assert_eq!(
            sweep_point_path_with(&pl, &region, &points, &plain).unwrap().record(),
            crate::point_path::sweep_point_path_with(&pl, &region, &points, &plain).unwrap().record()
        );
        let off =
            Region2 { outer: Loop2::polygon(&[Vec2::new(4.0, -0.5), Vec2::new(5.0, -0.5), Vec2::new(5.0, 0.5), Vec2::new(4.0, 0.5)]), holes: vec![] };
        let xz = Plane::XZ;
        let (o, z) = (Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(sweep_helix(&xz, &off, o, z, 2.0, 1.5).unwrap().record(), crate::helix::sweep_helix(&xz, &off, o, z, 2.0, 1.5).unwrap().record());
        let law = [(0.0, 0.0), (1.5, -0.3)];
        assert_eq!(
            sweep_helix_radial(&xz, &off, o, z, 2.0, 1.5, &law, true).unwrap().record(),
            crate::helix::sweep_helix_radial(&xz, &off, o, z, 2.0, 1.5, &law, true).unwrap().record()
        );
        let sections = [(Plane::XY, sq(2.0)), (Plane::XY.offset(5.0), sq(1.0))];
        assert_eq!(loft(&sections).unwrap().record(), crate::build::loft(&sections).unwrap().record());
        let rails = [vec![Vec3::new(2.0, 2.0, 0.0), Vec3::new(1.5, 1.5, 2.5), Vec3::new(1.0, 1.0, 5.0)]];
        assert_eq!(
            loft_with_rails(&sections, &rails, RailFit::Stretch).unwrap().record(),
            crate::build::loft_with_rails(&sections, &rails, RailFit::Stretch).unwrap().record()
        );
        let regions = [(Plane::XY, region.clone()), (Plane::XY.offset(4.0), Region2 { outer: sq(2.0), holes: vec![] })];
        assert_eq!(loft_regions(&regions).unwrap().record(), crate::build::loft_regions(&regions).unwrap().record());
        let apex = Vec3::new(0.0, 0.0, 6.0);
        assert_eq!(loft_to_point(&pl, &sq(2.0), apex).unwrap().record(), crate::helix::loft_to_point(&pl, &sq(2.0), apex).unwrap().record());
    }
}

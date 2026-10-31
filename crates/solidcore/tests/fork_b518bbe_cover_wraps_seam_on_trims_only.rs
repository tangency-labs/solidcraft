//! The fork's tangency-labs/smlib@b518bbe, under WR-599's
//! `Kernel::fixes.cover_wraps_across_closed_seam`: `sm_CoverOtherCoincidentFace` asks the
//! cover on the faces' stored domains to stay within one period of the covering surface
//! (`bAcrossWrappedSeams` FALSE); only its trim-derived attempt may cross a seam the surface
//! wraps. Stored domains are conservative and may be the faces' whole surfaces: across the
//! seam they cover too, and the merged face took them as its domain.
//!
//! The case is the `prog_test` `my_test_coaxial_merge_domain`
//! (`tests/prog_test/src/smTopoTest.cpp`) with its broad inputs, the one the fork's commit
//! names: three half-turn faces 1 high on coaxial cylinders of radius 50 (the middle one
//! framed a quarter turn round), stitched and merged by `RemoveTopologicalEdgesAndVertices`. Each face's stored
//! domain is its cylinder's whole natural domain. The merged face spans 180 degrees by 3;
//! the test allows the cover sampler's expansion, 200 by 4. With 16bf1cb alone the stored
//! domains merged across the seam to 360 by 208.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::math::{Point3, Vec3};
use solidcore::surface::cone::Cone;
use solidcore::surface::surface::p4::NoObject;
use solidcore::surface::surface::p5::DropPointOptions;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p5::{
    remove_topological_edges_and_vertices, stitch_and_orient, StitchAndOrientOptions,
};
use solidcore::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use solidcore::topology::brep_cache::{arena_identity, CacheObject};
use solidcore::topology::face::{p1 as face1, p2 as face2};
use solidcore::topology::topology_traverser::Marks;
use solidcore::Kernel;

/// `sm_coaxial_face_uv_box`: the UV box of the half turn x >= ... through (0, r) from z0
/// to z1 (the drops of (+-r, 0, z0|z1) guessed from the drop of (0, r, mid)).
fn coaxial_face_uv_box(k: &Kernel, s: &Surface, r: f64, z0: f64, z1: f64) -> Extent2d {
    let dom = s.natural_uv_domain_with_nurb(k).unwrap();
    let mid = s
        .drop_point::<NoObject>(
            k,
            Point3::new(0.0, r, 0.5 * (z0 + z1)),
            &dom,
            &DropPointOptions::default(),
        )
        .unwrap()
        .result_uv;
    let mut bx = Extent2d::new();
    for x in [r, -r] {
        for z in [z0, z1] {
            let opts = DropPointOptions {
                guess_uv: Some(mid),
                ..DropPointOptions::default()
            };
            let uv = s
                .drop_point::<NoObject>(k, Point3::new(x, 0.0, z), &dom, &opts)
                .unwrap()
                .result_uv;
            bx.add_point(uv);
        }
    }
    bx
}

/// The merged face's STEP domain (degrees by length) after each of the test's two passes,
/// and whether its trims lie inside its domain, for the broad (or face-sized) inputs.
fn merged_domains(fix: bool, broad: bool, reverse: bool) -> Vec<(Extent2d, bool)> {
    let mut k = Kernel::default();
    k.fixes.cover_wraps_across_closed_seam = fix;
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    for ii in 0..3 {
        let face = if reverse { 2 - ii } else { ii };
        let x = if face == 1 {
            Vec3::new(0.0, -1.0, 0.0)
        } else {
            Vec3::new(1.0, 0.0, 0.0)
        };
        let y = Vec3::new(0.0, 0.0, 1.0).cross(x);
        let surface = Surface::from(Cone::new_cylinder_bounded(
            Point3::new(0.0, 0.0, -100.0),
            x,
            y,
            50.0,
            0.0,
            360.0,
            200.0,
            false,
            false,
            false,
        ));
        let z0 = f64::from(face);
        let domain = coaxial_face_uv_box(&k, &surface, 50.0, z0, z0 + 1.0);
        let natural = surface.natural_uv_domain_with_nurb(&k).unwrap();
        let f = {
            let mut state = ArenaNotifyState::new(&b);
            let mut env = state.env(None);
            let mut marks = Marks::default();
            let this_face = |face| arena_identity(CacheObject::Face(face));
            let mut fenv = FaceBuildEnv {
                env: &mut env,
                marks: &mut marks,
                this_face: &this_face,
            };
            create_face_from_surface(&mut k, &mut b, Some(surface), &domain, &mut fenv).unwrap()
        };
        if broad {
            face2::set_uv_domain(&mut b, f, &natural).unwrap();
        }
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    let mut out = Vec::new();
    for _pass in 0..2 {
        remove_topological_edges_and_vertices(&mut k, &mut b, None, None).unwrap();
        let faces = bp1::faces(&b, None).unwrap();
        assert_eq!(faces.len(), 1, "the three faces merge into one");
        let f = faces[0];
        let domain = face2::uv_domain(&b, f).unwrap();
        let s = face2::surface(&b, f).unwrap().unwrap().clone();
        let step = s.convert_domain_from_nurbs_to_step(&k, &domain).unwrap();
        let trims =
            face1::calculate_uv_domain_from_uv_trim_curves(&k, &mut b, f, true, false, false)
                .unwrap()
                .domain;
        out.push((step, trims.is_contained_by(&domain, 1e-7)));
    }
    out
}

/// With the fix the broad inputs merge to the faces' 180 degrees by 3 (within the
/// `prog_test`'s 200 by 4), in either order and on both passes, as the face-sized ones do.
#[test]
fn fork_b518bbe_broad_stored_domains_merge_to_the_trims() {
    for reverse in [false, true] {
        for broad in [false, true] {
            for (pass, (step, inside)) in merged_domains(true, broad, reverse).iter().enumerate() {
                let size = step.size();
                assert!(
                    size.x <= 200.0 && size.y <= 4.0,
                    "reverse {reverse} broad {broad} pass {pass}: {size:?}"
                );
                assert!(
                    inside,
                    "reverse {reverse} broad {broad} pass {pass}: trims inside"
                );
            }
        }
    }
}

/// Without the option no cover crosses a seam (upstream), and the broad inputs merge to
/// the trims as well.
#[test]
fn broad_stored_domains_merge_to_the_trims_without_the_option() {
    for (pass, (step, inside)) in merged_domains(false, true, false).iter().enumerate() {
        let size = step.size();
        assert!(size.x <= 200.0 && size.y <= 4.0, "pass {pass}: {size:?}");
        assert!(inside, "pass {pass}: trims inside");
    }
}

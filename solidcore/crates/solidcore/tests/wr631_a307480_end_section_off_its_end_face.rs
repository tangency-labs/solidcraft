//! The SolidCraft fork's tangency-labs/smlib@a307480 (`fillet_cliff_overflow`, WR-631): a
//! fillet whose end section runs off its end face overflows it, and is built by
//! subtraction. SolidCraft rig fillets/cov-chamfer-outer-corner-edge-of-a-shelled-box-
//! with-a-lip: an 80 x 50 x 30 box shelled 2 open at the top, a 1 x 2 lip on the inner
//! half of its rim, its outer corner edge at (0, 50) chamfered 3. At the top the
//! chamfer's end section (from (3, 50) to (0, 47) at z = 30) runs off the 2 mm rim strip,
//! its end face, under the lip's foot. Before, the Nx1 corner's end rib left its face and
//! the chamfer came back unsewn (not a closed solid). With the fork's ruling the lip stays
//! over the chamfer: the body less the chamfer's prism, V0 - 135 = 22473.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, Kernel};

fn kernel(cliff: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_run_on_bspline_trims = true;
    k.fixes.offset_extension_from_base = true;
    k.fixes.offset_extension_as_itself = true;
    k.fixes.fillet_cliff_overflow = cliff;
    k
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    b.refresh_cached_topology();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

/// The shelled box with its lip: V0 = 22608.
fn shelled_box_with_lip(k: &mut Kernel) -> Brep {
    let outer = create_box(k, p(0.0, 0.0, 0.0), 80.0, 50.0, 30.0).unwrap();
    let inner = create_box(k, p(2.0, 2.0, 2.0), 76.0, 46.0, 30.0).unwrap();
    let shell = boolean_difference(k, outer, inner).unwrap();
    let lip_outer = create_box(k, p(1.0, 1.0, 30.0), 78.0, 48.0, 2.0).unwrap();
    let lip_inner = create_box(k, p(2.0, 2.0, 29.0), 76.0, 46.0, 4.0).unwrap();
    let lip = boolean_difference(k, lip_outer, lip_inner).unwrap();
    boolean_union(k, shell, lip).unwrap()
}

/// The edge from `a` to `q` (either way).
fn edge_between(b: &Brep, a: Point3, q: Point3) -> EdgeId {
    b.edges()
        .ids()
        .find(|&e| {
            let (s, t) = edge::start_end_vertices(b, e).unwrap();
            let (s, t) = (
                vertex::point(b, s.unwrap()).unwrap(),
                vertex::point(b, t.unwrap()).unwrap(),
            );
            (s.distance(a) < 1e-9 && t.distance(q) < 1e-9)
                || (s.distance(q) < 1e-9 && t.distance(a) < 1e-9)
        })
        .expect("edge")
}

/// Chamfers the outer corner edge at (0, 50) by 3: the volume, or the error code and
/// comment (or "not a closed solid").
fn chamfer_corner(cliff: bool) -> Result<f64, (i32, String)> {
    let mut k = kernel(cliff);
    let mut b = shelled_box_with_lip(&mut k);
    let v0 = volume(&k, &mut b);
    assert!((v0 - 22608.0).abs() < 1e-6, "V0 {v0}");
    let e = edge_between(&b, p(0.0, 50.0, 0.0), p(0.0, 50.0, 30.0));
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_constant_radius_edges(&mut k, &[e], 3.0, 0, 1, 1.0)
        .unwrap();
    if s.do_fillet(&mut k).is_err() {
        let info = s.error_info();
        return Err((info.code, info.comment.clone()));
    }
    drop(s);
    if !is_manifold_solid(&b).unwrap() {
        return Err((-1, "not a closed solid".into()));
    }
    Ok(volume(&k, &mut b))
}

#[test]
fn a307480_chamfer_end_section_under_the_lip_overflows_its_end_face() {
    let v = chamfer_corner(true).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(
        (v - 22473.0).abs() <= 1e-8 * 22473.0,
        "volume {v} against 22473"
    );
}

/// Without the flag the chamfer is not built as an overflow (refused, or left unsewn).
#[test]
fn a307480_without_the_flag_no_overflow() {
    assert!(chamfer_corner(false).is_err());
}

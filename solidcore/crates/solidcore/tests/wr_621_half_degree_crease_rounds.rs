//! Regression test for WR-621 (#1808; the fork's tangency-labs/smlib@2e8758b, capi test
//! `crease_rounds_from_half_a_degree`): `SmEdge::GetBlendEdgeuse` takes two faces for
//! tangent within 2 degrees, so a crease of 0.5 to 2 degrees cannot be rounded. The port's
//! limit is half a degree (`edge::CREASE_ANGLE`).
//!
//! The part is a block 40 x 20, 20 deep, whose top rises to a crease at x = 20, each half
//! sloping `turn / 2` degrees, so the faces turn by `turn` across it. A round of radius r
//! along the crease takes off `20 r^2 (tan(d/2) - d/2)`, d the turn. The fork asserts
//! this to 1e-6.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Vec3;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::edge;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

fn line(a: (f64, f64), b: (f64, f64)) -> Curve {
    Curve::from(create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap())
}

/// The rise of the crease above y = 20 for a turn of `turn` degrees.
fn rise(turn: f64) -> f64 {
    20.0 * (0.5 * turn * PI / 180.0).tan()
}

/// The creased block, extruded 20 along z.
fn block(k: &mut Kernel, turn: f64) -> Brep {
    let h = rise(turn);
    let curves = [
        line((0.0, 0.0), (40.0, 0.0)),
        line((40.0, 0.0), (40.0, 20.0)),
        line((40.0, 20.0), (20.0, 20.0 + h)),
        line((20.0, 20.0 + h), (0.0, 20.0)),
        line((0.0, 20.0), (0.0, 0.0)),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_taper_extrude(k, &refs, 20.0, 0.0, 3, false).unwrap()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-10, &attrs).unwrap()
}

/// The crease: the edge along z at (20, 20 + rise).
fn crease(k: &Kernel, b: &Brep, turn: f64) -> EdgeId {
    let h = rise(turn);
    let on = |p: Vec3| (p.x - 20.0).abs() < 1e-9 && (p.y - 20.0 - h).abs() < 1e-9;
    b.edge_list()
        .iter()
        .copied()
        .find(|&e| {
            let curve = edge::curve(b, e).unwrap().unwrap();
            let ivl = edge::interval(b, e).unwrap();
            let p = curve.evaluate_point(k, ivl.min()).unwrap();
            let q = curve.evaluate_point(k, ivl.max()).unwrap();
            on(p) && on(q) && (p.z - q.z).abs() > 10.0
        })
        .expect("the crease edge")
}

/// The crease of the block turning `turn` degrees rounded `r`: the volume removed, or
/// the round's failure.
fn round(turn: f64, r: f64) -> solidcore::Result<f64> {
    let mut k = Kernel::default();
    let mut b = block(&mut k, turn);
    let before = volume(&k, &mut b);
    let e = crease(&k, &b, turn);
    fillet_edges(&mut k, &mut b, &[e], r, 1, 1, 1.0)?;
    assert!(
        is_manifold_solid(&b).unwrap(),
        "turn {turn}, r {r}: not a solid"
    );
    Ok(before - volume(&k, &mut b))
}

fn removed(turn: f64, r: f64) -> f64 {
    let d = turn * PI / 180.0;
    20.0 * r * r * ((0.5 * d).tan() - 0.5 * d)
}

#[test]
fn wr_621_creases_from_half_a_degree_round() {
    for turn in [1.0, 3.0] {
        for r in [2.0, 20.0] {
            let got = round(turn, r)
                .unwrap_or_else(|e| panic!("turn {turn}, r {r}: the round failed: {e:?}"));
            let want = removed(turn, r);
            assert!(
                (got - want).abs() < 1e-6,
                "turn {turn}, r {r}: removed {got}, expected {want}"
            );
        }
    }
    // within half a degree the faces are tangent: nothing to round
    for r in [2.0, 20.0] {
        assert!(round(0.3, r).is_err(), "0.3 degrees, r {r}");
    }
}

/// The upstream `Fillet/nurb_cone.smb`, read as the goldens' file cases read it.
fn nurb_cone(k: &mut Kernel) -> Brep {
    use solidcore::geom::object::ObjectRef;
    use solidcore::topology::brep::p10::TopologyEnv;
    use solidcore::topology::brep::p7::read_from_file;
    use solidcore::topology::database_io::FileType;
    use solidcore::topology::topology::NotifyRecords;
    use solidcore::EntityRef;
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../upstream/TestFiles/pt_TestFiles/Fillet/nurb_cone.smb"
    );
    let bytes = std::fs::read(path).unwrap();
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = |_: EntityRef| ObjectRef {
        key: ObjectKey(0),
        type_code: 0,
    };
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    b
}

/// Edge 8 of `Fillet/nurb_cone.smb` (goldens `blend.fillet_executive_p3`
/// `stubs4-degenerate-nurb_cone-e8-r0.5`) turns by 0.61 degrees: a crease under the half
/// degree limit, not a tangent edge. A round of r 0.5 there stands
/// `r (sec(turn / 2) - 1)` off the edge, under its tolerance, so it is refused as too small
/// before the solver runs (which leaves the solver's step latch unset), naming the
/// smallest radius `tolerance / (sec(turn / 2) - 1)` that rounds it.
#[test]
fn wr_621_a_shallow_crease_too_small_to_round_is_refused_before_solving() {
    use solidcore::blend::blend_refusal::{round_too_small, BlendRefusal};
    let mut k = Kernel::default();
    // the refusal's query refits the edge's tolerance, so it runs on its own copy
    let mut probe = nurb_cone(&mut k);
    let e = probe.edge_list()[8];
    let refusal = round_too_small(&k, &mut probe, e, 0.5).unwrap();
    let Some(BlendRefusal::RoundTooSmall {
        edge,
        turn,
        tolerance,
        smallest_radius,
    }) = refusal
    else {
        panic!("edge 8 at r 0.5: {refusal:?}");
    };
    assert_eq!(edge, e);
    // a crease: above the half degree limit, below upstream's 2 degrees
    assert!(
        turn > 0.5 * PI / 180.0 && turn < 2.0 * PI / 180.0,
        "turn {turn}"
    );
    let reach = 1.0 / (0.5 * turn).cos() - 1.0;
    assert!(0.5 * reach < tolerance, "r 0.5 stands {} off", 0.5 * reach);
    assert_eq!(smallest_radius, tolerance / reach);
    assert!((smallest_radius - 0.715).abs() < 1e-3, "{smallest_radius}");

    let mut k = Kernel::default();
    let mut b = nurb_cone(&mut k);
    assert_eq!(b.edge_list()[8], e);
    let err = fillet_edges(&mut k, &mut b, &[e], 0.5, 1, 1, 1.0).unwrap_err();
    assert!(
        matches!(
            err,
            solidcore::SolidError::BlendRefused {
                refusal: BlendRefusal::RoundTooSmall { .. },
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(k.fillet_point_step.get(), None);
}

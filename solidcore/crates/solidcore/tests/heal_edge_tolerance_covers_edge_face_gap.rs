//! The healer's tolerance fix (`Fix_TolSizes`) gives an edge a tolerance that covers its
//! edge/face gap.
//!
//! Upstream sets a fixed edge's tolerance to `gain * gap - def` (`SmTol::CalcEdgeZoneTol3d`),
//! which is under the gap itself for gaps below ten times the default. The check's edge/face
//! test (`SmBrep` Test #4) then refuses the edge it has just fixed. Two corpus files read with
//! healing show it: on `Fillet/sphere_plane.smb` the plane/sphere circle edge is 1.69e-5 from
//! the sphere with a tolerance of 1e-5. On `Solids/revolution_mold.smb` two edges have gaps of
//! 2.99e-5 and 9.25e-5, with tolerances of 2.29e-5 and 9.17e-5.
//!
//! The kept NURB of WR-493 also brings this out. The found sphere evaluates the file's own
//! UV trims, so they are no longer dropped and rebuilt with a grown tolerance.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::attribute::AttributeStore;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::database_io::FileType;
use solidcore::topology::edgeuse::p1::max_edge_face_gap;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel};

/// Reads an upstream `.smb` test file (healing on, as `SmBrep::ReadFromFile`).
fn read(k: &mut Kernel, name: &str) -> Brep {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(name);
    let bytes = std::fs::read(&path).unwrap();
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = solidcore::topology::loopuse::notify_ref;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b
}

/// Every loop edge use's edge/face gap is within its edge's tolerance, and the check
/// reports no edge/face gap.
fn assert_gaps_covered(name: &str) {
    let mut k = Kernel::default();
    let mut b = read(&mut k, name);
    let eus: Vec<_> = b.edgeuses().iter().map(|(id, _)| id).collect();
    for eu in eus {
        let Some(e) = b.edgeuse(eu).unwrap().edge else {
            continue;
        };
        let Some(gap) = max_edge_face_gap(&k, &mut b, eu, false).unwrap() else {
            continue;
        };
        let tol = b.edge(e).unwrap().tolerance;
        assert!(
            gap.gap_3d <= tol,
            "{name}: {eu:?} of {e:?}: gap {:e} over tolerance {tol:e}",
            gap.gap_3d
        );
    }
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    let lines: Vec<_> = lines
        .iter()
        .filter(|l| l.contains("Edge/Surface gap"))
        .collect();
    assert!(lines.is_empty(), "{name}: {lines:?}");
}

#[test]
fn sphere_plane_circle_edge_covers_its_gap() {
    assert_gaps_covered("Fillet/sphere_plane.smb");
}

#[test]
fn revolution_mold_edges_cover_their_gaps() {
    assert_gaps_covered("Solids/revolution_mold.smb");
}

/// The UV trim of the sphere's use of `Fillet/sphere_plane.smb`'s circle edge (edge 3),
/// built after the read once the caller has dropped the file's trim, so
/// `GetOrCreateUVTrimCurve` makes it. Returns the curve's interval ends and 65 points
/// along it, and the edge's tolerance.
fn sphere_plane_circle_trim(k: &Kernel, b: &mut Brep) -> (Vec<[u64; 2]>, f64) {
    use solidcore::topology::edgeuse::{p1 as eu1, p3 as eu3};
    use solidcore::topology::{EdgeId, FaceId};
    let e = EdgeId::from_index(3);
    let eus = b.edge(e).unwrap().edgeuses.clone();
    let eu = eus
        .into_iter()
        .find(|&eu| eu1::face(b, eu).unwrap() == Some(FaceId::from_index(0)))
        .expect("the sphere's use of the circle edge");
    eu3::get_or_create_uv_trim_curve(k, b, eu, false, false, true)
        .unwrap()
        .status
        .unwrap();
    let trim = b.edgeuse(eu).unwrap().uv_trim_curve.clone().unwrap();
    let ivl = trim.natural_interval();
    let mut bits = vec![[ivl.min().to_bits(), ivl.max().to_bits()]];
    for i in 0..=64 {
        let p = trim
            .evaluate_point(k, ivl.evaluate(f64::from(i) / 64.0))
            .unwrap();
        bits.push([p.x.to_bits(), p.y.to_bits()]);
    }
    (bits, b.edge(e).unwrap().tolerance)
}

/// Raising an edge's tolerance to cover its gap does not loosen the UV trim fitted
/// later. On `Fillet/sphere_plane.smb` the healer raises the circle edge from the
/// default 1e-5 to 1.85e-5 (gain times its 1.69e-5 gap); the sphere's trim, made after
/// the read, is the trim made at the healer's tolerance without that raise (1e-5,
/// upstream's `CalcEdgeZoneTol3d` for the gap), bit for bit. A trim fitted at the raised
/// tolerance bounded a cap 3.9e-7 from the one the edge bounds, against 4.7e-8.
#[test]
fn sphere_plane_trim_fit_does_not_grow_with_the_gap_cover() {
    use solidcore::topology::edgeuse::p1 as eu1;
    use solidcore::topology::{EdgeId, FaceId};
    let mut k = Kernel::default();
    let mut b = read(&mut k, "Fillet/sphere_plane.smb");
    let e = EdgeId::from_index(3);
    let raised = b.edge(e).unwrap().tolerance;
    assert!(raised > 1.0e-5, "the circle edge is raised: {raised:e}");
    // the sphere's trim made again after the read (the read keeps the file's own trim,
    // WR-493)
    for eu in b.edge(e).unwrap().edgeuses.clone() {
        if eu1::face(&b, eu).unwrap() == Some(FaceId::from_index(0)) {
            eu1::set_uv_trim_curve(&mut b, eu, eu1::UvTrimCurveArg::Null, 0.0, true, None).unwrap();
        }
    }
    let (healed, tol) = sphere_plane_circle_trim(&k, &mut b);

    // the same read with the circle edge at the tolerance before the gap cover, and the
    // sphere's trim made again there
    let mut k0 = Kernel::default();
    let mut b0 = read(&mut k0, "Fillet/sphere_plane.smb");
    for eu in b0.edge(e).unwrap().edgeuses.clone() {
        if eu1::face(&b0, eu).unwrap() == Some(FaceId::from_index(0)) {
            eu1::set_uv_trim_curve(&mut b0, eu, eu1::UvTrimCurveArg::Null, 0.0, true, None)
                .unwrap();
        }
    }
    b0.edge_mut(e).unwrap().tolerance = 1.0e-5;
    let (unraised, tol0) = sphere_plane_circle_trim(&k0, &mut b0);
    // (both trims raise the edge to twice their distance to the sphere, the same)
    assert_eq!(tol.to_bits(), tol0.to_bits(), "{tol:e} {tol0:e}");
    assert_eq!(healed, unraised);
}

/// The fillet `blend.fillet_executive_p3` `source-file-corner_box-edge13` makes on
/// `Fillet/corner_box.smb` (edge 13, radius 0.1, circular section, G1, thumbweight 1),
/// with edge 13 at its tolerance as read (`lower` false) or at the tolerance before the
/// gap cover (`lower` true, the default 1e-5). Returns the vertex, edge and face counts
/// and the bits of the area, volume and centroid at the golden's accuracy (1e-4).
fn corner_box_edge13_fillet(lower: bool) -> ([usize; 3], [u64; 5]) {
    use solidcore::blend::fillet_executive::api::fillet_edges;
    use solidcore::mass::integrator::properties::brep_mass_properties_default;
    use solidcore::math::Vec3;
    use solidcore::topology::brep::p1 as bl;
    let mut k = Kernel::default();
    let mut b = read(&mut k, "Fillet/corner_box.smb");
    let e = bl::edges(&b, None).unwrap()[13];
    let raised = b.edge(e).unwrap().tolerance;
    // (the healer raised it to cover its 1.59e-5 edge/face gap)
    assert!(raised > 1.7e-5, "edge 13 is raised: {raised:e}");
    if lower {
        b.edge_mut(e).unwrap().tolerance = 1.0e-5;
    }
    fillet_edges(&mut k, &mut b, &[e], 0.1, 1, 1, 1.0).unwrap();
    let counts = [
        bl::vertices(&b, None).unwrap().len(),
        bl::edges(&b, None).unwrap().len(),
        bl::faces(&b, None).unwrap().len(),
    ];
    let m = brep_mass_properties_default(&k, &mut b, 1e-4, 1.0, Vec3::ZERO).unwrap();
    let bits = [
        m.properties.area.to_bits(),
        m.properties.volume.to_bits(),
        m.centroid.x.to_bits(),
        m.centroid.y.to_bits(),
        m.centroid.z.to_bits(),
    ];
    (counts, bits)
}

/// Raising an edge's tolerance to cover its gap does not loosen the fillet along it. On
/// `Fillet/corner_box.smb` the healer raises edge 13 from the default 1e-5 to 1.745e-5
/// (gain times its 1.59e-5 gap). The fillet solver approximates at `TOL_FACTOR` times the
/// edge's fit tolerance, which is the 1e-5 the edge had before the raise, so the round is
/// the round made with the edge at 1e-5, bit for bit. Approximated at 4 times the raised
/// tolerance, the round's volume moved 2.3e-9 relative.
#[test]
fn corner_box_fillet_does_not_loosen_with_the_gap_cover() {
    let (counts, bits) = corner_box_edge13_fillet(false);
    let (counts0, bits0) = corner_box_edge13_fillet(true);
    assert_eq!(counts, [12, 18, 8]);
    assert_eq!(counts, counts0);
    let values = bits.map(f64::from_bits);
    let values0 = bits0.map(f64::from_bits);
    assert_eq!(bits, bits0, "{values:?} {values0:?}");
}

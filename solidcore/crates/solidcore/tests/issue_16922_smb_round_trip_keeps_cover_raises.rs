//! #16922: a body written to `.smb` reads back the same.
//!
//! A read covers kept UV trim curves that stray from their edge by raising the edge's
//! tolerance (`Brep::trim_cover_raises`, not upstream), and the edge then fits at its
//! tolerance from before the raise. The writer stored the raised value as the edge's own
//! tolerance, so a read of the written file raised it again from there and fitted along
//! the edge at the raised value: coarser trims on the affected faces, and the goldens
//! case `stubs4-delete-edge-sphere_diff_3cylinders-e2-r0.1` (fillet executive) read back
//! with its area moved by 1.0e-7 relative (38235.992674532565 -> 38235.996573337565).
//! The writer now stores a still-held raise as the tolerance before it, so the second
//! read re-derives the same raise from the same trims.
//!
//! `Fillet/sphere_diff_3cylinders.smb` is that case's input: its read raises edges from
//! 1e-5 to about 2.7e-4 (MEASURED).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::topology::api_files::{read_brep_from_file, write_brep_to_file};
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::loopuse::notify_ref;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{EdgeId, EdgeuseId, FaceId};
use solidcore::{Brep, EntityRef, Kernel};

fn read(k: &mut Kernel, bytes: Vec<u8>) -> Brep {
    let mut attributes = AttributeStore::default();
    let mut records = NotifyRecords::new();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &notify_ref,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(bytes), true, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

fn write(k: &Kernel, b: &Brep) -> Vec<u8> {
    let store = AttributeStore::new();
    let records = NotifyRecords::new();
    let key_of = |e: EntityRef| b.owner_identity(e);
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &key_of,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    write_brep_to_file(k, Some(b), &env, true).unwrap()
}

/// Per edge then per vertex in walk order: the tolerance and the cover's raise of it,
/// as bits.
fn tolerances(b: &Brep) -> Vec<(u64, Option<(u64, u64)>)> {
    let raises = b.trim_cover_raises();
    let bits = |r: Option<&(f64, f64)>| r.map(|&(x, y)| (x.to_bits(), y.to_bits()));
    let mut out = Vec::new();
    for e in bl::edges(b, None).unwrap() {
        let t = b.edge(e).unwrap().tolerance;
        out.push((t.to_bits(), bits(raises.edges.get(&e))));
    }
    for v in bl::vertices(b, None).unwrap() {
        let t = b.vertex(v).unwrap().tolerance;
        out.push((t.to_bits(), bits(raises.vertices.get(&v))));
    }
    out
}

#[test]
fn a_written_body_reads_back_with_the_same_tolerances_and_cover_raises() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Fillet/sphere_diff_3cylinders.smb");
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        eprintln!("unavailable upstream test file: {}", path.display());
        return;
    };
    let mut k = Kernel::default();
    let b = read(&mut k, bytes);
    assert!(
        !b.trim_cover_raises().edges.is_empty(),
        "the input's read raises edges"
    );
    let first = write(&k, &b);
    let r = read(&mut k, first);
    assert_eq!(tolerances(&r), tolerances(&b));
    // (the written file is not a byte-for-byte fixed point: the read promotes a straight
    // B-spline edge curve to a line, as upstream's reader does, and the second write
    // stores the line; MEASURED, the only difference)
}

/// A raised vertex goes back to its tolerance from before the raise only while no edge
/// at it is written above that: after an edit sets a raised edge at it to a tolerance
/// between the vertex's two values (so the edge's raise no longer holds and the edge is
/// written as it is), the vertex is written at its raised tolerance, still covering the
/// edge.
#[test]
fn a_raised_vertex_is_written_no_tighter_than_its_edges() {
    use solidcore::topology::brep_data::BrepData;
    use solidcore::topology::{edge, vertex};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Fillet/sphere_diff_3cylinders.smb");
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        eprintln!("unavailable upstream test file: {}", path.display());
        return;
    };
    let mut k = Kernel::default();
    let mut b = read(&mut k, bytes);
    let raises = b.trim_cover_raises().clone();
    // a raised vertex (held, strictly raised) with a raised edge at it
    let (v, e, vb, va) = bl::edges(&b, None)
        .unwrap()
        .into_iter()
        .filter(|e| raises.edges.contains_key(e))
        .find_map(|e| {
            vertex::edge_vertices(&b, e)
                .unwrap()
                .into_iter()
                .flatten()
                .find_map(|v| {
                    let &(vb, va) = raises.vertices.get(&v)?;
                    let held = b.vertex(v).unwrap().tolerance.to_bits() == va.to_bits();
                    (held && vb < va).then_some((v, e, vb, va))
                })
        })
        .expect("the input's read raises a vertex at a raised edge");
    let t = 0.5 * (vb + va);
    assert!(vb < t && t < va && t.to_bits() != raises.edges[&e].1.to_bits());
    edge::set_tolerance(&mut b, e, t, false, false).unwrap();

    let store = AttributeStore::new();
    let records = NotifyRecords::new();
    let key_of = |x: EntityRef| b.owner_identity(x);
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &key_of,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    let bd = BrepData::from_brep(&k, &b, &env, &mut Vec::new()).unwrap();
    let i = bl::vertices(&b, None)
        .unwrap()
        .iter()
        .position(|&x| x == v)
        .unwrap();
    assert!(
        bd.edges
            .iter()
            .any(|ed| ed.zone_tol_3d.to_bits() == t.to_bits()),
        "the edited edge is written as it is"
    );
    assert_eq!(bd.vertices[i].zone_tol_3d.to_bits(), va.to_bits());
}

/// A raise goes back to its tolerance from before only while the read of the written file
/// keeps every trim it kept before the write: a trim whose end needs the raise (here a
/// vertex moved off a raised edge's trim end by more than its tolerance before the raise
/// and less than after it, as a trim replaced after the read can be) keeps its edge and
/// the edge's vertices written raised, and the round trip keeps the trim (review of
/// #21705).
#[test]
fn a_trim_that_needs_the_raise_keeps_it_written() {
    use solidcore::math::Point2;
    use solidcore::topology::brep_data::BrepData;
    use solidcore::topology::edgeuse::p1 as eu1;
    use solidcore::topology::face::p2 as face2;
    use solidcore::topology::{edge, vertex};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Fillet/sphere_diff_3cylinders.smb");
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        eprintln!("unavailable upstream test file: {}", path.display());
        return;
    };
    let mut k = Kernel::default();
    let mut b = read(&mut k, bytes);
    let raises = b.trim_cover_raises().clone();
    let trims = |b: &Brep| {
        bl::edges(b, None)
            .unwrap()
            .into_iter()
            .flat_map(|e| edge::edgeuses(b, e, None, None).unwrap())
            .filter(|&eu| eu1::uv_trim_curve_pointer(b, eu).unwrap().is_some())
            .count()
    };
    let before_trims = trims(&b);
    // a held raised edge with two held, strictly raised vertices, and a trim on it
    let mut found = None;
    for e in bl::edges(&b, None).unwrap() {
        let Some(&(_, ea)) = raises.edges.get(&e) else {
            continue;
        };
        if b.edge(e).unwrap().tolerance.to_bits() != ea.to_bits() {
            continue;
        }
        let vs: Vec<_> = vertex::edge_vertices(&b, e)
            .unwrap()
            .into_iter()
            .flatten()
            .collect();
        if vs.len() != 2 || vs[0] == vs[1] {
            continue;
        }
        let Some(&(vb, va)) = raises.vertices.get(&vs[0]) else {
            continue;
        };
        if b.vertex(vs[0]).unwrap().tolerance.to_bits() != va.to_bits() || vb >= va {
            continue;
        }
        for eu in edge::edgeuses(&b, e, None, None).unwrap() {
            let Some(trim) = eu1::uv_trim_curve_pointer(&b, eu).unwrap() else {
                continue;
            };
            let f = eu1::face(&b, eu).unwrap().unwrap();
            let s = face2::surface(&b, f).unwrap().unwrap();
            let ends = trim.ends(&k).unwrap();
            let p = vertex::point(&b, vs[0]).unwrap();
            let at =
                |q: solidcore::math::Point3| s.evaluate_point(&k, Point2::new(q.x, q.y)).unwrap();
            let (s0, s1) = (at(ends.start), at(ends.end));
            let near = if s0.distance(p) <= s1.distance(p) {
                s0
            } else {
                s1
            };
            found = Some((e, vs[0], eu, near, face2::tolerance(&b, f).unwrap(), vb, va));
            break;
        }
        if found.is_some() {
            break;
        }
    }
    let (e, v, eu, near, ft, vb, va) =
        found.expect("the input's read raises an edge, its vertex and keeps a trim on it");
    // move the vertex to `ft + (vb + va) / 2` from the trim's end at it
    let p = vertex::point(&b, v).unwrap();
    let d = p - near;
    let dir = if d.length() > 0.0 {
        d * (1.0 / d.length())
    } else {
        solidcore::math::Vec3::new(1.0, 0.0, 0.0)
    };
    b.vertex_mut(v).unwrap().point = near + dir * (ft + 0.5 * (vb + va));
    assert!(
        bl::check_uv_trim_curve(&k, &b, eu).is_ok(),
        "the trim passes the end check at the held tolerances"
    );
    let ea = b.edge(e).unwrap().tolerance;

    let store = AttributeStore::new();
    let records = NotifyRecords::new();
    let key_of = |x: EntityRef| b.owner_identity(x);
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &key_of,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    let bd = BrepData::from_brep(&k, &b, &env, &mut Vec::new()).unwrap();
    let ie = bl::edges(&b, None)
        .unwrap()
        .iter()
        .position(|&x| x == e)
        .unwrap();
    let iv = bl::vertices(&b, None)
        .unwrap()
        .iter()
        .position(|&x| x == v)
        .unwrap();
    assert_eq!(
        bd.edges[ie].zone_tol_3d.to_bits(),
        ea.to_bits(),
        "edge kept raised"
    );
    assert_eq!(
        bd.vertices[iv].zone_tol_3d.to_bits(),
        va.to_bits(),
        "vertex kept raised"
    );

    // the read of the written file keeps every trim that passes at the held tolerances
    let kept = bl::edges(&b, None)
        .unwrap()
        .into_iter()
        .flat_map(|e| edge::edgeuses(&b, e, None, None).unwrap())
        .filter(|&eu| {
            eu1::uv_trim_curve_pointer(&b, eu).unwrap().is_some()
                && bl::check_uv_trim_curve(&k, &b, eu).is_ok()
        })
        .count();
    assert!(kept <= before_trims);
    let bytes = write(&k, &b);
    let r = read(&mut k, bytes);
    assert_eq!(trims(&r), kept);
}

/// A raise goes back to its tolerance from before only while that is no tighter than the
/// B-rep's tolerance, or the B-rep still holds the tolerance the raise was made at: after
/// the B-rep's tolerance is raised (only where larger) between a raised edge's two values,
/// the edge still holds its raise but is written raised, not below the B-rep's tolerance
/// (review of #21705).
#[test]
fn a_raise_is_not_written_below_a_later_body_tolerance() {
    use solidcore::topology::brep::p2;
    use solidcore::topology::brep_data::BrepData;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Fillet/sphere_diff_3cylinders.smb");
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        eprintln!("unavailable upstream test file: {}", path.display());
        return;
    };
    let mut k = Kernel::default();
    let mut b = read(&mut k, bytes);
    let raises = b.trim_cover_raises().clone();
    assert_eq!(raises.body_tolerance.to_bits(), b.tolerance().to_bits());
    // the held raised edge with the largest raise
    let (e, eb, ea) = bl::edges(&b, None)
        .unwrap()
        .into_iter()
        .filter_map(|e| {
            let &(eb, ea) = raises.edges.get(&e)?;
            let held = b.edge(e).unwrap().tolerance.to_bits() == ea.to_bits();
            (held && eb < ea).then_some((e, eb, ea))
        })
        .max_by(|x, y| x.2.total_cmp(&y.2))
        .expect("the input's read raises an edge");
    let t = 0.5 * (eb + ea);
    assert!(eb < t && t < ea && b.tolerance() < t);
    p2::set_tolerance(&k, &mut b, t, true, true).unwrap();
    assert_eq!(b.tolerance().to_bits(), t.to_bits());
    assert_eq!(b.edge(e).unwrap().tolerance.to_bits(), ea.to_bits());
    assert_eq!(b.trim_cover_raises().edges.get(&e), Some(&(eb, ea)));

    let store = AttributeStore::new();
    let records = NotifyRecords::new();
    let key_of = |x: EntityRef| b.owner_identity(x);
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &key_of,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    let bd = BrepData::from_brep(&k, &b, &env, &mut Vec::new()).unwrap();
    let ie = bl::edges(&b, None)
        .unwrap()
        .iter()
        .position(|&x| x == e)
        .unwrap();
    assert_eq!(bd.edges[ie].zone_tol_3d.to_bits(), ea.to_bits());
    for ed in &bd.edges {
        assert!(
            ed.zone_tol_3d >= t,
            "an edge written below the B-rep's tolerance"
        );
    }
    for vd in &bd.vertices {
        assert!(
            vd.zone_tol_3d >= t,
            "a vertex written below the B-rep's tolerance"
        );
    }
}

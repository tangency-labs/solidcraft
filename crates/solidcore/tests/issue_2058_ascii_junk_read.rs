//! tangency-labs/solidcore#2058: `read_brep_from_file` with `ascii = true` read the 16
//! bytes `not a B-rep file` as a B-rep with no faces, edges or vertices, where the
//! binary read of the same bytes is an error.
//!
//! Upstream (`SmBrepData::ReadFromDB`) extracts the shell count into the uninitialised
//! local `lCount`. On such a stream the extraction stores nothing (the stream has
//! already ended) and `m_vShells.SetSize(lCount)` runs on an indeterminate value
//! (undefined behaviour: the issue saw the C++ read crash once). The port read it as
//! `0`. It now refuses the stream with `InvalidInput`, as a short binary read is, and
//! so it does wherever upstream extracts into an uninitialised local: the version
//! number, `dMin`/`dMax` of an edge, a composite edge's count and first edge, and an
//! old composite curve's values. A valid ASCII file reads as before.
#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::api_files::{read_brep_from_file, write_brep_to_file};
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{EdgeId, EdgeuseId, FaceId};
use solidcore::{Brep, Kernel, Point3, SolidError};

/// `read_brep_from_file` of `bytes` under the B-rep's own owner identities.
fn read(k: &mut Kernel, bytes: &[u8], ascii: bool) -> solidcore::Result<Brep> {
    let mut attributes = AttributeStore::new();
    let mut records = NotifyRecords::new();
    let fresh = Brep::new();
    let key_of = |e| fresh.owner_identity(e);
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key_of,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(bytes.to_vec()), ascii, false, &mut env)?;
    b.refresh_cached_topology();
    Ok(b)
}

/// A 10 x 20 x 30 box written as an ASCII (`ascii`) or binary database file.
fn box_file(k: &mut Kernel, ascii: bool) -> Vec<u8> {
    let b = create_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
    let store = AttributeStore::new();
    let records = NotifyRecords::new();
    let key_of = |e| b.owner_identity(e);
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &key_of,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    write_brep_to_file(k, Some(&b), &env, ascii).unwrap()
}

/// (faces, edges, vertices) reached by the topology walks.
fn counts(b: &Brep) -> [usize; 3] {
    [
        bl::faces(b, None).unwrap().len(),
        bl::edges(b, None).unwrap().len(),
        bl::vertices(b, None).unwrap().len(),
    ]
}

/// `InvalidInput`, with the faces of a B-rep read by mistake in the message.
fn assert_invalid_input(r: solidcore::Result<Brep>, what: &str) {
    assert!(
        matches!(r, Err(SolidError::InvalidInput { .. })),
        "{what}: {:?}",
        r.map(|b| counts(&b))
    );
}

#[test]
fn ascii_junk_is_refused() {
    let mut k = Kernel::default();
    assert_invalid_input(read(&mut k, b"not a B-rep file", true), "ascii junk");
    // as the binary read of the same bytes is
    assert_invalid_input(read(&mut k, b"not a B-rep file", false), "binary junk");
}

#[test]
fn ascii_without_data_is_refused() {
    let mut k = Kernel::default();
    // nothing: version 30, then no shell count
    assert_invalid_input(read(&mut k, b"", true), "empty");
    // a version header without its number
    assert_invalid_input(read(&mut k, b"Version\n", true), "header without number");
    // only the header lines of a valid file: no shell count
    let file = box_file(&mut k, true);
    let text = std::str::from_utf8(&file).unwrap();
    assert!(
        text.starts_with("Version "),
        "{}",
        &text[..40.min(text.len())]
    );
    let header: usize = text.split_inclusive('\n').take(2).map(str::len).sum();
    assert_invalid_input(read(&mut k, &file[..header], true), "header only");
}

#[test]
fn ascii_box_still_reads() {
    let mut k = Kernel::default();
    for ascii in [true, false] {
        let file = box_file(&mut k, ascii);
        let b = read(&mut k, &file, ascii).unwrap();
        assert_eq!(counts(&b), [6, 12, 8], "ascii {ascii}");
    }
}

#[test]
fn ascii_box_cut_short_never_reads_a_partial_box() {
    let mut k = Kernel::default();
    let file = box_file(&mut k, true);
    let text = std::str::from_utf8(&file).unwrap();
    let mut end = 0;
    let mut refused = 0;
    for line in text.split_inclusive('\n') {
        end += line.len();
        match read(&mut k, &file[..end], true) {
            Ok(b) => assert_eq!(counts(&b), [6, 12, 8], "cut after byte {end}"),
            Err(_) => refused += 1,
        }
    }
    // every cut before the vertex records is refused
    assert!(refused > 0);
}

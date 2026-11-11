//! The flat encoding of a body ([`goldens::flat`]): its header and vertices, the round trip
//! through the validating decoder, and the refusals, on the hand-written tetrahedron and disk
//! file that `solidcore-record`'s import tests read.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test

use goldens::flat;
use step_export::{import, Limits};

#[test]
fn flat_encoding_of_the_hand_written_file() {
    let text = include_str!("../../step-export/src/tetra_disk.stp");
    let m = import(text.as_bytes(), &Limits::default()).unwrap();
    let t = &m.bodies[0];
    // The flat encoding starts with its header and covers every vertex.
    let flat = flat::encode(t);
    assert_eq!(&flat[..7], &[21.0, 1.0, 0.0, 4.0, 6.0, 4.0, 1.0]);
    assert_eq!(&flat[7..10], &[0.0, 0.0, 0.0]);
    // The encoding round-trips through the validating decoder.
    for b in &m.bodies {
        let e = flat::encode(b);
        assert_eq!(flat::encode(&flat::decode(&e).unwrap()), e);
    }
    // Truncations, bad indices and non-finite numbers are refused.
    for cut in 0..flat.len() {
        assert!(flat::decode(&flat[..cut]).is_err());
    }
    let mut bad = flat.clone();
    bad[3] = 1e18;
    assert!(flat::decode(&bad).is_err());
    let mut bad = flat.clone();
    bad[8] = f64::NAN;
    assert!(flat::decode(&bad).is_err());
}

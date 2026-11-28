//! The rows of `topology.brep_p7` that a copy keeping its face's trims moves (tests only;
//! WR-722, #18923 part 1): `CopyFaces` without the merge passes each source face's UV trims
//! (`CreateCurvesFromFace`) to `MakeFaceWithCurves` with `pbOptCrvOnSurf` set, so the copy's
//! edge uses hold the source's trims where main left them empty. The rows write whole bodies
//! (`p7`'s destination graph), and the move is a UV trim block. A row is certified as
//! [`crate::operand_trim_rows::trim_blocks`] certifies its rows:
//! * **replay**: the case's Rust side, run again, has the output digest recorded for this
//!   branch, so the run is the row;
//! * **only trims moved**: run again with every edge use's trim cleared on each body the unit
//!   writes, just before it is written, its output has the digest main's (59cc18ae6c) code
//!   has run alike, so every value but the trim blocks is main's, bit for bit;
//! * **trims on edges**: every trim each written body holds lies on its edge within the
//!   edge's tolerance ([`crate::trims_on_edges`], rounded interval arithmetic), so each kept
//!   trim is the edge's own.
//!
//! Run: `cargo test --profile goldens -p goldens --lib copied_trim_rows -- --nocapture`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;
use std::rc::Rc;

use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{edge, Brep};

use crate::operand_trim_rows::trim_blocks::{certify, Row};
use crate::trim_hook::with_hook;
use crate::UnitDiff;

// From main's (59cc18ae6c) and this branch's runs: (case, this branch's output digest,
// main's output digest with trims cleared).
const ROWS: &[Row] = &[
    Row {
        case: "copy_disk.smb_0",
        new: 0x7fc4f89e33eafe2b,
        main: 0xe2fcbfbf1463a62f,
    },
    Row {
        case: "copy_plane_circ.smb_0",
        new: 0x987b3a663ed0a4f2,
        main: 0x485a35cd10abc7f8,
    },
    Row {
        case: "copy_plane_square.smb_0",
        new: 0x1e6d75d2407ed264,
        main: 0xa13bbb11488a4bcb,
    },
    Row {
        case: "copy_repeat2_disk.smb",
        new: 0x3567ab23e20d7e6d,
        main: 0x82f52c688ef60b12,
    },
    Row {
        case: "copy_repeat2_plane_circ.smb",
        new: 0x37b382fb4d133e76,
        main: 0xd69e93fae058e58a,
    },
    Row {
        case: "copy_repeat2_plane_square.smb",
        new: 0x758d62b2ad77a600,
        main: 0xd5f5d73e5d69fba1,
    },
    Row {
        case: "copy_repeat3_disk.smb",
        new: 0x8636f969ee189e7d,
        main: 0x0bfad9441954a521,
    },
    Row {
        case: "copy_repeat3_plane_circ.smb",
        new: 0x34e38fe02160a58d,
        main: 0xf9d8fdb03bf6201c,
    },
    Row {
        case: "copy_repeat3_plane_square.smb",
        new: 0xc1ee6b95cce53187,
        main: 0x8682cbbac06254d8,
    },
];

/// The 9 rows.
#[test]
fn copied_trim_rows_are_certified() {
    let trims = certify(&crate::units::topology_brep_p7::Runner, ROWS);
    eprintln!("{} rows, {trims} trims", ROWS.len());
    assert_eq!(ROWS.len(), 9);
}

/// Every trim the rows' bodies hold shares its edge's interval, bit for bit
/// (`docs/validity-and-tolerances.md`: "A pcurve shares its edge's interval"): the copy keeps
/// the source trim's parameterization, not only its image, which the certificate above
/// matches by nearest parameters.
#[test]
fn copied_trims_share_their_edges_interval() {
    let runner = crate::units::topology_brep_p7::Runner;
    for row in ROWS {
        let case = crate::case::regression_case(&runner, row.case);
        let seen: Rc<RefCell<(usize, Vec<String>)>> = Rc::default();
        let sink = Rc::clone(&seen);
        with_hook(
            Box::new(move |b: &mut Brep| {
                let mut s = sink.borrow_mut();
                let ids: Vec<_> = b.edgeuses().ids().collect();
                for eu in ids {
                    let Some(trim) = b.edgeuse(eu).unwrap().uv_trim_curve.as_ref() else {
                        continue;
                    };
                    let e = eu1::edge(b, eu)
                        .unwrap()
                        .expect("a trimmed edge use has an edge");
                    let (t, i) = (trim.natural_interval(), edge::interval(b, e).unwrap());
                    s.0 += 1;
                    if t.min().to_bits() != i.min().to_bits()
                        || t.max().to_bits() != i.max().to_bits()
                    {
                        s.1.push(format!(
                            "edge use {}: trim [{:e}, {:e}], edge [{:e}, {:e}]",
                            eu.index(),
                            t.min(),
                            t.max(),
                            i.min(),
                            i.max()
                        ));
                    }
                }
            }),
            || runner.run_rust(&case),
        )
        .unwrap();
        let s = seen.borrow();
        eprintln!("{}: {} trims share their edges' intervals", row.case, s.0);
        assert!(s.0 > 0, "{}: no trims", row.case);
        assert!(s.1.is_empty(), "{}: {:?}", row.case, s.1);
    }
}

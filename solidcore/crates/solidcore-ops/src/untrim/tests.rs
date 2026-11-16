//! Untrim against closed forms (moved from SolidCraft's untrim tests), on the corpus's bodies:
//! a 20 x 20 sheet with a round hole r 4 (`cov-untrim-sheet-hole-in-place`), the block
//! 20 x 20 x 10 with that hole through it (`cov-untrim-solid-top-face-copied-out`, its record face
//! 0 the top z = 10) and a quarter cylinder r 10, 20 high, with a window
//! (`cov-untrim-cylinder-sheet-hole-in-place`). In each record, face 0's outer loop has edges
//! 0-3 and its hole edge 4 (4-7 on the cylinder). The corpus's own cases are checked against the
//! rig's area and counts.
// The closed forms use pi; the tests compute them, not ops.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::panic)]

use std::f64::consts::PI;

use super::*;
use crate::test_corpus::{area, entry, operand, raw_counts, rel, rig, rig_fixes, valid};

const SHEET: &str = "surfaces/cov-untrim-sheet-hole-in-place";
const BLOCK: &str = "surfaces/cov-untrim-solid-top-face-copied-out";
const QUARTER: &str = "surfaces/cov-untrim-cylinder-sheet-hole-in-place";

/// The whole square, and the square less the hole.
const SQUARE: f64 = 400.0;
const HOLED: f64 = 400.0 - 16.0 * PI;

fn sheet() -> (Body, Order) {
    operand(SHEET, "b1")
}

fn block() -> (Body, Order) {
    operand(BLOCK, "b12")
}

/// One face of `want` area, valid.
fn one_face(what: &str, b: &Body, want: f64) {
    rig_fixes();
    valid(what, b);
    assert_eq!(b.counts().unwrap()[0], 1, "{what}: faces");
    let a = area(b);
    assert!(rel(a, want) < 1e-9, "{what}: area {a} vs {want}");
}

#[test]
fn a_sheets_holes_go_in_place() {
    let (b, o) = sheet();
    let all = untrim_face(&b, &o, 0, Holes::All, false, true).unwrap();
    one_face("every hole", &all, SQUARE);
    let one = untrim_face(&b, &o, 0, Holes::Along(&[4]), false, true).unwrap();
    one_face("the hole along edge 4", &one, SQUARE);
}

/// The outer boundary back on the plane's box of the face's span, the hole kept or gone.
#[test]
fn a_sheets_outer_boundary_goes_back() {
    let (b, o) = sheet();
    let kept = untrim_face(&b, &o, 0, Holes::Along(&[]), true, true).unwrap();
    one_face("hole kept", &kept, HOLED);
    let gone = untrim_face(&b, &o, 0, Holes::All, true, true).unwrap();
    one_face("holes gone", &gone, SQUARE);
    assert_eq!(gone.counts().unwrap(), [1, 4, 4]);
}

/// A solid's top face copied out, its hole taken away or its outer boundary put back.
#[test]
fn a_solids_face_comes_out() {
    let (b, o) = block();
    let all = untrim_face(&b, &o, 0, Holes::All, false, false).unwrap();
    one_face("every hole", &all, SQUARE);
    let one = untrim_face(&b, &o, 0, Holes::Along(&[4]), false, false).unwrap();
    one_face("the hole along edge 4", &one, SQUARE);
    let kept = untrim_face(&b, &o, 0, Holes::Along(&[]), true, false).unwrap();
    one_face("hole kept", &kept, HOLED);
    let gone = untrim_face(&b, &o, 0, Holes::All, true, false).unwrap();
    one_face("holes gone", &gone, SQUARE);
}

/// In the solid, the hole's edges are the bore's too: no hole of a sheet.
#[test]
fn a_hole_of_a_solid_stays_in_place() {
    let (b, o) = block();
    assert!(matches!(
        untrim_face(&b, &o, 0, Holes::All, false, true),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn bad_asks_are_refused() {
    let (b, o) = sheet();
    let refused = |face, holes, outer| untrim_face(&b, &o, face, holes, outer, true).err();
    assert_eq!(
        refused(1, Holes::All, false),
        Some(Error::Argument),
        "no face 1"
    );
    assert_eq!(
        refused(0, Holes::Along(&[]), false),
        Some(Error::Argument),
        "nothing to do"
    );
    assert_eq!(
        refused(0, Holes::Along(&[99]), false),
        Some(Error::Argument),
        "no edge 99"
    );
    // edge 0 is on the outer loop, no hole
    assert_eq!(
        refused(0, Holes::Along(&[0]), false),
        Some(Error::Argument),
        "edge on no hole"
    );
}

/// The body untrimmed is never changed.
#[test]
fn the_body_stays_as_it_was() {
    let (b, o) = block();
    let was = b.counts().unwrap();
    let _ = untrim_face(&b, &o, 0, Holes::All, true, false);
    let _ = untrim_face(&b, &o, 0, Holes::All, false, true);
    assert_eq!(b.counts().unwrap(), was);
}

/// A corpus case: the traced untrim on its operand, against the rig's area and the result
/// record's counts.
fn corpus_case(case: &str, operand_name: &str) {
    rig_fixes();
    let e = entry(case);
    let call = e["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["op"] == "untrim_face")
        .unwrap();
    let p = &call["params"];
    assert!(p["holes"].is_null(), "{case}: every hole");
    let face = usize::try_from(p["face"]["picked"][0]["index"].as_u64().unwrap()).unwrap();
    let (outer, in_place) = (
        p["outer"].as_bool().unwrap(),
        p["in_place"].as_bool().unwrap(),
    );
    let (b, o) = operand(case, operand_name);
    let r = untrim_face(&b, &o, face, Holes::All, outer, in_place).unwrap();
    valid(case, &r);
    let (a, m) = (area(&r), rig(&e));
    println!(
        "{case}: area {a} (rig {}), counts {:?} (rig raw {:?})",
        m.area,
        r.counts().unwrap(),
        raw_counts(&e)
    );
    assert!(rel(a, m.area) < 1e-9, "{case}: area {a} vs rig {}", m.area);
    assert_eq!(r.counts().unwrap(), raw_counts(&e), "{case}: counts");
}

/// The whole square, 400, one plane face with its 4 outer edges free.
#[test]
fn corpus_untrim_sheet_hole_in_place() {
    corpus_case(SHEET, "b1");
}

/// The block's top copied out without its hole: 400.
#[test]
fn corpus_untrim_solid_top_face_copied_out() {
    corpus_case(BLOCK, "b12");
}

/// The quarter cylinder whole again: (pi / 2) 10 20 = 100 pi, its two arcs and two rulings.
#[test]
fn corpus_untrim_cylinder_sheet_hole_in_place() {
    corpus_case(QUARTER, "b10");
    let (b, o) = operand(QUARTER, "b10");
    let r = untrim_face(&b, &o, 0, Holes::All, false, true).unwrap();
    assert!(rel(area(&r), 100.0 * PI) < 1e-9);
}

/// A body whose face and edge lists are not in id order (as an importer may leave them): the
/// record's face and hole edge are the body's own, not those of a renumbered copy.
#[test]
fn lists_out_of_id_order_untrim_the_face_asked() {
    let (b, o) = block();
    {
        let mut g = b.lock();
        let mut faces = g.face_list().to_vec();
        faces.reverse();
        g.set_face_list(faces).unwrap();
        let mut edges = g.edge_list().to_vec();
        edges.reverse();
        g.set_edge_list(edges).unwrap();
    }
    let o = Order {
        faces: o.faces.iter().rev().copied().collect(),
        edges: o.edges.iter().rev().copied().collect(),
        vertices: o.vertices.clone(),
    };
    let (top, hole) = (o.faces.len() - 1, o.edges.len() - 1 - 4);
    let one = untrim_face(&b, &o, top, Holes::Along(&[hole]), false, false).unwrap();
    one_face("the hole along the reversed edge 4", &one, SQUARE);
    let kept = untrim_face(&b, &o, top, Holes::Along(&[]), true, false).unwrap();
    one_face("the reversed top, hole kept", &kept, HOLED);
}

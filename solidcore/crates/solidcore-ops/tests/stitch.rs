//! The stitch and merge family on boxes.
#![allow(clippy::unwrap_used)]

use solidcore_ops::stitch::{gather, open_edges, sew, stitch};
use solidcore_ops::sweep::{planar_faces, Curve};
use solidcore_ops::{make_box, session, Body, Error};

fn all_fixes() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
}

#[test]
fn a_closed_box_stitches_to_itself_closed() {
    all_fixes();
    let b = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]).unwrap();
    let (s, closed) = stitch(b, 1e-6).unwrap();
    assert!(closed);
    assert_eq!(s.counts().unwrap(), [6, 12, 8]);
    for t in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let b = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
        assert_eq!(stitch(b, t).unwrap_err(), Error::Argument, "{t}");
    }
}

/// The planar sheet bounded by the closed polygon through `corners`.
fn sheet(corners: &[[f64; 3]]) -> Body {
    let next = corners.iter().cycle().skip(1);
    let sides: Vec<Curve> = corners
        .iter()
        .zip(next)
        .map(|(&start, &end)| Curve::Line { start, end })
        .collect();
    planar_faces(&sides).unwrap()
}

/// The square of side `side` in z = 0 whose lower x is `x0`.
fn square(x0: f64, side: f64) -> Body {
    let x1 = x0 + side;
    sheet(&[
        [x0, 0.0, 0.0],
        [x1, 0.0, 0.0],
        [x1, side, 0.0],
        [x0, side, 0.0],
    ])
}

/// The record edges of `body` one face uses once, from its record (each edge's uses), and the
/// record edges whose both ends have x in `[lo, hi]`.
fn record_edges(body: &Body, lo: f64, hi: f64) -> (Vec<usize>, Vec<usize>) {
    let (rec, _) = body.to_record().unwrap();
    let x = |v: u32| rec.vertices.get(v as usize).unwrap().point[0];
    let edges = rec.edges.iter().enumerate();
    let one_use = edges
        .clone()
        .filter(|(_, e)| e.uses.len() == 1)
        .map(|(i, _)| i)
        .collect();
    let across = edges
        .filter(|(_, e)| e.vertices.iter().all(|&v| (lo..=hi).contains(&x(v))))
        .map(|(i, _)| i)
        .collect();
    (one_use, across)
}

#[test]
fn two_squares_sharing_an_edge_sew_into_one_open_sheet() {
    all_fixes();
    let (a, b) = (square(0.0, 1.0), square(1.0, 1.0));
    let gathered = gather(&[&a, &b]).unwrap();
    assert_eq!(gathered.counts().unwrap()[..2], [2, 8]);
    assert_eq!(open_edges(&gathered).unwrap(), (0..8).collect::<Vec<_>>());
    let sewn = sew(&[&a, &b], 1e-6).unwrap();
    assert!(!sewn.closed);
    assert_eq!(sewn.body.counts().unwrap()[..2], [2, 7]);
    let (one_use, shared) = record_edges(&sewn.body, 1.0, 1.0);
    assert_eq!(sewn.open_edges.len(), 6);
    assert_eq!(sewn.open_edges, one_use);
    assert_eq!(shared.len(), 1);
    assert!(shared.iter().all(|e| !sewn.open_edges.contains(e)));
    // the inputs are unchanged, and a body given twice is taken once
    assert_eq!(a.counts().unwrap()[..2], [1, 4]);
    assert_eq!(gather(&[&a, &a]).unwrap().counts().unwrap()[..2], [1, 4]);
    assert_eq!(gather(&[]).unwrap_err(), Error::Argument);
}
#[test]
fn a_boxs_six_faces_as_six_sheets_sew_closed() {
    all_fixes();
    let p = |x: f64, y: f64, z: f64| [x, y, z];
    let faces = [
        sheet(&[p(0., 0., 0.), p(0., 1., 0.), p(1., 1., 0.), p(1., 0., 0.)]),
        sheet(&[p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.), p(0., 1., 1.)]),
        sheet(&[p(0., 0., 0.), p(1., 0., 0.), p(1., 0., 1.), p(0., 0., 1.)]),
        sheet(&[p(0., 1., 0.), p(0., 1., 1.), p(1., 1., 1.), p(1., 1., 0.)]),
        sheet(&[p(0., 0., 0.), p(0., 0., 1.), p(0., 1., 1.), p(0., 1., 0.)]),
        sheet(&[p(1., 0., 0.), p(1., 1., 0.), p(1., 1., 1.), p(1., 0., 1.)]),
    ];
    let refs: Vec<&Body> = faces.iter().collect();
    let sewn = sew(&refs, 1e-6).unwrap();
    assert!(sewn.closed);
    assert!(sewn.open_edges.is_empty());
    assert_eq!(sewn.body.counts().unwrap(), [6, 12, 8]);
}

/// Squares of side 10 with a gap of 0.5 between them: a tolerance a fifth of the gap sews
/// nothing; one twice the gap, a tenth of the side, sews the two edges across the gap into one.
#[test]
fn sheets_half_apart_sew_only_at_a_tolerance_past_the_gap() {
    all_fixes();
    let (side, gap) = (10.0, 0.5);
    let (a, b) = (square(0.0, side), square(side + gap, side));
    let apart = sew(&[&a, &b], gap / 5.0).unwrap();
    assert!(!apart.closed);
    assert_eq!(apart.body.counts().unwrap()[..2], [2, 8]);
    assert_eq!(apart.open_edges, (0..8).collect::<Vec<_>>());
    assert_eq!(apart.open_edges, record_edges(&apart.body, 0.0, 0.0).0);
    let joined = sew(&[&a, &b], 2.0 * gap).unwrap();
    assert!(!joined.closed);
    assert_eq!(joined.body.counts().unwrap()[..2], [2, 7]);
    let (one_use, across) = record_edges(&joined.body, side, side + gap);
    assert_eq!(joined.open_edges.len(), 6);
    assert_eq!(joined.open_edges, one_use);
    assert_eq!(across.len(), 1);
    assert!(across.iter().all(|e| !joined.open_edges.contains(e)));
    for t in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(sew(&[&a, &b], t).unwrap_err(), Error::Argument, "{t}");
    }
}

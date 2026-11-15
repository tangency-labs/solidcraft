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

/// The unit square in z = 0 whose lower x is `x0`.
fn unit_square(x0: f64) -> Body {
    let x1 = x0 + 1.0;
    sheet(&[
        [x0, 0.0, 0.0],
        [x1, 0.0, 0.0],
        [x1, 1.0, 0.0],
        [x0, 1.0, 0.0],
    ])
}

#[test]
fn two_squares_sharing_an_edge_sew_into_one_open_sheet() {
    all_fixes();
    let (a, b) = (unit_square(0.0), unit_square(1.0));
    let gathered = gather(&[&a, &b]).unwrap();
    assert_eq!(gathered.counts().unwrap()[..2], [2, 8]);
    assert_eq!(open_edges(&gathered).unwrap().len(), 8);
    let sewn = sew(&[&a, &b], 1e-6).unwrap();
    assert!(!sewn.closed);
    assert_eq!(sewn.body.counts().unwrap()[..2], [2, 7]);
    assert_eq!(sewn.open_edges.len(), 6);
    assert_eq!(sewn.open_edges, open_edges(&sewn.body).unwrap());
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

#[test]
fn sheets_half_apart_sew_only_at_a_tolerance_past_the_gap() {
    all_fixes();
    let (a, b) = (unit_square(0.0), unit_square(1.5));
    let apart = sew(&[&a, &b], 0.1).unwrap();
    assert!(!apart.closed);
    assert_eq!(apart.open_edges.len(), 8);
    let joined = sew(&[&a, &b], 1.0).unwrap();
    assert_eq!(joined.open_edges.len(), 6);
    for t in [0.0, -1.0, f64::NAN] {
        assert_eq!(sew(&[&a, &b], t).unwrap_err(), Error::Argument, "{t}");
    }
}

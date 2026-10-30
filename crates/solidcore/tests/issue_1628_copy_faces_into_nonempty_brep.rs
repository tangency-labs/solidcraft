//! Issue #1628: `p7::copy_faces` without the merge into a B-rep that already has
//! faces failed on the second copy ("notified object is not a topology object") and
//! left the destination half-copied. Upstream `SmBrep::CopyFaces(faces, dest, NULL,
//! FALSE)` copies any number of times into one B-rep.

use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::geom::vector3d::Vec3;
use solidcore::mass::integrator::properties::brep_area;
use solidcore::query::api_queries::get_edges;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::p7::copy_faces;
use solidcore::topology::Brep;
use solidcore::{EntityRef, FaceId, Kernel, Result};

/// The four sides of the 10 x 10 square at height `z`.
fn square(z: f64) -> Result<Vec<Curve>> {
    let p = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    p.iter()
        .zip(p.iter().cycle().skip(1))
        .map(|(a, b)| {
            let line = Line::create_line_segment(
                3,
                Vec3::new(a[0], a[1], z),
                Vec3::new(b[0], b[1], z),
                None,
            )?;
            Ok(Curve::from(line))
        })
        .collect()
}

/// Copies the walls of a 10 x 10 x 10 open tube based at `z` into `dest`, with the
/// notification environment `PrimitiveModel::copy_faces_into` builds; the result of
/// the copy.
fn copy_tube(k: &mut Kernel, dest: &mut Brep, z: f64) -> Result<(Vec<FaceId>, bool)> {
    let s = square(z)?;
    let refs: Vec<Option<&Curve>> = s.iter().map(Some).collect();
    let mut piece = create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 10.0, false)?;
    let faces = bp1::faces(&piece, None)?;
    let mut state = ArenaNotifyState::new(dest);
    state.seed(&piece);
    let mut env = state.env(None);
    let key_of = env.key_of;
    let this_face = |f| key_of(EntityRef::Face(f));
    copy_faces(k, &mut piece, &faces, dest, false, &this_face, &mut env)
}

#[test]
fn copy_faces_twice_into_one_brep() {
    let mut k = Kernel::default();
    let mut dest = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    for z in [0.0, 10.0] {
        let r = copy_tube(&mut k, &mut dest, z);
        assert!(
            matches!(r, Ok((ref made, true)) if made.len() == 4),
            "z = {z}: {r:?}"
        );
    }
    assert_eq!(bp1::faces(&dest, None).unwrap().len(), 8);

    stitch_and_orient(&mut k, &mut dest, &StitchAndOrientOptions::default()).unwrap();
    assert_eq!(bp1::faces(&dest, None).unwrap().len(), 8);
    // the walls of a 10 x 10 x 20 open tube
    let area = brep_area(&k, &mut dest, 1e-9).unwrap();
    assert!((area - 800.0).abs() < 1e-6, "area {area}");
    // stitched into one tube: 4 vertical edges per piece and 4 rings, the middle
    // ring shared by both pieces
    assert_eq!(get_edges(&dest).unwrap().len(), 2 * 4 + 3 * 4);
}

#[test]
fn copy_faces_three_times_into_one_brep() {
    let mut k = Kernel::default();
    let mut dest = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    for z in [0.0, 10.0, 20.0] {
        let r = copy_tube(&mut k, &mut dest, z);
        assert!(
            matches!(r, Ok((ref made, true)) if made.len() == 4),
            "z = {z}: {r:?}"
        );
    }
    assert_eq!(bp1::faces(&dest, None).unwrap().len(), 12);

    stitch_and_orient(&mut k, &mut dest, &StitchAndOrientOptions::default()).unwrap();
    assert_eq!(bp1::faces(&dest, None).unwrap().len(), 12);
    let area = brep_area(&k, &mut dest, 1e-9).unwrap();
    assert!((area - 1200.0).abs() < 1e-6, "area {area}");
    assert_eq!(get_edges(&dest).unwrap().len(), 3 * 4 + 4 * 4);
}

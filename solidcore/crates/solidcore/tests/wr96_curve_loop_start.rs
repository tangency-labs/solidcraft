//! WR-96: later loops start after the previous loop's inclusive end.

use solidcore::topology::trimming_tools::curve_loops::CurveLoops;
use solidcore::Kernel;

#[test]
fn later_loop_ranges_are_disjoint() {
    let keys: Vec<_> = (0..8)
        .map(solidcore::topology::trimming_tools::curve_loops::CurveKey)
        .collect();
    let loops = CurveLoops::from_loops(&[3, 4, 1], None, Some(&keys), &[]);
    for (index, range) in [
        (0, (0, 2)),
        (2, (0, 2)),
        (3, (3, 6)),
        (4, (3, 6)),
        (6, (3, 6)),
        (7, (7, 7)),
    ] {
        assert_eq!(loops.loop_start_end(index).unwrap(), range);
    }
    assert!(loops.loop_start_end(8).is_err());
    assert!(CurveLoops::new().loop_start_end(0).is_err());
}

#[test]
fn adjacency_and_sampling_wrap_within_the_second_loop() {
    use solidcore::curve::curve::Curve;
    use solidcore::curve::line::Line;
    use solidcore::math::Point3;
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::topology::trimming_tools::curve_loops::CurveKey;

    let store: Vec<Curve> = (0..7)
        .map(|i| {
            let x = f64::from(i);
            Curve::from(Line::from_points(
                Point3::new(x, 0.0, 0.0),
                Point3::new(x, 1.0, 0.0),
                3,
            ))
        })
        .collect();
    let keys: Vec<_> = (0..7).map(CurveKey).collect();
    let loops = CurveLoops::from_loops(&[3, 4], None, Some(&keys), &[OrientType::Same; 7]);
    let k = Kernel::default();
    // The second loop is curves 3..=6: before its first curve comes its last, and
    // after its last comes its first.
    let (previous, next) = (6, 3);
    assert_eq!(
        loops
            .find_adjacent_loop_curve(&store, CurveKey(3), 0.0)
            .unwrap(),
        previous
    );
    assert_eq!(
        loops
            .find_adjacent_loop_curve(&store, CurveKey(6), 1.0)
            .unwrap(),
        next
    );
    let (_, before) = loops
        .sample_nearby_points(&k, &store, CurveKey(3), 0.0)
        .unwrap();
    let (_, after) = loops
        .sample_nearby_points(&k, &store, CurveKey(6), 1.0)
        .unwrap();
    assert_eq!(before.x, previous as f64);
    assert_eq!(after.x, next as f64);
}

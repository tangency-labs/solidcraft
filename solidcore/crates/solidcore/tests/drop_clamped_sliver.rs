//! A dropped UV trim does not fold back over a sliver of its edge that lies just outside
//! the face's domain.
//!
//! On `Fillet/sphere_diff_3cylinders.smb`, edge 30 starts about `2.1e-8` below `v = 0`
//! of face 7 (domain `[0, 1]²`) and reaches the boundary at `t = 0.5`, a 3D piece about
//! `2.6e-6` long. The drop's first span covers that sliver. Its polygon had both ends
//! clamped onto `v = 0` while its interior points kept the unclamped tangent
//! (`v = 0, +7e-9, -7e-9, 5.6e-15`): an S that the B-spline check reports. The span is
//! now left out as a clamped sliver and the next span stretched over its range
//! (`intersect::surface_drop_lib::clamped_sliver`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::assert_array::{AssertArray, ObjectKey};
use solidcore::geom::attribute::AttributeStore;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::edgeuse::p1::face;
use solidcore::topology::edgeuse::p3::create_uv_trim_curve;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::EdgeuseId;
use solidcore::{Brep, EdgeId, Kernel};

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

/// The control points of a B-spline curve in Euclidean form, with its knots.
fn polygon(c: &Curve) -> (Vec<(f64, f64)>, Vec<f64>) {
    let mut c = c.clone();
    let CurveKind::BSpline(s) = c.kind_mut() else {
        panic!("the dropped trim is a B-spline");
    };
    let n = s.get_or_create_gw_nurb().unwrap();
    let points = n
        .cpts()
        .iter()
        .map(|q| {
            let e = q.to_euclid();
            (e.x, e.y)
        })
        .collect();
    (points, n.knots().to_vec())
}

#[test]
fn the_trim_over_the_sliver_is_monotone_and_passes_the_curve_checks() {
    let mut k = Kernel::default();
    let mut b = read(&mut k, "Fillet/sphere_diff_3cylinders.smb");
    let eu = EdgeuseId::from_index(142);
    let edge = b.edgeuse(eu).unwrap().edge.unwrap();
    assert_eq!(edge, EdgeId::from_index(30));
    let f = face(&b, eu).unwrap().unwrap();
    assert_eq!(f.index(), 7);
    let interval = b.edge(edge).unwrap().interval;

    let tolerance = b.edge(edge).unwrap().tolerance;
    let made = create_uv_trim_curve(&k, &mut b, eu, false, None).unwrap();

    // the trim's image stays within the edge's tolerance of the edge (the stretched
    // span is accepted only where its enclosure proves this; this is the drop's own
    // measure of it)
    assert!(
        made.max_dist_to_surf <= tolerance,
        "{} > {tolerance}",
        made.max_dist_to_surf
    );

    // the trim still covers the whole edge
    let ivl = made.curve.natural_interval();
    assert_eq!(ivl.min(), interval.min());
    assert_eq!(ivl.max(), interval.max());

    // The first span's polygon is monotone in v and inside v >= 0, so the span is
    // (variation diminishing): no S at the start.
    let (points, knots) = polygon(&made.curve);
    assert_eq!(knots[0], interval.min());
    let first: Vec<f64> = points[..4].iter().map(|p| p.1).collect();
    assert!(first.iter().all(|&v| v >= 0.0), "{first:?}");
    assert!(
        first.windows(2).all(|w| w[0] <= w[1]) || first.windows(2).all(|w| w[0] >= w[1]),
        "{first:?}"
    );

    // The B-spline check on the face raises none of the vanishing first derivative (11),
    // radius of curvature (13) or coincident control point (15) tests the S raised.
    let surface = b.face(f).unwrap().surface.as_ref();
    let CurveKind::BSpline(s) = made.curve.kind() else {
        panic!("the dropped trim is a B-spline");
    };
    let mut list = AssertArray::new();
    s.assert_valid_on(&k, Some(&mut list), true, ObjectKey(5), surface)
        .unwrap();
    let failed: Vec<_> = (0..list.len())
        .filter_map(|i| list.get(i))
        .filter(|r| !r.ok)
        .map(|r| r.test_index)
        .collect();
    for test in [11, 13, 15] {
        assert!(!failed.contains(&test), "test {test} fails: {failed:?}");
    }
}

/// A sliver whose end shared with its neighbour lies outside the domain is kept.
///
/// The goldens' `surface.plane_p1` gen0059 drops a rational cubic onto a bounded plane
/// whose domain corner `(u0, v0)` the curve starts beyond, in both coordinates. The
/// first span (`t` in `[0, 1/128]`) clamps to that corner bar a fold of its polygon's
/// second point to `v0 + 0.0356`; its end at `t = 1/128` is outside the domain too, so
/// the second span's first point was clamped onto the corner as well. Between the two
/// spans the flush moves that knot's neighbours onto the corner (an interior knot on the
/// boundary), which makes the second span one point. Stretched over the first span's
/// range, the second span became the first and kept its unclamped tangent (an end knot's
/// neighbour is only clamped): a fold to `v0 + 0.0363`, larger than the sliver's. The
/// sliver stays, and so does the second span's single point.
#[test]
fn a_sliver_whose_neighbour_was_clamped_too_is_kept() {
    use solidcore::geom::extent1d::Extent1d;
    use solidcore::geom::extent2d::Extent2d;
    use solidcore::geom::vector2d::Vec2;
    use solidcore::geom::vector3d::Vec3;
    use solidcore::nurbs::b_spline_curve::BSplineCurve;
    use solidcore::nurbs::math::CPoint;
    use solidcore::surface::plane::Plane;

    let k = Kernel::default();
    let (u0, v0) = (-3.939751354186126, -2.2663521374286457);
    let domain = Extent2d::from_bounds(u0, v0, 1.4990071576082489, 3.416408169619536);
    let plane = Plane::new(
        Vec3::new(-5.506926495872477, -5.321096480489718, 2.0483477950681728),
        Vec3::new(0.6832481332704575, 0.514387417094488, -0.5182447042829248),
        Vec3::new(0.6603198186441991, -0.7382291802009027, 0.13782385354355428),
        Vec2::new(1.0, 1.0),
        &domain,
    );
    let pw = [
        [
            -6.071826408556264,
            -3.757976352056512,
            1.248345966817094,
            0.5563792854718375,
        ],
        [
            -6.316452876656257,
            -5.431616967682362,
            2.14903822675801,
            0.5922278942243393,
        ],
        [
            -6.242326367663992,
            -8.216051640221346,
            4.188315303320852,
            1.9490570872199018,
        ],
        [
            -6.536393280361178,
            -6.482309386164908,
            3.0338705324600865,
            1.4995658539132009,
        ],
        [
            -4.895568399450589,
            -6.662479819186284,
            2.227089209672163,
            1.0772178787446909,
        ],
    ]
    .map(|q| CPoint {
        x: q[0],
        y: q[1],
        z: q[2],
        w: q[3],
    });
    let knots = [0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0];
    let curve = Curve::from(BSplineCurve::from_data(3, 4, 8, 3, &pw, &knots).unwrap());

    let natural = plane.natural_uv_domain();
    let dropped = plane
        .drop_curve(
            &k,
            &natural,
            &curve,
            &Extent1d::from_min_max(0.0, 1.0),
            2.0,
            false,
            true,
            None,
        )
        .unwrap();
    let (points, knots) = polygon(&dropped.uv_curves[0]);
    let corner = (u0, v0);
    assert_eq!(knots[4], 1.0 / 128.0, "{knots:?}");
    assert!(points[3..=6].iter().all(|&p| p == corner), "{points:?}");
}

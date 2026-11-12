//! tangency-labs/solidcore#3139: a skin says which profile curves each side face was
//! swept from, traced through its construction (`create_skin_primitive_with_sources`).
//!
//! Each wall is checked by closed form: where a wall holds one curve of each of two
//! sections, its surface at the start of the loft (u min) and across the middle of its
//! profile span passes through that curve's midpoint in the first section, and at the
//! end of the loft (u max) through the matching curve's midpoint in the second.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::curve::curve::Curve;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::primitive::api_curves::create_arc;
use solidcore::sweep::api_surfaces::{
    create_skin_primitive, create_skin_primitive_with_sources, NotTraced, SkinPrimitive,
    SkinSources,
};
use solidcore::topology::face::p2 as face2;
use solidcore::topology::FaceId;
use solidcore::Kernel;

fn line(p: [f64; 3], q: [f64; 3]) -> Curve {
    let p = Point3::new(p[0], p[1], p[2]);
    let q = Point3::new(q[0], q[1], q[2]);
    Curve::from(BSplineCurve::create_line_segment(3, p, q).unwrap())
}

/// The square's sides at height `z`, scaled by `s` about the axis, in the order given
/// by `order`; `reversed` sides run backwards.
fn square(z: f64, s: f64, order: [usize; 4], reversed: &[usize]) -> Vec<Curve> {
    let c = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
    order
        .iter()
        .map(|&i| {
            let (a, b) = (c[i], c[(i + 1) % 4]);
            let (a, b) = if reversed.contains(&i) {
                (b, a)
            } else {
                (a, b)
            };
            line([s * a[0], s * a[1], z], [s * b[0], s * b[1], z])
        })
        .collect()
}

fn midpoint(k: &Kernel, c: &Curve) -> Point3 {
    let ivl = c.natural_interval();
    c.evaluate_point(k, 0.5 * (ivl.min() + ivl.max())).unwrap()
}

fn skin(
    k: &mut Kernel,
    counts: &[usize],
    curves: &[Curve],
    cap: bool,
) -> (SkinPrimitive, SkinSources) {
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    // every body: the traced skin is the plain one, bit for bit (each on a fresh kernel)
    let plain = create_skin_primitive(&mut Kernel::default(), counts, &refs, 1, cap).unwrap();
    let (again, _) =
        create_skin_primitive_with_sources(&mut Kernel::default(), counts, &refs, 1, cap).unwrap();
    same_skin(&plain, &again);
    create_skin_primitive_with_sources(k, counts, &refs, 1, cap).unwrap()
}

/// The two skins have the same faces, in the same roles, on the same surfaces bit for bit
/// (Debug prints each f64 exactly).
fn same_skin(plain: &SkinPrimitive, traced: &SkinPrimitive) {
    assert_eq!(plain.start_faces, traced.start_faces);
    assert_eq!(plain.side_faces, traced.side_faces);
    assert_eq!(plain.end_faces, traced.end_faces);
    let faces = solidcore::topology::brep::p1::faces(&plain.brep, None).unwrap();
    assert_eq!(
        faces,
        solidcore::topology::brep::p1::faces(&traced.brep, None).unwrap()
    );
    for f in faces {
        let a = format!("{:?}", face2::surface(&plain.brep, f).unwrap());
        let b = format!("{:?}", face2::surface(&traced.brep, f).unwrap());
        assert_eq!(a, b, "{f:?}");
    }
}

fn traced(s: SkinSources) -> Vec<(FaceId, Vec<usize>)> {
    match s {
        SkinSources::Traced(walls) => walls,
        SkinSources::NotTraced { why } => panic!("not traced: {why:?}"),
    }
}

/// A wall holding curve `a` of the first section and `b` of the second passes through
/// their midpoints at its two ends, across the middle of its span.
fn wall_runs_from(k: &Kernel, skin: &SkinPrimitive, f: FaceId, a: &Curve, b: &Curve) {
    let surface = face2::surface(&skin.brep, f).unwrap().unwrap();
    let d = surface.natural_uv_domain_with_nurb(k).unwrap();
    for (u, c) in [(0.0, a), (1.0, b)] {
        let p = surface.evaluate_point(k, d.evaluate(u, 0.5)).unwrap();
        let m = midpoint(k, c);
        assert!(
            p.distance(m) <= 1e-9 * (1.0 + m.length()),
            "{f:?} at u {u}: {p:?} vs {m:?}"
        );
    }
}

#[test]
fn a_shuffled_square_s_walls_are_its_sides() {
    let mut k = Kernel::default();
    // sides 2, 0, 3, 1, side 3 reversed; the second section the same, scaled (a frustum)
    let mut curves = square(0.0, 1.0, [2, 0, 3, 1], &[3]);
    curves.extend(square(5.0, 0.5, [2, 0, 3, 1], &[3]));
    let (made, sources) = skin(&mut k, &[4, 4], &curves, true);
    let walls = traced(sources);
    assert_eq!(walls.len(), 4, "{walls:?}");
    assert_eq!(
        walls.iter().map(|(f, _)| *f).collect::<Vec<_>>(),
        made.side_faces
    );
    let mut seen = Vec::new();
    for (f, curves_of) in &walls {
        // one curve of each section, the same side of the square
        let [a, b] = curves_of[..] else {
            panic!("{f:?}: {curves_of:?}")
        };
        assert_eq!(b, a + 4, "{f:?}: {curves_of:?}");
        wall_runs_from(&k, &made, *f, &curves[a], &curves[b]);
        seen.push(a);
    }
    seen.sort_unstable();
    assert_eq!(seen, [0, 1, 2, 3]);
}

#[test]
fn the_traced_skin_is_the_skin() {
    let mut curves = square(0.0, 1.0, [2, 0, 3, 1], &[3]);
    curves.extend(square(5.0, 0.5, [2, 0, 3, 1], &[3]));
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let plain = create_skin_primitive(&mut Kernel::default(), &[4, 4], &refs, 1, true).unwrap();
    let (traced, _) =
        create_skin_primitive_with_sources(&mut Kernel::default(), &[4, 4], &refs, 1, true)
            .unwrap();
    same_skin(&plain, &traced);
}

#[test]
fn sections_with_different_knots_are_not_traced() {
    let mut k = Kernel::default();
    // the second section's top side is two lines: five curves, knots 0..5 against 0..4
    let mut curves = square(0.0, 1.0, [0, 1, 2, 3], &[]);
    curves.extend([
        line([-1.0, -1.0, 5.0], [1.0, -1.0, 5.0]),
        line([1.0, -1.0, 5.0], [1.0, 1.0, 5.0]),
        line([1.0, 1.0, 5.0], [0.0, 1.0, 5.0]),
        line([0.0, 1.0, 5.0], [-1.0, 1.0, 5.0]),
        line([-1.0, 1.0, 5.0], [-1.0, -1.0, 5.0]),
    ]);
    let (made, sources) = skin(&mut k, &[4, 5], &curves, true);
    assert!(!made.side_faces.is_empty());
    assert_eq!(
        sources,
        SkinSources::NotTraced {
            why: NotTraced::SectionsNotKnotIdentical
        }
    );
}

#[test]
fn a_kinked_curve_spans_a_wall_per_kink() {
    let mut k = Kernel::default();
    // each section's square is one closed degree-1 B-spline: one curve, four corners
    let closed = |z: f64| {
        let pts = [
            [-1.0, -1.0],
            [1.0, -1.0],
            [1.0, 1.0],
            [-1.0, 1.0],
            [-1.0, -1.0],
        ]
        .map(|[x, y]| Point3::new(x, y, z));
        let mut sides: Vec<BSplineCurve> = (0..4)
            .map(|i| BSplineCurve::create_line_segment(3, pts[i], pts[i + 1]).unwrap())
            .collect();
        Curve::from(BSplineCurve::create_by_joining(&mut sides, None).unwrap())
    };
    let curves = [closed(0.0), closed(5.0)];
    let (made, sources) = skin(&mut k, &[1, 1], &curves, true);
    let walls = traced(sources);
    assert_eq!(walls.len(), 4, "{walls:?}");
    assert_eq!(made.side_faces.len(), 4);
    for (_, curves_of) in &walls {
        assert_eq!(curves_of, &[0, 1]);
    }
}

#[test]
fn a_rounded_square_s_tangent_joins_still_split_walls() {
    // The faces split at knots of continuity G1 or less judged on the knots, so each
    // join (all tangent here) is a split: eight curves, eight walls, each one curve's.
    let mut k = Kernel::default();
    // sides along x = +-1, y = +-1 between the corner arcs of radius 0.5
    let section = |z: f64| -> Vec<Curve> {
        let r = 0.5;
        let e = 1.0 - r;
        let arc = |cx: f64, cy: f64, a0: f64| {
            Curve::from(create_arc(Point3::new(cx, cy, z), r, a0, a0 + 90.0).unwrap())
        };
        vec![
            line([-e, -1.0, z], [e, -1.0, z]),
            arc(e, -e, 270.0),
            line([1.0, -e, z], [1.0, e, z]),
            arc(e, e, 0.0),
            line([e, 1.0, z], [-e, 1.0, z]),
            arc(-e, e, 90.0),
            line([-1.0, e, z], [-1.0, -e, z]),
            arc(-e, -e, 180.0),
        ]
    };
    let mut curves = section(0.0);
    curves.extend(section(5.0));
    let (made, sources) = skin(&mut k, &[8, 8], &curves, true);
    let walls = traced(sources);
    assert_eq!(walls.len(), 8, "{walls:?}");
    assert_eq!(made.side_faces.len(), 8);
    let mut seen = Vec::new();
    for (f, curves_of) in &walls {
        let [a, b] = curves_of[..] else {
            panic!("{f:?}: {curves_of:?}")
        };
        assert_eq!(b, a + 8, "{f:?}: {curves_of:?}");
        wall_runs_from(&k, &made, *f, &curves[a], &curves[b]);
        seen.push(a);
    }
    seen.sort_unstable();
    assert_eq!(seen, (0..8).collect::<Vec<_>>(), "{walls:?}");
}

#[test]
fn a_holed_square_s_inner_walls_are_the_hole_s_sides() {
    let mut k = Kernel::default();
    let section = |z: f64| {
        let mut c = square(z, 2.0, [0, 1, 2, 3], &[]);
        c.extend(square(z, 0.5, [1, 3, 0, 2], &[]));
        c
    };
    let mut curves = section(0.0);
    curves.extend(section(5.0));
    let (made, sources) = skin(&mut k, &[8, 8], &curves, true);
    let walls = traced(sources);
    assert_eq!(walls.len(), 8, "{walls:?}");
    let mut seen = Vec::new();
    for (f, curves_of) in &walls {
        let [a, b] = curves_of[..] else {
            panic!("{f:?}: {curves_of:?}")
        };
        assert_eq!(b, a + 8, "{f:?}: {curves_of:?}");
        wall_runs_from(&k, &made, *f, &curves[a], &curves[b]);
        seen.push(a);
    }
    seen.sort_unstable();
    assert_eq!(seen, (0..8).collect::<Vec<_>>());
}

#[test]
fn open_profiles_are_not_traced() {
    let mut k = Kernel::default();
    let curves = [
        line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        line([1.0, 0.0, 0.0], [1.0, 1.0, 0.0]),
        line([0.0, 0.0, 5.0], [1.0, 0.0, 5.0]),
        line([1.0, 0.0, 5.0], [1.0, 1.0, 5.0]),
    ];
    let (_, sources) = skin(&mut k, &[2, 2], &curves, false);
    assert_eq!(
        sources,
        SkinSources::NotTraced {
            why: NotTraced::OpenProfiles
        }
    );
}

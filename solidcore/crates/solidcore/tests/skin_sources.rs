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
    create_skin_primitive, create_skin_primitive_with_sources, SkinPrimitive, SkinSources,
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

/// The distance from `p` to line segment `c` (its points at its interval's ends).
fn off_segment(k: &Kernel, c: &Curve, p: Point3) -> f64 {
    let ivl = c.natural_interval();
    let a = c.evaluate_point(k, ivl.min()).unwrap();
    let b = c.evaluate_point(k, ivl.max()).unwrap();
    let d = b - a;
    let t = ((p - a).dot(d) / d.dot(d)).clamp(0.0, 1.0);
    p.distance(a + d * t)
}

/// Sections whose knots differ (tangency-labs/solidcraft#13632): a square of four sides
/// against one whose top side is two lines, five curves. The skin rescales both sections'
/// knots to [0, 1] and gives each the other's, so the trace runs through each section's
/// knot map. Where the loops start and how they are parametrised are the planar faces' and
/// the join's choices, so the walls are checked by what those cannot change: each wall holds
/// at least one curve of each section, every curve is on some wall, and, by closed form,
/// across the middle of each wall's span its surface lies on one of its first section's
/// curves at the start of the loft and on one of its second's at the end, within the B-rep's
/// tolerance. The two collinear halves meet straight on, a join the skin does not split at,
/// so one wall holds both. The walls themselves are pinned as run fmt-13632-3 traced them.
#[test]
fn sections_whose_knots_differ_are_traced_through_the_skin_s_maps() {
    let mut k = Kernel::default();
    let mut curves = square(0.0, 1.0, [0, 1, 2, 3], &[]);
    curves.extend([
        line([-1.0, -1.0, 5.0], [1.0, -1.0, 5.0]),
        line([1.0, -1.0, 5.0], [1.0, 1.0, 5.0]),
        line([1.0, 1.0, 5.0], [0.0, 1.0, 5.0]),
        line([0.0, 1.0, 5.0], [-1.0, 1.0, 5.0]),
        line([-1.0, 1.0, 5.0], [-1.0, -1.0, 5.0]),
    ]);
    let (made, sources) = skin(&mut k, &[4, 5], &curves, true);
    let walls = traced(sources);
    assert_eq!(
        walls.iter().map(|(f, _)| *f).collect::<Vec<_>>(),
        made.side_faces
    );
    // on a curve within the B-rep's own tolerance (`Brep::tolerance`, upstream
    // `SmBrep::GetTolerance`, its zone tolerance): the distance under which the kernel
    // takes two points as one, so the kernel's own test of a point lying on a curve
    let tol = made.brep.tolerance();
    let mut seen = [false; 9];
    for (f, of) in &walls {
        let (first, second): (Vec<usize>, Vec<usize>) = of.iter().partition(|&&c| c < 4);
        assert!(!first.is_empty() && !second.is_empty(), "{f:?}: {of:?}");
        for &c in of {
            seen[c] = true;
        }
        let surface = face2::surface(&made.brep, *f).unwrap().unwrap();
        let d = surface.natural_uv_domain_with_nurb(&k).unwrap();
        for (u, these) in [(0.0, &first), (1.0, &second)] {
            let p = surface.evaluate_point(&k, d.evaluate(u, 0.5)).unwrap();
            let off = these
                .iter()
                .map(|&c| off_segment(&k, &curves[c], p))
                .fold(f64::INFINITY, f64::min);
            assert!(
                off <= tol,
                "{f:?} at u {u}: {p:?} is {off} off {these:?} (tol {tol})"
            );
        }
    }
    assert!(seen.iter().all(|&s| s), "{walls:?}");
    let got: Vec<Vec<usize>> = walls.iter().map(|(_, of)| of.clone()).collect();
    assert_eq!(
        got,
        [
            vec![3, 8],
            vec![3, 4],
            vec![0, 4],
            vec![0, 5],
            vec![1, 5],
            vec![1, 6],
            vec![2, 6, 7]
        ]
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

/// Open profiles (tangency-labs/solidcraft#13632): each is lofted as one composite, which
/// the builder chains end to end, here taking the second section's curves out of order and
/// one reversed. The builder says which curve each segment is, so each wall holds one curve
/// of each section: the ones whose segments share its span. Each wall is checked by closed
/// form, its surface through the two curves' midpoints at the loft's two ends.
#[test]
fn open_profiles_are_traced_through_the_builder_s_segments() {
    let mut k = Kernel::default();
    let curves = [
        line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        line([1.0, 0.0, 0.0], [1.0, 1.0, 0.0]),
        // the second section from its other end, its first curve given second and reversed
        line([1.0, 1.0, 5.0], [1.0, 0.0, 5.0]),
        line([0.0, 0.0, 5.0], [1.0, 0.0, 5.0]),
    ];
    let (made, sources) = skin(&mut k, &[2, 2], &curves, false);
    let walls = traced(sources);
    assert_eq!(
        walls.iter().map(|(f, _)| *f).collect::<Vec<_>>(),
        made.side_faces
    );
    let mut pairs = Vec::new();
    for (f, of) in &walls {
        let [a, b] = of[..] else {
            panic!("{f:?}: {of:?}")
        };
        wall_runs_from(&k, &made, *f, &curves[a], &curves[b]);
        pairs.push((a, b));
    }
    pairs.sort_unstable();
    // the builder chains the second section from (1, 1): its first segment is curve 2
    assert_eq!(pairs, [(0, 2), (1, 3)]);
}

/// #15263: the end sections share z = 0, and the middle square spans z = 5..15.
/// Both caps of a valid arch must face down. Check shell orientation separately
/// from the volume sign so a local reversal cannot masquerade as an inside-out solid.
#[test]
fn a_skin_returning_to_its_start_plane_has_two_outward_downward_caps() {
    use solidcore::mass::integrator::properties::brep_volume_default;
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::nurbs::knots::Side;
    use solidcore::topology::{brep::p2 as brep2, edgeuse::p1 as eu1};

    let sections = [
        [[0., 0., 0.], [10., 0., 0.], [10., 10., 0.], [0., 10., 0.]],
        [
            [15., 0., 5.],
            [15., 10., 5.],
            [15., 10., 15.],
            [15., 0., 15.],
        ],
        [[20., 0., 0.], [30., 0., 0.], [30., 10., 0.], [20., 10., 0.]],
    ];
    for joined_profiles in [false, true] {
        let mut k = Kernel::default();
        let curves: Vec<Curve> = sections
            .iter()
            .flat_map(|p| {
                let mut sides: Vec<_> = (0..4)
                    .map(|i| {
                        BSplineCurve::create_line_segment(
                            3,
                            Point3::new(p[i][0], p[i][1], p[i][2]),
                            Point3::new(p[(i + 1) % 4][0], p[(i + 1) % 4][1], p[(i + 1) % 4][2]),
                        )
                        .unwrap()
                    })
                    .collect();
                if joined_profiles {
                    vec![Curve::from(
                        BSplineCurve::create_by_joining(&mut sides, None).unwrap(),
                    )]
                } else {
                    sides.into_iter().map(Curve::from).collect()
                }
            })
            .collect();
        let refs: Vec<_> = curves.iter().map(Some).collect();
        let count = if joined_profiles { 1 } else { 4 };
        let (mut made, sources) =
            create_skin_primitive_with_sources(&mut k, &[count; 3], &refs, 2, true).unwrap();
        assert_eq!(made.brep.face_list().len(), 6);
        assert_eq!(made.side_faces.len(), 4);
        assert_eq!(made.start_faces.len(), 1);
        assert_eq!(made.end_faces.len(), 1);
        let walls = traced(sources);
        assert_eq!(walls.len(), 4);
        for (_, sources) in walls {
            assert_eq!(sources.len(), 3, "{sources:?}");
        }
        assert!(brep2::is_manifold_solid(&made.brep).unwrap());
        // A reversed cap would disagree with its neighbouring edge uses. Check the
        // stitched exterior shell directly, independently of its signed volume.
        for &f in made.brep.face_list() {
            let outer = face2::outer_faceuse(&made.brep, f).unwrap().unwrap();
            for eu in eu1::faceuse_edgeuses(&made.brep, outer, false, None).unwrap() {
                let radial = eu1::radial(&made.brep, eu).unwrap().unwrap();
                assert_ne!(
                    eu1::orientation(&made.brep, eu).unwrap(),
                    eu1::orientation(&made.brep, radial).unwrap(),
                    "inconsistent exterior edge uses on {f:?}"
                );
            }
        }
        let mut cap_normals = Vec::new();
        for &f in made.start_faces.iter().chain(&made.end_faces) {
            let surface = face2::surface(&made.brep, f).unwrap().unwrap();
            let d = surface.natural_uv_domain_with_nurb(&k).unwrap();
            let uv = d.evaluate(0.5, 0.5);
            let p = surface.evaluate_point(&k, uv).unwrap();
            let mut n = surface
                .evaluate_normal(&k, uv, Side::Left, Side::Left)
                .unwrap();
            let outer = face2::outer_faceuse(&made.brep, f).unwrap().unwrap();
            if eu1::faceuse_orientation(&made.brep, outer).unwrap() == OrientType::Opposite {
                n = -n;
            }
            eprintln!("cap {f:?}: point {p:?}, outward normal {n:?}");
            assert!(p.z.abs() < 1e-9, "cap point {p:?}");
            cap_normals.push(n);
        }
        assert!(
            cap_normals.iter().all(|n| n.z < -0.999999),
            "cap outward normals {cap_normals:?}"
        );
        let v = brep_volume_default(&k, &mut made.brep, 1e-9).unwrap();
        eprintln!("joined profiles {joined_profiles}: volume {v} mm3");
        // Untwisted quadratic boundaries enclose 800/3 mm2, across a 10 mm width.
        // The historical ~1338 mm3 is not the volume of this untwisted quadratic.
        assert!((v - 8000.0 / 3.0).abs() < 1e-5, "signed volume {v}");
        assert!(solidcore::topology::brep::is_outward_solid(&mut k, &mut made.brep).unwrap());
    }
}

//! WR-698 (#2273, the fork's 014f66f, c6d9b9a and 0136260): a procedural blend evaluates
//! as its definition, a station solve and the section exact in t, with the kernel's
//! station memo; its file record is written and read back (type 41 only with the flags).
//!
//! The geometry: a ball of radius 2 between the plane y = 0 (side -1) and the plane
//! z = 0 (side +1), or a biquadratic bump over z = 0 (centre control point 0.6 high),
//! along the leader (0, 2, 2) -> (10, 2, 2). The values were checked against the fork
//! (tangency-labs/smlib@0136260, `SMLib` and `SM_API` built locally with the harness flags)
//! through `SmProceduralBlend::Create`, `EvaluateDefinition`, `GetCounts`, `WriteToDB` and
//! `ReadFromDB` on the same inputs: every partial (3564 values over 11 definitions, 12
//! points each, to order 2 in s and t), the counts and the binary records agreed. The
//! values quoted below are the fork's. Since 3bc54a4 an order-n definition's jets of
//! order n are the face curves' normal parts: the bump's G^n definitions are fork
//! 3bc54a4's (all 3564 values again bit for bit), the others are unchanged. The circle's points are also checked against the
//! closed form.

#![allow(clippy::unwrap_used)]

use solidcore::blend::fillet_section::section::{GnSectionFsg, GN_THUMBWEIGHT_EQUAL_AREA};
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::math::Point2;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, KnotType};
use solidcore::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
};
use solidcore::surface::procedural_blend::{
    BlendCounts, BlendDefinitionInput, ProceduralBlend, PROCEDURAL_BLEND_TYPE,
};
use solidcore::surface::surface::Surface;
use solidcore::topology::database_io::{DatabaseReader, DatabaseWriter, FileType};
use solidcore::topology::geometry_write::write_surface;

const L: f64 = 10.0;
const R: f64 = 2.0;

fn kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_gn_section_generator = true;
    k.fixes.fillet_round_as_definition = true;
    k
}

fn bilinear(p: [Point3; 4]) -> BSplineSurface {
    BSplineSurface::create_bilinear_surface(p[0], p[1], p[2], p[3]).unwrap()
}

fn bump(h: f64) -> BSplineSurface {
    let mut cps = Vec::new();
    for i in 0..3 {
        for j in 0..3 {
            let z = if i == 1 && j == 1 { h } else { 0.0 };
            cps.push(Point3::new(
                f64::from(i) * L / 2.0,
                f64::from(j) * L / 2.0,
                z,
            ));
        }
    }
    BSplineSurface::create_canonical(&CanonicalSurfaceData {
        u_degree: 2,
        v_degree: 2,
        control_points: &cps,
        form: BSplineSurfaceForm::Unspecified,
        u_multiplicities: &[3, 3],
        v_multiplicities: &[3, 3],
        u_knots: &[0.0, 1.0],
        v_knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: None,
        uv_domain: None,
    })
    .unwrap()
}

fn line(a: Point3, b: Point3) -> BSplineCurve {
    BSplineCurve::create_line_segment(3, a, b).unwrap()
}

/// The definition: `bumpy` puts the bump in place of z = 0, `law` the radius
/// 1.5 + s (s over [0, 1]).
fn blend(k: &Kernel, bumpy: bool, order: u32, thumbweight: f64, law: bool) -> ProceduralBlend {
    let o = Point3::new(0.0, 0.0, 0.0);
    let a = Surface::from(bilinear([
        o,
        Point3::new(L, 0.0, 0.0),
        Point3::new(0.0, 0.0, L),
        Point3::new(L, 0.0, L),
    ]));
    let b = if bumpy {
        Surface::from(bump(0.6))
    } else {
        Surface::from(bilinear([
            o,
            Point3::new(L, 0.0, 0.0),
            Point3::new(0.0, L, 0.0),
            Point3::new(L, L, 0.0),
        ]))
    };
    let net = bilinear([
        o,
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
    ]);
    let leader = line(Point3::new(0.0, R, R), Point3::new(L, R, R));
    let seed = line(Point3::new(0.0, R / L, 0.0), Point3::new(1.0, R / L, 0.0));
    let radius = line(Point3::new(1.5, 0.0, 0.0), Point3::new(2.5, 0.0, 0.0));
    let input = BlendDefinitionInput {
        supports: [&a, &b],
        sides: [-1.0, 1.0],
        offsets: [R, R],
        law: law.then_some(&radius),
        order,
        thumbweight,
        leader: &leader,
        seeds: [&seed, &seed],
        bound: 1e-3,
    };
    ProceduralBlend::create(k, &net, &input, |n, w| {
        GnSectionFsg::is_valid(n as usize, w)
    })
    .unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * 1f64.max(a.abs()).max(b.abs())
}

fn assert_vec(got: Vec3, want: [f64; 3], what: &str) {
    assert!(
        close(got.x, want[0]) && close(got.y, want[1]) && close(got.z, want[2]),
        "{what}: {got:?} != {want:?}"
    );
}

/// At (s, t) = (0.37, 0.6): the point, d^2 S / ds dt and d^4 S / ds^2 dt^2, the fork's.
#[test]
fn definitions_evaluate_as_the_fork_s() {
    let k = kernel();
    let ea = GN_THUMBWEIGHT_EQUAL_AREA;
    #[allow(clippy::type_complexity)]
    let cases: [(&str, bool, u32, f64, bool, [[f64; 3]; 3]); 7] = [
        (
            "plane n 2 w 0",
            false,
            2,
            0.0,
            false,
            [
                [3.6999999999999988, 0.92571428571428493, 0.18285714285714266],
                [0.0; 3],
                [0.0; 3],
            ],
        ),
        (
            "plane n 3 w -1",
            false,
            3,
            -1.0,
            false,
            [
                [3.6999999999999997, 1.2525773195876284, 0.18851251840942546],
                [0.0; 3],
                [0.0; 3],
            ],
        ),
        (
            "plane n 2 equal area",
            false,
            2,
            ea,
            false,
            [
                [3.6999999999999993, 1.0735924639518113, 0.23047636434570989],
                [0.0; 3],
                [0.0; 3],
            ],
        ),
        (
            "bump n 2 equal area",
            true,
            2,
            ea,
            false,
            [
                [3.7076153013462654, 1.1247965039773156, 0.29932533840732245],
                [
                    -0.3431805027283312,
                    0.15952786530177496,
                    0.04843807232410644,
                ],
                [0.18400915253383338, 7.530381731203525, -3.6365459706073375],
            ],
        ),
        (
            "bump n 3 w 0",
            true,
            3,
            0.0,
            false,
            [
                [3.706489982768767, 1.044145558011038, 0.19006839035877368],
                [-0.3133140632937431, 0.17260627567208248, 0.1103965265929085],
                [0.09460235306688654, 8.697164266958284, -2.5690245545045434],
            ],
        ),
        (
            "bump n 1",
            true,
            1,
            0.0,
            false,
            [
                [3.7166223003140217, 0.87103605536466744, 0.43990796265579407],
                [
                    -0.14685906050567388,
                    0.10685015971002275,
                    0.012807829671101237,
                ],
                [
                    0.09921158399671709,
                    -0.14682964597773029,
                    -2.3572242028696042,
                ],
            ],
        ),
        (
            "bump n 1 law",
            true,
            1,
            0.0,
            true,
            [
                [3.7927539614653258, 0.81750870003218679, 0.41335652579648763],
                [
                    -0.16045050294581342,
                    1.4665607799716496,
                    -0.93913899925185074,
                ],
                [1.0741110766659812, -0.11777436551962284, -2.195182493991612],
            ],
        ),
    ];
    for (name, bumpy, n, w, law, want) in cases {
        let pb = blend(&k, bumpy, n, w, law);
        let d = pb.evaluate_definition(&k, 0.37, 0.6, 2, 2).unwrap();
        assert_vec(d[0][0], want[0], name);
        // (the fork's d[4] and d[8] beyond the point are compared where they are not
        // rounding noise: the planes' are zero to 1e-14)
        if bumpy {
            assert_vec(d[1][1], want[1], name);
            assert_vec(d[2][2], want[2], name);
        } else {
            assert!(
                d[1][1].length() < 1e-12 && d[2][2].length() < 1e-12,
                "{name}"
            );
        }
    }
}

/// The circle between the planes: about (x, 2, 2), radius 2, from the foot on y = 0
/// (t = 0) to the foot on z = 0 (t = 1), x = 10 s (closed form).
#[test]
fn the_circle_between_planes_is_the_ball_s() {
    let k = kernel();
    let pb = blend(&k, false, 1, 0.0, false);
    for s in [0.0, 0.37, 1.0] {
        for t in [0.0, 0.25, 0.6, 1.0] {
            let d = pb.evaluate_definition(&k, s, t, 1, 1).unwrap();
            let p = d[0][0];
            assert!((p.x - L * s).abs() < 1e-12, "{p:?}");
            let r = ((p.y - R).powi(2) + (p.z - R).powi(2)).sqrt();
            assert!((r - R).abs() < 1e-12, "{s} {t}: {r}");
            assert!((d[1][0] - Vec3::new(L, 0.0, 0.0)).length() < 1e-12);
        }
        let d0 = pb.evaluate_definition(&k, s, 0.0, 0, 0).unwrap();
        let d1 = pb.evaluate_definition(&k, s, 1.0, 0, 0).unwrap();
        assert!((d0[0][0] - Vec3::new(L * s, 0.0, R)).length() < 1e-12);
        assert!((d1[0][0] - Vec3::new(L * s, R, 0.0)).length() < 1e-12);
    }
}

/// The station memo: points at one s solve once (the fork's counts: 12 points at 3 s
/// values, 3 solves; on the bump 9 Newton steps, at most 3 in one solve).
#[test]
fn a_row_of_points_solves_its_station_once() {
    let k = kernel();
    let pb = blend(&k, true, 2, GN_THUMBWEIGHT_EQUAL_AREA, false);
    k.blend_counts.set(BlendCounts::default());
    let mut first = Vec::new();
    for s in [0.0, 0.37, 1.0] {
        for t in [0.0, 0.25, 0.6, 1.0] {
            first.push(pb.evaluate_definition(&k, s, t, 2, 2).unwrap());
        }
    }
    let c = k.blend_counts.get();
    assert_eq!(
        (c.evaluations, c.solves, c.newton_steps, c.most_newton_steps),
        (12, 3, 9, 3)
    );
    // a hit gives what a fresh solve gives: a copy (a new serial) solves afresh
    let copy = pb.copy(&k);
    let mut i = 0;
    for s in [0.0, 0.37, 1.0] {
        for t in [0.0, 0.25, 0.6, 1.0] {
            let d = copy.evaluate_definition(&k, s, t, 2, 2).unwrap();
            for (a, b) in d.iter().flatten().zip(first[i].iter().flatten()) {
                assert_eq!(a, b);
            }
            i += 1;
        }
    }
    assert_eq!(k.blend_counts.get().solves, 6);
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for &b in bytes {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The binary record is the fork's (its length and FNV-1a hash), and reads back.
#[test]
fn the_record_is_the_fork_s_and_reads_back() {
    let k = kernel();
    for (bumpy, n, w, law, len, hash) in [
        (false, 1, 0.0, false, 988, 0xa632_9df0_1772_45bd_u64),
        (
            true,
            2,
            GN_THUMBWEIGHT_EQUAL_AREA,
            false,
            1140,
            0x59b3_c770_4a8d_1f3a,
        ),
        (true, 1, 0.0, true, 1250, 0x695b_ae87_4d74_044a),
    ] {
        let surface = Surface::from(blend(&k, bumpy, n, w, law));
        let want = surface.evaluate_point(&k, Point2::new(0.37, 0.6)).unwrap();
        for ft in [FileType::Binary, FileType::Ascii] {
            let mut db = DatabaseWriter::open_for_write(ft);
            write_surface(&surface, &mut db, 30).unwrap();
            let bytes = db.into_bytes();
            if ft == FileType::Binary {
                assert_eq!((bytes.len(), fnv1a(&bytes)), (len, hash));
            }
            let mut r = DatabaseReader::new();
            r.open_for_read(bytes.clone(), ft);
            let back = Surface::read_from_db(&k, PROCEDURAL_BLEND_TYPE, &mut r, 30).unwrap();
            let p = back.evaluate_point(&k, Point2::new(0.37, 0.6)).unwrap();
            // binary is exact; ASCII rounds the doubles it writes
            let tol = if ft == FileType::Binary { 0.0 } else { 1e-9 };
            assert!((p - want).length() <= tol, "{ft:?}: {p:?} != {want:?}");

            // without the flag (or with only one of the two) the type is refused, as the
            // pinned reader refuses it
            for (gn, def) in [(false, false), (true, false), (false, true)] {
                let mut off = Kernel::default();
                off.fixes.fillet_gn_section_generator = gn;
                off.fixes.fillet_round_as_definition = def;
                let mut r = DatabaseReader::new();
                r.open_for_read(bytes.clone(), ft);
                assert!(Surface::read_from_db(&off, PROCEDURAL_BLEND_TYPE, &mut r, 30).is_err());
            }
        }
    }
}

/// A hostile record is refused: a singular map, a number not finite, a negative order.
#[test]
fn hostile_records_are_refused() {
    let k = kernel();
    let surface = Surface::from(blend(&k, false, 2, 0.0, false));
    let mut db = DatabaseWriter::open_for_write(FileType::Binary);
    write_surface(&surface, &mut db, 30).unwrap();
    let good = db.into_bytes();
    // the record opens with the order (8 bytes), two booleans (4 each), then the ten
    // doubles: sides, offsets, thumbweight, bound, a0, b0, a1, b1
    let at = |i: usize| 16 + 8 * i;
    for (offset, value) in [
        (at(7), 0.0_f64),
        (at(5), f64::NAN),
        (at(2), f64::INFINITY),
        (at(0), 0.5),
    ] {
        let mut bad = good.clone();
        bad[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        let mut r = DatabaseReader::new();
        r.open_for_read(bad, FileType::Binary);
        assert!(Surface::read_from_db(&k, PROCEDURAL_BLEND_TYPE, &mut r, 30).is_err());
    }
    let mut bad = good.clone();
    bad[0..8].copy_from_slice(&(-1_i64).to_le_bytes());
    let mut r = DatabaseReader::new();
    r.open_for_read(bad, FileType::Binary);
    assert!(Surface::read_from_db(&k, PROCEDURAL_BLEND_TYPE, &mut r, 30).is_err());
    // a truncated record
    let mut r = DatabaseReader::new();
    r.open_for_read(good[..good.len() / 2].to_vec(), FileType::Binary);
    assert!(Surface::read_from_db(&k, PROCEDURAL_BLEND_TYPE, &mut r, 30).is_err());
}

/// Partials past what the series hold are refused, not overrun.
#[test]
fn partials_past_the_series_are_refused() {
    let k = kernel();
    let pb = blend(&k, false, 4, 0.0, false);
    assert!(pb.evaluate_definition(&k, 0.5, 0.5, 8, 0).is_err());
    assert!(pb.evaluate_definition(&k, 0.5, 0.5, 0, 65).is_err());
    let ea = blend(&k, false, 2, GN_THUMBWEIGHT_EQUAL_AREA, false);
    assert!(ea.evaluate_definition(&k, 0.5, 0.5, 5, 0).is_err());
    assert!(ea.evaluate_definition(&k, 0.5, 0.5, 4, 3).is_ok());
}

/// The `SmSurface` and `SmBSplineSurface` paths that call the blend's virtual
/// evaluators and box (fork 0136260's values on the same definitions, whose net is the
/// unit square z = 0): the box is the net's grown by the bound, so the blend is not
/// planar within 1e-6 although its net is; the degeneracy tests, the largest analytic
/// domain and the extension follow the fork's.
#[test]
fn the_base_class_paths_take_the_blend_s_box_and_points() {
    use solidcore::geom::extent2d::Extent2d;
    use solidcore::geom::geom_utility::p2::ContinuityType;
    use solidcore::math::EFF_ZERO_DEG;
    let k = kernel();
    for (bumpy, n, w, law) in [
        (false, 1, 0.0, false),
        (true, 2, GN_THUMBWEIGHT_EQUAL_AREA, false),
        (true, 1, 0.0, true),
    ] {
        let s = Surface::from(blend(&k, bumpy, n, w, law));
        let nat = s.natural_uv_domain_with_nurb(&k).unwrap();
        let b = s
            .calculate_bounding_box(&k, &nat, [true, false, false], false)
            .unwrap()
            .normal_box
            .unwrap();
        assert!((b.min() - Point3::new(-0.001, -0.001, -0.001)).length() < 1e-15);
        assert!(
            (b.max() - Point3::new(1.0009999999999999, 1.0009999999999999, 0.001)).length() < 1e-15
        );
        assert!(!s.is_planar(&k, 1e-6).unwrap().is_planar);
        assert!(s.is_planar(&k, 0.01).unwrap().is_planar);
        let sub = Extent2d::from_bounds(0.2, 0.2, 0.8, 0.8);
        assert!(!s.is_degenerate_point(&k, 1e-6, None).unwrap());
        assert!(!s.is_degenerate_point(&k, 1e-6, Some(&sub)).unwrap());
        assert!(!s.is_degenerate_curve(&k, EFF_ZERO_DEG, None, 1e-6).unwrap());
        let m = s.max_analytic_domain(&k).unwrap();
        assert_eq!(
            (m.min().x, m.min().y, m.max().x, m.max().y),
            (-100.0, -100.0, 101.0, 101.0)
        );
        let ext = s
            .create_extended_surface(&k, 0.1, ContinuityType::G1)
            .unwrap();
        let ed = ext.natural_uv_domain_with_nurb(&k).unwrap();
        assert!(close(ed.min().x, -0.1) && close(ed.max().y, 1.1));
        let p0 = ext.evaluate_point(&k, ed.min()).unwrap();
        assert!((p0 - Point3::new(-0.1, -0.1, 0.0)).length() < 1e-15);
    }
}

/// Where the fork casts an `SmProceduralBlend` to `SmBSplineSurface` it takes the net
/// (fork 0136260 through `AssertValid`, `SplitAt`, the halves' `CalculateBoundingBox`,
/// `JoinSurface` of the halves as `MoveSeam` joins them, and the `SmBSplineSurface`
/// copy `TurnToNURBS` makes): the record checks out as a B-spline's, the halves are
/// blends whose boxes are the net's grown by the zone tolerance and the bound, and the
/// joined surface is a plain B-spline of the nets. The values are the fork's.
#[test]
fn a_blend_cast_to_a_b_spline_is_its_net() {
    use solidcore::geom::assert_array::ObjectKey;
    use solidcore::geom::tol::ZONE_TOL_3D;
    use solidcore::nurbs::b_spline_surface::SurfParamType;
    use solidcore::surface::surface::SurfaceKind;
    let k = kernel();
    for (bumpy, n, w, law) in [
        (false, 1, 0.0, false),
        (true, 2, GN_THUMBWEIGHT_EQUAL_AREA, false),
        (true, 1, 0.0, true),
    ] {
        let s = Surface::from(blend(&k, bumpy, n, w, law));
        // AssertValid at levels 0 and 2: SmBSplineSurface's, on the net (before, the
        // check refused the blend as "not an SmBSplineSurface")
        assert!(s.assert_valid(&k, None, 0, ObjectKey(1)).unwrap());
        assert!(s.assert_valid(&k, None, 2, ObjectKey(1)).unwrap());
        // SplitAt(0.4, U): two blends (type 15041), boxed as the fork boxes them
        let (l, r) = s.split_at(&k, 0.4, SurfParamType::U).unwrap();
        assert_eq!(
            (l.type_code(), r.type_code()),
            (PROCEDURAL_BLEND_TYPE, PROCEDURAL_BLEND_TYPE)
        );
        let want = [
            (
                [-0.00101, -0.00101, -0.00101],
                [0.40101000000000003, 1.00101, 0.00101],
            ),
            (
                [0.39899000000000001, -0.00101, -0.00101],
                [1.00101, 1.00101, 0.00101],
            ),
        ];
        for (half, (lo, hi)) in [&l, &r].into_iter().zip(want) {
            let dom = half.natural_uv_domain_with_nurb(&k).unwrap();
            let b = half
                .calculate_bounding_box(&k, &dom, [true, false, false], true)
                .unwrap()
                .normal_box
                .unwrap();
            assert_vec(b.min(), lo, "half box min");
            assert_vec(b.max(), hi, "half box max");
        }
        // MoveSeam's JoinSurface of the halves, as SmBSplineSurfaces: a plain B-spline
        let net = |h: Surface| match h.into_kind() {
            SurfaceKind::ProceduralBlend(pb) => pb.net().clone(),
            _ => panic!("not a blend"),
        };
        let (mut nl, mut nr) = (net(l), net(r));
        let j = Surface::from(
            nr.join_surface(&mut nl, SurfParamType::U, ZONE_TOL_3D)
                .unwrap(),
        );
        assert_eq!(j.type_code(), BSplineSurface::TYPE);
        let p = j.evaluate_point(&k, Point2::new(0.3, 0.4)).unwrap();
        assert_vec(
            p,
            [0.38333333333333341, 0.40000000000000008, 0.0],
            "joined point",
        );
    }
}

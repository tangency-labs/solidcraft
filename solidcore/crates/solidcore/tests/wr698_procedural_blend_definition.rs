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

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use solidcore::blend::fillet_section::section::{GnSectionFsg, GN_THUMBWEIGHT_EQUAL_AREA};
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::math::Point2;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, KnotType};
use solidcore::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
};
use solidcore::surface::implicit::{AnalyticSupport, SupportKind};
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
        chamfer: None,
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

            // without the flag the type is refused, as the pinned reader refuses it
            let off = Kernel::default();
            let mut r = DatabaseReader::new();
            r.open_for_read(bytes.clone(), ft);
            assert!(Surface::read_from_db(&off, PROCEDURAL_BLEND_TYPE, &mut r, 30).is_err());
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
/// domain follow the fork's; the extension is the exact one (`extended_sides`).
#[test]
fn the_base_class_paths_take_the_blend_s_box_and_points() {
    use solidcore::error::SolidError;
    use solidcore::geom::extent2d::Extent2d;
    use solidcore::math::EFF_ZERO_DEG;
    use solidcore::surface::extend::{Continuity, SideExtension};
    use solidcore::surface::side::BoxSide;
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
        // the exact extension (`extended_sides`): across (v, the section's t) the toy net
        // continued with its bound grown in closed form; along (u) the toy net is not the
        // construction's cubic Hermite net, so no span of the definition joins it
        // (refused); the equal area G^n section's thumbweight is proved only on a net of
        // its definition (refused)
        let across = s.extended_sides(
            &k,
            &[BoxSide::VMin, BoxSide::VMax],
            Continuity::Tangent,
            0.1,
        );
        if n == 2 {
            assert!(
                matches!(across, Err(SolidError::InvalidInput { .. })),
                "{across:?}"
            );
        } else {
            let ext = across.unwrap();
            assert!(matches!(ext.sides[2], SideExtension::Extended(_)));
            assert!(matches!(ext.sides[3], SideExtension::Extended(_)));
            let ed = ext.surface.natural_uv_domain_with_nurb(&k).unwrap();
            assert!(close(ed.min().y, -0.1) && close(ed.max().y, 1.1), "{ed:?}");
            assert!(ed.min().x == 0.0 && ed.max().x == 1.0, "{ed:?}");
            // the net continued: the unit square's corner moved by 0.1 across
            let solidcore::surface::surface::SurfaceKind::ProceduralBlend(pb) = ext.surface.kind()
            else {
                panic!("not a blend");
            };
            let p0 = pb.net().evaluate_point(ed.min()).unwrap();
            assert!(
                (p0 - Point3::new(0.0, -0.1, 0.0)).length() < 1e-12,
                "{p0:?}"
            );
        }
        for side in [BoxSide::UMin, BoxSide::UMax] {
            let along = s.extended_sides(&k, &[side], Continuity::Tangent, 0.1);
            assert!(
                matches!(along, Err(SolidError::Unsupported { .. })),
                "{along:?}"
            );
        }
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

// ---- #2388: the definition as plain data ----------------------------------------------

use solidcore::surface::plane::Plane;
use solidcore::surface::procedural_blend::convert::{from_record, rebuild, to_record};
use solidcore::surface::procedural_blend::record::{
    ProceduralBlendRecord, RadiusRecord, SupportKindRecord, SupportRecord,
};

/// The record through JSON (as SolidCraft's record would store it), back to the kernel
/// and to a record again: `(json, record back, json back)`.
fn round_trip(k: &Kernel, pb: &ProceduralBlend) -> (String, ProceduralBlend, String) {
    let rec = to_record(k, pb).unwrap();
    let json = serde_json::to_string(&rec).unwrap();
    let read: ProceduralBlendRecord = serde_json::from_str(&json).unwrap();
    let back = from_record(k, &read).unwrap();
    let again = serde_json::to_string(&to_record(k, &back).unwrap()).unwrap();
    (json, back, again)
}

/// The partials of S at a grid of (u, v), as bits.
fn s_bits(k: &Kernel, pb: &ProceduralBlend) -> Vec<Result<[u64; 3], String>> {
    let s = Surface::from(pb.clone());
    let mut out = Vec::new();
    for u in [0.0, 0.13, 0.37, 0.8, 1.0] {
        for v in [0.0, 0.25, 0.6, 1.0] {
            let d = s.evaluate(
                k,
                Point2::new(u, v),
                1,
                1,
                solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags::default(),
            );
            match d {
                Ok(d) => {
                    for row in &d {
                        for p in row {
                            out.push(Ok([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]));
                        }
                    }
                }
                Err(e) => out.push(Err(e.to_string())),
            }
        }
    }
    out
}

/// A round on the round between y = 0 and z = 0 (`blend(false, 1, ..)`, the quarter
/// cylinder about (x, 2, 2) of radius 2): a ball of radius 0.5 inside it against the plane
/// x = 5, its centre on the circle of radius 1.5 about (4.5, 2, 2).
fn round_on_a_round(k: &Kernel) -> ProceduralBlend {
    let inner = Surface::from(blend(k, false, 1, 0.0, false));
    let wall = Surface::from(bilinear([
        Point3::new(5.0, 0.0, 0.0),
        Point3::new(5.0, L, 0.0),
        Point3::new(5.0, 0.0, L),
        Point3::new(5.0, L, L),
    ]));
    let c = Point3::new(4.5, R, R);
    let w = 0.5_f64.sqrt();
    let leader = BSplineCurve::create_canonical(&solidcore::nurbs::b_spline_curve::CanonicalData {
        dim: 3,
        degree: 2,
        control_points: &[
            c + Vec3::new(0.0, -1.5, 0.0),
            c + Vec3::new(0.0, -1.5, -1.5),
            c + Vec3::new(0.0, 0.0, -1.5),
        ],
        form: solidcore::nurbs::b_spline_curve::BSplineCurveForm::Unspecified,
        knot_multiplicities: &[3, 3],
        knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: Some(&[1.0, w, 1.0]),
        trim: None,
    })
    .unwrap();
    // the feet: on the inner round at s = 0.45 across t, on the wall at (y, z) / L
    let seed_inner = line(Point3::new(0.45, 0.0, 0.0), Point3::new(0.45, 1.0, 0.0));
    let seed_wall = line(Point3::new(0.05, 0.2, 0.0), Point3::new(0.2, 0.05, 0.0));
    // the ball is inside the inner round: on the side of its normal towards the axis
    let solidcore::surface::surface::SurfaceKind::ProceduralBlend(ip) = inner.kind() else {
        unreachable!()
    };
    let d = ip.evaluate_definition(k, 0.45, 0.5, 1, 1).unwrap();
    let n = d[1][0].cross(d[0][1]);
    let side_inner = if (c - d[0][0]).dot(n) > 0.0 {
        1.0
    } else {
        -1.0
    };
    let input = BlendDefinitionInput {
        supports: [&inner, &wall],
        sides: [side_inner, 1.0],
        offsets: [0.5, 0.5],
        law: None,
        order: 1,
        thumbweight: 0.0,
        leader: &leader,
        seeds: [&seed_inner, &seed_wall],
        bound: 1e-3,
        chamfer: None,
    };
    let net = bilinear([
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
    ]);
    ProceduralBlend::create(k, &net, &input, |n, w| {
        GnSectionFsg::is_valid(n as usize, w)
    })
    .unwrap()
}

/// #2388: a constant round, a G^n round, a variable round and a round on a round go to
/// data and back with their records equal bit for bit (the JSON text, every number
/// written exactly), and the blend read back evaluates S as the original does, bit for
/// bit, not its net.
#[test]
fn a_definition_goes_to_data_and_back_bit_for_bit() {
    let k = kernel();
    let cases = [
        ("round", blend(&k, false, 1, 0.0, false)),
        (
            "G2 equal area",
            blend(&k, true, 2, GN_THUMBWEIGHT_EQUAL_AREA, false),
        ),
        ("variable", blend(&k, true, 1, 0.0, true)),
        ("round on a round", round_on_a_round(&k)),
    ];
    for (name, pb) in &cases {
        let (json, back, again) = round_trip(&k, pb);
        assert_eq!(json, again, "{name}: the record changed through the kernel");
        assert_eq!(back.depth(), pb.depth(), "{name}");
        let (want, got) = (s_bits(&k, pb), s_bits(&k, &back));
        assert_eq!(want, got, "{name}: S read back is not S");
        // S solved, not the net: at the middle the net (the unit square z = 0) is far
        let uv = Point2::new(0.37, 0.6);
        let p = Surface::from(back.clone()).evaluate_point(&k, uv).unwrap();
        let q = back.net().evaluate_point(uv).unwrap();
        assert!(p.distance(q) > 0.1, "{name}: evaluates its net");
    }
    // the nested record keeps its inner definition by value
    let rec = to_record(&k, &cases[3].1).unwrap();
    assert_eq!(rec.depth(), 2);
    assert!(matches!(
        rec.definition.supports[0],
        SupportRecord::Blend(_)
    ));
    assert!(matches!(
        rec.definition.supports[1],
        SupportRecord::Nurbs { .. }
    ));
    assert!(matches!(
        to_record(&k, &cases[2].1).unwrap().definition.radius,
        RadiusRecord::Law(_)
    ));
}

/// #2388: a corrupted record is refused, before anything is evaluated: counts that do not
/// agree, numbers out of range, knots that decrease, a weight that is not positive, a
/// singular map, and supports nested past three definitions.
#[test]
fn a_corrupted_definition_is_refused() {
    type Corrupt = fn(&mut ProceduralBlendRecord);
    let k = kernel();
    let good = to_record(&k, &blend(&k, true, 1, 0.0, true)).unwrap();
    assert!(from_record(&k, &good).is_ok());
    let corruptions: [(&str, Corrupt); 13] = [
        ("a knot dropped", |r| {
            r.definition.leader.knots.pop();
        }),
        ("a point dropped", |r| {
            r.net.points.pop();
        }),
        ("knots that decrease", |r| {
            r.definition.seeds[0].knots.reverse();
        }),
        ("a weight below 0", |r| {
            r.net.weights = Some(vec![-1.0; r.net.points.len()]);
        }),
        ("a weight per point missing", |r| {
            r.net.weights = Some(vec![1.0]);
        }),
        ("a side of 0.5", |r| r.definition.sides[0] = 0.5),
        ("an order of 7", |r| r.definition.order = 7),
        ("a bound below 0", |r| r.definition.bound = -1.0),
        ("a bound not finite", |r| r.definition.bound = f64::INFINITY),
        ("a singular map", |r| r.definition.map.b[1] = 0.0),
        ("a degree of 0", |r| r.definition.leader.degree = 0),
        ("a support labelled a sphere it is not", |r| {
            let SupportRecord::Nurbs { kind, .. } = &mut r.definition.supports[0] else {
                unreachable!()
            };
            *kind = SupportKindRecord::Sphere;
        }),
        ("four definitions deep", |r| {
            let mut inner = r.clone();
            for _ in 0..3 {
                let mut outer = r.clone();
                outer.definition.supports[0] = SupportRecord::Blend(Box::new(inner));
                inner = outer;
            }
            *r = inner;
        }),
    ];
    for (what, corrupt) in corruptions {
        let mut bad = good.clone();
        corrupt(&mut bad);
        assert!(from_record(&k, &bad).is_err(), "{what} was taken");
    }
    // labelled the plane it is (y = 0), the support is rebuilt as a plane derived from it
    let mut planar = good.clone();
    let SupportRecord::Nurbs { kind, .. } = &mut planar.definition.supports[0] else {
        unreachable!()
    };
    *kind = SupportKindRecord::Plane;
    let back = from_record(&k, &planar).unwrap();
    assert_eq!(back.support(0).unwrap().type_code(), Plane::TYPE);
}

/// #2388: a support labelled a plane is rebuilt as the plane its form parametrises. A form
/// of higher degree (a plane form from another writer, or a hostile record) is a plane
/// only where its own (u, v) is the plane's affine one: the plane y = 0 as a degree-2 by
/// degree-1 form with its middle control row half way along is taken, and the rebuilt
/// plane evaluates as the form; with that row moved within the plane (still planar, not
/// affine in u) it is refused.
#[test]
fn a_plane_label_needs_the_plane_s_parametrisation() {
    use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
    use solidcore::surface::procedural_blend::record::NurbsSurfaceRecord;
    use solidcore::surface::surface::SurfaceKind;
    let k = kernel();
    let good = to_record(&k, &blend(&k, true, 1, 0.0, true)).unwrap();
    let plane = |middle: f64| {
        let mut r = good.clone();
        let mut points = Vec::new();
        for x in [0.0, middle, L] {
            for z in [0.0, L] {
                points.push([x, 0.0, z]);
            }
        }
        r.definition.supports[0] = SupportRecord::Nurbs {
            kind: SupportKindRecord::Plane,
            form: NurbsSurfaceRecord {
                u_degree: 2,
                v_degree: 1,
                u_count: 3,
                v_count: 2,
                u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                v_knots: vec![0.0, 0.0, 1.0, 1.0],
                points,
                weights: None,
            },
        };
        r
    };
    let back = from_record(&k, &plane(0.5 * L)).unwrap();
    let support = back.support(0).unwrap();
    assert_eq!(support.type_code(), Plane::TYPE);
    let SurfaceKind::Plane(p) = support.kind() else {
        unreachable!()
    };
    let uv = Point2::new(0.3, 0.7);
    let analytic = Surface::from(p.clone())
        .evaluate(&k, uv, 0, 0, EvaluateFlags::default())
        .unwrap()[0][0];
    assert!(
        (analytic - Point3::new(0.3 * L, 0.0, 0.7 * L)).length() <= 1e-12,
        "{analytic:?}"
    );
    assert!(
        from_record(&k, &plane(0.2 * L)).is_err(),
        "a plane form not affine in u was taken"
    );
}

/// #2388: a net off its definition is refused when the record is rebuilt. These nets are
/// the unit square z = 0, nowhere near their definitions' rounds; the bump's G^n round's
/// supports are NURBS, so it is the certificate that refuses it.
#[test]
fn a_net_off_its_definition_is_refused_on_rebuild() {
    let k = kernel();
    for pb in [
        blend(&k, false, 1, 0.0, false),
        blend(&k, true, 2, GN_THUMBWEIGHT_EQUAL_AREA, false),
        blend(&k, true, 1, 0.0, true),
    ] {
        let rec = to_record(&k, &pb).unwrap();
        assert!(from_record(&k, &rec).is_ok());
        assert!(rebuild(&k, &rec).is_err());
    }
}

/// S's partials at a row of points at one s, as bits.
fn row_bits(k: &Kernel, pb: &ProceduralBlend, s: f64) -> Vec<u64> {
    let mut out = Vec::new();
    for t in [0.0, 0.3, 0.6, 1.0] {
        for row in pb.evaluate_definition(k, s, t, 2, 2).unwrap() {
            for p in row {
                out.extend([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]);
            }
        }
    }
    out
}

/// #2388, contract 4: evaluating S is a pure function of its definition. Two threads,
/// each with its own kernel, evaluating blends that share a definition give what one
/// kernel gives, bit for bit, also where a thread's kernel has made and evaluated
/// definitions of its own at the same s (its station memo holds their stations).
#[test]
fn evaluating_on_two_threads_equals_one_bit_for_bit() {
    let k = kernel();
    let shared = [
        blend(&k, true, 2, GN_THUMBWEIGHT_EQUAL_AREA, false),
        blend(&k, true, 1, 0.0, true),
    ];
    let ss = [0.0, 0.37, 0.5, 1.0];
    let serial: Vec<Vec<u64>> = shared
        .iter()
        .flat_map(|pb| ss.iter().map(|&s| row_bits(&k, pb, s)))
        .collect();
    let parallel: Vec<Vec<Vec<u64>>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|thread| {
                let shared = shared.clone();
                scope.spawn(move || {
                    let k = kernel();
                    // this kernel's own definitions, of the same orders, evaluated first at
                    // the same s
                    let own = [
                        blend(&k, false, 2, GN_THUMBWEIGHT_EQUAL_AREA, false),
                        blend(&k, false, 1, 0.0, true),
                    ];
                    let mut out = Vec::new();
                    for (pb, mine) in shared.iter().zip(&own) {
                        for &s in &ss {
                            if thread == 1 {
                                let _ = row_bits(&k, mine, s);
                            }
                            out.push(row_bits(&k, pb, s));
                        }
                    }
                    out
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for (thread, got) in parallel.iter().enumerate() {
        assert_eq!(got, &serial, "thread {thread} differs from one kernel");
    }
}

/// #2388: a support labelled a cylinder, cone, sphere or torus is rebuilt as the analytic
/// surface its form parametrises: at every (u, v) of the form the rebuilt support
/// evaluates as the original, to the rounding of the part, whatever frame, angles and
/// swap the original was made on (the primitives' faces, partial ones turned from 30
/// degrees, and a box edge's round of the exact line class, whose frame is its start
/// rail's). A form on the surface whose own (u, v) is not the analytic's (a quarter cylinder
/// whose arc is reparametrised) is refused.
#[test]
fn a_revolved_label_needs_the_form_s_parametrisation() {
    use solidcore::blend::fillet_executive::api::fillet_edges;
    use solidcore::blend::fillet_section::station::as_b_spline;
    use solidcore::primitive::api_primitives::{
        create_box, create_cone, create_cylinder, create_partial_cone, create_partial_cylinder,
        create_partial_sphere, create_partial_torus, create_sphere, create_torus,
    };
    use solidcore::topology::face::p2 as face;
    let mut k = kernel();
    k.fixes.fillet_exact_round_classes = true;
    let o = Point3::new(1.0, -2.0, 3.0);
    let mut rounded = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 8.0, 6.0).unwrap();
    let edge = rounded.edges().ids().next().unwrap();
    fillet_edges(&mut k, &mut rounded, &[edge], 1.5, 1, 1, 1.0).unwrap();
    let bodies = [
        create_cylinder(&mut k, o, 2.0, 5.0).unwrap(),
        create_cone(&mut k, o, 3.0, 1.0, 4.0).unwrap(),
        create_sphere(&mut k, o, 2.5).unwrap(),
        create_torus(&mut k, o, 5.0, 1.5).unwrap(),
        create_partial_cylinder(&mut k, o, 2.0, 5.0, 30.0, 140.0).unwrap(),
        create_partial_cone(&mut k, o, 3.0, 1.0, 4.0, 30.0, 140.0).unwrap(),
        create_partial_sphere(&mut k, o, 2.5, 30.0, 140.0).unwrap(),
        create_partial_torus(&mut k, o, 5.0, 1.5, 30.0, 140.0).unwrap(),
        rounded,
    ];
    let mut checked = 0;
    let mut refused = 0;
    for (index, b) in bodies.iter().enumerate() {
        for f in b.faces().ids() {
            let s = face::surface(b, f).unwrap().unwrap();
            let Some(a) = AnalyticSupport::of(s) else {
                continue;
            };
            if a.kind() == SupportKind::Plane {
                continue;
            }
            let form = as_b_spline(&k, s).unwrap().unwrap().into_owned();
            if index == 7 {
                // the partial torus, a Torus since #14425: on this kernel its derived
                // support is refused (tangency-labs/solidcraft#15314)
                assert_eq!(a.kind(), SupportKind::Torus);
                let derived = AnalyticSupport::derive(&k, a.kind(), &form)
                    .and_then(|d| d.build(&k, form.copy()));
                assert!(
                    matches!(
                        derived,
                        Err(solidcore::error::SolidError::InvalidInput {
                            operation: "surface::implicit::AnalyticSupport::build",
                            reason: "a support's analytic kind disagrees with its NURBS form",
                        })
                    ),
                    "{derived:?}"
                );
                refused += 1;
                continue;
            }
            let back = AnalyticSupport::derive(&k, a.kind(), &form)
                .and_then(|d| d.build(&k, form.copy()))
                .unwrap();
            assert_eq!(back.type_code(), s.type_code());
            let d = s.natural_uv_domain_with_nurb(&k).unwrap();
            for i in 0..=8 {
                for j in 0..=8 {
                    let uv = Point2::new(
                        d.min().x + (d.max().x - d.min().x) * f64::from(i) / 8.0,
                        d.min().y + (d.max().y - d.min().y) * f64::from(j) / 8.0,
                    );
                    let off = (s.evaluate_point(&k, uv).unwrap()
                        - back.evaluate_point(&k, uv).unwrap())
                    .length();
                    assert!(
                        off <= 64.0 * f64::EPSILON * 20.0,
                        "a {:?} support rebuilt is {off:e} off at {uv:?}",
                        a.kind()
                    );
                }
            }
            checked += 1;
        }
    }
    // (the partial torus's one face is refused above, tangency-labs/solidcraft#15314)
    assert_eq!((checked, refused), (8, 1));

    // a quarter cylinder of radius 2 about z from height 0 to 5, its quarter circle the
    // rational quadratic arc with end weights 1 and c^2 (c = 1 the circle's own
    // parametrisation; another c the same arc reparametrised, by a Moebius map)
    let quarter = |c: f64| {
        let w = std::f64::consts::FRAC_1_SQRT_2 * c;
        let arc = [(2.0, 0.0, 1.0), (2.0, 2.0, w), (0.0, 2.0, c * c)];
        let rows: Vec<Vec<_>> = [0.0, 5.0]
            .iter()
            .map(|&z| {
                arc.iter()
                    .map(|&(x, y, wt)| {
                        solidcore::nurbs::math::CPoint::new(x * wt, y * wt, z * wt, wt)
                    })
                    .collect()
            })
            .collect();
        let nurb = solidcore::nurbs::srf_basic::Surface::new(
            rows,
            1,
            2,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .unwrap();
        BSplineSurface::from_nurb(nurb, true)
    };
    let own = quarter(1.0);
    let built = AnalyticSupport::derive(&k, SupportKind::Cylinder, &own)
        .and_then(|d| d.build(&k, own.copy()))
        .unwrap();
    for uv in [Point2::new(0.3, 0.7), Point2::new(0.9, 0.1)] {
        let off =
            (built.evaluate_point(&k, uv).unwrap() - own.evaluate_point(uv).unwrap()).length();
        assert!(off <= 64.0 * f64::EPSILON * 5.0, "{off:e}");
    }
    let moved = quarter(2.0);
    assert!(
        AnalyticSupport::derive(&k, SupportKind::Cylinder, &moved)
            .and_then(|d| d.build(&k, moved.copy()))
            .is_err(),
        "a cylinder form whose angle is not the circle's own parameter was taken"
    );
}

/// A support built from the label `AnalyticSupport::of` reads off `s`, on the form `s`
/// parametrises: the built support is of the surface's kind, carries the label's own
/// numbers (so `of` reads the label back unchanged) and evaluates as the form at the
/// form's own parameters. The bound is the #2388 test's above, 64 ulps on a part of
/// size 20.
fn builds_from_its_own_label(k: &Kernel, s: &Surface, kind: SupportKind) {
    use solidcore::blend::fillet_section::station::as_b_spline;
    let label = AnalyticSupport::of(s).unwrap();
    assert_eq!(label.kind(), kind);
    let form = as_b_spline(k, s).unwrap().unwrap().into_owned();
    let built = label.build(k, form.copy()).unwrap();
    assert_eq!(built.type_code(), s.type_code());
    assert_eq!(
        AnalyticSupport::of(&built),
        Some(label),
        "a {kind:?} support built from its own label lost it"
    );
    let d = form.natural_uv_domain();
    for i in 0..=4 {
        for j in 0..=4 {
            let uv = Point2::new(
                d.min().x + (d.max().x - d.min().x) * f64::from(i) / 4.0,
                d.min().y + (d.max().y - d.min().y) * f64::from(j) / 4.0,
            );
            let off =
                (built.evaluate_point(k, uv).unwrap() - form.evaluate_point(uv).unwrap()).length();
            assert!(
                off <= 64.0 * f64::EPSILON * 20.0,
                "a {kind:?} support built from its own label is {off:e} off at {uv:?}"
            );
        }
    }
}

/// #2554: a support builds from its own label. `AnalyticSupport::of` reads the label off
/// the surface itself; `build` used to refuse it, because it compared the label with the
/// surface it recognises in the form for bit equality, and recognition recomputes the
/// placement, radii and semi-angle from the control points, so it reproduces them only to
/// rounding (1 to 2 ulps on the cases below). What `build` proves is that the form lies on
/// the label's implicit surface and that the label's surface evaluates as the form at the
/// form's own parameters; the surface it returns carries the label's own numbers.
#[test]
fn a_support_builds_from_its_own_label() {
    use solidcore::blend::fillet_section::station::as_b_spline;
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::geom::extent2d::Extent2d;
    use solidcore::primitive::api_primitives::{
        create_cone, create_cylinder, create_partial_cone, create_partial_cylinder,
        create_partial_sphere, create_partial_torus, create_sphere, create_torus,
    };
    use solidcore::surface::cone::Cone;
    use solidcore::topology::face::p2 as face;
    let mut k = kernel();

    // the #2554 cone: radius 1 at the placement's origin, 2 at height 3 (semi-angle
    // atan(1/3)), over the full turn and over a quarter, in each swap and inside-out
    // setting
    let pl = Axis2Placement::new(Vec3::new(1.0, -2.0, 0.5), Vec3::X, Vec3::Y);
    let mut cones = 0;
    for dom in [None, Some(Extent2d::from_bounds(0.0, 0.0, 90.0, 3.0))] {
        for swap_uv in [false, true] {
            for inside_out in [false, true] {
                let mut c =
                    Cone::create_canonical(&k, &pl, 1.0, 2.0, 3.0, swap_uv, inside_out).unwrap();
                if let Some(d) = dom {
                    c.adjust_step_uv_domain(&k, &d).unwrap();
                }
                builds_from_its_own_label(&k, &Surface::from(c), SupportKind::Cone);
                cones += 1;
            }
        }
    }
    assert_eq!(cones, 8);

    // the cylinder, cone, sphere and torus the canonical constructors make, full and
    // partial, as the primitives' own faces
    let o = Point3::new(1.0, -2.0, 3.0);
    let bodies = [
        create_cylinder(&mut k, o, 2.0, 5.0).unwrap(),
        create_cone(&mut k, o, 3.0, 1.0, 4.0).unwrap(),
        create_sphere(&mut k, o, 2.5).unwrap(),
        create_torus(&mut k, o, 5.0, 1.5).unwrap(),
        create_partial_cylinder(&mut k, o, 2.0, 5.0, 30.0, 140.0).unwrap(),
        create_partial_cone(&mut k, o, 3.0, 1.0, 4.0, 30.0, 140.0).unwrap(),
        create_partial_sphere(&mut k, o, 2.5, 30.0, 140.0).unwrap(),
        create_partial_torus(&mut k, o, 5.0, 1.5, 30.0, 140.0).unwrap(),
    ];
    let mut faces = 0;
    let mut refused = 0;
    let not_its_form = "a support's analytic surface is not its NURBS form's parametrisation";
    for (index, b) in bodies.iter().enumerate() {
        for f in b.faces().ids() {
            let s = face::surface(b, f).unwrap().unwrap();
            let Some(label) = AnalyticSupport::of(s) else {
                continue;
            };
            if label.kind() == SupportKind::Plane {
                continue;
            }
            if index == 7 {
                // the partial torus, a Torus since #14425: on this kernel its own label
                // is refused (tangency-labs/solidcraft#15314)
                assert_eq!(label.kind(), SupportKind::Torus);
                let form = as_b_spline(&k, s).unwrap().unwrap().into_owned();
                let own = label.build(&k, form.copy());
                assert!(
                    matches!(
                        own,
                        Err(solidcore::error::SolidError::InvalidInput {
                            operation: "surface::implicit::AnalyticSupport::build",
                            reason,
                        }) if reason == not_its_form
                    ),
                    "{own:?}"
                );
                refused += 1;
                continue;
            }
            builds_from_its_own_label(&k, s, label.kind());
            faces += 1;
        }
    }
    // (the partial torus's one face is refused above, tangency-labs/solidcraft#15314)
    assert_eq!((faces, refused), (7, 1));

    // a label off its form is still refused: the #2554 cone's radius changed by 1e-9,
    // which is far above the rounding of the recognition the old check compared against
    let c = Cone::create_canonical(&k, &pl, 1.0, 2.0, 3.0, false, false).unwrap();
    let s = Surface::from(c);
    let form = as_b_spline(&k, &s).unwrap().unwrap().into_owned();
    let mut moved = AnalyticSupport::of(&s).unwrap();
    assert_eq!(moved.kind(), SupportKind::Cone);
    if let AnalyticSupport::Cone { base_radius, .. } = &mut moved {
        *base_radius += 1e-9;
    }
    assert!(
        moved.build(&k, form.copy()).is_err(),
        "a cone label 1e-9 off its form was taken"
    );
}

/// `label` with its placement's x and y axes turned by `angle` radians about its own z
/// axis, its origin and axis unchanged. The implicit equation of a cylinder, cone, sphere
/// or torus does not depend on that frame, and neither does the NURBS form, so a turned
/// label is off its form only in the parametrisation.
fn turned_about_its_axis(label: AnalyticSupport, angle: f64) -> AnalyticSupport {
    use solidcore::geom::axis2_placement::Axis2Placement;
    let mut turned = label;
    let p = match &mut turned {
        AnalyticSupport::Plane { placement, .. }
        | AnalyticSupport::Cylinder { placement, .. }
        | AnalyticSupport::Cone { placement, .. }
        | AnalyticSupport::Sphere { placement, .. }
        | AnalyticSupport::Torus { placement, .. } => placement,
    };
    let (c, s) = (angle.cos(), angle.sin());
    *p = Axis2Placement::new(
        p.origin(),
        p.x_axis() * c + p.y_axis() * s,
        p.y_axis() * c - p.x_axis() * s,
    );
    turned
}

/// #2588: the frame a support imposes is read. `AnalyticSupport::build` puts the label's
/// own placement on the parametrisation it recognises in the form, and `evaluates_as` is
/// what proves label and form agree. Turning the label about its own axis leaves the
/// implicit equation (so `lies_on`) and the form untouched, and the recognised sweep
/// angles are the form's, so the turned label parametrises the same point set with its
/// u offset by the turn: only the analytic evaluation of the imposed frame can refuse it.
/// A sphere's implicit equation holds for every orientation, so its tilted axis must be
/// refused the same way. The turn is 1e-6 rad, which moves a point at distance `r` from
/// the axis by about `r * 1e-6`: on the forms below between 1e-6 (the cone's base) and
/// 6.5e-6 (the torus's outer circle), orders of magnitude above the `gamma_K` rounding
/// bound `evaluates_as` allows.
#[test]
fn a_support_s_turned_frame_is_refused() {
    use solidcore::blend::fillet_section::station::as_b_spline;
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::primitive::api_primitives::{create_cylinder, create_sphere, create_torus};
    use solidcore::surface::cone::Cone;
    use solidcore::topology::face::p2 as face;
    let mut k = kernel();

    let o = Point3::new(1.0, -2.0, 3.0);
    let bodies = [
        create_cylinder(&mut k, o, 2.0, 5.0).unwrap(),
        create_sphere(&mut k, o, 2.5).unwrap(),
        create_torus(&mut k, o, 5.0, 1.5).unwrap(),
    ];

    // the #2554 cone: radius 1 at the placement's origin, 2 at height 3
    let pl = Axis2Placement::new(Vec3::new(1.0, -2.0, 0.5), Vec3::X, Vec3::Y);
    let cone = Surface::from(Cone::create_canonical(&k, &pl, 1.0, 2.0, 3.0, false, false).unwrap());

    // the cone above, and the cylinder, sphere and torus of the primitives' own faces
    let mut surfaces = vec![&cone];
    for b in &bodies {
        for f in b.faces().ids() {
            let s = face::surface(b, f).unwrap().unwrap();
            if AnalyticSupport::of(s).is_some_and(|a| a.kind() != SupportKind::Plane) {
                surfaces.push(s);
            }
        }
    }
    assert_eq!(surfaces.len(), 4, "the cone, cylinder, sphere and torus");

    for s in surfaces {
        let label = AnalyticSupport::of(s).unwrap();
        let kind = label.kind();
        let form = as_b_spline(&k, s).unwrap().unwrap().into_owned();

        // the label as the surface carries it builds
        label.build(&k, form.copy()).unwrap();

        for angle in [1e-6, -1e-6] {
            let turned = turned_about_its_axis(label, angle);
            assert!(
                turned.build(&k, form.copy()).is_err(),
                "a {kind:?} label turned by {angle:e} rad about its own axis was taken"
            );
        }

        // a sphere's axis is unconstrained by its implicit equation too
        if let AnalyticSupport::Sphere { placement, radius } = label {
            let (c, si) = (1e-6_f64.cos(), 1e-6_f64.sin());
            let tilted = AnalyticSupport::Sphere {
                placement: Axis2Placement::new(
                    placement.origin(),
                    placement.x_axis(),
                    placement.y_axis() * c + placement.z_axis() * si,
                ),
                radius,
            };
            assert!(
                tilted.build(&k, form.copy()).is_err(),
                "a sphere label whose axis is tilted by 1e-6 rad was taken"
            );
        }
    }
}

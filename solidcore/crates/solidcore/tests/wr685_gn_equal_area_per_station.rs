//! WR-685 (#2250), completed by the fork's b9d6a72 and 7102bc5 (smlib#79, #2273; the
//! prerequisites of #2274): a G^n section's thumbweight is any w below 1 (the equal area default moves to 2), and the equal area
//! thumbweight is resolved at every station from that station's own wedge, not once per
//! generator from the first blend's middle station. The surface then interpolates the
//! section's homogeneous rows, each a cubic Hermite curve of its values and
//! s-derivatives, and the certificate bounds their remainder too.
//!
//! Expected values: fork 7102bc5 (`SMLib` and `SM_API` only), built locally with the harness'
//! flags and driven through `SmApiFilletEdges` on the same bodies. Each case's blend faces'
//! nets (control point counts, the sums of the control points' coordinates and weights,
//! and one control point) are the fork's within 1e-9 relative, and its volume within 1e-7
//! (the volume integrator's tolerance, as in `wr685_gn_section.rs`). A lone box edge's
//! rails run past the box in the fork (3 spans over s in [0, 80], at d01d7b0 too) and
//! along it here (2 spans), with or without this change: there only the section's own
//! values (control point (1, 2)'s y, z and weight) are compared. So they are for the
//! chamfered box's and the pipe's rounds: the fork's nets keep a sliver span past the
//! edge's end (7 rows in u, the last three 4e-4 to 6e-4 past it), which the shrink here
//! ends on the knot (WR-702): each is one span along the edge, its x the edge's own.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_section::section::GN_DEFAULT_THUMBWEIGHT;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::KernelOptions;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::ControlPointForm;
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (0.1f64, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance <= best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap_or_else(|| panic!("no edge near {p:?}"))
}

fn volume_of(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-7, &attrs).unwrap()
}

/// A blend face's net as the fork's driver prints it: the control point counts in u and
/// v, the sums of the control points' x, y, z and weights and of their |x| + |y| + |z|,
/// and control point (1, 2) with its weight (Euclidean).
#[derive(Debug)]
struct Net {
    nu: usize,
    nv: usize,
    sum: [f64; 4],
    abs: f64,
    cp12: [f64; 4],
    /// The x coordinates of the first and of the last row of control points in u.
    x_ends: [Vec<f64>; 2],
}

/// The blend faces (B-spline surfaces of degree `vdeg` in v), in face order.
fn blend_nets(b: &Brep, vdeg: usize) -> Vec<Net> {
    let mut out = Vec::new();
    for &f in b.face_list() {
        let Some(s) = b.face(f).unwrap().surface.clone() else {
            continue;
        };
        let SurfaceKind::BSpline(bs) = s.kind() else {
            continue;
        };
        if bs.degree(SurfParamType::V) != vdeg {
            continue;
        }
        let (nu, nv, _) = bs.control_points().unwrap();
        let mut net = Net {
            nu,
            nv,
            sum: [0.0; 4],
            abs: 0.0,
            cp12: [0.0; 4],
            x_ends: [Vec::new(), Vec::new()],
        };
        for iu in 0..nu {
            for iv in 0..nv {
                let (p, w) = bs
                    .control_point(ControlPointForm::EuclideanRational, iu, iv)
                    .unwrap();
                net.sum[0] += p.x;
                net.sum[1] += p.y;
                net.sum[2] += p.z;
                net.sum[3] += w;
                net.abs += p.x.abs() + p.y.abs() + p.z.abs();
                if iu == 1 && iv == 2 {
                    net.cp12 = [p.x, p.y, p.z, w];
                }
                if iu == 0 {
                    net.x_ends[0].push(p.x);
                }
                if iu + 1 == nu {
                    net.x_ends[1].push(p.x);
                }
            }
        }
        out.push(net);
    }
    out
}

/// Every net is one cubic span along the edge from x = 0 to `length` (WR-702: no sliver
/// span past its end): its first row of control points at x = 0, its last at x = `length`,
/// and its rows' x summing to `nv` times 0 + L/3 + 2L/3 + L.
fn one_span_along(what: &str, nets: &[Net], length: f64) {
    for n in nets {
        assert_eq!(n.nu, 4, "{what}: one span along the edge: {n:?}");
        for (row, at) in n.x_ends.iter().zip([0.0, length]) {
            assert_eq!(row.len(), n.nv, "{what}: an end row of the net");
            assert!(
                row.iter().all(|&x| close(x, at)),
                "{what}: end row x {row:?} want {at}"
            );
        }
        let want = n.nv as f64 * 2.0 * length;
        assert!(
            close(n.sum[0], want),
            "{what}: x sum {} want {want}",
            n.sum[0]
        );
    }
}

/// `|a - b| <= 1e-9 max(|a|, |b|)`, with an absolute floor of 1e-9 times the body's size
/// (40) for values the fork's rounding puts near 0 (a control point on a coordinate plane).
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()) || (a - b).abs() <= 4e-8
}

type ForkNet = (usize, usize, [f64; 4], f64, [f64; 4]);

fn check(what: &str, v: f64, want_v: f64, nets: &[Net], want: &[ForkNet], full: bool) {
    eprintln!("{what}: volume {v:.17e}");
    for n in nets {
        eprintln!("  {n:?}");
    }
    assert!(
        (v - want_v).abs() <= 1e-7 * want_v,
        "{what}: volume {v} want {want_v}"
    );
    assert_eq!(nets.len(), want.len(), "{what}: blend faces");
    // the faces may come in another order: match each fork net to one of ours
    for (nu, nv, sum, abs, cp) in want {
        let found = nets.iter().any(|n| {
            let section =
                n.nv == *nv && n.cp12[1..].iter().zip(&cp[1..]).all(|(a, b)| close(*a, *b));
            section
                && (!full
                    || (n.nu == *nu
                        && n.sum.iter().zip(sum).all(|(a, b)| close(*a, *b))
                        && close(n.abs, *abs)
                        && close(n.cp12[0], cp[0])))
        });
        assert!(
            found,
            "{what}: no net matches the fork's {nu} x {nv} {sum:?} {abs} {cp:?}"
        );
    }
}

/// The box edge (two planes at a right angle): every station's wedge is the same, so the
/// per-station w is the square corner's at every order, and the round takes off 40 times
/// the circle's area within the certificate (closed form `24000 - 360 (1 - pi/4)`), with
/// the fork's nets. Thumbweights below -1 (b9d6a72) are now accepted: -1 and -3, the
/// fork's values too.
#[test]
fn box_edge_rounds_are_the_fork_s() {
    let cases: [(usize, f64, f64, ForkNet); 6] = [
        (
            2,
            GN_DEFAULT_THUMBWEIGHT,
            23922.743344461505,
            (
                10,
                8,
                [
                    1600.0064000000011,
                    2316.7652591738888,
                    1516.7652591738888,
                    107.2064144384202,
                ],
                5433.5436383477763,
                [
                    -0.00013999999999847007,
                    28.078960928359447,
                    20.0,
                    0.82198089967453902,
                ],
            ),
        ),
        (
            3,
            GN_DEFAULT_THUMBWEIGHT,
            23922.743341081863,
            (
                10,
                11,
                [
                    2200.0088000000001,
                    3183.8262386494716,
                    2083.8262386494716,
                    206.80353340746868,
                ],
                7467.6705172989396,
                [
                    -0.00013999999999847007,
                    27.644155275690085,
                    20.0,
                    1.7132095022232583,
                ],
            ),
        ),
        (
            4,
            GN_DEFAULT_THUMBWEIGHT,
            23922.743340991627,
            (
                10,
                14,
                [
                    2800.0112000000004,
                    4050.6720285896099,
                    2650.6720285896081,
                    326.61809704196418,
                ],
                9501.3670171792182,
                [
                    -0.00013999999999847004,
                    27.451996259493047,
                    20.0,
                    2.7598019309643012,
                ],
            ),
        ),
        (
            2,
            0.0,
            23944.490652523978,
            (
                10,
                8,
                [
                    1600.0063999999991,
                    2326.5000000000005,
                    1526.5,
                    105.83005244258369,
                ],
                5453.0131200000014,
                [
                    -0.00013999999999847004,
                    28.500000000000007,
                    19.999999999999996,
                    0.7559289460184544,
                ],
            ),
        ),
        (
            2,
            -1.0,
            23908.416874552884,
            (
                10,
                8,
                [
                    1600.0064000000002,
                    2311.1818181818185,
                    1511.181818181818,
                    110.06545671510557,
                ],
                5422.3767563636375,
                [
                    -0.00013999999999847007,
                    27.81818181818182,
                    20.0,
                    0.94884014409573703,
                ],
            ),
        ),
        (
            2,
            -3.0,
            23875.280513035461,
            (
                10,
                8,
                [
                    1600.0064000000004,
                    2301.334886143341,
                    1501.3348861433406,
                    119.87688795699461,
                ],
                5402.6828922866835,
                [
                    -0.00013999999999847007,
                    27.308411214953278,
                    20.0,
                    1.6640927622468131,
                ],
            ),
        ),
    ];
    let circle = 24000.0 - 40.0 * 9.0 * (1.0 - core::f64::consts::FRAC_PI_4);
    for (n, w, want_v, net) in cases {
        let mut k = Kernel::new(&KernelOptions::default());
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
        let e = pick(&k, &b, [20.0, 30.0, 20.0]);
        fillet_edges(&mut k, &mut b, &[e], 3.0, 2, n as u32, w).unwrap();
        let v = volume_of(&k, &mut b);
        check(
            &format!("box n {n} w {w}"),
            v,
            want_v,
            &blend_nets(&b, 3 * n + 1),
            &[net],
            false,
        );
        if w == GN_DEFAULT_THUMBWEIGHT {
            assert!((v - circle).abs() <= 1e-7 * circle, "{v} want {circle}");
        }
    }
    // 1 and past are refused, before and after b9d6a72
    let mut k = Kernel::new(&KernelOptions::default());
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let e = pick(&k, &b, [20.0, 30.0, 20.0]);
    assert!(fillet_edges(&mut k, &mut b, &[e], 3.0, 2, 2, 1.0).is_err());
    assert!(fillet_edges(&mut k, &mut b, &[e], 3.0, 2, 2, f64::NAN).is_err());
}

/// Two edges of different wedges rounded r 2 in one call, after the box's top back edge is
/// chamfered 5: the chamfer's top edge (135 degrees) and the box's bottom front edge (90).
/// Each station takes its own wedge's thumbweight, so the volume no longer depends on the
/// edges' order (before 7102bc5 the first blend's wedge resolved it for both: d01d7b0 gave
/// 23468.183912205881 and 23461.498019658513). Fork 7102bc5: 23462.221395287481 either
/// way, 23496.557686572651 and 23465.663708708671 for each alone.
#[test]
fn equal_area_thumbweight_is_each_station_s_own() {
    let chamfer_net: ForkNet = (
        7,
        8,
        [
            1600.0097600000006,
            1395.6181050217608,
            1109.4211697146393,
            74.21892800854549,
        ],
        4105.0490347364012,
        [
            13.333333333333336,
            24.539986171346552,
            20.000000000000004,
            0.76571009990913541,
        ],
    );
    let box_net: ForkNet = (
        7,
        8,
        [
            1600.0097600000013,
            38.842879052185523,
            38.842879052185545,
            75.04449010689406,
        ],
        1677.6955181043716,
        [
            13.333333333333336,
            1.2806927144270359,
            0.0,
            0.82198089967453869,
        ],
    );
    let cases: [(&[usize], f64, Vec<ForkNet>); 4] = [
        (&[0, 1], 23462.221395287481, vec![chamfer_net, box_net]),
        (&[1, 0], 23462.221395287481, vec![chamfer_net, box_net]),
        (&[0], 23496.557686572651, vec![chamfer_net]),
        (&[1], 23465.663708708671, vec![box_net]),
    ];
    for (order, want_v, nets) in cases {
        let mut k = Kernel::new(&KernelOptions::default());
        let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
        let c = pick(&k, &b, [20.0, 30.0, 20.0]);
        fillet_edges(&mut k, &mut b, &[c], 5.0, 0, 2, 0.0).unwrap();
        let both = [
            pick(&k, &b, [20.0, 25.0, 20.0]),
            pick(&k, &b, [20.0, 0.0, 0.0]),
        ];
        let es: Vec<EdgeId> = order.iter().map(|&i| both[i]).collect();
        fillet_edges(&mut k, &mut b, &es, 2.0, 2, 2, GN_DEFAULT_THUMBWEIGHT).unwrap();
        let v = volume_of(&k, &mut b);
        let what = format!("edges {order:?}");
        let got = blend_nets(&b, 7);
        check(&what, v, want_v, &got, &nets, false);
        one_span_along(&what, &got, 40.0);
    }
}

/// The fork's pipe on a plate (`wr685_gn_section.rs`): a plane and a cylinder, the jets
/// curved, its rows certified with the varying thumbweight's terms. Fork 3bc54a4 at the
/// equal area default (orders 2 and 3) and at w = 0: since 3bc54a4 the section's jets of
/// order n are the face curves' normal parts (the cylinder's from its order-n partials),
/// which moves these nets from 7102bc5's (planes are unchanged). The fork's volumes there
/// integrate the round's definition (4a10ed8), within the integrator's 1e-7 of the net's.
#[test]
fn pipe_root_rounds_are_the_fork_s() {
    use solidcore::boolean::api_brep::boolean_union;
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::math::Vec3;
    use solidcore::primitive::api_primitives::create_cylinder;
    let cases: [(usize, f64, f64, ForkNet); 3] = [
        (
            2,
            GN_DEFAULT_THUMBWEIGHT,
            40307.885637878353,
            (
                7,
                8,
                [
                    2400.014560000001,
                    1672.5032862093772,
                    617.99033980872366,
                    76.340345337018292,
                ],
                4690.508186018099,
                [
                    19.999999999999996,
                    29.185391179444686,
                    12.011560328527208,
                    0.9051790507776456,
                ],
            ),
        ),
        (
            2,
            0.0,
            40267.709270590589,
            (
                7,
                8,
                [
                    2400.014560000001,
                    1660.151886377726,
                    611.05246913580243,
                    74.081036709808572,
                ],
                4671.2189155135275,
                [
                    19.999999999999996,
                    28.991604748421537,
                    11.570987654320987,
                    0.7559289460184544,
                ],
            ),
        ),
        (
            3,
            GN_DEFAULT_THUMBWEIGHT,
            40302.538375279823,
            (
                7,
                11,
                [
                    3300.0200199999999,
                    2298.9900940535895,
                    848.8826512459334,
                    149.41774934689863,
                ],
                6447.8927652995208,
                [
                    20.0,
                    29.289337727685155,
                    12.285024031652828,
                    1.9249659751603994,
                ],
            ),
        ),
    ];
    for (n, w, want_v, net) in cases {
        let mut k = Kernel::new(&KernelOptions::default());
        let plate = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
        let mut pipe = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 60.0).unwrap();
        let pl = Axis2Placement::new(
            Point3::new(0.0, 20.0, 16.0),
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        solidcore::topology::brep::p5::transform(&mut k, &mut pipe, &pl, None, false).unwrap();
        let mut b = boolean_union(&mut k, plate, pipe).unwrap();
        let e = pick(&k, &b, [30.0, 28.0, 10.0]);
        fillet_edges(&mut k, &mut b, &[e], 2.0, 2, n as u32, w).unwrap();
        let v = volume_of(&k, &mut b);
        let what = format!("pipe n {n} w {w}");
        let got = blend_nets(&b, 3 * n + 1);
        check(&what, v, want_v, &got, &[net], false);
        one_span_along(&what, &got, 60.0);
    }
}

/// smlib#109 (f9b133a, b8ec238): an equal area round solves each order and u once in its
/// operation, and its certificate sums I(w) once per point and per interval off its cells.
/// The counts over the blend are gated about 5% over today's (the fork's capi `rooted` and
/// `summed`, counts, not times). Fork 5e8c9c1's gates, which also count its definition's
/// evaluations: box edge 7 roots and 2 sums, boss root 13 and 680 (before smlib#109, 4720
/// and 14215 roots, 10389 sums on the boss root). Before this port the same blends solved
/// to the same bits (face nets and volume compared against #2274's head, 7ad58eb).
#[test]
fn equal_area_roots_and_sums_are_solved_once_each() {
    fn box_edge(k: &mut Kernel) -> solidcore::Result<Brep> {
        create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0)
    }
    fn boss_root(k: &mut Kernel) -> solidcore::Result<Brep> {
        let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0)?;
        let boss = create_cylinder(k, Point3::new(30.0, 20.0, 10.0), 8.0, 10.0)?;
        boolean_union(k, plate, boss)
    }
    struct Case {
        name: &'static str,
        build: fn(&mut Kernel) -> solidcore::Result<Brep>,
        pick: [f64; 3],
        r: f64,
        most_roots: u64,
        most_sums: u64,
    }
    let d = 4.0 * 2f64.sqrt();
    let cases = [
        Case {
            name: "box edge",
            build: box_edge,
            pick: [20.0, 30.0, 20.0],
            r: 3.0,
            most_roots: 1,
            most_sums: 1,
        },
        Case {
            name: "boss root",
            build: boss_root,
            pick: [30.0 + d, 20.0 + d, 10.0],
            r: 2.0,
            most_roots: 10,
            most_sums: 680,
        },
    ];
    for Case {
        name,
        build,
        pick: p,
        r,
        most_roots,
        most_sums,
    } in cases
    {
        let mut k = Kernel::new(&KernelOptions::default());
        let mut b = build(&mut k).unwrap();
        let e = pick(&k, &b, p);
        fillet_edges(&mut k, &mut b, &[e], r, 2, 2, GN_DEFAULT_THUMBWEIGHT).unwrap();
        let c = k.blend_counts.get();
        assert!(
            (1..=most_roots).contains(&c.area_roots),
            "{name}: {} roots",
            c.area_roots
        );
        assert!(
            (1..=most_sums).contains(&c.area_sums),
            "{name}: {} sums",
            c.area_sums
        );
        // the memo is the operation's: the kernel keeps what it solved
        assert!(!k.blend_w_memo.borrow().is_empty(), "{name}");
    }
}

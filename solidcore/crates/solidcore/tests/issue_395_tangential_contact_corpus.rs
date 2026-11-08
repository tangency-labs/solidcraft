//! Issue #395 (WR-75): the tangential-contact corpus against its closed forms.
//!
//! The same surface pairs as `fixtures/surface.surface_p2/ssi_tangential_contact.json`
//! (`harness/fixturegen/ssi_tangential_contact.py`), rebuilt here: exact rational
//! cylinders, spheres and tori and exact polynomial Bezier graphs, each touching the
//! other surface along a known line, circle or point. Each pair runs through
//! `GlobalSurfaceIntersect` at the ssi corpus tolerance (1e-6) with the default kernel
//! (upstream behaviour, which difftest holds at parity with the C++) and with
//! `Kernel::fixes.ssi_tangent_curve_tracer` (the SolidCraft fork's tracer, #1663), and
//! the result is measured against the closed form: the share of the contact that a
//! returned curve covers, whether every returned curve lies on the contact, and the
//! touching points.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::too_many_lines
)] // test helpers

use solidcore::geom::tol::approx_tol3d;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::surface::p2::{SurfaceIntersection, TsectCurveType};
use solidcore::surface::surface::Surface;

const W: f64 = std::f64::consts::FRAC_1_SQRT_2;
/// The 9-point unit circle `(x, y, w)` from `+x`, counter-clockwise.
const CIRCLE: [(f64, f64, f64); 9] = [
    (1.0, 0.0, 1.0),
    (1.0, 1.0, W),
    (0.0, 1.0, 1.0),
    (-1.0, 1.0, W),
    (-1.0, 0.0, 1.0),
    (-1.0, -1.0, W),
    (0.0, -1.0, 1.0),
    (1.0, -1.0, W),
    (1.0, 0.0, 1.0),
];
const CIRCLE_KNOTS: [f64; 12] = [
    0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
];
/// The 5-point half circle `(radial, axial, w)` from `-axis` to `+axis`.
const HALF: [(f64, f64, f64); 5] = [
    (0.0, -1.0, 1.0),
    (1.0, -1.0, W),
    (1.0, 0.0, 1.0),
    (1.0, 1.0, W),
    (0.0, 1.0, 1.0),
];
const HALF_KNOTS: [f64; 8] = [0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0];

const X: Vec3 = Vec3::new(1.0, 0.0, 0.0);
const Y: Vec3 = Vec3::new(0.0, 1.0, 0.0);
const Z: Vec3 = Vec3::new(0.0, 0.0, 1.0);

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}

fn hom(p: Vec3, w: f64) -> CPoint {
    CPoint::new(p.x * w, p.y * w, p.z * w, w)
}

fn bss(net: Vec<Vec<CPoint>>, p: usize, q: usize, u: Vec<f64>, kv: Vec<f64>) -> Surface {
    let nurb = NurbSurface::new(net, p, q, u, kv).unwrap();
    Surface::from(BSplineSurface::from_nurb(nurb, false))
}

fn clamped(n: usize) -> Vec<f64> {
    let mut k = vec![0.0; n + 1];
    k.extend(vec![1.0; n + 1]);
    k
}

/// The bilinear patch of `z = 0` over `[-s, s]^2`.
fn plane(s: f64) -> Surface {
    let net = [-s, s]
        .iter()
        .map(|&x| [-s, s].iter().map(|&y| hom(v(x, y, 0.0), 1.0)).collect())
        .collect();
    bss(net, 1, 1, clamped(1), clamped(1))
}

/// `center + r (cos e1 + sin e2) + h axis`, `h` in `[0, len]`.
fn cylinder(center: Vec3, e1: Vec3, e2: Vec3, axis: Vec3, r: f64, len: f64) -> Surface {
    let net = CIRCLE
        .iter()
        .map(|&(cx, cy, w)| {
            let base = center + e1 * (r * cx) + e2 * (r * cy);
            [0.0, len]
                .iter()
                .map(|&h| hom(base + axis * h, w))
                .collect()
        })
        .collect();
    bss(net, 2, 1, CIRCLE_KNOTS.to_vec(), clamped(1))
}

/// `profile` (`(radial, axial, w)`) revolved about `axis` through `center`.
fn revolved(
    center: Vec3,
    e1: Vec3,
    e2: Vec3,
    axis: Vec3,
    profile: &[(f64, f64, f64)],
    knots: &[f64],
) -> Surface {
    let net = CIRCLE
        .iter()
        .map(|&(cx, cy, wu)| {
            profile
                .iter()
                .map(|&(rho, z, wv)| {
                    hom(
                        center + e1 * (rho * cx) + e2 * (rho * cy) + axis * z,
                        wu * wv,
                    )
                })
                .collect()
        })
        .collect();
    bss(net, 2, 2, CIRCLE_KNOTS.to_vec(), knots.to_vec())
}

fn sphere(center: Vec3, e1: Vec3, e2: Vec3, axis: Vec3, r: f64) -> Surface {
    let prof: Vec<_> = HALF.iter().map(|&(a, b, w)| (r * a, r * b, w)).collect();
    revolved(center, e1, e2, axis, &prof, &HALF_KNOTS)
}

fn torus(center: Vec3, big: f64, small: f64) -> Surface {
    let prof: Vec<_> = CIRCLE
        .iter()
        .map(|&(a, b, w)| (big + small * a, small * b, w))
        .collect();
    revolved(center, X, Y, Z, &prof, &CIRCLE_KNOTS)
}

fn choose(n: usize, k: usize) -> f64 {
    (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
}

/// The degree-`n` Bernstein coefficients of `x^k`, `x = lo + (hi - lo) t`.
fn bernstein_power(k: usize, n: usize, lo: f64, hi: f64) -> Vec<f64> {
    let d = hi - lo;
    // monomial coefficients in t of (lo + d t)^k
    let mono: Vec<f64> = (0..=n)
        .map(|j| {
            if j > k {
                0.0
            } else {
                choose(k, j) * lo.powi((k - j) as i32) * d.powi(j as i32)
            }
        })
        .collect();
    (0..=n)
        .map(|i| (0..=i).map(|j| choose(i, j) / choose(n, j) * mono[j]).sum())
        .collect()
}

/// `z = sum c x^a y^b` over `[lo, hi]^2` as a degree `(n, n)` Bezier patch.
fn bezier_graph(terms: &[(usize, usize, f64)], n: usize, lo: f64, hi: f64) -> Surface {
    let b: Vec<Vec<f64>> = (0..=n).map(|k| bernstein_power(k, n, lo, hi)).collect();
    let xs = &b[1];
    let net = (0..=n)
        .map(|i| {
            (0..=n)
                .map(|j| {
                    let z: f64 = terms.iter().map(|&(a, e, c)| c * b[a][i] * b[e][j]).sum();
                    hom(v(xs[i], xs[j], z), 1.0)
                })
                .collect()
        })
        .collect();
    bss(net, n, n, clamped(n), clamped(n))
}

/// `z = x^k` over `[-1, 1]^2`, degree `k` in u, 1 in v.
fn ruled_graph(k: usize) -> Surface {
    let xs = bernstein_power(1, k, -1.0, 1.0);
    let zs = bernstein_power(k, k, -1.0, 1.0);
    let net = (0..=k)
        .map(|i| {
            [-1.0, 1.0]
                .iter()
                .map(|&y| hom(v(xs[i], y, zs[i]), 1.0))
                .collect()
        })
        .collect();
    bss(net, k, 1, clamped(k), clamped(1))
}

/// The exact contact: polylines (sampled finely) and isolated points.
struct Contact {
    curves: Vec<Vec<Vec3>>,
    points: Vec<Vec3>,
}

fn segment(a: Vec3, b: Vec3) -> Vec<Vec3> {
    (0..=200)
        .map(|i| a + (b - a) * (f64::from(i) / 200.0))
        .collect()
}

fn circle(center: Vec3, e1: Vec3, e2: Vec3, r: f64) -> Vec<Vec3> {
    (0..=720)
        .map(|i| {
            let t = std::f64::consts::TAU * f64::from(i) / 720.0;
            center + e1 * (r * t.cos()) + e2 * (r * t.sin())
        })
        .collect()
}

/// One corpus pair (the fixture's `tc_<name>`).
struct Pair {
    name: &'static str,
    a: Surface,
    b: Surface,
    contact: Contact,
}

fn corpus() -> Vec<Pair> {
    let line_y = |z: f64| Contact {
        curves: vec![segment(v(0.0, -1.0, z), v(0.0, 1.0, z))],
        points: vec![],
    };
    // the Steinmetz ellipses x = +-z, y^2 + z^2 = 1 (z in [-1, 1])
    let ellipse = |s: f64| {
        (0..=720)
            .map(|i| {
                let t = std::f64::consts::TAU * f64::from(i) / 720.0;
                v(s * t.sin(), t.cos(), t.sin())
            })
            .collect()
    };
    let diag = |s: f64, z: fn(f64) -> f64| {
        (0..=200)
            .map(|i| {
                let x = -1.0 + 2.0 * f64::from(i) / 200.0;
                v(x, s * x, z(x))
            })
            .collect()
    };
    vec![
        Pair {
            name: "cylinder_plane_line",
            a: cylinder(v(0.0, -1.0, 1.0), X, Z, Y, 1.0, 2.0),
            b: plane(2.0),
            contact: line_y(0.0),
        },
        Pair {
            name: "parabolic_cylinder_plane_line",
            a: ruled_graph(2),
            b: plane(2.0),
            contact: line_y(0.0),
        },
        Pair {
            name: "cylinders_parallel_external_line",
            a: cylinder(v(0.0, -1.0, 1.0), X, Z, Y, 1.0, 2.0),
            b: cylinder(v(0.0, -2.0, -1.0), X, Z, Y, 1.0, 4.0),
            contact: line_y(0.0),
        },
        Pair {
            name: "cylinders_parallel_internal_line",
            a: cylinder(v(0.0, -1.0, -1.0), X, Z, Y, 1.0, 2.0),
            b: cylinder(v(0.0, -2.0, 0.0), X, Z, Y, 2.0, 4.0),
            contact: line_y(-2.0),
        },
        Pair {
            name: "steinmetz_equal_cylinders",
            a: cylinder(v(0.0, 0.0, -2.0), X, Y, Z, 1.0, 4.0),
            b: cylinder(v(-2.0, 0.0, 0.0), Y, Z, X, 1.0, 4.0),
            contact: Contact {
                curves: vec![ellipse(1.0), ellipse(-1.0)],
                points: vec![v(0.0, 1.0, 0.0), v(0.0, -1.0, 0.0)],
            },
        },
        Pair {
            name: "sphere_plane_point",
            a: sphere(v(0.0, 0.0, 1.0), Y, Z, X, 1.0),
            b: plane(2.0),
            contact: Contact {
                curves: vec![],
                points: vec![Vec3::ZERO],
            },
        },
        Pair {
            name: "paraboloid_plane_point",
            a: bezier_graph(&[(2, 0, 1.0), (0, 2, 1.0)], 2, -1.0, 1.0),
            b: plane(2.0),
            contact: Contact {
                curves: vec![],
                points: vec![Vec3::ZERO],
            },
        },
        Pair {
            name: "torus_plane_circle",
            a: torus(v(0.0, 0.0, 0.5), 2.0, 0.5),
            b: plane(3.0),
            contact: Contact {
                curves: vec![circle(Vec3::ZERO, X, Y, 2.0)],
                points: vec![],
            },
        },
        Pair {
            name: "quartic_plane_circle",
            a: bezier_graph(
                &[
                    (4, 0, 1.0),
                    (0, 4, 1.0),
                    (2, 2, 2.0),
                    (2, 0, -2.0),
                    (0, 2, -2.0),
                    (0, 0, 1.0),
                ],
                4,
                -2.0,
                2.0,
            ),
            b: plane(3.0),
            contact: Contact {
                curves: vec![circle(Vec3::ZERO, X, Y, 1.0)],
                points: vec![],
            },
        },
        Pair {
            name: "sphere_in_cylinder_circle",
            a: sphere(Vec3::ZERO, Y, Z, X, 1.0),
            b: cylinder(v(-2.0, 0.0, 0.0), Y, Z, X, 1.0, 4.0),
            contact: Contact {
                curves: vec![circle(Vec3::ZERO, Y, Z, 1.0)],
                points: vec![],
            },
        },
        Pair {
            name: "cubic_plane_line",
            a: ruled_graph(3),
            b: plane(2.0),
            contact: line_y(0.0),
        },
        Pair {
            name: "quartic_ridge_plane_line",
            a: ruled_graph(4),
            b: plane(2.0),
            contact: line_y(0.0),
        },
        Pair {
            name: "saddle_plane_node",
            a: bezier_graph(&[(2, 0, 1.0), (0, 2, -1.0)], 2, -1.0, 1.0),
            b: plane(2.0),
            contact: Contact {
                curves: vec![diag(1.0, |_| 0.0), diag(-1.0, |_| 0.0)],
                points: vec![Vec3::ZERO],
            },
        },
        Pair {
            name: "parabolic_cylinders_crossing_node",
            a: bezier_graph(&[(2, 0, 1.0)], 2, -1.0, 1.0),
            b: bezier_graph(&[(0, 2, 1.0)], 2, -1.0, 1.0),
            contact: Contact {
                curves: vec![diag(1.0, |x| x * x), diag(-1.0, |x| x * x)],
                points: vec![Vec3::ZERO],
            },
        },
    ]
}

/// How far a sample may be from the contact (or the result) and still count.
const NEAR: f64 = 1e-3;

fn seg_dist(p: Vec3, a: Vec3, b: Vec3) -> f64 {
    let d = b - a;
    let l2 = d.dot(d);
    let t = if l2 > 0.0 {
        ((p - a).dot(d) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + d * t)
}

fn poly_dist(p: Vec3, poly: &[Vec3]) -> f64 {
    if poly.len() == 1 {
        return p.distance(poly[0]);
    }
    poly.windows(2)
        .map(|w| seg_dist(p, w[0], w[1]))
        .fold(f64::INFINITY, f64::min)
}

/// What a result is, measured against the contact.
#[derive(Debug)]
struct Measure {
    /// The status error, if the intersection failed.
    error: Option<String>,
    /// The curve types, in order.
    types: Vec<TsectCurveType>,
    /// Per contact curve: the share of its samples a returned non-point curve covers.
    covered: Vec<f64>,
    /// Per contact point: whether a returned touching point (or a curve) passes within
    /// [`NEAR`].
    points_found: Vec<bool>,
    /// The largest distance from a sample of a returned curve to the contact.
    off: f64,
}

fn measure(r: &Result<SurfaceIntersection, solidcore::SolidError>, c: &Contact) -> Measure {
    let k = Kernel::default();
    let x = match r {
        Ok(x) => x,
        Err(e) => {
            return Measure {
                error: Some(e.to_string()),
                types: vec![],
                covered: vec![0.0; c.curves.len()],
                points_found: vec![false; c.points.len()],
                off: 0.0,
            }
        }
    };
    let mut polys: Vec<Vec<Vec3>> = Vec::new();
    let mut touching: Vec<Vec3> = Vec::new();
    for (cv, t) in x.curves_3d.iter().zip(&x.curve_types) {
        let ivl = cv.natural_interval();
        let pts: Vec<Vec3> = (0..=200)
            .filter_map(|j| {
                cv.evaluate_point(&k, ivl.evaluate(f64::from(j) / 200.0))
                    .ok()
            })
            .collect();
        if *t == TsectCurveType::Touching {
            touching.extend(pts.first().copied());
        } else {
            polys.push(pts);
        }
    }
    let mut targets: Vec<&[Vec3]> = c.curves.iter().map(Vec::as_slice).collect();
    let pt_polys: Vec<Vec<Vec3>> = c.points.iter().map(|&p| vec![p]).collect();
    targets.extend(pt_polys.iter().map(Vec::as_slice));
    let off = polys
        .iter()
        .flatten()
        .chain(&touching)
        .map(|&p| {
            targets
                .iter()
                .map(|t| poly_dist(p, t))
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max);
    let covered = c
        .curves
        .iter()
        .map(|cc| {
            let hit = cc
                .iter()
                .filter(|&&p| polys.iter().any(|poly| poly_dist(p, poly) < NEAR))
                .count();
            hit as f64 / cc.len() as f64
        })
        .collect();
    let points_found = c
        .points
        .iter()
        .map(|&p| {
            touching.iter().any(|&q| q.distance(p) < NEAR)
                || polys.iter().any(|poly| poly_dist(p, poly) < NEAR)
        })
        .collect();
    Measure {
        error: None,
        types: x.curve_types.clone(),
        covered,
        points_found,
        off,
    }
}

/// `GlobalSurfaceIntersect` over the natural domains at the corpus tolerance (1e-6).
fn intersect(
    fixes: Fixes,
    a: &Surface,
    b: &Surface,
) -> Result<SurfaceIntersection, solidcore::SolidError> {
    let mut k = Kernel::default();
    k.fixes.ssi_tangent_curve_tracer = fixes != Fixes::Upstream;
    k.fixes.ssi_tangent_contact_points = fixes == Fixes::TracerAndPoints;
    let d1 = a.natural_uv_domain_with_nurb(&k)?;
    let d2 = b.natural_uv_domain_with_nurb(&k)?;
    let tol = approx_tol3d(&k, 1e-6);
    a.global_surface_intersect(&k, &d1, b, &d2, [true, true], Some(tol), None)
}

/// The opt-in fixes a run turns on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fixes {
    /// None: upstream behaviour (difftest holds it at parity with the C++).
    Upstream,
    /// `ssi_tangent_curve_tracer` (WR-75, this issue).
    Tracer,
    /// `ssi_tangent_curve_tracer` and `ssi_tangent_contact_points` (WR-74, #394).
    TracerAndPoints,
}

fn run(p: &Pair, swapped: bool, fixes: Fixes) -> Measure {
    let (a, b) = if swapped { (&p.b, &p.a) } else { (&p.a, &p.b) };
    measure(&intersect(fixes, a, b), &p.contact)
}

/// Prints the whole corpus, both kernels, both operand orders (`--nocapture`).
#[test]
#[ignore = "survey: prints the corpus table"]
fn issue_395_survey() {
    for p in corpus() {
        for swapped in [false, true] {
            for fixes in [Fixes::Upstream, Fixes::Tracer, Fixes::TracerAndPoints] {
                let m = run(&p, swapped, fixes);
                eprintln!(
                    "{:<36} {:<8} {:<16} err={:?} covered={:?} points={:?} off={:.1e} types={:?}",
                    p.name,
                    if swapped { "swapped" } else { "" },
                    format!("{fixes:?}"),
                    m.error,
                    m.covered
                        .iter()
                        .map(|c| (c * 1000.0).round() / 10.0)
                        .collect::<Vec<_>>(),
                    m.points_found,
                    m.off,
                    m.types
                );
            }
        }
    }
}

fn pair(name: &str) -> Pair {
    corpus().into_iter().find(|p| p.name == name).unwrap()
}

/// With the tracer, every tangential (and crossing) contact curve of the corpus is
/// covered end to end by a returned curve, in both operand orders, and every returned
/// curve lies on the exact contact (within the sampling of the closed form).
#[test]
fn issue_395_tracer_covers_every_contact_curve() {
    for p in corpus().iter().filter(|p| !p.contact.curves.is_empty()) {
        for swapped in [false, true] {
            let m = run(p, swapped, Fixes::Tracer);
            assert!(m.error.is_none(), "{} {swapped}: {m:?}", p.name);
            assert!(
                m.covered.iter().all(|&c| c >= 0.995),
                "{} {swapped}: {m:?}",
                p.name
            );
            assert!(m.off < 1e-4, "{} {swapped}: {m:?}", p.name);
        }
    }
}

/// Upstream (the default kernel, at parity with the C++ in difftest's
/// `tc_quartic_plane_circle`, `tc_cubic_plane_line` and `tc_quartic_ridge_plane_line`):
/// the closed tangent circle of the quartic is missed, and the 3rd and 4th order
/// tangent lines collapse to their two end points. The tracer traces each as one
/// `Tangent` curve.
#[test]
fn issue_395_upstream_misses_or_collapses_high_order_contact() {
    for swapped in [false, true] {
        let circle = pair("quartic_plane_circle");
        let m = run(&circle, swapped, Fixes::Upstream);
        assert!(m.types.is_empty() && m.covered == [0.0], "{swapped}: {m:?}");
        let m = run(&circle, swapped, Fixes::Tracer);
        assert_eq!(m.types, [TsectCurveType::Tangent], "{swapped}");

        for name in ["cubic_plane_line", "quartic_ridge_plane_line"] {
            let line = pair(name);
            let m = run(&line, swapped, Fixes::Upstream);
            assert_eq!(
                m.types,
                [TsectCurveType::Touching, TsectCurveType::Touching],
                "{name} {swapped}"
            );
            assert_eq!(m.covered, [0.0], "{name} {swapped}");
            let m = run(&line, swapped, Fixes::Tracer);
            assert_eq!(m.types, [TsectCurveType::Tangent], "{name} {swapped}");
        }
    }
}

/// The second-order line and circle contacts of the corpus (cylinder, parabolic
/// cylinder, parallel cylinders, torus, sphere in a cylinder) are traced upstream
/// already, as one `Tangent` curve; the tracer leaves them as they are.
#[test]
fn issue_395_second_order_contacts_traced_upstream_and_unchanged() {
    for name in [
        "cylinder_plane_line",
        "parabolic_cylinder_plane_line",
        "cylinders_parallel_external_line",
        "cylinders_parallel_internal_line",
        "torus_plane_circle",
        "sphere_in_cylinder_circle",
    ] {
        let p = pair(name);
        for swapped in [false, true] {
            let up = run(&p, swapped, Fixes::Upstream);
            assert_eq!(up.types, [TsectCurveType::Tangent], "{name} {swapped}");
            assert!(up.covered.iter().all(|&c| c >= 0.995), "{name} {swapped}");
            let fixed = run(&p, swapped, Fixes::Tracer);
            assert_eq!(fixed.types, up.types, "{name} {swapped}");
            assert_eq!(fixed.covered, up.covered, "{name} {swapped}");
        }
    }
}

/// The Steinmetz crossing nodes `(0, +-1, 0)` are reported as touching points with the
/// tracer only; the ellipses through them are traced either way.
#[test]
fn issue_395_steinmetz_nodes_with_the_tracer() {
    let p = pair("steinmetz_equal_cylinders");
    for swapped in [false, true] {
        let touching = |m: &Measure| {
            m.types
                .iter()
                .filter(|&&t| t == TsectCurveType::Touching)
                .count()
        };
        let up = run(&p, swapped, Fixes::Upstream);
        assert_eq!(touching(&up), 0, "{swapped}: {up:?}");
        let fixed = run(&p, swapped, Fixes::Tracer);
        assert_eq!(touching(&fixed), 2, "{swapped}: {fixed:?}");
        assert!(
            fixed.points_found.iter().all(|&f| f),
            "{swapped}: {fixed:?}"
        );
    }
}

/// Isolated tangential contact points (sphere or paraboloid on a plane) are not this
/// issue's tracer's: upstream and the tracer return nothing; WR-74's
/// `ssi_tangent_contact_points` (#394) finds the touching point.
#[test]
fn issue_395_point_contacts_need_wr74() {
    for name in ["sphere_plane_point", "paraboloid_plane_point"] {
        let p = pair(name);
        for swapped in [false, true] {
            for fixes in [Fixes::Upstream, Fixes::Tracer] {
                let m = run(&p, swapped, fixes);
                assert!(m.types.is_empty(), "{name} {swapped} {fixes:?}: {m:?}");
            }
            let m = run(&p, swapped, Fixes::TracerAndPoints);
            assert_eq!(m.types, [TsectCurveType::Touching], "{name} {swapped}");
            assert_eq!(m.points_found, [true], "{name} {swapped}");
        }
    }
}

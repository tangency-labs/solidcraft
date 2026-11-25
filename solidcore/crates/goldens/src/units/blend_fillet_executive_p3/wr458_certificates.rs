//! WR-458, WR-633 and WR-650 (#18921): an offset surface extends from its base surface,
//! not from its last extension, and a cylinder, cone, sphere, torus or circle revolution
//! extends as itself. The real-solid rows this moves keep their status and counts and
//! change only their topology order, geometry and mass values. Each row is checked here,
//! one test per row, on the body the golden records (checked by a [`with_hook`] hook at the
//! record's own [`observe`] call, so it is the golden's body bit for bit):
//! - the record's status is a success and the body's vertex, edge and face counts are the
//!   golden's;
//! - `check_brep` Level 2 reports nothing (or, on the two rows of [`MAIN_LEVEL2`], exactly
//!   the lines main's body of that row reports: this change adds none) and, for a closed
//!   result, the body is one closed manifold solid;
//! - every blend face that is a definition re-certifies its net within its bound;
//! - for a closed result, the volume the mass integrator converges to lies in the body's
//!   certified volume enclosure ([`enclose_volume`], widened by the definitions' spread),
//!   and every other mass value lies in its certified band ([`mass_bands`]). This is a
//!   consistency check: it certifies that the recorded values are this body's.
//!
//! [`with_hook`]: crate::trim_hook::with_hook
//! [`observe`]: crate::trim_hook::observe
//! [`enclose_volume`]: solidcore::mass::enclosure::enclose_volume
//! [`mass_bands`]: super::round_definition_proof::mass_bands

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::round_definition_proof::{mass_bands, volume_band};
use super::round_definition_rows::{counts, inputs};
use super::*;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::FaceId;
use std::cell::RefCell;
use std::rc::Rc;

/// A moved row: name, main's volume (`None`: an open result), and its status and counts
/// (the same on main).
type Row = (&'static str, Option<f64>, [i64; 4]);

const ROWS: &[Row] = &[
    ("corner3x2-union-inset-side-step-edges1-14-r1", Some(2600.6420851684975), [0, 25, 38, 15]),
    ("corner3x2-union-inset-side-step-edges12-13-r1", Some(2600.6420849381543), [0, 25, 38, 15]),
    ("curve-sih-box_diff_cyl-all-edges-r0.25", Some(949.2763524350879), [0, 40, 76, 39]),
    ("issue_2020_mitred_put_back_d9_full", Some(1006.3206273131401), [0, 8, 12, 6]),
    ("issue_2176_tabbed_plate_tab_side_r0_5", Some(4054.672334381553), [0, 14, 21, 9]),
    ("issue_2176_tabbed_plate_tab_top_r0_5", Some(4054.77021264696), [0, 14, 21, 9]),
    ("source-file-box_with_large_fillet-edge4", Some(862.6278909025616), [0, 14, 22, 10]),
    ("source-file-corner_box-all-edges", Some(455.9755263923814), [0, 30, 60, 32]),
    ("source-file-nurb_box-all-edges", Some(899.6454001648498), [0, 24, 48, 26]),
    ("stubs3-union-inset-side-step-edges13-17-r1", Some(2602.8856942157045), [0, 23, 35, 14]),
    ("stubs3-union-inset-side-step-edges14-15-r1", Some(2603.9587033987177), [0, 23, 35, 14]),
    ("stubs3-union-inset-side-step-edges14-18-r1", Some(2602.8856942157045), [0, 23, 35, 14]),
    ("stubs3-union-overhang-corner-edges14-15-r1", Some(2602.6710923791024), [0, 23, 35, 14]),
    ("stubs3-union-overhang-front-edges14-15-r1", Some(2603.100296052308), [0, 22, 34, 14]),
    ("stubs4-Heart-all-edges-r0.1", Some(326.05828850759485), [0, 14, 26, 14]),
    ("stubs4-coons_01_03_29-e3-r0.5", None, [0, 8, 12, 5]),
    ("stubs4-degenerate-brepForFillet2-e7-r0.5", Some(3614.981459311338), [0, 11, 17, 8]),
    ("stubs4-degenerate-brepForFillet2-e9-r0.5", Some(3616.232731993582), [0, 11, 17, 8]),
    ("sweep5-ext_w_hole-e15-r1.0", Some(234.29244337666438), [0, 16, 24, 10]),
    ("sweep5-nurb_box1-all-r0.5", Some(3793.323246921), [0, 18, 36, 20]),
];

/// One test per moved row, so the test runner spreads the rows over its threads.
macro_rules! row_tests {
    ($($test:ident => $name:literal,)*) => {
        $(
            #[test]
            fn $test() {
                check($name);
            }
        )*

        /// The rows the generated tests take, in [`ROWS`]'s order.
        const ROW_TESTS: &[&str] = &[$($name),*];
    };
}

row_tests! {
    corner3x2_union_inset_side_step_edges1_14_r1 => "corner3x2-union-inset-side-step-edges1-14-r1",
    corner3x2_union_inset_side_step_edges12_13_r1 => "corner3x2-union-inset-side-step-edges12-13-r1",
    curve_sih_box_diff_cyl_all_edges_r0_25 => "curve-sih-box_diff_cyl-all-edges-r0.25",
    issue_2020_mitred_put_back_d9_full => "issue_2020_mitred_put_back_d9_full",
    issue_2176_tabbed_plate_tab_side_r0_5 => "issue_2176_tabbed_plate_tab_side_r0_5",
    issue_2176_tabbed_plate_tab_top_r0_5 => "issue_2176_tabbed_plate_tab_top_r0_5",
    source_file_box_with_large_fillet_edge4 => "source-file-box_with_large_fillet-edge4",
    source_file_corner_box_all_edges => "source-file-corner_box-all-edges",
    source_file_nurb_box_all_edges => "source-file-nurb_box-all-edges",
    stubs3_union_inset_side_step_edges13_17_r1 => "stubs3-union-inset-side-step-edges13-17-r1",
    stubs3_union_inset_side_step_edges14_15_r1 => "stubs3-union-inset-side-step-edges14-15-r1",
    stubs3_union_inset_side_step_edges14_18_r1 => "stubs3-union-inset-side-step-edges14-18-r1",
    stubs3_union_overhang_corner_edges14_15_r1 => "stubs3-union-overhang-corner-edges14-15-r1",
    stubs3_union_overhang_front_edges14_15_r1 => "stubs3-union-overhang-front-edges14-15-r1",
    stubs4_heart_all_edges_r0_1 => "stubs4-Heart-all-edges-r0.1",
    stubs4_coons_01_03_29_e3_r0_5 => "stubs4-coons_01_03_29-e3-r0.5",
    stubs4_degenerate_brepforfillet2_e7_r0_5 => "stubs4-degenerate-brepForFillet2-e7-r0.5",
    stubs4_degenerate_brepforfillet2_e9_r0_5 => "stubs4-degenerate-brepForFillet2-e9-r0.5",
    sweep5_ext_w_hole_e15_r1_0 => "sweep5-ext_w_hole-e15-r1.0",
    sweep5_nurb_box1_all_r0_5 => "sweep5-nurb_box1-all-r0.5",
}

/// Every row of [`ROWS`] has exactly one test.
#[test]
fn every_moved_row_has_its_own_test() {
    let rows: Vec<&str> = ROWS.iter().map(|row| row.0).collect();
    assert_eq!(rows, ROW_TESTS);
}

/// The rows whose body on main already reports Level 2 lines (the same check on main's
/// recorded body of the row, at this branch's merge base): each line and how many times.
/// The moved body reports exactly these, so the lines are main's, not this change's. They
/// are kept as main's facts here, not excused: any other line, or a count that differs,
/// fails.
const MAIN_LEVEL2: &[(&str, &[(&str, usize)])] = &[
    (
        "issue_2176_tabbed_plate_tab_side_r0_5",
        &[(
            "SmBSplineCurve::Test #11 Bad SpeedPt: 2D BSplineCurve Warning: Interior 1st Deriv \
             too slow, less than AvgSpeed/10000 (often an almost kink or almost singular point - \
             never good practice) (offset=2.5724395e-12, tolerance=6.6664096e-05) [owner \
             SmBSplineCurve]",
            1,
        )],
    ),
    (
        "sweep5-nurb_box1-all-r0.5",
        &[
            (
                "SmBrep::Test #5 Tolerance: Face tolerance relative to brep tolerance [owner \
                 SmFace]",
                15,
            ),
            (
                "SmBrep::Test #59 Tolerance: Edge tolerance relative to brep tolerance \
                 (offset=0.020938086, tolerance=  0.01) [owner SmEdge]",
                36,
            ),
            (
                "SmBrep::Test #82 Tolerance: Vertex tolerance is too wide to excuse by clearance \
                 (offset=0.020938086, tolerance=  0.02) [owner SmVertex]",
                18,
            ),
            ("AssertValid skipped: ValidatePointers failed", 1),
        ],
    ),
];

/// The faces of `b` whose surface is a blend by its definition, each with its proved net
/// distance (`recertify`, at most its bound); none is fine (a bare net has no spread).
fn definitions(k: &Kernel, b: &Brep, name: &str) -> Vec<(FaceId, f64)> {
    let mut blends = Vec::new();
    for f in p1::faces(b, None).unwrap() {
        let surface = b.face(f).unwrap().surface.clone();
        let Some(blend) = surface.as_ref().and_then(Surface::procedural_blend) else {
            continue;
        };
        let d = blend
            .recertify(k)
            .unwrap_or_else(|e| panic!("{name}: {f:?} does not re-certify: {e}"));
        let bound = blend.bound();
        assert!(d <= bound, "{name}: {f:?} proved {d:e} past its bound {bound:e}");
        blends.push((f, d));
    }
    blends
}

/// The [`ROWS`] row named `row`, as the module header says.
fn check(row: &str) {
    let picked: Vec<&Row> = ROWS.iter().filter(|r| r.0 == row).collect();
    assert_eq!(picked.len(), 1, "{row}: one row of that name");
    for &&(name, old_volume, after) in &picked {
        let input = inputs(name).real_solid.expect("a real solid");
        assert_eq!(input.checkpoint, Checkpoint::Full, "{name}");
        let seen: Rc<RefCell<usize>> = Rc::default();
        let into = Rc::clone(&seen);
        // the checks run on the recorded body itself, at the record's `observe` call
        let output = crate::trim_hook::with_hook(
            Box::new(move |body: &mut Brep| {
                *into.borrow_mut() += 1;
                certify(body, name, old_volume, after);
            }),
            || rust(&input),
        );
        let output = output.unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let recorded = output.ints.get("api_status_vertex_edge_face_counts");
        assert_eq!(recorded.map(Vec::as_slice), Some(&after[..]), "{name}: a success, the golden's counts");
        assert_eq!(*seen.borrow(), 1, "{name}: one recorded body");
    }
}

/// The certificates of the recorded body of row `name`, as the module header says.
fn certify(body: &mut Brep, name: &str, old_volume: Option<f64>, after: [i64; 4]) {
    assert_eq!(counts(body, 0), after, "{name}");
    let mut k = Kernel::default();
    let lines = check_brep(&mut k, body, CheckLevel::Level2).unwrap().lines();
    let mut main: Vec<(&str, usize)> =
        MAIN_LEVEL2.iter().find(|r| r.0 == name).map_or(&[][..], |r| r.1).to_vec();
    main.sort_unstable();
    let mut got: Vec<(&str, usize)> = Vec::new();
    for l in &lines {
        match got.iter_mut().find(|g| g.0 == l.as_str()) {
            Some(g) => g.1 += 1,
            None => got.push((l.as_str(), 1)),
        }
    }
    got.sort_unstable();
    assert_eq!(got, main, "{name}: Level 2 lines other than main's: {lines:?}");
    let blends = definitions(&k, body, name);
    if let Some(old) = old_volume {
        assert!(is_manifold_solid(body).unwrap(), "{name}: not one closed manifold solid");
        let estimates = volume_band(&k, body, &blends, old, name);
        mass_bands(&k, body, &estimates, name);
    }
}

/// The surface-surface row the extension moves (`surface-surface-mirror-complement-approx`:
/// a round between two bilinear sheets, open). It moves only its geometry. Its result is
/// one B-spline face, its counts are the golden's, and it lies within the row's tolerance
/// of the closed form, the mirrored complement of the radius 2 round between the two
/// sheets' planes: the cylinder of radius 2 about x = 5, z = 0, on its 270 degree arc.
/// The rails' ends, extended from the base, put one section's arc a knot over the others',
/// so the sections take a second pass; each is mirrored once (WR-21288).
#[test]
fn surface_surface_row_is_a_certified_definition() {
    use solidcore::blend::fillet_executive::api::{surface_surface_fillet, FilletSurfaceArg};
    use solidcore::nurbs::b_spline_surface::BSplineSurface;
    let name = "surface-surface-mirror-complement-approx";
    let input = inputs(name).surface_surface.expect("a surface-surface input");
    let mut k = crate::runner_kernel();
    let sheet = |c: &[[f64; 3]; 4]| {
        let [a, b, c, d] = c.map(Vec3::from);
        Surface::from(BSplineSurface::create_bilinear_surface(a, b, c, d).unwrap())
    };
    let (first, second) = (sheet(&input.first), sheet(&input.second));
    let body = surface_surface_fillet(
        &mut k,
        FilletSurfaceArg::Free(&first),
        FilletSurfaceArg::Free(&second),
        input.radius1,
        input.radius2,
        input.tolerance,
        input.xsect,
        input.mirror,
        input.complement,
    )
    .unwrap()
    .expect("a result body");
    assert_eq!(counts(&body, 0), [0, 4, 4, 1], "{name}");
    // the closed form: the sheets are the planes z = 0 and x = 5 (each extends as
    // itself), so a round of radius 2 between them has its centre on the line x = 3,
    // z = 2 and touches them at x = 3, z = 0 and x = 5, z = 2. Mirrored about that chord
    // (`mirror`), the section is the circle of radius 2 about x = 5, z = 0, and its
    // complement (`complement`) is the 270 degree arc through x = 5 + sqrt 2,
    // z = -sqrt 2: the cylinder of radius 2 about the line x = 5, z = 0 (parallel to y).
    assert_eq!(
        (input.first, input.second, input.radius1, input.radius2),
        (
            [[0., 0., 0.], [10., 0., 0.], [0., 10., 0.], [10., 10., 0.]],
            [[5., 0., -5.], [5., 0., 5.], [5., 10., -5.], [5., 10., 5.]],
            2.0,
            2.0
        ),
        "{name}: the closed form's input"
    );
    let faces = p1::faces(&body, None).unwrap();
    assert_eq!(faces.len(), 1, "{name}: one face");
    let surface = body.face(faces[0]).unwrap().surface.clone().unwrap();
    let SurfaceKind::BSpline(net) = surface.kind() else {
        panic!("{name}: not a B-spline face")
    };
    let worst = cylinder_bound(net.gw_nurb().expect("a NURB"), [5.0, 0.0], 2.0);
    eprintln!("{name}: off the cylinder by at most {worst:e} (tolerance {})", input.tolerance);
    assert!(worst <= input.tolerance, "{name}: {worst:e} off the cylinder");
    // the complement's arc, not the quarter: every section's middle is the arc's far point
    let d = surface.natural_uv_domain_with_nurb(&k).unwrap();
    for f in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let u = d.u_interval().min() + f * (d.u_interval().max() - d.u_interval().min());
        let v = 0.5 * (d.v_interval().min() + d.v_interval().max());
        let p = surface.evaluate_point(&k, solidcore::math::Point2::new(u, v)).unwrap();
        let far = ((p.x - (5.0 + 2f64.sqrt())).powi(2) + (p.z + 2f64.sqrt()).powi(2)).sqrt();
        assert!(far <= input.tolerance, "{name}: the middle of the section at u = {u} is {far:e} off the arc's far point");
    }
}

/// A bound on the distance of a polynomial bicubic-or-less net from the cylinder of
/// radius `r` about the line `(x, z) = axis` parallel to y: on every Bezier patch (the net
/// split at its knots by Boehm insertion) `F = (x - ax)^2 + (z - az)^2 - r^2` is a
/// polynomial of degree twice the net's in each parameter, whose Bernstein coefficients
/// (products of the patch's) bound it, and `rho - r = F / (rho + r)` with `rho >= 0`, so
/// `|rho - r| <= max |F| / r`. The insertion's rounding (convex combinations, a few ulps
/// of the coordinates, about `1e-14` here) is far below the tolerance it is compared to.
fn cylinder_bound(n: &solidcore::nurbs::srf_basic::Surface, axis: [f64; 2], r: f64) -> f64 {
    use solidcore::nurbs::math::NOW;
    let (p, q) = n.degrees();
    let rows: Vec<Vec<[f64; 2]>> = n
        .cpts()
        .iter()
        .map(|row| {
            row.iter()
                .map(|c| {
                    assert!(c.w == NOW, "a polynomial net");
                    [c.x - axis[0], c.z - axis[1]]
                })
                .collect()
        })
        .collect();
    // split along v (each row), then along u (each column of every v segment)
    let by_v: Vec<Vec<Vec<[f64; 2]>>> =
        rows.iter().map(|row| bezier_segments(n.v_knots(), q, row)).collect();
    let segs_v = by_v[0].len();
    let mut worst = 0.0_f64;
    for sv in 0..segs_v {
        let cols: Vec<Vec<[f64; 2]>> =
            (0..=q).map(|j| by_v.iter().map(|r| r[sv][j]).collect()).collect();
        let by_u: Vec<Vec<Vec<[f64; 2]>>> =
            cols.iter().map(|c| bezier_segments(n.u_knots(), p, c)).collect();
        for (su, _) in by_u[0].iter().enumerate() {
            // patch[i][j]: u index i, v index j
            let patch: Vec<Vec<[f64; 2]>> =
                (0..=p).map(|i| (0..=q).map(|j| by_u[j][su][i]).collect()).collect();
            let sq = |c: usize| bernstein_square(&patch, p, q, c);
            let (xx, zz) = (sq(0), sq(1));
            for (a, b) in xx.iter().flatten().zip(zz.iter().flatten()) {
                worst = worst.max((a + b - r * r).abs());
            }
        }
    }
    worst / r
}

/// The Bernstein coefficients (degree `2p` by `2q`) of the square of coordinate `c` of a
/// Bezier patch of degree `p` by `q`.
fn bernstein_square(patch: &[Vec<[f64; 2]>], p: usize, q: usize, c: usize) -> Vec<Vec<f64>> {
    let binom = |n: usize, k: usize| -> f64 {
        (0..k).fold(1.0, |a, i| a * (n - i) as f64 / (i + 1) as f64)
    };
    let mut out = vec![vec![0.0; 2 * q + 1]; 2 * p + 1];
    for (i1, r1) in patch.iter().enumerate() {
        for (i2, r2) in patch.iter().enumerate() {
            for (j1, a) in r1.iter().enumerate() {
                for (j2, b) in r2.iter().enumerate() {
                    let w = binom(p, i1) * binom(p, i2) / binom(2 * p, i1 + i2)
                        * binom(q, j1)
                        * binom(q, j2)
                        / binom(2 * q, j1 + j2);
                    out[i1 + i2][j1 + j2] += w * a[c] * b[c];
                }
            }
        }
    }
    out
}

/// The Bezier segments of a B-spline polygon of degree `p` on clamped `knots`: every
/// interior knot raised to multiplicity `p` by Boehm insertion.
fn bezier_segments(knots: &[f64], p: usize, pts: &[[f64; 2]]) -> Vec<Vec<[f64; 2]>> {
    let mut u = knots.to_vec();
    let mut pw = pts.to_vec();
    let (lo, hi) = (u[p], u[u.len() - 1 - p]);
    let interior: Vec<f64> = {
        let mut v: Vec<f64> = u.iter().copied().filter(|&t| t > lo && t < hi).collect();
        v.dedup();
        v
    };
    for t in interior {
        while u.iter().filter(|&&x| x == t).count() < p {
            // span k: u[k] <= t < u[k+1]
            let k = (0..u.len() - 1).rfind(|&k| u[k] <= t && t < u[k + 1]).unwrap();
            let mut next = Vec::with_capacity(pw.len() + 1);
            for i in 0..=pw.len() {
                if i + p <= k {
                    next.push(pw[i]);
                } else if i > k {
                    next.push(pw[i - 1]);
                } else {
                    let a = (t - u[i]) / (u[i + p] - u[i]);
                    let (b, c) = (pw[i - 1], pw[i]);
                    next.push([(1.0 - a) * b[0] + a * c[0], (1.0 - a) * b[1] + a * c[1]]);
                }
            }
            pw = next;
            u.insert(k + 1, t);
        }
    }
    pw.windows(p + 1).step_by(p).map(<[[f64; 2]]>::to_vec).collect()
}

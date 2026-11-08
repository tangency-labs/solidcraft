//! Issue #1999: shallow crease rounds preserve contact, definition and volume.
//! Regression using the roof fixture from superseded PR #2003; review follow-up to #2530.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;

use solidcore::blend::definition_check::{BlendFaceKind, BlendFaceLog};
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::curve::curve::Curve;
use solidcore::mass::integrator::properties::brep_volume_default;
use solidcore::math::Vec3;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::surface::procedural_blend::BlendNet;
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::edge;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

fn line(a: (f64, f64), b: (f64, f64)) -> Curve {
    Curve::from(create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap())
}

/// The rise of the crease above y = 1 for a turn of `turn` degrees.
fn rise(turn: f64) -> f64 {
    3.0 * (0.5 * turn * PI / 180.0).tan()
}

/// The 6 x 1 roof profile, extruded 2 along z.
fn block(k: &mut Kernel, turn: f64) -> Brep {
    block_scaled(k, turn, 1.0)
}

fn block_scaled(k: &mut Kernel, turn: f64, scale: f64) -> Brep {
    let line =
        |a: (f64, f64), b: (f64, f64)| line((a.0 * scale, a.1 * scale), (b.0 * scale, b.1 * scale));
    let h = rise(turn);
    let curves = [
        line((0.0, 0.0), (6.0, 0.0)),
        line((6.0, 0.0), (6.0, 1.0)),
        line((6.0, 1.0), (3.0, 1.0 + h)),
        line((3.0, 1.0 + h), (0.0, 1.0)),
        line((0.0, 1.0), (0.0, 0.0)),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_taper_extrude(k, &refs, 2.0 * scale, 0.0, 3, false).unwrap()
}

/// The crease: the edge along z at (3, 1 + rise).
fn crease(k: &Kernel, b: &Brep, turn: f64) -> EdgeId {
    crease_scaled(k, b, turn, 1.0)
}

fn crease_scaled(k: &Kernel, b: &Brep, turn: f64, scale: f64) -> EdgeId {
    let h = rise(turn);
    let on = |p: Vec3| {
        (p.x - 3.0 * scale).abs() < 1e-9 * scale && (p.y - (1.0 + h) * scale).abs() < 1e-9 * scale
    };
    b.edge_list()
        .iter()
        .copied()
        .find(|&e| {
            let curve = edge::curve(b, e).unwrap().unwrap();
            let ivl = edge::interval(b, e).unwrap();
            let p = curve.evaluate_point(k, ivl.min()).unwrap();
            let q = curve.evaluate_point(k, ivl.max()).unwrap();
            on(p) && on(q) && (p.z - q.z).abs() > scale
        })
        .expect("the crease edge")
}

// Preserve default-path characterizations and reject invalid success in the new blend.
#[test]
fn shallow_crease_round_contact_profiles() {
    use solidcore::kernel::UpstreamFixes;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    let mut invalid = Vec::new();
    for profile in ["default", "half_degree", "new_blend", "all_fixes"] {
        for (turn, radius, multi) in [
            (2.5, 0.12, false),
            (0.6, 2.0, false),
            (1.5, 0.5, false),
            (1.0, 2.0, false),
            (1.0, 20.0, false),
            (2.5, 2.0, false),
            (2.5, 1.0, true),
            (1.5, 1.0, true),
            (1.9, 1.0, true),
        ] {
            let mut k = Kernel::default();
            if profile == "half_degree" {
                k.fixes.fillet_half_degree_crease = true;
            }
            if profile == "new_blend" {
                // The current generic-blend construction and supporting fillet fixes;
                // leave unrelated boolean, sweep and validation fixes at default.
                for &name in UpstreamFixes::NAMES {
                    if name.starts_with("fillet_") || name.starts_with("corner_") {
                        *k.fixes.flag_mut(name).unwrap() = true;
                    }
                }
                k.fixes.curve_check_zero_speed_tangent_end = true;
            }
            if profile == "all_fixes" {
                k.fixes = UpstreamFixes::all();
                k.fixes.geometric_g2_continuity = false;
            }
            if turn == 2.5 && radius == 0.12 {
                let enabled: Vec<_> = k
                    .fixes
                    .flags()
                    .filter(|(_, on)| *on)
                    .map(|(n, _)| n)
                    .collect();
                println!("FLAGS {profile}: {enabled:?}");
            }
            let mut b = block(&mut k, turn);
            let before = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
            assert!(
                before.is_valid(),
                "invalid input {profile}: {:?}",
                before.lines()
            );
            let edges = if multi {
                let ridge = Vec3::new(3.0, 1.0 + rise(turn), 0.0);
                let edges: Vec<_> = b
                    .edge_list()
                    .iter()
                    .copied()
                    .filter(|&e| {
                        let c = edge::curve(&b, e).unwrap().unwrap();
                        let iv = edge::interval(&b, e).unwrap();
                        [iv.min(), iv.max()]
                            .into_iter()
                            .any(|t| c.evaluate_point(&k, t).unwrap().distance(ridge) < 1e-9)
                    })
                    .collect();
                assert_eq!(edges.len(), 3);
                edges
            } else {
                vec![crease(&k, &b, turn)]
            };
            let result = fillet_edges(&mut k, &mut b, &edges, radius, 1, 1, 1.0);
            let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
            let manifold = is_manifold_solid(&b).unwrap();
            println!("CASE profile={profile} turn={turn} radius={radius} multi={multi} result={result:?} faces={} manifold={manifold} valid={} pointers={} failures={:?}", b.face_list().len(), check.is_valid(), check.pointers_status, check.failures().collect::<Vec<_>>());
            if result.is_ok()
                && !check.is_valid()
                && (profile == "new_blend" || profile == "all_fixes")
            {
                invalid.push(format!("{profile}/{turn}/{radius}/multi={multi}"));
            }
        }
    }
    assert!(
        invalid.is_empty(),
        "new construction still returns invalid success: {invalid:?}"
    );
}
/// The original failures must produce valid rounds in the current construction.
#[test]
fn new_blend_constructs_valid_shallow_crease_rounds() {
    use solidcore::kernel::UpstreamFixes;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    for scale in [0.1, 1.0, 10.0] {
        for (turn, radius) in [
            (2.5, 0.12),
            (0.6, 2.0),
            (1.5, 0.5),
            (1.0, 2.0),
            (1.0, 20.0),
            (2.5, 2.0),
        ] {
            let mut k = Kernel {
                fixes: UpstreamFixes::all(),
                blend_face_log: Some(BlendFaceLog::default()),
                ..Kernel::default()
            };
            k.fixes.geometric_g2_continuity = false;
            k.tolerances.zone_tol_3d *= scale;
            let mut b = block_scaled(&mut k, turn, scale);
            // The builder starts with fixed 1e-5 entity tolerances. Scale those
            // input tolerances as well as the geometry and kernel tolerance.
            let tol = 1e-5 * scale;
            b.set_tolerance(tol);
            for f in b.face_list().to_vec() {
                solidcore::topology::face::p3::set_tolerance(&mut b, f, tol, false, false).unwrap();
            }
            for e in b.edge_list().to_vec() {
                edge::set_tolerance(&mut b, e, tol, false, false).unwrap();
            }
            let vertices: Vec<_> = b.vertices().iter().map(|(id, _)| id).collect();
            for v in vertices {
                solidcore::topology::vertex::set_tolerance(&mut b, v, tol, false, false).unwrap();
            }
            let e = crease_scaled(&k, &b, turn, scale);
            fillet_edges(&mut k, &mut b, &[e], radius * scale, 1, 1, 1.0)
                .unwrap_or_else(|e| panic!("turn={turn} radius={radius} scale={scale}: {e:?}"));
            let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
            assert!(
                check.is_valid(),
                "turn={turn} radius={radius} scale={scale}: {:?}",
                check.lines()
            );
            assert!(is_manifold_solid(&b).unwrap());
            assert_eq!(b.face_list().len(), 8, "turn={turn} radius={radius}");
            assert_eq!(b.tolerance(), tol);

            // Check the face the blend actually made, not just whichever surfaces
            // happen to expose a definition (which could silently find none).
            let made = k.blend_face_log.as_ref().unwrap().last();
            assert_eq!(made.len(), 1, "turn={turn} radius={radius} scale={scale}");
            assert_eq!(made[0].kind, BlendFaceKind::ExactRound);
            let surface = b.face(made[0].face).unwrap().surface.as_ref().unwrap();
            let definition = surface.procedural_blend().unwrap_or_else(|| {
                panic!("bare round face: turn={turn} radius={radius} scale={scale}")
            });
            assert!(matches!(definition.net(), BlendNet::Exact(_)));
            assert_eq!(definition.bound(), 0.0);
            assert_eq!(definition.recertify(&k).unwrap(), 0.0);

            // Interior angle theta = pi - turn. The two tangent triangles minus
            // their circular sector remove r^2 * (cot(theta/2) - (pi-theta)/2)
            // of section. Here that is r^2 * (tan(turn/2) - turn/2).
            // The unrounded section is the 6x1 rectangle plus the roof triangle;
            // extruding it by 2 and scaling all lengths gives the volume below.
            let half_turn = 0.5 * turn * PI / 180.0;
            let part_volume = (6.0 + 3.0 * rise(turn)) * 2.0 * scale.powi(3);
            let removed_volume =
                radius * radius * (half_turn.tan() - half_turn) * 2.0 * scale.powi(3);
            let expected = part_volume - removed_volume;
            let rounding = 1e-9 * part_volume;
            assert!(
                removed_volume > rounding,
                "the gate must detect a missing round"
            );
            let volume = brep_volume_default(&k, &mut b, 1e-11).unwrap();
            assert!(
                (volume - expected).abs() <= rounding,
                "turn={turn} radius={radius} scale={scale}: volume={volume} expected={expected} rounding={rounding}"
            );
            for &edge_id in b.edge_list() {
                let curve = edge::curve(&b, edge_id).unwrap().unwrap();
                let interval = edge::interval(&b, edge_id).unwrap();
                for (parameter, vertex) in [
                    (
                        interval.min(),
                        edge::start_vertex(&b, edge_id).unwrap().unwrap(),
                    ),
                    (
                        interval.max(),
                        edge::end_vertex(&b, edge_id).unwrap().unwrap(),
                    ),
                ] {
                    let gap = curve
                        .evaluate_point(&k, parameter)
                        .unwrap()
                        .distance(b.vertex(vertex).unwrap().point);
                    assert!(
                        gap < 1e-9 * scale,
                        "turn={turn} radius={radius} scale={scale} edge={edge_id:?} gap={gap}"
                    );
                }
            }
        }
    }
}

/// Vary the crease and sagitta independently across both tolerance boundaries.
#[test]
fn new_blend_vertex_contact_boundary() {
    use solidcore::kernel::UpstreamFixes;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    for turn in [0.6, 1.5, 2.5, 5.0, 10.0, 30.0] {
        for sagitta in [1.9e-5, 2.1e-5, 4.9e-5, 5.1e-5] {
            let radius = sagitta / (1.0 / (0.5 * turn * PI / 180.0).cos() - 1.0);
            let mut k = Kernel {
                fixes: UpstreamFixes::all(),
                ..Kernel::default()
            };
            k.fixes.geometric_g2_continuity = false;
            let mut b = block(&mut k, turn);
            let e = crease(&k, &b, turn);
            fillet_edges(&mut k, &mut b, &[e], radius, 1, 1, 1.0)
                .unwrap_or_else(|e| panic!("turn={turn} sagitta={sagitta}: {e:?}"));
            let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
            assert!(
                check.is_valid(),
                "turn={turn} sagitta={sagitta}: {:?}",
                check.lines()
            );
            assert!(is_manifold_solid(&b).unwrap());
            assert_eq!(b.face_list().len(), 8, "turn={turn} radius={radius}");
        }
    }
}

/// Entity allowances must not replace the body's stricter incidence contract.
#[test]
fn new_blend_contact_resolution_is_body_tolerance() {
    use solidcore::kernel::UpstreamFixes;
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    for entity_tol in [4e-5, 1e-4] {
        let mut k = Kernel {
            fixes: UpstreamFixes::all(),
            ..Kernel::default()
        };
        k.fixes.geometric_g2_continuity = false;
        let mut b = block(&mut k, 2.5);
        for f in b.face_list().to_vec() {
            solidcore::topology::face::p3::set_tolerance(&mut b, f, entity_tol, false, false)
                .unwrap();
        }
        let vertices: Vec<_> = b.vertices().iter().map(|(id, _)| id).collect();
        for v in vertices {
            solidcore::topology::vertex::set_tolerance(&mut b, v, entity_tol, false, false)
                .unwrap();
        }
        assert!(check_brep(&mut k, &mut b, CheckLevel::Level2)
            .unwrap()
            .is_valid());
        let e = crease(&k, &b, 2.5);
        fillet_edges(&mut k, &mut b, &[e], 0.12, 1, 1, 1.0).unwrap();
        let check = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        assert!(
            check.is_valid(),
            "entity_tol={entity_tol}: {:?}",
            check.lines()
        );
        assert_eq!(b.tolerance(), 1e-5);
        assert_eq!(b.face_list().len(), 8);
    }
}

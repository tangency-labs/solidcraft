//! Issue #21432: preserving cached topology gaps across B-rep copies so a small
//! boolean cut on a multi-hole part does not re-measure every untouched edge and
//! vertex, while ensuring mutators (scale, interval edit, split, vertex/edge glue,
//! and analytic surface promotion) invalidate stale caches accurately.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::mass::integrator::properties::brep_volume_default;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::primitive::api_curves::{create_circle, create_line_segment};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::surface::surface::SurfaceKind;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::brep::p2::{
    copy_source_ref, create_missing_uv_trim_curves, ArenaNotifyState,
};
use solidcore::topology::brep::p3::{brep_copy, brep_copy_mapped, copy_brep, merge_brep};
use solidcore::topology::brep::p5::{glue_vertices, transform, turn_to_nurbs_plain};
use solidcore::topology::brep::p7::glue_edges_general;
use solidcore::topology::brep::validation::{
    check_brep, check_brep_full, update_tolerances_after_boolean, CheckLevel, ValidityScope,
};
use solidcore::topology::edge::{self, set_interval};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p2 as face2;
use solidcore::topology::vertexuse as vu1;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId, VertexId, VertexuseId};
use solidcore::{EntityRef, Kernel};
use std::f64::consts::PI;

/// A `side` by `side` by 10 plate with an `n` by `n` grid of circular holes of
/// radius 3 spaced by 20, swept linearly along +Z with caps.
fn perforated_plate(k: &mut Kernel, n: u32) -> Brep {
    let side = 20.0 * f64::from(n);
    let corners = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(side, 0.0, 0.0),
        Point3::new(side, side, 0.0),
        Point3::new(0.0, side, 0.0),
    ];
    let mut curves: Vec<Curve> = (0..4)
        .map(|i| Curve::from(create_line_segment(corners[i], corners[(i + 1) % 4]).unwrap()))
        .collect();
    for i in 0..n * n {
        let centre = Point3::new(
            10.0 + f64::from(i % n) * 20.0,
            10.0 + f64::from(i / n) * 20.0,
            0.0,
        );
        curves.push(Curve::from(create_circle(centre, 3.0).unwrap()));
    }
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 10.0, true).unwrap()
}

/// Populates all four gap caches on `b` (`max_edge_face_gap`, `edge_ccw_edge_gap`,
/// `vertex_edge_gap`, `vertex_face_gap`).
fn warm_all_gap_caches(k: &mut Kernel, b: &mut Brep) {
    create_missing_uv_trim_curves(k, b).unwrap();
    update_tolerances_after_boolean(k, b).unwrap();
    let eus: Vec<EdgeuseId> = b.edgeuses().ids().collect();
    for eu in eus {
        let _ = eu1::edge_ccw_edge_gap(k, b, eu, false).unwrap();
    }
}

/// `brep_copy` preserves all four cached gaps (`max_edge_face_gap`, `edge_ccw_edge_gap`,
/// `vertex_edge_gap`, `vertex_face_gap`) with remapped entity ids, and the copied body
/// matches the source's clean digests in `check_memo`.
#[test]
fn brep_copy_preserves_all_four_gap_caches_and_check_memo() {
    let mut k = Kernel::default();
    let mut plate = perforated_plate(&mut k, 3);
    warm_all_gap_caches(&mut k, &mut plate);

    let check = check_brep(&mut k, &mut plate, CheckLevel::Level2).unwrap();
    assert!(check.is_valid(), "{:?}", check.lines());

    let (mut copy, map) = brep_copy_mapped(&mut k, &plate).unwrap();
    let mapped_eu = |eu: EdgeuseId| -> EdgeuseId {
        match map.get_at(Some(EntityRef::Edgeuse(eu))).unwrap() {
            Some(EntityRef::Edgeuse(id)) => id,
            other => panic!("expected mapped edgeuse for {eu:?}, got {other:?}"),
        }
    };
    let mapped_vu = |vu: VertexuseId| -> VertexuseId {
        match map.get_at(Some(EntityRef::Vertexuse(vu))).unwrap() {
            Some(EntityRef::Vertexuse(id)) => id,
            other => panic!("expected mapped vertexuse for {vu:?}, got {other:?}"),
        }
    };

    let mut edge_face_preserved = 0usize;
    let mut edge_ccw_preserved = 0usize;
    for (eu, src) in plate.edgeuses().iter() {
        let dst = copy.edgeuse(mapped_eu(eu)).unwrap();
        if src.max_edge_face_gap.is_init() {
            assert!(dst.max_edge_face_gap.is_init());
            assert_eq!(
                dst.max_edge_face_gap.gap_3d.to_bits(),
                src.max_edge_face_gap.gap_3d.to_bits()
            );
            assert_eq!(
                dst.max_edge_face_gap.edge_t.to_bits(),
                src.max_edge_face_gap.edge_t.to_bits()
            );
            assert_eq!(
                dst.max_edge_face_gap.edgeuse,
                src.max_edge_face_gap.edgeuse.map(mapped_eu)
            );
            edge_face_preserved += 1;
        }
        if src.edge_ccw_edge_gap.is_init()
            && src.edge_ccw_edge_gap.edgeuse0 == Some(eu)
            && src.edge_ccw_edge_gap.edgeuse1 == src.ccw
        {
            assert!(dst.edge_ccw_edge_gap.is_init());
            assert_eq!(
                dst.edge_ccw_edge_gap.gap_3d.to_bits(),
                src.edge_ccw_edge_gap.gap_3d.to_bits()
            );
            assert_eq!(
                dst.edge_ccw_edge_gap.edgeuse0,
                src.edge_ccw_edge_gap.edgeuse0.map(mapped_eu)
            );
            assert_eq!(
                dst.edge_ccw_edge_gap.edgeuse1,
                src.edge_ccw_edge_gap.edgeuse1.map(mapped_eu)
            );
            edge_ccw_preserved += 1;
        }
    }
    assert!(edge_face_preserved > 0);
    assert!(edge_ccw_preserved > 0);

    let mut vertex_edge_preserved = 0usize;
    let mut vertex_face_preserved = 0usize;
    for (vu, src) in plate.vertexuses().iter() {
        let dst = copy.vertexuse(mapped_vu(vu)).unwrap();
        if src.vertex_edge_gap.is_init() {
            assert!(dst.vertex_edge_gap.is_init());
            assert_eq!(
                dst.vertex_edge_gap.gap_3d.to_bits(),
                src.vertex_edge_gap.gap_3d.to_bits()
            );
            assert_eq!(
                dst.vertex_edge_gap.vertexuse,
                src.vertex_edge_gap.vertexuse.map(mapped_vu)
            );
            vertex_edge_preserved += 1;
        }
        if src.vertex_face_gap.is_init() {
            assert!(dst.vertex_face_gap.is_init());
            assert_eq!(
                dst.vertex_face_gap.gap_3d.to_bits(),
                src.vertex_face_gap.gap_3d.to_bits()
            );
            assert_eq!(
                dst.vertex_face_gap.vertexuse,
                src.vertex_face_gap.vertexuse.map(mapped_vu)
            );
            vertex_face_preserved += 1;
        }
        assert_eq!(dst.uv_point.x.to_bits(), src.uv_point.x.to_bits());
        assert_eq!(dst.uv_point.y.to_bits(), src.uv_point.y.to_bits());
    }
    assert!(vertex_edge_preserved > 0);
    assert!(vertex_face_preserved > 0);

    // Every entity of the copy matches the clean digest stored during the check of `plate`.
    let copy_check = check_brep(&mut k, &mut copy, CheckLevel::Level2).unwrap();
    assert!(copy_check.is_valid(), "{:?}", copy_check.lines());
    assert_eq!(
        k.check_memo.borrow().last_clean(),
        [
            copy.face_list().len(),
            copy.edge_list().len(),
            copy.vertex_list().len(),
        ]
    );
}

/// Cutting a small unrelated corner cylinder from a copied perforated plate does not
/// re-measure untouched hole edges in `update_tolerances_after_boolean`, and the real
/// boolean result passes a full uncached check.
#[test]
fn untouched_holes_are_not_remeasured_after_copy_and_corner_cut() {
    let mut k = Kernel::default();
    let mut plate = perforated_plate(&mut k, 4);
    warm_all_gap_caches(&mut k, &mut plate);

    // First verify the real (unperturbed) copy + cut + tolerance update + full check.
    let copied_real = brep_copy(&mut k, &plate).unwrap();
    let tool_real = create_cylinder(&mut k, Point3::new(0.0, 0.0, -2.0), 1.5, 14.0).unwrap();
    let mut cut_real = boolean_difference(&mut k, copied_real, tool_real).unwrap();
    update_tolerances_after_boolean(&mut k, &mut cut_real).unwrap();
    let full = check_brep_full(
        &mut k,
        &mut cut_real,
        CheckLevel::Level2,
        ValidityScope::Body,
    )
    .unwrap();
    assert!(full.is_valid(), "{:?}", full.lines());
    let volume = brep_volume_default(&k, &mut cut_real, 1e-7).unwrap();
    let expected_volume = 10.0 * (80.0 * 80.0 - 16.0 * PI * 9.0 - PI * 1.5 * 1.5 / 4.0);
    assert!(
        (volume - expected_volume).abs() <= 1e-6 * expected_volume,
        "expected corner-cut plate volume ~{expected_volume}, got {volume}"
    );

    // Now place a sub-tolerance sentinel in `max_edge_face_gap.gap_3d` on every edge use
    // belonging to a cylindrical hole wall (none of which touch the corner cut at (0, 0)).
    // If `brep_copy` preserves the cache and `update_tolerances_after_boolean` reuses it
    // without re-measuring, every untouched hole wall's edge uses keep the exact sentinel.
    let sentinel = 1.234_567_89e-11_f64;
    let hole_eus: Vec<EdgeuseId> = plate
        .edgeuses()
        .ids()
        .filter(|&eu| {
            eu1::face(&plate, eu)
                .ok()
                .flatten()
                .and_then(|f| plate.face(f).ok())
                .and_then(|f| f.surface.as_ref())
                .is_some_and(|s| matches!(s.kind(), SurfaceKind::Cone(_)))
        })
        .collect();
    assert!(!hole_eus.is_empty(), "expected cylindrical hole walls");
    for &eu in &hole_eus {
        let u = plate.edgeuse_mut(eu).unwrap();
        assert!(u.max_edge_face_gap.is_init());
        u.max_edge_face_gap.gap_3d = sentinel;
    }

    let copied = brep_copy(&mut k, &plate).unwrap();
    let tool = create_cylinder(&mut k, Point3::new(0.0, 0.0, -2.0), 1.5, 14.0).unwrap();
    let mut cut = boolean_difference(&mut k, copied, tool).unwrap();
    update_tolerances_after_boolean(&mut k, &mut cut).unwrap();

    let preserved_sentinels = cut
        .edgeuses()
        .iter()
        .filter(|(_, u)| {
            u.max_edge_face_gap.is_init()
                && u.max_edge_face_gap.gap_3d.to_bits() == sentinel.to_bits()
        })
        .count();
    assert_eq!(
        preserved_sentinels,
        hole_eus.len(),
        "untouched hole wall edge uses must not be re-measured after copy + corner cut"
    );
}

/// Scaling or editing an edge's interval after `brep_copy` clears the preserved gap
/// caches and re-measures real gaps accurately.
#[test]
fn scaled_and_split_edges_invalidate_and_remeasure_gaps_after_copy() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    warm_all_gap_caches(&mut k, &mut b);

    // 1. Non-unit scale on a copy must clear edge and vertex gap caches and recompute.
    let mut scaled = brep_copy(&mut k, &b).unwrap();
    for (_, u) in scaled.edgeuses().iter() {
        assert!(u.max_edge_face_gap.is_init());
        assert!(u.edge_ccw_edge_gap.is_init());
    }
    transform(
        &mut k,
        &mut scaled,
        &Axis2Placement::IDENTITY,
        Some(Vec3::new(2.0, 1.0, 1.0)),
        true,
    )
    .unwrap();
    for (_, u) in scaled.edgeuses().iter() {
        assert!(!u.max_edge_face_gap.is_init());
        assert!(!u.edge_ccw_edge_gap.is_init());
    }
    for (_, v) in scaled.vertexuses().iter() {
        assert!(!v.vertex_edge_gap.is_init());
        assert!(!v.vertex_face_gap.is_init());
    }
    update_tolerances_after_boolean(&mut k, &mut scaled).unwrap();
    let check =
        check_brep_full(&mut k, &mut scaled, CheckLevel::Level2, ValidityScope::Body).unwrap();
    assert!(check.is_valid(), "{:?}", check.lines());

    // 2. Changing an edge's interval on a copy must invalidate its caches and re-measure
    // the new 5 mm vertex/edge gap rather than keeping the pre-copy 0 mm cache.
    let mut shortened = brep_copy(&mut k, &b).unwrap();
    let e = shortened.edge_list()[0];
    let ivl = edge::interval(&shortened, e).unwrap();
    let half = Extent1d::from_min_max(ivl.min(), 0.5 * (ivl.min() + ivl.max()));
    set_interval(&mut shortened, e, &half).unwrap();
    let eus = shortened.edge(e).unwrap().edgeuses.clone();
    for eu in &eus {
        let r = shortened.edgeuse(*eu).unwrap();
        assert!(!r.max_edge_face_gap.is_init());
        assert!(!r.edge_ccw_edge_gap.is_init());
        if let Some(vu) = r.vertexuse {
            assert!(!shortened.vertexuse(vu).unwrap().vertex_edge_gap.is_init());
        }
    }
    let mut max_ve_gap = 0.0_f64;
    for eu in eus {
        if let Some(vu) = shortened.edgeuse(eu).unwrap().vertexuse {
            if let Some(g) = vu1::vertex_edge_gap(&k, &mut shortened, vu, false).unwrap() {
                max_ve_gap = max_ve_gap.max(g.gap_3d);
            }
        }
    }
    assert!(
        (max_ve_gap - 5.0).abs() < 1e-6,
        "expected ~5.0 mm vertex/edge gap after shortening, got {max_ve_gap}"
    );
}

/// Gluing vertices or edges after copying/merging bodies with warm gap caches resets
/// the moved uses' caches so they measure against the surviving vertex/edge rather than
/// serving stale pre-glue zeros.
#[test]
fn glued_vertices_and_edges_invalidate_copied_gap_caches() {
    let mut k = Kernel::default();
    let offset = 1.0e-4;
    let mut left = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    let mut right = create_box(
        &mut k,
        Point3::new(10.0 + offset, 0.0, 0.0),
        10.0,
        10.0,
        10.0,
    )
    .unwrap();
    warm_all_gap_caches(&mut k, &mut left);
    warm_all_gap_caches(&mut k, &mut right);

    let mut state = ArenaNotifyState::new(&left);
    let mut env = state.env(None);
    merge_brep(
        &mut k,
        &mut env,
        &mut left,
        &right,
        &copy_source_ref,
        None,
        None,
        false,
    )
    .unwrap();
    left.set_editing_enabled(true);

    // Locate the two vertex pairs along the adjacent vertical edge at x=10 / x=10+offset, y=0.
    let find_vertex = |b: &Brep, target: Point3| -> VertexId {
        b.vertex_list()
            .iter()
            .copied()
            .find(|&v| b.vertex(v).unwrap().point.distance(target) < 1e-9)
            .unwrap()
    };
    let v_left_bot = find_vertex(&left, Point3::new(10.0, 0.0, 0.0));
    let v_right_bot = find_vertex(&left, Point3::new(10.0 + offset, 0.0, 0.0));
    let v_left_top = find_vertex(&left, Point3::new(10.0, 0.0, 10.0));
    let v_right_top = find_vertex(&left, Point3::new(10.0 + offset, 0.0, 10.0));

    let moved_bot_vus = left.vertex(v_right_bot).unwrap().vertexuses.clone();
    assert!(
        moved_bot_vus
            .iter()
            .all(|&vu| left.vertexuse(vu).unwrap().vertex_edge_gap.is_init()),
        "merge_brep should have preserved the source vertex/edge gap caches"
    );

    glue_vertices(&mut k, &mut left, Some(v_left_bot), Some(v_right_bot), None).unwrap();
    for &vu in &moved_bot_vus {
        let u = left.vertexuse(vu).unwrap();
        assert!(
            !u.vertex_edge_gap.is_init(),
            "glue_vertices must reset vertex_edge_gap"
        );
        assert!(
            !u.vertex_face_gap.is_init(),
            "glue_vertices must reset vertex_face_gap"
        );
    }
    let measured_ve = moved_bot_vus
        .iter()
        .filter_map(|&vu| vu1::vertex_edge_gap(&k, &mut left, vu, false).unwrap())
        .map(|g| g.gap_3d)
        .fold(0.0_f64, f64::max);
    assert!(
        (measured_ve - offset).abs() < 1e-9,
        "expected ~{offset} vertex/edge gap after gluing vertices, got {measured_ve}"
    );

    glue_vertices(&mut k, &mut left, Some(v_left_top), Some(v_right_top), None).unwrap();

    // Find the two vertical edges now sharing (v_left_bot, v_left_top).
    let shared_edges: Vec<EdgeId> = left
        .edge_list()
        .iter()
        .copied()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(&left, e).unwrap();
            (s == Some(v_left_bot) && t == Some(v_left_top))
                || (s == Some(v_left_top) && t == Some(v_left_bot))
        })
        .collect();
    assert_eq!(shared_edges.len(), 2);
    let surv_edge = shared_edges[0];
    let del_edge = shared_edges[1];
    let moved_eus = left.edge(del_edge).unwrap().edgeuses.clone();
    assert!(
        moved_eus
            .iter()
            .all(|&eu| left.edgeuse(eu).unwrap().max_edge_face_gap.is_init()),
        "merge_brep should have preserved the source max_edge_face_gap caches"
    );

    let rel_orient = if edge::start_vertex(&left, surv_edge).unwrap()
        == edge::start_vertex(&left, del_edge).unwrap()
    {
        OrientType::Same
    } else {
        OrientType::Opposite
    };
    let mut glue_state = ArenaNotifyState::new(&left);
    let mut glue_env = glue_state.env(None);
    glue_edges_general(
        &mut k,
        &mut left,
        Some(surv_edge),
        rel_orient,
        offset,
        false,
        Some(del_edge),
        None,
        &mut glue_env,
    )
    .unwrap();

    for &eu in &moved_eus {
        let u = left.edgeuse(eu).unwrap();
        assert!(
            !u.max_edge_face_gap.is_init(),
            "glue_edges_general must reset max_edge_face_gap"
        );
        assert!(
            !u.edge_ccw_edge_gap.is_init(),
            "glue_edges_general must reset edge_ccw_edge_gap"
        );
    }
    let measured_ef = moved_eus
        .iter()
        .filter_map(|&eu| eu1::max_edge_face_gap(&k, &mut left, eu, false).unwrap())
        .map(|g| g.gap_3d)
        .fold(0.0_f64, f64::max);
    assert!(
        (measured_ef - offset).abs() < 1e-9,
        "expected ~{offset} edge/face gap after gluing edges, got {measured_ef}"
    );
}

/// Copying with `add_analytics = true` leaves surface-dependent gap caches reset on any
/// face whose surface kind changed from B-spline to an analytic, while preserving
/// curve-only (`vertex_edge_gap`) caches.
#[test]
fn add_analytics_resets_surface_gap_caches_when_surface_kind_changes() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0).unwrap();
    turn_to_nurbs_plain(&mut k, &mut b).unwrap();
    for &f in b.face_list() {
        assert!(matches!(
            b.face(f).unwrap().surface.as_ref().unwrap().kind(),
            SurfaceKind::BSpline(_)
        ));
    }
    warm_all_gap_caches(&mut k, &mut b);

    let mut state = ArenaNotifyState::new(&Brep::new());
    let mut env = state.env(None);
    let copy = copy_brep(
        &mut k,
        &mut env,
        &b,
        &copy_source_ref,
        None,
        None,
        true,
        true,
    )
    .unwrap();

    let promoted_faces: Vec<FaceId> = copy
        .face_list()
        .iter()
        .copied()
        .filter(|&f| {
            !matches!(
                copy.face(f).unwrap().surface.as_ref().unwrap().kind(),
                SurfaceKind::BSpline(_)
            )
        })
        .collect();
    assert!(
        !promoted_faces.is_empty(),
        "expected B-spline planes promoted to analytic planes"
    );

    for f in promoted_faces {
        let fu = face2::upward_faceuse(&copy, f).unwrap().unwrap();
        for eu in eu1::faceuse_edgeuses(&copy, fu, false, None).unwrap() {
            assert!(
                !copy.edgeuse(eu).unwrap().max_edge_face_gap.is_init(),
                "promoted face must not keep pre-promotion max_edge_face_gap"
            );
            if let Some(vu) = copy.edgeuse(eu).unwrap().vertexuse {
                assert!(
                    !copy.vertexuse(vu).unwrap().vertex_face_gap.is_init(),
                    "promoted face must not keep pre-promotion vertex_face_gap"
                );
                assert!(
                    copy.vertexuse(vu).unwrap().vertex_edge_gap.is_init(),
                    "curve-only vertex_edge_gap should still be preserved"
                );
            }
        }
    }
}

use super::*;
use crate::Mesh;

/// `n` segments of a circle in the XY plane from `from` to `to` (radians), as a polyline.
fn arc_pts(center: Vec3, radius: f64, from: f64, to: f64, n: usize) -> Vec<Vec3> {
    (0..=n)
        .map(|i| {
            let a = from + (to - from) * i as f64 / n as f64;
            center + Vec3::new(radius * a.cos(), radius * a.sin(), 0.0)
        })
        .collect()
}

fn circle_pts(center: Vec3, radius: f64, n: usize) -> Vec<Vec3> {
    arc_pts(center, radius, 0.0, std::f64::consts::TAU, n)
}

fn edges(edges: Vec<Vec<Vec3>>) -> Mesh {
    let n = edges.len();
    Mesh { edges, edge_faces: vec![vec![0]; n], ..Default::default() }
}

/// One face from triangles given as point triples.
fn face(tris: Vec<[Vec3; 3]>) -> Mesh {
    let mut m = Mesh::default();
    for [a, b, c] in tris {
        let i = m.positions.len() as u32;
        m.positions.extend([a, b, c]);
        m.triangles.push([i, i + 1, i + 2]);
        m.tri_face.push(0);
    }
    m
}

fn kinds(cs: &[SnapCandidate]) -> Vec<SnapKind> {
    cs.iter().map(|c| c.kind).collect()
}

fn of_kind(cs: &[SnapCandidate], k: SnapKind) -> Vec<Vec3> {
    cs.iter().filter(|c| c.kind == k).map(|c| c.at).collect()
}

#[test]
fn a_straight_edge_offers_its_ends_its_middle_and_a_point_along_it() {
    let m = edges(vec![vec![Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0)]]);
    let cs = edge_candidates(&m, 0, Vec3::new(3.0, 1.0, 0.0), ALL);
    assert_eq!(kinds(&cs), [SnapKind::Vertex, SnapKind::Vertex, SnapKind::Midpoint, SnapKind::OnEdge], "{cs:?}");
    assert_eq!(of_kind(&cs, SnapKind::Midpoint), [Vec3::new(5.0, 0.0, 0.0)]);
    // The point along the edge follows the cursor.
    assert_eq!(of_kind(&cs, SnapKind::OnEdge), [Vec3::new(3.0, 0.0, 0.0)]);
    assert!(circle_of(&[Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0)]).is_none(), "a straight edge lies on no circle");
}

#[test]
fn a_circular_edge_offers_its_centre_and_quadrants_but_no_middle() {
    let c = Vec3::new(4.0, 5.0, 6.0);
    let m = edges(vec![circle_pts(c, 3.0, 32)]);
    let cs = edge_candidates(&m, 0, c + Vec3::new(3.0, 0.0, 0.0), ALL);
    let centre = of_kind(&cs, SnapKind::Center);
    assert_eq!(centre.len(), 1, "{cs:?}");
    assert!(centre[0].dist(c) < 1e-9, "{centre:?}");
    // Four quarter turns, one of which is the seam the polyline starts at, so it stays a vertex.
    assert_eq!(of_kind(&cs, SnapKind::Quadrant).len() + of_kind(&cs, SnapKind::Vertex).len(), 4, "{cs:?}");
    for q in of_kind(&cs, SnapKind::Quadrant) {
        assert!((q.dist(c) - 3.0).abs() < 1e-6, "a quadrant lies on the circle: {q:?}");
    }
    assert!(of_kind(&cs, SnapKind::Midpoint).is_empty(), "a closed circle has no middle: {cs:?}");
}

#[test]
fn an_arc_offers_only_the_quarter_turns_it_passes() {
    use std::f64::consts::FRAC_PI_4;
    // From -45° to 135°: it passes the quarter turns at 0° and 90° and misses 180° and 270°.
    let m = edges(vec![arc_pts(Vec3::ZERO, 2.0, -FRAC_PI_4, FRAC_PI_4 * 3.0, 24)]);
    let qs = of_kind(&edge_candidates(&m, 0, Vec3::new(2.0, 0.0, 0.0), QUADRANT), SnapKind::Quadrant);
    assert_eq!(qs.len(), 2, "{qs:?}");
    assert!(qs.iter().any(|q| q.dist(Vec3::new(2.0, 0.0, 0.0)) < 1e-6), "{qs:?}");
    assert!(qs.iter().any(|q| q.dist(Vec3::new(0.0, 2.0, 0.0)) < 1e-6), "{qs:?}");
    // An arc does have a middle.
    let mid = of_kind(&edge_candidates(&m, 0, Vec3::ZERO, MIDPOINT), SnapKind::Midpoint);
    assert_eq!(mid.len(), 1, "{mid:?}");
    let diagonal = 2.0 * std::f64::consts::FRAC_1_SQRT_2;
    assert!(mid[0].dist(Vec3::new(diagonal, diagonal, 0.0)) < 2e-2, "halfway from -45° to 135° is 45°: {mid:?}");
}

#[test]
fn a_quarter_turn_stays_put_when_the_edge_starts_elsewhere_or_runs_the_other_way() {
    let full = circle_pts(Vec3::ZERO, 5.0, 36);
    let shifted: Vec<Vec3> = full[9..].iter().chain(full[1..10].iter()).copied().collect();
    let reversed: Vec<Vec3> = full.iter().rev().copied().collect();
    let sets: Vec<Vec<Vec3>> = [full.clone(), shifted, reversed]
        .into_iter()
        .map(|pts| edge_candidates(&edges(vec![pts]), 0, Vec3::new(5.0, 0.0, 0.0), QUADRANT).iter().map(|c| c.at).collect())
        .collect();
    assert_eq!(sets[0].len(), 4, "{sets:?}");
    // The same four places, whatever order they come in.
    for later in &sets[1..] {
        assert_eq!(later.len(), sets[0].len(), "{sets:?}");
        for a in &sets[0] {
            assert!(later.iter().any(|b| a.dist(*b) < 1e-6), "{a:?} missing from {later:?}");
        }
    }
}

#[test]
fn a_seam_edge_offers_nothing() {
    let mut m = edges(vec![vec![Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0)]]);
    m.seams = vec![true];
    assert!(edge_candidates(&m, 0, Vec3::ZERO, ALL).is_empty(), "a hidden seam is not picked, so it offers no snap");
    m.sharp_edges = vec![true];
    assert!(!edge_candidates(&m, 0, Vec3::ZERO, ALL).is_empty(), "an authored split is picked again");
}

#[test]
fn a_planar_face_offers_its_centre_and_an_l_shaped_one_does_not() {
    let p = |x: f64, y: f64| Vec3::new(x, y, 0.0);
    let square = face(vec![[p(0.0, 0.0), p(2.0, 0.0), p(2.0, 2.0)], [p(0.0, 0.0), p(2.0, 2.0), p(0.0, 2.0)]]);
    let cs = face_candidates(&square, 0, p(0.5, 0.5), FACE_CENTER);
    assert_eq!(of_kind(&cs, SnapKind::FaceCenter), [p(1.0, 1.0)], "{cs:?}");
    // An L: the area-weighted centre at (1.1, 1.1) falls in the notch, off the face.
    let l = face(vec![
        [p(0.0, 0.0), p(3.0, 0.0), p(3.0, 1.0)],
        [p(0.0, 0.0), p(3.0, 1.0), p(0.0, 1.0)],
        [p(0.0, 1.0), p(1.0, 1.0), p(1.0, 3.0)],
        [p(0.0, 1.0), p(1.0, 3.0), p(0.0, 3.0)],
    ]);
    assert_eq!(planar_face_center(&l, 0).map(|(c, _)| c), None, "the centre of an L is not on it");
    assert!(face_candidates(&l, 0, p(0.5, 0.5), FACE_CENTER).is_empty());
}

#[test]
fn a_curved_face_has_no_centre_to_offer() {
    let bent = face(vec![
        [Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0)],
        [Vec3::ZERO, Vec3::new(1.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 1.0)],
    ]);
    assert_eq!(planar_face_center(&bent, 0), None, "the triangles of a curved face face different ways");
}

#[test]
fn an_exact_place_outranks_a_point_that_only_slides_however_near_it_is() {
    let at = Vec3::new(4.0, 0.0, 0.0);
    // A place that slides sits on the cursor by construction, so only the tiers can order these.
    // No dedupe here (`tol` of zero): the two sliding places are the same point.
    let mut cs =
        vec![SnapCandidate::new(SnapKind::OnFace, at), SnapCandidate::new(SnapKind::OnEdge, at), SnapCandidate::new(SnapKind::Vertex, Vec3::ZERO)];
    rank(&mut cs, at, 0.0);
    assert_eq!(kinds(&cs), [SnapKind::Vertex, SnapKind::OnEdge, SnapKind::OnFace], "{cs:?}");
    // Among the exact places the nearest wins, with the kind to break a tie.
    let mut cs =
        vec![SnapCandidate::new(SnapKind::Midpoint, Vec3::new(4.2, 0.0, 0.0)), SnapCandidate::new(SnapKind::Vertex, Vec3::new(3.0, 0.0, 0.0))];
    rank(&mut cs, at, 1e-6);
    assert_eq!(kinds(&cs), [SnapKind::Midpoint, SnapKind::Vertex], "a midpoint 0.2 away beats a vertex 1.0 away: {cs:?}");
    let mut cs =
        vec![SnapCandidate::new(SnapKind::Midpoint, Vec3::new(4.5, 0.0, 0.0)), SnapCandidate::new(SnapKind::Vertex, Vec3::new(3.4, 0.0, 0.0))];
    rank(&mut cs, at, 1e-6);
    assert_eq!(kinds(&cs), [SnapKind::Vertex, SnapKind::Midpoint], "0.4 * 0.6 beats 0.6 * 0.5: {cs:?}");
}

#[test]
fn only_the_places_that_slide_are_a_fallback() {
    for k in EVERY {
        assert_eq!(k.tier(), u8::from(EXACT & k.bit() == 0), "{}", k.id());
    }
    assert_eq!(EVERY.iter().filter(|k| k.tier() == 1).map(|k| k.id()).collect::<Vec<_>>(), ["on_edge", "on_face"]);
}

#[test]
fn ranking_drops_a_weaker_place_that_sits_on_a_stronger_one() {
    let p = Vec3::new(1.0, 2.0, 3.0);
    let mut cs = vec![SnapCandidate::new(SnapKind::OnFace, p), SnapCandidate::new(SnapKind::Vertex, p)];
    rank(&mut cs, p, 1e-6);
    assert_eq!(kinds(&cs), [SnapKind::Vertex], "{cs:?}");
}

#[test]
fn a_hole_centre_comes_back_for_a_place_near_the_centre_not_one_on_the_rim() {
    let c = Vec3::new(10.0, 0.0, 0.0);
    let m = edges(vec![circle_pts(c, 4.0, 32)]);
    // Looking near the centre: the centre is in reach even though every point of the rim is 4 away.
    let near = candidates_near(&m, c + Vec3::new(0.2, 0.0, 0.0), 1.0, ALL);
    assert_eq!(kinds(&near).first(), Some(&SnapKind::Center), "{near:?}");
    // Looking at the rim, 30° round from the seam and from any quarter turn: the centre is a
    // radius away, so the proximity search leaves it out and only the point along the rim is left.
    let on_rim = c + Vec3::new(4.0 * 0.75_f64.sqrt(), 2.0, 0.0);
    let rim = candidates_near(&m, on_rim, 1.0, ALL);
    assert!(!kinds(&rim).contains(&SnapKind::Center), "{rim:?}");
    assert_eq!(kinds(&rim), [SnapKind::OnEdge], "{rim:?}");
    // The hover question is the other way round: the edge offers its centre whatever the distance.
    assert!(kinds(&edge_candidates(&m, 0, on_rim, ALL)).contains(&SnapKind::Center));
}

#[test]
fn only_the_kinds_asked_for_come_back() {
    let m = edges(vec![circle_pts(Vec3::ZERO, 3.0, 32), vec![Vec3::ZERO, Vec3::new(9.0, 0.0, 0.0)]]);
    for k in EVERY {
        for (e, _) in m.edges.iter().enumerate() {
            for c in edge_candidates(&m, e, Vec3::new(1.0, 1.0, 0.0), k.bit()) {
                assert_eq!(c.kind, k, "asked for {} only", k.id());
            }
        }
    }
    assert!(edge_candidates(&m, 0, Vec3::ZERO, 0).is_empty(), "no kinds, no candidates");
}

#[test]
fn hostile_places_and_radii_never_panic_and_find_nothing() {
    let m = edges(vec![circle_pts(Vec3::ZERO, 3.0, 32)]);
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0] {
        assert!(candidates_near(&m, Vec3::ZERO, bad, ALL).is_empty(), "radius {bad}");
    }
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(candidates_near(&m, Vec3::new(bad, 0.0, 0.0), 10.0, ALL).is_empty(), "place {bad}");
        // A hover on a broken place still answers, and never with a broken point.
        for c in edge_candidates(&m, 0, Vec3::new(bad, bad, bad), ALL) {
            assert!(c.at.is_finite(), "{c:?}");
        }
    }
    // Degenerate edges.
    let short = edges(vec![vec![], vec![Vec3::ZERO], vec![Vec3::ZERO, Vec3::ZERO]]);
    for e in 0..4 {
        for c in edge_candidates(&short, e, Vec3::ZERO, ALL) {
            assert!(c.at.is_finite(), "{c:?}");
        }
    }
    assert!(candidates_near(&Mesh::default(), Vec3::ZERO, 1.0, ALL).is_empty());
}

#[test]
fn a_broad_search_is_capped() {
    // Many concentric circles, all within reach of the centre.
    let m = edges((1..200).map(|i| circle_pts(Vec3::ZERO, i as f64 * 0.1, 8)).collect());
    assert!(candidates_near(&m, Vec3::ZERO, 1e6, ALL).len() <= MAX_CANDIDATES);
}

#[test]
fn every_kind_has_its_own_bit_id_and_label() {
    for (i, k) in EVERY.iter().enumerate() {
        assert_eq!(SnapKind::from_id(k.id()), Some(*k), "{}", k.id());
        assert!(ALL & k.bit() != 0, "{}", k.id());
        for other in &EVERY[i + 1..] {
            assert_ne!(k.bit(), other.bit(), "{} and {}", k.id(), other.id());
            assert_ne!(k.id(), other.id());
        }
    }
    assert_eq!(EXACT.count_ones(), 6, "the places that lock onto exact geometry");
    assert_eq!(ALL.count_ones(), EVERY.len() as u32);
    assert_eq!(kinds_of(["vertex", "center"]), Ok(VERTEX | CENTER));
    assert_eq!(kinds_of(["vertex", "nonesuch"]), Err("nonesuch"));
    assert_eq!(kinds_of([]), Ok(0));
    // An exact place always pulls harder than one that slides.
    for e in EVERY.iter().filter(|k| EXACT & k.bit() != 0) {
        for slide in EVERY.iter().filter(|k| EXACT & k.bit() == 0) {
            assert!(e.weight() < slide.weight(), "{} vs {}", e.id(), slide.id());
        }
    }
}

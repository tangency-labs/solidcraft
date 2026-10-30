//! Fillets with a radius per edge. SMLib rounds every picked edge at its own radius in one
//! blend, with the corners where the rounds meet: an end in the face beyond, two rounds trimming
//! each other, or a corner patch where three meet.
//!
//! A later round at a corner where earlier rounds already meet in a mitre (the owner's ruling of
//! 2026-10-30) makes the corner all of them would have made rounded at once: one smooth patch,
//! the mitre gone. A body made by constant-radius rounds keeps what they were made on and which
//! edges they rounded at which radius ([`Rounds`]); a later round reaching such a corner
//! ([`later_corners`]) blends the earlier rounds' edges, from the earliest of the rounds that
//! meet there on, with the new edges in one SMLib blend of the body before them, and the result's
//! history is put over the body the later round was given ([`rebase`]): an earlier round's face
//! is that face, trimmed anew; the corner patch, and every face the new edges make, is the later
//! round's. The mitre's seam is no edge to round on its own: picked (a tangent chain reaches it
//! from the new edge), it only says the corner is blended again.

use std::collections::BTreeSet;

use solidcraft_geom::Vec3;

use crate::body::Body;
use crate::history::{Change, EdgeOrigin, EdgeProv, FaceOrigin, FaceProv, History};
use crate::{KernelError, Result};

/// The constant-radius rounds that made a body ([`crate::fillet`], [`fillet_radii`]): the body
/// they were made on, and each edge they rounded (its index in that body's edge order) with its
/// radius. The body's history is over `before`.
#[derive(Clone, Debug)]
pub(crate) struct Rounds {
    pub(crate) before: Body,
    pub(crate) edges: Vec<(usize, f64)>,
}

/// A later round at corners where earlier rounds meet ([`later_corners`]): what is blended in
/// one go, and what puts its history over the body the later round was given.
pub(crate) struct Later {
    /// The body before the earliest of the rounds that meet at the new edges' corners.
    pub(crate) base: Body,
    /// The edges blended on `base` (its edge order) with their radii: the edges of every round
    /// from that earliest on, and the new edges.
    pub(crate) edges: Vec<(usize, f64)>,
    /// The body over `base`: the histories of those rounds composed.
    chain: History,
    /// The new edges: each one's index in the body, and in `base`.
    new: Vec<(usize, usize)>,
    /// The body's mitre seams at the new edges' corners, which the corner patches replace.
    seams: Vec<usize>,
}

/// The history of `body` over the body `levels` rounds back ([`Rounds::before`] taken that many
/// times), composed; `None` when a step has no record or history, or for no levels.
fn down(body: &Body, levels: usize) -> Option<History> {
    let mut at = body.clone();
    let mut composed: Option<History> = None;
    for _ in 0..levels {
        let rounds = at.rounds.clone()?;
        let step = at.history().filter(|h| h.operands == 1)?.clone();
        composed = Some(match composed {
            None => step,
            Some(h) => h.substituted(0, &step).ok()?,
        });
        at = rounds.before.clone();
    }
    composed
}

/// The operand edge `edge` of a result comes from, through `h`, when it is (a piece of) one.
fn edge_origin(h: &History, edge: usize) -> Option<usize> {
    match h.edges.get(edge).map(|e| e.origins.as_slice()) {
        Some([EdgeOrigin::Edge { operand: 0, edge }]) => Some(*edge),
        _ => None,
    }
}

/// Faces a blend made in place of edges (rounds, their corner patches), through `h`.
fn rounds_of(h: &History) -> Vec<bool> {
    h.faces
        .iter()
        .map(|f| f.change == Change::Generated && !f.origins.is_empty() && f.origins.iter().all(|o| matches!(o, FaceOrigin::Edge { .. })))
        .collect()
}

/// Is `edge` (its faces through `h`) where the rounds of different edges meet: between two of
/// `rounds` made in place of edges none of which both are (a mitre, or rounds running on into
/// each other), not between a round and its own end cap or a corner patch it is part of?
fn between_rounds(h: &History, rounds: &[bool], edge: usize) -> bool {
    let round = |f: usize| rounds.get(f).copied().unwrap_or(false).then(|| h.faces.get(f)).flatten();
    match h.edges.get(edge).map(|e| e.faces.as_slice()) {
        Some([f, g]) if f != g => match (round(*f), round(*g)) {
            (Some(a), Some(b)) => a.origins.iter().all(|o| !b.origins.contains(o)),
            _ => false,
        },
        _ => false,
    }
}

/// The later rounds of `picked` (the body's edges, `sizes` their radii, as the blend resolved
/// them) when one of them reaches a corner where rounds that made the body meet: the blend to
/// make in their place, and the new edges (in the body's edge order, with their radii) the
/// body's record of rounds takes. `Ok(None)` when none does, or when the rounds are not on record
/// down to that corner. A picked seam between earlier rounds is refused unless a new edge of
/// the same radius meets it: it has no round of its own.
pub(crate) fn later_corners(body: &Body, picked: &[usize], sizes: &[f64]) -> Result<Option<(Later, Vec<(usize, f64)>)>> {
    // The rounds on record, latest first: each one's body before it and the body over that.
    let mut levels: Vec<(Rounds, History)> = Vec::new();
    while let Some(h) = down(body, levels.len() + 1) {
        let Some(r) = (match levels.last() {
            None => body.rounds.as_deref().cloned(),
            Some((r, _)) => r.before.rounds.as_deref().cloned(),
        }) else {
            break;
        };
        levels.push((r, h));
    }
    let Some((_, deepest)) = levels.last() else { return Ok(None) };
    let rounds = rounds_of(deepest);
    let record = body.record();
    let ends = |e: usize| record.edges.get(e).map(|e| e.vertices).unwrap_or([u32::MAX; 2]);
    let (seam_picks, new): (Vec<(usize, f64)>, Vec<(usize, f64)>) =
        picked.iter().copied().zip(sizes.iter().copied()).partition(|(e, _)| between_rounds(deepest, &rounds, *e));
    let corners: BTreeSet<u32> = new.iter().flat_map(|(e, _)| ends(*e)).collect();
    let seams: Vec<usize> =
        (0..record.edges.len()).filter(|e| between_rounds(deepest, &rounds, *e) && ends(*e).iter().any(|v| corners.contains(v))).collect();
    for (seam, size) in &seam_picks {
        let meets = new.iter().any(|(e, r)| r.to_bits() == size.to_bits() && ends(*e).iter().any(|v| ends(*seam).contains(v)));
        if !meets {
            return Err(KernelError::Invalid(
                "fillet: a picked edge is where earlier rounds meet (a mitre), which has no round of its own: pick the edge that runs \
                 into that corner, at its radius, and the corner is rounded again with it"
                    .into(),
            ));
        }
    }
    if seams.is_empty() {
        return Ok(None);
    }
    // The latest round from which on every seam is between rounds on record.
    let Some(level) = levels.iter().position(|(_, h)| {
        let rounds = rounds_of(h);
        seams.iter().all(|s| between_rounds(h, &rounds, *s))
    }) else {
        return Ok(None);
    };
    let Some((base, chain)) = levels.get(level).map(|(r, h)| (r.before.clone(), h.clone())) else { return Ok(None) };
    // Every round's edges from that level on, in `base`'s edge order.
    let mut edges: Vec<(usize, f64)> = Vec::new();
    for (step, (r, _)) in levels.iter().enumerate().take(level + 1) {
        let inner = down(&r.before, level - step);
        for (e, radius) in &r.edges {
            let at = match &inner {
                None => Some(*e),
                Some(h) => edge_origin(h, *e),
            };
            let Some(at) = at else { return Ok(None) };
            edges.push((at, *radius));
        }
    }
    let mut mapped = Vec::with_capacity(new.len());
    for (e, radius) in &new {
        let Some(at) = edge_origin(&chain, *e) else { return Ok(None) };
        mapped.push((*e, at));
        edges.push((at, *radius));
    }
    Ok(Some((Later { base, edges, chain, new: mapped, seams }, new)))
}

/// The history of `result`, the blend `later` made of its base (`h`, over the base), over the
/// body the later round was given (`body`, `later.chain` over the base): a face of the base's
/// faces is the body's face of them, kept where both kept it, else trimmed anew (split or joined
/// where they are several); an earlier round's face is that round's face in the body; a face
/// made in place of a new edge (its round, and a corner patch where it meets earlier rounds) is
/// new, made in place of the new edges and of the body's seams between the rounds it joins.
/// Each edge is the body's edge between the faces it lies between there, else where its faces
/// meet. `None` where a face has no counterpart.
pub(crate) fn rebase(h: &History, later: &Later, body: &Body) -> Option<History> {
    let chain = &later.chain;
    let set = |p: &FaceProv| p.origins.iter().copied().collect::<BTreeSet<FaceOrigin>>();
    let mut faces = Vec::with_capacity(h.faces.len());
    for p in &h.faces {
        let origins = set(p);
        let new_edges: Vec<FaceOrigin> = later
            .new
            .iter()
            .filter(|(_, at)| origins.contains(&FaceOrigin::Edge { operand: 0, edge: *at }))
            .map(|(e, _)| FaceOrigin::Edge { operand: 0, edge: *e })
            .collect();
        if !new_edges.is_empty() {
            // The seams between rounds all of whose origins this face takes up.
            let replaced = later.seams.iter().filter(|s| {
                chain.edges.get(**s).is_some_and(|e| e.faces.iter().all(|f| chain.faces.get(*f).is_some_and(|f| set(f).is_subset(&origins))))
            });
            let mut all = new_edges;
            all.extend(replaced.map(|s| FaceOrigin::Edge { operand: 0, edge: *s }));
            faces.push(crate::record::generated(all));
            continue;
        }
        let candidates: Vec<usize> = if p.change == Change::Generated {
            (0..chain.faces.len()).filter(|f| chain.faces.get(*f).is_some_and(|c| c.change == Change::Generated && set(c) == origins)).collect()
        } else {
            (0..chain.faces.len())
                .filter(|f| chain.faces.get(*f).is_some_and(|c| c.change != Change::Generated && c.origins.iter().any(|o| origins.contains(o))))
                .collect()
        };
        let change = match candidates[..] {
            [] => return None,
            [f] if p.change == Change::Kept && chain.faces.get(f).is_some_and(|c| c.change == Change::Kept) => Change::Kept,
            [_] => Change::Modified,
            _ => Change::Merged,
        };
        faces.push(FaceProv { origins: candidates.into_iter().map(|face| FaceOrigin::Face { operand: 0, face }).collect(), change });
    }
    crate::record::mark_shared_split(&mut faces);
    // The body's faces under each result face, and the body's edges by the faces they separate.
    let under = |f: usize| -> BTreeSet<usize> {
        faces
            .get(f)
            .filter(|p| p.change != Change::Generated)
            .map(|p| p.origins.iter().filter_map(|o| if let FaceOrigin::Face { face, .. } = o { Some(*face) } else { None }).collect())
            .unwrap_or_default()
    };
    let body_edges = crate::history::edge_faces(body.record());
    let edges = h
        .edges
        .iter()
        .map(|e| {
            let sides: Vec<BTreeSet<usize>> = e.faces.iter().map(|f| under(*f)).collect();
            let same: Vec<EdgeOrigin> = if sides.iter().all(|s| !s.is_empty()) {
                body_edges
                    .iter()
                    .enumerate()
                    .filter(|(_, between)| {
                        between.len() == sides.len()
                            && (between.iter().zip(&sides).all(|(f, s)| s.contains(f))
                                || between.iter().rev().zip(&sides).all(|(f, s)| s.contains(f)))
                    })
                    .map(|(edge, _)| EdgeOrigin::Edge { operand: 0, edge })
                    .collect()
            } else {
                Vec::new()
            };
            let origins = if same.is_empty() { crate::record::meeting(&e.faces, &faces) } else { same };
            EdgeProv { faces: e.faces.clone(), origins }
        })
        .collect();
    Some(History { operands: 1, faces, edges })
}

/// Fillet each edge nearest a point with its own radius.
pub fn fillet_radii(body: &Body, edges: &[(Vec3, f64)]) -> Result<Body> {
    body.require_brep("fillet")?;
    if edges.is_empty() || edges.len() > 1000 {
        return Err(KernelError::Invalid("select 1…1000 edges".into()));
    }
    if edges.iter().any(|(p, r)| !(p.is_finite() && r.is_finite() && *r > 1e-6 && *r < 1e6)) {
        return Err(KernelError::Invalid("fillet size must be positive".into()));
    }
    let (points, radii): (Vec<Vec3>, Vec<f64>) = edges.iter().copied().unzip();
    crate::blend_smlib::fillet_each(body, &points, &radii)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::blend::Faces;
    use crate::{box_solid, fillet, measure};
    use solidcraft_geom::brep::eval::CurveEval;

    /// The 20 mm box and the three edges at its corner (20, 0, 20): `a` along y, `b` along x,
    /// `c` up.
    fn setup() -> (Body, Vec3, Vec3, Vec3) {
        let b = box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 20.0)).unwrap();
        (b, Vec3::new(20.0, 10.0, 20.0), Vec3::new(10.0, 0.0, 20.0), Vec3::new(20.0, 0.0, 10.0))
    }

    fn vol(b: &Body) -> f64 {
        measure(b).unwrap().volume
    }

    /// The body minus the material outside each round (the strips between the faces and the
    /// cylinders), for rounds `ra`, `rb`, `rc` on edges a, b, c: the strips integrated exactly
    /// and their overlap at the corner numerically.
    fn strips(ra: f64, rb: f64, rc: f64) -> f64 {
        let len = |d: f64, r: f64| if d < r { r - (r * r - (r - d) * (r - d)).max(0.0).sqrt() } else { 0.0 };
        let (n, l) = (800, 5.0);
        let hh = l / n as f64;
        let (mut union, mut sum) = (0.0, 0.0);
        for i in 0..n {
            let dx = (i as f64 + 0.5) * hh;
            for j in 0..n {
                let dy = (j as f64 + 0.5) * hh;
                let tc = rc > 0.0 && dx < rc && dy < rc && (rc - dx).powi(2) + (rc - dy).powi(2) > rc * rc;
                let (a, b) = (len(dx, ra).min(l), len(dy, rb).min(l));
                union += if tc { l } else { a.max(b) };
                sum += a + b + if tc { l } else { 0.0 };
            }
        }
        let full = (1.0 - std::f64::consts::FRAC_PI_4) * (ra * ra + rb * rb + rc * rc) * 20.0;
        8000.0 - full + (sum - union) * hh * hh
    }

    /// What the strips leave of the corner block where a patch over the corner of rounds `ra`,
    /// `rb`, `rc` on a, b, c lies: beyond a's end section (y < rc), b's (x > 20 − rc) and c's end
    /// (z > 20 − max(ra, rb), where c's rails meet a's and b's), the box's material that no
    /// strip removes, by columns along z. Outside that block such a body is the strips' (its
    /// rounds are the strips' cylinders up to those ends), so its patch takes at most this
    /// beyond them.
    fn corner_block(ra: f64, rb: f64, rc: f64) -> f64 {
        let len = |d: f64, r: f64| if d < r { r - (r * r - (r - d) * (r - d)).max(0.0).sqrt() } else { 0.0 };
        let (n, depth) = (2000, ra.max(rb));
        let h = rc / n as f64;
        let mut kept = 0.0;
        for i in 0..n {
            let dx = (i as f64 + 0.5) * h;
            for j in 0..n {
                let dy = (j as f64 + 0.5) * h;
                if (rc - dx).powi(2) + (rc - dy).powi(2) > rc * rc && dx < rc && dy < rc {
                    continue;
                }
                kept += depth - len(dx, ra).max(len(dy, rb)).min(depth);
            }
        }
        kept * h * h
    }

    /// Largest angle (degrees) between the two faces' normals along edges between curved faces.
    fn kink(b: &Body) -> f64 {
        let rec = Faces::of(b).unwrap();
        let mut worst: f64 = 0.0;
        for (i, e) in rec.rec.edges.iter().enumerate() {
            let faces: Vec<usize> = rec.faces_of(i).into_iter().map(|f| f as usize).collect();
            let [f, g] = faces[..] else { continue };
            if rec.plane_normal(f).is_some() || rec.plane_normal(g).is_some() {
                continue;
            }
            let c = CurveEval::new(e.parameters()).unwrap();
            let [t0, t1] = e.interval;
            for k in 1..10 {
                let p = c.point(t0 + (t1 - t0) * k as f64 / 10.0).unwrap();
                let (a, bb) = (rec.normal_at(f, p, 1e-3).unwrap(), rec.normal_at(g, p, 1e-3).unwrap());
                worst = worst.max(a.dot(bb).abs().min(1.0).acos().to_degrees());
            }
        }
        worst
    }

    /// The body's vertex points.
    fn vertices(b: &Body) -> Vec<Vec3> {
        crate::native::brep_record(b).unwrap().vertices.iter().map(|v| Vec3::new(v.point[0], v.point[1], v.point[2])).collect()
    }

    #[test]
    fn analytic_volumes_of_the_reference() {
        // One round and the sphere corner of three equal ones.
        let one = 8000.0 - (1.0 - std::f64::consts::FRAC_PI_4) * 4.0 * 20.0;
        assert!((strips(2.0, 0.0, 0.0) - one).abs() < 1e-6);
        // Rounds 2 and 3 on a and b: the overlap is ∫ (2 − √(4 − (z − 18)²))(3 − √(9 − (z − 17)²)) dz
        // over z from 18 to 20 (Simpson, 200000 steps).
        assert!((strips(2.0, 3.0, 0.0) - 7945.579002).abs() < 1e-3);
    }

    #[test]
    fn a_round_next_to_a_round_of_another_radius() {
        let (bx, a, b, _) = setup();
        let ra = fillet(&bx, &[a], 2.0).unwrap();
        // The round of b ends on the round of a: their cylinders trim each other.
        let r = fillet(&ra, &[b], 3.0).unwrap();
        assert_eq!(r.face_count(), 8);
        assert_eq!(r.unmeshed_faces(), 0);
        assert!((vol(&r) - strips(2.0, 3.0, 0.0)).abs() < 0.05, "{} vs {}", vol(&r), strips(2.0, 3.0, 0.0));
        // The same in one go with a radius per edge, either way round.
        for sel in [[(a, 2.0), (b, 3.0)], [(b, 3.0), (a, 2.0)]] {
            let s = fillet_radii(&bx, &sel).unwrap();
            assert!((vol(&s) - vol(&r)).abs() < 1e-3);
        }
        let s = fillet_radii(&bx, &[(a, 3.0), (b, 2.0)]).unwrap();
        assert!((vol(&s) - strips(3.0, 2.0, 0.0)).abs() < 0.05);
        // Equal radii: the mitre.
        let e = fillet(&ra, &[b], 2.0).unwrap();
        assert!((vol(&e) - strips(2.0, 2.0, 0.0)).abs() < 0.05);
    }

    #[test]
    fn issue1557_vertical_after_bottom_rounds() {
        let b = box_solid(Vec3::ZERO, Vec3::new(25.0, 25.0, 10.0)).unwrap();
        let bottom = [Vec3::new(12.5, 0.0, 0.0), Vec3::new(25.0, 12.5, 0.0), Vec3::new(12.5, 25.0, 0.0), Vec3::new(0.0, 12.5, 0.0)];
        let rounded = fillet(&b, &bottom, 2.0).unwrap();
        let vertical = Vec3::new(25.0, 0.0, 6.0);
        let seam = Vec3::new(24.287266336668306, 0.7127336633316916, 0.48266373325510686);
        let mut together = bottom.to_vec();
        together.push(vertical);
        let reference = fillet(&b, &together, 2.0).unwrap();
        // The vertical edge, alone or with the seam, with its tangent chain (as the Fillet feature
        // asks for it): every pick gives the same body.
        for picks in [vec![vertical], vec![vertical, seam], vec![seam, vertical]] {
            let result = fillet(&rounded, &crate::tangent_chain(&rounded, &picks).unwrap(), 2.0).unwrap();
            assert_eq!(result.face_count(), reference.face_count());
            assert_eq!(result.unmeshed_faces(), 0);
            assert!(result.validity().is_empty(), "{:?}", result.validity());
            assert!((vol(&result) - vol(&reference)).abs() < 0.05);
            assert!(!result.contains(Vec3::new(24.9, 0.1, 5.0)));
            assert!(result.contains(Vec3::new(24.0, 1.0, 5.0)));
        }
    }

    #[test]
    fn issue1557_connected_rounds_keep_their_radii() {
        let b = box_solid(Vec3::ZERO, Vec3::new(25.0, 25.0, 10.0)).unwrap();
        let bottom = [Vec3::new(12.5, 0.0, 0.0), Vec3::new(25.0, 12.5, 0.0), Vec3::new(12.5, 25.0, 0.0), Vec3::new(0.0, 12.5, 0.0)];
        let rounded = fillet(&b, &bottom, 2.0).unwrap();
        let vertical = Vec3::new(25.0, 0.0, 6.0);
        let seam = Vec3::new(24.287266336668306, 0.7127336633316916, 0.48266373325510686);
        for radius in [0.5, 1.0, 3.0] {
            let mut together: Vec<_> = bottom.iter().map(|p| (*p, 2.0)).collect();
            together.push((vertical, radius));
            let reference = fillet_radii(&b, &together).unwrap();
            let result = fillet(&rounded, &[seam, vertical], radius).unwrap();
            assert!(result.validity().is_empty(), "{:?}", result.validity());
            assert!((vol(&result) - vol(&reference)).abs() < 0.05, "radius {radius}");
            // The old bottom radius is still 2, away from the newly rounded corner.
            assert!(!result.contains(Vec3::new(12.5, 0.5, 0.5)));
            assert!(result.contains(Vec3::new(12.5, 1.0, 1.0)));
        }
        // A tangent boundary of an earlier round is not a sharp edge, and cannot be
        // reinterpreted as the sharp edge exposed when that round is taken off.
        assert!(fillet(&rounded, &[Vec3::new(12.5, 0.0, 2.0)], 1.0).is_err());
        // Consuming a mitre must not silently discard its independently requested radius,
        // or turn a mitre-only selection into a different sharp edge.
        assert!(fillet_radii(&rounded, &[(vertical, 1.0), (seam, 2.0)]).is_err());
        assert!(fillet_radii(&rounded, &[(seam, 2.0)]).is_err());
    }

    /// A 40 x 80 x 27 box (and the same shelled 1 mm, open at the top) with its bottom edges
    /// rounded R2 as four separate fillets (mitred at the bottom corners), then one vertical
    /// edge R2: the same solid whichever of its picks comes first. Horizontal sections: below
    /// z = r above the bottom, s up, the rectangle moved in by d = r − ρ, ρ = √(r² − (r − s)²),
    /// with the rounded corner a quarter disc of radius ρ less than its square (the sphere octant
    /// where the three rounds meet); above, the rectangle less that corner at r. With
    /// ∫ ρ = π r² / 4, ∫ ρ² = 2 r³ / 3, ∫ d = r² (1 − π/4) and ∫ d² = r³ (5/3 − π/2) over the
    /// bottom r. The shell's cavity, 38 x 78 x 26, lies clear of both rounds (the bottom round's
    /// arc passes 2 − √2 from the outer corner along the diagonal, the cavity's corner 1).
    #[test]
    fn issue2422_vertical_after_separate_bottom_rounds() {
        use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};
        let origin = Vec3::new(-5.0, 7.2, -2.0);
        let (l, w, h, r) = (40.0, 80.0, 27.0, 2.0);
        let b = box_solid(origin, origin + Vec3::new(l, w, h)).unwrap();
        let bottom = [Vec3::new(15.0, 7.2, -2.0), Vec3::new(35.0, 47.2, -2.0), Vec3::new(15.0, 87.2, -2.0), Vec3::new(-5.0, 47.2, -2.0)];
        let spandrel = 1.0 - FRAC_PI_4;
        let solid = l * w * r - 2.0 * (l + w) * r * r * spandrel + 4.0 * r * r * r * (5.0 / 3.0 - FRAC_PI_2) - spandrel * 2.0 * r * r * r / 3.0
            + (h - r) * (l * w - spandrel * r * r);
        let cavity = (l - 2.0) * (w - 2.0) * (h - 1.0);
        // The issue has no design attached. Reconstruct its outside corner and bottom mitre,
        // using the reported picks, with and without the open cavity visible in the screenshot.
        let picks = [Vec3::new(-5.0, 87.2, 12.5), Vec3::new(-4.283446394180081, 86.48344639418008, -1.5001085324947512)];
        for (base, want) in [(b.clone(), solid), (crate::shell(&b, &[Vec3::new(15.0, 47.2, 25.0)], 1.0).unwrap(), solid - cavity)] {
            // Each fillet with its tangent chain, as the Fillet feature asks for it by default
            // (tangent propagation).
            let mut rounded = base;
            for edge in bottom {
                rounded = fillet(&rounded, &crate::tangent_chain(&rounded, &[edge]).unwrap(), 2.0).unwrap();
            }
            for selection in [&picks[..1], &picks[..], &[picks[1], picks[0]][..]] {
                let result = fillet(&rounded, &crate::tangent_chain(&rounded, selection).unwrap(), 2.0).unwrap();
                assert!(result.validity().is_empty(), "{:?}", result.validity());
                assert!((vol(&result) - want).abs() < 1e-6 * want, "{selection:?}: {} vs {want}", vol(&result));
                assert!(!result.contains(Vec3::new(-4.9, 87.1, 10.0)));
                assert!(result.contains(Vec3::new(-4.1, 86.3, 10.0)));
            }
        }
    }

    #[test]
    fn issue1557_all_vertical_corners_match_analytic_volume() {
        let origin = Vec3::new(10.0, -5.0, 7.0);
        let b = box_solid(origin, origin + Vec3::new(25.0, 25.0, 10.0)).unwrap();
        let bottom =
            [Vec3::new(12.5, 0.0, 0.0), Vec3::new(25.0, 12.5, 0.0), Vec3::new(12.5, 25.0, 0.0), Vec3::new(0.0, 12.5, 0.0)].map(|p| p + origin);
        let vertical =
            [Vec3::new(0.0, 0.0, 6.0), Vec3::new(25.0, 0.0, 6.0), Vec3::new(25.0, 25.0, 6.0), Vec3::new(0.0, 25.0, 6.0)].map(|p| p + origin);
        let rounded = fillet(&b, &bottom, 2.0).unwrap();
        // Each vertical edge with its tangent chain (on through the bottom rounds' corners), as
        // the Fillet feature asks for it.
        let result = fillet(&rounded, &crate::tangent_chain(&rounded, &vertical).unwrap(), 2.0).unwrap();
        assert!(result.validity().is_empty(), "{:?}", result.validity());
        assert!(kink(&result) < 0.05);
        // Integrate rounded-square horizontal sections. Above z=r the area is
        // L²-(4-π)r²; below, a square of side L-2r plus a disk offset of radius
        // sqrt(r²-(z-r)²). This includes the four spherical bottom corners.
        let (l, h, r) = (25.0, 10.0, 2.0);
        let pi = std::f64::consts::PI;
        let a = l - 2.0 * r;
        let expected = (h - r) * (l * l - (4.0 - pi) * r * r) + a * a * r + pi * a * r * r + 2.0 * pi * r * r * r / 3.0;
        assert!((vol(&result) - expected).abs() < 0.1, "{} vs {expected}", vol(&result));
    }

    #[test]
    fn three_rounds_at_a_corner() {
        let (bx, a, b, c) = setup();
        // Two rounds meeting in a mitre, then the third edge with its tangent chain, as the Fillet
        // feature asks for it (84bd2daad): the mitre's seam leaves c's end along c, so the chain
        // is c and the seam, and the corner is a sphere octant. The cube less its three rounds
        // (each (1 − π/4) r² over its 18 mm straight part) and the corner's 2 mm cube less the
        // octant (r³ (1 − π/6)).
        let ab = fillet(&bx, &[a, b], 2.0).unwrap();
        let r = fillet(&ab, &crate::tangent_chain(&ab, &[c]).unwrap(), 2.0).unwrap();
        let sphere = 8000.0 - 3.0 * (1.0 - std::f64::consts::FRAC_PI_4) * 4.0 * 18.0 - 8.0 * (1.0 - std::f64::consts::PI / 6.0);
        assert_eq!(r.face_count(), 10);
        assert!(r.validity().is_empty(), "{:?}", r.validity());
        assert!((vol(&r) - sphere).abs() < 1e-11 * sphere, "{} vs {sphere}", vol(&r));
        assert!(kink(&r) < 0.01);
        // Different radii: a smooth patch over the corner, more than the strips take, and no more
        // than they leave in the block it lies in: its rounds end at a's section y = rc, b's
        // x = 20 − rc, and c's rails where they meet a's and b's (z = 20 − ra, z = 20 − rb), the
        // patch's four corners. (2, 2, 3): 1.168 beyond the strips, of the 11.86 the block keeps;
        // (2, 3, 4): 3.099 of 32.69.
        for (rs, sel) in [((2.0, 2.0, 3.0), fillet(&ab, &[c], 3.0)), ((2.0, 3.0, 4.0), fillet_radii(&bx, &[(a, 2.0), (b, 3.0), (c, 4.0)]))] {
            let r = sel.unwrap();
            assert_eq!(r.face_count(), 10);
            assert_eq!(r.unmeshed_faces(), 0);
            let k = kink(&r);
            assert!(k < 0.05, "kink {k}°");
            let (ra, rb, rc) = rs;
            let ends = [
                Vec3::new(20.0 - ra, rc, 20.0),
                Vec3::new(20.0, rc, 20.0 - ra),
                Vec3::new(20.0 - rc, rb, 20.0),
                Vec3::new(20.0 - rc, 0.0, 20.0 - rb),
            ];
            for p in ends {
                assert!(vertices(&r).iter().any(|q| q.dist(p) < 1e-9), "{rs:?}: no patch corner at {p:?}");
            }
            let (v, s) = (vol(&r), strips(ra, rb, rc));
            assert!(v < s + 0.05 && v > s - corner_block(ra, rb, rc), "{v} vs {s}");
        }
    }

    #[test]
    fn corners_refuse_what_they_cannot_build() {
        let (bx, a, b, _) = setup();
        // Larger than the faces, or not a size: errors, no panic. Large but fitting is fine.
        assert!(fillet_radii(&bx, &[(a, 15.0), (b, 2.0)]).is_ok_and(|r| r.unmeshed_faces() == 0));
        assert!(fillet_radii(&bx, &[(a, 25.0), (b, 2.0)]).is_err());
        assert!(fillet_radii(&bx, &[(a, f64::NAN)]).is_err());
        assert!(fillet_radii(&bx, &[(a, 0.0)]).is_err());
        assert!(fillet_radii(&bx, &[]).is_err());
        assert!(fillet_radii(&bx, &[(Vec3::new(500.0, 0.0, 0.0), 1.0)]).is_err());
    }

    #[test]
    fn sloped_basin_rim_fillets() {
        use solidcraft_geom::{Loop2, Plane, Vec2};
        let rectangle = |x0, y0, x1, y1| Loop2::polygon(&[Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]);
        let slab = box_solid(Vec3::ZERO, Vec3::new(100.0, 60.0, 30.0)).unwrap();
        let pocket =
            crate::loft(&[(Plane::XY.offset(30.0), rectangle(20.0, 4.0, 80.0, 50.0)), (Plane::XY.offset(10.0), rectangle(30.0, 20.0, 70.0, 40.0))])
                .unwrap();
        let basin = crate::boolean(&slab, &pocket, crate::BoolOp::Cut).unwrap().unwrap();
        let picks = [Vec3::new(50.0, 4.0, 30.0), Vec3::new(80.0, 27.0, 30.0), Vec3::new(50.0, 50.0, 30.0), Vec3::new(20.0, 27.0, 30.0)];
        for count in 1..=4 {
            let result = fillet(&basin, &picks[..count], 2.0).unwrap();
            assert!(result.validity().is_empty());
            assert_eq!(result.unmeshed_faces(), 0);
            assert!(vol(&result) < vol(&basin));
            let mesh = result.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
            assert!(mesh.contains(Vec3::new(50.0, 2.0, 27.0)));
            assert!(!mesh.contains(Vec3::new(50.0, 3.9, 29.99)));
            // The small blend removes only a strip at the rim, and never escapes
            // the slab. Topology checks alone do not catch crossed faces.
            assert!(vol(&basin) - vol(&result) < 2.0_f64.powi(2) * (60.0 + 46.0) * 2.0);
            for p in vertices(&result) {
                assert!(p.x >= 0.0 && p.x <= 100.0 && p.y >= 0.0 && p.y <= 60.0 && p.z >= 0.0 && p.z <= 30.0 + 1e-8);
            }
        }
    }

    /// A chamfer 2 of a basin's rim loop (walls all sloping 10 over 20, the front rim 1 mm wide)
    /// whose front leg runs off the rim over the slab's front edge builds, cut by the front face
    /// (the owner's ruling of 2026-10-30, d7: a chamfer that reaches or runs past a rim builds).
    /// Sliced at depth t below the top, each chamfer's strip beside its wall is
    /// w(t) = d (1 − t/δ) wide, δ = d / √(1 + k²) with k = 1/2 the walls' run over drop; the four
    /// meet in mitres at the rim's corners, so the section removed is the pocket's rectangle at t,
    /// [20 + t/2, 80 − t/2] × [1 + t/2, 50 − t/2], grown by w(t) on every side and clipped to the
    /// slab's front face y >= 0, less that rectangle. Its area is quadratic in t between δ and
    /// where the front strip's edge 1 + t/2 − w(t) crosses y = 0, so Simpson's rule is exact on
    /// each piece: 375.673069 taken from V0 = 140333.333333 (the prismatoid), 139957.660264.
    #[test]
    #[ignore = "fails until SMLib builds the cliff overflow of a blended loop: CloseRemovalRegion refuses it (SM_FILERR_NO_ROOM, its fillet boundary closes on one face)"]
    fn a_rim_loop_chamfer_running_off_its_rim_builds() {
        use solidcraft_geom::{Loop2, Plane, Vec2};
        let rectangle = |x0, y0, x1, y1| Loop2::polygon(&[Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]);
        let slab = box_solid(Vec3::ZERO, Vec3::new(100.0, 60.0, 30.0)).unwrap();
        let pocket =
            crate::loft(&[(Plane::XY.offset(30.0), rectangle(20.0, 1.0, 80.0, 50.0)), (Plane::XY.offset(10.0), rectangle(30.0, 11.0, 70.0, 40.0))])
                .unwrap();
        let basin = crate::boolean(&slab, &pocket, crate::BoolOp::Cut).unwrap().unwrap();
        let picks = [Vec3::new(50.0, 1.0, 30.0), Vec3::new(80.0, 27.0, 30.0), Vec3::new(50.0, 50.0, 30.0), Vec3::new(20.0, 27.0, 30.0)];
        let (d, k) = (2.0_f64, 0.5_f64);
        let delta = d / (1.0 + k * k).sqrt();
        let area = |t: f64| {
            let w = d * (1.0 - t / delta);
            let (x0, x1, y0, y1) = (20.0 + k * t, 80.0 - k * t, 1.0 + k * t, 50.0 - k * t);
            (x1 - x0 + 2.0 * w) * (y1 + w - (y0 - w).max(0.0)) - (x1 - x0) * (y1 - y0)
        };
        // 1 + k t − d (1 − t/δ) = 0.
        let crossing = (d - 1.0) / (k + d / delta);
        let removed: f64 =
            [(0.0, crossing), (crossing, delta)].iter().map(|&(a, b)| (b - a) / 6.0 * (area(a) + 4.0 * area(0.5 * (a + b)) + area(b))).sum();
        assert!((removed - 375.673069).abs() < 1e-6, "{removed}");
        let before = crate::measure(&basin).unwrap().volume;
        assert!((before - 140333.333333).abs() < 1e-5, "{before}");
        let result = crate::chamfer(&basin, &picks, d).unwrap_or_else(|e| panic!("{e}"));
        assert!(result.validity().is_empty(), "{:?}", result.validity());
        let v = crate::measure(&result).unwrap().volume;
        assert!((v - (before - removed)).abs() < 1e-6 * before, "{v} vs {}", before - removed);
    }
}

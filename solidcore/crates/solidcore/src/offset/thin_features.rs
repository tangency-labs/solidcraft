//! Not upstream (the Dutchman stress corpus's Shell family: S0003, S0012, S0061): an inward
//! shell of a solid with a feature narrower than twice the wall.
//!
//! A pin, boss or pad no wider than twice the wall holds no cavity: every point in it lies
//! within the wall of its own sides. The offset still offsets the feature's faces, and their
//! offsets collapse (a round's offset by its radius or more), close an edge to nothing, or
//! meet across the feature, and the shell is refused for it. That refusal is sound for the
//! offset, but the hollow is well defined: it is the hollow of the body without the
//! feature. Over a planar footprint, the offset of the face the feature stands on runs on
//! across the footprint, so the offset (mitred, as this kernel's is) of the body is the
//! offset of the body with the feature taken off.
//!
//! [`shell_taking_off_thin_features`] takes such a feature off (solidcore's own defeature,
//! [`defeature_and_rebuild`], its footprint capped with a plane on both sides), shells what
//! is left, and joins the feature back on, solid. A feature is a set of faces, at most half
//! the body's, reached from a face across convex edges (a feature's own faces meet at convex
//! edges; the face it stands on meets it at concave ones); the smallest are tried first, one
//! at a time, until the shell of what is left builds. One is taken off only where the room
//! test finds no cavity in it ([`shell_wall_room`] shows no witness: the room test proves a
//! cavity, it does not prove there is none), and the whole result stands only if every step
//! builds; otherwise the caller's refusal stands. The features with a face whose inset
//! collapses come off together, first, before one shell of what is left (solidcore#3353).

use std::collections::BTreeSet;

use crate::blend::feature_executive::{defeature_and_rebuild, RebuildBrepType};
use crate::boolean::api_brep::boolean_union;
use crate::error::offset::OffsetFailure;
use crate::error::{Reason, Result, SolidError};
use crate::kernel::Kernel;
use crate::offset::offset_executive::p2::{inset_face_collapses, shell_brep_simple, ShellOptions};
use crate::offset::wall_room::{shell_wall_room, WallRoom};
use crate::topology::edgeuse::{p1 as eu1, p2 as eu2};
use crate::topology::topology_traverser::face_edges;
use crate::topology::{edge, Brep, FaceId};

/// The most features one shell takes off.
const MAX_FEATURES: usize = 16;

/// The angle within which two faces at an edge count as tangent (degrees): a tangent edge is
/// not crossed, so a feature is never grown across a blend into the face it stands on.
const TANGENT_DEG: f64 = 0.5;

/// Whether `e` is the refusal of an offset by a feature narrower than twice the wall: a face
/// whose offset collapses, folds, or meets its opposite face, or an edge whose offset closes
/// to nothing.
#[must_use]
pub fn refused_for_a_thin_feature(e: &SolidError) -> bool {
    matches!(
        e.failure().map(|f| &f.reason),
        Some(Reason::Offset(
            OffsetFailure::Collapses { .. }
                | OffsetFailure::EdgeVanishes { .. }
                | OffsetFailure::WallsMeetAcross { .. }
                | OffsetFailure::Folds { .. }
        ))
    )
}

/// The faces reached from `seed` across convex edges, `None` when that is every face of `b`
/// (no feature: the seed's convex neighbours are the whole body).
fn feature_from(k: &Kernel, b: &mut Brep, seed: FaceId) -> Result<Option<Vec<FaceId>>> {
    let all = b.faces().ids().count();
    let mut found: BTreeSet<FaceId> = BTreeSet::from([seed]);
    let mut todo = vec![seed];
    while let Some(f) = todo.pop() {
        let outer = crate::topology::face::p2::outer_faceuse(b, f)?;
        for e in face_edges(b, f)? {
            let faces = edge::faces(b, e, None)?;
            // the use of the edge on `f`'s face use that faces out of the material
            let mut on_f = None;
            for u in edge::edgeuses(b, e, None, None)? {
                let Some(fu) = eu1::faceuse(b, u)? else {
                    continue;
                };
                if eu1::face(b, u)? == Some(f) && Some(fu) == outer {
                    on_f = Some(u);
                }
            }
            let Some(eu) = on_f else {
                continue;
            };
            // a seam, a tangent edge or a concave one is not crossed
            if faces.len() != 2
                || faces.first() == faces.get(1)
                || eu2::is_tangent_sector(k, b, eu, TANGENT_DEG, false, true)?
                || !eu2::is_convex_radial_sector(k, b, eu, 5, false, TANGENT_DEG)?
            {
                continue;
            }
            for g in edge::faces(b, e, None)? {
                if found.insert(g) {
                    todo.push(g);
                }
            }
        }
        if found.len() >= all {
            return Ok(None);
        }
    }
    Ok(Some(found.into_iter().collect()))
}

/// The features of `b`: each set of two or more faces, at most half of them, reached from a
/// face across convex edges ([`feature_from`]), smallest first, each once.
fn features(k: &Kernel, b: &Brep) -> Result<Vec<Vec<FaceId>>> {
    // a measurement copy: it reads the topology and tangency of the body and its faces are
    // only grouped, never built from, so its surfaces' definitions are not needed
    let mut probe = b.clone_standing_for_self()?;
    let mut seen: BTreeSet<FaceId> = BTreeSet::new();
    let mut found = Vec::new();
    for f in b.faces().ids().collect::<Vec<_>>() {
        if seen.contains(&f) {
            continue;
        }
        let Some(faces) = feature_from(k, &mut probe, f)? else {
            seen.insert(f);
            continue;
        };
        seen.extend(faces.iter().copied());
        // a feature is the smaller part: at most half the faces
        if faces.len() >= 2 && 2 * faces.len() <= b.faces().ids().count() {
            found.push(faces);
        }
    }
    found.sort_by_key(Vec::len);
    Ok(found)
}

/// The features of `b` ([`features`]) in the steps they are taken off in, each step followed
/// by a shell of what is left: first, together, every feature one of whose faces' inset by
/// `wall` collapses ([`inset_face_collapses`]: a pin or boss whose side the wall shrinks onto
/// its axis), since each such feature left on is refused again, and its collapse is refused
/// only once every face's offset and mitre is built (solidcore#3353: seven Ø4 bosses on a drum
/// of 120 holes took eight shells, seven of them refused, before the one that builds; now
/// two); then each other feature on its own, smallest first.
fn steps(k: &Kernel, b: &Brep, wall: f64) -> Result<Vec<Vec<Vec<FaceId>>>> {
    let mut collapsing: Vec<Vec<FaceId>> = Vec::new();
    let mut others: Vec<Vec<FaceId>> = Vec::new();
    for faces in features(k, b)? {
        let mut collapses = false;
        for &f in &faces {
            match inset_face_collapses(k, b, f, wall) {
                Ok(true) => {
                    collapses = true;
                    break;
                }
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Ok(false) | Err(_) => {}
            }
        }
        if collapses {
            collapsing.push(faces);
        } else {
            others.push(faces);
        }
    }
    let mut steps = Vec::new();
    if !collapsing.is_empty() {
        steps.push(collapsing);
    }
    steps.extend(others.into_iter().map(|f| vec![f]));
    Ok(steps)
}

/// `b` with `faces` taken off as a feature: the base and the feature, each closed by a planar
/// cap on the footprint. `None` when it does not come off, or has room for a cavity of `wall`.
fn take_off(k: &mut Kernel, b: &Brep, faces: &[FaceId], wall: f64) -> Result<Option<(Brep, Brep)>> {
    // the footprint capped with its plane (the module's claim holds over a planar one only);
    // the base is shelled and the feature joined back, so both keep the faces' definitions
    let mut base = b.clone_standing_for_self_keeping_definitions()?;
    let mut feature = match defeature_and_rebuild(
        k,
        &mut base,
        faces,
        RebuildBrepType::PlanarCap,
        RebuildBrepType::PlanarCap,
    ) {
        Ok(Some(f)) => f,
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        _ => return Ok(None),
    };
    base.refresh_cached_topology();
    feature.refresh_cached_topology();
    // a feature with room for a cavity is not one the wall fills
    Ok(match shell_wall_room(k, &mut feature, &[], wall)? {
        WallRoom::NotShown => Some((base, feature)),
        _ => None,
    })
}

/// The shell of `b` (`options`, inward: a negative distance, no faces opened) whose offset
/// was refused for a feature narrower than twice the wall
/// ([`refused_for_a_thin_feature`]): each such feature taken off, what is left shelled, and
/// the features joined back on, solid (module docs). `None` when a feature cannot be taken
/// off or has room for a cavity, more than [`MAX_FEATURES`] are refused, the shell of what
/// is left is refused for another reason, or a join fails: the caller's refusal stands.
///
/// # Errors
///
/// A cancellation, or an unported step, of any call made.
pub fn shell_taking_off_thin_features(
    k: &mut Kernel,
    b: &Brep,
    options: ShellOptions,
) -> Result<Option<Brep>> {
    match taking_off(k, b, options) {
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(None),
        ok => ok,
    }
}

/// [`shell_taking_off_thin_features`], any failure of a step its own.
fn taking_off(k: &mut Kernel, b: &Brep, options: ShellOptions) -> Result<Option<Brep>> {
    let wall = -options.offset.distance;
    let usable = wall.is_finite() && wall > 0.0 && options.create_offset_solid;
    if !usable {
        return Ok(None);
    }
    let passes = |r: Result<Option<Brep>>| -> Result<Option<Brep>> {
        match r {
            Err(e) if e.is_unported_or_cancelled() => Err(e),
            Err(_) => Ok(None),
            ok => ok,
        }
    };
    // the body each feature is taken off and shelled, so its definitions are kept
    let mut current = b.clone_standing_for_self_keeping_definitions()?;
    let mut taken: Vec<Brep> = Vec::new();
    for step in steps(k, b, wall)? {
        let mut took = false;
        for faces in step {
            if taken.len() >= MAX_FEATURES {
                return Ok(None);
            }
            let Some((base, feature)) = take_off(k, &current, &faces, wall)? else {
                continue;
            };
            taken.push(feature);
            current = base;
            took = true;
        }
        if !took {
            continue;
        }
        match shell_brep_simple(k, &current, options, &[]) {
            Ok(shelled) => {
                let mut result = shelled;
                for f in taken {
                    let Some(joined) = passes(boolean_union(k, result, f).map(Some))? else {
                        return Ok(None);
                    };
                    result = joined;
                }
                result.refresh_cached_topology();
                return Ok(Some(result));
            }
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(e) if refused_for_a_thin_feature(&e) => {}
            Err(_) => return Ok(None),
        }
    }
    Ok(None)
}

/// `SmApiShellBrepFull` ([`shell_brep_simple`]) and, for an inward shell of a closed body
/// refused for a feature narrower than twice the wall ([`refused_for_a_thin_feature`]),
/// [`shell_taking_off_thin_features`]: the shell every caller makes, so that each gets the
/// same hollow. The first refusal stands where the second way builds nothing.
///
/// # Errors
///
/// Those of [`shell_brep_simple`]; a cancellation or an unported step of the second way.
pub fn shell_brep(
    k: &mut Kernel,
    b: &Brep,
    options: ShellOptions,
    open: &[FaceId],
) -> Result<Brep> {
    match shell_brep_simple(k, b, options, open) {
        Err(e)
            if options.offset.distance < 0.0
                && open.is_empty()
                && refused_for_a_thin_feature(&e) =>
        {
            match shell_taking_off_thin_features(k, b, options)? {
                Some(made) => Ok(made),
                None => Err(e),
            }
        }
        r => r,
    }
}

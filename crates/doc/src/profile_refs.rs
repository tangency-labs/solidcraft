//! Exact profile references through analytic offset topology transitions.
use solidcraft_geom::prelude::*;
use std::collections::BTreeSet;

use crate::{CurveSide, DocError, ProfileSel, Result, SidedCurve, SolvedSketch, WholeProfileBoundary};

const MAX_SELECTIONS: usize = 128;
/// An exact model profile may have many independent spline-drawn holes (#18759). Its
/// curve count and per-boundary sizes are still bounded below.
const MAX_MODEL_WHOLE_HOLES: usize = 1024;
/// Drawing fills retain their separate 128-hole policy until their own limits are reviewed.
pub(crate) const WHOLE_MAX_HOLES: usize = 128;
pub(crate) const WHOLE_MAX_BOUNDARY_CURVES: usize = 512;
pub(crate) const WHOLE_MAX_TOTAL_CURVES: usize = 20_000;
pub(crate) const WHOLE_MAX_ID_BYTES: usize = 512;

/// The most curves one loop of a whole-source profile reference names: as many as a sketch read
/// from a design may hold, so the outline of a long line of script text or its offset can be
/// picked (it used to stop at 512).
pub(crate) const MAX_WHOLE_LOOP_CURVES: usize = solidcraft_sketch::limits::CEILING_POINTS;
/// The most curves all loops of one whole-source reference name together: a curve bounds at
/// most the regions on its two sides, so twice what one sketch holds.
pub(crate) const MAX_WHOLE_CURVES: usize = 2 * solidcraft_sketch::limits::CEILING_POINTS;

/// What a whole-source profile pick past [`MAX_WHOLE_LOOP_CURVES`] or [`MAX_WHOLE_CURVES`]
/// says. The interface translates it by this exact text.
pub const WHOLE_PROFILE_TOO_MANY_CURVES: &str =
    "the picked profiles have too many curves to keep track of; pick fewer profiles at once, or draw their outlines with fewer curves";

fn whole_boundary(profile: &solidcraft_sketch::Profile) -> Option<WholeProfileBoundary> {
    let (outer, holes) = profile.whole_curve_boundaries()?;
    Some(WholeProfileBoundary { outer, holes })
}

pub(crate) fn validate_whole(profiles: &[WholeProfileBoundary]) -> Result<()> {
    if profiles.is_empty() || profiles.len() > MAX_SELECTIONS {
        return Err(DocError::Invalid("whole-source profile selection needs bounded unique profiles".into()));
    }
    let mut total = 0usize;
    let mut identities = BTreeSet::new();
    for profile in profiles {
        if profile.holes.len() > MAX_MODEL_WHOLE_HOLES {
            return Err(DocError::Invalid(format!("whole-source profile exceeds {MAX_MODEL_WHOLE_HOLES} holes")));
        }
        if !identities.insert((&profile.outer, &profile.holes)) {
            return Err(DocError::Invalid("whole-source profile has a duplicate identity".into()));
        }
        // Holes may share a curve: a curve that splits an island bounds two of its regions, and
        // both are holes of the profile around the island (#17613). No hole shares the outer's.
        let mut outer = BTreeSet::new();
        for (hole, boundary) in std::iter::once(&profile.outer).chain(&profile.holes).enumerate() {
            total = total.saturating_add(boundary.len());
            if boundary.is_empty() {
                return Err(DocError::Invalid("whole-source boundary needs canonical disjoint curve identities".into()));
            }
            if boundary.len() > MAX_WHOLE_LOOP_CURVES || total > MAX_WHOLE_CURVES {
                return Err(DocError::Invalid(WHOLE_PROFILE_TOO_MANY_CURVES.into()));
            }
            let mut previous = None;
            for curve in boundary {
                if curve.is_empty()
                    || curve.len() > WHOLE_MAX_ID_BYTES
                    || curve.chars().any(char::is_control)
                    || previous.is_some_and(|previous| previous >= curve)
                    || if hole == 0 { !outer.insert(curve) } else { outer.contains(curve) }
                {
                    return Err(DocError::Invalid("whole-source boundary needs canonical disjoint curve identities".into()));
                }
                previous = Some(curve);
            }
        }
        if profile.holes.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(DocError::Invalid("whole-source holes need canonical unique identities".into()));
        }
    }
    Ok(())
}

fn whole_profile_indices(ss: &SolvedSketch, profiles: &[WholeProfileBoundary]) -> Result<Vec<usize>> {
    whole_profile_indices_in(&ss.profiles, &ss.name, profiles)
}

/// Exact complete-boundary matching shared by model profiles and sheet fills.
/// The caller must refuse an incomplete profile search before asking for picks.
pub(crate) fn whole_profile_indices_in(solved: &[solidcraft_sketch::Profile], name: &str, profiles: &[WholeProfileBoundary]) -> Result<Vec<usize>> {
    validate_whole(profiles)?;
    let current = solved.iter().map(whole_boundary).collect::<Vec<_>>();
    profiles
        .iter()
        .map(|identity| {
            let mut matches = current.iter().enumerate().filter(|(_, candidate)| candidate.as_ref() == Some(identity));
            let (index, _) = matches.next().ok_or_else(|| DocError::Unknown(format!("complete source profile in {name}")))?;
            if matches.next().is_some() {
                return Err(DocError::Invalid(format!("ambiguous complete source profile in {name}")));
            }
            Ok(index)
        })
        .collect()
}

fn same_ids(a: &[String], b: &[String]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let (mut a, mut b) = (a.iter().collect::<Vec<_>>(), b.iter().collect::<Vec<_>>());
    a.sort();
    b.sort();
    a == b
}

/// A migrated whole projection names its source's entire single loop, independently of the
/// number and kinds of curves used to represent it. Complete graph coverage certifies the
/// regenerated region; a cut loop, holes or multiple loops cannot stand for the saved outline.
/// No mesh, nearest region or shared-identifier score participates in this resolution.
fn projected_boundary(ss: &SolvedSketch, curves: &[String]) -> Option<Result<usize>> {
    use solidcraft_sketch::{CurvePick, LinkKind, LinkSource};

    let mut links = ss.sketch.links.iter().filter(|link| !link.profile_boundary.is_empty() && same_ids(curves, &link.profile_boundary));
    let link = links.next()?;
    let missing = || crate::curve_pick_error(CurvePick::Missing, curves, &ss.name);
    if links.next().is_some() || link.lost || link.kind != LinkKind::Project || !matches!(link.source, LinkSource::Body { .. }) {
        return Some(Err(missing()));
    }
    let output: Vec<String> =
        ss.sketch.curves.iter().filter(|curve| curve.link.as_deref() == Some(link.id.as_str())).map(|curve| curve.id.clone()).collect();
    if output.is_empty() || output.len() > solidcraft_sketch::MAX_LINK_ENTITIES {
        return Some(Err(missing()));
    }
    // Cached profiles do not serialize their complete-coverage certificates. Re-find them
    // from the exact sketch curves, and refuse an incomplete search rather than guessing.
    let search = solidcraft_sketch::find_profiles_checked(&ss.sketch);
    if search.incomplete {
        return Some(Err(missing()));
    }
    let mut matches = search.profiles.iter().filter_map(|profile| {
        let (outer, holes) = profile.whole_curve_boundaries()?;
        (holes.is_empty() && same_ids(&outer, &output)).then_some(outer)
    });
    let Some(boundary) = matches.next() else { return Some(Err(missing())) };
    if matches.next().is_some() {
        return Some(Err(missing()));
    }
    Some(solidcraft_sketch::profile_by_curves(&ss.profiles, &boundary).map_err(|error| crate::curve_pick_error(error, curves, &ss.name)))
}

fn unique_profile(ss: &SolvedSketch, curves: &[String]) -> Result<usize> {
    let mut matches = ss.profiles.iter().enumerate().filter(|(_, profile)| same_ids(curves, &profile.outer_curves));
    let (index, _) = matches.next().ok_or_else(|| DocError::Unknown(format!("profile bounded by {curves:?} in {}", ss.name)))?;
    if matches.next().is_some() {
        return Err(DocError::Invalid(format!("ambiguous profile boundary in {}", ss.name)));
    }
    Ok(index)
}

fn reserved_boundaries(ss: &SolvedSketch) -> Result<std::collections::BTreeMap<Vec<String>, String>> {
    let mut found = std::collections::BTreeMap::new();
    for offset in &ss.sketch.offsets {
        let Some(topology) = &offset.topology else { continue };
        let mut reserved = topology.roles.iter().map(|role| role.output.clone()).collect::<Vec<_>>();
        reserved.sort();
        if found.insert(reserved, topology.id.clone()).is_some() {
            return Err(DocError::Invalid("profile role set belongs to multiple offset associations".into()));
        }
    }
    Ok(found)
}

/// Resolve exact selection identities to the solved sketch's current profile indices.
/// Complete reserved role sets follow their association; partial sets never expand.
pub fn profile_selection_indices(ss: &SolvedSketch, selection: &ProfileSel) -> Result<Vec<usize>> {
    selection_indices(ss, selection, None)
}

/// [`profile_selection_indices`] for evaluating a feature: an ordinary curve loop that no longer
/// bounds a profile exactly (a curve now cuts the region, or a side was deleted and drawn again)
/// falls back to the profiles that best match it (`fallback_loop`) instead of failing, and says
/// so in the returned notes. Commands still check picks with the exact form.
pub fn profile_selection_indices_lenient(ss: &SolvedSketch, selection: &ProfileSel) -> Result<(Vec<usize>, Vec<String>)> {
    let mut notes = Vec::new();
    let indices = selection_indices(ss, selection, Some(&mut notes))?;
    Ok((indices, notes))
}

/// What a feature's profile selection falls back to when evaluated: one note per curve loop that
/// no longer bounds a profile exactly; empty when everything matched exactly (or nothing did).
pub fn profile_fallback_notes(ss: &SolvedSketch, selection: &ProfileSel) -> Vec<String> {
    if !matches!(selection, ProfileSel::Curves { .. } | ProfileSel::SidedCurves { .. }) || profile_selection_indices(ss, selection).is_ok() {
        return Vec::new();
    }
    profile_selection_indices_lenient(ss, selection).map(|(_, notes)| notes).unwrap_or_default()
}

/// A curve that is a line, an arc or a circle, and not the output of a free-form offset.
fn is_analytic(sketch: &solidcraft_sketch::Sketch, id: &str) -> bool {
    sketch.curves.iter().any(|curve| {
        curve.id == id
            && matches!(
                curve.kind,
                solidcraft_sketch::CurveKind::Line { .. } | solidcraft_sketch::CurveKind::Arc { .. } | solidcraft_sketch::CurveKind::Circle { .. }
            )
    }) && !sketch.freeform_offsets.iter().any(|offset| offset.output == id)
        && !sketch.freeform_chain_offsets.iter().any(|offset| offset.outputs.iter().any(|out| out == id))
}

/// A loop made only of analytic curves: how many pieces each is cut into does not matter.
fn is_analytic_loop(sketch: &solidcraft_sketch::Sketch, curves: &[String]) -> bool {
    !curves.is_empty() && curves.iter().all(|id| is_analytic(sketch, id))
}

fn quoted(ids: &[String]) -> String {
    ids.iter().map(|id| format!("`{id}`")).collect::<Vec<_>>().join(", ")
}

/// The profiles that best stand for an ordinary curve loop `curves` (the outer loop of a profile
/// when it was picked) that no longer bounds any profile exactly, with a note saying what changed.
/// `None` when nothing plausibly matches.
///
/// 1. All of its curves are still there and still close a loop on their own, but other curves
///    now cut the region inside: the pieces inside that loop that touch it, whose union is the
///    region that was picked (islands inside it stay holes, as they were).
/// 2. Otherwise the profile whose outer loop shares the most of its curves, at least half of
///    them; a tie goes to the one with the fewest curves the loop did not have, and a tie left
///    after that is not guessed.
///
/// Analytic loops (lines, arcs and circles only) may list a curve more than once, as the pieces a
/// circle was cut into when the loop was saved; the repeats are one curve here, since a sketch
/// edit changes how many pieces a curve has but not the region it bounds. The outputs of an
/// analytic offset (`sketch.offsets`) take step 1 only: their loop closes on its own or the pick
/// is lost, a partial match by identifiers never stands for it. Loops with free-form offset
/// outputs, free curves (splines, ellipses, conics) and legacy graph-span multisets keep their
/// exact meaning and never fall back.
fn fallback_loop(ss: &SolvedSketch, curves: &[String]) -> Option<(Vec<usize>, String)> {
    let sketch = &ss.sketch;
    let analytic_loop = is_analytic_loop(sketch, curves);
    let deduped: Vec<String> = if analytic_loop {
        let mut seen = BTreeSet::new();
        curves.iter().filter(|id| seen.insert(id.as_str())).cloned().collect()
    } else {
        curves.to_vec()
    };
    let curves = deduped.as_slice();
    let stored: BTreeSet<&str> = curves.iter().map(String::as_str).collect();
    if stored.is_empty() || stored.len() != curves.len() || curves.len() > 512 || ss.profiles.len() > 10_000 {
        return None;
    }
    if stored.iter().any(|id| {
        sketch.freeform_offsets.iter().any(|offset| offset.output == *id)
            || sketch.freeform_chain_offsets.iter().any(|offset| offset.outputs.iter().any(|out| out == *id))
    }) {
        return None;
    }
    let from_offset = stored.iter().any(|id| sketch.offsets.iter().any(|offset| offset.owns_output(id)));
    if from_offset && !analytic_loop {
        return None;
    }
    let drawn = |id: &str| sketch.curves.iter().any(|curve| curve.id == id && !curve.construction && !curve.centerline);
    let touches = |profile: &solidcraft_sketch::Profile| profile.outer_curves.iter().any(|id| stored.contains(id.as_str()));
    // 1. The loop alone still closes: take the pieces of the region it bounds.
    if stored.iter().all(|id| drawn(id)) {
        let mut alone = sketch.clone();
        for curve in &mut alone.curves {
            if !stored.contains(curve.id.as_str()) {
                curve.construction = true;
            }
        }
        let loops = solidcraft_sketch::find_profiles(&alone);
        if let Ok(index) = solidcraft_sketch::profile_by_curves(&loops, curves)
            && let Some(region) = loops.get(index).map(|profile| &profile.region)
        {
            let pieces: Vec<usize> = ss
                .profiles
                .iter()
                .enumerate()
                .filter(|(_, profile)| touches(profile) && region.outer.contains(profile.region.interior_point()))
                .map(|(index, _)| index)
                .collect();
            if !pieces.is_empty() {
                let note = format!(
                    "the profile of {} bounded by {} is cut by another curve now; using the {} pieces inside it",
                    ss.name,
                    quoted(curves),
                    pieces.len()
                );
                return Some((pieces, note));
            }
        }
    }
    if from_offset {
        return None;
    }
    // 2. The closest loop by the curves it shares.
    let count = |profile: &solidcraft_sketch::Profile, inside: bool| {
        profile.outer_curves.iter().map(String::as_str).filter(|id| stored.contains(id) == inside).collect::<BTreeSet<_>>().len()
    };
    let best = ss.profiles.iter().map(|profile| count(profile, true)).max()?;
    if best == 0 || best.saturating_mul(2) < stored.len() {
        return None;
    }
    let tied: Vec<&solidcraft_sketch::Profile> = ss.profiles.iter().filter(|profile| count(profile, true) == best).collect();
    let fewest = tied.iter().map(|profile| count(profile, false)).min()?;
    let mut chosen = ss.profiles.iter().enumerate().filter(|(_, profile)| count(profile, true) == best && count(profile, false) == fewest);
    let (index, profile) = chosen.next()?;
    if chosen.next().is_some() {
        return None;
    }
    let now: BTreeSet<&str> = profile.outer_curves.iter().map(String::as_str).collect();
    let gone: Vec<String> = curves.iter().filter(|id| !now.contains(id.as_str())).cloned().collect();
    let new: Vec<String> = solidcraft_sketch::distinct_curves(&profile.outer_curves).into_iter().filter(|id| !stored.contains(id.as_str())).collect();
    let mut changes = Vec::new();
    if !gone.is_empty() {
        changes.push(format!("{} no longer on it", quoted(&gone)));
    }
    if !new.is_empty() {
        changes.push(format!("{} new", quoted(&new)));
    }
    let note = format!(
        "no profile of {} is bounded by exactly {} any more ({}); using the profile that shares {best} of its {} curves",
        ss.name,
        quoted(curves),
        changes.join(", "),
        curves.len()
    );
    Some((vec![index], note))
}

fn selection_indices(ss: &SolvedSketch, selection: &ProfileSel, mut notes: Option<&mut Vec<String>>) -> Result<Vec<usize>> {
    let indices = match selection {
        ProfileSel::All => (0..ss.profiles.len()).collect(),
        ProfileSel::Indices { indices } => indices.clone(),
        ProfileSel::Curves { loops, sides } => {
            let boundaries = reserved_boundaries(ss)?;
            let mut indices = Vec::with_capacity(loops.len());
            for (k, curves) in loops.iter().enumerate() {
                if let Some(index) = projected_boundary(ss, curves) {
                    indices.push(index?);
                    continue;
                }
                let mut key = curves.clone();
                key.sort();
                if let Some(id) = boundaries.get(&key) {
                    if curves.len() > 512 || curves.iter().collect::<BTreeSet<_>>().len() != curves.len() {
                        return Err(DocError::Invalid("offset profile has repeated or excessive curve roles".into()));
                    }
                    indices.push(unique_profile(ss, &ss.sketch.offset_boundary(id)?)?);
                } else {
                    // Repeated saved IDs of free curves identify exact graph spans. Their
                    // multiplicity must not silently turn into a whole-source certificate on
                    // resize. Lines, arcs and circles are exempt: how many pieces such a curve is
                    // cut into changes with every line drawn across it (#9368).
                    if curves.iter().collect::<BTreeSet<_>>().len() != curves.len()
                        && !is_analytic_loop(&ss.sketch, curves)
                        && !ss.profiles.iter().any(|profile| same_ids(curves, &profile.outer_curves))
                    {
                        return Err(crate::curve_pick_error(solidcraft_sketch::CurvePick::Missing, curves, &ss.name));
                    }
                    match solidcraft_sketch::profile_by_curves(&ss.profiles, curves) {
                        Ok(index) => indices.push(index),
                        Err(error) => {
                            // Twins a change made (#23291): the curves bound several profiles
                            // now, and the sides saved with the loop name the one picked.
                            if matches!(error, solidcraft_sketch::CurvePick::Ambiguous(_))
                                && let Some(index) = saved_sides(loops, sides, k).and_then(|sided| sided_twin(ss, sided))
                            {
                                if !indices.contains(&index) {
                                    indices.push(index);
                                }
                                continue;
                            }
                            let fallback = match (&error, notes.as_deref_mut()) {
                                (solidcraft_sketch::CurvePick::Missing, Some(notes)) => fallback_loop(ss, curves).map(|found| (found, notes)),
                                _ => None,
                            };
                            let Some(((found, note), notes)) = fallback else {
                                return Err(crate::curve_pick_error(error, curves, &ss.name));
                            };
                            notes.push(note);
                            // Two loops that fell back to one piece take it once.
                            for index in found {
                                if !indices.contains(&index) {
                                    indices.push(index);
                                }
                            }
                        }
                    }
                }
            }
            indices
        }
        ProfileSel::OffsetBoundaries { associations } => {
            if associations.is_empty()
                || associations.len() > MAX_SELECTIONS
                || associations.iter().collect::<BTreeSet<_>>().len() != associations.len()
            {
                return Err(DocError::Invalid("offset profile selection needs unique bounded association identities".into()));
            }
            associations.iter().map(|id| unique_profile(ss, &ss.sketch.offset_boundary(id)?)).collect::<Result<Vec<_>>>()?
        }
        ProfileSel::WholeBoundaries { profiles } => whole_profile_indices(ss, profiles)?,
        ProfileSel::TextRegions { links } => text_region_indices(ss, links)?,
        ProfileSel::SidedCurves { loops } => sided_indices(ss, loops, notes)?,
        ProfileSel::Points { points } => {
            let mut indices = Vec::with_capacity(points.len());
            for point in points {
                let (index, _) = ss
                    .profiles
                    .iter()
                    .enumerate()
                    .filter(|(_, profile)| profile.contains(*point))
                    .min_by(|(_, a), (_, b)| a.area.total_cmp(&b.area))
                    .ok_or_else(|| DocError::Unknown(format!("profile at {:?} in {}", [point.x, point.y], ss.name)))?;
                indices.push(index);
            }
            indices
        }
    };
    if indices.iter().any(|index| *index >= ss.profiles.len()) {
        return Err(DocError::Invalid(format!("out-of-range profile selection in {}", ss.name)));
    }
    Ok(indices)
}

/// The letters of the texts `links` among the profiles of `ss`, in profile order: each profile
/// whose outer loop is made of one of those texts' curves alone, less the letters' counters. A
/// profile whose outer loop mixes a picked text's curves with other curves (a line drawn across a
/// letter, or another picked text that new words run into) is refused: which of its pieces the
/// text meant cannot be told.
fn text_region_indices(ss: &SolvedSketch, links: &[String]) -> Result<Vec<usize>> {
    use solidcraft_sketch::LinkKind;
    if links.is_empty() || links.len() > MAX_SELECTIONS || links.iter().collect::<BTreeSet<_>>().len() != links.len() {
        return Err(DocError::Invalid("a text profile selection needs a bounded list of different texts".into()));
    }
    let sk = &ss.sketch;
    for id in links {
        if !sk.link(id).is_some_and(|link| link.kind == LinkKind::Text && !link.lost) {
            return Err(DocError::Unknown(format!("text {id} in {}", ss.name)));
        }
    }
    let wanted: BTreeSet<&str> = links.iter().map(String::as_str).collect();
    let link_of: std::collections::HashMap<&str, Option<&str>> = sk.curves.iter().map(|curve| (curve.id.as_str(), curve.link.as_deref())).collect();
    let owner = |id: &String| link_of.get(id.as_str()).copied().flatten().filter(|link| wanted.contains(link));
    let counters = solidcraft_sketch::text_counters(sk, &ss.profiles);
    let mut indices = Vec::new();
    for (index, profile) in ss.profiles.iter().enumerate() {
        let Some(text) = profile.outer_curves.iter().find_map(owner) else {
            continue;
        };
        if !profile.outer_curves.iter().all(|id| owner(id) == Some(text)) {
            return Err(DocError::Invalid(format!("other curves of {} now cut into a picked text's letters; pick its profiles again", ss.name)));
        }
        if !counters.get(index).copied().unwrap_or(false) {
            indices.push(index);
        }
    }
    if indices.is_empty() {
        return Err(DocError::Unknown(format!("letters of the picked text in {}", ss.name)));
    }
    Ok(indices)
}

/// The texts whose letters are exactly the profiles `checked`, when every one of them is a letter
/// of a single text and none of those texts' letters is left out; otherwise none, and the pick is
/// stored as before. A part of a text never grows into the whole text.
fn text_capture(ss: &SolvedSketch, checked: &[usize]) -> Option<ProfileSel> {
    let link_of: std::collections::HashMap<&str, Option<&str>> =
        ss.sketch.curves.iter().map(|curve| (curve.id.as_str(), curve.link.as_deref())).collect();
    let mut links = BTreeSet::new();
    for index in checked {
        let profile = ss.profiles.get(*index)?;
        let mut owners = profile.outer_curves.iter().map(|id| link_of.get(id.as_str()).copied().flatten());
        let owner = owners.next()??;
        if owners.any(|other| other != Some(owner)) {
            return None;
        }
        links.insert(owner.to_string());
    }
    let links: Vec<String> = links.into_iter().collect();
    let mut found = text_region_indices(ss, &links).ok()?;
    let mut picked = checked.to_vec();
    found.sort_unstable();
    picked.sort_unstable();
    (found == picked).then_some(ProfileSel::TextRegions { links })
}

/// How far, relative to its size, a piece of a profile's loop may lie from a curve's line or
/// circle and still be a piece of it: the pieces are made from the curves, so only rounding
/// separates them.
const SIDE_TOL: f64 = 1e-9;

/// The side of each curve of `profile`'s outer loop that the profile lies on. The outer loop
/// runs counter-clockwise, with the region on its left; each piece is matched to the one curve of
/// the loop whose line or circle it lies on, and the kernel says which side of that curve the
/// region is on (`planar::left_side_along_line`, `left_side_inside_circle`). `None` when a
/// curve is not a line, an arc or a circle, a piece lies on no curve of the loop or on several, or
/// the region lies on both sides of one curve.
fn curve_sides(sketch: &solidcraft_sketch::Sketch, profile: &solidcraft_sketch::Profile) -> Option<std::collections::BTreeMap<String, CurveSide>> {
    use solidcore_record::Seg2;
    use solidcraft_kernel::planar;
    use solidcraft_sketch::CurveKind;
    enum Carrier {
        Line([f64; 2], [f64; 2]),
        Circle([f64; 2], f64),
    }
    let ids = solidcraft_sketch::distinct_curves(&profile.outer_curves);
    if ids.is_empty() || ids.len() > 512 || profile.region.outer.segs.len() > 4096 {
        return None;
    }
    let at = |i: usize| sketch.points.get(i).map(|p| <[f64; 2]>::from(p.pos));
    let carriers = ids
        .iter()
        .map(|id| {
            if !is_analytic(sketch, id) {
                return None;
            }
            let curve = sketch.curves.iter().find(|curve| &curve.id == id)?;
            let carrier = match curve.kind {
                CurveKind::Line { a, b } => Carrier::Line(at(a)?, at(b)?),
                CurveKind::Circle { c, r } => Carrier::Circle(at(c)?, r),
                CurveKind::Arc { c, a, .. } => {
                    let center = at(c)?;
                    Carrier::Circle(center, planar::distance(center, at(a)?))
                }
                _ => return None,
            };
            Some((id, carrier))
        })
        .collect::<Option<Vec<_>>>()?;
    let mut sides = std::collections::BTreeMap::new();
    for seg in &profile.region.outer.segs {
        let mut on = carriers.iter().filter_map(|(id, carrier)| {
            let side = match (seg, carrier) {
                (Seg2::Line { a, b }, Carrier::Line(p, q)) => {
                    if planar::left_side_along_line((*a).into(), (*b).into(), *p, *q, SIDE_TOL)? {
                        CurveSide::Left
                    } else {
                        CurveSide::Right
                    }
                }
                (Seg2::Arc { center, radius, sweep, .. }, Carrier::Circle(c, r)) => {
                    if planar::left_side_inside_circle((*center).into(), *radius, *sweep, *c, *r, SIDE_TOL)? {
                        CurveSide::Inside
                    } else {
                        CurveSide::Outside
                    }
                }
                _ => return None,
            };
            Some((*id, side))
        });
        let (id, side) = on.next()?;
        if on.next().is_some() || *sides.entry(id.clone()).or_insert(side) != side {
            return None;
        }
    }
    (sides.len() == ids.len()).then_some(sides)
}

/// Twins picked by index (regions the curves around them can't tell apart) saved by the sides
/// of those curves each lies on, when every picked region has them and they name it alone.
fn sided_capture(ss: &SolvedSketch, picked: &[usize]) -> Option<ProfileSel> {
    if picked.is_empty() || picked.len() > MAX_SELECTIONS {
        return None;
    }
    let loops = picked
        .iter()
        .map(|index| {
            let sides = curve_sides(&ss.sketch, ss.profiles.get(*index)?)?;
            Some(sides.into_iter().map(|(curve, side)| SidedCurve { curve, side }).collect::<Vec<_>>())
        })
        .collect::<Option<Vec<_>>>()?;
    (sided_indices(ss, &loops, None).ok()? == picked).then_some(ProfileSel::SidedCurves { loops })
}

/// The side of each curve every picked profile lies on, one loop per profile, for a pick saved by
/// its curves ([`ProfileSel::Curves`]'s `sides`, #23291). A loop is left empty when `skip` says
/// so (its curves are offset roles, which their offset renames), when its profile has a free
/// curve, or when a piece of its outline lies on two curves; that loop is then saved by its
/// curves alone. Empty when no loop has sides.
pub fn picked_sides(ss: &SolvedSketch, picked: &[usize], skip: &[bool]) -> Vec<Vec<SidedCurve>> {
    if picked.is_empty() || picked.len() > MAX_SELECTIONS || skip.len() != picked.len() {
        return Vec::new();
    }
    let sides: Vec<Vec<SidedCurve>> = picked
        .iter()
        .zip(skip)
        .map(|(index, skip)| {
            let found = if *skip { None } else { ss.profiles.get(*index).and_then(|profile| curve_sides(&ss.sketch, profile)) };
            found.map(|sides| sides.into_iter().map(|(curve, side)| SidedCurve { curve, side }).collect()).unwrap_or_default()
        })
        .collect();
    if sides.iter().all(Vec::is_empty) { Vec::new() } else { sides }
}

/// The sides saved for loop `k` of a curve pick, when the pick saved one per loop and they name
/// exactly that loop's curves.
fn saved_sides<'a>(loops: &[Vec<String>], sides: &'a [Vec<SidedCurve>], k: usize) -> Option<&'a [SidedCurve]> {
    if sides.len() != loops.len() {
        return None;
    }
    let curves: BTreeSet<&str> = loops.get(k)?.iter().map(String::as_str).collect();
    sides
        .get(k)
        .map(Vec::as_slice)
        .filter(|sided| !sided.is_empty() && sided.iter().map(|side| side.curve.as_str()).collect::<BTreeSet<_>>() == curves)
}

/// The one profile that lies on the same sides of the same curves as `sided`; none, or several,
/// is no answer.
fn sided_twin(ss: &SolvedSketch, sided: &[SidedCurve]) -> Option<usize> {
    match sided_indices(ss, &[sided.to_vec()], None).ok()?.as_slice() {
        [index] => Some(*index),
        _ => None,
    }
}

fn sided_names(want: &std::collections::BTreeMap<&str, CurveSide>) -> String {
    let side = |side: &CurveSide| match side {
        CurveSide::Inside => "inside",
        CurveSide::Outside => "outside",
        CurveSide::Left => "left of",
        CurveSide::Right => "right of",
    };
    want.iter().map(|(curve, s)| format!("{} `{curve}`", side(s))).collect::<Vec<_>>().join(", ")
}

/// The profiles saved by the sides of their curves ([`ProfileSel::SidedCurves`]). Each saved loop
/// takes the one profile with the same curves on the same sides. When there is none (the
/// regions changed) and `notes` is given (evaluating a feature), the profile that shares the most
/// of the saved curves on the same sides, at least half of them, and lies on the other side of
/// none; of those, the one with the fewest curves the loop did not have. A tie is an error. Two
/// loops that land in one profile take it once.
fn sided_indices(ss: &SolvedSketch, loops: &[Vec<SidedCurve>], mut notes: Option<&mut Vec<String>>) -> Result<Vec<usize>> {
    use std::collections::BTreeMap;
    if loops.is_empty() || loops.len() > MAX_SELECTIONS || ss.profiles.len() > 10_000 {
        return Err(DocError::Invalid("a sided profile selection needs 1 to 128 profiles".into()));
    }
    let sides: Vec<Option<BTreeMap<String, CurveSide>>> = ss.profiles.iter().map(|profile| curve_sides(&ss.sketch, profile)).collect();
    let mut indices = Vec::with_capacity(loops.len());
    for stored in loops {
        let want: BTreeMap<&str, CurveSide> = stored.iter().map(|sided| (sided.curve.as_str(), sided.side)).collect();
        if want.is_empty() || want.len() != stored.len() || want.len() > 512 {
            return Err(DocError::Invalid("a sided profile lists no curve, or a curve twice".into()));
        }
        let same =
            |found: &BTreeMap<String, CurveSide>| found.len() == want.len() && found.iter().all(|(id, side)| want.get(id.as_str()) == Some(side));
        let exact: Vec<usize> = sides.iter().enumerate().filter(|(_, found)| found.as_ref().is_some_and(same)).map(|(index, _)| index).collect();
        let index = match exact.as_slice() {
            [index] => *index,
            [] => {
                let lost = || DocError::Unknown(format!("profile {} in {}", sided_names(&want), ss.name));
                let Some(notes) = notes.as_deref_mut() else { return Err(lost()) };
                let (index, shared) = sided_fallback(&sides, &want).ok_or_else(lost)?;
                notes.push(format!(
                    "no profile of {} lies {} any more; using the profile that lies the same way on {shared} of those {} curves",
                    ss.name,
                    sided_names(&want),
                    want.len()
                ));
                index
            }
            more => {
                return Err(DocError::Invalid(format!(
                    "{} profiles of {} lie {}; pick by point, or add a curve that tells them apart",
                    more.len(),
                    ss.name,
                    sided_names(&want)
                )));
            }
        };
        if !indices.contains(&index) {
            indices.push(index);
        }
    }
    Ok(indices)
}

/// [`sided_indices`]'s match after a change of the regions: the profile index and how many of the
/// saved curves it shares on the same sides.
fn sided_fallback(
    sides: &[Option<std::collections::BTreeMap<String, CurveSide>>],
    want: &std::collections::BTreeMap<&str, CurveSide>,
) -> Option<(usize, usize)> {
    // (index, curves shared on the same side, curves the saved loop did not have); none when the
    // profile lies on the other side of a saved curve.
    let scored: Vec<(usize, usize, usize)> = sides
        .iter()
        .enumerate()
        .filter_map(|(index, found)| {
            let found = found.as_ref()?;
            let mut shared = 0usize;
            for (id, side) in found {
                match want.get(id.as_str()) {
                    Some(saved) if saved != side => return None,
                    Some(_) => shared += 1,
                    None => {}
                }
            }
            Some((index, shared, found.len() - shared))
        })
        .collect();
    let best = scored.iter().map(|(_, shared, _)| *shared).max()?;
    if best == 0 || best.saturating_mul(2) < want.len() {
        return None;
    }
    let fewest = scored.iter().filter(|(_, shared, _)| *shared == best).map(|(_, _, new)| *new).min()?;
    let mut chosen = scored.iter().filter(|(_, shared, new)| *shared == best && *new == fewest);
    let (index, ..) = *chosen.next()?;
    if chosen.next().is_some() {
        return None;
    }
    Some((index, best))
}

/// Persist selected complete offset boundaries by association identity. Mixed selections
/// retain exact ordinary loops and the complete reserved role set of each offset boundary.
pub fn capture_profile_selection(ss: &SolvedSketch, indices: &[usize]) -> Result<ProfileSel> {
    let checked = profile_selection_indices(ss, &ProfileSel::Indices { indices: indices.to_vec() })?;
    if checked.is_empty() || checked.iter().collect::<BTreeSet<_>>().len() != checked.len() {
        return Ok(ProfileSel::Indices { indices: indices.to_vec() });
    }
    if let Some(texts) = text_capture(ss, &checked) {
        return Ok(texts);
    }
    // Capture adaptive free-form boundaries only with complete source provenance,
    // including every hole. Ordinary analytic captures keep their historical form.
    let adaptive = checked.iter().any(|index| {
        ss.profiles.get(*index).is_some_and(|profile| {
            std::iter::once(&profile.outer_curves)
                .chain(&profile.hole_curves)
                .any(|curves| curves.iter().collect::<BTreeSet<_>>().len() != curves.len())
        })
    });
    if adaptive {
        let identities = checked.iter().map(|index| ss.profiles.get(*index).and_then(whole_boundary)).collect::<Option<Vec<_>>>();
        if let Some(profiles) = identities {
            // Dynamic analytic offset groups need their own association identities.
            // Do not silently flatten a mixed selection into current active curve IDs.
            let mut dynamic = false;
            for profile in &profiles {
                for boundary in std::iter::once(&profile.outer).chain(&profile.holes) {
                    dynamic |= ss.sketch.offset_boundary_for_curves(boundary)?.is_some();
                }
            }
            if !dynamic && whole_profile_indices(ss, &profiles)? == checked {
                return Ok(ProfileSel::WholeBoundaries { profiles });
            }
        }
    }
    let mut associations = Vec::new();
    let mut loops = Vec::new();
    let mut offset_loops = Vec::new();
    let picked = checked.clone();
    for index in checked {
        let curves = &ss.profiles.get(index).ok_or_else(|| DocError::Invalid("profile index changed during capture".into()))?.outer_curves;
        if curves.is_empty() {
            return Ok(ProfileSel::Indices { indices: indices.to_vec() });
        }
        // Ordinary free-form loops retain one curve ID per exact span. That
        // historical multiset is valid profile identity, but is not a candidate
        // for the unique reserved-role set of a topology-aware association.
        let association =
            if curves.iter().collect::<BTreeSet<_>>().len() == curves.len() { ss.sketch.offset_boundary_for_curves(curves)? } else { None };
        if let Some(id) = association {
            unique_profile(ss, curves)?;
            let offset = ss
                .sketch
                .offsets
                .iter()
                .filter_map(|offset| offset.topology.as_ref())
                .find(|topology| topology.id == id)
                .ok_or_else(|| DocError::Unknown(format!("offset association {id}")))?;
            loops.push(offset.roles.iter().map(|role| role.output.clone()).collect());
            offset_loops.push(true);
            associations.push(id);
        } else {
            if ss.profiles.iter().filter(|profile| same_ids(curves, &profile.outer_curves)).count() != 1 {
                // Twins: the curves can't tell them apart, the sides of those curves may (#22548).
                return Ok(sided_capture(ss, &picked).unwrap_or_else(|| ProfileSel::Indices { indices: indices.to_vec() }));
            }
            loops.push(curves.clone());
            offset_loops.push(false);
        }
    }
    if associations.len() > MAX_SELECTIONS {
        return Err(DocError::Invalid("too many offset association profiles".into()));
    }
    if associations.len() == loops.len() {
        return Ok(ProfileSel::OffsetBoundaries { associations });
    }
    // Loops of offset associations keep their saved form alone: their roles are renamed by the
    // offset, so sides saved against today's names could name other curves later.
    let sides = picked_sides(ss, &picked, &offset_loops);
    Ok(ProfileSel::Curves { loops, sides })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;
    use serde_json::json;
    use solidcore_record::{Plane, Vec2};
    use solidcraft_sketch::{Sketch, find_profiles, solve};

    fn rounded_source(sketch: &mut Sketch, radius: f64) -> Vec<(String, bool)> {
        let endpoints = [
            Vec2::new(-37., -32. - radius),
            Vec2::new(37., -32. - radius),
            Vec2::new(37. + radius, -32.),
            Vec2::new(37. + radius, 32.),
            Vec2::new(37., 32. + radius),
            Vec2::new(-37., 32. + radius),
            Vec2::new(-37. - radius, 32.),
            Vec2::new(-37. - radius, -32.),
        ];
        let points: Vec<_> =
            endpoints.iter().enumerate().map(|(index, point)| sketch.add_point(*point, Some(&format!("source-p{index}"))).unwrap()).collect();
        let centers = [Vec2::new(37., -32.), Vec2::new(37., 32.), Vec2::new(-37., 32.), Vec2::new(-37., -32.)];
        for index in 0..8 {
            let id = format!("source-edge{index}");
            if index % 2 == 0 {
                sketch.add_line_pts(points[index], points[(index + 1) % 8], Some(&id)).unwrap();
            } else {
                let center = centers[index / 2];
                sketch
                    .add_arc(
                        center,
                        endpoints[index],
                        endpoints[(index + 1) % 8],
                        [None, Some(points[index]), Some(points[(index + 1) % 8])],
                        Some(&id),
                    )
                    .unwrap();
            }
        }
        for point in &mut sketch.points {
            point.fixed = true;
        }
        (0..8).map(|index| (format!("source-edge{index}"), true)).collect()
    }

    fn set_source_radius(sketch: &mut Sketch, radius: f64) {
        let points = [
            Vec2::new(-37., -32. - radius),
            Vec2::new(37., -32. - radius),
            Vec2::new(37. + radius, -32.),
            Vec2::new(37. + radius, 32.),
            Vec2::new(37., 32. + radius),
            Vec2::new(-37., 32. + radius),
            Vec2::new(-37. - radius, 32.),
            Vec2::new(-37. - radius, -32.),
        ];
        for (index, value) in points.into_iter().enumerate() {
            sketch.points.iter_mut().find(|point| point.id == format!("source-p{index}")).unwrap().pos = value;
        }
    }

    #[test]
    fn captured_sharp_offset_profile_survives_arc_resurrection_and_reopen() {
        let mut sketch = Sketch::new();
        let sources = rounded_source(&mut sketch, 1.);
        let created = sketch.add_convex_offset(sources, 1.4).unwrap();
        assert_eq!(created.curves.len(), 4);
        let mut document = crate::Document::new("selected logical inner boundary");
        let feature =
            document.add_feature(crate::FeatureKind::Sketch { plane: crate::PlaneRef::Origin { name: "XY".into() }, sketch }, None).unwrap();
        let mut model = crate::Model::new();
        model.evaluate(&document);
        let state = model.state();
        let solved = state.sketch(feature).unwrap();
        let index = unique_profile(solved, &created.curves).unwrap();
        let selection = capture_profile_selection(solved, &[index]).unwrap();
        assert_eq!(selection, ProfileSel::OffsetBoundaries { associations: vec![created.association.clone()] });
        let original_roles = solved.sketch.offsets[0].topology.as_ref().unwrap().roles.iter().map(|role| role.output.clone()).collect::<Vec<_>>();
        let sharp_area = crate::select_profiles(solved, &selection).unwrap()[0].area();
        assert!((sharp_area - 73.2 * 63.2).abs() < 1e-7, "{sharp_area}");
        document
            .add_feature(
                crate::FeatureKind::Extrude {
                    thin: None,
                    sketch: crate::SketchId(feature),
                    profiles: selection.clone(),
                    extent: crate::Extent {
                        convention: crate::ExtrudeConvention::LegacyPerSide,
                        distance: "2".into(),
                        direction: crate::Direction::Positive,
                        distance2: None,
                        start_offset: None,
                        symmetric_offset: false,
                        through_all: false,
                        second_follows_first: false,
                        taper: None,
                        taper2: None,
                        corner: Default::default(),
                        to: None,
                        to_offset: None,
                        from: None,
                        to_body_apart: false,
                        through_all2: false,
                        to2: None,
                        to_offset2: None,
                        from_level: false,
                        picked_placements: Vec::new(),
                        one_sided_direction: None,
                        to_ref: None,
                        from_ref: None,
                        extend_to_face: false,
                    },
                    operation: crate::Operation::NewBody,
                    targets: vec![],
                },
                None,
            )
            .unwrap();
        model.evaluate(&document);
        let volume = solidcraft_kernel::measure(&model.state().bodies[0].body).unwrap().volume;
        assert!((volume - 2. * 73.2 * 63.2).abs() < 1e-6, "{volume}");
        let reopened = crate::Document::from_json(&document.to_json()).unwrap();
        assert_eq!(reopened, document);
        let mut cold = crate::Model::new();
        cold.evaluate(&reopened);
        assert!((solidcraft_kernel::measure(&cold.state().bodies[0].body).unwrap().volume - volume).abs() < 1e-6);
        let crate::FeatureKind::Sketch { sketch, .. } = &mut document.features[0].kind else { panic!("sketch") };
        set_source_radius(sketch, 2.);
        model.evaluate(&document);
        assert!(model.results.iter().all(|result| result.error.is_none()), "{:?}", model.results);
        let state = model.state();
        let solved = state.sketch(feature).unwrap();
        let active = solved.sketch.offset_boundary(&created.association).unwrap();
        assert_eq!(active, original_roles);
        let rounded_area = 75.2 * 65.2 - (4. - std::f64::consts::PI) * 0.6 * 0.6;
        assert!((crate::select_profiles(solved, &selection).unwrap()[0].area() - rounded_area).abs() < 1e-7);
        let volume = solidcraft_kernel::measure(&model.state().bodies[0].body).unwrap().volume;
        // Kernel volume integrates the curved display mesh; the analytic region above
        // checks the exact rounded geometry independently of that tessellation.
        assert!((volume - 2. * rounded_area).abs() < 0.002, "{volume}");
        assert!(
            crate::select_profiles(solved, &ProfileSel::Curves { loops: vec![created.curves.clone()], sides: Vec::new() }).is_err(),
            "partial old active set must not expand after resurrection"
        );
        assert_eq!(
            profile_selection_indices(solved, &ProfileSel::Curves { loops: vec![original_roles], sides: Vec::new() }).unwrap(),
            profile_selection_indices(solved, &selection).unwrap()
        );
        let reopened = crate::Document::from_json(&document.to_json()).unwrap();
        cold.evaluate(&reopened);
        assert!((solidcraft_kernel::measure(&cold.state().bodies[0].body).unwrap().volume - volume).abs() < 1e-5);
    }

    #[test]
    fn ordinary_profile_selection_keeps_legacy_large_and_duplicate_semantics() {
        let mut sketch = Sketch::new();
        sketch.add_circle(Vec2::new(0., 0.), 3., None, Some("circle")).unwrap();
        let report = solve(&mut sketch);
        let profiles = find_profiles(&sketch);
        let ss = SolvedSketch {
            feature: 1,
            name: "ordinary circle".into(),
            plane: Plane::XY,
            sketch,
            report,
            profiles,
            profiles_3d: Vec::new(),
            ends: Default::default(),
        };
        assert_eq!(ss.profiles.len(), 1);
        let indices = vec![0; 600];
        let selection = ProfileSel::Indices { indices: indices.clone() };
        assert_eq!(profile_selection_indices(&ss, &selection).unwrap(), indices);
        assert_eq!(capture_profile_selection(&ss, &indices).unwrap(), selection);
        let points = ProfileSel::Points { points: vec![Vec2::new(0., 0.); 600] };
        assert_eq!(profile_selection_indices(&ss, &points).unwrap(), indices);
        let curves = ProfileSel::Curves { loops: vec![vec!["circle".into()]; 600], sides: Vec::new() };
        let regions = crate::select_profiles(&ss, &curves).unwrap();
        assert_eq!(regions.len(), 600);
        assert!(regions.iter().all(|region| (region.area() - std::f64::consts::PI * 9.).abs() < 1e-9));
        assert!(profile_selection_indices(&ss, &ProfileSel::Indices { indices: vec![1] }).is_err());
    }

    /// An SVG-traced sketch can have hundreds of spline-bounded holes. The total curve
    /// budget still bounds the validator, and drawing fills keep their separate limit.
    #[test]
    fn whole_source_accepts_many_spline_holes_within_curve_budget() {
        let profile = |count: usize, edges: usize| WholeProfileBoundary {
            outer: vec!["outer".to_string()],
            holes: (0..count).map(|i| (0..edges).map(|edge| format!("h{i:04}-{edge:03}")).collect()).collect(),
        };
        assert_eq!(WHOLE_MAX_HOLES, 128, "the drawing-fill hole cap stays unchanged");
        for count in [128, 129, 707, MAX_MODEL_WHOLE_HOLES] {
            let large = profile(count, 4);
            validate_whole(std::slice::from_ref(&large)).unwrap_or_else(|e| panic!("{count} holes should be accepted: {e}"));
            let duplicate = validate_whole(&[large.clone(), large]).unwrap_err().to_string();
            assert!(duplicate.contains("duplicate identity"), "{duplicate}");
        }
        let exceeded = validate_whole(&[profile(MAX_MODEL_WHOLE_HOLES + 1, 4)]).unwrap_err().to_string();
        assert!(exceeded.contains("1024 holes"), "{exceeded}");
        assert!(validate_whole(&[profile(MAX_MODEL_WHOLE_HOLES, 20)]).is_ok(), "20 curves per hole is far under the curve budget");
        let edges = MAX_WHOLE_CURVES / MAX_MODEL_WHOLE_HOLES + 1;
        let too_many_curves = validate_whole(&[profile(MAX_MODEL_WHOLE_HOLES, edges)]).unwrap_err().to_string();
        assert_eq!(too_many_curves, DocError::Invalid(WHOLE_PROFILE_TOO_MANY_CURVES.into()).to_string());
    }

    #[test]
    fn a_whole_source_selection_loads_in_any_curve_order() {
        let ss = solved_square(&[]);
        assert_eq!(ss.profiles.len(), 1);
        let canonical = ProfileSel::WholeBoundaries {
            profiles: vec![WholeProfileBoundary { outer: ["a", "b", "c", "d"].map(str::to_string).to_vec(), holes: Vec::new() }],
        };
        assert_eq!(profile_selection_indices(&ss, &canonical).unwrap(), vec![0]);
        // The same loop written from another corner, or the other way round, names the same
        // region: a design saved that way opens instead of being refused as damaged (#6499).
        for order in [["b", "c", "d", "a"], ["d", "c", "b", "a"], ["c", "d", "a", "b"]] {
            let written = json!({"type":"whole_boundaries","profiles":[{"outer":order,"holes":[]}]});
            let loaded: ProfileSel = serde_json::from_value(written).unwrap();
            assert_eq!(loaded, canonical, "the loaded identity is canonical");
            assert_eq!(profile_selection_indices(&ss, &loaded).unwrap(), vec![0]);
            assert_eq!(crate::select_profiles(&ss, &loaded).unwrap().len(), 1);
        }
    }

    #[test]
    fn a_whole_source_profile_with_hundreds_of_curves_can_be_picked() {
        // The outline of a line of script text, or its offset, has hundreds of curves around one
        // profile. Picking it used to fail with "whole-source boundary exceeds its identity
        // budget" past 512 curves.
        let mut sketch = Sketch::new();
        let sides = 600;
        let points: Vec<_> = (0..sides)
            .map(|index| {
                let angle = std::f64::consts::TAU * index as f64 / sides as f64;
                sketch.add_point(Vec2::new(50. * angle.cos(), 50. * angle.sin()), None).unwrap()
            })
            .collect();
        for index in 0..sides {
            sketch.add_line_pts(points[index], points[(index + 1) % sides], Some(&format!("side{index:03}"))).unwrap();
        }
        let report = solve(&mut sketch);
        let profiles = find_profiles(&sketch);
        let ss = SolvedSketch {
            feature: 1,
            name: "S".into(),
            plane: Plane::XY,
            sketch,
            report,
            profiles,
            profiles_3d: Vec::new(),
            ends: Default::default(),
        };
        assert_eq!(ss.profiles.len(), 1);
        let (outer, holes) = ss.profiles[0].whole_curve_boundaries().unwrap();
        assert_eq!(outer.len(), sides);
        let picked = ProfileSel::WholeBoundaries { profiles: vec![WholeProfileBoundary { outer, holes }] };
        assert_eq!(profile_selection_indices(&ss, &picked).unwrap(), vec![0]);
        // Past what a sketch may hold, the refusal says what to do in words the interface translates.
        let huge = (0..=MAX_WHOLE_LOOP_CURVES).map(|index| format!("c{index:06}")).collect::<Vec<_>>();
        let refused = ProfileSel::WholeBoundaries { profiles: vec![WholeProfileBoundary { outer: huge, holes: Vec::new() }] };
        assert_eq!(profile_selection_indices(&ss, &refused).unwrap_err().to_string(), WHOLE_PROFILE_TOO_MANY_CURVES);
    }

    fn solved_square(extra: &[(&str, [f64; 4])]) -> SolvedSketch {
        let mut sketch = Sketch::new();
        let corners = [Vec2::new(0., 0.), Vec2::new(10., 0.), Vec2::new(10., 10.), Vec2::new(0., 10.)];
        let points: Vec<_> = corners.iter().map(|corner| sketch.add_point(*corner, None).unwrap()).collect();
        for (index, id) in ["a", "b", "c", "d"].iter().enumerate() {
            sketch.add_line_pts(points[index], points[(index + 1) % 4], Some(id)).unwrap();
        }
        for (id, [x0, y0, x1, y1]) in extra {
            let p = sketch.add_point(Vec2::new(*x0, *y0), None).unwrap();
            let q = sketch.add_point(Vec2::new(*x1, *y1), None).unwrap();
            sketch.add_line_pts(p, q, Some(id)).unwrap();
        }
        let report = solve(&mut sketch);
        let profiles = find_profiles(&sketch);
        SolvedSketch { feature: 1, name: "S".into(), plane: Plane::XY, sketch, report, profiles, profiles_3d: Vec::new(), ends: Default::default() }
    }

    #[test]
    fn a_split_loop_falls_back_to_its_pieces_only_when_evaluating() {
        let loops = ProfileSel::Curves { loops: vec![vec!["a".into(), "b".into(), "c".into(), "d".into()]], sides: Vec::new() };
        let split = solved_square(&[("cut", [4., 0., 4., 10.])]);
        assert_eq!(split.profiles.len(), 2);
        assert!(profile_selection_indices(&split, &loops).is_err(), "a command's pick stays exact");
        let (indices, notes) = profile_selection_indices_lenient(&split, &loops).unwrap();
        assert_eq!(indices.len(), 2);
        assert!(notes[0].contains("cut by another curve now"), "{notes:?}");
        assert_eq!(profile_fallback_notes(&split, &loops), notes);
        let area: f64 = crate::select_profiles(&split, &loops).unwrap().iter().map(|region| region.area()).sum();
        assert!((area - 100.).abs() < 1e-9, "{area}");
        // Exact matches have no note; a loop sharing under half its curves matches nothing.
        let whole = solved_square(&[]);
        assert!(profile_fallback_notes(&whole, &loops).is_empty());
        let few = ProfileSel::Curves { loops: vec![vec!["a".into(), "x".into(), "y".into(), "z".into(), "w".into()]], sides: Vec::new() };
        assert!(crate::select_profiles(&whole, &few).is_err());
    }

    #[test]
    fn periodic_offset_ring_whole_source_survives_adaptive_spans_and_reopen() {
        let mut sketch = Sketch::new();
        let major = sketch.add_point(Vec2::new(10., 0.), None).unwrap();
        let ellipse = sketch.add_curve(solidcraft_sketch::CurveKind::Ellipse { c: 0, m: major, r: 5. }, None).unwrap();
        let first = sketch.add_freeform_offset(ellipse, true, -1.).unwrap();
        sketch.add_freeform_offset(first, true, -0.5).unwrap();
        let mut document = crate::Document::new("periodic offset ring multiset");
        let feature =
            document.add_feature(crate::FeatureKind::Sketch { plane: crate::PlaneRef::Origin { name: "XY".into() }, sketch }, None).unwrap();
        let mut model = crate::Model::new();
        model.evaluate(&document);
        let state = model.state();
        let solved = state.sketch(feature).unwrap();
        assert_eq!(solved.profiles.len(), 3);
        let (index, profile) = solved.profiles.iter().enumerate().min_by(|(_, a), (_, b)| a.area.total_cmp(&b.area)).unwrap();
        assert!(profile.outer_curves.len() > 1);
        assert_eq!(profile.outer_curves.iter().collect::<BTreeSet<_>>().len(), 1);
        assert!(solved.sketch.offset_boundary_for_curves(&profile.outer_curves).is_err(), "strict association identity still refuses repeated roles");
        let selection = capture_profile_selection(solved, &[index]).unwrap();
        assert!(matches!(&selection, ProfileSel::WholeBoundaries { profiles } if profiles.len() == 1 && profiles[0].holes.len() == 1));
        let legacy_spans = ProfileSel::Curves { loops: vec![profile.outer_curves.clone()], sides: Vec::new() };
        assert_eq!(profile_selection_indices(solved, &legacy_spans).unwrap(), vec![index]);
        let legacy_roundtrip: ProfileSel = serde_json::from_str(&serde_json::to_string(&legacy_spans).unwrap()).unwrap();
        assert_eq!(legacy_roundtrip, legacy_spans, "ordinary legacy span multiplicities remain serialized exactly");
        assert_eq!(profile_selection_indices(solved, &selection).unwrap(), vec![index]);
        let selected = crate::select_profiles(solved, &selection).unwrap();
        assert_eq!(selected.len(), 1);
        assert!((selected[0].area() - profile.area).abs() < 1e-10);
        assert!(selected[0].contains(Vec2::new(11.25, 0.)));
        assert!(!selected[0].contains(Vec2::new(10.5, 0.)));
        assert!(!selected[0].contains(Vec2::new(11.75, 0.)));
        let saved = serde_json::to_string(&selection).unwrap();
        let reopened: ProfileSel = serde_json::from_str(&saved).unwrap();
        let reopened_doc = crate::Document::from_json(&document.to_json()).unwrap();
        let mut cold = crate::Model::new();
        cold.evaluate(&reopened_doc);
        let cold_state = cold.state();
        let regions = crate::select_profiles(cold_state.sketch(feature).unwrap(), &reopened).unwrap();
        assert_eq!(regions, selected);

        // An identical complete identity in two profiles is ambiguous, even when
        // their boundary geometry and graph coverage certificates are identical.
        let mut ambiguous = cold_state.sketch(feature).unwrap().clone();
        ambiguous.profiles.push(ambiguous.profiles[index].clone());
        assert!(profile_selection_indices(&ambiguous, &reopened).is_err());

        document
            .add_feature(
                crate::FeatureKind::Extrude {
                    thin: None,
                    sketch: crate::SketchId(feature),
                    profiles: selection.clone(),
                    extent: crate::Extent {
                        convention: crate::ExtrudeConvention::LegacyPerSide,
                        distance: "2".into(),
                        direction: crate::Direction::Positive,
                        distance2: None,
                        start_offset: None,
                        symmetric_offset: false,
                        through_all: false,
                        second_follows_first: false,
                        taper: None,
                        taper2: None,
                        corner: Default::default(),
                        to: None,
                        to_offset: None,
                        from: None,
                        to_body_apart: false,
                        through_all2: false,
                        to2: None,
                        to_offset2: None,
                        from_level: false,
                        picked_placements: Vec::new(),
                        one_sided_direction: None,
                        to_ref: None,
                        from_ref: None,
                        extend_to_face: false,
                    },
                    operation: crate::Operation::NewBody,
                    targets: vec![],
                },
                None,
            )
            .unwrap();
        model.evaluate(&document);
        let initial_volume = solidcraft_kernel::measure(&model.state().bodies[0].body).unwrap().volume;
        // Kernel measurement integrates the curved mesh. Scale the established
        // periodic-ring tolerance (0.05 mm³ at height3) to this height2 extrusion.
        // Exact profile identity and cold/warm equality below remain strict.
        let volume_tolerance = 0.05 * 2. / 3.;
        assert!(
            (initial_volume - 2. * profile.area).abs() < volume_tolerance,
            "initial kernel volume {initial_volume}, analytic area {}",
            profile.area
        );
        let crate::FeatureKind::Sketch { sketch, .. } = &mut document.features[0].kind else { panic!("sketch") };
        sketch.points[major].pos.x = 12.;
        model.evaluate(&document);
        assert!(model.results.iter().all(|result| result.error.is_none()), "{:?}", model.results);
        let updated_state = model.state();
        let updated = updated_state.sketch(feature).unwrap();
        assert!(profile_selection_indices(updated, &legacy_spans).is_err(), "legacy tessellation-span multiplicity intentionally remains exact");
        let updated_index = profile_selection_indices(updated, &selection).unwrap()[0];
        let updated_region = &updated.profiles[updated_index].region;
        // Probe away from exact spline-span seams, using the independent analytic
        // ellipse point and unit outward gradient rather than a stored profile sample.
        let parameter = 0.2_f64;
        let source_point = Vec2::new(12. * parameter.cos(), 5. * parameter.sin());
        let normal = Vec2::new(parameter.cos() / 12., parameter.sin() / 5.).normalized().unwrap();
        let ring_point = source_point + normal * 1.25;
        assert!(updated_region.contains(ring_point));
        assert!(!updated_region.contains(source_point + normal * 0.5));
        assert!(!updated_region.contains(source_point + normal * 1.75));
        let updated_volume = solidcraft_kernel::measure(&updated_state.bodies[0].body).unwrap().volume;
        assert!(updated_volume > initial_volume);
        assert!(
            (updated_volume - 2. * updated_region.area()).abs() < volume_tolerance,
            "updated kernel volume {updated_volume}, analytic area {}",
            updated_region.area()
        );
        let reopened_document = crate::Document::from_json(&document.to_json()).unwrap();
        cold.evaluate(&reopened_document);
        assert!(cold.results.iter().all(|result| result.error.is_none()));
        assert!((solidcraft_kernel::measure(&cold.state().bodies[0].body).unwrap().volume - updated_volume).abs() < 1e-6);

        // A new hole is a semantic boundary change, not the old ring. Whole-source
        // identities include holes and must not silently absorb the new material cutout.
        let crate::FeatureKind::Sketch { sketch, .. } = &mut document.features[0].kind else { panic!("sketch") };
        sketch.add_circle(ring_point, 0.1, None, Some("new-hole")).unwrap();
        model.evaluate(&document);
        let changed = model.state();
        assert!(profile_selection_indices(changed.sketch(feature).unwrap(), &selection).is_err());
        assert!(model.results.last().unwrap().error.is_some());
    }
}

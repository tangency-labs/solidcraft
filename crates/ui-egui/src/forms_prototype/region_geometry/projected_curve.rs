//! Bounded region predicates for positively weighted, projected rational Bezier curves.
//! Exact curve witnesses and certified hull bounds share the selection work budget.

use crate::selection::BoxSel;
use solidcore_record::Vec3;
use solidcraft_engine::geom::bspline::HPoint;

const MAX_CONTROLS: usize = 33;
const MAX_DEPTH: usize = 24;
const UNCERTAIN_SIZE: f64 = 0.25;

/// Project an exact world-space homogeneous Bezier restriction. A curve wholly
/// behind the near plane contributes nothing. Mixed coverage is refused until
/// the adapter can clip its exact parameter interval.
pub(super) fn project_controls(
    proj: &crate::viewport::Proj,
    controls: &[solidcraft_engine::geom::bspline::HPoint],
) -> Result<Option<Vec<[f64; 3]>>, &'static str> {
    if controls.len() < 2
        || controls.len() > MAX_CONTROLS
        || controls.iter().any(|p| !p.is_finite() || p.w < f64::MIN_POSITIVE)
        || !proj.rect.is_finite()
        || proj.rect.width() <= 0.0
        || proj.rect.height() <= 0.0
    {
        return Err("invalid Form curve projection input");
    }
    let mut result = Vec::with_capacity(controls.len());
    let mut front = 0;
    for p in controls {
        let world = [p.x, p.y, p.z, p.w];
        let clip: [f64; 4] = std::array::from_fn(|row| (0..4).map(|column| proj.vp.0[column][row] * world[column]).sum());
        if clip.iter().any(|v| !v.is_finite()) {
            return Err("Form curve projection overflowed");
        }
        front += usize::from(clip[3] > 1e-9 * p.w);
        result.push([
            f64::from(proj.rect.left()) * clip[3] + (clip[0] + clip[3]) * f64::from(proj.rect.width()) * 0.5,
            f64::from(proj.rect.top()) * clip[3] + (clip[3] - clip[1]) * f64::from(proj.rect.height()) * 0.5,
            clip[3],
        ]);
    }
    if front == 0 {
        return Ok(None);
    }
    if front != controls.len() {
        return Err("the Form selection crosses the camera near plane; rotate or zoom out");
    }
    if result.iter().flatten().any(|v| !v.is_finite()) {
        return Err("Form curve projection overflowed");
    }
    Ok(Some(result))
}

/// Controls are `(screen_x * w, screen_y * w, w)` after homogeneous projection.
/// The caller must clip/certify near-plane coverage before calling this predicate.
/// A hull certifies containment or disjointness; it never supplies a positive crossing pick.
/// `remaining` is shared by every curve in one region query.
pub(super) fn curve_takes(region: &BoxSel, controls: &[[f64; 3]], remaining: &mut usize) -> Result<bool, &'static str> {
    if controls.len() < 2 || controls.len() > MAX_CONTROLS || controls.iter().any(|p| p.iter().any(|v| !v.is_finite()) || p[2] < f64::MIN_POSITIVE) {
        return Err("invalid projected Form curve");
    }
    if !region.rect.is_finite()
        || region.rect.width() < 0.0
        || region.rect.height() < 0.0
        || (region.is_lasso() && (region.lasso.len() < 3 || region.lasso.len() > 4096 || region.lasso.iter().any(|p| !p.is_finite())))
    {
        return Err("invalid Form selection region");
    }
    classify(region, controls, remaining, 0)
}

fn classify(region: &BoxSel, controls: &[[f64; 3]], remaining: &mut usize, depth: usize) -> Result<bool, &'static str> {
    // Charge the de Casteljau triangle and up to four complete polygon scans
    // (three curve witnesses and the hull certificate), not just curve nodes.
    let work = controls.len() * (controls.len() + 1) / 2 + region.lasso.len() * 4;
    *remaining = remaining.checked_sub(work).ok_or("Form region selection exceeded its curve budget")?;
    let points: Vec<_> = controls.iter().map(|p| [p[0] / p[2], p[1] / p[2]]).collect();
    if points.iter().flatten().any(|v| !v.is_finite()) {
        return Err("Form curve projection overflowed");
    }
    let bounds = points.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |mut b, p| {
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
        b
    });
    let rect = [f64::from(region.rect.min.x), f64::from(region.rect.min.y), f64::from(region.rect.max.x), f64::from(region.rect.max.y)];
    if bounds[2] < rect[0] || bounds[3] < rect[1] || bounds[0] > rect[2] || bounds[1] > rect[3] {
        return Ok(false);
    }
    let crossing = region.crossing && !region.is_lasso();
    let (left, right) = split(controls);
    let witnesses = [points.first().copied(), points.last().copied(), left.last().map(|p| [p[0] / p[2], p[1] / p[2]])];
    for point in witnesses.into_iter().flatten() {
        if point.iter().any(|v| !v.is_finite()) {
            return Err("Form curve projection overflowed");
        }
        let inside = contains(region, point);
        if crossing && inside {
            return Ok(true);
        }
        if !crossing && !inside {
            return Ok(false);
        }
    }
    let hull_inside = if region.is_lasso() {
        // This rectangle contains the complete curve. If no polygon boundary
        // touches it, the even-odd membership of any witness holds throughout it.
        region
            .lasso
            .iter()
            .zip(region.lasso.iter().cycle().skip(1))
            .take(region.lasso.len())
            .all(|(a, b)| !segment_touches_box([f64::from(a.x), f64::from(a.y)], [f64::from(b.x), f64::from(b.y)], bounds))
    } else {
        bounds[0] >= rect[0] && bounds[1] >= rect[1] && bounds[2] <= rect[2] && bounds[3] <= rect[3]
    };
    if hull_inside {
        return Ok(true);
    }
    if depth == MAX_DEPTH || (bounds[2] - bounds[0]).max(bounds[3] - bounds[1]) <= UNCERTAIN_SIZE {
        return Err("a Form curve lies too close to the selection boundary; move the boundary slightly");
    }
    let a = classify(region, &left, remaining, depth + 1);
    if a == Ok(crossing) {
        return a;
    }
    let b = classify(region, &right, remaining, depth + 1);
    if b == Ok(crossing) {
        return b;
    }
    a.and(b)
}

fn contains(region: &BoxSel, p: [f64; 2]) -> bool {
    if p.iter().any(|v| !v.is_finite()) {
        return false;
    }
    let r = region.rect;
    if p[0] < f64::from(r.min.x) || p[1] < f64::from(r.min.y) || p[0] > f64::from(r.max.x) || p[1] > f64::from(r.max.y) {
        return false;
    }
    if !region.is_lasso() {
        return true;
    }
    let mut inside = false;
    for (a, b) in region.lasso.iter().zip(region.lasso.iter().cycle().skip(1)).take(region.lasso.len()) {
        let (a, b) = ([f64::from(a.x), f64::from(a.y)], [f64::from(b.x), f64::from(b.y)]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
    }
    inside
}

fn segment_touches_box(a: [f64; 2], b: [f64; 2], r: [f64; 4]) -> bool {
    let d = [b[0] - a[0], b[1] - a[1]];
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    for axis in 0..2 {
        if d[axis] == 0.0 {
            if a[axis] < r[axis] || a[axis] > r[axis + 2] {
                return false;
            }
        } else {
            let x = (r[axis] - a[axis]) / d[axis];
            let y = (r[axis + 2] - a[axis]) / d[axis];
            lo = lo.max(x.min(y));
            hi = hi.min(x.max(y));
            if lo > hi {
                return false;
            }
        }
    }
    true
}

fn split(controls: &[[f64; 3]]) -> (Vec<[f64; 3]>, Vec<[f64; 3]>) {
    let mut row = controls.to_vec();
    let (mut left, mut right) = (Vec::with_capacity(row.len()), Vec::with_capacity(row.len()));
    while let (Some(a), Some(b)) = (row.first().copied(), row.last().copied()) {
        left.push(a);
        right.push(b);
        row = row
            .windows(2)
            .filter_map(|w| match w {
                [a, b] => Some(std::array::from_fn(|axis| a[axis] * 0.5 + b[axis] * 0.5)),
                _ => None,
            })
            .collect();
    }
    right.reverse();
    (left, right)
}

/// Existential visibility in the region, independent of the drag direction.
/// `visible` tests one exact world point. `occluded_hull` may return true only
/// when the complete convex hull of these positive homogeneous controls is hidden;
/// false means no proof, never visibility. Both callbacks charge this same budget.
#[cfg(test)]
pub(super) fn curve_has_visible<V, O>(
    proj: &crate::viewport::Proj,
    region: &BoxSel,
    world: &[solidcraft_engine::geom::bspline::HPoint],
    remaining: &mut usize,
    visible: &mut V,
    occluded_hull: &mut O,
) -> Result<bool, String>
where
    V: FnMut(Vec3, &mut usize) -> Result<bool, String>,
    O: FnMut(&[solidcraft_engine::geom::bspline::HPoint], &mut usize) -> Result<bool, String>,
{
    curves_have_visible(proj, region, std::iter::once(world), remaining, visible, occluded_hull)
}
/// Fair existential traversal across every exact restriction of one logical
/// edge. An unresolved early piece must not starve another visible restriction.
/// Frontier storage is capped separately from work; discarded or unresolved
/// branches preserve an error, never a negative visibility answer.
pub(super) fn curves_have_visible<'a, V, O>(
    proj: &crate::viewport::Proj,
    region: &BoxSel,
    curves: impl IntoIterator<Item = &'a [solidcraft_engine::geom::bspline::HPoint]>,
    remaining: &mut usize,
    visible: &mut V,
    occluded_hull: &mut O,
) -> Result<bool, String>
where
    V: FnMut(Vec3, &mut usize) -> Result<bool, String>,
    O: FnMut(&[solidcraft_engine::geom::bspline::HPoint], &mut usize) -> Result<bool, String>,
{
    const MAX_FRONTIER_CONTROLS: usize = 4096;
    validate_visibility_region(region, remaining)?;
    let mut pending = std::collections::VecDeque::<VisibilityCursor>::new();
    let mut controls = 0usize;
    // Input restrictions stay borrowed. Each active restriction follows a DFS
    // path with exact sibling nets, rotating between restrictions after each
    // node. Periodic oldest-sibling turns also prevent a deep unresolved left
    // path from starving a right-side witness.
    let mut roots = curves.into_iter();
    // Incident fitted charts may supply the same exact restriction. Keep
    // a bounded borrowed memo per logical edge: one retained root already
    // owns every certificate/witness obligation for a bit-identical copy.
    let mut recent = [None::<&[HPoint]>; 8];
    let mut roots_done = false;
    let mut root_turn = true;
    let mut saw_root = false;
    let mut uncertain = None;
    loop {
        let root = if !roots_done && (root_turn || pending.is_empty()) {
            match roots.next() {
                Some(world) => Some(world),
                None => {
                    roots_done = true;
                    None
                }
            }
        } else {
            None
        };
        let (result, depth, mut cursor) = if let Some(world) = root {
            saw_root = true;
            root_turn = false;
            if world.len() < 2 || world.len() > MAX_CONTROLS {
                return Err("invalid exact Form visibility curve size".into());
            }
            visibility_spend(remaining, world.len())?;
            if world.iter().any(|point| !point.is_finite() || point.w < f64::MIN_POSITIVE) {
                return Err("Form visibility requires finite positive rational controls".into());
            }
            let mut repeated = false;
            for previous in recent.iter().flatten().filter(|previous| previous.len() == world.len()) {
                visibility_spend(remaining, world.len())?;
                if world.iter().zip(previous.iter()).all(|(a, b)| control_bits(a) == control_bits(b)) {
                    repeated = true;
                    break;
                }
            }
            if repeated {
                continue;
            }
            recent.rotate_right(1);
            if let Some(slot) = recent.first_mut() {
                *slot = Some(world);
            }
            (visible_curve_node(proj, region, world, remaining, visible, occluded_hull, 0), 0, VisibilityCursor::default())
        } else if let Some(mut cursor) = pending.pop_front() {
            cursor.turn = (cursor.turn + 1) % 8;
            let branch = if cursor.turn == 0 { cursor.stack.pop_front() } else { cursor.stack.pop_back() };
            let (world, depth) = branch.ok_or("missing exact Form visibility sibling")?;
            controls = controls.checked_sub(world.len()).ok_or("invalid Form visibility frontier accounting")?;
            root_turn = true;
            (visible_curve_node(proj, region, &world, remaining, visible, occluded_hull, depth), depth, cursor)
        } else if roots_done {
            break;
        } else {
            root_turn = true;
            continue;
        };
        match result {
            Ok(VisibilityNode::Visible) => return Ok(true),
            Ok(VisibilityNode::Hidden) => {}
            Ok(VisibilityNode::Split([left, right])) => {
                let count = left.len().checked_add(right.len()).and_then(|n| controls.checked_add(n));
                if let Some(count) = count.filter(|&n| n <= MAX_FRONTIER_CONTROLS) {
                    match visibility_spend(remaining, left.len() + right.len()) {
                        Ok(()) => {
                            controls = count;
                            cursor.stack.push_back((right, depth + 1));
                            cursor.stack.push_back((left, depth + 1));
                        }
                        Err(error) => uncertain = Some(error),
                    }
                } else {
                    // Refuse incomplete coverage, while still allowing a
                    // later root/sibling to provide a valid positive witness.
                    uncertain = Some("Form visibility exceeded its exact frontier budget; move the view or boundary slightly".into());
                }
            }
            Err(error) if error == super::CANCELLED => return Err(error),
            Err(error) => uncertain = Some(error),
        }
        if !cursor.stack.is_empty() {
            pending.push_back(cursor);
        }
    }
    if !saw_root {
        return Err("an exact Form edge has no visibility restrictions".into());
    }
    uncertain.map_or(Ok(false), Err)
}
fn control_bits(p: &HPoint) -> [u64; 4] {
    [p.x.to_bits(), p.y.to_bits(), p.z.to_bits(), p.w.to_bits()]
}
#[derive(Default)]
struct VisibilityCursor {
    stack: std::collections::VecDeque<(Vec<solidcraft_engine::geom::bspline::HPoint>, usize)>,
    turn: usize,
}

enum VisibilityNode {
    Visible,
    Hidden,
    Split([Vec<solidcraft_engine::geom::bspline::HPoint>; 2]),
}

pub(super) fn validate_visibility_region(region: &BoxSel, remaining: &mut usize) -> Result<(), String> {
    if !region.rect.is_finite() || region.rect.width() < 0.0 || region.rect.height() < 0.0 || region.lasso.len() > 4096 {
        return Err("invalid Form visibility region".into());
    }
    visibility_spend(remaining, region.lasso.len())?;
    if region.is_lasso() && (region.lasso.len() < 3 || region.lasso.iter().any(|point| !point.is_finite())) {
        return Err("invalid Form visibility lasso".into());
    }
    Ok(())
}
fn visibility_spend(remaining: &mut usize, work: usize) -> Result<(), String> {
    *remaining = remaining.checked_sub(work).ok_or("Form visibility exceeded its shared work budget")?;
    Ok(())
}
/// The rectangle encloses the complete projected image. No polygon boundary in
/// that connected rectangle makes membership constant; an outside probe then
/// proves that the whole image misses even a concave or self-intersecting lasso.
pub(super) fn visibility_outside(region: &BoxSel, bounds: [f64; 4]) -> bool {
    if bounds.iter().any(|value| !value.is_finite()) || bounds[0] > bounds[2] || bounds[1] > bounds[3] {
        return false;
    }
    let rect = [f64::from(region.rect.min.x), f64::from(region.rect.min.y), f64::from(region.rect.max.x), f64::from(region.rect.max.y)];
    if bounds[2] < rect[0] || bounds[3] < rect[1] || bounds[0] > rect[2] || bounds[1] > rect[3] {
        return true;
    }
    if !region.is_lasso() {
        return false;
    }
    // Probe the intersection with rect, so a caller's conservative rect need
    // not exactly equal the lasso's bounds.
    let probe = [bounds[0].max(rect[0]), bounds[1].max(rect[1])];
    !contains(region, probe)
        && region
            .lasso
            .iter()
            .zip(region.lasso.iter().cycle().skip(1))
            .take(region.lasso.len())
            .all(|(a, b)| !segment_touches_box([f64::from(a.x), f64::from(a.y)], [f64::from(b.x), f64::from(b.y)], bounds))
}
fn split_world(
    controls: &[solidcraft_engine::geom::bspline::HPoint],
) -> (Vec<solidcraft_engine::geom::bspline::HPoint>, Vec<solidcraft_engine::geom::bspline::HPoint>) {
    let mut row = controls.to_vec();
    let (mut left, mut right) = (Vec::with_capacity(row.len()), Vec::with_capacity(row.len()));
    while let (Some(a), Some(b)) = (row.first().copied(), row.last().copied()) {
        left.push(a);
        right.push(b);
        row = row.windows(2).map(|pair| pair[0] * 0.5 + pair[1] * 0.5).collect();
    }
    right.reverse();
    (left, right)
}
fn visible_curve_node<V, O>(
    proj: &crate::viewport::Proj,
    region: &BoxSel,
    world: &[solidcraft_engine::geom::bspline::HPoint],
    remaining: &mut usize,
    visible: &mut V,
    occluded_hull: &mut O,
    depth: usize,
) -> Result<VisibilityNode, String>
where
    V: FnMut(Vec3, &mut usize) -> Result<bool, String>,
    O: FnMut(&[solidcraft_engine::geom::bspline::HPoint], &mut usize) -> Result<bool, String>,
{
    // Both child endpoints were checked by the parent before it split. They
    // are copied unchanged by de Casteljau, including the shared midpoint.
    let witnesses = if depth == 0 { 3 } else { 1 };
    // One de Casteljau triangle, projection and up to two lasso scans beyond
    // the witness tests. Callbacks charge their own ray and triangle work.
    visibility_spend(remaining, world.len() * (world.len() + 1) / 2 + world.len() + 2 * witnesses + region.lasso.len() * (2 + witnesses))?;
    let Some(projected) = project_controls(proj, world).map_err(str::to_owned)? else { return Ok(VisibilityNode::Hidden) };
    if projected.iter().any(|point| point[2] < f64::MIN_POSITIVE) {
        return Err("Form curve visibility projection has vanishing rational weights".into());
    }
    let mut bounds = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for control in &projected {
        let point = [control[0] / control[2], control[1] / control[2]];
        if point.iter().any(|v| !v.is_finite()) {
            return Err("Form visibility projection overflowed".into());
        }
        bounds[0] = bounds[0].min(point[0]);
        bounds[1] = bounds[1].min(point[1]);
        bounds[2] = bounds[2].max(point[0]);
        bounds[3] = bounds[3].max(point[1]);
    }
    if visibility_outside(region, bounds) {
        return Ok(VisibilityNode::Hidden);
    }
    let (left, right) = split_world(world);
    let endpoints = if depth == 0 { [world.first(), world.last()] } else { [None; 2] };
    for witness in endpoints.into_iter().chain([left.last()]).flatten() {
        let point = witness.project().ok_or("invalid exact Form visibility witness")?;
        let screen =
            project_controls(proj, &[*witness, *witness]).map_err(str::to_owned)?.ok_or("Form visibility witness changed its camera coverage")?;
        let screen = screen.first().ok_or("missing exact Form visibility projection")?;
        if screen[2] < f64::MIN_POSITIVE {
            return Err("Form visibility witness has a vanishing projected weight".into());
        }
        let screen = [screen[0] / screen[2], screen[1] / screen[2]];
        if screen.iter().any(|v| !v.is_finite()) {
            return Err("Form visibility witness projection overflowed".into());
        }
        if contains(region, screen) && visible(point, remaining)? {
            return Ok(VisibilityNode::Visible);
        }
    }
    if occluded_hull(world, remaining)? {
        return Ok(VisibilityNode::Hidden);
    }
    if depth == MAX_DEPTH {
        return Err("Form curve visibility remains unresolved; move the selection boundary or view slightly".into());
    }
    Ok(VisibilityNode::Split([left, right]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visibility_projection() -> crate::viewport::Proj {
        use solidcraft_viewport::{Camera, Mat4};
        crate::viewport::Proj {
            cam: Camera::default(),
            vp: Mat4([[0.02, 0.0, 0.0, 0.0], [0.0, -0.02, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [-1.0, 1.0, 0.0, 1.0]]),
            rect: egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0)),
        }
    }
    #[test]
    fn visibility_queries_search_beyond_hidden_endpoints_and_midpoint_for_both_drag_directions() {
        use solidcraft_engine::geom::bspline::HPoint;
        let world =
            [HPoint::weighted(Vec3::ZERO, 1.0), HPoint::weighted(Vec3::new(10.0, 20.0, 0.0), 1.0), HPoint::weighted(Vec3::new(20.0, 0.0, 0.0), 1.0)];
        for region in [
            rect(false, [-1.0, -1.0], [21.0, 21.0]),
            rect(true, [-1.0, -1.0], [21.0, 21.0]),
            BoxSel::lasso(&[pos2(-1.0, -1.0), pos2(21.0, -1.0), pos2(21.0, 21.0), pos2(-1.0, 21.0)]),
        ] {
            let mut visible = |p: Vec3, budget: &mut usize| {
                visibility_spend(budget, 1)?;
                Ok(p.x > 4.9 && p.x < 5.1)
            };
            let mut no_certificate = |_: &[HPoint], budget: &mut usize| {
                visibility_spend(budget, 1)?;
                Ok(false)
            };
            assert_eq!(curve_has_visible(&visibility_projection(), &region, &world, &mut 1_000_000, &mut visible, &mut no_certificate), Ok(true));
        }
    }
    #[test]
    fn fair_visibility_finds_right_quarter_and_later_piece_before_uncertain_left_branches() {
        use solidcraft_engine::geom::bspline::HPoint;
        let world = [HPoint::weighted(Vec3::new(0.0, 5.0, 0.0), 1.0), HPoint::weighted(Vec3::new(20.0, 5.0, 0.0), 1.0)];
        let region = rect(false, [-1.0, -1.0], [41.0, 21.0]);
        let mut quarter = |point: Vec3, budget: &mut usize| {
            visibility_spend(budget, 1)?;
            Ok(point.x == 15.0)
        };
        let mut unknown = |_: &[HPoint], budget: &mut usize| {
            visibility_spend(budget, 1)?;
            Ok(false)
        };
        assert_eq!(curve_has_visible(&visibility_projection(), &region, &world, &mut 1024, &mut quarter, &mut unknown), Ok(true));
        let second = [HPoint::weighted(Vec3::new(30.0, 5.0, 0.0), 1.0), HPoint::weighted(Vec3::new(40.0, 5.0, 0.0), 1.0)];
        let mut later = |point: Vec3, budget: &mut usize| {
            visibility_spend(budget, 1)?;
            Ok(point.x == 35.0)
        };
        assert_eq!(
            curves_have_visible(&visibility_projection(), &region, [world.as_slice(), second.as_slice()], &mut 1024, &mut later, &mut unknown),
            Ok(true)
        );
        // Native routes can legally exceed the owned frontier coefficient
        // cap. A first exact root witness needs no subdivision allocation.
        let mut first = |point: Vec3, _: &mut usize| Ok(point.x == 0.0);
        assert_eq!(
            curves_have_visible(&visibility_projection(), &region, std::iter::repeat_n(world.as_slice(), 4097), &mut 1024, &mut first, &mut unknown),
            Ok(true)
        );
        let mut hidden = |_: Vec3, _: &mut usize| Ok(false);
        assert!(
            curve_has_visible(&visibility_projection(), &region, &world, &mut 1_000_000, &mut hidden, &mut unknown).is_err(),
            "frontier or unresolved depth cannot become a hidden-curve claim"
        );
        let mut certified = |_: &[HPoint], _: &mut usize| Ok(true);
        assert_eq!(curve_has_visible(&visibility_projection(), &region, &world, &mut 1024, &mut hidden, &mut certified), Ok(false));
        assert_eq!(
            curves_have_visible(&visibility_projection(), &region, [world.as_slice(), second.as_slice()], &mut 1024, &mut hidden, &mut certified),
            Ok(false)
        );
        assert!(
            curves_have_visible(&visibility_projection(), &region, std::iter::empty::<&[HPoint]>(), &mut 1024, &mut hidden, &mut certified).is_err()
        );
    }
    #[test]
    fn bounded_sibling_stacks_complete_deep_hidden_coverage_without_a_breadth_frontier() {
        use solidcraft_engine::geom::bspline::HPoint;
        let first = [0.0, 8.0, 16.0, 24.0].map(|x| HPoint::weighted(Vec3::new(x, 5.0, 0.0), 1.0));
        let second = [32.0, 40.0, 48.0, 56.0].map(|x| HPoint::weighted(Vec3::new(x, 5.0, 0.0), 1.0));
        let mut witnesses = std::collections::BTreeSet::new();
        let mut hidden = |point: Vec3, budget: &mut usize| {
            visibility_spend(budget, 1)?;
            assert!(witnesses.insert(point.x.to_bits()), "a checked dyadic station was queried again");
            Ok(false)
        };
        let mut certificates = 0usize;
        let mut certified = |world: &[HPoint], budget: &mut usize| {
            visibility_spend(budget, 1)?;
            let first = world.first().and_then(|point| point.project()).ok_or("missing exact fixture start")?;
            let last = world.last().and_then(|point| point.project()).ok_or("missing exact fixture end")?;
            // These linear cubic restrictions need depth12. Every negative
            // answer still certifies a complete exact subinterval, not samples.
            let complete = (last.x - first.x).abs() <= 0.006;
            certificates += usize::from(complete);
            Ok(complete)
        };
        let region = rect(true, [-1.0, -1.0], [57.0, 11.0]);
        assert_eq!(
            curves_have_visible(
                &visibility_projection(),
                &region,
                [first.as_slice(), second.as_slice()],
                &mut 1_000_000,
                &mut hidden,
                &mut certified
            ),
            Ok(false)
        );
        assert_eq!(certificates, 8192, "both restrictions require all 4096 depth-12 intervals");
        assert_eq!(witnesses.len(), 2 * 8193, "one witness per dyadic station across both restrictions");
    }
    #[test]
    fn a_saturated_earlier_frontier_does_not_hide_a_later_root_witness_or_become_negative() {
        use solidcraft_engine::geom::bspline::HPoint;
        let first = [HPoint::weighted(Vec3::new(0.0, 5.0, 0.0), 1.0), HPoint::weighted(Vec3::new(1.0, 5.0, 0.0), 1.0)];
        let later = [HPoint::weighted(Vec3::new(30.0, 5.0, 0.0), 1.0), HPoint::weighted(Vec3::new(31.0, 5.0, 0.0), 1.0)];
        let region = rect(true, [-1.0, -1.0], [41.0, 11.0]);
        // Distinct restrictions exercise the frontier even when an exact
        // duplicate root would share its existing certification obligation.
        let earlier: Vec<_> = (0..5000)
            .map(|i| {
                first.map(|mut point| {
                    point.y += f64::from(i) * 1e-6;
                    point
                })
            })
            .collect();
        let roots = || earlier.iter().map(|piece| piece.as_slice()).chain(std::iter::once(later.as_slice()));
        let mut visible = |point: Vec3, budget: &mut usize| {
            visibility_spend(budget, 1)?;
            Ok(point.x >= 30.0)
        };
        let mut unknown = |_: &[HPoint], budget: &mut usize| {
            visibility_spend(budget, 1)?;
            Ok(false)
        };
        assert_eq!(curves_have_visible(&visibility_projection(), &region, roots(), &mut 1_000_000, &mut visible, &mut unknown), Ok(true));
        let mut hidden = |_: Vec3, _: &mut usize| Ok(false);
        assert!(curves_have_visible(&visibility_projection(), &region, roots(), &mut 1_000_000, &mut hidden, &mut unknown).is_err());
    }

    #[test]
    fn bit_identical_incident_restrictions_share_work_but_nearby_geometry_keeps_its_witness() {
        let world = [HPoint::weighted(Vec3::new(0.0, 5.0, 0.0), 1.0), HPoint::weighted(Vec3::new(10.0, 5.0, 0.0), 1.0)];
        let copy = world;
        let mut near = world;
        near[1].z = near[1].z.next_up();
        let region = rect(true, [-1.0, -1.0], [11.0, 10.0]);
        let mut hidden = |_: Vec3, _: &mut usize| Ok(false);
        let mut calls = 0;
        let mut certified = |_: &[HPoint], budget: &mut usize| {
            calls += 1;
            visibility_spend(budget, 64)?;
            Ok(true)
        };
        assert_eq!(
            curves_have_visible(&visibility_projection(), &region, [world.as_slice(), copy.as_slice()], &mut 100, &mut hidden, &mut certified),
            Ok(false)
        );
        assert_eq!(calls, 1, "two allocated but bit-identical nets certify their whole hull once");
        let mut visible = |point: Vec3, _: &mut usize| Ok(point.z > 0.0);
        let mut certified = |_: &[HPoint], _: &mut usize| Ok(true);
        assert_eq!(
            curves_have_visible(&visibility_projection(), &region, [world.as_slice(), near.as_slice()], &mut 100, &mut visible, &mut certified),
            Ok(true)
        );
        let mut unknown = |_: &[HPoint], _: &mut usize| Ok(false);
        assert!(
            curves_have_visible(&visibility_projection(), &region, [world.as_slice(), copy.as_slice()], &mut 100, &mut hidden, &mut unknown).is_err(),
            "a repeated root must not convert incomplete coverage into a negative result"
        );
    }

    #[test]
    fn fair_visibility_preserves_callback_errors_and_shared_budget_even_after_other_pieces_are_certified_hidden() {
        use solidcraft_engine::geom::bspline::HPoint;
        let first = [HPoint::weighted(Vec3::new(0.0, 5.0, 0.0), 1.0), HPoint::weighted(Vec3::new(1.0, 5.0, 0.0), 1.0)];
        let second = [HPoint::weighted(Vec3::new(2.0, 5.0, 0.0), 1.0), HPoint::weighted(Vec3::new(3.0, 5.0, 0.0), 1.0)];
        let region = rect(true, [-1.0, -1.0], [4.0, 10.0]);
        let mut visible = |point: Vec3, _: &mut usize| {
            if point.x < 2.0 { Err("exact point visibility failed".to_owned()) } else { Ok(false) }
        };
        let mut certified = |_: &[HPoint], _: &mut usize| Ok(true);
        assert_eq!(
            curves_have_visible(&visibility_projection(), &region, [first.as_slice(), second.as_slice()], &mut 1024, &mut visible, &mut certified),
            Err("exact point visibility failed".to_owned())
        );
        let mut hidden = |_: Vec3, _: &mut usize| Ok(false);
        let mut failed = |_: &[HPoint], _: &mut usize| Err("occlusion coverage failed".to_owned());
        assert_eq!(
            curves_have_visible(&visibility_projection(), &region, [first.as_slice(), second.as_slice()], &mut 1024, &mut hidden, &mut failed),
            Err("occlusion coverage failed".to_owned())
        );
        assert!(
            curves_have_visible(&visibility_projection(), &region, [first.as_slice(), second.as_slice()], &mut 1, &mut hidden, &mut certified)
                .is_err()
        );
    }

    #[test]
    fn hidden_samples_need_a_whole_hull_certificate_or_an_explicit_error() {
        use solidcraft_engine::geom::bspline::HPoint;
        let world = [HPoint::weighted(Vec3::new(2.0, 5.0, 0.0), 1.0), HPoint::weighted(Vec3::new(18.0, 5.0, 0.0), 1.0)];
        let mut hidden = |_: Vec3, _: &mut usize| Ok(false);
        let mut uncertain = |_: &[HPoint], _: &mut usize| Ok(false);
        let region = rect(false, [0.0, 0.0], [20.0, 10.0]);
        assert!(curve_has_visible(&visibility_projection(), &region, &world, &mut 1_000_000, &mut hidden, &mut uncertain).is_err());
        let mut certified = |_: &[HPoint], _: &mut usize| Ok(true);
        assert_eq!(curve_has_visible(&visibility_projection(), &region, &world, &mut 1_000_000, &mut hidden, &mut certified), Ok(false));
    }
    #[test]
    fn visibility_never_uses_a_visible_point_outside_the_region_or_a_lasso_notch() {
        use solidcraft_engine::geom::bspline::HPoint;
        let world = [HPoint::weighted(Vec3::new(0.0, 5.0, 0.0), 1.0), HPoint::weighted(Vec3::new(20.0, 5.0, 0.0), 1.0)];
        let mut visible = |p: Vec3, _: &mut usize| Ok(p.x >= 10.0);
        let mut uncertain = |_: &[HPoint], _: &mut usize| Ok(false);
        assert!(
            curve_has_visible(&visibility_projection(), &rect(false, [0.0, 0.0], [4.0, 10.0]), &world, &mut 1_000_000, &mut visible, &mut uncertain)
                .is_err()
        );
        let lasso = BoxSel::lasso(&[
            pos2(0.0, 0.0),
            pos2(20.0, 0.0),
            pos2(20.0, 20.0),
            pos2(12.0, 20.0),
            pos2(12.0, 8.0),
            pos2(8.0, 8.0),
            pos2(8.0, 20.0),
            pos2(0.0, 20.0),
        ]);
        let world = [HPoint::weighted(Vec3::new(9.0, 15.0, 0.0), 1.0), HPoint::weighted(Vec3::new(11.0, 15.0, 0.0), 1.0)];
        let mut must_not_query = |_: Vec3, _: &mut usize| -> Result<bool, String> { panic!("outside curve must be rejected before visibility") };
        assert_eq!(curve_has_visible(&visibility_projection(), &lasso, &world, &mut 1_000_000, &mut must_not_query, &mut uncertain), Ok(false));
    }
    #[test]
    fn subpixel_curve_visibility_still_requires_a_witness_or_full_occlusion_certificate() {
        use solidcraft_engine::geom::bspline::HPoint;
        let world = [HPoint::weighted(Vec3::new(0.0, 5.0, 0.0), 1.0), HPoint::weighted(Vec3::new(0.1, 5.0, 0.0), 1.0)];
        let mut visible = |point: Vec3, _: &mut usize| Ok(point.x == 0.025);
        let mut unknown = |_: &[HPoint], _: &mut usize| Ok(false);
        assert!(
            curve_has_visible(&visibility_projection(), &rect(false, [-1.0, -1.0], [21.0, 21.0]), &world, &mut 100_000, &mut visible, &mut unknown)
                .unwrap()
        );
    }
    #[test]
    fn rational_visibility_witnesses_keep_world_weights_and_share_work_failures() {
        use solidcraft_engine::geom::{bspline::HPoint, prelude::*};
        let world =
            [HPoint::weighted(Vec3::ZERO, 1.0), HPoint::weighted(Vec3::new(10.0, 20.0, 0.0), 2.0), HPoint::weighted(Vec3::new(20.0, 0.0, 0.0), 1.0)];
        let mut visible = |point: Vec3, _: &mut usize| {
            if point.y > 1.0 {
                assert!(point.dist(Vec3::new(10.0, 40.0 / 3.0, 0.0)) < 1e-12);
                Ok(true)
            } else {
                Ok(false)
            }
        };
        let mut no_certificate = |_: &[HPoint], _: &mut usize| Ok(false);
        let region = rect(true, [-1.0, -1.0], [21.0, 21.0]);
        assert_eq!(curve_has_visible(&visibility_projection(), &region, &world, &mut 1_000_000, &mut visible, &mut no_certificate), Ok(true));
        assert!(curve_has_visible(&visibility_projection(), &region, &world, &mut 1, &mut visible, &mut no_certificate).is_err());
        let mut failed = |_: Vec3, _: &mut usize| Err("visibility budget exhausted".to_owned());
        assert!(curve_has_visible(&visibility_projection(), &region, &world, &mut 1_000_000, &mut failed, &mut no_certificate).is_err());
        let mut bad = world;
        bad[1].w = 0.0;
        assert!(curve_has_visible(&visibility_projection(), &region, &bad, &mut 1_000_000, &mut visible, &mut no_certificate).is_err());
    }
    use egui::pos2;
    fn rect(crossing: bool, min: [f32; 2], max: [f32; 2]) -> BoxSel {
        BoxSel { rect: egui::Rect::from_min_max(pos2(min[0], min[1]), pos2(max[0], max[1])), crossing, lasso: Vec::new() }
    }
    fn takes(r: &BoxSel, p: &[[f64; 3]]) -> Result<bool, &'static str> {
        curve_takes(r, p, &mut 65536)
    }
    #[test]
    fn projected_curve_hulls_are_not_crossing_witnesses() {
        let curve = [[0.0, 0.0, 1.0], [0.0, 20.0, 1.0], [20.0, 20.0, 1.0]];
        assert_eq!(takes(&rect(true, [0.9, 18.9], [1.1, 19.1]), &curve), Ok(false));
        assert_eq!(takes(&rect(true, [4.9, 14.9], [5.1, 15.1]), &curve), Ok(true));
    }
    #[test]
    fn projected_curve_windows_reject_excursions_and_accept_containment() {
        let curve = [[0.0, 0.0, 1.0], [10.0, 40.0, 1.0], [20.0, 0.0, 1.0]];
        assert_eq!(takes(&rect(false, [-1.0, -1.0], [21.0, 2.0]), &curve), Ok(false));
        assert_eq!(takes(&rect(false, [-1.0, -1.0], [21.0, 41.0]), &curve), Ok(true));
    }
    #[test]
    fn projected_curve_lasso_rejects_a_concave_notch() {
        let lasso = BoxSel::lasso(&[
            pos2(0.0, 0.0),
            pos2(20.0, 0.0),
            pos2(20.0, 20.0),
            pos2(12.0, 20.0),
            pos2(12.0, 8.0),
            pos2(8.0, 8.0),
            pos2(8.0, 20.0),
            pos2(0.0, 20.0),
        ]);
        assert_eq!(takes(&lasso, &[[2.0, 15.0, 1.0], [18.0, 15.0, 1.0]]), Ok(false));
        assert_eq!(takes(&lasso, &[[2.0, 2.0, 1.0], [18.0, 2.0, 1.0]]), Ok(true));
    }
    #[test]
    fn projected_curve_weights_and_resource_limits_are_explicit() {
        let curve = [[0.0, 0.0, 1.0], [20.0, 40.0, 2.0], [20.0, 0.0, 1.0]];
        assert_eq!(takes(&rect(true, [9.9, 13.2], [10.1, 13.5]), &curve), Ok(true));
        assert!(curve_takes(&rect(true, [-1.0, -1.0], [21.0, 41.0]), &curve, &mut 1).is_err());
        assert!(takes(&rect(true, [-1.0, -1.0], [21.0, 41.0]), &[[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]]).is_err());
        assert!(takes(&rect(true, [-1.0, -1.0], [21.0, 41.0]), &[[f64::NAN, 0.0, 1.0], [1.0, 1.0, 1.0]]).is_err());
        let tiny = f64::from_bits(1);
        assert!(takes(&rect(true, [-1.0, -1.0], [1.0, 1.0]), &[[-2.0 * tiny, 0.0, tiny], [2.0 * tiny, 0.0, tiny]]).is_err());
        assert!(!contains(&rect(true, [-1.0, -1.0], [1.0, 1.0]), [f64::NAN, 0.0]));
        let lasso = BoxSel::lasso(&[pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(10.0, 10.0), pos2(0.0, 10.0)]);
        assert!(curve_takes(&lasso, &[[2.0, 2.0, 1.0], [8.0, 8.0, 1.0]], &mut 10).is_err());
    }
    #[test]
    fn homogeneous_projection_preserves_world_weights_and_rejects_near_plane_mixes() {
        use solidcraft_engine::geom::bspline::HPoint;
        use solidcraft_viewport::{Camera, Mat4};
        let mut proj = crate::viewport::Proj {
            cam: Camera::default(),
            vp: Mat4::IDENTITY,
            rect: egui::Rect::from_min_max(pos2(10.0, 20.0), pos2(810.0, 620.0)),
        };
        let controls = [HPoint::weighted(Vec3::new(-0.5, 0.5, 1.0), 2.0), HPoint::weighted(Vec3::new(0.5, -0.5, 2.0), 1.0)];
        let projected = project_controls(&proj, &controls).unwrap().unwrap();
        let (_, halves) = split(&projected);
        let h = halves.first().unwrap();
        let world = (controls[0] + controls[1]) * 0.5;
        let expected = proj.to_screen(world.project().unwrap()).unwrap();
        assert!((h[0] / h[2] - f64::from(expected.x)).abs() < 1e-4);
        assert!((h[1] / h[2] - f64::from(expected.y)).abs() < 1e-4);
        proj.vp = Mat4([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 0.0]]);
        let behind = HPoint::weighted(Vec3::new(0.0, 0.0, -1.0), 1.0);
        assert!(project_controls(&proj, &[behind, behind]).unwrap().is_none());
        assert!(project_controls(&proj, &[behind, controls[0]]).is_err());
    }
}

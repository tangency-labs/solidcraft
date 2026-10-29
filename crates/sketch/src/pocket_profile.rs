//! Inset a closed profile and round the corners a round cutter cannot reach.
//!
//! A positive distance moves every segment to its left. On a counter-clockwise outer loop that
//! is inward, so the region shrinks; on a clockwise hole it is outward, so the island grows.
//! Only a left turn between two lines is rounded. A right turn, the outside of an island, stays
//! sharp.

use solidcraft_geom::{Loop2, Region2, Seg2, Vec2};

use crate::SketchError;

const MIN_SPAN: f64 = 1e-6;

type Result<T> = std::result::Result<T, SketchError>;

fn invalid(message: &str) -> SketchError {
    SketchError::Invalid(message.to_string())
}

/// Move `region` inward by `distance` (mm). Zero leaves the loops as they are, oriented with a
/// counter-clockwise outer boundary and clockwise holes.
pub fn inset_region(region: &Region2, distance: f64) -> Result<Region2> {
    if !distance.is_finite() || distance < 0.0 {
        return Err(invalid("the offset distance must be finite and at least zero"));
    }
    let region = oriented(region);
    if distance <= MIN_SPAN {
        return Ok(region);
    }
    Ok(Region2 { outer: offset_loop(&region.outer, distance)?, holes: offset_holes(&region.holes, distance)? })
}

/// Replace each left turn between lines with an arc of `radius` (mm). Zero leaves the profile
/// unchanged. An edge too short to take the arc is an error.
pub fn round_convex_corners(region: &Region2, radius: f64) -> Result<Region2> {
    if !radius.is_finite() || radius < 0.0 {
        return Err(invalid("the tool radius must be finite and at least zero"));
    }
    let region = oriented(region);
    if radius <= MIN_SPAN {
        return Ok(region);
    }
    Ok(Region2 {
        outer: round_loop(&region.outer, radius)?,
        holes: region.holes.iter().map(|hole| round_loop(hole, radius)).collect::<Result<Vec<_>>>()?,
    })
}

fn oriented(region: &Region2) -> Region2 {
    Region2 { outer: region.outer.ccw(), holes: region.holes.iter().map(|hole| if hole.is_ccw() { hole.reversed() } else { hole.clone() }).collect() }
}

fn offset_holes(holes: &[Loop2], distance: f64) -> Result<Vec<Loop2>> {
    holes.iter().map(|hole| offset_loop(hole, distance)).collect()
}

enum Carrier {
    Line { origin: Vec2, dir: Vec2 },
    Arc { center: Vec2, radius: f64 },
}

fn offset_loop(loop2: &Loop2, distance: f64) -> Result<Loop2> {
    // A circle split at its seam is two arcs. Offsetting each piece on its own moves the
    // seam and the sweep check refuses a bay that is still a single arc.
    let mut segs = merge_continued_arcs(&loop2.segs);
    if segs.is_empty() {
        return Err(invalid("the profile has no boundary"));
    }
    // A bay narrower than the inset disappears. Drop that edge and offset what remains,
    // which is what a face offset does, instead of refusing the whole profile.
    for _ in 0..=segs.len() {
        match offset_kept(&segs, distance) {
            Ok(out) => return Ok(out),
            Err(OffsetFail::Consume(index)) => {
                segs.remove(index);
                if segs.is_empty() {
                    return Err(invalid("the offset is larger than the profile allows"));
                }
            }
            Err(OffsetFail::Fatal(err)) => return Err(err),
        }
    }
    Err(invalid("the offset is larger than the profile allows"))
}

enum OffsetFail {
    Consume(usize),
    Fatal(SketchError),
}

fn offset_kept(segs: &[Seg2], distance: f64) -> std::result::Result<Loop2, OffsetFail> {
    let fatal = |err| OffsetFail::Fatal(err);
    for (index, seg) in segs.iter().enumerate() {
        if let Seg2::Arc { radius, sweep, .. } = seg {
            let grown = if *sweep > 0.0 { radius - distance } else { radius + distance };
            if grown <= MIN_SPAN {
                return Err(OffsetFail::Consume(index));
            }
        }
    }
    let carriers = segs.iter().map(|seg| carrier(seg, distance)).collect::<Result<Vec<_>>>().map_err(fatal)?;
    let n = carriers.len();
    let mut joints = Vec::with_capacity(n);
    for i in 0..n {
        let prev = (i + n - 1) % n;
        let near = segs[i].start();
        joints.push(join(&carriers[prev], &carriers[i], near, distance).map_err(fatal)?);
    }
    let mut out_segs = Vec::with_capacity(n);
    for i in 0..n {
        let end = (i + 1) % n;
        match rebuild(&segs[i], &carriers[i], joints[i], joints[end]) {
            Ok(seg) => out_segs.push(seg),
            Err(Rebuild::Consume) => return Err(OffsetFail::Consume(i)),
            Err(Rebuild::Fatal(err)) => return Err(fatal(err)),
        }
    }
    let out = Loop2 { segs: out_segs };
    if !out.is_closed(1e-4) {
        return Err(fatal(invalid("the offset is larger than the profile allows")));
    }
    let before = Loop2 { segs: segs.to_vec() }.signed_area();
    let after = out.signed_area();
    if !after.is_finite() || after.abs() <= MIN_SPAN || before.signum() != after.signum() {
        return Err(fatal(invalid("the offset is larger than the profile allows (the region flipped)")));
    }
    Ok(out)
}

fn carrier(seg: &Seg2, distance: f64) -> Result<Carrier> {
    match *seg {
        Seg2::Line { a, b } => {
            let dir = (b - a).normalized().ok_or_else(|| invalid("the profile has a zero-length edge"))?;
            Ok(Carrier::Line { origin: a + dir.perp() * distance, dir })
        }
        Seg2::Arc { center, radius, sweep, .. } => {
            if !(radius.is_finite() && radius > MIN_SPAN && sweep.is_finite() && sweep.abs() > MIN_SPAN) {
                return Err(invalid("the profile has a degenerate arc"));
            }
            // A counter-clockwise arc has its centre on the left, so moving left reduces the radius.
            let radius = if sweep > 0.0 { radius - distance } else { radius + distance };
            if radius <= MIN_SPAN {
                return Err(invalid("the offset is larger than the profile allows (the arc radius)"));
            }
            Ok(Carrier::Arc { center, radius })
        }
        Seg2::Cubic { .. } | Seg2::Conic { .. } | Seg2::Bezier { .. } => Err(invalid("profiles must be lines and circular arcs")),
    }
}

fn join(prev: &Carrier, next: &Carrier, near: Vec2, distance: f64) -> Result<Vec2> {
    let point = match (prev, next) {
        (Carrier::Line { origin: o1, dir: d1 }, Carrier::Line { origin: o2, dir: d2 }) => {
            let den = d1.cross(*d2);
            if den.abs() < 1e-9 {
                if d1.dot(*d2) <= 0.0 {
                    return Err(invalid("the offset is larger than the profile allows (the lines fold back)"));
                }
                near + d1.perp() * distance
            } else {
                let t = (*o2 - *o1).cross(*d2) / den;
                *o1 + *d1 * t
            }
        }
        (Carrier::Line { origin, dir }, Carrier::Arc { center, radius, .. })
        | (Carrier::Arc { center, radius, .. }, Carrier::Line { origin, dir }) => {
            let hits = circle_line(*center, *radius, *origin, *dir);
            closest(&hits, radial_hint(*center, *radius, near))
                .ok_or_else(|| invalid("the offset is larger than the profile allows (a line misses its arc)"))?
        }
        (Carrier::Arc { center: c1, radius: r1, .. }, Carrier::Arc { center: c2, radius: r2, .. }) => {
            if c1.dist(*c2) < 1e-8 && (r1 - r2).abs() < 1e-8 {
                let dir =
                    (near - *c1).normalized().ok_or_else(|| invalid("the offset is larger than the profile allows (the arcs have no direction)"))?;
                *c1 + dir * *r1
            } else {
                let hits = circle_circle(*c1, *r1, *c2, *r2);
                closest(&hits, radial_hint(*c2, *r2, near))
                    .ok_or_else(|| invalid("the offset is larger than the profile allows (the arcs miss each other)"))?
            }
        }
    };
    if !point.is_finite() || point.dist(near) > 1e6 {
        return Err(invalid("the offset is larger than the profile allows (the corner ran off)"));
    }
    Ok(point)
}

fn radial_hint(center: Vec2, radius: f64, near: Vec2) -> Vec2 {
    (near - center).normalized().map(|dir| center + dir * radius).unwrap_or(near)
}

fn closest(points: &[Vec2], hint: Vec2) -> Option<Vec2> {
    let mut best: Option<(f64, Vec2)> = None;
    for point in points {
        let score = point.dist(hint);
        if best.is_none_or(|(so_far, _)| score < so_far) {
            best = Some((score, *point));
        }
    }
    best.map(|(_, point)| point)
}

fn circle_line(center: Vec2, radius: f64, origin: Vec2, dir: Vec2) -> Vec<Vec2> {
    let along = (center - origin).dot(dir);
    let side = (center - origin).dot(dir.perp());
    let height2 = radius * radius - side * side;
    if height2 < -1e-8 {
        return Vec::new();
    }
    let foot = origin + dir * along;
    let height = height2.max(0.0).sqrt();
    if height < 1e-8 {
        vec![foot]
    } else {
        vec![foot - dir * height, foot + dir * height]
    }
}

fn circle_circle(c1: Vec2, r1: f64, c2: Vec2, r2: f64) -> Vec<Vec2> {
    let delta = c2 - c1;
    let dist = delta.len();
    if dist < 1e-12 {
        return Vec::new();
    }
    let along = (dist * dist + r1 * r1 - r2 * r2) / (2.0 * dist);
    let height2 = r1 * r1 - along * along;
    if height2 < -1e-8 {
        return Vec::new();
    }
    let foot = c1 + delta * (along / dist);
    let height = height2.max(0.0).sqrt();
    if height < 1e-8 {
        vec![foot]
    } else {
        let side = delta.perp() / dist;
        vec![foot + side * height, foot - side * height]
    }
}

enum Rebuild {
    Consume,
    Fatal(SketchError),
}

fn rebuild(original: &Seg2, carrier: &Carrier, start: Vec2, end: Vec2) -> std::result::Result<Seg2, Rebuild> {
    match (original, carrier) {
        (Seg2::Line { a, b }, Carrier::Line { .. }) => {
            let Some(old) = (*b - *a).normalized() else {
                return Err(Rebuild::Fatal(invalid("the profile has a zero-length edge")));
            };
            let span = end - start;
            // The edge was shorter than the inset: the bay it closed is gone.
            if span.dot(old) <= MIN_SPAN {
                return Err(Rebuild::Consume);
            }
            Ok(Seg2::Line { a: start, b: end })
        }
        (Seg2::Arc { sweep, .. }, Carrier::Arc { center, radius, .. }) => {
            let start_angle = (start - *center).angle();
            let end_angle = (end - *center).angle();
            let new_sweep = sweep_near(start_angle, end_angle, *sweep);
            // The arc flipped onto the other side of its circle: this bay did not survive.
            if new_sweep.abs() <= MIN_SPAN || new_sweep.signum() != sweep.signum() || (new_sweep - sweep).abs() > std::f64::consts::PI {
                return Err(Rebuild::Consume);
            }
            Ok(Seg2::Arc { center: *center, radius: *radius, start: start_angle, sweep: new_sweep })
        }
        _ => Err(Rebuild::Fatal(invalid("profiles must be lines and circular arcs"))),
    }
}

/// Pieces of one circle, split only at its seam, offset as one arc.
fn merge_continued_arcs(segs: &[Seg2]) -> Vec<Seg2> {
    if segs.len() < 2 {
        return segs.to_vec();
    }
    let mut segs = segs.to_vec();
    if let Some(index) = segs.iter().position(|seg| !matches!(seg, Seg2::Arc { .. })) {
        segs.rotate_left(index);
    }
    let mut out: Vec<Seg2> = Vec::new();
    for seg in segs {
        let continued = match (out.last_mut(), seg) {
            (
                Some(Seg2::Arc { center, radius, start, sweep }),
                Seg2::Arc { center: next_center, radius: next_radius, start: next_start, sweep: next_sweep },
            ) if center.dist(next_center) < 1e-6
                && (*radius - next_radius).abs() < 1e-6
                && sweep.signum() == next_sweep.signum()
                && angle_gap(*start + *sweep, next_start) < 1e-4 =>
            {
                *sweep += next_sweep;
                true
            }
            _ => false,
        };
        if !continued {
            out.push(seg);
        }
    }
    if out.len() >= 2 {
        let last = *out.last().unwrap();
        let first = out[0];
        if let Seg2::Arc { center: last_center, radius: last_radius, start: last_start, sweep: last_sweep } = last
            && let Seg2::Arc { center, radius, start, sweep } = first
            && last_center.dist(center) < 1e-6
            && (last_radius - radius).abs() < 1e-6
            && last_sweep.signum() == sweep.signum()
            && angle_gap(last_start + last_sweep, start) < 1e-4
        {
            out[0] = Seg2::Arc { center, radius, start: last_start, sweep: last_sweep + sweep };
            out.pop();
        }
    }
    out
}

fn angle_gap(a: f64, b: f64) -> f64 {
    let turn = (a - b).rem_euclid(std::f64::consts::TAU);
    turn.min(std::f64::consts::TAU - turn)
}

/// The turn from `start_angle` to `end_angle` closest to `original`.
fn sweep_near(start_angle: f64, end_angle: f64, original: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    let base = (end_angle - start_angle).rem_euclid(tau);
    let mut best = base;
    let mut best_gap = (base - original).abs();
    for extra in [-tau, tau] {
        let candidate = base + extra;
        let gap = (candidate - original).abs();
        if gap < best_gap {
            best = candidate;
            best_gap = gap;
        }
    }
    best
}

#[derive(Clone, Copy)]
struct Turn {
    cut: f64,
    alpha: f64,
    center: Vec2,
    start: f64,
}

fn round_loop(loop2: &Loop2, radius: f64) -> Result<Loop2> {
    let n = loop2.segs.len();
    if n == 0 {
        return Err(invalid("the profile has no boundary"));
    }
    let mut turns = vec![None; n];
    for (i, turn) in turns.iter_mut().enumerate() {
        let next = (i + 1) % n;
        if let Some(found) = left_turn(&loop2.segs[i], &loop2.segs[next], radius)? {
            *turn = Some(found);
        }
    }
    for (i, seg) in loop2.segs.iter().enumerate() {
        if !matches!(seg, Seg2::Line { .. }) {
            continue;
        }
        let start_cut = end_cut(&turns, (i + n - 1) % n);
        let end = end_cut(&turns, i);
        if start_cut + end > seg.length() + 1e-7 {
            return Err(invalid("the tool radius is larger than the profile allows"));
        }
    }
    let mut segs = Vec::new();
    for (i, seg) in loop2.segs.iter().enumerate() {
        match *seg {
            Seg2::Line { a, b } => {
                let dir = (b - a).normalized().ok_or_else(|| invalid("the profile has a zero-length edge"))?;
                let start_cut = end_cut(&turns, (i + n - 1) % n);
                let end = end_cut(&turns, i);
                let a2 = a + dir * start_cut;
                let b2 = b - dir * end;
                if (b2 - a2).dot(dir) > MIN_SPAN {
                    segs.push(Seg2::Line { a: a2, b: b2 });
                }
                if let Some(turn) = &turns[i] {
                    segs.push(Seg2::Arc { center: turn.center, radius, start: turn.start, sweep: turn.alpha });
                }
            }
            other => {
                if turns[i].is_some() || turns[(i + n - 1) % n].is_some() {
                    return Err(invalid("only a corner between two lines is rounded"));
                }
                segs.push(other);
            }
        }
    }
    let out = Loop2 { segs };
    if !out.is_closed(1e-4) || out.signed_area().signum() != loop2.signed_area().signum() {
        return Err(invalid("the tool radius is larger than the profile allows"));
    }
    Ok(out)
}

fn end_cut(turns: &[Option<Turn>], index: usize) -> f64 {
    turns.get(index).and_then(|turn| turn.as_ref()).map(|turn| turn.cut).unwrap_or(0.0)
}

fn left_turn(incoming: &Seg2, outgoing: &Seg2, radius: f64) -> Result<Option<Turn>> {
    let (Seg2::Line { a, b }, Seg2::Line { a: c, b: d }) = (*incoming, *outgoing) else {
        return Ok(None);
    };
    if b.dist(c) > 1e-3 {
        return Err(invalid("the profile is not closed"));
    }
    let u = (b - a).normalized().ok_or_else(|| invalid("the profile has a zero-length edge"))?;
    let v = (d - c).normalized().ok_or_else(|| invalid("the profile has a zero-length edge"))?;
    let alpha = u.cross(v).atan2(u.dot(v));
    // A straight joint and a right turn stay as they are. A left turn is a corner the cutter
    // cannot reach.
    if alpha <= 1e-3 {
        return Ok(None);
    }
    let cut = radius / (alpha * 0.5).tan();
    if !(cut.is_finite() && cut > 0.0) {
        return Err(invalid("the tool radius is larger than the profile allows"));
    }
    let corner = b;
    let tangent_in = corner - u * cut;
    let center = tangent_in + u.perp() * radius;
    Ok(Some(Turn { cut, alpha, center, start: (tangent_in - center).angle() }))
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use solidcraft_geom::Vec2;

    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Region2 {
        Region2 { outer: Loop2::polygon(&[Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]), holes: Vec::new() }
    }

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-6 * (1.0 + expected.abs()), "{actual} != {expected}");
    }

    #[test]
    fn a_rectangle_shrinks_by_the_inset_on_every_side() {
        let pocket = inset_region(&rect(0.0, 0.0, 10.0, 10.0), 2.0).unwrap();
        close(pocket.area(), 6.0 * 6.0);
        assert!(pocket.contains(Vec2::new(5.0, 5.0)));
        assert!(!pocket.contains(Vec2::new(1.0, 5.0)));
        assert_eq!(pocket.outer.segs.len(), 4);
    }

    #[test]
    fn a_hole_grows_so_the_island_keeps_a_wall() {
        let mut region = rect(0.0, 0.0, 20.0, 20.0);
        region.holes.push(Loop2::polygon(&[Vec2::new(5.0, 5.0), Vec2::new(5.0, 15.0), Vec2::new(15.0, 15.0), Vec2::new(15.0, 5.0)]));
        let pocket = inset_region(&region, 1.0).unwrap();
        close(pocket.area(), 18.0 * 18.0 - 12.0 * 12.0);
        assert!(pocket.contains(Vec2::new(1.5, 10.0)));
        assert!(!pocket.contains(Vec2::new(4.5, 10.0)));
    }

    #[test]
    fn a_circle_keeps_its_centre_and_loses_the_inset() {
        let region = Region2 { outer: Loop2::circle(Vec2::new(3.0, 4.0), 10.0), holes: Vec::new() };
        let pocket = inset_region(&region, 2.0).unwrap();
        close(pocket.area(), PI * 64.0);
        assert!(pocket.outer.segs.iter().all(|seg| matches!(seg, Seg2::Arc { radius, .. } if (radius - 8.0).abs() < 1e-6)));
    }

    #[test]
    fn an_inset_past_the_middle_is_refused() {
        let err = inset_region(&rect(0.0, 0.0, 10.0, 4.0), 3.0).unwrap_err();
        assert!(err.to_string().contains("larger than the profile"));
    }

    #[test]
    fn a_left_turn_becomes_a_quarter_arc_and_a_right_turn_stays_sharp() {
        let rounded = round_convex_corners(&rect(0.0, 0.0, 10.0, 6.0), 1.0).unwrap();
        let removed = (4.0 - PI) * 1.0;
        close(rounded.area(), 60.0 - removed);
        assert_eq!(rounded.outer.segs.iter().filter(|seg| matches!(seg, Seg2::Arc { .. })).count(), 4);

        let mut with_island = rect(0.0, 0.0, 20.0, 20.0);
        with_island.holes.push(Loop2::polygon(&[Vec2::new(5.0, 5.0), Vec2::new(5.0, 15.0), Vec2::new(15.0, 15.0), Vec2::new(15.0, 5.0)]));
        let kept = round_convex_corners(&with_island, 1.0).unwrap();
        assert!(kept.holes[0].segs.iter().all(|seg| matches!(seg, Seg2::Line { .. })));
        close(kept.area(), 400.0 - removed - 100.0);
    }

    /// The truss bay whose node circle is stored as two arcs meeting at the seam.
    /// Insetting by half of a 2 mm wall keeps the bay.
    #[test]
    fn a_split_node_circle_still_insets() {
        let region = Region2 {
            outer: Loop2 {
                segs: vec![
                    Seg2::Arc { center: Vec2::new(-150.0, 60.0), radius: 2.0, start: 1.2268272070715291, sweep: -0.42503706936607244 },
                    Seg2::Line { a: Vec2::new(-148.6091571449679, 61.437204283533184), b: Vec2::new(0.0, 215.0) },
                    Seg2::Line { a: Vec2::new(0.0, 215.0), b: Vec2::new(-190.0, 215.0) },
                    Seg2::Line { a: Vec2::new(-190.0, 215.0), b: Vec2::new(-127.17634339535009, 128.61747216860638) },
                    Seg2::Arc { center: Vec2::new(-126.0, 127.0), radius: 2.0, start: 2.199592613210326, sweep: -2.199592613210326 },
                    Seg2::Arc { center: Vec2::new(-126.0, 127.0), radius: 2.0, start: 6.283185307179586, sweep: -1.9147654465182606 },
                    Seg2::Line { a: Vec2::new(-126.67445272318704, 125.11715281443618), b: Vec2::new(-149.32554727681298, 61.882847185563804) },
                ],
            },
            holes: Vec::new(),
        };
        let pocket = inset_region(&region, 1.0).unwrap_or_else(|err| panic!("{err}"));
        assert!(pocket.area() > 1.0, "{}", pocket.area());
        assert!(pocket.area() < region.area(), "{} vs {}", pocket.area(), region.area());
    }

    #[test]
    fn a_corner_shorter_than_the_tool_is_refused() {
        let err = round_convex_corners(&rect(0.0, 0.0, 4.0, 10.0), 3.0).unwrap_err();
        assert!(err.to_string().contains("tool radius"));
    }
}

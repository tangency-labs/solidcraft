//! Contact between a finite segment and a band of linearly varying width.

use crate::Vec2;

/// A point of `trace` inside the band about `axis`. The radius interpolates between the
/// axis endpoints at the nearest point of the axis, giving round caps. A collapsed axis uses
/// its first radius. Returns `None` for invalid inputs or when the trace misses the band.
#[must_use]
pub fn contact([from, to]: [Vec2; 2], [a, b]: [Vec2; 2], [r0, r1]: [f64; 2]) -> Option<Vec2> {
    if ![from, to, a, b]
        .into_iter()
        .flat_map(Vec2::to_array)
        .all(f64::is_finite)
        || ![r0, r1].into_iter().all(|r| r.is_finite() && r >= 0.0)
    {
        return None;
    }
    let trace = to - from;
    let axis = b - a;
    let length_squared = axis.length_squared();
    let (at, speed) = if length_squared > 0.0 {
        (
            (from - a).dot(axis) / length_squared,
            trace.dot(axis) / length_squared,
        )
    } else {
        (0.0, 0.0)
    };

    // The nearest point runs along the axis between the caps. Split where it reaches
    // either endpoint so that both the offset and the radius are linear on each span.
    if !at.is_finite() || !speed.is_finite() {
        return None;
    }
    let [first, second] = if speed == 0.0 {
        [0.0, 1.0]
    } else {
        let mut cuts = [0.0, 1.0].map(|end| ((end - at) / speed).clamp(0.0, 1.0));
        cuts.sort_unstable_by(f64::total_cmp);
        cuts
    };
    let sample = |t: f64| {
        let axis_t = (at + t * speed).clamp(0.0, 1.0);
        (
            from + trace * t - (a + axis * axis_t),
            r0 + (r1 - r0) * axis_t,
        )
    };
    let gap = |(offset, radius): (Vec2, f64)| offset.length_squared() - radius * radius;
    let mut best = None;
    for span in [0.0, first, second, 1.0].windows(2) {
        let &[start, end] = span else { continue };
        if start == end {
            continue;
        }
        let (offset, radius) = sample(start);
        let (end_offset, end_radius) = sample(end);
        let step = end_offset - offset;
        let radius_step = end_radius - radius;
        // Minimize squared distance minus squared radius over the span.
        let curvature = step.length_squared() - radius_step * radius_step;
        let t = if curvature > 0.0 {
            let slope = offset.dot(step) - radius * radius_step;
            start + (end - start) * (-slope / curvature).clamp(0.0, 1.0)
        } else if gap((offset, radius)) <= gap((end_offset, end_radius)) {
            start
        } else {
            end
        };
        let distance = gap(sample(t));
        if distance <= 0.0 && best.is_none_or(|(old, _)| distance < old) {
            best = Some((distance, t));
        }
    }
    best.map(|(_, t)| from + trace * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_thin_crossing_between_trace_endpoints() {
        let trace = [Vec2::new(0.0, -40.0), Vec2::new(0.0, 40.0)];
        let axis = [Vec2::new(-12.0, 0.0), Vec2::new(12.0, 0.0)];
        assert_eq!(contact(trace, axis, [0.1, 0.1]), Some(Vec2::ZERO));
    }

    #[test]
    fn applies_the_local_radius_and_tests_round_caps() {
        let trace = [Vec2::new(0.0, 2.0), Vec2::new(10.0, 2.0)];
        let axis = [Vec2::ZERO, Vec2::new(10.0, 0.0)];
        assert_eq!(contact(trace, axis, [0.5, 3.5]), Some(trace[1]));
        assert_eq!(contact(trace, axis, [0.5, 1.5]), None);
        let cap_trace = [Vec2::new(12.0, -3.0), Vec2::new(12.0, 3.0)];
        assert_eq!(
            contact(cap_trace, axis, [0.5, 2.0]),
            Some(Vec2::new(12.0, 0.0))
        );
    }

    #[test]
    fn tests_stationary_traces_and_collapsed_axes() {
        let trace = [Vec2::new(2.0, 0.5); 2];
        let axis = [Vec2::ZERO, Vec2::new(10.0, 0.0)];
        assert_eq!(contact(trace, axis, [1.0; 2]), Some(trace[0]));
        assert_eq!(contact(trace, [Vec2::ZERO; 2], [1.0, 3.0]), None);
    }
}

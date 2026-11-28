//! The geometry of the "keep edge" round: a concave round taller than the wall it rises on,
//! ending on the wall's top edge instead of running on past it. SolidCraft's kernel layer reads
//! the wall's faces and builds the prism; the measurements it decides on are here.
//!
//! Not upstream: SolidCraft's `keep_edge.rs`, made the kernel's.
//!
//! In the section square to a straight edge, with the floor along x, the wall's top edge at
//! `T = (tx, h)` and the wall rising from the origin, the section is the arc of the circle of
//! radius `R` centred at `(tx + c, R)`, `c = sqrt(R^2 - (R - h)^2)`, from its lowest point
//! `(tx + c, 0)` round to `T` ([`round_section`]).

type V3 = [f64; 3];

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn length(a: V3) -> f64 {
    dot(a, a).sqrt()
}

/// The unit vector from `a` to `b`; `None` where they coincide or are not finite.
#[must_use]
pub fn unit_between(a: V3, b: V3) -> Option<V3> {
    let d = sub(b, a);
    let l = length(d);
    (l.is_finite() && l > 0.0).then(|| [d[0] / l, d[1] / l, d[2] / l])
}

/// The wall's lean against the floor: `(wx, wy)`, the wall's direction square to the edge
/// (`w_dir`, into the wall) along the floor's (`f_dir`, into the floor) and along its normal
/// `nf`; `nw` is the wall's normal. `None` where the corner is not concave (the wall must rise
/// from the floor on its outer side, and the floor run off the wall's).
#[must_use]
pub fn wall_frame(f_dir: V3, w_dir: V3, nf: V3, nw: V3) -> Option<(f64, f64)> {
    let (wx, wy) = (dot(w_dir, f_dir), dot(w_dir, nf));
    if wy <= 1e-9 || dot(f_dir, nw) <= 1e-9 {
        return None;
    }
    Some((wx, wy))
}

/// The height above the floor of point `p`, where the wall (direction `w_dir` from the edge
/// point `a`, rising `wy` per unit of it) is measured.
#[must_use]
pub fn wall_height(a: V3, p: V3, w_dir: V3, wy: f64) -> f64 {
    dot(sub(p, a), w_dir) * wy
}

/// The wall's top edge `t0`..`t1` measured from the edge point `a`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TopEdge {
    /// The distance up the wall of each end of the top edge.
    pub length: f64,
    /// The far end's distance up the wall.
    pub level: f64,
    /// Whether the top edge is parallel to the edge (`along` is its unit direction).
    pub parallel: bool,
}

/// The wall's top edge from `t0` to `t1`; `None` where it has no direction.
#[must_use]
pub fn top_edge(a: V3, t0: V3, t1: V3, w_dir: V3, along: V3) -> Option<TopEdge> {
    let top_dir = unit_between(t0, t1)?;
    Some(TopEdge {
        length: dot(sub(t0, a), w_dir),
        level: dot(sub(t1, a), w_dir),
        parallel: length(cross(top_dir, along)) <= 1e-6,
    })
}

/// The round's section: the wall's top edge at `(tx, h)`, the circle of radius `r` tangent to
/// the floor through it, centred at `(cx, r)`, reaching round from `(cx, 0)` by the angle
/// `theta` (`c` is the run from the top edge to the contact point).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    /// The wall's height.
    pub h: f64,
    /// The top edge's distance along the floor from the wall's foot.
    pub tx: f64,
    /// The run from the top edge to the contact point.
    pub c: f64,
    /// The circle's centre along the floor.
    pub cx: f64,
    /// The angle the arc sweeps from its lowest point to the top edge.
    pub theta: f64,
}

/// The section of the round of radius `r` over a wall whose top edge is `length` up it,
/// leaning `(wx, wy)`; `None` where it is not a circle (the wall is as high as the diameter).
#[must_use]
pub fn round_section(length: f64, wx: f64, wy: f64, r: f64) -> Option<Section> {
    let h = length * wy;
    let tx = length * wx;
    let c2 = r * r - (r - h) * (r - h);
    if c2.is_nan() || c2 < 0.0 {
        return None;
    }
    let c = c2.sqrt();
    Some(Section {
        h,
        tx,
        c,
        cx: tx + c,
        theta: c.atan2(r - h),
    })
}

/// Whether the arc of `section` (radius `r`) stays on the air side of the wall's plane (the
/// wall leaning `(wx, wy)`) to within `tol`.
#[must_use]
pub fn arc_clears_wall(section: &Section, wx: f64, wy: f64, r: f64, tol: f64) -> bool {
    (0..=16).all(|i| {
        let ang = -std::f64::consts::FRAC_PI_2 - section.theta * f64::from(i) / 16.0;
        let (px, py) = (section.cx + r * ang.cos(), r + r * ang.sin());
        px * wy - py * wx >= -tol
    })
}

/// The prism's extent along the edge, in the section plane's normal `f_dir x nf`, from the
/// edge point `a` to `b`, widened by `margin_a` past `a` and `margin_b` past `b` (in the
/// direction of the edge `along`): `(lo, hi)`.
#[must_use]
pub fn prism_span(
    a: V3,
    b: V3,
    f_dir: V3,
    nf: V3,
    along: V3,
    margin_a: f64,
    margin_b: f64,
) -> (f64, f64) {
    let n = cross(f_dir, nf);
    let sign = if dot(n, along) > 0.0 { 1.0 } else { -1.0 };
    let s_b = dot(sub(b, a), n) + sign * margin_b;
    let s_a = -sign * margin_a;
    if s_a <= s_b {
        (s_a, s_b)
    } else {
        (s_b, s_a)
    }
}

/// How the prism ends at an end face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndKind {
    /// A flat face square to the edge: the prism ends in its plane.
    Square,
    /// An upright face: the prism runs on and is kept within the floor's outline.
    Upright,
    /// A face tilted against the floor's normal: not supported.
    Tilted,
}

/// The end of a flat end face of normal `n`, on a floor of normal `nf`, along an edge `along`.
#[must_use]
pub fn plane_end(n: V3, nf: V3, along: V3) -> EndKind {
    if dot(n, nf).abs() > 1e-6 {
        EndKind::Tilted
    } else if length(cross(n, along)) <= 1e-6 {
        EndKind::Square
    } else {
        EndKind::Upright
    }
}

/// Whether a cylinder with frame axes `x_axis`, `y_axis` is about the floor's normal `nf`
/// (an upright end).
#[must_use]
pub fn cylinder_is_upright(x_axis: V3, y_axis: V3, nf: V3) -> bool {
    length(cross(cross(x_axis, y_axis), nf)) <= 1e-6
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_wall_gives_the_closed_form_section() {
        // A square wall of height 1 on a floor, radius 2: c = sqrt(4 - 1) = sqrt(3).
        let s = round_section(1.0, 0.0, 1.0, 2.0).unwrap();
        assert!((s.c - 3f64.sqrt()).abs() < 1e-12);
        assert!((s.cx - 3f64.sqrt()).abs() < 1e-12);
        assert!((s.theta - 3f64.sqrt().atan2(1.0)).abs() < 1e-12);
        assert!(arc_clears_wall(&s, 0.0, 1.0, 2.0, 1e-9));
        assert!(round_section(5.0, 0.0, 1.0, 2.0).is_none());
    }

    #[test]
    fn the_wall_is_concave_only_when_it_rises_off_the_floor() {
        let (f, nf) = ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        assert_eq!(
            wall_frame(f, [0.0, 0.0, 1.0], nf, [1.0, 0.0, 0.0]),
            Some((0.0, 1.0))
        );
        assert_eq!(wall_frame(f, [0.0, 0.0, -1.0], nf, [1.0, 0.0, 0.0]), None);
    }

    #[test]
    fn ends_are_classified_by_their_normal() {
        let (nf, along) = ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
        assert_eq!(plane_end([0.0, 1.0, 0.0], nf, along), EndKind::Square);
        assert_eq!(plane_end([1.0, 0.0, 0.0], nf, along), EndKind::Upright);
        assert_eq!(plane_end([0.0, 0.6, 0.8], nf, along), EndKind::Tilted);
        assert!(cylinder_is_upright([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], nf));
        assert!(!cylinder_is_upright([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], nf));
    }

    #[test]
    fn the_prism_span_widens_past_each_end() {
        let (a, b) = ([0.0; 3], [0.0, 10.0, 0.0]);
        let (f, nf, along) = ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
        // f x nf = -y, so the span runs from the far end back.
        let (lo, hi) = prism_span(a, b, f, nf, along, 2.0, 3.0);
        assert!((lo - -13.0).abs() < 1e-12 && (hi - 2.0).abs() < 1e-12);
    }
}

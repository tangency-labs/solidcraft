//! Interior isoparm lines of a sheet for display: on each face, lines of constant u and of
//! constant v evenly spaced inside its parameter box, clipped to the face's boundary and sampled
//! by the rules a free curve is ([`crate::curve_points`]). The surfaces and boundaries are the
//! ones the mesher plans the face with ([`crate::mesher::prepare`]), so the lines lie on what is
//! drawn and stop where its boundary does. Display points, not geometry: drawn, never measured
//! or modelled with.

use solidcore::Vec3;
use solidcore::eval::plan::EvalSurface;
use solidcore_record::brep::Brep;

use crate::mesher::edges::bisect;
use crate::mesher::size::Rules;
use crate::mesher::{Polygons, comp, polygons, prepare, uv};
use crate::vec::VecExt as _;
use crate::{MeshTolerance, Result, TessError};

/// Most isoparms one direction of a face may ask for.
pub const MAX_ISOPARMS: usize = 1024;

/// Most copies, a period apart, of a line on a closed direction that can cross one face.
const MAX_COPIES: usize = 8;

/// The interior isoparms of every face of `sheet`: on each face, `u_count` lines of constant u
/// at `u0 + i (u1 - u0) / (u_count + 1)`, `i` from 1 to `u_count`, over its parameter box
/// `[u0, u1] × [v0, v1]` (the record's `domain`), then `v_count` lines of constant v likewise.
/// Each line is cut where it crosses the face's boundary (inside its outer loop, outside its
/// holes: one polyline per run inside) and sampled from end to end with every chord within
/// `tolerance` of the surface, as [`crate::curve_points`] samples a curve. `size` is the body's
/// size (its box diagonal) as [`crate::plan`] takes it, so the tolerance is the plan's (never
/// finer than a millionth of the body) and the boundary is the one the plan draws: its edges'
/// chords, which a run ends on, within the tolerance of the trim. A face the
/// mesher leaves out of its plan gets no lines (one it cannot read, close, or tell the poles
/// of).
///
/// The polylines are display points, not geometry: points on the surface for drawing, each
/// chord within the tolerance, never a curve to model with or measure (an isoparm as geometry
/// is the kernel's iso curve of the surface).
///
/// # Errors
///
/// [`TessError::Invalid`] for more than [`MAX_ISOPARMS`] lines a direction; the mesher's errors
/// for a body none of whose faces can be read; [`TessError::Failed`] for a face whose surface
/// does not evaluate on a line.
pub fn isoparms(
    sheet: &Brep,
    u_count: usize,
    v_count: usize,
    tolerance: MeshTolerance,
    size: f64,
) -> Result<Vec<Vec<[f64; 3]>>> {
    if u_count > MAX_ISOPARMS || v_count > MAX_ISOPARMS {
        return Err(TessError::Invalid(format!(
            "isoparms: {u_count} by {v_count} lines, at most {MAX_ISOPARMS} a direction"
        )));
    }
    let mut out = Vec::new();
    if u_count == 0 && v_count == 0 {
        return Ok(out);
    }
    let (tol, turn) = tolerance.rules(size);
    let prepared = prepare(sheet, tol, turn)?;
    let rules = Rules {
        tol,
        max_len: f64::INFINITY,
        turn,
    };
    for (fi, face) in sheet.faces.iter().enumerate() {
        if prepared.why.get(fi).is_some_and(Option::is_some) {
            continue;
        }
        let (Some(s), Some(map), Some(&(h, _))) = (
            prepared.evals.get(fi),
            prepared.params.get(fi),
            prepared.sizes.get(fi),
        ) else {
            continue;
        };
        // The boundary as the planner triangulates it: its pole and seam fixes made. One the
        // planner cannot close leaves the face unplanned too.
        let Ok(poly) = polygons(
            s,
            prepared.loops(fi),
            &prepared.rules,
            h,
            prepared.reach(fi),
        ) else {
            continue;
        };
        // A point whose poles the kernel could not tell refuses the face in the plan
        // ([`crate::plan`]): it gets no lines either.
        if s.refusal().is_some() {
            continue;
        }
        let period = uv::domain(s).period;
        let [[u0, u1], [v0, v1]] = face.domain;
        for (count, constant_u) in [(u_count, true), (v_count, false)] {
            for i in 1..=count {
                let f = i as f64 / (count + 1) as f64;
                // The line's ends on the parameter box, in the surface's parameters.
                let (a, b) = if constant_u {
                    let u = u0 + f * (u1 - u0);
                    (map.to_eval([u, v0]), map.to_eval([u, v1]))
                } else {
                    let v = v0 + f * (v1 - v0);
                    (map.to_eval([u0, v]), map.to_eval([u1, v]))
                };
                // The surface's parameter the line keeps (the maps only copy, negate or
                // reparametrise one coordinate at a time, so one is kept exactly).
                let d = match (a[0] == b[0], a[1] == b[1]) {
                    (true, false) => 0,
                    (false, true) => 1,
                    _ => continue,
                };
                let per = period.get(d).copied().flatten();
                for c in copies(&poly.uvs, d, comp(&a, d), per) {
                    for run in inside(&poly, d, c) {
                        out.push(sample(s, fi, d, c, run, &rules)?);
                    }
                }
            }
        }
    }
    Ok(out)
}

/// The values of the surface's parameter `d`, `c` and its copies a whole `period` along, that
/// lie within the boundary's extent in `d` (`c` alone on an open direction).
fn copies(uvs: &[[f64; 2]], d: usize, c: f64, period: Option<f64>) -> Vec<f64> {
    let Some(per) = period.filter(|p| p.is_finite() && *p > 0.0) else {
        return vec![c];
    };
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for q in uvs {
        lo = lo.min(comp(q, d));
        hi = hi.max(comp(q, d));
    }
    if lo > hi {
        return Vec::new();
    }
    let first = c + per * ((lo - c) / per).ceil();
    (0..MAX_COPIES)
        .map(|k| first + per * k as f64)
        .take_while(|x| *x < hi)
        .collect()
}

/// The runs of the line where the surface's parameter `d` is `c` that lie inside the boundary `poly`, by the
/// even-odd rule: increasing intervals of the other parameter.
fn inside(poly: &Polygons, d: usize, c: f64) -> Vec<[f64; 2]> {
    let o = usize::from(d == 0);
    let mut at: Vec<f64> = Vec::new();
    for range in &poly.ranges {
        let Some(nodes) = poly.uvs.get(range.clone()) else {
            continue;
        };
        let next = nodes.iter().cycle().skip(1);
        for (p, q) in nodes.iter().zip(next) {
            let (pd, qd) = (comp(p, d), comp(q, d));
            // half open, so a node on the line is crossed once or not at all
            if (pd <= c) != (qd <= c) {
                let t = (c - pd) / (qd - pd);
                at.push(comp(p, o) + t * (comp(q, o) - comp(p, o)));
            }
        }
    }
    at.sort_by(f64::total_cmp);
    at.as_chunks::<2>()
        .0
        .iter()
        .filter(|[x0, x1]| x1 > x0)
        .copied()
        .collect()
}

/// The points of the run `[x0, x1]` of the line where parameter `d` of face `fi`'s surface `s`
/// is `c`, every chord within the rules.
fn sample(
    s: &EvalSurface,
    fi: usize,
    d: usize,
    c: f64,
    [x0, x1]: [f64; 2],
    rules: &Rules,
) -> Result<Vec<[f64; 3]>> {
    let at = |x: f64| -> Result<Vec3> {
        let q = if d == 0 { [c, x] } else { [x, c] };
        s.derivs(q[0], q[1])
            .map(|p| p.0)
            .filter(|p| p.is_finite())
            .ok_or_else(|| {
                TessError::Failed(format!(
                    "isoparms: face {fi}: the surface does not evaluate at {q:?}"
                ))
            })
    };
    let ends = [at(x0)?, at(x1)?];
    let [front, back] = ends;
    let initial = if front.dist(back) <= rules.tol { 4 } else { 1 };
    let nodes = bisect(&at, [x0, x1], ends, initial, rules, f64::INFINITY)?;
    Ok(nodes.points.iter().map(|p| [p.x, p.y, p.z]).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{fixture, size_of};
    use solidcore_record::brep::Surface;

    fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    /// How far `p` is from the segment `a`–`b`.
    fn off_segment(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
        let (ab, ap) = (sub(b, a), sub(p, a));
        let t = (dot(ap, ab) / dot(ab, ab)).clamp(0.0, 1.0);
        let q = [a[0] + t * ab[0], a[1] + t * ab[1], a[2] + t * ab[2]];
        dot(sub(p, q), sub(p, q)).sqrt()
    }

    /// Whether `p` lies on an edge of `b` (all of `b`'s edges straight).
    fn on_an_edge(b: &Brep, p: [f64; 3]) -> bool {
        b.edges.iter().any(|e| {
            let [a, c] = e.vertices.map(|v| b.vertices[v as usize].point);
            off_segment(p, a, c) < 1e-9
        })
    }

    #[test]
    fn a_boxs_lines_are_at_their_parameters_and_end_on_its_edges() {
        let (b, size) = (fixture("box_10x6x4"), size_of("box_10x6x4"));
        let lines = isoparms(&b, 3, 0, MeshTolerance::chord(0.01), size).unwrap();
        assert!(lines.len() >= 12, "{}", lines.len());
        for line in &lines {
            // a plane's lines are straight: one chord
            assert_eq!(line.len(), 2, "{line:?}");
            let (p, q) = (line[0], line[1]);
            assert!(on_an_edge(&b, p) && on_an_edge(&b, q), "{line:?}");
            // on the face whose plane holds it, at one of its three u
            let at_its_u = b.faces.iter().any(|f| {
                let Surface::Plane {
                    origin,
                    u_axis,
                    v_axis,
                } = f.parameters()
                else {
                    return false;
                };
                let n = cross(*u_axis, *v_axis);
                let on_plane = |x: [f64; 3]| dot(sub(x, *origin), n).abs() < 1e-9;
                let u = |x: [f64; 3]| dot(sub(x, *origin), *u_axis) / dot(*u_axis, *u_axis);
                let [u0, u1] = f.domain[0];
                on_plane(p)
                    && on_plane(q)
                    && (u(p) - u(q)).abs() < 1e-9
                    && (1..=3).any(|i| (u(p) - (u0 + f64::from(i) * (u1 - u0) / 4.0)).abs() < 1e-9)
            });
            assert!(at_its_u, "{line:?}");
        }
        // none asked, none drawn; too many refused
        assert!(
            isoparms(&b, 0, 0, MeshTolerance::chord(0.01), size)
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            isoparms(&b, MAX_ISOPARMS + 1, 0, MeshTolerance::chord(0.01), size),
            Err(TessError::Invalid(_))
        ));
    }

    /// `p`'s distance from the axis of a face's cylinder, and its radius.
    fn radial(s: &Surface, p: [f64; 3]) -> Option<(f64, f64)> {
        let Surface::Cylinder { frame, radius } = s else {
            return None;
        };
        let w = sub(p, frame.origin);
        let (x, y) = (dot(w, frame.x_axis), dot(w, frame.y_axis));
        Some((x.hypot(y), *radius))
    }

    #[test]
    fn lines_stop_at_a_hole() {
        let (b, size) = (fixture("box_with_hole"), size_of("box_with_hole"));
        let tol = 0.01;
        let lines = isoparms(&b, 7, 7, MeshTolerance::chord(tol), size).unwrap();
        let hole = b
            .faces
            .iter()
            .find(|f| matches!(f.surface, Surface::Cylinder { .. }))
            .unwrap();
        // ends on the hole's rim: on its sampled circle, within the tolerance inside it
        let on_rim = |p: [f64; 3]| {
            let (r, radius) = radial(&hole.surface, p).unwrap();
            (radius - tol * 1.01..=radius + 1e-9).contains(&r)
        };
        let at_hole = lines
            .iter()
            .filter(|l| on_rim(l[0]) || on_rim(l[l.len() - 1]))
            .count();
        assert!(at_hole > 0, "no line ends at the hole");
        // and no line crosses it: no chord's middle in the hole on the faces it cuts
        for l in &lines {
            for w in l.windows(2) {
                let m = [
                    (w[0][0] + w[1][0]) / 2.0,
                    (w[0][1] + w[1][1]) / 2.0,
                    (w[0][2] + w[1][2]) / 2.0,
                ];
                let (r, radius) = radial(&hole.surface, m).unwrap();
                let on_wall = on_rim(w[0]) && on_rim(w[1]);
                assert!(
                    on_wall || r >= radius - tol * 1.01,
                    "{w:?} crosses the hole"
                );
            }
        }
    }

    #[test]
    fn a_cylinders_lines_sag_within_the_tolerance() {
        let (b, size) = (fixture("cylinder_r10_h5"), size_of("cylinder_r10_h5"));
        let side = b
            .faces
            .iter()
            .find(|f| matches!(f.surface, Surface::Cylinder { .. }))
            .unwrap();
        for tol in [0.1, 0.01] {
            let lines = isoparms(&b, 4, 4, MeshTolerance::chord(tol), size).unwrap();
            let mut round = 0;
            for l in &lines {
                let on_side = l.iter().all(|p| {
                    let (r, radius) = radial(&side.surface, *p).unwrap();
                    (r - radius).abs() < 1e-6
                });
                if !on_side || l.len() < 3 {
                    continue;
                }
                round += 1;
                for w in l.windows(2) {
                    let m = [
                        (w[0][0] + w[1][0]) / 2.0,
                        (w[0][1] + w[1][1]) / 2.0,
                        (w[0][2] + w[1][2]) / 2.0,
                    ];
                    let (r, radius) = radial(&side.surface, m).unwrap();
                    assert!(radius - r <= tol * 1.0001, "sag {} at {tol}", radius - r);
                }
            }
            assert!(round > 0, "no line round the cylinder at {tol}");
        }
    }

    /// The centre and radius of `b`'s sphere face.
    fn sphere(b: &Brep) -> ([f64; 3], f64) {
        b.faces
            .iter()
            .find_map(|f| match &f.surface {
                Surface::Sphere { frame, radius } => Some((frame.origin, *radius)),
                _ => None,
            })
            .unwrap()
    }

    /// The lines of `b` that lie on its sphere (every point at its radius), each checked: its
    /// chords sag at most `tol` inside the sphere.
    fn sphere_lines(b: &Brep, lines: &[Vec<[f64; 3]>], tol: f64) -> Vec<Vec<[f64; 3]>> {
        let (c, r) = sphere(b);
        let from_centre = |p: [f64; 3]| dot(sub(p, c), sub(p, c)).sqrt();
        let on: Vec<Vec<[f64; 3]>> = lines
            .iter()
            .filter(|l| l.iter().all(|p| (from_centre(*p) - r).abs() < 1e-6))
            .cloned()
            .collect();
        for l in &on {
            for w in l.windows(2) {
                let m = [0, 1, 2].map(|i| (w[0][i] + w[1][i]) / 2.0);
                let sag = r - from_centre(m);
                assert!((-1e-9..=tol * 1.0001).contains(&sag), "sag {sag} on {w:?}");
            }
        }
        on
    }

    #[test]
    fn a_whole_spheres_lines_are_one_run_each() {
        let (b, size) = (fixture("sphere_r12"), size_of("sphere_r12"));
        let tol = 0.01;
        let lines = isoparms(&b, 5, 3, MeshTolerance::chord(tol), size).unwrap();
        // one face, its boundary a seam from pole to pole: every line is one run
        assert_eq!(lines.len(), 8, "{}", lines.len());
        let on = sphere_lines(&b, &lines, tol);
        assert_eq!(on.len(), 8);
        // the meridians run from pole to pole, the parallels close
        let pole = |p: [f64; 3]| p[0].hypot(p[1]) < 1e-6;
        let meridians = on
            .iter()
            .filter(|l| pole(l[0]) && pole(l[l.len() - 1]))
            .count();
        let rounds = on
            .iter()
            .filter(|l| dot(sub(l[0], l[l.len() - 1]), sub(l[0], l[l.len() - 1])) < 1e-12)
            .count();
        assert_eq!(
            meridians + rounds,
            8,
            "{meridians} meridians, {rounds} round"
        );
        assert!(meridians > 0 && rounds > 0);
    }

    #[test]
    fn a_half_spheres_lines_stay_on_its_half() {
        let (b, size) = (fixture("smlib_half_sphere"), size_of("smlib_half_sphere"));
        let tol = 0.01;
        let lines = isoparms(&b, 4, 4, MeshTolerance::chord(tol), size).unwrap();
        let on = sphere_lines(&b, &lines, tol);
        assert!(!on.is_empty());
        // every point of the sphere's lines on one side of the rim's plane z = 0
        let zs: Vec<f64> = on.iter().flatten().map(|p| p[2]).collect();
        assert!(
            zs.iter().all(|z| *z >= -1e-9) || zs.iter().all(|z| *z <= 1e-9),
            "both sides of the rim"
        );
        // and a meridian reaches the pole
        assert!(on.iter().flatten().any(|p| p[0].hypot(p[1]) < 1e-6));
    }

    #[test]
    fn a_pocketed_spheres_lines_lie_on_it() {
        let (b, size) = (
            fixture("sphere_r10_with_pocket"),
            size_of("sphere_r10_with_pocket"),
        );
        let tol = 0.01;
        let lines = isoparms(&b, 6, 6, MeshTolerance::chord(tol), size).unwrap();
        let on = sphere_lines(&b, &lines, tol);
        assert!(on.len() >= 6, "{}", on.len());
    }

    #[test]
    fn the_tolerance_is_the_plans_for_the_bodys_size() {
        // a chord of 1e-9 on a body a million across is the plan's millionth of it: 1
        let b = fixture("cylinder_r10_h5");
        let fine = isoparms(&b, 0, 2, MeshTolerance::chord(1e-3), 0.0).unwrap();
        let coarse = isoparms(&b, 0, 2, MeshTolerance::chord(1e-9), 1e6).unwrap();
        let count = |ls: &[Vec<[f64; 3]>]| ls.iter().map(Vec::len).sum::<usize>();
        assert!(
            count(&coarse) < count(&fine),
            "{} {}",
            count(&coarse),
            count(&fine)
        );
        let side = b
            .faces
            .iter()
            .find(|f| matches!(f.surface, Surface::Cylinder { .. }))
            .unwrap();
        let on_side = |p: &[f64; 3]| {
            let (r, radius) = radial(&side.surface, *p).unwrap();
            (r - radius).abs() < 1e-6
        };
        for l in coarse.iter().filter(|l| l.iter().all(on_side)) {
            for w in l.windows(2) {
                let m = [0, 1, 2].map(|i| (w[0][i] + w[1][i]) / 2.0);
                let (r, radius) = radial(&side.surface, m).unwrap();
                assert!(radius - r <= 1.0001, "sag {}", radius - r);
            }
        }
    }
}

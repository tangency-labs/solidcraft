//! Interior isoparm lines of a sheet for display: on each face, lines of constant u and of
//! constant v evenly spaced inside its parameter box, drawn on the face's display triangles. The
//! plan the mesh is evaluated from ([`crate::plan`], [`crate::TessPlan::evaluate`]) gives each
//! face's triangles in its (u, v) and their corners' points; a line is cut by the triangles it
//! crosses in (u, v), and each piece mapped linearly onto its triangle. So the lines lie on the
//! shaded triangles, never behind them where the surface curves away from the chords (a concave
//! neck), and stop where the face's triangles do. Display points, not geometry: drawn, never
//! measured or modelled with.

use solidcore_record::Vec3;
use solidcore_record::brep::Brep;

use crate::mesher::comp;
use crate::mesher::surface::eval_surface;
use crate::mesher::uv;
use crate::{MeshTolerance, Result, TessError, plan};

/// Most isoparms one direction of a face may ask for.
pub const MAX_ISOPARMS: usize = 1024;

/// Most copies, a period apart, of a line on a closed direction that can cross one face.
const MAX_COPIES: usize = 8;

/// The interior isoparms of every face of `sheet`, on its display mesh: on each face,
/// `u_count` lines of constant u at `u0 + i (u1 - u0) / (u_count + 1)`, `i` from 1 to
/// `u_count`, over its parameter box `[u0, u1] × [v0, v1]` (the record's `domain`), then
/// `v_count` lines of constant v likewise. `tolerance` and `size` are the plan's
/// ([`crate::plan`]): the lines are drawn on the triangles of the mesh planned with them. Each
/// line is cut by the face's triangles in (u, v) and each piece mapped linearly onto its
/// triangle, so every point lies on a triangle of the evaluated mesh, and the lines follow the
/// surface as closely as the mesh does; one polyline per run of triangles it crosses, joined
/// where consecutive triangles share a side or node (a line broken by a hole or a trim gives
/// one each side), and carried along a side into a pole the line runs into (a sphere's
/// meridian reaches both poles). A face the plan leaves out, or whose nodes
/// the evaluator refuses, gets no lines.
///
/// The polylines are display points, not geometry: points on the display mesh for drawing,
/// never a curve to model with or measure (an isoparm as geometry is the kernel's iso curve of
/// the surface).
///
/// # Errors
///
/// [`TessError::Invalid`] for more than [`MAX_ISOPARMS`] lines a direction; those of
/// [`crate::plan`].
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
    let (planned, _) = plan(sheet, &tolerance, size)?;
    // a node within the plan's chord tolerance of a collapsed side is a pole of the display, as
    // the planner bounds its own pole reach by it
    let (reach, _) = tolerance.rules(size);
    for fp in &planned.faces {
        let Some(face) = sheet.faces.get(fp.face as usize) else {
            continue;
        };
        // the triangles the mesh keeps (on the face's own nodes), and their corners' points
        let triangles: Vec<[u32; 3]> = fp
            .triangles
            .iter()
            .filter(|t| t.iter().all(|&i| (i as usize) < fp.uv.len()))
            .copied()
            .collect();
        if triangles.is_empty() {
            continue;
        }
        let nodes = planned.nodes(fp);
        if nodes.failure.is_some() {
            continue;
        }
        // the record's parameters to the surface's the plan's (u, v) are in
        let Ok((surface, map)) = eval_surface(face.parameters(), face.domain) else {
            continue;
        };
        let period = uv::domain(&surface).period;
        // the surface's poles, by the planner's classifier
        let poles = crate::mesher::poles::Face::new(surface, 0.0);
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
                // a node where the surface is collapsed along d: the line's every value of d
                // is that point there
                let pole = |i: u32| {
                    fp.uv
                        .get(i as usize)
                        .is_some_and(|q| poles.pole(*q, d, reach))
                };
                let mesh = Triangles {
                    uvs: &fp.uv,
                    at: &nodes.positions,
                    corners: &triangles,
                };
                for c in copies(&fp.uv, d, comp(&a, d), per) {
                    out.extend(mesh.line(d, c, &pole));
                }
            }
        }
    }
    Ok(out)
}

/// The values of the surface's parameter `d`, `c` and its copies a whole `period` along, that
/// lie within the face's extent in `d` (`c` alone on an open direction).
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

/// Where a line crosses a triangle's side: at one of its nodes, or inside the side between two.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Crossing {
    /// At node `i`.
    Node(u32),
    /// Inside the side between nodes `i < j`.
    Side(u32, u32),
}

impl Crossing {
    /// The nodes the crossing is on.
    fn nodes(self) -> [Option<u32>; 2] {
        match self {
            Crossing::Node(i) => [Some(i), None],
            Crossing::Side(i, j) => [Some(i), Some(j)],
        }
    }
}

/// One triangle's piece of a line: its ends' parameter along the line, where they cross the
/// triangle's sides, and their points.
struct Piece {
    x: [f64; 2],
    at: [Crossing; 2],
    points: [Vec3; 2],
}

/// A face's display triangles in (u, v): its nodes' parameters and points, and the triangles.
struct Triangles<'a> {
    uvs: &'a [[f64; 2]],
    at: &'a [Vec3],
    /// Each triangle's three nodes.
    corners: &'a [[u32; 3]],
}

impl Triangles<'_> {
    /// The line where parameter `d` is `c` on the triangles: each triangle it crosses gives
    /// the piece between the two sides it crosses, its ends the sides' points at the crossing
    /// (straight on its flat triangle). A side is crossed by the half-open rule (one end at or
    /// below `c`, the other above), so a line through a node or along a side between two
    /// triangles is taken once. Pieces join where one ends on the very side or node of the mesh
    /// the next starts on (two triangles sharing it), never by distance. A polyline that ends on
    /// a side at a node where the surface collapses along `d` (`pole`: a pole the line runs into,
    /// every value of `d` the same point there) is carried along that side to the node, so a
    /// line into a pole reaches it.
    fn line(&self, d: usize, c: f64, pole: &dyn Fn(u32) -> bool) -> Vec<Vec<[f64; 3]>> {
        let o = usize::from(d == 0);
        let corner = |i: u32| Some((*self.uvs.get(i as usize)?, *self.at.get(i as usize)?));
        let mut pieces: Vec<Piece> = Vec::new();
        for &[i0, i1, i2] in self.corners {
            let (Some(a), Some(b), Some(e)) = (corner(i0), corner(i1), corner(i2)) else {
                continue;
            };
            let mut hits: Vec<(f64, Crossing, Vec3)> = Vec::with_capacity(2);
            for ((ip, p), (iq, q)) in [((i0, a), (i1, b)), ((i1, b), (i2, e)), ((i2, e), (i0, a))] {
                let (pd, qd) = (comp(&p.0, d), comp(&q.0, d));
                if (pd <= c) != (qd <= c) {
                    let s = (c - pd) / (qd - pd);
                    let x = comp(&p.0, o) + s * (comp(&q.0, o) - comp(&p.0, o));
                    // exactly at a node (s is exactly 0 or 1 there): that node, its own point
                    let (at, point) = if s == 0.0 {
                        (Crossing::Node(ip), p.1)
                    } else if s == 1.0 {
                        (Crossing::Node(iq), q.1)
                    } else {
                        (
                            Crossing::Side(ip.min(iq), ip.max(iq)),
                            p.1 + (q.1 - p.1) * s,
                        )
                    };
                    hits.push((x, at, point));
                }
            }
            if let [(x0, a0, p0), (x1, a1, p1)] = hits.as_slice() {
                if x0 < x1 {
                    pieces.push(Piece {
                        x: [*x0, *x1],
                        at: [*a0, *a1],
                        points: [*p0, *p1],
                    });
                } else if x1 < x0 {
                    pieces.push(Piece {
                        x: [*x1, *x0],
                        at: [*a1, *a0],
                        points: [*p1, *p0],
                    });
                }
            }
        }
        pieces.sort_by(|a, b| a.x[0].total_cmp(&b.x[0]));
        // each piece by where it starts; a piece no other ends where it starts heads a polyline
        let mut starting: std::collections::HashMap<Crossing, Vec<usize>> =
            std::collections::HashMap::new();
        for (k, piece) in pieces.iter().enumerate() {
            starting.entry(piece.at[0]).or_default().push(k);
        }
        let ends: std::collections::HashSet<Crossing> = pieces.iter().map(|p| p.at[1]).collect();
        let mut used = vec![false; pieces.len()];
        let xyz = |p: Vec3| [p.x, p.y, p.z];
        let mut lines = Vec::new();
        for head in 0..pieces.len() {
            let Some(first) = pieces.get(head) else {
                continue;
            };
            if used.get(head).copied().unwrap_or(true) || ends.contains(&first.at[0]) {
                continue;
            }
            let mut line = Vec::new();
            if let Some(p) = self.to_pole(first.at[0], first.x[0], o, false, pole) {
                line.push(xyz(p));
            }
            line.push(xyz(first.points[0]));
            let mut k = head;
            while let Some(piece) = pieces.get(k) {
                if let Some(u) = used.get_mut(k) {
                    *u = true;
                }
                line.push(xyz(piece.points[1]));
                let next = starting
                    .get(&piece.at[1])
                    .and_then(|ks| ks.iter().find(|&&j| !used.get(j).copied().unwrap_or(true)));
                if let Some(&j) = next {
                    k = j;
                } else {
                    if let Some(p) = self.to_pole(piece.at[1], piece.x[1], o, true, pole) {
                        line.push(xyz(p));
                    }
                    break;
                }
            }
            line.dedup();
            if line.len() >= 2 {
                lines.push(line);
            }
        }
        lines
    }

    /// The point of a node of `at` where the surface collapses along the line (`pole`) and that
    /// lies on along the line from `x` (beyond it at the line's `end`, before it at its start),
    /// if there is one: the pole the line runs into along that side.
    fn to_pole(
        &self,
        at: Crossing,
        x: f64,
        o: usize,
        end: bool,
        pole: &dyn Fn(u32) -> bool,
    ) -> Option<Vec3> {
        at.nodes().into_iter().flatten().find_map(|i| {
            let q = self.uvs.get(i as usize)?;
            let ahead = if end {
                comp(q, o) >= x
            } else {
                comp(q, o) <= x
            };
            if ahead && pole(i) {
                self.at.get(i as usize).copied()
            } else {
                None
            }
        })
    }
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
            // a plane's lines are straight: every point on the chord between its ends
            let (p, q) = (line[0], line[line.len() - 1]);
            assert!(
                line.iter().all(|x| off_segment(*x, p, q) < 1e-9),
                "{line:?}"
            );
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
                    (r - radius).abs() <= tol * 1.0001
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
            .filter(|l| {
                l.iter()
                    .all(|p| (from_centre(*p) - r).abs() <= tol * 1.0001)
            })
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
            (r - radius).abs() <= 1.0001
        };
        for l in coarse.iter().filter(|l| l.iter().all(on_side)) {
            for w in l.windows(2) {
                let m = [0, 1, 2].map(|i| (w[0][i] + w[1][i]) / 2.0);
                let (r, radius) = radial(&side.surface, m).unwrap();
                assert!(radius - r <= 1.0001, "sag {}", radius - r);
            }
        }
    }

    /// Whether `p` lies on a triangle of `mesh` within `eps`: in its plane and inside it.
    fn on_mesh(mesh: &crate::Mesh, p: [f64; 3], eps: f64) -> bool {
        mesh.triangles.iter().any(|t| {
            let [a, b, c] = t.map(|i| {
                let v = mesh.positions[i as usize];
                [v.x, v.y, v.z]
            });
            let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
            let n = cross(ab, ac);
            let area = dot(n, n).sqrt();
            if area == 0.0 || dot(ap, n).abs() > eps * area {
                return false;
            }
            // the barycentric coordinates of p, within eps of the triangle's sides
            let (d00, d01, d11) = (dot(ab, ab), dot(ab, ac), dot(ac, ac));
            let (d20, d21) = (dot(ap, ab), dot(ap, ac));
            let den = d00 * d11 - d01 * d01;
            let v = (d11 * d20 - d01 * d21) / den;
            let w = (d00 * d21 - d01 * d20) / den;
            let slack = eps * (d00.max(d11)).sqrt() / area;
            v >= -slack && w >= -slack && v + w <= 1.0 + slack
        })
    }

    #[test]
    fn a_concave_necks_lines_lie_on_its_triangles() {
        // the inside of a torus is a neck: concave along its tube's circles, where the true
        // surface lies behind the chords; every isoparm point on a displayed triangle
        let (b, size) = (fixture("torus_22_4"), size_of("torus_22_4"));
        let t = MeshTolerance::display_of(size);
        let lines = isoparms(&b, 6, 6, t, size).unwrap();
        assert!(lines.len() >= 12, "{}", lines.len());
        let (mesh, _) = plan(&b, &t, size).unwrap().0.evaluate();
        for l in &lines {
            for p in l {
                assert!(on_mesh(&mesh, *p, 1e-9), "{p:?} off the mesh");
            }
        }
    }
}

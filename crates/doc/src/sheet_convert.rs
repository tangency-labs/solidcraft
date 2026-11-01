//! Convert to Sheet Metal with bend edges: the faces of a solid, or the walls of a shelled one,
//! become one sheet.
//!
//! The planar faces reached from the fixed face across the bend edges are the sheet's panels,
//! all on one skin of it (the one the fixed face is on). A body with a parallel face the same
//! small distance behind the fixed face and behind the faces at every bend is a shell: it keeps
//! its walls and their thickness. Any other body's faces become the outside of the sheet, with
//! the rule's thickness taken into the body. A bend edge stays its bend's virtual sharp: the
//! panels on either side are set back from it by (R + T)·tan(θ/2) where the skin is the bend's
//! outside and R·tan(θ/2) where it is the inside, so the body keeps its envelope. An edge between
//! two panels that is no bend is cut open: each panel ends square to the sheet, half the rule's
//! gap from the plane halfway between the two.
//!
//! The bends on the fixed face are finished as the flanges of one edge flange feature are: where
//! two meet at a corner of it, their walls are closed with the rule's gap and the corner gets the
//! rule's corner relief (round, rectangular, a tear, or the notch the bends leave), and where one
//! ends beside more of the fixed face, the rule's bend relief is cut there. A wall that stands on
//! part of an edge of the fixed face (its outside in one plane with the plate's edge, which runs
//! on past it a strip no taller than the bend's setback) bends only where it stands: the strip is
//! the fixed face's own edge.
//!
//! A bend can also be a chamfer: a flat strip between two faces, as a model without bend radii
//! draws a bend (the chord of its arc). Picked in its middle, the strip is no face of the sheet:
//! the faces on either side are taken out to where their planes meet, and that line is the
//! bend's virtual sharp, bent with the rule's radius at the angle between the two faces.
//!
//! A bend modelled with its radius, a cylinder between two planar faces tangent to it along its
//! length, is picked anywhere on its cylinder, on either skin, and taken the same way: the two
//! faces out to their virtual sharp, bent there with the rule's radius and K-factor. The
//! modelled radius gives way to the rule's, as a sharp edge's does: the flat is measured from
//! the virtual sharps, which don't depend on it.
//!
//! The result is the model every sheet metal feature builds ([`SheetBody`]): the fixed face is
//! the base panel and every other panel a flange on its parent's edge, its outline taken through
//! the inverse of its fold. Unfold, the flat pattern, the bend table and cuts then work as on a
//! sheet made from flanges.
//!
//! A face's outline is read exactly, as a plate's is: where it runs along a round wall standing
//! square on the face (a rounded corner, a tooth) it is an arc of the wall's circle, and its
//! holes and slots are cut-outs of the sheet where they lie in its panel. Its pockets (an
//! engraved label: an inner loop whose walls end on flat floors, at most half the sheet deep)
//! are pockets of the sheet from that skin, round their islands; on a shell the faces of the
//! other skin are read for theirs.
//!
//! Not supported yet: curved faces other than bends (rolled sheet), holes and pockets that reach
//! into a bend, pockets deeper than half the sheet or with a step or a hole in their floor, round
//! edges that end at a bend or a cut edge, loops of bends, two faces that meet along more than
//! one edge, a wall on part of an edge of a face other than the fixed one.

use std::collections::BTreeMap;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

use solidcraft_geom::{Loop2, Mesh, Plane, Region2, Ring, Seg2, UnitVec3, Vec2, Vec3};
use solidcraft_kernel::{Body, FaceSurface, face_surface};

use super::{FlatWork, ReliefOverride, RuleValues, SheetBody, SheetFlange, base_flange, polys_cross, signed_area, snap};
use crate::{DocError, ModelBody, Result, apply_point, mat_inverse};

const MAX_BENDS: usize = 1000;

fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(DocError::Invalid(msg.into()))
}

fn at(p: Vec3) -> String {
    format!("({:.3}, {:.3}, {:.3})", p.x, p.y, p.z)
}

/// A planar face of the body: its outward normal and its outline, counter-clockwise about the
/// normal. Edge `i` runs from corner `i` to the next one, along face `across[i]`: straight, or an
/// arc where that face is a round wall (`round[i]`: the circle's centre and the arc's middle).
/// `holes`: its inner loops, in `plane`, each with the faces of its walls.
struct Face {
    n: UnitVec3,
    pts: Vec<Vec3>,
    across: Vec<Option<usize>>,
    round: Vec<Option<(Vec3, Vec3)>>,
    plane: Plane,
    holes: Vec<(Loop2, Vec<usize>)>,
}

/// A face of the sheet: the panel it hangs off (none for the fixed face), and the bend between
/// the two: its angle (positive toward the material, as a flange's) and its setback on this skin.
struct Panel {
    face: Face,
    parent: Option<usize>,
    angle: f64,
    setback: f64,
}

/// Area of a loop in space, positive when it runs counter-clockwise about `n`.
fn area_about(pts: &[Vec3], n: UnitVec3) -> f64 {
    let Some(&o) = pts.first() else { return 0.0 };
    pts.windows(2).map(|w| (w[0] - o).cross(w[1] - o).dot(n)).sum::<f64>() / 2.0
}

/// Turn a loop's per-edge values round with its corners (edge `i` starts at corner `i`).
fn turn<T>(per_edge: &mut [T]) {
    per_edge.reverse();
    if !per_edge.is_empty() {
        per_edge.rotate_left(1);
    }
}

/// The circle (centre in `plane` coordinates, radius) of face `f` when it is a round wall
/// standing square on the plane: a cylinder about the plane's normal `n`.
pub(crate) fn round_wall(body: &Body, plane: &Plane, n: Vec3, f: u32) -> Option<(Vec2, f64)> {
    match face_surface(body, f as usize)? {
        FaceSurface::Cylinder { origin, axis, radius } if axis.cross(n).len() < 1e-6 => Some((plane.to_local(origin), radius)),
        _ => None,
    }
}

/// A loop of points on a face, each with the B-rep face across the loop from it to the next
/// point (the wall it runs along there), as segments: each run along one round wall (`wall`
/// gives a face's circle) is an arc of that circle, the rest stays straight. Coaxial cylinders of
/// one radius are one wall, however many faces it comes in (many systems write a round hole's
/// wall as two half-cylinders), so such a hole is one circle. `eps`: how far off its circle a
/// point may be.
pub(crate) fn outline_loop(pts: &[(Vec2, Option<u32>)], wall: &mut impl FnMut(u32) -> Option<(Vec2, f64)>, eps: f64) -> Loop2 {
    Loop2 { segs: outline_segs(pts, wall, eps).into_iter().map(|(seg, _)| seg).collect() }
}

/// [`outline_loop`], each segment with its wall (an arc's: that of its first point).
fn outline_segs(pts: &[(Vec2, Option<u32>)], wall: &mut impl FnMut(u32) -> Option<(Vec2, f64)>, eps: f64) -> Vec<(Seg2, Option<u32>)> {
    let m = pts.len();
    let point = |i: usize| pts.get(i % m).map(|x| x.0).unwrap_or_default();
    let face = |i: usize| pts.get(i % m).and_then(|x| x.1);
    // The circle each segment runs along: its wall's, when both its ends lie on it.
    let circles: Vec<Option<(Vec2, f64)>> = (0..m)
        .map(|i| {
            let (c, r) = wall(pts.get(i)?.1?)?;
            ((point(i).dist(c) - r).abs() <= eps && (point(i + 1).dist(c) - r).abs() <= eps).then_some((c, r))
        })
        .collect();
    let circle = |i: usize| circles.get(i % m).copied().flatten();
    let same = |i: usize, (c, r): (Vec2, f64)| circle(i).is_some_and(|(d, s)| d.dist(c) <= eps && (s - r).abs() <= eps);
    // Start where no arc runs on across the start (anywhere when the whole loop is one circle).
    let start = (0..m).find(|i| circle(*i + m - 1).is_none_or(|c| !same(*i, c))).unwrap_or(0);
    let mut segs = Vec::new();
    let mut i = 0;
    while i < m {
        let k = start + i;
        let Some((c, r)) = circle(k) else {
            segs.push((Seg2::Line { a: point(k), b: point(k + 1) }, face(k)));
            i += 1;
            continue;
        };
        let mut j = i + 1;
        while j < m && same(start + j, (c, r)) {
            j += 1;
        }
        let sweep: f64 = (i..j)
            .map(|s| {
                let d = (point(start + s + 1) - c).angle() - (point(start + s) - c).angle();
                (d + PI).rem_euclid(TAU) - PI
            })
            .sum();
        // Arcs of up to half a turn each.
        let pieces = ((sweep.abs() / PI - 1e-9).ceil() as usize).clamp(1, 4);
        let (a0, step) = ((point(k) - c).angle(), sweep / pieces as f64);
        segs.extend((0..pieces).map(|p| (Seg2::Arc { center: c, radius: r, start: a0 + step * p as f64, sweep: step }, face(k))));
        i = j;
    }
    segs
}

/// `segs` (a closed loop) with lines that run on in one line joined: a slot's side in two faces
/// is one line.
fn joined_lines(segs: impl IntoIterator<Item = Seg2>, tol: f64) -> Vec<Seg2> {
    let on = |p: Vec2, a: Vec2, b: Vec2| p.extend(0.0).dist_to_segment(a.extend(0.0), b.extend(0.0)) <= tol;
    let mut out: Vec<Seg2> = Vec::new();
    for seg in segs {
        if let (Some(Seg2::Line { a, b }), Seg2::Line { b: c, .. }) = (out.last_mut(), seg)
            && on(*b, *a, c)
        {
            *b = c;
            continue;
        }
        out.push(seg);
    }
    // Across the loop's start.
    if let (true, Some(Seg2::Line { a, b }), Some(Seg2::Line { b: c, .. })) = (out.len() > 2, out.last().copied(), out.first().copied())
        && on(b, a, c)
    {
        out.pop();
        if let Some(Seg2::Line { a: first, .. }) = out.first_mut() {
            *first = a;
        }
    }
    out
}

/// Face `f` of the body as a panel outline. Only a planar face can be one. Its loops are read
/// exactly, as a plate's are (an arc where one runs along a round wall standing square on the
/// face): the outer one is the outline, the others are its holes.
fn outline(b: &Body, mesh: &Mesh, f: usize, tol: f64) -> Result<Face> {
    let Some(FaceSurface::Plane { normal: n, point }) = face_surface(b, f) else {
        return err("not supported yet: curved faces in a converted sheet (only planar faces convert)");
    };
    let plane = Plane::from_normal(point, n).ok_or_else(|| DocError::Invalid("a face of the body has no plane".into()))?;
    // Its edges, each with the face on its other side.
    let mut edges: Vec<(&[Vec3], Option<usize>)> = mesh
        .edges
        .iter()
        .zip(&mesh.edge_faces)
        .filter(|(line, faces)| line.len() >= 2 && faces.iter().any(|g| *g as usize == f))
        .map(|(line, faces)| (line.as_slice(), faces.iter().map(|g| *g as usize).find(|g| *g != f)))
        .collect();
    // Chained head to tail, they close the outline and its holes: each point with the face the
    // loop runs along from it to the next point.
    let mut loops: Vec<Vec<(Vec3, Option<usize>)>> = Vec::new();
    while let Some((line, across)) = edges.pop() {
        let (Some(&start), Some(&last)) = (line.first(), line.last()) else { continue };
        let mut lp: Vec<(Vec3, Option<usize>)> = line.iter().take(line.len().saturating_sub(1)).map(|p| (*p, across)).collect();
        let mut end = last;
        while end.dist(start) > tol {
            let meets = |p: Option<&Vec3>| p.is_some_and(|p| p.dist(end) <= tol);
            let Some(k) = edges.iter().position(|(l, _)| meets(l.first()) || meets(l.last())) else {
                return err("the outline of a face of the body does not close");
            };
            let (l, g) = edges.swap_remove(k);
            let run: Vec<Vec3> = if meets(l.first()) { l.to_vec() } else { l.iter().rev().copied().collect() };
            lp.extend(run.iter().take(run.len().saturating_sub(1)).map(|p| (*p, g)));
            end = run.last().copied().unwrap_or(start);
        }
        loops.push(lp);
    }
    let mut walls = std::collections::HashMap::new();
    let mut wall = |g: u32| *walls.entry(g).or_insert_with(|| round_wall(b, &plane, *n, g));
    // Each loop with the faces of its walls.
    let mut shaped: Vec<(Vec<(Seg2, Option<u32>)>, Vec<usize>)> = loops
        .iter()
        .map(|lp| {
            let pts: Vec<(Vec2, Option<u32>)> = lp.iter().map(|(p, g)| (plane.to_local(*p), g.and_then(|g| u32::try_from(g).ok()))).collect();
            let mut walls: Vec<usize> = lp.iter().filter_map(|(_, g)| *g).collect();
            walls.sort_unstable();
            walls.dedup();
            (outline_segs(&pts, &mut wall, tol * 10.0), walls)
        })
        .collect();
    let area = |segs: &[(Seg2, Option<u32>)]| segs.iter().map(|(seg, _)| seg.area_term()).sum::<f64>().abs();
    shaped.sort_by(|x, y| area(&y.0).total_cmp(&area(&x.0)));
    let mut it = shaped.into_iter();
    let Some((outer, _)) = it.next() else { return err("a face of the body has no edges") };
    let holes = it.map(|(segs, walls)| (Loop2 { segs: joined_lines(segs.into_iter().map(|(seg, _)| seg), tol) }, walls)).collect();
    // The outline's corners, where its segments start (an arc's in quarter turns at most, so that
    // the corners alone still make an outline), each with the face along it.
    let world = |v: Vec2| plane.to_world(v);
    let (mut pts, mut tags, mut round) = (Vec::new(), Vec::new(), Vec::new());
    for (seg, g) in outer {
        let g = g.map(|g| g as usize);
        if let Seg2::Arc { center, radius, start, sweep } = seg {
            let k = ((sweep.abs() / FRAC_PI_2 - 1e-9).ceil() as usize).clamp(1, 8);
            let step = sweep / k as f64;
            for p in 0..k {
                let a = start + step * p as f64;
                pts.push(world(center + Vec2::from_angle(a) * radius));
                tags.push(g);
                round.push(Some((world(center), world(center + Vec2::from_angle(a + step / 2.0) * radius))));
            }
        } else {
            pts.push(world(seg.start()));
            tags.push(g);
            round.push(None);
        }
    }
    // A straight run along one neighbour is one edge, however the body's edges divide it (a
    // shell's outer corners are divided at the height of its floor).
    let m = pts.len();
    let corner = |i: usize| -> bool {
        let before = (i + m - 1) % m;
        let (Some(&a), Some(&p), Some(&b)) = (pts.get(before), pts.get(i), pts.get((i + 1) % m)) else { return true };
        let arc = |k: usize| round.get(k).is_some_and(Option::is_some);
        tags.get(before) != tags.get(i) || arc(before) || arc(i) || p.dist_to_segment(a, b) > tol
    };
    let keep: Vec<usize> = (0..m).filter(|i| corner(*i)).collect();
    let mut pts: Vec<Vec3> = keep.iter().filter_map(|i| pts.get(*i).copied()).collect();
    let mut tags: Vec<Option<usize>> = keep.iter().filter_map(|i| tags.get(*i).copied()).collect();
    let mut round: Vec<Option<(Vec3, Vec3)>> = keep.iter().filter_map(|i| round.get(*i).copied()).collect();
    if area_about(&pts, n) < 0.0 {
        pts.reverse();
        turn(&mut tags);
        turn(&mut round);
    }
    Ok(Face { n, pts, across: tags, round, plane, holes })
}

/// The face straight through the material behind planar face `f`, when it is parallel to it,
/// and how far behind it lies.
fn behind(b: &Body, mesh: &Mesh, f: usize) -> Option<(usize, f64)> {
    let Some(FaceSurface::Plane { normal: n, point: o }) = face_surface(b, f) else { return None };
    // The face most of it looks through to, from points in its largest triangles: a few of them
    // may look at the floor of a label engraved in the other skin.
    let area = |t: &[Vec3; 3]| (t[1] - t[0]).cross(t[2] - t[0]).len();
    let mut tris: Vec<[Vec3; 3]> =
        mesh.triangles.iter().zip(&mesh.tri_face).filter(|(_, g)| **g as usize == f).filter_map(|(t, _)| mesh.tri(t)).collect();
    tris.sort_by(|x, y| area(y).total_cmp(&area(x)));
    let mut seen: Vec<(usize, f64)> = Vec::new();
    for tri in tris.iter().take(8) {
        let middle = (tri[0] + tri[1] + tri[2]) * (1.0 / 3.0);
        for from in std::iter::once(middle).chain(tri.iter().map(|v| (middle + *v) * 0.5)) {
            let Some(g) = mesh.raycast(from, n * -1.0).and_then(|(_, hit)| mesh.tri_face.get(hit)).map(|g| *g as usize) else { continue };
            match seen.iter_mut().find(|(h, _)| *h == g) {
                Some(entry) => entry.1 += area(tri),
                None => seen.push((g, area(tri))),
            }
        }
    }
    let g = seen.iter().max_by(|x, y| x.1.total_cmp(&y.1))?.0;
    let Some(FaceSurface::Plane { normal, point }) = face_surface(b, g) else { return None };
    (normal.dot(n) < -1.0 + 1e-9).then_some((g, (o - point).dot(n)))
}

/// A chamfer or a bend's cylinder picked as a bend: the face `strip` between the planar faces
/// `a` and `b`, which meet in that bend at the line where their planes cross.
#[derive(Clone, Copy)]
struct Chamfer {
    strip: usize,
    a: usize,
    b: usize,
}

/// The chamfer or bend cylinder a bend point picks. A bend cylinder ([`bend_cylinder`]) is
/// picked by a point anywhere on it (within `near`, the cylinder being drawn in chords). A
/// chamfer by a point on the middle half of a planar face that runs between two planar faces
/// along two parallel edges, the two faces' planes crossing in a line parallel to them and the
/// strip facing between the two (as the chord of a bend's arc does). A point on any other face,
/// or near a chamfer's edge, picks an edge as before.
fn chamfer_at(mb: &ModelBody, mesh: &Mesh, p: Vec3, tol: f64, near: f64) -> Option<Chamfer> {
    let b = &mb.body;
    let (d, f) = crate::appearance::nearest_face(mb, p)?;
    if d <= near {
        if let Some(c) = bend_cylinder(b, mesh, f, tol) {
            return Some(c);
        }
        // A tangent edge belongs to both the cylinder and a planar panel. The nearest
        // face can be either: resolve the cylinder from the edge's incident faces too.
        let edge = mesh
            .edges
            .iter()
            .zip(&mesh.edge_faces)
            .map(|(line, faces)| {
                let distance = line
                    .windows(2)
                    .filter_map(|w| match w {
                        [a, b] => Some(p.dist_to_segment(*a, *b)),
                        _ => None,
                    })
                    .fold(f64::INFINITY, f64::min);
                (distance, faces)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, faces)) = edge.filter(|(distance, _)| *distance <= near)
            && let Some(c) = faces.iter().find_map(|g| bend_cylinder(b, mesh, *g as usize, tol))
        {
            return Some(c);
        }
    }
    if d > tol {
        return None;
    }
    let strip = outline(b, mesh, f, tol).ok()?;
    let plane = |g: usize| match face_surface(b, g) {
        Some(FaceSurface::Plane { normal, .. }) => Some(normal),
        _ => None,
    };
    let m = strip.pts.len();
    let edge = |i: usize| -> Option<(Vec3, Vec3, usize)> {
        let (p, q) = (*strip.pts.get(i)?, *strip.pts.get((i + 1) % m)?);
        Some((p, (q - p).normalized()?, strip.across.get(i).copied().flatten()?))
    };
    let mut sides = Vec::new();
    for i in 0..m {
        for j in i + 1..m {
            let (Some((pa, da, a)), Some((pb, db, bb))) = (edge(i), edge(j)) else { continue };
            let (Some(na), Some(nb)) = (plane(a), plane(bb)) else { continue };
            let Some(sharp) = na.cross(nb).normalized() else { continue };
            let lined_up = da.dot(db) < -1.0 + 1e-7 && sharp.dot(da).abs() > 1.0 - 1e-7;
            if a != bb && a != f && bb != f && lined_up && strip.n.dot(na) > 1e-9 && strip.n.dot(nb) > 1e-9 {
                sides.push((pa, pb, da, a, bb));
            }
        }
    }
    let &[(pa, pb, d, a, bb)] = sides.as_slice() else { return None };
    // Its middle half: a quarter of its width or more from either edge.
    let off = |o: Vec3| (p - o - d * (p - o).dot(d)).len();
    let width = off(pa) + off(pb);
    (off(pa) >= width / 4.0 && off(pb) >= width / 4.0).then_some(Chamfer { strip: f, a, b: bb })
}

/// Face `f` as a bend modelled with its radius: a cylinder with a planar face tangent to it
/// along a straight edge parallel to its axis on either side, the two planes crossing in a line
/// (a bend between 0 and 180°). Other faces along it (the walls of a notch) don't count.
fn bend_cylinder(b: &Body, mesh: &Mesh, f: usize, tol: f64) -> Option<Chamfer> {
    let Some(FaceSurface::Cylinder { origin, axis, .. }) = face_surface(b, f) else { return None };
    let mut sides: Vec<(usize, Vec3)> = Vec::new();
    for (line, faces) in mesh.edges.iter().zip(&mesh.edge_faces) {
        let g = match *faces.as_slice() {
            [x, y] if x as usize == f => y as usize,
            [x, y] if y as usize == f => x as usize,
            _ => continue,
        };
        let (Some(&p), Some(&q)) = (line.first(), line.last()) else { continue };
        let along = (q - p).normalized().is_some_and(|d| d.cross(axis).len() <= 1e-6);
        if g == f || !along || line.iter().any(|x| x.dist_to_segment(p, q) > tol) || sides.iter().any(|(h, _)| *h == g) {
            continue;
        }
        // Tangent: the plane's normal is the cylinder's there, square to the axis.
        let Some(FaceSurface::Plane { normal: n, .. }) = face_surface(b, g) else { continue };
        let radial = (p - origin) - axis * (p - origin).dot(axis);
        if radial.normalized().is_some_and(|out| n.dot(out).abs() >= 1.0 - 1e-6) {
            sides.push((g, *n));
        }
    }
    let &[(a, na), (bb, nb)] = sides.as_slice() else { return None };
    na.cross(nb).normalized().map(|_| Chamfer { strip: f, a, b: bb })
}

/// The cylinder of bend `c` on a shell's other skin: the bend cylinder between the faces
/// straight behind `c`'s two sides, so that a bend picked on either skin is that bend on both.
fn other_skin(b: &Body, mesh: &Mesh, c: &Chamfer, tol: f64) -> Option<Chamfer> {
    let (x, y) = (behind(b, mesh, c.a)?.0, behind(b, mesh, c.b)?.0);
    let beside_x = mesh.edge_faces.iter().filter(|faces| faces.contains(&(x as u32))).flatten().map(|g| *g as usize);
    beside_x
        .filter(|g| *g != x && *g != c.strip)
        .find_map(|g| bend_cylinder(b, mesh, g, tol).filter(|o| (o.a, o.b) == (x, y) || (o.a, o.b) == (y, x)))
}

/// Face `g`'s outline with each picked chamfer along it taken out: the edge along the chamfer
/// moves to where `g`'s plane meets the plane of the face across the chamfer (its corners
/// sliding along the edges before and after it), and runs along that face.
fn without_chamfers(b: &Body, mut face: Face, g: usize, chamfers: &[Chamfer], tol: f64) -> Result<Face> {
    for c in chamfers {
        let other = if c.a == g {
            c.b
        } else if c.b == g {
            c.a
        } else {
            continue;
        };
        let Some(FaceSurface::Plane { normal: n, point: o }) = face_surface(b, other) else {
            return err("the face across a chamfer picked as a bend is not flat");
        };
        let m = face.pts.len();
        let along: Vec<usize> = (0..m).filter(|i| face.across.get(*i).copied().flatten() == Some(c.strip)).collect();
        let &[k] = along.as_slice() else {
            return err("not supported yet: a chamfer picked as a bend that runs along a face more than once");
        };
        // Its corners slide along the edges before and after it: those must be straight.
        if [k + m - 1, k + 1].iter().any(|i| face.round.get(i % m).is_some_and(Option::is_some)) {
            return err("not supported yet: a round edge of a face that ends at a chamfer picked as a bend");
        }
        // Each end of the edge slides along the edge on its other side, out to the other plane.
        let slide = |corner: usize, before: usize| -> Option<Vec3> {
            let (p, q) = (*face.pts.get(corner % m)?, *face.pts.get(before % m)?);
            let d = (p - q).normalized()?;
            let s = (o - p).dot(n) / d.dot(n);
            (s.is_finite() && s >= -tol).then_some(p + d * s)
        };
        let (Some(p), Some(q)) = (slide(k, k + m - 1), slide(k + 1, k + 2)) else {
            return err("a face beside a chamfer picked as a bend does not reach the face across it");
        };
        if let Some(x) = face.pts.get_mut(k) {
            *x = p;
        }
        if let Some(x) = face.pts.get_mut((k + 1) % m) {
            *x = q;
        }
        if let Some(x) = face.across.get_mut(k) {
            *x = Some(other);
        }
    }
    Ok(face)
}

/// How far a panel's edge along a rip moves in, so that the panel, cut square to the sheet,
/// stays half the gap from the plane halfway between it and the panel `y` across the rip.
/// `e` is the edge's direction round panel `x`.
fn rip_cut(x: &Face, y: &Face, e: Vec3, t: f64, gap: f64) -> Result<f64> {
    // Into each panel from the edge, square to the edge.
    let (wx, wy) = (x.n.cross(e), e.cross(y.n));
    let mid = (wx - wy).normalized().filter(|m| wx.dot(*m) > 1e-9);
    let Some(mid) = mid else { return err("two faces of the sheet lie on each other along the edge between them") };
    // The sheet's other face lies `t` behind the panel: nearer that plane, or further from it.
    let lean = -x.n.dot(mid) * t;
    Ok((gap / 2.0 - lean.min(0.0)) / wx.dot(mid))
}

/// A counter-clockwise outline with each edge moved in by its own amount. Moved edges meet
/// where their lines cross; two edges in one line that move by different amounts are joined by
/// a square step. Returns the corners and, per edge, the edge it was (none for a step).
fn inset(pts: &[Vec2], by: &[f64]) -> Result<(Vec<Vec2>, Vec<Option<usize>>)> {
    let n = pts.len();
    if n < 3 {
        return err("a face of the sheet has no outline");
    }
    let moved = |i: usize| -> Option<(Vec2, Vec2)> {
        let (a, b) = (*pts.get(i % n)?, *pts.get((i + 1) % n)?);
        let d = (b - a).normalized()?;
        Some((a + Vec2::new(-d.y, d.x) * *by.get(i % n)?, d))
    };
    let (mut out, mut from) = (Vec::new(), Vec::new());
    for i in 0..n {
        let (Some((p0, d0)), Some((p1, d1))) = (moved(i + n - 1), moved(i)) else { return err("a face of the sheet has an edge without length") };
        let cross = d0.cross(d1);
        if cross.abs() > 1e-9 {
            out.push(p0 + d0 * ((p1 - p0).cross(d1) / cross));
        } else if d0.dot(d1) > 0.0 {
            let end = p0 + d0 * (p1 - p0).dot(d0);
            if end.dist(p1) > 1e-9 {
                out.push(end);
                from.push(None);
            }
            out.push(p1);
        } else {
            return err("the outline of a face of the sheet doubles back on itself");
        }
        from.push(Some(i));
    }
    // Every edge still runs the way it did, and the outline is still one simple loop.
    let m = out.len();
    let kept = (0..m).all(|j| match (from.get(j).copied().flatten().and_then(moved), out.get(j), out.get((j + 1) % m)) {
        (Some((_, d)), Some(&a), Some(&b)) => (b - a).dot(d) > 1e-9,
        _ => true,
    });
    if !kept || !(signed_area(&out) > 0.0) || polys_cross(&out, &out) {
        return err("the bends' setbacks and the gaps at the cut edges use up an edge of a face");
    }
    Ok((out, from))
}

/// A wall standing on part of an edge of the fixed face: where the outline of `wall`, the face
/// across a bend from the fixed face `base` (`ids`: the two faces), leaves the bend's edge up a
/// strip no taller than the bend's setback `by`, runs back along the edge and climbs the wall's
/// end, the strip is the fixed face's own edge (a plate's edge face in one plane with the outside
/// of a wall that stands on part of it), which the setback would use up. It is cut off the wall,
/// whose bend's edge then ends where the line of the wall's end meets it, and the fixed face's
/// edge is split there: the pieces beside the wall are no bend. `bend` says whether a face is a
/// bend of the wall. Any other wall is left as it is.
fn end_at_wall(base: &mut Face, wall: &mut Face, ids: (usize, usize), by: f64, bend: &dyn Fn(usize) -> bool, tol: f64) {
    let (m, n) = (wall.pts.len(), base.pts.len());
    let at_wall: Vec<usize> = (0..m).filter(|i| wall.across.get(*i).copied().flatten() == Some(ids.0)).collect();
    let at_base: Vec<usize> = (0..n).filter(|i| base.across.get(*i).copied().flatten() == Some(ids.1)).collect();
    let (&[k], &[i]) = (at_wall.as_slice(), at_base.as_slice()) else { return };
    if m < 5 {
        return;
    }
    // Corner `j` and edge `j` (from corner `j`) counted from the bend's edge on.
    let pt = |j: usize| wall.pts.get((k + j) % m).copied().unwrap_or_default();
    let (p, q) = (pt(0), pt(1));
    let Some(d) = (q - p).normalized() else { return };
    // Up the wall from the bend's edge (the outline runs counter-clockwise about the normal), and
    // along the edge.
    let up = |x: Vec3| (x - p).dot(wall.n.cross(d));
    let along = |x: Vec3| (x - p).dot(d);
    let free = |j: usize| {
        let j = (k + j) % m;
        wall.round.get(j).is_some_and(Option::is_none) && wall.across.get(j).copied().flatten().is_none_or(|g| !bend(g))
    };
    let low = |x: Vec3| up(x) > tol && up(x) <= by + tol;
    // A strip from an end of the bend's edge up to `x1`, back along the edge (`back`: the way it
    // runs) to `x2`, then the wall's end up to `x3`: where the line of that end meets the edge.
    let foot = |x1: Vec3, x2: Vec3, x3: Vec3, back: f64| {
        let rise = up(x3) - up(x2);
        (low(x1) && low(x2) && (x2 - x1).dot(d) * back > tol && rise > tol).then(|| x2 - (x3 - x2) * (up(x2) / rise))
    };
    // At the edge's end the strip is edges 1 and 2, the wall's end edge 3; at its start the strip
    // is edges m - 1 and m - 2, the wall's end edge m - 3.
    let ok = |js: [usize; 3]| js.iter().all(|j| free(*j));
    let end = if ok([1, 2, 3]) { foot(pt(2), pt(3), pt(4), -1.0) } else { None };
    let start = if ok([m - 1, m - 2, m - 3]) { foot(pt(m - 1), pt(m - 2), pt(m - 3), 1.0) } else { None };
    let (p1, q1) = (start.unwrap_or(p), end.unwrap_or(q));
    let len = along(q);
    let inside = |x: Option<Vec3>| x.is_none_or(|x| along(x) > tol && along(x) < len - tol);
    if (start.is_none() && end.is_none())
        || !inside(start)
        || !inside(end)
        || along(q1) - along(p1) <= tol
        || (start.is_some() && end.is_some() && m < 7)
    {
        return;
    }
    // The wall: the strips' corners and edges go, the bend's edge runs from `p1` to `q1`.
    let (mut corners_out, mut edges_out) = (Vec::new(), Vec::new());
    if end.is_some() {
        corners_out.extend([2, 3]);
        edges_out.extend([1, 2]);
    }
    if start.is_some() {
        corners_out.extend([m - 1, m - 2]);
        edges_out.extend([m - 1, m - 2]);
    }
    if let Some(x) = wall.pts.get_mut(k) {
        *x = p1;
    }
    if let Some(x) = wall.pts.get_mut((k + 1) % m) {
        *x = q1;
    }
    // Counted from the bend's edge, so each kept corner is still followed by its own edge
    // wherever the face's loop starts.
    wall.pts = kept_from(&wall.pts, k, &corners_out);
    wall.across = kept_from(&wall.across, k, &edges_out);
    wall.round = kept_from(&wall.round, k, &edges_out);
    // The fixed face's edge: the bend from `p1` to `q1` (it runs the other way along it), no bend
    // beside it.
    let (Some(&a), Some(&b)) = (base.pts.get(i), base.pts.get((i + 1) % n)) else { return };
    let from_a = |x: Vec3| (x - a).dot(b - a);
    let (x, y) = if from_a(p1) < from_a(q1) { (p1, q1) } else { (q1, p1) };
    let mut pieces: Vec<(Vec3, Option<usize>)> = Vec::with_capacity(3);
    if x.dist(a) > tol {
        pieces.push((a, None));
    }
    pieces.push((if x.dist(a) > tol { x } else { a }, Some(ids.1)));
    if y.dist(b) > tol {
        pieces.push((y, None));
    }
    let count = pieces.len();
    base.pts.splice(i..=i, pieces.iter().map(|(x, _)| *x));
    base.across.splice(i..=i, pieces.iter().map(|(_, g)| *g));
    base.round.splice(i..=i, std::iter::repeat_n(None, count));
}

/// The items of a loop's per-corner or per-edge list `of`, counted from item `k` on, less those
/// at the places `out` (counted from `k`). Corners and edges taken out at the same places counted
/// from one start stay paired (edge `j` from corner `j`); taken out at their places in the list
/// they would not where the places run across the list's start.
fn kept_from<T: Copy>(of: &[T], k: usize, out: &[usize]) -> Vec<T> {
    let m = of.len();
    (0..m).filter(|j| !out.contains(j)).filter_map(|j| of.get((k + j) % m).copied()).collect()
}

/// Give flange `i` its panel: `outline` in flat coordinates, counter-clockwise, its edge at the
/// bend first trimmed to the end of the bend zone.
fn set_panel(sh: &mut SheetBody, i: usize, outline: &[Vec2]) {
    let ba = sh.flanges.get(i).map_or(0.0, |f| sh.allowance(f));
    let Some(f) = sh.flanges.get_mut(i) else { return };
    // Its corners along the bend (u) and past the bend zone (v); those at the zone exactly on it.
    let uv: Vec<(f64, f64)> =
        outline.iter().map(|q| ((*q - f.p).dot(f.d), (*q - f.p).dot(f.n) - ba)).map(|(u, v)| (u, if v.abs() < 1e-6 { 0.0 } else { v })).collect();
    f.leg = uv.iter().fold(0.0, |m, (_, v)| m.max(*v));
    let level = |h: f64| -> Vec<f64> { uv.iter().filter(|(_, v)| (v - h).abs() < 1e-6).map(|(u, _)| *u).collect() };
    match (level(0.0).as_slice(), level(f.leg).as_slice()) {
        // Two corners at the bend and two on a far edge parallel to it: a flange's own panel,
        // with its run-ons past the bend's ends and its slanted ends.
        (&[a, b], &[c, d]) if uv.len() == 4 && f.leg > 1e-6 => {
            let zero = |x: f64| if x.abs() < 1e-12 { 0.0 } else { x };
            f.ext = (zero(-a.min(b)), zero(a.max(b) - f.len));
            f.slope = (zero((a.min(b) - c.min(d)) / f.leg), zero((c.max(d) - a.max(b)) / f.leg));
        }
        _ => f.shape = Some(uv.iter().map(|(u, v)| f.p + f.d * *u + f.n * (ba + *v)).collect()),
    }
}

/// A panel's trimmed outline as drawn: an edge that was a round edge of the face is an arc again
/// (`round`, per edge of the untrimmed outline `pts`: the circle's centre and the arc's middle),
/// pieces of one circle in a row one arc of up to half a turn. None when the face has no round
/// edges. A round edge keeps both its ends: one that a bend's setback or a cut edge would move is
/// not supported yet.
fn drawn(trimmed: &[Vec2], from: &[Option<usize>], pts: &[Vec2], round: &[Option<(Vec2, Vec2)>]) -> Result<Option<Loop2>> {
    if round.iter().all(Option::is_none) {
        return Ok(None);
    }
    let (m, n) = (trimmed.len(), pts.len());
    let mut segs: Vec<Seg2> = Vec::with_capacity(m);
    for j in 0..m {
        let (Some(&a), Some(&b)) = (trimmed.get(j), trimmed.get((j + 1) % m)) else { continue };
        let arc = from.get(j).copied().flatten().and_then(|i| Some((i, round.get(i).copied().flatten()?)));
        let Some((i, (c, mid))) = arc else {
            segs.push(Seg2::Line { a: snap(a), b: snap(b) });
            continue;
        };
        let (Some(&p), Some(&q)) = (pts.get(i), pts.get((i + 1) % n)) else { continue };
        if a.dist(p) > 1e-6 || b.dist(q) > 1e-6 {
            return err("not supported yet: a round edge of a face that ends at a bend or a cut edge of the sheet");
        }
        let (start, r) = ((p - c).angle(), c.dist(p));
        let half = ((mid - c).angle() - start + PI).rem_euclid(TAU) - PI;
        match segs.last_mut() {
            Some(Seg2::Arc { center, radius, sweep, .. })
                if center.dist(c) <= 1e-9 && (*radius - r).abs() <= 1e-9 && *sweep * half > 0.0 && (*sweep + 2.0 * half).abs() <= PI + 1e-9 =>
            {
                *sweep += 2.0 * half;
            }
            _ => segs.push(Seg2::Arc { center: c, radius: r, start, sweep: 2.0 * half }),
        }
    }
    Ok(Some(Loop2 { segs }))
}

/// Names of the edges of `body` at the bend points, to find them again after the body changes.
/// A point on no edge gets none: the conversion then says so, instead of bending the edge that
/// happens to be nearest. A point on a chamfer or a bend cylinder gets none either: it stays that
/// chamfer's or cylinder's point.
pub fn bend_edge_names(st: &crate::ModelState, body: &str, pts: &[Vec3]) -> Vec<String> {
    let Some(b) = st.body(body) else { return Vec::new() };
    let (mesh, near, tol) = (b.mesh(), (b.body.size() * 2e-3).max(1e-3), (b.body.size() * 1e-6).max(1e-6));
    let on = |p: &Vec3| {
        chamfer_at(b, &mesh, *p, tol, near).is_none() && mesh.edges.iter().any(|e| e.windows(2).any(|w| p.dist_to_segment(w[0], w[1]) <= near))
    };
    pts.iter().zip(crate::naming::edge_names_at(b, pts)).map(|(p, name)| if on(p) { name } else { String::new() }).collect()
}

/// Convert body `mb` to a sheet under the rule `rule`, whose values for a sheet of a thickness
/// (`None`: the rule's own) are `values`: `face` is a point on the fixed face and `bends` are
/// points on the sharp edges that become bends, in the middle of chamfers or on bend cylinders
/// that do (the body's coordinates, mm). See the module docs. `body_to_world` places diagnostic
/// points in world coordinates. Geometry and picks stay local. Returns the sheet and a note for
/// every corner or end of a bend not finished as the rule says.
pub fn convert_body(
    mb: &ModelBody,
    rule: &str,
    values: &dyn Fn(Option<f64>) -> Result<RuleValues>,
    face: Vec3,
    bends: &[Vec3],
    body_to_world: &crate::Mat,
) -> Result<(SheetBody, Vec<String>)> {
    let at = |p| at(apply_point(body_to_world, p));
    if bends.is_empty() || bends.len() > MAX_BENDS {
        return err(format!("pick 1…{MAX_BENDS} bend edges"));
    }
    let rv = values(None)?;
    let (b, mesh) = (&mb.body, mb.mesh());
    let size = b.size();
    if !(size.is_finite() && size > 0.0) {
        return err("the body has no size");
    }
    // Points closer than `tol` are one corner; a pick lies within `near` of what it picks.
    let (tol, near) = ((size * 1e-6).max(1e-6), (size * 2e-3).max(1e-3));
    let Some((_, f0)) = crate::appearance::nearest_face(mb, face).filter(|(d, _)| *d <= near) else {
        return err(format!("no face of the body at the fixed face point {}", at(face)));
    };
    // The two faces along each bend edge, or on either side of each chamfer or bend cylinder
    // (with a bend cylinder its other skin's too).
    let mut picks = Vec::with_capacity(bends.len());
    let mut chamfers: Vec<Chamfer> = Vec::new();
    for p in bends {
        if let Some(c) = chamfer_at(mb, &mesh, *p, tol, near) {
            picks.push((*p, (c.a, c.b)));
            for c in std::iter::once(c).chain(other_skin(b, &mesh, &c, tol)) {
                if !chamfers.iter().any(|x| x.strip == c.strip) {
                    chamfers.push(c);
                }
            }
            continue;
        }
        let dist = |e: &Vec<Vec3>| e.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min);
        let edge = mesh.edges.iter().zip(&mesh.edge_faces).map(|(e, faces)| (dist(e), faces)).min_by(|x, y| x.0.total_cmp(&y.0));
        match edge.filter(|(d, _)| *d <= near).map(|(_, faces)| faces.as_slice()) {
            Some(&[x, y]) if x != y => picks.push((*p, (x as usize, y as usize))),
            Some(_) => return err(format!("the edge at the bend point {} does not lie between two faces", at(*p))),
            None => return err(format!("no edge of the body at the bend point {}", at(*p))),
        }
    }
    if let Some((p, _)) = picks.iter().find(|(_, (x, y))| chamfers.iter().any(|c| c.strip == *x || c.strip == *y)) {
        return err(format!("the bend at {} is on a chamfer picked as a bend (pick the chamfer or its edges, not both)", at(*p)));
    }
    let face_of = |f: usize| outline(b, &mesh, f, tol).and_then(|face| without_chamfers(b, face, f, &chamfers, tol));
    let fixed = face_of(f0).map_err(|e| DocError::Invalid(format!("the fixed face can't be a face of the sheet: {e}")))?;
    // A shell: the same small distance of material behind the fixed face and the faces at every
    // bend. It keeps its thickness, and a bend picked on one skin is that bend on the other too.
    let thin = behind(b, &mesh, f0).map(|(_, d)| d).filter(|d| *d > 0.0 && d * d * 16.0 <= area_about(&fixed.pts, fixed.n));
    let wall = |f: usize, d: f64| behind(b, &mesh, f).filter(|(_, e)| (e - d).abs() <= 1e-6 * d.max(1.0)).map(|(g, _)| g);
    let backs: Vec<Option<(usize, usize)>> = picks.iter().map(|(_, (x, y))| thin.and_then(|d| Some((wall(*x, d)?, wall(*y, d)?)))).collect();
    let shell = thin.filter(|_| backs.iter().all(Option::is_some));
    // On a shell, the face behind each face at a bend (its other skin, read for its pockets).
    let skin_behind: BTreeMap<usize, usize> = picks
        .iter()
        .zip(&backs)
        .filter(|_| shell.is_some())
        .filter_map(|((_, (x, y)), back)| back.map(|(bx, by)| [(*x, bx), (bx, *x), (*y, by), (by, *y)]))
        .flatten()
        .collect();
    let t = shell.map_or(rv.thickness, |d| (d * 1e9).round() / 1e9);
    // A shell's sheet is as thick as its walls: the rule's values that follow its thickness (a
    // bend radius of `Thickness`, the gap, the reliefs) follow the walls', not the rule's own.
    let rv = if shell.is_some() { values(Some(t))? } else { rv };
    if !(rv.gap >= 0.0 && rv.bend_radius >= 0.0) {
        return err("the rule's gap and bend radius must not be negative");
    }
    let r = rv.bend_radius;
    let joined = |x: usize, y: usize| {
        mesh.edge_faces.iter().any(|faces| faces.contains(&(x as u32)) && faces.contains(&(y as u32)))
            || chamfers.iter().any(|c| (c.a, c.b) == (x, y) || (c.a, c.b) == (y, x))
    };
    let mut todo: Vec<(Vec3, Vec<(usize, usize)>)> = Vec::with_capacity(picks.len());
    for (&(p, pair), back) in picks.iter().zip(&backs) {
        let other = back.filter(|(x, y)| shell.is_some() && joined(*x, *y));
        todo.push((p, std::iter::once(pair).chain(other).collect()));
    }
    // The panels: the faces reached from the fixed face across the bends, each hanging off the
    // face it was reached from.
    let mut panels = BTreeMap::from([(f0, Panel { face: fixed, parent: None, angle: 0.0, setback: 0.0 })]);
    let mut order = vec![f0];
    loop {
        let waiting = todo.len();
        let mut rest = Vec::new();
        for (p, pairs) in todo {
            let reached = pairs.iter().find(|(x, y)| panels.contains_key(x) || panels.contains_key(y));
            let Some(&(x, y)) = reached else {
                rest.push((p, pairs));
                continue;
            };
            let (from, to) = if panels.contains_key(&x) { (x, y) } else { (y, x) };
            if let Some(seen) = panels.get(&to) {
                // The same bend picked twice is one bend; any other edge between two panels
                // would close a ring of bends, which no flat sheet folds into.
                if seen.parent == Some(from) || panels.get(&from).is_some_and(|k| k.parent == Some(to)) {
                    continue;
                }
                return err(format!("the bend at {} closes a loop of bends (leave one of its edges unpicked: it is cut open)", at(p)));
            }
            let child =
                face_of(to).map_err(|e| DocError::Invalid(format!("the face across the bend at {} can't be a face of the sheet: {e}", at(p))))?;
            let Some(parent) = panels.get(&from) else { continue };
            let theta = parent.face.n.dot(child.n).clamp(-1.0, 1.0).acos();
            if !(theta > 1e-6 && theta < std::f64::consts::PI - 1e-6) {
                return err(format!("the faces at the bend at {} must meet at an angle between 0 and 180°", at(p)));
            }
            // Up: the child rises on the material side of its parent, which is behind its face.
            let o = parent.face.pts.first().copied().unwrap_or_default();
            let lift = child.pts.iter().map(|q| (*q - o).dot(parent.face.n)).fold(0.0, |m: f64, h| if h.abs() > m.abs() { h } else { m });
            let (angle, sharp) = if lift < 0.0 { (theta, r + t) } else { (-theta, r) };
            panels.insert(to, Panel { face: child, parent: Some(from), angle, setback: sharp * (theta / 2.0).tan() });
            order.push(to);
        }
        todo = rest;
        if todo.is_empty() || todo.len() == waiting {
            break;
        }
    }
    if let Some((p, _)) = todo.first() {
        return err(format!("the bend at {} does not connect to the fixed face through the other bends", at(*p)));
    }
    // The flat frame: on the fixed face, x along its longest edge, z into the material.
    let Some(base) = panels.get(&f0) else { return err("fixed face") };
    let base_ring = Ring::from(base.face.pts.clone());
    let corners = base_ring.edges();
    let straight = corners.zip(&base.face.round).filter(|(_, r)| r.is_none()).map(|(c, _)| c);
    let longest = straight.max_by(|x, y| x.0.dist(*x.1).total_cmp(&y.0.dist(*y.1)));
    let frame = longest
        .and_then(|(a, b)| Some((*a, (*b - *a).normalized()?)))
        .and_then(|(a, x)| Plane::new(a, x, x.cross(base.face.n)))
        .ok_or_else(|| DocError::Invalid("the fixed face has no plane".into()))?;
    // A wall on part of an edge of the fixed face bends only where it stands (a bend picked as a
    // chamfer or a cylinder is taken as it is).
    let walls: Vec<(usize, f64)> = panels.iter().filter(|(_, p)| p.parent == Some(f0)).map(|(g, p)| (*g, p.setback)).collect();
    for (g, by) in walls {
        if chamfers.iter().any(|c| (c.a, c.b) == (f0, g) || (c.a, c.b) == (g, f0)) {
            continue;
        }
        let kids: Vec<usize> = panels.iter().filter(|(_, p)| p.parent == Some(g)).map(|(h, _)| *h).collect();
        let Some(mut wall) = panels.remove(&g) else { continue };
        if let Some(base) = panels.get_mut(&f0) {
            end_at_wall(&mut base.face, &mut wall.face, (f0, g), by, &|h| h == f0 || kids.contains(&h), tol);
        }
        panels.insert(g, wall);
    }
    let mut sh = base_flange(&mb.name, rule, &RuleValues { thickness: t, ..rv }, frame, Region2::default());
    // How far behind a face of the sheet the body's face `g` starts and ends: a hole's wall
    // reaches through the sheet, a pocket's ends on its floor, an island's starts at the face.
    let mut points: BTreeMap<usize, Vec<Vec3>> = BTreeMap::new();
    for (tri, g) in mesh.triangles.iter().zip(&mesh.tri_face) {
        points.entry(*g as usize).or_default().extend(tri.iter().filter_map(|k| mesh.positions.get(*k as usize)));
    }
    let span = |g: usize, face: &Face| {
        let o = face.pts.first().copied().unwrap_or_default();
        let ds = points.get(&g).into_iter().flatten().map(|p| (o - *p).dot(face.n));
        ds.fold((f64::INFINITY, 0.0), |(lo, hi): (f64, f64), d| (lo.min(d.max(0.0)), hi.max(d)))
    };
    // The span of a loop's walls: the least any starts at, and the least and most any ends at.
    let walls_span = |walls: &[usize], face: &Face| {
        walls
            .iter()
            .map(|g| span(*g, face))
            .fold((f64::INFINITY, f64::INFINITY, 0.0), |(s, lo, hi): (f64, f64, f64), (a, b)| (s.min(a), lo.min(b), hi.max(b)))
    };
    let mut work = FlatWork::new();
    // Lay the panels out, parents first: each is its face, seen through the inverse of its
    // fold, with its edges moved in by a bend's setback or a rip's cut.
    let mut queue: Vec<(usize, Option<usize>)> = vec![(f0, None)];
    let mut next = 0;
    while let Some(&(pf, fl)) = queue.get(next) {
        next += 1;
        let Some(panel) = panels.get(&pf) else { continue };
        let inv = mat_inverse(&sh.world(fl)).ok_or_else(|| DocError::Invalid("sheet panel frame".into()))?;
        let n = panel.face.pts.len();
        let (mut pts, mut by) = (Vec::with_capacity(n), Vec::with_capacity(n));
        for (i, p) in panel.face.pts.iter().enumerate() {
            let q = apply_point(&inv, *p);
            if !(q.z.abs() <= tol) {
                return err(format!("the face at {} does not lie where its bend puts it ({:.6} mm off)", at(*p), q.z));
            }
            pts.push(Vec2::new(q.x, q.y));
            let e = panel.face.pts.get((i + 1) % n).and_then(|b| (*b - *p).normalized()).unwrap_or(Vec3::X);
            by.push(match panel.face.across.get(i).copied().flatten().and_then(|g| Some((g, panels.get(&g)?))) {
                Some((g, _)) if panel.parent == Some(g) => panel.setback,
                Some((_, other)) if other.parent == Some(pf) => other.setback,
                Some((_, other)) => rip_cut(&panel.face, &other.face, e, t, rv.gap)?,
                None => 0.0,
            });
        }
        let mut across = panel.face.across.clone();
        let flat = |p: Vec3| {
            let q = apply_point(&inv, p);
            Vec2::new(q.x, q.y)
        };
        let mut round: Vec<Option<(Vec2, Vec2)>> = panel.face.round.iter().map(|r| r.map(|(c, m)| (flat(c), flat(m)))).collect();
        if signed_area(&pts) < 0.0 {
            pts.reverse();
            turn(&mut by);
            turn(&mut across);
            turn(&mut round);
        }
        let first = panel.face.pts.first().copied().unwrap_or_default();
        let face_at = |e: DocError| DocError::Invalid(format!("{e} (the face at {})", at(first)));
        let (trimmed, from) = inset(&pts, &by).map_err(face_at)?;
        let exact = drawn(&trimmed, &from, &pts, &round).map_err(face_at)?;
        match fl {
            None => sh.base.outer = exact.unwrap_or_else(|| Loop2::polygon(&trimmed.iter().map(|q| snap(*q)).collect::<Vec<_>>())),
            Some(i) => {
                set_panel(&mut sh, i, &trimmed);
                if let (Some(lp), Some(f)) = (exact, sh.flanges.get_mut(i)) {
                    f.shape = Some(trimmed.clone());
                    f.exact = Some(lp);
                }
            }
        }
        // The face's holes: cut-outs of the sheet where they lie, inside its panel. A hole's walls
        // reach through the sheet; a pocket's (an engraved label) end inside it, on floors facing
        // out as the face does: a pocket of the sheet from that skin, round its islands. On a
        // shell the face behind, the other skin, is read for its pockets too.
        let region = match fl {
            None => sh.base.clone(),
            Some(i) => sh.flanges.get(i).map(|f| sh.panel(f)).unwrap_or_default(),
        };
        let back = skin_behind.get(&pf).and_then(|g| outline(b, &mesh, *g, tol).ok());
        let skins: Vec<(&Face, bool)> = std::iter::once((&panel.face, true)).chain(back.as_ref().map(|f| (f, false))).collect();
        let bound = if skins.iter().all(|(f, _)| f.holes.is_empty()) { Ring::default() } else { region.outer.ccw().polyline(1e-3) };
        for (face, front) in skins {
            for (h, walls) in &face.holes {
                let p = face.plane.to_world(h.centroid());
                let (_, reach, deepest) = walls_span(walls, face);
                let through = reach >= t - tol;
                if through && !front {
                    continue;
                }
                let cut = sh.region_to_flat(fl, &face.plane, &Region2 { outer: h.clone(), holes: Vec::new() })?;
                let ring = cut.outer.polyline(1e-3);
                if !ring.iter().all(|q| bound.contains(*q)) || polys_cross(&bound, &ring) {
                    let kind = if through { "hole" } else { "pocket" };
                    return err(format!("not supported yet: the {kind} at {} reaches into a bend (cut it after converting)", at(p)));
                }
                if through {
                    sh.holes.push(cut);
                    continue;
                }
                let pocket =
                    |why: &str| err(format!("not supported yet: the pocket at {} in a face of the sheet {why} (cut it after converting)", at(p)));
                let eps = t * 1e-3;
                if reach > t / 2.0 + eps {
                    return pocket(&format!("ends {reach:.3} deep, more than half the sheet's {t:.3}"));
                }
                if deepest - reach > eps {
                    return pocket("has walls of more than one depth");
                }
                // Its floors: the faces at its depth beside its walls, facing out as the face does.
                let mut floors: Vec<usize> = mesh
                    .edge_faces
                    .iter()
                    .filter(|fs| fs.iter().any(|g| walls.contains(&(*g as usize))))
                    .flatten()
                    .map(|g| *g as usize)
                    .filter(|g| {
                        !walls.contains(g)
                            && matches!(face_surface(b, *g), Some(FaceSurface::Plane { normal, .. }) if normal.dot(face.n) > 1.0 - 1e-9)
                    })
                    .filter(|g| {
                        let (lo, hi) = span(*g, face);
                        (lo - reach).abs() <= eps && (hi - reach).abs() <= eps
                    })
                    .collect();
                floors.sort_unstable();
                floors.dedup();
                if floors.is_empty() {
                    return pocket("has no flat floor");
                }
                // The islands left standing in it (a letter's counter): walls from a floor up to the face.
                let mut islands = Vec::new();
                for g in floors {
                    let floor = outline(b, &mesh, g, tol)?;
                    for (k, kw) in &floor.holes {
                        let (start, lo, hi) = walls_span(kw, face);
                        if start > eps || (lo - reach).abs() > eps || (hi - reach).abs() > eps {
                            return pocket("has a hole or a step in its floor");
                        }
                        islands.push(sh.region_to_flat(fl, &floor.plane, &Region2 { outer: k.clone(), holes: Vec::new() })?.outer.reversed());
                    }
                }
                let (z0, z1) = if front { (0.0, reach) } else { (t - reach, t) };
                sh.record_pocket(Region2 { outer: cut.outer, holes: islands }, z0, z1, &mut work)?;
            }
        }
        // Its children: flanges on the trimmed edges they bend at.
        let m = trimmed.len();
        for &g in &order {
            let Some(child) = panels.get(&g).filter(|c| c.parent == Some(pf)) else { continue };
            let runs: Vec<usize> =
                (0..m).filter(|j| from.get(*j).copied().flatten().and_then(|i| across.get(i).copied().flatten()) == Some(g)).collect();
            let (a, b) = match runs.as_slice() {
                &[j] => (trimmed.get(j).copied().map(snap), trimmed.get((j + 1) % m).copied().map(snap)),
                _ => return err("not supported yet: two faces of the sheet that meet along more than one edge"),
            };
            let (Some(a), Some(b)) = (a, b) else { continue };
            let Some(d) = (b - a).normalized() else { return err("a bend edge has no length") };
            sh.flanges.push(SheetFlange {
                parent: fl,
                p: a,
                d,
                len: a.dist(b),
                n: Vec2::new(d.y, -d.x),
                angle: child.angle,
                radius: r,
                leg: 0.0,
                ext: (0.0, 0.0),
                slope: (0.0, 0.0),
                shape: None,
                exact: None,
                tag: String::new(),
                tear: (false, false),
            });
            queue.push((g, Some(sh.flanges.len() - 1)));
        }
    }
    // The bends on the fixed face finished as an edge flange feature finishes its own
    // (`SheetBody::add_flanges_each`): the rule's bend relief where one ends beside more of the
    // fixed face; where two meet at a corner of it, their walls closed with the rule's gap (which
    // the cuts above already are) and the rule's corner relief.
    let made: Vec<usize> = (0..sh.flanges.len()).collect();
    let mut notes = Vec::new();
    for &i in &made {
        notes.extend(sh.relieve_bend_ends(i, (false, false)));
    }
    let corners = sh.corners(&made);
    let mut closed = Vec::with_capacity(corners.len());
    for k in &corners {
        match sh.close_corner(k) {
            Ok(()) => closed.push(*k),
            Err(why) => notes.push(format!("the corner at {} stays cut open: {why}", sh.shown(k.c))),
        }
    }
    notes.extend(sh.relieve_corners(&corners, &closed, ReliefOverride::default()));
    Ok((sh, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_move_in_by_their_own_amounts_and_step_along_one_line() {
        let v = Vec2::new;
        let square = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)];
        let (p, from) = inset(&square, &[1.0, 0.0, 2.0, 0.0]).unwrap();
        assert_eq!(p, vec![v(0.0, 1.0), v(10.0, 1.0), v(10.0, 8.0), v(0.0, 8.0)]);
        assert_eq!(from, vec![Some(0), Some(1), Some(2), Some(3)]);
        // Two edges in one line that move by different amounts: a square step between them,
        // so 5 x 2 goes from the 10 x 10.
        let split = [v(0.0, 0.0), v(5.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)];
        let (p, from) = inset(&split, &[0.0, 2.0, 0.0, 0.0, 0.0]).unwrap();
        assert_eq!(p, vec![v(0.0, 0.0), v(5.0, 0.0), v(5.0, 2.0), v(10.0, 2.0), v(10.0, 10.0), v(0.0, 10.0)]);
        assert_eq!(from, vec![Some(0), None, Some(1), Some(2), Some(3), Some(4)]);
        // A slanted end keeps its line: the corner slides along it (2 up a 45° edge is 2 along).
        let slanted = [v(0.0, 0.0), v(10.0, 0.0), v(15.0, 5.0), v(0.0, 5.0)];
        let (p, _) = inset(&slanted, &[2.0, 0.0, 0.0, 0.0]).unwrap();
        assert!(p[1].dist(v(12.0, 2.0)) < 1e-12 && p[0].dist(v(0.0, 2.0)) < 1e-12, "{p:?}");
        // Setbacks longer than the face leave nothing of it.
        assert!(inset(&square, &[6.0, 0.0, 6.0, 0.0]).is_err());
        assert!(inset(&square, &[f64::NAN, 0.0, 0.0, 0.0]).is_err());
    }
}

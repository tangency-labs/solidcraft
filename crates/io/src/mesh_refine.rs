//! Smooth curved faces back out of a coarse display mesh (used by the Plasticity reader).
//!
//! Plasticity caches a coarse tessellation (about ten segments per hole) whose vertices lie
//! exactly on the true surfaces and carry the exact surface normals. From that:
//!  1. each face is fitted as a plane, sphere or surface of revolution (cylinder, cone,
//!     torus: the axis is the line every normal line meets, the profile a line or circle),
//!     checked against every vertex and normal; anything else is freeform and is curved
//!     through its normals (cubic Hermite edges);
//!  2. T-junctions between faces are resolved by splitting the edge at the neighbour's vertex
//!     (moved onto the shared curve when it sits on the chord);
//!  3. the mesh is subdivided adaptively: an edge is split while its midpoint on the true
//!     curve is more than a tolerance away from the chord, or its end normals differ by more
//!     than a set angle. A midpoint on an edge between two faces lies on both (alternating
//!     projection), so neighbours share it and the solid stays watertight. Interior diagonals
//!     of curved faces are flipped when the other diagonal lies closer to the surface.

use std::collections::{HashMap, HashSet};

use solidcraft_geom::prelude::*;
use solidcore_record::Vec3;

const MAX_LEVELS: usize = 8;
/// Split edges whose end normals differ more than this (radians, 15°).
const MAX_TURN: f64 = 15.0 * std::f64::consts::PI / 180.0;

#[derive(Clone, Copy, Debug)]
enum Profile {
    Line { h: f64, r: f64, dh: f64, dr: f64 },
    Circle { h: f64, r: f64, rad: f64 },
}

#[derive(Clone, Copy, Debug)]
enum Surf {
    Plane { n: Vec3, d: f64 },
    Sphere { c: Vec3, r: f64 },
    Rev { c: Vec3, a: Vec3, prof: Profile },
    Free,
}

impl Surf {
    fn is_plane(&self) -> bool {
        matches!(self, Surf::Plane { .. })
    }
    fn is_free(&self) -> bool {
        matches!(self, Surf::Free)
    }
}

/// The refined mesh: welded vertices, triangles and each triangle's face (the input's face ids).
pub struct Refined {
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    /// Not used by the mesh import yet (its bodies have no faces); kept for callers and tests.
    #[allow(dead_code)]
    pub tri_face: Vec<u32>,
}

fn unit(v: Vec3) -> Vec3 {
    v.normalized().unwrap_or(Vec3::new(0.0, 0.0, 1.0))
}
fn angle(a: Vec3, b: Vec3) -> f64 {
    let l = (a.len2() * b.len2()).sqrt();
    if l < 1e-300 { 0.0 } else { (a.dot(b) / l).clamp(-1.0, 1.0).acos() }
}
fn ekey(a: usize, b: usize) -> u64 {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    ((lo as u64) << 32) | hi as u64
}
fn ends(k: u64) -> (usize, usize) {
    ((k >> 32) as usize, (k & 0xffff_ffff) as usize)
}

/// Weld non-indexed triangles (three corners each) on a 1e-4 grid.
pub fn weld(corners: &[Vec3]) -> (Vec<Vec3>, Vec<usize>) {
    let mut map: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut pts = Vec::new();
    let ids = corners
        .iter()
        .map(|p| {
            let k = ((p.x * 1e4).round() as i64, (p.y * 1e4).round() as i64, (p.z * 1e4).round() as i64);
            *map.entry(k).or_insert_with(|| {
                pts.push(*p);
                pts.len() - 1
            })
        })
        .collect();
    (pts, ids)
}

/// Refine the curved faces of a mesh given as non-indexed triangles (`corners`, three per
/// triangle, millimetres), each corner's exact normal, and each triangle's face id. `None`
/// when nothing needed refining.
pub fn refine(corners: &[Vec3], normals: &[Vec3], tri_face: &[u32]) -> Option<Refined> {
    let tri_count = tri_face.len();
    if tri_count == 0 || corners.len() != tri_count * 3 || normals.len() != corners.len() {
        return None;
    }
    let (mut p, weld_ids) = weld(corners);
    let mut face_ids: Vec<u32> = Vec::new();
    let mut ord: HashMap<u32, usize> = HashMap::new();
    for &f in tri_face {
        ord.entry(f).or_insert_with(|| {
            face_ids.push(f);
            face_ids.len() - 1
        });
    }
    let nf = face_ids.len();
    let nk = |v: usize, fo: usize| (v * nf + fo) as u64;
    let mut nrm: HashMap<u64, Vec3> = HashMap::new();
    let mut tris: Vec<[usize; 4]> = Vec::with_capacity(tri_count);
    for t in 0..tri_count {
        let fo = ord.get(&tri_face[t]).copied().unwrap_or(0);
        let tri = [weld_ids[t * 3], weld_ids[t * 3 + 1], weld_ids[t * 3 + 2], fo];
        if tri[0] == tri[1] || tri[1] == tri[2] || tri[0] == tri[2] {
            continue;
        }
        tris.push(tri);
        for j in 0..3 {
            nrm.entry(nk(tri[j], fo)).or_insert_with(|| unit(normals[t * 3 + j]));
        }
    }
    let (lo, hi) =
        p.iter().fold((Vec3::new(f64::MAX, f64::MAX, f64::MAX), Vec3::new(f64::MIN, f64::MIN, f64::MIN)), |(lo, hi), q| (lo.min(*q), hi.max(*q)));
    let diag = (hi - lo).len();
    let tol = (5e-4 * diag).clamp(0.01, 0.1);
    let normal_of = |nrm: &HashMap<u64, Vec3>, v: usize, fo: usize| nrm.get(&nk(v, fo)).copied().unwrap_or(Vec3::new(0.0, 0.0, 1.0));

    // ---- fit each face ----
    let mut surf = vec![Surf::Free; nf];
    let mut curved_faces = 0;
    {
        let mut by_face: Vec<Vec<usize>> = vec![Vec::new(); nf];
        for t in &tris {
            by_face[t[3]].extend_from_slice(&t[..3]);
        }
        for fo in 0..nf {
            let mut seen = HashSet::new();
            let vs: Vec<usize> = by_face[fo].iter().copied().filter(|v| seen.insert(*v)).collect();
            let ps: Vec<Vec3> = vs.iter().map(|&v| p[v]).collect();
            let ns: Vec<Vec3> = vs.iter().map(|&v| normal_of(&nrm, v, fo)).collect();
            surf[fo] = fit(&ps, &ns);
            if !surf[fo].is_plane() {
                curved_faces += 1;
            }
        }
    }
    if curved_faces == 0 {
        return None;
    }

    // ---- T-junctions: split a face's edge at the neighbouring face's vertex ----
    // Faces need not share vertices along an edge: one face can have a vertex part-way along
    // the other's. Frozen, such an edge stays a coarse chord and the refined face fills the
    // gap to the curve with flat triangles. Inserting the vertex makes the faces share the
    // edge pieces. A vertex on the chord (not the curve) used by that one neighbour only is
    // moved onto the shared curve.
    for _ in 0..16 {
        let mut uses: HashMap<u64, u32> = HashMap::new();
        for t in &tris {
            for j in 0..3 {
                *uses.entry(ekey(t[j], t[(j + 1) % 3])).or_default() += 1;
            }
        }
        let mut by_v: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
        for t in &tris {
            for j in 0..3 {
                let (a, b) = (t[j], t[(j + 1) % 3]);
                if uses.get(&ekey(a, b)) != Some(&1) {
                    continue;
                }
                by_v.entry(a).or_default().push((b, t[3]));
                by_v.entry(b).or_default().push((a, t[3]));
            }
        }
        let mut vfaces: HashMap<usize, HashSet<usize>> = HashMap::new();
        for t in &tris {
            for j in 0..3 {
                vfaces.entry(t[j]).or_default().insert(t[3]);
            }
        }
        let mut snapped: HashSet<usize> = HashSet::new();
        let mut next = Vec::with_capacity(tris.len());
        let mut n = 0;
        for t in &tris {
            let mut split: Option<(usize, usize, Option<usize>)> = None; // edge position, vertex, face to snap with
            for j in 0..3 {
                if split.is_some() {
                    break;
                }
                let (a, b) = (t[j], t[(j + 1) % 3]);
                if uses.get(&ekey(a, b)) != Some(&1) {
                    continue;
                }
                let pa = p[a];
                let ab = p[b] - pa;
                let l2 = ab.len2();
                if l2 < 1e-12 {
                    continue;
                }
                let sf = surf[t[3]];
                let mut best: Option<(usize, f64, Option<usize>)> = None;
                for end in [a, b] {
                    for &(o, f) in by_v.get(&end).map(Vec::as_slice).unwrap_or(&[]) {
                        if f == t[3] || o == a || o == b {
                            continue;
                        }
                        let q = p[o] - pa;
                        let u = q.dot(ab) / l2;
                        if !(0.02..=0.98).contains(&u) {
                            continue;
                        }
                        let d = (q - ab * u).len();
                        let on_surf = if sf.is_free() { d < 0.15 * l2.sqrt() } else { project(&sf, p[o]).dist(p[o]) < tol.min(0.01) };
                        let movable =
                            !on_surf && !sf.is_free() && vfaces.get(&o).is_some_and(|s| s.len() == 1 && s.contains(&f)) && !snapped.contains(&o);
                        if (on_surf || movable) && d < 0.3 * l2.sqrt() && best.is_none_or(|(_, bd, _)| d < bd) {
                            best = Some((o, d, movable.then_some(f)));
                        }
                    }
                }
                if let Some((o, _, snap)) = best {
                    split = Some((j, o, snap));
                }
            }
            let Some((j, o, snap)) = split else {
                next.push(*t);
                continue;
            };
            let (fo, p0, p1, p2) = (t[3], t[j], t[(j + 1) % 3], t[(j + 2) % 3]);
            if let Some(fb) = snap.filter(|_| !snapped.contains(&o)) {
                // onto the curve both faces share (alternating projection)
                let sb = surf[fb];
                let mut m = p[o];
                for _ in 0..40 {
                    let prev = m;
                    m = project(&surf[fo], m);
                    if !sb.is_free() {
                        m = project(&sb, m);
                    }
                    if m.dist(prev) < 1e-10 {
                        break;
                    }
                }
                p[o] = m;
                snapped.insert(o);
                if let Some(nb0) = nrm.get(&nk(o, fb)).copied()
                    && !sb.is_free()
                    && !sb.is_plane()
                {
                    let nn = normal_at(&sb, m);
                    nrm.insert(nk(o, fb), if nn.dot(nb0) < 0.0 { -nn } else { nn });
                }
            }
            if !nrm.contains_key(&nk(o, fo)) {
                let (na, nb) = (normal_of(&nrm, p0, fo), normal_of(&nrm, p1, fo));
                let s0 = surf[fo];
                let mut nn = if s0.is_free() { na + nb } else { normal_at(&s0, p[o]) };
                if nn.dot(na + nb) < 0.0 {
                    nn = -nn;
                }
                nrm.insert(nk(o, fo), unit(nn));
            }
            next.push([p0, o, p2, fo]);
            next.push([o, p1, p2, fo]);
            n += 1;
        }
        tris = next;
        if n == 0 {
            break;
        }
    }

    // ---- edges that must not be split: T-junctions left in the mesh ----
    let mut frozen: HashSet<u64> = HashSet::new();
    {
        let mut faces: HashMap<u64, HashSet<usize>> = HashMap::new();
        let mut uses: HashMap<u64, u32> = HashMap::new();
        for t in &tris {
            for j in 0..3 {
                let k = ekey(t[j], t[(j + 1) % 3]);
                faces.entry(k).or_default().insert(t[3]);
                *uses.entry(k).or_default() += 1;
            }
        }
        // one-sided: a single triangle uses the edge (an edge inside a face has two)
        let one_sided = |k: u64| uses.get(&k) == Some(&1);
        let mut single: Vec<(usize, usize, u64)> = Vec::new();
        for (&k, s) in &faces {
            if s.len() > 2 {
                frozen.insert(k);
            } else if one_sided(k) {
                let (a, b) = ends(k);
                single.push((a, b, k));
            }
        }
        single.sort_unstable_by_key(|x| x.2);
        let mut face_of: HashMap<u64, usize> = HashMap::new();
        for t in &tris {
            for j in 0..3 {
                let k = ekey(t[j], t[(j + 1) % 3]);
                if one_sided(k) {
                    face_of.insert(k, t[3]);
                }
            }
        }
        let mut by_vertex: HashMap<usize, Vec<usize>> = HashMap::new();
        for (i, &(a, b, _)) in single.iter().enumerate() {
            by_vertex.entry(a).or_default().push(i);
            by_vertex.entry(b).or_default().push(i);
        }
        let near = |q: Vec3, a: Vec3, b: Vec3| {
            let ab = b - a;
            let l2 = ab.len2();
            if l2 < 1e-18 {
                return false;
            }
            let t = (q - a).dot(ab) / l2;
            if t <= 1e-6 || t >= 1.0 - 1e-6 {
                return false;
            }
            (a + ab * t).dist(q) < (0.05 * l2.sqrt()).max(1e-4)
        };
        for &(a, b, k) in &single {
            let f = face_of.get(&k).copied();
            for (v, w) in [(a, b), (b, a)] {
                let dir = unit(p[w] - p[v]);
                for &i in by_vertex.get(&v).map(Vec::as_slice).unwrap_or(&[]) {
                    let (c, d, k2) = single[i];
                    if k2 == k || face_of.get(&k2).copied() == f {
                        continue;
                    }
                    let o = if c == v { d } else { c };
                    if unit(p[o] - p[v]).dot(dir) > 0.9 || near(p[o], p[a], p[b]) {
                        frozen.insert(k);
                        frozen.insert(k2);
                    }
                }
            }
        }
        // one-sided edges of different faces lying along each other, with endpoints that
        // need not be shared
        let cell = (diag / 64.0).max(1e-3);
        let cells_of = |a: Vec3, b: Vec3| {
            let (lo, hi) = (a.min(b), a.max(b));
            let r = |x: f64| (x / cell).floor() as i64;
            let mut out = Vec::new();
            for x in r(lo.x)..=r(hi.x) {
                for y in r(lo.y)..=r(hi.y) {
                    for z in r(lo.z)..=r(hi.z) {
                        out.push((x, y, z));
                        if out.len() > 4096 {
                            return Vec::new();
                        }
                    }
                }
            }
            out
        };
        let mut grid: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
        for (i, &(a, b, _)) in single.iter().enumerate() {
            for c in cells_of(p[a], p[b]) {
                grid.entry(c).or_default().push(i);
            }
        }
        for (i, &(a, b, k)) in single.iter().enumerate() {
            let f = face_of.get(&k).copied();
            let (pa, pb) = (p[a], p[b]);
            let mid = (pa + pb) * 0.5;
            let len = pa.dist(pb);
            let mut seen = HashSet::new();
            for c in cells_of(pa, pb) {
                for &j in grid.get(&c).map(Vec::as_slice).unwrap_or(&[]) {
                    if j == i || !seen.insert(j) {
                        continue;
                    }
                    let (c2, d2, k2) = single[j];
                    if face_of.get(&k2).copied() == f {
                        continue;
                    }
                    let (pc, pd) = (p[c2], p[d2]);
                    let tol_t = (0.02 * len.min(pc.dist(pd))).max(0.02);
                    if mid.dist_to_segment(pc, pd) < tol_t || ((pc + pd) * 0.5).dist_to_segment(pa, pb) < tol_t {
                        frozen.insert(k);
                        frozen.insert(k2);
                    }
                }
            }
        }
    }

    // Inside a curved face, a diagonal joining two points of the same rim leaves a flat
    // triangle lying across the surface; splitting can't fix it (every midpoint lands on the
    // rim again). Flip an interior diagonal when the other diagonal lies closer to the surface.
    let flip_pass = |tris: &mut Vec<[usize; 4]>, p: &[Vec3]| {
        let mut flips = 0;
        for _ in 0..16 {
            let mut owner: HashMap<u64, Vec<usize>> = HashMap::new();
            for (i, t) in tris.iter().enumerate() {
                let s = surf[t[3]];
                if s.is_plane() || s.is_free() {
                    continue;
                }
                for j in 0..3 {
                    owner.entry(ekey(t[j], t[(j + 1) % 3])).or_default().push(i);
                }
            }
            let mut keys: Vec<u64> = owner.keys().copied().collect();
            keys.sort_unstable();
            let mut used = HashSet::new();
            let mut n = 0;
            for k in keys {
                let Some(l) = owner.get(&k) else { continue };
                if l.len() != 2 || used.contains(&l[0]) || used.contains(&l[1]) || frozen.contains(&k) {
                    continue;
                }
                let (t1, t2) = (tris[l[0]], tris[l[1]]);
                let fo = t1[3];
                if t2[3] != fo {
                    continue;
                }
                let sf = surf[fo];
                let (a, b) = ends(k);
                let (Some(c), Some(d)) = (t1[..3].iter().copied().find(|&v| v != a && v != b), t2[..3].iter().copied().find(|&v| v != a && v != b))
                else {
                    continue;
                };
                if c == d || owner.contains_key(&ekey(c, d)) {
                    continue;
                }
                let dev = |u: usize, v: usize| {
                    let m = (p[u] + p[v]) * 0.5;
                    project(&sf, m).dist(m)
                };
                let (dab, dcd) = (dev(a, b), dev(c, d));
                if !(dab > 0.25 * tol && dcd < 0.5 * dab) {
                    continue;
                }
                // keep t1's winding: t1 reads (x, y, c), so t2 reads (y, x, d)
                let Some(i1) = t1[..3].iter().position(|&v| v == c) else { continue };
                let (x, y) = (t1[(i1 + 1) % 3], t1[(i1 + 2) % 3]);
                let (nt1, nt2) = ([x, d, c, fo], [d, y, c, fo]);
                let side = |t: &[usize; 4]| {
                    let (pa, pb, pc) = (p[t[0]], p[t[1]], p[t[2]]);
                    let nn = (pb - pa).cross(pc - pa);
                    let len = nn.len();
                    if len < 1e-14 { 0.0 } else { nn.dot(normal_at(&sf, (pa + pb + pc) / 3.0)) / len }
                };
                let s1 = side(&t1);
                let s0 = (if s1 != 0.0 { s1 } else { side(&t2) }).signum();
                if s0 == 0.0 || side(&nt1) * s0 < 0.2 || side(&nt2) * s0 < 0.2 {
                    continue;
                }
                tris[l[0]] = nt1;
                tris[l[1]] = nt2;
                used.insert(l[0]);
                used.insert(l[1]);
                n += 1;
            }
            flips += n;
            if n == 0 {
                break;
            }
        }
        flips
    };

    // ---- adaptive subdivision ----
    let mut mids: HashMap<u64, Option<usize>> = HashMap::new();
    let tangent_in = |d: Vec3, n: Vec3| d - n * d.dot(n);
    flip_pass(&mut tris, &p);
    let mut refined = 0;
    let mut level = 0;
    while level < MAX_LEVELS {
        level += 1;
        let mut edge_faces: HashMap<u64, Vec<usize>> = HashMap::new();
        for t in &tris {
            for j in 0..3 {
                let l = edge_faces.entry(ekey(t[j], t[(j + 1) % 3])).or_default();
                if !l.contains(&t[3]) {
                    l.push(t[3]);
                }
            }
        }
        let mut keys: Vec<u64> = edge_faces.keys().copied().collect();
        keys.sort_unstable();
        let mut any = false;
        for k in keys {
            if mids.contains_key(&k) || frozen.contains(&k) {
                continue;
            }
            let Some(fs) = edge_faces.get(&k) else { continue };
            let (a, b) = ends(k);
            let (pa, pb) = (p[a], p[b]);
            let d = pb - pa;
            let len = d.len();
            if len < 1e-6 || !fs.iter().any(|&fo| !surf[fo].is_plane()) {
                mids.insert(k, None);
                continue;
            }
            // tangents at both ends: along the edge curve when two faces meet there
            let (na0, nb0) = (normal_of(&nrm, a, fs[0]), normal_of(&nrm, b, fs[0]));
            let (ta, tb) = if let [_, f1] = fs.as_slice() {
                let (na1, nb1) = (normal_of(&nrm, a, *f1), normal_of(&nrm, b, *f1));
                // the crease direction n1 × n2 is the tangent where the faces meet at a real
                // angle and it runs along the edge; on a smooth join use the chord laid into
                // the shared tangent plane
                let along = |c: Vec3, n_a: Vec3, n_b: Vec3| {
                    let l = c.len();
                    if l > 0.02 && c.dot(d).abs() / (l * len) > 0.7 { unit(c) * (c.dot(d).signum() * len) } else { tangent_in(d, unit(n_a + n_b)) }
                };
                (along(na0.cross(na1), na0, na1), along(nb0.cross(nb1), nb0, nb1))
            } else {
                (tangent_in(d, na0), tangent_in(d, nb0))
            };
            let mid0 = (pa + pb) * 0.5;
            let mut m = mid0 + (ta - tb) * 0.125;
            // keep the cubic honest on coarse, stretched triangles, the way an arc behaves
            {
                let mut off = m - mid0;
                let dn = d / len;
                if fs.len() == 1 {
                    let nav = na0 + nb0;
                    if nav.len2() > 1e-12 {
                        let nav = unit(nav);
                        off = nav * off.dot(nav);
                    }
                } else {
                    off -= dn * off.dot(dn);
                }
                let max_bulge = 0.5 * len * (angle(ta, tb).min(std::f64::consts::PI * 0.9) / 4.0).tan() * 1.5 + 1e-9;
                let ol = off.len();
                if ol > max_bulge {
                    off = off * (max_bulge / ol);
                }
                m = mid0 + off;
            }
            let exact: Vec<usize> = fs.iter().copied().filter(|&fo| !surf[fo].is_free()).collect();
            match exact.as_slice() {
                [f] => m = project(&surf[*f], m),
                [f0, f1] => {
                    for _ in 0..40 {
                        let prev = m;
                        m = project(&surf[*f1], project(&surf[*f0], m));
                        if m.dist(prev) < 1e-9 {
                            break;
                        }
                    }
                }
                _ => {}
            }
            let sag = m.dist(mid0);
            let turn =
                fs.iter().filter(|&&fo| !surf[fo].is_plane()).map(|&fo| angle(normal_of(&nrm, a, fo), normal_of(&nrm, b, fo))).fold(0.0, f64::max);
            if !(sag > tol || (turn > MAX_TURN && len > (10.0 * tol).max(0.25))) {
                mids.insert(k, None);
                continue;
            }
            let v = p.len();
            p.push(m);
            mids.insert(k, Some(v));
            any = true;
            refined += 1;
            for &fo in fs {
                let s = surf[fo];
                let (gn_a, gn_b) = (normal_of(&nrm, a, fo), normal_of(&nrm, b, fo));
                let guess = if (gn_a + gn_b).len2() < 1e-12 { gn_a } else { unit(gn_a + gn_b) };
                let mut n = if s.is_free() { guess } else { normal_at(&s, m) };
                if n.dot(guess) < 0.0 {
                    n = -n;
                }
                nrm.insert(nk(v, fo), n);
            }
        }
        if !any {
            if flip_pass(&mut tris, &p) > 0 {
                continue;
            }
            break;
        }
        // split triangles; unsplit edges keep their neighbours conforming
        let mut next = Vec::with_capacity(tris.len() * 2);
        for t in &tris {
            let fo = t[3];
            let mm: [Option<usize>; 3] = std::array::from_fn(|j| mids.get(&ekey(t[j], t[(j + 1) % 3])).copied().flatten());
            let cnt = mm.iter().filter(|x| x.is_some()).count();
            if cnt == 0 {
                next.push(*t);
                continue;
            }
            let (mut v, mut m) = ([t[0], t[1], t[2]], mm);
            for _ in 0..3 {
                if cnt == 3 || (cnt == 1 && m[0].is_some()) || (cnt == 2 && m[0].is_some() && m[1].is_some()) {
                    break;
                }
                v = [v[1], v[2], v[0]];
                m = [m[1], m[2], m[0]];
            }
            let [v0, v1, v2] = v;
            match (cnt, m) {
                (1, [Some(m01), _, _]) => {
                    next.push([v0, m01, v2, fo]);
                    next.push([m01, v1, v2, fo]);
                }
                (2, [Some(m01), Some(m12), _]) => {
                    next.push([m01, v1, m12, fo]);
                    if p[v0].dist(p[m12]) < p[m01].dist(p[v2]) {
                        next.push([v0, m01, m12, fo]);
                        next.push([v0, m12, v2, fo]);
                    } else {
                        next.push([v0, m01, v2, fo]);
                        next.push([m01, m12, v2, fo]);
                    }
                }
                (3, [Some(m01), Some(m12), Some(m20)]) => {
                    next.push([v0, m01, m20, fo]);
                    next.push([m01, v1, m12, fo]);
                    next.push([m20, m12, v2, fo]);
                    next.push([m01, m12, m20, fo]);
                }
                _ => next.push(*t),
            }
        }
        tris = next;
        flip_pass(&mut tris, &p);
    }
    if refined == 0 {
        return None;
    }
    let as_u32 = |i: usize| u32::try_from(i).unwrap_or(u32::MAX);
    Some(Refined {
        triangles: tris.iter().map(|t| [as_u32(t[0]), as_u32(t[1]), as_u32(t[2])]).collect(),
        tri_face: tris.iter().map(|t| face_ids.get(t[3]).copied().unwrap_or(0)).collect(),
        positions: p,
    })
}

// ---------- surfaces ----------
fn project(s: &Surf, q: Vec3) -> Vec3 {
    match *s {
        Surf::Plane { n, d } => q + n * (d - n.dot(q)),
        Surf::Sphere { c, r } => {
            let v = q - c;
            let l = v.len();
            if l < 1e-12 { q } else { c + v * (r / l) }
        }
        Surf::Rev { c, a, prof } => {
            let v = q - c;
            let h = v.dot(a);
            let w = v - a * h;
            let rho = w.len();
            if rho < 1e-12 {
                return q;
            }
            let (h2, r2) = project_profile(prof, h, rho);
            c + a * h2 + (w / rho) * r2
        }
        Surf::Free => q,
    }
}
fn project_profile(pr: Profile, h: f64, r: f64) -> (f64, f64) {
    match pr {
        Profile::Line { h: ph, r: pr_, dh, dr } => {
            let t = (h - ph) * dh + (r - pr_) * dr;
            (ph + t * dh, pr_ + t * dr)
        }
        Profile::Circle { h: ph, r: pr_, rad } => {
            let (dh, dr) = (h - ph, r - pr_);
            let l = dh.hypot(dr);
            let l = if l == 0.0 { 1.0 } else { l };
            (ph + dh / l * rad, pr_ + dr / l * rad)
        }
    }
}
fn normal_at(s: &Surf, q: Vec3) -> Vec3 {
    match *s {
        Surf::Plane { n, .. } => n,
        Surf::Sphere { c, .. } => unit(q - c),
        Surf::Rev { c, a, prof } => {
            let v = q - c;
            let h = v.dot(a);
            let w = v - a * h;
            let rho = w.len();
            let w = if rho > 0.0 { w / rho } else { w };
            let (nh, nr) = match prof {
                Profile::Line { dh, dr, .. } => (-dr, dh),
                Profile::Circle { h: ph, r: pr_, .. } => {
                    let (nh, nr) = (h - ph, rho - pr_);
                    let l = nh.hypot(nr);
                    let l = if l == 0.0 { 1.0 } else { l };
                    (nh / l, nr / l)
                }
            };
            unit(a * nh + w * nr)
        }
        Surf::Free => Vec3::new(0.0, 0.0, 1.0),
    }
}

/// Fit a face from its vertices and their exact normals.
fn fit(ps: &[Vec3], ns: &[Vec3]) -> Surf {
    let n = ps.len();
    if n == 0 {
        return Surf::Free;
    }
    let (lo, hi) = ps.iter().fold((ps[0], ps[0]), |(lo, hi), q| (lo.min(*q), hi.max(*q)));
    let size = (hi - lo).len().max(1e-6);
    let tol_p = 5e-4 + 2e-6 * size;
    let tol_n = 2e-4;
    let ok_all = |s: &Surf| ps.iter().zip(ns).all(|(q, nn)| project(s, *q).dist(*q) < tol_p && normal_at(s, *q).dot(*nn).abs() > 1.0 - tol_n);
    // plane
    let mean = ns.iter().fold(Vec3::new(0.0, 0.0, 0.0), |acc, x| acc + *x);
    if mean.len2() > 1e-12 {
        let nn = unit(mean);
        if ns.iter().all(|x| x.dot(nn) > 1.0 - 1e-6) {
            let pl = Surf::Plane { n: nn, d: ps.iter().map(|q| nn.dot(*q)).sum::<f64>() / n as f64 };
            if ok_all(&pl) {
                return pl;
            }
        }
    }
    if n < 4 {
        return Surf::Free;
    }
    // sphere: the point all normal lines pass through
    {
        let mut a = [[0.0; 3]; 3];
        let mut b = [0.0; 3];
        for (q, nn) in ps.iter().zip(ns) {
            let v = [nn.x, nn.y, nn.z];
            let pq = [q.x, q.y, q.z];
            for r in 0..3 {
                for c in 0..3 {
                    let m = if r == c { 1.0 } else { 0.0 } - v[r] * v[c];
                    a[r][c] += m;
                    b[r] += m * pq[c];
                }
            }
        }
        if let Some(c) = solve3(a, b) {
            let c = Vec3::new(c[0], c[1], c[2]);
            let r = ps.iter().map(|q| q.dist(c)).sum::<f64>() / n as f64;
            if r > 1e-3 && r < 1e4 * size {
                let sp = Surf::Sphere { c, r };
                if ok_all(&sp) {
                    return sp;
                }
            }
        }
    }
    // surface of revolution: every normal line meets the axis. In Plücker terms the line
    // (p, n) meets the axis (a, m) when a·(p×n) + m·n = 0: a null vector of a 6×6 system.
    {
        let o = ps.iter().fold(Vec3::new(0.0, 0.0, 0.0), |acc, q| acc + *q) / n as f64;
        let mut mat = [[0.0; 6]; 6];
        for (q, nn) in ps.iter().zip(ns) {
            let pq = (*q - o) / size;
            let m = pq.cross(*nn);
            let r = [m.x, m.y, m.z, nn.x, nn.y, nn.z];
            for x in 0..6 {
                for y in 0..6 {
                    mat[x][y] += r[x] * r[y];
                }
            }
        }
        let (values, vectors) = eigen_sym6(mat);
        let mut idx: Vec<usize> = (0..6).collect();
        idx.sort_by(|&x, &y| values[x].total_cmp(&values[y]));
        // null space: one vector, or two for a cylinder (the second is the line at infinity
        // along the axis). Three or more: a sphere or a strip too narrow to say.
        let small = 1e-8 * n as f64;
        let nullity = idx.iter().filter(|&&i| values[i] < small).count();
        if (1..=2).contains(&nullity) && values[idx[nullity]] > 1e-6 * n as f64 {
            let col = |c: usize| -> [f64; 6] { std::array::from_fn(|r| vectors[r][c]) };
            let mut e = col(idx[0]);
            if nullity == 2 {
                // the combination with the largest direction part
                let e2 = col(idx[1]);
                let g11 = e[0] * e[0] + e[1] * e[1] + e[2] * e[2];
                let g22 = e2[0] * e2[0] + e2[1] * e2[1] + e2[2] * e2[2];
                let g12 = e[0] * e2[0] + e[1] * e2[1] + e[2] * e2[2];
                let phi = 0.5 * (2.0 * g12).atan2(g11 - g22);
                e = std::array::from_fn(|i| phi.cos() * e[i] + phi.sin() * e2[i]);
            }
            let a = Vec3::new(e[0], e[1], e[2]);
            let mm = Vec3::new(e[3], e[4], e[5]);
            let al = a.len();
            if al > 1e-6 {
                let (a, mm) = (a / al, mm / al);
                let c = a.cross(mm) * size + o;
                let hr: Vec<(f64, f64)> = ps
                    .iter()
                    .map(|q| {
                        let v = *q - c;
                        let h = v.dot(a);
                        (h, (v - a * h).len())
                    })
                    .collect();
                let span = |f: fn(&(f64, f64)) -> f64| {
                    let (mn, mx) = hr.iter().map(f).fold((f64::MAX, f64::MIN), |(mn, mx), x| (mn.min(x), mx.max(x)));
                    mx - mn
                };
                if span(|x| x.0).max(span(|x| x.1)) > 10.0 * tol_p {
                    for prof in [fit_line(&hr), fit_circle(&hr)].into_iter().flatten() {
                        let s = Surf::Rev { c, a, prof };
                        if ok_all(&s) {
                            return s;
                        }
                    }
                }
            }
        }
    }
    Surf::Free
}
fn fit_line(pts: &[(f64, f64)]) -> Option<Profile> {
    let n = pts.len() as f64;
    if n == 0.0 {
        return None;
    }
    let mh = pts.iter().map(|x| x.0).sum::<f64>() / n;
    let mr = pts.iter().map(|x| x.1).sum::<f64>() / n;
    let (mut shh, mut shr, mut srr) = (0.0, 0.0, 0.0);
    for &(h, r) in pts {
        shh += (h - mh) * (h - mh);
        shr += (h - mh) * (r - mr);
        srr += (r - mr) * (r - mr);
    }
    let ang = 0.5 * (2.0 * shr).atan2(shh - srr);
    Some(Profile::Line { h: mh, r: mr, dh: ang.cos(), dr: ang.sin() })
}
fn fit_circle(pts: &[(f64, f64)]) -> Option<Profile> {
    let mut a = [[0.0; 3]; 3];
    let mut b = [0.0; 3];
    for &(x, y) in pts {
        let row = [x, y, 1.0];
        let rhs = -(x * x + y * y);
        for i in 0..3 {
            b[i] += row[i] * rhs;
            for j in 0..3 {
                a[i][j] += row[i] * row[j];
            }
        }
    }
    let s = solve3(a, b)?;
    let (h, r) = (-s[0] / 2.0, -s[1] / 2.0);
    let rad2 = h * h + r * r - s[2];
    (rad2 > 0.0).then(|| Profile::Circle { h, r, rad: rad2.sqrt() })
}

// ---------- small linear algebra ----------
// Row and column indices read as the textbook elimination.
#[allow(clippy::needless_range_loop)]
fn solve3(a: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let mut m = [[a[0][0], a[0][1], a[0][2], b[0]], [a[1][0], a[1][1], a[1][2], b[1]], [a[2][0], a[2][1], a[2][2], b[2]]];
    for c in 0..3 {
        let mut piv = c;
        for r in c + 1..3 {
            if m[r][c].abs() > m[piv][c].abs() {
                piv = r;
            }
        }
        if m[piv][c].abs() < 1e-12 {
            return None;
        }
        m.swap(c, piv);
        for r in 0..3 {
            if r != c {
                let k = m[r][c] / m[c][c];
                for k2 in c..4 {
                    m[r][k2] -= k * m[c][k2];
                }
            }
        }
    }
    Some([m[0][3] / m[0][0], m[1][3] / m[1][1], m[2][3] / m[2][2]])
}
/// Cyclic Jacobi for a symmetric 6×6 matrix; eigenvectors are the columns of the second result.
#[allow(clippy::needless_range_loop)]
fn eigen_sym6(s: [[f64; 6]; 6]) -> ([f64; 6], [[f64; 6]; 6]) {
    let mut a = s;
    let mut v = [[0.0; 6]; 6];
    for (i, row) in v.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for _ in 0..60 {
        let mut off = 0.0;
        for p in 0..6 {
            for q in p + 1..6 {
                off += a[p][q] * a[p][q];
            }
        }
        if off < 1e-30 {
            break;
        }
        for p in 0..6 {
            for q in p + 1..6 {
                if a[p][q].abs() < 1e-300 {
                    continue;
                }
                let th = 0.5 * (2.0 * a[p][q]).atan2(a[q][q] - a[p][p]);
                let (c, sn) = (th.cos(), th.sin());
                for row in a.iter_mut() {
                    let (kp, kq) = (row[p], row[q]);
                    row[p] = c * kp - sn * kq;
                    row[q] = sn * kp + c * kq;
                }
                for k in 0..6 {
                    let (pk, qk) = (a[p][k], a[q][k]);
                    a[p][k] = c * pk - sn * qk;
                    a[q][k] = sn * pk + c * qk;
                }
                for row in v.iter_mut() {
                    let (kp, kq) = (row[p], row[q]);
                    row[p] = c * kp - sn * kq;
                    row[q] = sn * kp + c * kq;
                }
            }
        }
    }
    (std::array::from_fn(|i| a[i][i]), v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A coarse cylinder (10 sides, r 10, h 5) as Plasticity caches it: side faces split per
    /// segment into one face, caps as fans; exact normals at the corners.
    fn coarse_cylinder() -> (Vec<Vec3>, Vec<Vec3>, Vec<u32>) {
        let (r, h, n) = (10.0, 5.0, 10usize);
        let pt = |i: usize, z: f64| {
            let t = (i % n) as f64 / n as f64 * std::f64::consts::TAU;
            Vec3::new(r * t.cos(), r * t.sin(), z)
        };
        let (mut c, mut nn, mut f) = (Vec::new(), Vec::new(), Vec::new());
        for i in 0..n {
            let (a0, a1, b0, b1) = (pt(i, 0.0), pt(i + 1, 0.0), pt(i, h), pt(i + 1, h));
            let rad = |q: Vec3| unit(Vec3::new(q.x, q.y, 0.0));
            for tri in [[a0, a1, b1], [a0, b1, b0]] {
                for q in tri {
                    c.push(q);
                    nn.push(rad(q));
                }
                f.push(0);
            }
            for (z, face, flip) in [(0.0, 1, true), (h, 2, false)] {
                let (p0, p1) = (pt(i, z), pt(i + 1, z));
                let tri = if flip { [Vec3::new(0.0, 0.0, z), p1, p0] } else { [Vec3::new(0.0, 0.0, z), p0, p1] };
                for q in tri {
                    c.push(q);
                    nn.push(Vec3::new(0.0, 0.0, if flip { -1.0 } else { 1.0 }));
                }
                f.push(face);
            }
        }
        (c, nn, f)
    }

    #[test]
    fn a_coarse_cylinder_comes_back_round_and_watertight() {
        let (c, n, f) = coarse_cylinder();
        let r = refine(&c, &n, &f).unwrap();
        // every side vertex on the true circle
        for (t, face) in r.triangles.iter().zip(&r.tri_face) {
            if *face == 0 {
                for &v in t {
                    let q = r.positions[v as usize];
                    assert!((q.x.hypot(q.y) - 10.0).abs() < 1e-6, "{q:?}");
                }
            }
        }
        // finer than the 10 sides it came with, within the chord tolerance (0.01 mm here)
        let side_tris = r.tri_face.iter().filter(|f| **f == 0).count();
        assert!(side_tris > 20, "{side_tris}");
        // closed: every edge used twice
        let mut uses: HashMap<u64, u32> = HashMap::new();
        for t in &r.triangles {
            for j in 0..3 {
                *uses.entry(ekey(t[j] as usize, t[(j + 1) % 3] as usize)).or_default() += 1;
            }
        }
        assert!(uses.values().all(|&u| u == 2), "open edges: {}", uses.values().filter(|&&u| u != 2).count());
    }

    #[test]
    fn flat_only_meshes_are_left_alone() {
        let c = vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)];
        let n = vec![Vec3::new(0.0, 0.0, 1.0); 3];
        assert!(refine(&c, &n, &[0]).is_none());
    }
}

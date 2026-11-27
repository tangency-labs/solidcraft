//! The source's faceted solid: adjacent triangles on one seed plane become one face.
//! Directed source edges retain their subdivisions and material strands at shared vertices.
//!
//! Deviations: no upstream counterpart. No recursion; all walks are source-count bounded.

use std::collections::{BTreeMap, BTreeSet};

use crate::curve::curve::Curve;
use crate::curve::line::Line;
use crate::kernel::Kernel;
use crate::mass::integrator::properties::no_unsure_region;
use crate::math::Vec3;
use crate::query::poly::{PolyBrep, PolyEdgeId};
use crate::query::poly_properties::poly_brep_compute_volume;
use crate::topology::brep::{
    is_manifold_solid,
    validation::{check_brep, CheckLevel},
};
use crate::{Brep, Result};

use super::geometry::{normalize, MeshPlane};
use super::topology::{self, Face, Region, Side};
use super::{invalid, native, TriMesh};

const COPLANAR: f64 = 1e-7;
type Owners = BTreeMap<(u32, u32), usize>;
type SourceShells = (Vec<Vec<usize>>, Vec<f64>, Vec<Vec<usize>>);

fn point(mesh: &TriMesh, i: u32) -> Result<Vec3> {
    mesh.positions
        .get(i as usize)
        .copied()
        .ok_or_else(|| invalid("missing facet vertex"))
}
fn triangle(mesh: &TriMesh, i: usize) -> Result<[u32; 3]> {
    mesh.triangles
        .get(i)
        .copied()
        .ok_or_else(|| invalid("missing facet triangle"))
}
fn sides([a, b, c]: [u32; 3]) -> [(u32, u32); 3] {
    [(a, b), (b, c), (c, a)]
}

/// Retain the original triangle shell, sharing the source's exact edge topology.
pub(super) fn build(k: &mut Kernel, mesh: &TriMesh) -> Result<Brep> {
    let mut touching = BTreeMap::<(u32, u32), Vec<usize>>::new();
    let mut owner = Owners::new();
    for (i, &t) in mesh.triangles.iter().enumerate() {
        for (a, b) in sides(t) {
            touching.entry((a.min(b), a.max(b))).or_default().push(i);
            owner.insert((a, b), i);
        }
    }
    let mut groups = vec![None; mesh.triangles.len()];
    let mut planes = Vec::new();
    for seed in 0..mesh.triangles.len() {
        if groups.get(seed).copied().flatten().is_some() {
            continue;
        }
        k.check_cancelled("reconstruct mesh")?;
        let [a, b, c] = triangle(mesh, seed)?;
        let (a, b, c) = (point(mesh, a)?, point(mesh, b)?, point(mesh, c)?);
        let n = normalize((b - a).cross(c - a)).ok_or_else(|| invalid("degenerate facet"))?;
        let group = planes.len();
        let distance = n.dot(a);
        planes.push((n, distance));
        *groups
            .get_mut(seed)
            .ok_or_else(|| invalid("missing facet group"))? = Some(group);
        let mut queue = vec![seed];
        while let Some(t) = queue.pop() {
            for (a, b) in sides(triangle(mesh, t)?) {
                for &u in touching.get(&(a.min(b), a.max(b))).into_iter().flatten() {
                    if groups.get(u).copied().flatten().is_some() {
                        continue;
                    }
                    let coplanar = triangle(mesh, u)?.iter().all(|&v| {
                        point(mesh, v).is_ok_and(|p| (n.dot(p) - distance).abs() <= COPLANAR)
                    });
                    if coplanar {
                        *groups
                            .get_mut(u)
                            .ok_or_else(|| invalid("missing facet group"))? = Some(group);
                        queue.push(u);
                    }
                }
            }
        }
    }
    let mut uses = BTreeMap::<(usize, (u32, u32)), Vec<(u32, u32)>>::new();
    for (i, &t) in mesh.triangles.iter().enumerate() {
        let g = groups
            .get(i)
            .copied()
            .flatten()
            .ok_or_else(|| invalid("missing facet group"))?;
        for (a, b) in sides(t) {
            uses.entry((g, (a.min(b), a.max(b))))
                .or_default()
                .push((a, b));
        }
    }
    let mut rims = vec![Vec::new(); planes.len()];
    for ((g, _), directed) in uses {
        if let [side] = directed.as_slice() {
            rims.get_mut(g)
                .ok_or_else(|| invalid("missing facet rim"))?
                .push(*side);
        }
    }
    let mut faces = Vec::new();
    let mut polygons = Vec::new();
    for (g, sides) in rims.iter().enumerate() {
        let &(normal, _) = planes
            .get(g)
            .ok_or_else(|| invalid("missing facet plane"))?;
        let loops = face_loops(mesh, sides, &owner, normal)?;
        let first = loops
            .first()
            .and_then(|l| l.first())
            .copied()
            .ok_or_else(|| invalid("empty facet boundary"))?;
        let frame = MeshPlane::from_normal(first, normal)
            .ok_or_else(|| invalid("invalid facet plane"))?
            .frame();
        let surface =
            native::plane_surface(&frame, &loops.iter().flatten().copied().collect::<Vec<_>>())?;
        let domain = surface.natural_uv_domain_with_nurb(k)?;
        polygons.push(loops.clone());
        let mut wires = Vec::new();
        for lp in loops {
            let mut wire = Vec::new();
            for (&a, &b) in lp.iter().zip(lp.iter().cycle().skip(1)).take(lp.len()) {
                wire.push(Side::new(Curve::from(Line::create_line_segment(
                    3, a, b, None,
                )?)));
            }
            wires.push(wire);
        }
        faces.push(Face {
            loops: wires,
            surface,
            domain,
            forward: true,
        });
    }
    let (shells, volumes, triangles) = shells(mesh, &touching, &groups)?;
    let outer: Vec<_> = volumes
        .iter()
        .enumerate()
        .filter_map(|(i, &v)| (v > 0.).then_some(i))
        .collect();
    let mut cavities = vec![Vec::new(); shells.len()];
    for (i, &v) in volumes.iter().enumerate() {
        if v >= 0. {
            continue;
        }
        let q = triangles
            .get(i)
            .and_then(|ts| ts.first())
            .and_then(|t| mesh.triangles.get(*t))
            .and_then(|t| t.first())
            .copied()
            .ok_or_else(|| invalid("empty cavity"))?;
        let q = point(mesh, q)?;
        let mut best: Option<(f64, usize)> = None;
        for &o in &outer {
            k.check_cancelled("reconstruct mesh")?;
            let contains = if outer.len() == 1 {
                true
            } else {
                winding(mesh, triangles.get(o).map_or(&[], Vec::as_slice), q)?.abs() > 0.5
            };
            let size = volumes.get(o).copied().unwrap_or(f64::INFINITY);
            if contains && best.is_none_or(|(v, _)| size < v) {
                best = Some((size, o));
            }
        }
        let (_, o) = best.ok_or_else(|| invalid("a cavity lies in no solid"))?;
        cavities
            .get_mut(o)
            .ok_or_else(|| invalid("missing cavity shell"))?
            .push(i);
    }
    let mut regions = vec![Region {
        void: true,
        shells: outer.clone(),
    }];
    for &o in &outer {
        let inner = cavities
            .get(o)
            .ok_or_else(|| invalid("missing shell cavities"))?;
        let mut list = vec![o];
        list.extend(inner);
        regions.push(Region {
            void: false,
            shells: list,
        });
        for &c in inner {
            regions.push(Region {
                void: true,
                shells: vec![c],
            });
        }
    }
    let rebuilt = polygonal_volume(k, &polygons, &shells)?;
    let mut body = topology::build(k, faces, &shells, &regions)?;
    let check = check_brep(k, &mut body, CheckLevel::Level0)?;
    if !is_manifold_solid(&body)?
        || !matches!(
            check.pointers,
            crate::topology::brep::validation::PointerCheck::Passed
        )
        || check.assert_valid != Some(true)
    {
        return Err(invalid("faceted solid did not pass validity checks"));
    }
    no_unsure_region(&body)?;
    let original = mesh.volume();
    if !original.is_finite()
        || original <= 0.
        || !rebuilt.is_finite()
        || rebuilt <= 0.
        || (rebuilt - original).abs() > original.abs() * 1e-6 + COPLANAR * mesh.size().powi(2)
    {
        return Err(invalid("faceted volume differs from the source mesh"));
    }
    Ok(body)
}

/// The kernel's exact polygonal volume primitive, on the produced face loops. Faceted
/// validity and source-volume acceptance do not integrate thousands of planar surfaces.
fn polygonal_volume(k: &Kernel, faces: &[Vec<Vec<Vec3>>], shells: &[Vec<usize>]) -> Result<f64> {
    let mut poly = PolyBrep::new(k, 5e-7);
    let region = poly.add_region()?;
    let mut vertices = BTreeMap::new();
    let mut uses = BTreeMap::<(usize, usize), Vec<PolyEdgeId>>::new();
    for list in shells {
        let shell = poly.add_shell(region)?;
        for &fi in list {
            let face = poly.add_face(k, shell, 5e-7, None)?;
            for lp in faces
                .get(fi)
                .ok_or_else(|| invalid("missing polygonal facet"))?
            {
                let ploop = poly.start_poly_edge_loop(face)?;
                for (&a, &b) in lp.iter().zip(lp.iter().cycle().skip(1)).take(lp.len()) {
                    let mut at = |p: Vec3| -> Result<_> {
                        let key = (p.x.to_bits(), p.y.to_bits(), p.z.to_bits());
                        if let Some(&v) = vertices.get(&key) {
                            return Ok(v);
                        }
                        let v = poly.add_vertex(k, p, 5e-7)?;
                        let i = vertices.len();
                        vertices.insert(key, (i, v));
                        Ok((i, v))
                    };
                    let (ai, av) = at(a)?;
                    let (bi, _) = at(b)?;
                    let e = poly.add_poly_edge(k, ploop, 5e-7, a, b, av)?;
                    uses.entry((ai.min(bi), ai.max(bi))).or_default().push(e);
                }
                poly.finish_poly_edge_loop(ploop)?;
            }
        }
    }
    for pair in uses.values() {
        poly.link_radials(pair)?;
    }
    poly_brep_compute_volume(k, &mut poly)
}

/// Source triangles, grouped through edges; planar groups cannot cross those components.
fn shells(
    mesh: &TriMesh,
    touching: &BTreeMap<(u32, u32), Vec<usize>>,
    groups: &[Option<usize>],
) -> Result<SourceShells> {
    let mut seen = vec![false; mesh.triangles.len()];
    let (mut shells, mut volumes, mut triangles) = (Vec::new(), Vec::new(), Vec::new());
    for seed in 0..mesh.triangles.len() {
        if seen.get(seed).copied().unwrap_or(true) {
            continue;
        }
        let mut queue = vec![seed];
        let mut faces = BTreeSet::new();
        let mut ts = Vec::new();
        let mut volume = 0.;
        let [start, _, _] = triangle(mesh, seed)?;
        let origin = point(mesh, start)?;
        while let Some(i) = queue.pop() {
            let slot = seen
                .get_mut(i)
                .ok_or_else(|| invalid("missing shell triangle"))?;
            if *slot {
                continue;
            }
            *slot = true;
            let t = triangle(mesh, i)?;
            let [a, b, c] = t;
            volume += (point(mesh, a)? - origin)
                .dot((point(mesh, b)? - origin).cross(point(mesh, c)? - origin))
                / 6.;
            ts.push(i);
            faces.insert(
                groups
                    .get(i)
                    .copied()
                    .flatten()
                    .ok_or_else(|| invalid("missing shell face"))?,
            );
            for (a, b) in sides(t) {
                queue.extend(
                    touching
                        .get(&(a.min(b), a.max(b)))
                        .into_iter()
                        .flatten()
                        .copied(),
                );
            }
        }
        if !volume.is_finite() || volume == 0. {
            return Err(invalid("a closed shell encloses no volume"));
        }
        shells.push(faces.into_iter().collect());
        volumes.push(volume);
        triangles.push(ts);
    }
    Ok((shells, volumes, triangles))
}

/// The exact solid-angle integral of each planar source triangle, divided by a full sphere.
/// Only shell nesting reads it; the reconstructed faces remain the source's own facets.
fn winding(mesh: &TriMesh, triangles: &[usize], q: Vec3) -> Result<f64> {
    let mut angle = 0.;
    for &i in triangles {
        let [a, b, c] = triangle(mesh, i)?;
        let (a, b, c) = (
            point(mesh, a)? - q,
            point(mesh, b)? - q,
            point(mesh, c)? - q,
        );
        let (la, lb, lc) = (a.length(), b.length(), c.length());
        angle += 2.
            * a.dot(b.cross(c))
                .atan2(la * lb * lc + a.dot(b) * lc + b.dot(c) * la + c.dot(a) * lb);
    }
    Ok(angle / (4. * std::f64::consts::PI))
}

/// Follow the strand reached by rotating through this group's incident source triangles.
fn face_loops(
    mesh: &TriMesh,
    sides: &[(u32, u32)],
    owner: &Owners,
    normal: Vec3,
) -> Result<Vec<Vec<Vec3>>> {
    let boundary: BTreeSet<_> = sides.iter().copied().collect();
    let mut used = BTreeSet::new();
    let mut loops = Vec::new();
    for &(start, into) in sides {
        if !used.insert((start, into)) {
            continue;
        }
        let (mut previous, mut current) = (start, into);
        let mut chain = vec![point(mesh, start)?];
        loop {
            chain.push(point(mesh, current)?);
            if chain.len() > sides.len() + 1 {
                return Err(invalid("a planar group's boundary does not close"));
            }
            let mut side = (previous, current);
            let mut next = None;
            for _ in 0..=mesh.triangles.len() {
                let &t = owner
                    .get(&side)
                    .ok_or_else(|| invalid("a facet side has no triangle"))?;
                let third = triangle(mesh, t)?
                    .into_iter()
                    .find(|&v| v != side.0 && v != side.1)
                    .ok_or_else(|| invalid("degenerate facet side"))?;
                if boundary.contains(&(current, third)) {
                    next = Some(third);
                    break;
                }
                side = (third, current);
            }
            let next = next.ok_or_else(|| invalid("a vertex has no way around its triangles"))?;
            used.insert((current, next));
            previous = current;
            if next == start {
                break;
            }
            current = next;
        }
        if chain.len() < 3 {
            return Err(invalid("a planar group's boundary has no loop"));
        }
        loops.push(chain);
    }
    let signed = |lp: &Vec<Vec3>| {
        lp.iter()
            .zip(lp.iter().cycle().skip(1))
            .take(lp.len())
            .fold(Vec3::ZERO, |v, (&a, &b)| v + a.cross(b))
            .dot(normal)
    };
    let outer = loops
        .iter()
        .position(|lp| signed(lp) > 0.)
        .ok_or_else(|| invalid("a planar group's boundary does not enclose it"))?;
    if let Some(index) = outer.checked_sub(1) {
        let (first, remaining) = loops
            .split_first_mut()
            .ok_or_else(|| invalid("a planar group has no boundary loops"))?;
        let outer_loop = remaining
            .get_mut(index)
            .ok_or_else(|| invalid("a planar group is missing its outer boundary loop"))?;
        std::mem::swap(first, outer_loop);
    }
    Ok(loops)
}

//! Physical properties of an assembly placement, with original body material assignments.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use solidcraft_doc::{Document, Mat};
use solidcraft_geom::inertia::{self, Tensor3};
use solidcraft_geom::{Aabb3, Vec3};

use crate::params::bad;
use crate::{Result, Session};

fn descendants(p: &Value, cmd: &str) -> Result<bool> {
    match p.get("descendants") {
        None => Ok(true),
        Some(Value::Bool(v)) => Ok(*v),
        Some(_) => Err(bad(cmd, "`descendants` must be a boolean")),
    }
}

/// Principal moments this close (a share of the largest) count as equal: a round body's
/// tessellation tells its two equal moments apart by about 1e-4, which turns their axes at random.
const EQUAL_MOMENTS: f64 = 1e-3;

/// The point `about` asks for the inertia tensor about, if any.
fn about(p: &Value, cmd: &str) -> Result<Option<Vec3>> {
    match p.get("about") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => crate::params::vec3(v).map(Some).ok_or_else(|| bad(cmd, "`about` must be a point [x, y, z] in mm")),
    }
}

/// The 3 × 3 linear part of a placement, row by row: its turn, and any scale or mirror.
fn linear(m: &Mat) -> Tensor3 {
    [0, 1, 2].map(|i| [0, 1, 2].map(|j| m[j][i]))
}

fn placed_doc(s: &Session) -> Document {
    let mut doc = (*s.doc).clone();
    for occurrence in &mut doc.occurrences {
        if let Some(transform) = s.pending_moves.get(&occurrence.id) {
            occurrence.transform = *transform;
        }
    }
    doc
}

/// A property total must cover the entire assembly. The display traversal intentionally
/// truncates hostile designs, so use an explicit error instead of accepting its prefix.
pub(in crate::cmd) fn placements(doc: &Document, cmd: &str) -> Result<Vec<(u64, Vec<u64>, Mat)>> {
    subtree(doc, (0, Vec::new(), solidcraft_doc::IDENTITY), true, cmd)
}

/// `root` and, with `descendants`, every placement under it, each with its world transform.
/// The occurrences are grouped by parent once: searching all of them at every placement made a
/// large assembly's traversal quadratic.
fn subtree(doc: &Document, root: (u64, Vec<u64>, Mat), descendants: bool, cmd: &str) -> Result<Vec<(u64, Vec<u64>, Mat)>> {
    let mut out = vec![root.clone()];
    if !descendants {
        return Ok(out);
    }
    let mut stack = vec![root];
    let mut children: std::collections::BTreeMap<u64, Vec<_>> = std::collections::BTreeMap::new();
    for occurrence in &doc.occurrences {
        children.entry(occurrence.parent).or_default().push(occurrence);
    }
    while let Some((component, path, transform)) = stack.pop() {
        for occurrence in children.get(&component).into_iter().flatten() {
            if path.len() >= 64 || out.len() >= 100_000 {
                return Err(bad(cmd, "assembly exceeds physical-properties traversal limits (64 levels or 100000 placements)"));
            }
            let mut child_path = path.clone();
            child_path.push(occurrence.id);
            let world = solidcraft_doc::mat_mul(&transform, &occurrence.transform);
            out.push((occurrence.component, child_path.clone(), world));
            stack.push((occurrence.component, child_path, world));
        }
    }
    Ok(out)
}

pub(super) fn component(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.properties";
    let id = if p.get("component").is_some() { super::component(s, p, "component", cmd)? } else { s.active_component };
    let doc = placed_doc(s);
    let path = doc.first_path(id);
    let name = if id == 0 {
        doc.name.clone()
    } else {
        doc.components.iter().find(|c| c.id == id).map(|c| c.name.clone()).ok_or_else(|| bad(cmd, "no active component"))?
    };
    let mut out = properties(s, &doc, &path, descendants(p, cmd)?, about(p, cmd)?, cmd)?;
    out["component"] = json!(id);
    out["name"] = json!(name);
    Ok(out)
}

pub(super) fn occurrence(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "occurrence.properties";
    let id = super::occurrence(s, p, cmd)?;
    let doc = placed_doc(s);
    let path = if p.get("path").is_some() {
        super::occurrence_path(s, p, id, cmd)?
    } else {
        let paths: Vec<Vec<u64>> =
            placements(&doc, cmd)?.into_iter().filter(|(_, path, _)| path.last() == Some(&id)).map(|(_, path, _)| path).collect();
        match paths.as_slice() {
            [path] => path.clone(),
            [] => return Err(bad(cmd, "occurrence has no placement")),
            _ => return Err(bad(cmd, "occurrence has multiple placements; supply its `path`")),
        }
    };
    let mut out = properties(s, &doc, &path, descendants(p, cmd)?, about(p, cmd)?, cmd)?;
    out["occurrence"] = json!(id);
    out["name"] = json!(doc.occurrences.iter().find(|o| o.id == id).map(|o| o.name.as_str()));
    Ok(out)
}

/// The linear part of `m` is a rotation (no scale, shear or mirror), to rounding.
fn proper_rotation(m: &Mat) -> bool {
    let column = |i: usize| m.get(i).map_or(Vec3::ZERO, |c| Vec3::new(c[0], c[1], c[2]));
    let (x, y, z) = (column(0), column(1), column(2));
    m.iter().flatten().all(|v| v.is_finite())
        && [x, y, z].iter().all(|a| (a.dot(*a) - 1.0).abs() < 1e-9)
        && x.dot(y).abs() < 1e-9
        && x.dot(z).abs() < 1e-9
        && y.dot(z).abs() < 1e-9
        && x.cross(y).dot(z) > 0.0
}

/// Every axis of `m` goes to an axis (a quarter turn, or none): a box turned so stays a box.
fn square_to_axes(m: &Mat) -> bool {
    m.iter().take(3).all(|c| c.iter().take(3).filter(|v| v.abs() > 1e-12).count() == 1)
}

/// The box around the corners of `b` placed by `m`.
fn placed_box(m: &Mat, b: &Aabb3) -> Aabb3 {
    let mut out = Aabb3::EMPTY;
    if b.is_empty() {
        return out;
    }
    for i in 0..8 {
        let pick = |bit: usize, lo: f64, hi: f64| if i & bit == 0 { lo } else { hi };
        out.add(solidcraft_doc::apply_point(m, Vec3::new(pick(1, b.min.x, b.max.x), pick(2, b.min.y, b.max.y), pick(4, b.min.z, b.max.z))));
    }
    out
}

/// A body's measurements where a placement puts it.
struct Placed {
    volume: f64,
    area: f64,
    centroid: Vec3,
    bbox: Aabb3,
    closed: bool,
    faces: usize,
    edges: usize,
    vertices: usize,
}

/// `body` measured where `transform` places it. A rigid placement moves nothing but the centre
/// and the box: the body's own measurement (worked out once and shared by every placement of it,
/// [`solidcraft_doc::ModelBody::measure`]) is reused, where measuring each placed copy again took
/// three fine tessellations per copy and made Properties of a large assembly take minutes
/// (#8706). A placement that turns the body off the axes takes its box from the body's mesh
/// placed there (shared with the display). Any other placement (scaled or mirrored) is measured
/// as placed, as before.
fn measure_placed(body: &solidcraft_doc::ModelBody, transform: Mat) -> Result<Placed> {
    if proper_rotation(&transform) {
        let m = body.measure()?;
        let bbox = if square_to_axes(&transform) {
            placed_box(&transform, &m.bbox)
        } else {
            let mesh = body.mesh();
            let mut b = Aabb3::EMPTY;
            for p in &mesh.positions {
                b.add(solidcraft_doc::apply_point(&transform, *p));
            }
            if b.is_empty() { placed_box(&transform, &m.bbox) } else { b }
        };
        return Ok(Placed {
            volume: m.volume,
            area: m.area,
            centroid: solidcraft_doc::apply_point(&transform, m.centroid),
            bbox,
            closed: crate::cmd::inspect::encloses_volume(&body.body),
            faces: m.merged.faces,
            edges: m.merged.edges,
            vertices: m.merged.vertices,
        });
    }
    #[cfg(test)]
    work::count(|w| w.placed_measures += 1);
    let placed = solidcraft_kernel::transform_matrix(&body.body, transform)?;
    let m = solidcraft_kernel::measure(&placed)?;
    Ok(Placed {
        volume: m.volume,
        area: m.area,
        centroid: m.centroid,
        bbox: m.bbox,
        closed: crate::cmd::inspect::encloses_volume(&placed),
        faces: m.merged.faces,
        edges: m.merged.edges,
        vertices: m.merged.vertices,
    })
}

fn properties(s: &Session, doc: &Document, selected: &[u64], descendants: bool, about: Option<Vec3>, cmd: &str) -> Result<Value> {
    let state = s.model.state();
    let mut bodies = Vec::new();
    let (mut volume, mut area, mut mass) = (0.0, 0.0, 0.0);
    let mut moment = Vec3::ZERO;
    let mut bounds = Aabb3::EMPTY;
    let (mut faces, mut edges, mut vertices) = (0, 0, 0);
    let mut placement_count = 0;
    let mut skipped = 0;
    let selected_placements = if selected.is_empty() {
        subtree(doc, (0, Vec::new(), solidcraft_doc::IDENTITY), descendants, cmd)?
    } else {
        let placement = s.occurrence_placement(selected).map_err(|e| bad(cmd, e.to_string()))?;
        subtree(doc, (placement.component, selected.to_vec(), placement.world), descendants, cmd)?
    };
    // Each component's bodies (with their index in the model), the timeline looked up once rather
    // than for every body at every placement.
    let mut by_component: std::collections::HashMap<u64, Vec<(usize, &solidcraft_doc::ModelBody)>> = std::collections::HashMap::new();
    for (index, (body, component)) in state.bodies.iter().zip(doc.model_body_components(&state)).enumerate() {
        by_component.entry(component).or_default().push((index, body));
    }
    // How each closed body's volume spreads about its centre of mass (#8095): one tessellation
    // however often it is placed, which each placement then turns.
    let components: BTreeSet<u64> = selected_placements.iter().map(|(component, _, _)| *component).collect();
    let spread_bodies: Vec<usize> = components
        .iter()
        .filter_map(|c| by_component.get(c))
        .flatten()
        .filter(|(_, b)| crate::cmd::inspect::encloses_volume(&b.body))
        .map(|(i, _)| *i)
        .collect();
    #[cfg(test)]
    work::count(|w| w.spreads += spread_bodies.len());
    let spreads = solidcraft_kernel::map_on_threads(&spread_bodies, 16, |&i| {
        state.bodies.get(i).and_then(|b| solidcraft_io::mass_properties(&b.body).ok().flatten()).and_then(|m| m.gyration())
    });
    let spread: BTreeMap<usize, Tensor3> = spread_bodies.into_iter().zip(spreads).filter_map(|(i, g)| Some((i, g.flatten()?))).collect();
    // A window waiting for the result (the Properties window) sees how far it is and can stop it.
    let watch = solidcraft_kernel::watch::current();
    let total_bodies: usize = selected_placements.iter().map(|(c, _, _)| by_component.get(c).map_or(0, Vec::len)).sum();
    let mut done = 0;
    // Closed bodies' (mass, centre of mass, inertia about it), all in world axes.
    let mut parts: Vec<(f64, Vec3, Option<Tensor3>)> = Vec::new();
    for (component, path, transform) in selected_placements {
        placement_count += 1;
        for &(index, body) in by_component.get(&component).into_iter().flatten() {
            if let Some(w) = &watch {
                if w.cancelled() {
                    return Err(bad(cmd, "cancelled"));
                }
                w.progress("measuring", done, total_bodies);
            }
            done += 1;
            let transform: Mat = doc.model_body_offset(body).map(|offset| solidcraft_doc::mat_mul(&transform, offset)).unwrap_or(transform);
            let measured = measure_placed(body, transform)?;
            let density = doc.density(&body.name);
            let body_mass = measured.volume * density / 1000.0;
            let closed = measured.closed;
            // The mesh's spread with the exact mass, so the tensor agrees with the mass shown.
            let body_inertia = spread
                .get(&index)
                .filter(|_| closed)
                .map(|g| inertia::from_gyration(&inertia::transformed(g, &linear(&transform)), body_mass))
                .filter(|i| i.iter().flatten().all(|x| x.is_finite()));
            if closed {
                volume += measured.volume;
                mass += body_mass;
                moment += measured.centroid * body_mass;
                parts.push((body_mass, measured.centroid, body_inertia));
            } else {
                skipped += 1;
            }
            area += measured.area;
            bounds = bounds.union(&measured.bbox);
            faces += measured.faces;
            edges += measured.edges;
            vertices += measured.vertices;
            bodies.push(json!({"name": body.name, "component": component, "path": path,
                "material": doc.body_material(&body.name),
                "closed": closed, "density_g_cm3": closed.then_some(density),
                "mass_g": closed.then_some(body_mass), "volume_mm3": closed.then_some(measured.volume),
                "area_mm2": measured.area, "center_of_mass": closed.then_some(measured.centroid),
                "inertia_g_mm2": body_inertia,
                "bbox": {"min": measured.bbox.min, "max": measured.bbox.max}}));
        }
    }
    if let Some(w) = &watch {
        w.progress("measuring", done, total_bodies);
    }
    if !(volume.is_finite() && area.is_finite() && mass.is_finite() && moment.is_finite()) {
        return Err(bad(cmd, "physical properties exceed supported numeric range"));
    }
    let centroid = if mass > 0.0 { Some(moment / mass) } else { None };
    if centroid.is_some_and(|point| !point.is_finite()) {
        return Err(bad(cmd, "center of mass exceeds supported numeric range"));
    }
    // About the centre of mass: each body's own tensor, plus its mass at its distance from that
    // centre (parallel axes). Unknown when any closed body's mesh did not close.
    let at_centre = centroid.and_then(|c| {
        parts.iter().try_fold([[0.0f64; 3]; 3], |sum, (m, at, i)| {
            let i = inertia::shifted(&(*i)?, *m, *at - c);
            Some([0, 1, 2].map(|r| [0, 1, 2].map(|k| sum[r][k] + i[r][k])))
        })
    });
    let at_centre = at_centre.filter(|i| i.iter().flatten().all(|x| x.is_finite()));
    let principal = at_centre.and_then(|i| inertia::principal(&i, EQUAL_MOMENTS));
    let bbox = if bodies.is_empty() { Value::Null } else { json!({"min": bounds.min, "max": bounds.max}) };
    let mut out = json!({"path": selected, "coordinate_frame": "world", "descendants": descendants,
        "placement_count": placement_count, "body_count": bodies.len(), "bodies": bodies,
        "total": {"open_bodies_skipped": skipped, "volume_mm3": volume, "area_mm2": area, "mass_g": mass,
            "center_of_mass": centroid, "first_moment_g_mm": moment, "inertia_g_mm2": at_centre,
            "principal_moments_g_mm2": principal.map(|(moments, _)| moments), "principal_axes": principal.map(|(_, axes)| axes),
            "bbox": bbox, "faces": faces, "edges": edges, "vertices": vertices}});
    if let Some(point) = about {
        let shifted =
            centroid.zip(at_centre).map(|(c, i)| inertia::shifted(&i, mass, point - c)).filter(|i| i.iter().flatten().all(|x| x.is_finite()));
        out["total"]["inertia_about"] = json!({"point": point, "inertia_g_mm2": shifted});
    }
    Ok(out)
}

/// The tessellating work a properties command did on this thread, for tests: placed copies
/// measured afresh (transformed, then tessellated) and bodies whose inertia spread was
/// tessellated. Counted where the work is asked for, on the command's own thread.
#[cfg(test)]
pub(super) mod work {
    use std::cell::Cell;

    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct Work {
        pub placed_measures: usize,
        pub spreads: usize,
    }

    thread_local! {
        static WORK: Cell<Work> = const { Cell::new(Work { placed_measures: 0, spreads: 0 }) };
    }

    pub(super) fn count(f: impl FnOnce(&mut Work)) {
        WORK.with(|w| {
            let mut work = w.get();
            f(&mut work);
            w.set(work);
        });
    }

    /// `f`'s result and the work it did.
    pub fn capture<T>(f: impl FnOnce() -> T) -> (T, Work) {
        let before = WORK.with(Cell::get);
        let out = f();
        let after = WORK.with(Cell::get);
        (
            out,
            Work {
                placed_measures: after.placed_measures.saturating_sub(before.placed_measures),
                spreads: after.spreads.saturating_sub(before.spreads),
            },
        )
    }
}

#[cfg(test)]
#[path = "component_properties_tests.rs"]
mod tests;

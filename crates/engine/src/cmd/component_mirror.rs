//! Mirror Components (#8400): occurrences placed again as their mirror image across a plane,
//! as the same part or as a new opposite-hand part, following their source
//! (`solidcraft_doc::occurrence_mirror`).

use serde_json::Value;
use solidcraft_doc::occurrence_mirror::{self, OccurrenceMirror};
use solidcraft_doc::{BodyName, FeatureKind, PlaneRef};
use solidcraft_geom::Vec3;

use crate::params::{bad, bool_, string_list};
use crate::schema::component::{MirrorComponentsResult, MirroredOccurrence};
use crate::{EngineError, Result, Session};

const CMD: &str = "component.mirror";
const AXES: [&str; 3] = ["X", "Y", "Z"];

/// What a mirror makes of a component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MirrorAs {
    /// Another occurrence of the same component.
    Instance,
    /// A new component, the source's bodies mirrored.
    OppositeHand,
}

/// `as`, read as the handler reads it.
pub(crate) fn mirror_as_form(v: &Value) -> std::result::Result<MirrorAs, String> {
    match v.as_str().map(|s| s.trim().to_ascii_lowercase()) {
        Some(s) if s == "instance" => Ok(MirrorAs::Instance),
        Some(s) if s == "opposite_hand" || s == "opposite" => Ok(MirrorAs::OppositeHand),
        _ => Err(format!("`as` must be instance or opposite_hand, not {v}")),
    }
}

/// `symmetry`, read as the handler reads it: `None` for auto, else the part's own axis.
pub(crate) fn mirror_symmetry_form(v: &Value) -> std::result::Result<Option<usize>, String> {
    match v.as_str().map(|s| s.trim().to_ascii_uppercase()) {
        Some(s) if s == "AUTO" => Ok(None),
        Some(s) if AXES.contains(&s.as_str()) => Ok(AXES.iter().position(|a| *a == s)),
        _ => Err(format!("`symmetry` must be auto, X, Y or Z, not {v}")),
    }
}

/// `component.mirror`: the plane is in world coordinates.
pub(super) fn mirror_components(s: &mut Session, p: &Value) -> Result<Value> {
    mirror(s, p, CMD, 0)
}

/// Mirror the components `p` lists across its plane, which is in the frame of component `frame`
/// (`solid.mirror` takes it in the active component's).
pub(crate) fn mirror(s: &mut Session, p: &Value, cmd: &str, frame: u64) -> Result<Value> {
    let kind = match p.get("as").filter(|v| !v.is_null()) {
        Some(v) => mirror_as_form(v).map_err(|e| bad(cmd, e))?,
        None => MirrorAs::Instance,
    };
    let symmetry = match p.get("symmetry").filter(|v| !v.is_null()) {
        Some(v) => mirror_symmetry_form(v).map_err(|e| bad(cmd, e))?,
        None => None,
    };
    let follow = bool_(p, "follow").unwrap_or(true);
    let names = string_list(cmd, p, "components")?;
    if names.is_empty() || names.len() > 1000 {
        return Err(bad(cmd, "`components` must list 1…1000 component or occurrence names"));
    }
    let mut sources: Vec<u64> = Vec::new();
    for n in &names {
        let id = source_occurrence(s, n).ok_or_else(|| bad(cmd, format!("no component or occurrence `{n}`")))?;
        if !sources.contains(&id) {
            sources.push(id);
        }
    }
    let plane = super::super::features::plane_param(s, p.get("plane"), cmd)?;
    let frame = match &plane {
        PlaneRef::Construction { feature } => s.doc.feature(feature.0).map_or(0, |f| f.component),
        _ => frame,
    };
    let st = s.model.state();
    let world = occurrence_mirror::world_plane(&s.doc, Some(&st), &plane, frame).map_err(|e| bad(cmd, e.to_string()))?;
    let mut mirrored = Vec::new();
    for source in sources {
        let Some(src) = s.doc.occurrences.iter().find(|o| o.id == source).cloned() else { continue };
        let part = s.doc.components.iter().find(|c| c.id == src.component).map(|c| c.name.clone()).unwrap_or_default();
        let mesh = occurrence_mirror::component_mesh(&s.doc, &st, src.component);
        if mesh.triangles.is_empty() {
            return Err(bad(cmd, format!("{} has nothing to mirror", src.name)));
        }
        // The part's own axes, the one most square to the mirror plane first.
        let placed = solidcraft_doc::joints::occurrence_world(&s.doc, source);
        let normal = world.normal();
        let mut order = [0usize, 1, 2];
        let along =
            |k: usize| solidcraft_doc::apply_vector(&placed, [Vec3::X, Vec3::Y, Vec3::Z].get(k).copied().unwrap_or(Vec3::Z)).dot(normal).abs();
        order.sort_by(|a, b| along(*b).total_cmp(&along(*a)));
        let axis = match (symmetry, kind) {
            (Some(axis), _) => axis,
            (None, MirrorAs::OppositeHand) => order.first().copied().unwrap_or(0),
            (None, MirrorAs::Instance) => order
                .into_iter()
                .find(|k| occurrence_mirror::mid_plane(&mesh, *k).is_some_and(|pl| occurrence_mirror::symmetric_about(&mesh, &pl)))
                .ok_or_else(|| {
                    bad(
                        cmd,
                        format!(
                            "{part} is not symmetric about its own X, Y or Z mid-plane, so another placement of it cannot show its mirror image: \
                             use as: opposite_hand for a new opposite-hand part, or give symmetry: X, Y or Z to place it anyway"
                        ),
                    )
                })?,
        };
        let mid = occurrence_mirror::mid_plane(&mesh, axis).ok_or_else(|| bad(cmd, format!("{} has nothing to mirror", src.name)))?;
        let local = occurrence_mirror::reflection(&mid);
        let (component, feature) = match kind {
            MirrorAs::Instance => (src.component, None),
            MirrorAs::OppositeHand => opposite_hand(s, &st, src.component, src.parent, &part, mid, cmd)?,
        };
        let occurrence = match kind {
            MirrorAs::Instance => s.doc_mut().add_occurrence(component, src.parent, solidcraft_doc::IDENTITY)?,
            MirrorAs::OppositeHand => s.doc.occurrence_of(component).map(|o| o.id).ok_or_else(|| bad(cmd, "the new part has no occurrence"))?,
        };
        let relation = OccurrenceMirror { occurrence, source, plane: plane.clone(), frame, local };
        let transform = occurrence_mirror::mirrored_transform(&s.doc, Some(&st), &relation).map_err(|e| bad(cmd, e.to_string()))?;
        let doc = s.doc_mut();
        if let Some(o) = doc.occurrences.iter_mut().find(|o| o.id == occurrence) {
            o.transform = transform;
            o.grounded = false;
        }
        if follow {
            if doc.assembly.mirrors.len() >= occurrence_mirror::MAX_MIRRORS {
                return Err(bad(cmd, "the design has too many mirrored occurrences"));
            }
            doc.assembly.mirrors.push(relation);
        }
        let name = s.doc.occurrences.iter().find(|o| o.id == occurrence).map(|o| o.name.clone()).unwrap_or_default();
        mirrored.push(MirroredOccurrence {
            source,
            occurrence,
            name,
            component,
            kind: if kind == MirrorAs::Instance { "instance" } else { "opposite_hand" }.into(),
            symmetry: AXES.get(axis).copied().unwrap_or("X").into(),
            follows: follow,
            feature,
        });
    }
    s.refresh();
    if let Some(e) = mirrored.iter().filter_map(|m| m.feature).find_map(|f| s.model.result(f).and_then(|r| r.error.clone())) {
        return Err(EngineError::Other(e));
    }
    let reply = MirrorComponentsResult { occurrences: mirrored.iter().map(|m| m.occurrence).collect(), mirrored, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// The occurrence a name means: an occurrence by name or id, else a component's first one.
fn source_occurrence(s: &Session, key: &str) -> Option<u64> {
    let key = key.trim();
    if key.is_empty() || key == "0" || key.eq_ignore_ascii_case("root") {
        return None;
    }
    if let Some(o) = s.doc.occurrences.iter().find(|o| o.name == key || o.id.to_string() == key) {
        return Some(o.id);
    }
    s.doc.find_component(key).filter(|c| *c != 0).and_then(|c| s.doc.occurrence_of(c)).map(|o| o.id)
}

/// A new component `<part> (mirror)` in `parent`, holding a Mirror feature of `component`'s own
/// bodies in plane `mid` of its own frame: its feature's id with the new component's.
fn opposite_hand(
    s: &mut Session,
    st: &solidcraft_doc::ModelState,
    component: u64,
    parent: u64,
    part: &str,
    mid: solidcraft_geom::Plane,
    cmd: &str,
) -> Result<(u64, Option<u64>)> {
    if s.doc.occurrences.iter().any(|o| o.parent == component) {
        return Err(bad(cmd, format!("{part} holds other components: mirror those, or place it as an instance (as: instance)")));
    }
    let owners = s.doc.model_body_components(st);
    let bodies: Vec<BodyName> = st.bodies.iter().zip(&owners).filter(|(_, c)| **c == component).map(|(b, _)| BodyName(b.name.clone())).collect();
    if bodies.is_empty() {
        return Err(bad(cmd, format!("{part} has no bodies to mirror")));
    }
    let mut name = format!("{part} (mirror)");
    for k in 2..10_000 {
        if !s.doc.components.iter().any(|c| c.name == name) {
            break;
        }
        name = format!("{part} (mirror {k})");
    }
    let id = s.doc_mut().add_component(Some(&name), parent)?;
    let kind = FeatureKind::Mirror { features: Vec::new(), plane: PlaneRef::Custom { plane: mid }, bodies, combine: false, placements: Vec::new() };
    let doc = s.doc_mut();
    let feature = doc.add_feature(kind, None)?;
    if let Some(f) = doc.feature_mut(feature) {
        f.component = id;
    }
    doc.name_feature_inputs(feature);
    Ok((id, Some(feature)))
}

/// Place every occurrence the design keeps at a mirror image where its source and plane put it
/// now, and drop the mirrors whose occurrence or source is gone. Run after the joints are solved,
/// so a mirror follows a source a joint moved.
pub(crate) fn follow(s: &mut Session) {
    if s.doc.assembly.mirrors.is_empty() {
        return;
    }
    let st = s.model.state();
    let (moves, _, gone) = occurrence_mirror::follow(&s.doc, Some(&st));
    if moves.is_empty() && gone.is_empty() {
        return;
    }
    let doc = s.doc_mut();
    for k in gone.into_iter().rev() {
        if k < doc.assembly.mirrors.len() {
            doc.assembly.mirrors.remove(k);
        }
    }
    for (id, t) in &moves {
        if let Some(o) = doc.occurrences.iter_mut().find(|o| o.id == *id) {
            o.transform = *t;
        }
    }
    // A feature that reads where a part is placed sees the new placements.
    if !moves.is_empty() {
        s.model.evaluate(&s.doc);
    }
}

/// The source a following mirror places occurrence `id` by, as a refusal to move it on its own.
pub(crate) fn placed_by_mirror(s: &Session, id: u64, cmd: &str) -> Result<()> {
    let Some(m) = s.doc.assembly.mirrors.iter().find(|m| m.occurrence == id) else { return Ok(()) };
    let name = |id: u64| s.doc.occurrences.iter().find(|o| o.id == id).map(|o| o.name.clone()).unwrap_or_default();
    Err(bad(cmd, format!("{} is placed as the mirror image of {}: move {} instead", name(id), name(m.source), name(m.source))))
}

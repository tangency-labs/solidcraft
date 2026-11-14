//! Sketch projection: Project, Intersect and Include 3D Geometry bring model edges, faces,
//! bodies, other sketches and the origin into the active sketch as linked reference geometry
//! (fixed, drawn purple, updated when the upstream geometry changes). Break Link turns them into
//! normal geometry.

use serde_json::{Value, json};
use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_doc::project::{resolve, resolve_at};
use solidcraft_doc::{Document, FeatureKind};
use solidcraft_geom::prelude::*;
use solidcraft_sketch::{ConstraintKind, LinkKind, LinkSource, Sketch};

use super::sketch_store;
use super::{CommandSpec, in_sketch};
use crate::params::{bad, bool_, str_, vec3};
use crate::schema::sketch_project::{
    AutoProjectParams, AutoProjectResult, FitCurvesParams, FitSectionResult, Include3dParams, IntersectParams, IntersectionCurveParams,
    IsoCurveParams, LinkMade, ProjectParams, ProjectResult, ProjectToSurfaceParams, ProjectToSurfaceResult, ProjectedLink, SketchBreakLinkParams,
    SketchBreakLinkResult, SketchLinkResult, SpunProfileParams,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

macro_rules! refs_doc {
    () => {
        "refs: [{edge: [x,y,z], body?} | {face: [x,y,z], body?} (its edge loops) | {body: name} (silhouette) | {vertex: [x,y,z], body?} | {sketch, curve} | {sketch, point} | {point: name} (a construction point) | \"origin\" | {axis: X|Y|Z|name} | {plane: XY|XZ|YZ|name}]"
    };
}

const PROJECT_CONTRACT: TypedContract<ProjectParams> = TypedContract::new();
const INTERSECT_CONTRACT: TypedContract<IntersectParams> = TypedContract::new();
const INCLUDE_3D_CONTRACT: TypedContract<Include3dParams> = TypedContract::new();
const PROJECT_TO_SURFACE_CONTRACT: TypedContract<ProjectToSurfaceParams> = TypedContract::new();
const INTERSECTION_CURVE_CONTRACT: TypedContract<IntersectionCurveParams> = TypedContract::new();
const SPUN_PROFILE_CONTRACT: TypedContract<SpunProfileParams> = TypedContract::new();
const ISO_CURVE_CONTRACT: TypedContract<IsoCurveParams> = TypedContract::new();
const FIT_SECTION_CONTRACT: TypedContract<FitCurvesParams> = TypedContract::new();
const BREAK_LINK_CONTRACT: TypedContract<SketchBreakLinkParams> = TypedContract::new();
const AUTO_PROJECT_CONTRACT: TypedContract<AutoProjectParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sketch.project", "Project", crate::checked!("sketch.project", project_cmd)).at("SKETCH", "CREATE").icon("project").key("P").enabled(in_sketch).contract(&PROJECT_CONTRACT).output(output_schema_for::<ProjectResult>).params(
        concat!(refs_doc!(), "; projection_link?: bool (default true); construction?: bool (default the session's preference, off: the created curves are construction geometry)", created_doc!()),
    ),
    CommandSpec::new("sketch.intersect", "Intersect", crate::checked!("sketch.intersect", intersect_cmd)).at("SKETCH", "CREATE").icon("intersect").enabled(in_sketch).contract(&INTERSECT_CONTRACT).output(output_schema_for::<ProjectResult>).params(
        concat!("refs: [{body}, {face: [x,y,z], body?}, {edge: [x,y,z], body?}, {sketch, curve}]: section curves (or points) with the sketch plane; projection_link?: bool (default true); construction?: bool (default the session's preference, off: the created curves are construction geometry)", created_doc!()),
    ),
    CommandSpec::new("sketch.include_3d", "Include 3D Geometry", crate::checked!("sketch.include_3d", include_cmd))
        .at("SKETCH", "CREATE")
        .icon("include")
        .enabled(in_sketch)
        .contract(&INCLUDE_3D_CONTRACT).output(output_schema_for::<ProjectResult>).params(concat!("refs: like Project (edges, vertices, sketch curves and points); curves out of the sketch plane are flattened onto it; construction?: bool (default the session's preference, off: the created curves are construction geometry)", created_doc!())),
    CommandSpec::new("sketch.project_to_surface", "Project To Surface", crate::checked!("sketch.project_to_surface", project_to_surface))
        .at("SKETCH", "CREATE")
        .icon("project_surface")
        .enabled(in_sketch)
        .contract(&PROJECT_TO_SURFACE_CONTRACT).output(output_schema_for::<ProjectToSurfaceResult>).params(concat!("curves: [{sketch, curve}…] (curves of other sketches), face: [x,y,z] point on the target face, body?: projected along the active sketch's normal onto the face, as linked 3D curves", created_doc!())),
    CommandSpec::new("sketch.intersection_curve", "Intersection Curve", crate::checked!("sketch.intersection_curve", intersection_curve))
        .at("SKETCH", "CREATE")
        .icon("intersection_curve")
        .enabled(in_sketch)
        .contract(&INTERSECTION_CURVE_CONTRACT).output(output_schema_for::<SketchLinkResult>).params(concat!("a, b: {body} | {face: [x,y,z], body?} | {sketch, curve} (a curve of another sketch, standing in for its extrusion along that sketch's normal): where they meet, as linked 3D curves", created_doc!())),
    CommandSpec::new("sketch.spun_profile", "Spun Profile", crate::checked!("sketch.spun_profile", spun_profile))
        .at("SKETCH", "CREATE")
        .icon("spun")
        .enabled(in_sketch)
        .contract(&SPUN_PROFILE_CONTRACT).output(output_schema_for::<SketchLinkResult>).params(concat!("body: name, axis: a line of the active sketch (curve id) | X|Y|Z: the body's outline revolved about the axis, in the sketch plane", created_doc!())),
    CommandSpec::new("sketch.isoparametric_curve", "Isoparametric Curve", crate::checked!("sketch.isoparametric_curve", iso_curve))
        .at("SKETCH", "CREATE")
        .icon("iso_curve")
        .enabled(in_sketch)
        .contract(&ISO_CURVE_CONTRACT).output(output_schema_for::<SketchLinkResult>).params(concat!("face: [x,y,z] point on the face (the curve passes through it), body?, direction?: u|v|[x,y,z] (default u): linked 3D curve", created_doc!())),
    CommandSpec::new("sketch.fit_curves_to_section", "Fit Curves to Mesh Section", crate::checked!("sketch.fit_curves_to_section", fit_section))
        .at("SKETCH", "CREATE")
        .icon("fit_section")
        .enabled(in_sketch)
        .contract(&FIT_SECTION_CONTRACT).output(output_schema_for::<FitSectionResult>).params(concat!("body: name (a mesh or solid body): its section with the sketch plane, fitted with lines and arcs, as normal (unlinked) geometry", created_doc!())),
    CommandSpec::new("sketch.break_link", "Break Link", crate::checked!("sketch.break_link", break_link)).icon("sketch_break")
        .enabled(in_sketch)
        .contract(&BREAK_LINK_CONTRACT).output(output_schema_for::<SketchBreakLinkResult>).params("link?: link id (j1) | entities?: [curve or point ids]: linked geometry becomes normal sketch geometry"),
    CommandSpec::new("sketch.auto_project", "Auto-Project Edges on Reference", crate::checked!("sketch.auto_project", auto_project)).icon("project")
        .noundo()
        .contract(&AUTO_PROJECT_CONTRACT).output(output_schema_for::<AutoProjectResult>).params("value?: bool (default toggles): project a face's edges when a sketch is created on it"),
];

/// What a reference says in its JSON, before the design is asked what it names (`source` goes on
/// from here, and so does the contract of the commands that take references).
pub(crate) enum RefForm<'a> {
    /// Text: `origin`, an axis letter or a body name, which the design decides.
    Name(&'a str),
    /// `{edge: [x,y,z], body?}`.
    Edge { body: String, at: Vec3 },
    /// `{face: [x,y,z], body?}`.
    Face { body: String, at: Vec3 },
    /// `{vertex: [x,y,z], body?}`.
    Vertex { body: String, at: Vec3 },
    /// `{sketch, curve}` or `{sketch, point}`: the sketch as the lookup reads it (an id or a name)
    /// and the `curve` or `point` given. With neither, the handler refuses it once the sketch is
    /// found.
    Sketch { key: String, curve: Option<&'a str>, point: Option<&'a str> },
    /// `{body: name}`.
    Body(&'a str),
    /// `{point: name}`: a construction point (#12075).
    Point(&'a str),
    /// `{axis: X|Y|Z|name}`.
    Axis(&'a str),
    /// `{plane: XY|XZ|YZ|name}`.
    Plane(&'a str),
    /// `{origin: …}`.
    Origin,
}

/// The form of one reference, or the message (without the command's name) for what no design
/// could make sense of. The forms are tried in this order, the first that fits wins: an edge, face
/// or vertex whose value is not a point is passed over, not refused.
pub(crate) fn ref_form(v: &Value) -> std::result::Result<RefForm<'_>, String> {
    if let Some(name) = v.as_str() {
        return Ok(RefForm::Name(name));
    }
    let body = || str_(v, "body").unwrap_or("").to_string();
    if let Some(at) = v.get("edge").and_then(vec3) {
        return Ok(RefForm::Edge { body: body(), at });
    }
    if let Some(at) = v.get("face").and_then(vec3) {
        return Ok(RefForm::Face { body: body(), at });
    }
    if let Some(at) = v.get("vertex").and_then(vec3) {
        return Ok(RefForm::Vertex { body: body(), at });
    }
    if let Some(sk) = v.get("sketch") {
        let key = match sk {
            Value::Number(n) => n.to_string(),
            Value::String(x) => x.clone(),
            _ => return Err("`sketch` must be an id or a name".into()),
        };
        return Ok(RefForm::Sketch { key, curve: str_(v, "curve"), point: str_(v, "point") });
    }
    if let Some(p) = str_(v, "point") {
        return Ok(RefForm::Point(p));
    }
    if let Some(b) = str_(v, "body") {
        return Ok(RefForm::Body(b));
    }
    if let Some(a) = str_(v, "axis") {
        return Ok(RefForm::Axis(a));
    }
    if let Some(p) = str_(v, "plane") {
        return Ok(RefForm::Plane(p));
    }
    if v.get("origin").is_some() {
        return Ok(RefForm::Origin);
    }
    Err(format!("cannot project {v}"))
}

/// Parse one reference.
fn source(s: &Session, v: &Value, cmd: &str) -> Result<LinkSource> {
    match ref_form(v).map_err(|why| bad(cmd, why))? {
        RefForm::Name(name) => match name.to_ascii_lowercase().as_str() {
            "origin" => Ok(LinkSource::Origin),
            "x" | "y" | "z" => Ok(LinkSource::Axis { name: name.to_ascii_uppercase() }),
            _ if s.model.state().body(name).is_some() => Ok(LinkSource::Body { body: name.to_string() }),
            _ => Err(bad(cmd, format!("unknown reference `{name}`"))),
        },
        RefForm::Edge { body, at } => Ok(LinkSource::Edge { body, at }),
        RefForm::Face { body, at } => Ok(LinkSource::Face { body, at }),
        RefForm::Vertex { body, at } => Ok(LinkSource::Vertex { body, at }),
        RefForm::Sketch { key, curve, point } => {
            let f = s.doc.find_feature(&key).ok_or_else(|| bad(cmd, format!("no sketch `{key}`")))?;
            if !matches!(f.kind, FeatureKind::Sketch { .. }) {
                return Err(bad(cmd, format!("`{key}` is not a sketch")));
            }
            if Some(f.id) == s.active_sketch {
                return Err(bad(cmd, "a sketch cannot project its own geometry"));
            }
            if let Some(c) = curve {
                return Ok(LinkSource::SketchCurve { sketch: f.id, curve: c.to_string() });
            }
            if let Some(p) = point {
                return Ok(LinkSource::SketchPoint { sketch: f.id, point: p.to_string() });
            }
            Err(bad(cmd, "a sketch reference needs `curve` or `point`"))
        }
        RefForm::Body(b) => Ok(LinkSource::Body { body: b.to_string() }),
        RefForm::Point(p) => point_source(s, p, cmd),
        RefForm::Axis(a) => axis_source(s, a, cmd),
        RefForm::Plane(p) => Ok(LinkSource::Plane { name: p.to_string() }),
        RefForm::Origin => Ok(LinkSource::Origin),
    }
}

/// A construction point by its name (#12075).
fn point_source(s: &Session, name: &str, cmd: &str) -> Result<LinkSource> {
    match s.doc.find_feature(name) {
        Some(f) if matches!(f.kind, FeatureKind::ConstructionPoint { .. }) => Ok(LinkSource::ConstructionPoint { name: f.name.clone() }),
        _ => Err(bad(cmd, format!("unknown point `{name}`: the name of a construction point"))),
    }
}

/// An origin axis (`X`, `Y` or `Z`, in any case), a construction axis by its name (#9324), or the
/// origin axis of a component by its Browser key (`o:2/origin/Z`, #15086).
fn axis_source(s: &Session, name: &str, cmd: &str) -> Result<LinkSource> {
    if ["X", "Y", "Z"].iter().any(|axis| name.eq_ignore_ascii_case(axis)) {
        return Ok(LinkSource::Axis { name: name.to_ascii_uppercase() });
    }
    if s.origin_axis(name).is_some() {
        return Ok(LinkSource::Axis { name: name.to_string() });
    }
    match s.doc.find_feature(name) {
        Some(f) if matches!(f.kind, FeatureKind::ConstructionAxis { .. }) => Ok(LinkSource::Axis { name: f.name.clone() }),
        _ => Err(bad(cmd, format!("unknown axis `{name}`: X, Y, Z or the name of a construction axis"))),
    }
}

/// The plane of a sketch feature, resolved against `st`: the model before the sketch as its
/// component sees it, which is what evaluation resolves it against. A construction plane built
/// from the model (through a sketch line, three points, between two faces…) exists only there.
fn sketch_plane(doc: &Document, id: u64, st: &solidcraft_doc::ModelState) -> Result<Plane> {
    let (vals, _) = doc.param_values();
    match doc.feature(id).map(|f| &f.kind) {
        Some(FeatureKind::Sketch { plane, .. }) => {
            let component = doc.feature(id).map(|f| f.component).unwrap_or(0);
            Ok(doc.resolve_plane_in(&vals, plane, 0, component, Some(st))?)
        }
        _ => Err(EngineError::Other(format!("feature {id} is not a sketch"))),
    }
}

/// The model before sketch `id` in the frame its links and its plane are resolved in: the
/// attached face's frame for a sketch on a face, else its component's. The further placements
/// `named` names (`Block (Part:2)`) are in it too, where they are (#9957).
fn state_for_sketch<'a>(
    s: &Session,
    doc: &Document,
    id: u64,
    st: &'a solidcraft_doc::ModelState,
    named: &[&str],
) -> std::result::Result<std::borrow::Cow<'a, solidcraft_doc::ModelState>, solidcraft_doc::DocError> {
    let comp = doc.feature(id).map(|f| f.component).unwrap_or(0);
    let attachment = doc.feature(id).and_then(|feature| match &feature.kind {
        solidcraft_doc::FeatureKind::Sketch { plane, .. } => super::sketch::face::anchor(plane),
        _ => None,
    });
    let (framed, frame) = match attachment {
        Some(anchor) => (solidcraft_doc::state_in_face_anchor_frame(doc, st, anchor, comp)?, anchor.author_frame(doc, comp)?),
        None => {
            let frame = owner_frame(s, doc, comp);
            (solidcraft_doc::state_in_frame_at(doc, st, comp, frame)?, frame)
        }
    };
    Ok(solidcraft_doc::with_placed_copies(doc, st, framed, &frame, named))
}

/// The placed bodies of a design (`Document::body_frames_with_source`): world name, definition
/// name, component, occurrence path and frame.
pub(super) type PlacedBodies = Vec<(String, String, u64, Vec<u64>, solidcraft_doc::Mat)>;

/// Sketch links keep a further placement's body by the name the world gives it (`Block (Part:2)`,
/// #9957), which a rename of the body or of its component changes: each link naming one of
/// `before` (the placed bodies before the rename) now names what that body is called in `doc`.
/// `renamed` is the body renamed (old name, new name), if one was.
pub(super) fn follow_placed_renames(doc: &mut Document, st: &solidcraft_doc::ModelState, before: &PlacedBodies, renamed: Option<(&str, &str)>) {
    let renames: std::collections::HashMap<String, String> = {
        let names = solidcraft_doc::WorldNames::new(doc, st);
        before
            .iter()
            .filter(|(world, own, ..)| world != own)
            .filter_map(|(world, own, component, path, _)| {
                let source = match renamed {
                    Some((old, new)) if own == old => new,
                    _ => own.as_str(),
                };
                let now = names.name(source, *component, path);
                (now != *world).then(|| (world.clone(), now))
            })
            .collect()
    };
    if renames.is_empty() {
        return;
    }
    for f in &mut doc.features {
        if let FeatureKind::Sketch { sketch, .. } = &mut f.kind {
            for link in &mut sketch.links {
                rename_link_body(&mut link.source, &renames);
            }
        }
    }
}

fn rename_link_body(source: &mut LinkSource, renames: &std::collections::HashMap<String, String>) {
    match source {
        LinkSource::Edge { body, .. }
        | LinkSource::Face { body, .. }
        | LinkSource::Body { body }
        | LinkSource::Vertex { body, .. }
        | LinkSource::OnSurface { body, .. }
        | LinkSource::Iso { body, .. }
        | LinkSource::Spun { body, .. } => {
            if let Some(new) = renames.get(body.as_str()) {
                body.clone_from(new);
            }
        }
        LinkSource::Intersection { a, b } => {
            rename_link_body(a, renames);
            rename_link_body(b, renames);
        }
        _ => {}
    }
}

/// Keep candidate feature edits while overlaying uncommitted occurrence placements.
fn placed_candidate<'a>(s: &Session, doc: &'a Document) -> std::borrow::Cow<'a, Document> {
    if s.pending_moves.is_empty() {
        return std::borrow::Cow::Borrowed(doc);
    }
    let mut placed = doc.clone();
    for o in &mut placed.occurrences {
        if let Some(m) = s.pending_moves.get(&o.id) {
            o.transform = *m;
        }
    }
    std::borrow::Cow::Owned(placed)
}

fn owner_frame(s: &Session, doc: &Document, comp: u64) -> solidcraft_doc::Mat {
    if comp == s.active_component { s.active_component_frame() } else { doc.component_transform(comp) }
}

/// The author component and its placement used by [`state_for_sketch`], including a face
/// attachment's explicit author path. `None` when that placement is gone.
fn sketch_frame(s: &Session, doc: &Document, id: u64) -> Option<(u64, solidcraft_doc::Mat)> {
    let comp = doc.feature(id).map(|f| f.component).unwrap_or(0);
    let attachment = doc.feature(id).and_then(|feature| match &feature.kind {
        FeatureKind::Sketch { plane, .. } => super::sketch::face::anchor(plane),
        _ => None,
    });
    match attachment {
        Some(anchor) => anchor.author_frame(doc, comp).ok().map(|frame| (comp, frame)),
        None => Some((comp, owner_frame(s, doc, comp))),
    }
}

/// Re-resolve the links of a sketch being edited (`sk`, a copy of the stored sketch `id`)
/// against the model before it, so edits see current reference geometry. Returns warnings.
///
/// When a link moved, the rest of the sketch follows it as the evaluation solves it, keeping
/// the stored sides and turns, so the edit goes on from the sketch the model shows. Left where
/// it was stored beside a line that has moved past it, a dimensioned point would give the
/// edit's solve the other side to keep. A sketch that does not solve that way keeps its stored
/// geometry, with the links moved.
pub(super) fn refresh(s: &Session, doc: &Document, id: u64, sk: &mut Sketch) -> Vec<String> {
    if sk.links.is_empty() {
        return Vec::new();
    }
    #[cfg(test)]
    refresh_count::record();
    let doc = placed_candidate(s, doc);
    let (vals, _) = doc.param_values();
    // A text's outlines are rebuilt from its numbers: the ones its expressions give come first
    // (an expression that cannot be evaluated leaves the text as it was).
    let _ = doc.apply_text_values(&vals, sk);
    let st = s.model.state_before(id);
    let attachment = doc.feature(id).and_then(|feature| match &feature.kind {
        solidcraft_doc::FeatureKind::Sketch { plane, .. } => super::sketch::face::anchor(plane),
        _ => None,
    });
    let named: Vec<&str> = sk.links.iter().flat_map(|l| l.source.bodies()).collect();
    let Ok(st) = state_for_sketch(s, &doc, id, &st, &named) else {
        return vec!["lost face attachment; reattach explicitly".into()];
    };
    let Ok(plane) = sketch_plane(&doc, id, &st) else { return Vec::new() };
    let frame = sketch_frame(s, &doc, id);
    let refreshed = solidcraft_doc::project::refresh_links_at(&doc, &vals, &st, &plane, sk, attachment, frame.as_ref());
    // A link whose named face or edge is gone fails the sketch when it is evaluated; while it is
    // edited, that is what the refresh says.
    let warnings = match refreshed {
        Ok(warnings) => warnings,
        Err(e) => return vec![e.in_feature(&doc.feature(id).map(|f| f.name.clone()).unwrap_or_default()).to_string()],
    };
    if let Ok(stored) = sketch_store::read(&doc, id)
        && sk.positions_in(stored).is_some()
    {
        let mut solved = sk.clone();
        let follows = solved.refresh_offsets().is_ok()
            && doc.solve_sketch(&vals, &mut solved, &stored.driving_values(), Some(stored)).is_ok_and(|report| report.keeps_orientation());
        if follows {
            *sk = solved;
        }
    }
    warnings
}

/// Refreshes of a sketch's links run on this thread by [`refresh`].
#[cfg(test)]
pub(super) mod refresh_count {
    use std::cell::Cell;

    thread_local! {
        static COUNT: Cell<usize> = const { Cell::new(0) };
    }

    pub fn record() {
        COUNT.with(|c| c.set(c.get() + 1));
    }

    /// The count since the last call.
    pub fn take() -> usize {
        COUNT.with(|c| c.replace(0))
    }
}

/// Resolve an origin axis as a mirror line: the axis's projection into the sketch plane, kept
/// linked and fixed. An axis perpendicular to the plane has no line in it and is refused.
pub(super) fn origin_axis_line(s: &Session, p: &Value, line: &Value, cmd: &str) -> Result<(LinkSource, Vec<solidcraft_sketch::LinkGeom>)> {
    let id = sketch_store::target(s, p, cmd)?;
    let doc = placed_candidate(s, &s.doc);
    let st = s.model.state_before(id);
    let st = state_for_sketch(s, &doc, id, &st, &[])?;
    let plane = sketch_plane(&doc, id, &st)?;
    let src = source(s, line, cmd)?;
    let LinkSource::Axis { name } = &src else { return Err(bad(cmd, "the mirror line must be a straight sketch curve or an origin axis")) };
    let dir = match name.as_str() {
        "X" => Vec3::X,
        "Y" => Vec3::Y,
        "Z" => Vec3::Z,
        _ => return Err(bad(cmd, "the origin axis must be X, Y or Z")),
    };
    // The resolve below projects the axis into the plane: its direction there is the plane's
    // share of `dir`, and empty means a point, not a mirror line.
    if Vec2::new(dir.dot(plane.x), dir.dot(plane.y)).len() <= 1e-9 {
        return Err(bad(cmd, "the origin axis is perpendicular to the sketch plane, so it has no line in it"));
    }
    let (vals, _) = doc.param_values();
    let r = resolve(&doc, &vals, &st, &plane, LinkKind::Project, &src)?;
    Ok((r.source, r.geom))
}

/// Add a link for `src` to sketch `id` in `doc` (not stored in the session). Returns the link id.
pub(super) fn add_link(s: &Session, doc: &Document, id: u64, sk: &mut Sketch, kind: LinkKind, src: LinkSource) -> Result<String> {
    add_link_unless_present(s, doc, id, sk, kind, src, false).map(|l| l.unwrap_or_default())
}

/// [`add_link`], or `None` (nothing added) when `skip_present` and everything `src` resolves to
/// is linked into the sketch already.
fn add_link_unless_present(
    s: &Session,
    doc: &Document,
    id: u64,
    sk: &mut Sketch,
    kind: LinkKind,
    src: LinkSource,
    skip_present: bool,
) -> Result<Option<String>> {
    let doc = placed_candidate(s, doc);
    let (vals, _) = doc.param_values();
    let st = s.model.state_before(id);
    // The model as the sketch's component sees it.
    let st = state_for_sketch(s, &doc, id, &st, &src.bodies())?;
    let plane = sketch_plane(&doc, id, &st)?;
    let frame = sketch_frame(s, &doc, id);
    let r = resolve_at(&doc, &vals, &st, &plane, kind, &src, frame.as_ref())?;
    let present = match kind {
        LinkKind::Intersect => sk.link_geometry_or_points_present(&r.geom),
        _ => sk.link_geometry_present(&r.geom),
    };
    if skip_present && r.wires.is_empty() && present {
        return Ok(None);
    }
    // A mesh body is one face whose name says nothing of the flat patch picked: the link keeps
    // the point instead (#13464), as an edge of a mesh body keeps no name.
    let name = if let LinkSource::Face { body, at } = &r.source {
        st.body(body).filter(|body| !body.body.is_mesh()).and_then(|body| {
            if let Some(index) = r.face_index {
                solidcraft_doc::naming::face_names(body).get(index).filter(|name| !name.is_empty()).cloned()
            } else {
                solidcraft_doc::naming::face_name_at_body(body, *at)
            }
        })
    } else {
        None
    };
    // The link is the edge or vertex that was picked, not whichever is nearest the picked point later.
    let edge_name = solidcraft_doc::project::resolved_edge_name(&st, &r);
    let vertex_name = solidcraft_doc::project::resolved_vertex_name(&st, &r);
    let link = sk.add_link_with_wires(kind, r.source, &r.geom, &r.wires)?;
    // The wires' exact curves, as a refresh of the link gives them (an included edge's record
    // curve): a link made now is the one its sketch would resolve.
    sk.set_link_wire_curves(&link, &r.exact)?;
    if let Some(l) = sk.links.iter_mut().find(|l| l.id == link) {
        l.face_name = name;
        l.edge_name = edge_name;
        l.vertex_name = vertex_name;
    }
    Ok(Some(link))
}

/// Make the curves of `link` construction geometry: references to draw against, left out of
/// profiles (#13142). They stay linked; a link rebuilt because its shape changed comes back as
/// normal geometry, as curves switched to construction by hand do.
fn mark_construction(sk: &mut Sketch, link: &str) {
    for c in sk.link_curves(link) {
        if let Some(curve) = sk.curves.get_mut(c) {
            curve.construction = true;
        }
    }
}

/// [`mark_construction`] for `link` in sketch `id`, as the session's preference asks of the face
/// edges a new sketch projects (#13142).
pub(super) fn construct_link(s: &mut Session, id: u64, link: &str) -> Result<()> {
    sketch_store::update(s, id, |_, _, sk| {
        mark_construction(sk, link);
        Ok(())
    })
}

/// A command's reply as JSON: they build their replies from the types their output schemas
/// describe, so the two can't drift apart.
fn reply(cmd: &str, reply: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(reply).map_err(|e| bad(cmd, e.to_string()))
}

/// The ids of the curves and of the points that link `link` made.
fn link_parts(sk: &Sketch, link: &str) -> (Vec<String>, Vec<String>) {
    let curves: Vec<String> = sk.link_curves(link).iter().filter_map(|c| sk.curves.get(*c).map(|c| c.id.clone())).collect();
    let points: Vec<String> = sk.link_points(link).iter().filter_map(|p| sk.points.get(*p).map(|p| p.id.clone())).collect();
    (curves, points)
}

/// Run a projection command (Project, Intersect, or Include 3D Geometry) on the given
/// references, adding a link for each to the active sketch. Returns the list of
/// projected links and any references that were already projected.
fn run_links(s: &mut Session, p: &Value, cmd: &str, kind: LinkKind, projection_link: bool) -> Result<Value> {
    let refs: Vec<Value> = match p.get("refs").or_else(|| p.get("ref")) {
        Some(Value::Array(a)) => a.iter().take(1000).cloned().collect(),
        Some(v) => vec![v.clone()],
        None => return Err(bad(cmd, "`refs` must list what to project")),
    };
    if refs.is_empty() {
        return Err(bad(cmd, "`refs` is empty"));
    }
    let construction = match p.get("construction") {
        None => s.project_construction,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err(bad(cmd, "`construction` must be a boolean")),
    };
    let id = sketch_store::active(s).ok_or_else(|| EngineError::Other("no sketch is being edited".into()))?;
    let sources: Vec<LinkSource> = refs.iter().map(|v| source(s, v, cmd)).collect::<Result<_>>()?;
    // #14190: projecting a whole body produces its silhouette — many curves from the whole
    // outline. Off by default: refuse a body reference for Project unless the preference
    // `project_body_silhouette` is on. Project a single edge or face instead.
    if kind == LinkKind::Project && !s.project_body_silhouette {
        for src in &sources {
            if matches!(src, LinkSource::Body { .. }) {
                return Err(bad(
                    cmd,
                    "projecting a whole body produces its silhouette: many curves from the whole outline. Pick a single edge or face instead, or enable the 'project body silhouette' preference.",
                ));
            }
        }
    }
    let (out, present) = sketch_store::update(s, id, |s, doc, sk| {
        refresh(s, doc, id, sk);
        let mut out = Vec::new();
        // Projecting what the sketch already shows (the border of the face it is on, an edge
        // projected before), or intersecting what was intersected before, would lay a second copy
        // over the first: those references are skipped.
        let mut present = Vec::new();
        for src in sources {
            let what = src.describe();
            match add_link_unless_present(s, doc, id, sk, kind, src, matches!(kind, LinkKind::Project | LinkKind::Intersect)) {
                Ok(Some(l)) => {
                    let (curves, points) = link_parts(sk, &l);
                    if construction {
                        mark_construction(sk, &l);
                    }
                    let mut result = ProjectedLink { link: Some(l.clone()), curves, points };
                    if !projection_link {
                        result.link = None;
                        sk.break_link(&l)?;
                    }
                    out.push(result);
                }
                Ok(None) => present.push(what),
                Err(e) => return Err(bad(cmd, format!("{what}: {e}"))),
            }
        }
        // Nothing new: the sketch is kept as it was.
        if out.is_empty() {
            let verb = if kind == LinkKind::Intersect { "intersected" } else { "projected" };
            return Err(bad(cmd, format!("already {verb} into this sketch: {}", present.join(", "))));
        }
        Ok((out, present))
    })?;
    reply(cmd, ProjectResult { links: out, already_projected: present, created: None, param_warnings: Vec::new() })
}

fn project_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let projection_link = match p.get("projection_link") {
        None => true,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err(bad("sketch.project", "`projection_link` must be a boolean")),
    };
    run_links(s, p, "sketch.project", LinkKind::Project, projection_link)
}

fn intersect_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let projection_link = match p.get("projection_link") {
        None => true,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err(bad("sketch.intersect", "`projection_link` must be a boolean")),
    };
    run_links(s, p, "sketch.intersect", LinkKind::Intersect, projection_link)
}

fn include_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    run_links(s, p, "sketch.include_3d", LinkKind::Include, true)
}

/// A constraint's operands may be model geometry (#6348): an `{edge: [x,y,z], body?}` or
/// `{vertex: [x,y,z], body?}` under one of `keys` is projected into the sketch first, as
/// Project does, and stands for the projected curve (or point) in the parameters returned.
/// Any body's edges work, a mirrored or patterned copy's too; the link keeps the edge's
/// persistent name, so the constraint follows the edge through rebuilds.
pub(super) fn project_operands(s: &mut Session, p: &Value, cmd: &str, keys: &[&str]) -> Result<Value> {
    if !keys.iter().any(|k| p.get(*k).is_some_and(model_ref)) {
        return Ok(p.clone());
    }
    let id = sketch_store::target(s, p, cmd)?;
    let mut doc = (*s.doc).clone();
    let mut sk = sketch_store::read(&doc, id)?.clone();
    refresh(s, &doc, id, &mut sk);
    let mut out = p.clone();
    for key in keys {
        let Some(v) = p.get(*key).filter(|v| model_ref(v)) else { continue };
        out[*key] = json!(project_ref(s, &doc, id, &mut sk, v, cmd)?);
    }
    sketch_store::write(&mut doc, id, sk)?;
    *s.doc_mut() = doc;
    Ok(out)
}

/// A model's reference among a command's operands (#6348): an `{edge: [x,y,z], body?}` or a
/// `{vertex: [x,y,z], body?}`, or an origin axis `{axis: X|Y|Z}`.
pub(crate) fn model_ref(v: &Value) -> bool {
    // Either case, as `source` takes it.
    v.get("edge").is_some() || v.get("vertex").is_some() || v.get("axis").and_then(Value::as_str).is_some_and(|a| matches!(a, "X" | "Y" | "Z"))
}

/// Project one model reference into the sketch and report the entity it stands for: an edge's
/// projected curve (a point when it is seen end on) or a vertex's projected point.
fn project_ref(s: &mut Session, doc: &Document, id: u64, sk: &mut Sketch, v: &Value, cmd: &str) -> Result<String> {
    let src = source(s, v, cmd)?;
    let what = src.describe();
    let link = add_link(s, doc, id, sk, LinkKind::Project, src).map_err(|e| bad(cmd, format!("{what}: {e}")))?;
    // An edge or axis stands for its curve (a point when it is seen end on), a vertex for its point.
    let curve = if v.get("edge").is_some() || v.get("axis").is_some() {
        sk.link_curves(&link).first().and_then(|c| sk.curves.get(*c)).map(|c| c.id.clone())
    } else {
        None
    };
    curve
        .or_else(|| sk.link_points(&link).first().and_then(|q| sk.points.get(*q)).map(|q| q.id.clone()))
        .ok_or_else(|| bad(cmd, format!("{what} projects to nothing on this sketch")))
}

/// Most model references one call may project into a sketch: each one is a link and a resolve,
/// so the count is capped before any of them is projected.
pub(crate) const MAX_PROJECTED_ENTITIES: usize = 1_000;

/// A dimension's entities may be model geometry (#7912): an `{edge: [x,y,z], body?}` or
/// `{vertex: [x,y,z], body?}` among them is projected into the sketch first, as Project does,
/// and stands for the projected curve (or point) in the entities returned.
pub(super) fn project_entities(s: &mut Session, p: &Value, cmd: &str) -> Result<Value> {
    let Some(Value::Array(entities)) = p.get("entities") else { return Ok(p.clone()) };
    // A long list is refused before anything is projected, so it costs nothing.
    if entities.len() > MAX_PROJECTED_ENTITIES {
        return Err(bad(cmd, format!("`entities` may list at most {MAX_PROJECTED_ENTITIES} entities")));
    }
    if !entities.iter().any(model_ref) {
        return Ok(p.clone());
    }
    let id = sketch_store::target(s, p, cmd)?;
    let mut doc = (*s.doc).clone();
    let mut sk = sketch_store::read(&doc, id)?.clone();
    refresh(s, &doc, id, &mut sk);
    let mut out = entities.clone();
    for entity in out.iter_mut().take(MAX_PROJECTED_ENTITIES) {
        if !model_ref(entity) {
            continue;
        }
        *entity = json!(project_ref(s, &doc, id, &mut sk, entity, cmd)?);
    }
    let mut params = p.clone();
    if let Some(o) = params.as_object_mut() {
        o.insert("entities".into(), Value::Array(out));
    }
    sketch_store::write(&mut doc, id, sk)?;
    *s.doc_mut() = doc;
    Ok(params)
}

/// Add one link from `src` to the active sketch and report it.
fn one_link(s: &mut Session, cmd: &str, kind: LinkKind, src: LinkSource) -> Result<LinkMade> {
    let id = sketch_store::active(s).ok_or_else(|| EngineError::Other("no sketch is being edited".into()))?;
    sketch_store::update(s, id, |s, doc, sk| {
        refresh(s, doc, id, sk);
        let what = src.describe();
        let l = add_link(s, doc, id, sk, kind, src).map_err(|e| bad(cmd, format!("{what}: {e}")))?;
        let (curves, points) = link_parts(sk, &l);
        let wires: Vec<String> = sk.link_wires(&l).iter().filter_map(|w| sk.wires.get(*w).map(|w| w.id.clone())).collect();
        Ok(LinkMade { link: l, curves, points, wires })
    })
}

fn project_to_surface(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.project_to_surface";
    let at = p.get("face").and_then(vec3).ok_or_else(|| bad(cmd, "`face` must be a point [x,y,z] on the target face"))?;
    let body = str_(p, "body").unwrap_or("").to_string();
    let list = p.get("curves").and_then(Value::as_array).cloned().ok_or_else(|| bad(cmd, "`curves` must list {sketch, curve}"))?;
    if list.is_empty() || list.len() > 500 {
        return Err(bad(cmd, "`curves` must list 1…500 curves"));
    }
    let mut out = Vec::new();
    for v in list {
        let LinkSource::SketchCurve { sketch, curve } = source(s, &v, cmd)? else {
            return Err(bad(cmd, "each entry must be {sketch, curve}"));
        };
        out.push(one_link(s, cmd, LinkKind::Project, LinkSource::OnSurface { sketch, curve, body: body.clone(), at })?);
    }
    reply(cmd, ProjectToSurfaceResult { links: out, created: None, param_warnings: Vec::new() })
}

fn intersection_curve(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.intersection_curve";
    let side = |k: &str| -> Result<LinkSource> {
        let v = p.get(k).ok_or_else(|| bad(cmd, format!("missing `{k}`")))?;
        match source(s, v, cmd)? {
            x @ (LinkSource::Body { .. } | LinkSource::Face { .. } | LinkSource::SketchCurve { .. }) => Ok(x),
            _ => Err(bad(cmd, format!("`{k}` must be a body, a face or a curve of another sketch"))),
        }
    };
    let (a, b) = (side("a")?, side("b")?);
    let made = one_link(s, cmd, LinkKind::Include, LinkSource::Intersection { a: Box::new(a), b: Box::new(b) })?;
    reply(cmd, SketchLinkResult { made, created: None, param_warnings: Vec::new() })
}

fn spun_profile(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.spun_profile";
    let body = str_(p, "body").ok_or_else(|| bad(cmd, "`body` must name a body"))?.to_string();
    let axis = str_(p, "axis").ok_or_else(|| bad(cmd, "`axis` must be a sketch line or X|Y|Z"))?;
    let (origin, dir) = match axis.to_ascii_uppercase().as_str() {
        "X" => (Vec3::ZERO, Vec3::X),
        "Y" => (Vec3::ZERO, Vec3::Y),
        "Z" => (Vec3::ZERO, Vec3::Z),
        _ => {
            let id = sketch_store::active(s).ok_or_else(|| EngineError::Other("no sketch is being edited".into()))?;
            let st = s.model.state();
            let ss = st.sketch(id).ok_or_else(|| EngineError::Other("the sketch is not evaluated".into()))?;
            let ci = ss.sketch.curve_index(axis).ok_or_else(|| bad(cmd, format!("unknown curve `{axis}`")))?;
            let Some(solidcraft_sketch::Shape::Line { a, b }) = ss.sketch.shape(ci) else { return Err(bad(cmd, "the axis must be a line")) };
            let (wa, wb) = (ss.plane.to_world(a), ss.plane.to_world(b));
            (wa, wb - wa)
        }
    };
    let made = one_link(s, cmd, LinkKind::Project, LinkSource::Spun { body, origin, dir })?;
    reply(cmd, SketchLinkResult { made, created: None, param_warnings: Vec::new() })
}

fn iso_curve(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.isoparametric_curve";
    let at = p.get("face").and_then(vec3).ok_or_else(|| bad(cmd, "`face` must be a point [x,y,z] on the face"))?;
    let body = str_(p, "body").unwrap_or("").to_string();
    let dir = match p.get("direction") {
        None => "u".to_string(),
        Some(Value::String(d)) if d.eq_ignore_ascii_case("u") || d.eq_ignore_ascii_case("v") => d.to_ascii_lowercase(),
        Some(v) => {
            let d = vec3(v).ok_or_else(|| bad(cmd, "`direction` must be u, v or [x,y,z]"))?;
            format!("{},{},{}", d.x, d.y, d.z)
        }
    };
    let made = one_link(s, cmd, LinkKind::Include, LinkSource::Iso { body, at, dir })?;
    reply(cmd, SketchLinkResult { made, created: None, param_warnings: Vec::new() })
}

fn fit_section(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.fit_curves_to_section";
    let body = str_(p, "body").ok_or_else(|| bad(cmd, "`body` must name a body"))?.to_string();
    if s.model.state().body(&body).is_none() {
        return Err(bad(cmd, format!("no body `{body}`")));
    }
    let id = sketch_store::active(s).ok_or_else(|| EngineError::Other("no sketch is being edited".into()))?;
    let curves = sketch_store::update(s, id, |s, doc, sk| {
        let l = add_link(s, doc, id, sk, LinkKind::Intersect, LinkSource::Body { body }).map_err(|e| bad(cmd, e.to_string()))?;
        let curves: Vec<String> = sk.link_curves(&l).iter().filter_map(|c| sk.curves.get(*c).map(|c| c.id.clone())).collect();
        sk.break_link(&l)?;
        Ok(curves)
    })?;
    reply(cmd, FitSectionResult { curves, created: None, param_warnings: Vec::new() })
}

fn break_link(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.break_link";
    let id = sketch_store::active(s).ok_or_else(|| EngineError::Other("no sketch is being edited".into()))?;
    let links = sketch_store::update(s, id, |_, _, sk| {
        let mut links: Vec<String> = Vec::new();
        if let Some(l) = str_(p, "link") {
            if sk.link(l).is_none() {
                return Err(bad(cmd, format!("no link `{l}`")));
            }
            links.push(l.to_string());
        }
        for e in crate::params::string_list(cmd, p, "entities")? {
            let l = sk
                .curves
                .iter()
                .find(|c| c.id == e)
                .and_then(|c| c.link.clone())
                .or_else(|| sk.points.iter().find(|q| q.id == e).and_then(|q| q.link.clone()))
                .ok_or_else(|| bad(cmd, format!("`{e}` is not linked geometry")))?;
            if !links.contains(&l) {
                links.push(l);
            }
        }
        if links.is_empty() {
            return Err(bad(cmd, "give `link` or `entities`"));
        }
        for l in &links {
            sk.break_link(l)?;
        }
        Ok(links)
    })?;
    reply(cmd, SketchBreakLinkResult { broken: links, created: None, param_warnings: Vec::new() })
}

fn auto_project(s: &mut Session, p: &Value) -> Result<Value> {
    s.auto_project = bool_(p, "value").unwrap_or(!s.auto_project);
    reply("sketch.auto_project", AutoProjectResult { auto_project: s.auto_project, created: None, param_warnings: Vec::new() })
}

/// `"x:<a>+<b>:x,y"` from interactive snapping: two distinct curve ids and a hint point.
fn curve_intersection_arg(v: &Value, cmd: &str) -> Result<Option<(String, String, Vec2)>> {
    let Some(raw) = v.as_str() else { return Ok(None) };
    let Some(t) = raw.strip_prefix("x:") else { return Ok(None) };
    if t.len() > 256 {
        return Err(bad(cmd, "`x:` snap is too long"));
    }
    let Some((ids, xy)) = t.rsplit_once(':') else {
        return Err(bad(cmd, "`x:` needs two curve ids and a point"));
    };
    if ids.bytes().filter(|b| *b == b'+').count() != 1 {
        return Err(bad(cmd, "`x:` needs two distinct curve ids"));
    }
    let Some((a, b)) = ids.split_once('+') else {
        return Err(bad(cmd, "`x:` needs two distinct curve ids"));
    };
    if a.is_empty() || b.is_empty() || a == b || a.len() > 128 || b.len() > 128 {
        return Err(bad(cmd, "`x:` needs two distinct curve ids"));
    }
    let n: Vec<f64> = xy.split(',').map(str::trim).filter(|s| !s.is_empty()).filter_map(|x| x.parse().ok()).collect();
    match n.as_slice() {
        [x, y] if x.is_finite() && y.is_finite() => Ok(Some((a.to_string(), b.to_string(), Vec2::new(*x, *y)))),
        _ => Err(bad(cmd, "`x:` point must be two finite numbers")),
    }
}

/// Point arguments that snap to model geometry while drawing, rewritten to sketch point ids:
/// `{"vertex": [x,y,z]}` projects the vertex (a linked point); `{"on_edge": [x,y,z]}` projects
/// the edge and makes a free point on it (coincident with the projection); `"mid:<line>"`
/// makes a point held at the line's midpoint; `"x:<a>+<b>:x,y"` makes a point at the crossing of
/// two lines, circles or arcs (and `"vertex:x,y,z"` is the vertex form). Everything else is left
/// as it is. The links are added to the active sketch in the session's document.
pub(super) fn snap_points(s: &mut Session, p: &Value, cmd: &str) -> Result<Value> {
    let Some(id) = sketch_store::active(s) else { return Ok(p.clone()) };
    let mut out = p.clone();
    let mut doc: Option<(Document, Sketch)> = None;
    let fix = |v: &mut Value, s: &Session, doc: &mut Option<(Document, Sketch)>| -> Result<()> {
        let (vertex, edge) = (v.get("vertex").and_then(vec3), v.get("on_edge").and_then(vec3));
        // String forms from interactive snapping: "vertex:x,y,z" and "mid:<line>".
        let vertex = vertex.or_else(|| {
            let t = v.as_str()?.strip_prefix("vertex:")?;
            let n: Vec<f64> = t.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            match n[..] {
                [x, y, z] if x.is_finite() && y.is_finite() && z.is_finite() => Some(Vec3::new(x, y, z)),
                _ => None,
            }
        });
        let mid = v.as_str().and_then(|x| x.strip_prefix("mid:")).map(str::to_string);
        // "on:<curve>:x,y": a point on a curve of the sketch near x,y.
        let on = v.as_str().and_then(|x| x.strip_prefix("on:")).and_then(|t| {
            let (c, xy) = t.rsplit_once(':')?;
            let n: Vec<f64> = xy.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            match n[..] {
                [x, y] if x.is_finite() && y.is_finite() => Some((c.to_string(), Vec2::new(x, y))),
                _ => None,
            }
        });
        let crossing = curve_intersection_arg(v, cmd)?;
        if vertex.is_none() && edge.is_none() && mid.is_none() && on.is_none() && crossing.is_none() {
            return Ok(());
        }
        if doc.is_none() {
            let d = (*s.doc).clone();
            let mut sk = sketch_store::read(&d, id)?.clone();
            refresh(s, &d, id, &mut sk);
            *doc = Some((d, sk));
        }
        let Some((d, sk)) = doc.as_mut() else { return Ok(()) };
        let body = str_(v, "body").unwrap_or("").to_string();
        let pid = if let Some((curve, near)) = on {
            let c = sk.curve_index(&curve).ok_or_else(|| bad(cmd, format!("unknown curve `{curve}`")))?;
            let at = match sk.shape(c) {
                Some(sh) => sh.project(near),
                None => near,
            };
            let pi = sk.add_point(at, None)?;
            sk.add_constraint(ConstraintKind::PointOnCurve { p: pi, c }, None)?;
            sk.points.get(pi).map(|q| q.id.clone()).unwrap_or_default()
        } else if let Some((a, b, near)) = crossing {
            let ci = sk.curve_index(&a).ok_or_else(|| bad(cmd, format!("unknown curve `{a}`")))?;
            let cj = sk.curve_index(&b).ok_or_else(|| bad(cmd, format!("unknown curve `{b}`")))?;
            if ci == cj {
                return Err(bad(cmd, "`x:` needs two distinct curves"));
            }
            sk.shape(ci).ok_or_else(|| bad(cmd, format!("`{a}` is not a line, circle or arc")))?;
            sk.shape(cj).ok_or_else(|| bad(cmd, format!("`{b}` is not a line, circle or arc")))?;
            let at = sk.nearest_crossing(ci, cj, near).ok_or_else(|| bad(cmd, "those curves do not meet"))?;
            let pi = sk.add_point(at, None)?;
            sk.add_constraint(ConstraintKind::PointOnCurve { p: pi, c: ci }, None)?;
            sk.add_constraint(ConstraintKind::PointOnCurve { p: pi, c: cj }, None)?;
            sk.points.get(pi).map(|q| q.id.clone()).unwrap_or_default()
        } else if let Some(line) = mid {
            // "mid:<line>": a new point at the line's midpoint, kept there.
            let l = sk.curve_index(&line).ok_or_else(|| bad(cmd, format!("unknown curve `{line}`")))?;
            let Some(solidcraft_sketch::Shape::Line { a, b }) = sk.shape(l) else { return Err(bad(cmd, format!("`{line}` is not a line"))) };
            let pi = sk.add_point((a + b) * 0.5, None)?;
            sk.add_constraint(ConstraintKind::Midpoint { p: pi, l }, None)?;
            sk.points.get(pi).map(|q| q.id.clone()).unwrap_or_default()
        } else if let Some(at) = vertex {
            let l = add_link(s, d, id, sk, LinkKind::Project, LinkSource::Vertex { body, at }).map_err(|e| bad(cmd, format!("vertex: {e}")))?;
            let pi = sk.link_points(&l).first().copied().ok_or_else(|| bad(cmd, "vertex projects to nothing"))?;
            sk.points.get(pi).map(|q| q.id.clone()).unwrap_or_default()
        } else if let Some(at) = edge {
            let l = add_link(s, d, id, sk, LinkKind::Project, LinkSource::Edge { body, at }).map_err(|e| bad(cmd, format!("edge: {e}")))?;
            let before = s.model.state_before(id);
            let plane = sketch_plane(d, id, &*state_for_sketch(s, d, id, &before, &[])?)?;
            let q = plane.to_local(at);
            let ci = sk.link_curves(&l).first().copied().ok_or_else(|| bad(cmd, "the edge projects to a point"))?;
            let pi = sk.add_point(q, None)?;
            sk.add_constraint(ConstraintKind::PointOnCurve { p: pi, c: ci }, None)?;
            sk.points.get(pi).map(|q| q.id.clone()).unwrap_or_default()
        } else {
            return Ok(());
        };
        *v = Value::String(pid);
        Ok(())
    };
    if let Some(o) = out.as_object_mut() {
        for (_, v) in o.iter_mut() {
            match v {
                Value::Array(a) if a.len() <= 10_000 => {
                    for x in a.iter_mut() {
                        fix(x, s, &mut doc)?;
                    }
                }
                Value::Object(_) | Value::String(_) => fix(v, s, &mut doc)?,
                _ => {}
            }
        }
    }
    if let Some((mut d, sk)) = doc {
        sketch_store::write(&mut d, id, sk)?;
        *s.doc_mut() = d;
    }
    Ok(out)
}

/// Private compatibility projection for generated face profiles. Named legacy links follow
/// edits, but cannot provide the construction certificate required by public attachments.
pub(super) fn project_internal_face(s: &mut Session, id: u64, body: &str, index: usize) -> Result<String> {
    project_internal_faces(s, id, body, &[index])?.pop().ok_or_else(|| bad("face profile", "empty face projection"))
}

/// Capture individual replay names together, against the same unchanged source body.
pub(super) fn project_internal_faces(s: &mut Session, id: u64, body: &str, indices: &[usize]) -> Result<Vec<String>> {
    if indices.len() > 1024 {
        return Err(bad("face profile", "too many faces"));
    }
    let mut doc = (*s.doc).clone();
    let raw = s.model.state_before(id);
    let component = doc.feature(id).map(|feature| feature.component).unwrap_or(0);
    let state = solidcraft_doc::state_in_frame_at(&doc, &raw, component, owner_frame(s, &doc, component))?;
    let plane = sketch_plane(&doc, id, &state)?;
    let names = state.body(body).map(solidcraft_doc::naming::face_names).unwrap_or_default();

    let mut sk = sketch_store::read(&doc, id)?.clone();
    let mut links = Vec::with_capacity(indices.len());
    for &index in indices {
        let resolved = solidcraft_doc::project::resolve_face_indices(&state, &plane, LinkKind::Project, body, &[index])?;
        let link = sk.add_link_with_wires(LinkKind::Project, resolved.source, &resolved.geom, &resolved.wires)?;
        if let Some(entry) = sk.links.iter_mut().find(|entry| entry.id == link) {
            entry.face_name = names.get(index).cloned();
        }
        links.push(link);
    }
    sketch_store::write(&mut doc, id, sk)?;
    *s.doc_mut() = doc;
    Ok(links)
}

/// Project the edges of the face a new sketch was created on (the default).
pub(super) fn auto_project_face(s: &mut Session, id: u64, _at: Vec3) -> Result<Option<String>> {
    let mut doc = (*s.doc).clone();
    let Some(anchor) = doc
        .feature(id)
        .and_then(|feature| match &feature.kind {
            solidcraft_doc::FeatureKind::Sketch { plane, .. } => super::sketch::face::anchor(plane),
            _ => None,
        })
        .cloned()
    else {
        return Ok(None);
    };
    let raw = s.model.state_before(id);
    let author = doc.feature(id).map(|feature| feature.component).unwrap_or(0);
    let state = solidcraft_doc::state_in_face_anchor_frame(&doc, &raw, &anchor, author)?;
    let plane = doc.resolve_face_anchor(&anchor, author, &raw)?;
    let resolved = solidcraft_doc::project::resolve_anchored_face(&doc, &state, &plane, LinkKind::Project, &anchor)?;
    let mut sk = sketch_store::read(&doc, id)?.clone();
    let link = sk.add_link_with_wires(LinkKind::Project, resolved.source, &resolved.geom, &resolved.wires)?;
    if let Some(link) = sk.links.iter_mut().find(|entry| entry.id == link) {
        link.face_name = Some(anchor.target.name.clone());
        link.auto_face = true;
    }
    sketch_store::write(&mut doc, id, sk)?;
    *s.doc_mut() = doc;
    Ok(Some(link))
}

#[cfg(test)]
#[path = "sketch_project_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "sketch_project_side_tests.rs"]
mod side_tests;

#[cfg(test)]
#[path = "sketch_project_edge_tests.rs"]
mod edge_tests;

#[cfg(test)]
#[path = "sketch_model_plane_tests.rs"]
mod model_plane_tests;

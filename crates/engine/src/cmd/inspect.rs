//! Inspection: measure bodies, describe the document, sketches and model topology.

use std::collections::BTreeMap;
use std::hash::{BuildHasher, RandomState};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};

use serde_json::{Value, json};
use solidcraft_sketch::CurveKind;

use solidcraft_geom::Vec3;

use super::CommandSpec;
use crate::params::{bad, bool_, expr, str_, string_list};
use crate::{EngineError, Result, Session};

#[path = "appearance_chain.rs"]
mod appearance_chain;

#[path = "appearance_gestures.rs"]
pub(super) mod appearance_gestures;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("inspect.colored_components", "Enable Component Colors", colored_components)
        .at("SOLID", "INSPECT").icon("colored_components").noundo()
        .params("enabled?: bool (omitted: toggle); temporary component colours, without changing appearances"),
    CommandSpec::new("inspect.measure", "Measure", measure)
        .at("SOLID", "INSPECT")
        .icon("measure")
        .key("I")
        .noundo()
        .params("bodies?: [names] (default all); names_only?: bool (each world body's name and its definition's, nothing measured) → closed per body; volume_mm3, center_of_mass, density_g_cm3 and mass_g only for closed, consistently oriented bodies; area, bbox and counts for all; total.open_bodies_skipped counts bodies excluded from volume/mass totals; or items: [1-2 selections] (face/edge: type, body, point; index optional) → length/area/volume; diameter, radius, circumference or arc_length/angle_deg of circles, arcs and cylinders; slot sizes; min (distance_mm, from, to), max, center_distance (centre to centre, or from an axis) and angle"),
    CommandSpec::new("document.inspect", "Inspect Design", inspect).icon("inspect").noundo().params("measure?: bool (include body measurements)"),
    CommandSpec::new("document.revision", "Design Revision", revision).icon("inspect").noundo().params(
        "→ document_id (this open design's id: kept while it stays open, another for each design started or opened), \
         revision (moves whenever the design, what is hidden or a component's uncaptured position changes, \
         never on a selection or a view; from one counter for the whole run, so it never repeats). Read-only",
    ),
    CommandSpec::new("sketch.inspect", "Inspect Sketch", sketch_inspect).at("SKETCH", "INSPECT").icon("inspect").noundo().params("sketch?: id|name (default active)"),
    CommandSpec::new("model.edges", "List Edges", model_edges).icon("inspect").noundo().params("body: name"),
    CommandSpec::new("model.faces", "List Faces", model_faces).icon("inspect").noundo().params(
        "body: name → per face: index, name (what face.delete, sheet.convert `face`, solid.shell … take in place of a point), exact, area, centroid (not always on the face), \
         plane_normal, point (a point on the face that picks it), cylinder? ({origin, axis, radius} of a cylindrical face), \
         sheet_face? (a folded sheet's face: `sheet:<body>:<panel>:<side>`, panel base or a flange's tag F<feature>.<k>, side top | bottom | rim.<edge> | bend.inner | bend.outer; \
         taken wherever a face name is, and it holds while sizes change and flanges come and go)",
    ),
    CommandSpec::new("model.threads", "List Threads", model_threads).icon("inspect").noundo(),
    CommandSpec::new("inspect.find_holes", "Find Holes", find_holes)
        .at("INSPECT", "MEASURE")
        .icon("hole")
        .noundo()
        .params(
            "max_diameter?: expr (only holes no wider than this); min_diameter?: expr (only holes wider than this); bodies?: [names] (default all); holes_only?: bool (default true: cylinders that lean in, holes not bosses); \
             type?: feature type (Hole, Extrude, the feature's base name, or solid.hole); feature?: feature name or id. \
             At least one of max_diameter, min_diameter, type and feature. \
             → holes: [{body, diameter, axis, center, concave, edges: [[x,y,z] on the round edges that close the hole, both ends, as solid.chamfer takes them], feature?: {id, name, type}}], count, edges (every closing edge, flat)",
        ),
    CommandSpec::new("material.assign", "Physical Material", physical_material)
        .at("SOLID", "MODIFY")
        .icon("material")
        .params("bodies: [names]; material: name (Steel, Aluminum, ABS Plastic, …, or one made with material.create; `material.list`)"),
    CommandSpec::new("appearance.assign", "Appearance", appearance).at("SOLID", "MODIFY").icon("appearance").key("A").params(
        "bodies?: [names]; faces?: [[x,y,z] on faces] (body?: which body, default the nearest); components?: [names or ids, or occurrence names]; \
             appearance?: library name or stable starter.* ID (appearance.library) | color: \"#rrggbb\" or [r, g, b] (0–255), opacity? (0–1, default 1); \
             name?: inert copied look label (requires color, mutually exclusive with appearance); pbr?: {metallic:0..1, roughness:0.045..1, dielectric_ior:1..3} (requires color/appearance and opacity 1); a starter ID or registered starter name resolves its recipe before color overrides; omitted pbr retains that response and pbr: null explicitly clears it. Unavailable glass recipes refuse through either selector. Other custom labels with an explicit color retain legacy scalar behavior. Neither appearance nor color (or color: null): clear, so the face follows its body, the body its component, the component its material",
    ),
    CommandSpec::new("appearance.copy", "Copy Look", appearance_gestures::copy).icon("copy").noundo().key("Ctrl+Alt+C")
        .params("target: appearance.chain target. Copies its resolved look to a transient session clipboard. No document change or dialogs."),
    CommandSpec::new("appearance.paste", "Paste Look", appearance_gestures::paste).icon("paste").key("Ctrl+Alt+V")
        .params("bodies/faces/components: appearance.assign targets. Applies the copied look atomically in one undo step; no dialogs."),
    CommandSpec::new("appearance.favorite", "Favorite Look", appearance_gestures::favorite).icon("star").noundo().doc_only()
        .params("look: appearance.assign look; favorite: bool → validated look and preference request; UI library preference only, no document change or dialogs."),
    CommandSpec::new("appearance.chain", "Appearance Chain", appearance_chain::chain).icon("inspect").noundo().params(
        "target: {body: name} | {body: name, face: zero-based model.faces index} | {component: name or id}; a placed copy's name (`KB (K:2)`, `K:2`) means the body or component it shows. Returns steps [{step: face|body|component|material, look: {name,color,opacity,pbr?} or null}] in precedence order and active (step or null). Component material is the common default look of its directly owned bodies, null if mixed/empty. Imported paint is a display fallback outside the four steps. Read-only; no dialogs",
    ),
    CommandSpec::new("appearance.replace", "Replace Appearance", appearance_chain::replace).icon("appearance").params(
        "from: library name | saved look {name,color,opacity,pbr?} (copy appearance.list in_design); to: library name | assignment look {appearance? | color, name?, opacity?, pbr?} (appearance.assign rules; no clear). Matches the full look including name and PBR. scope?: {bodies?: [names], components?: [names or ids]} (a placed copy's name means the body or component it shows), at most 10000 targets; body scope includes face overrides, component scope selects only the component override. Omitted scope means all overrides, including hidden/suppressed ones; physical defaults are unchanged. Duplicate targets count once. Returns count of rewritten overrides; zero adds no history. One undo step, atomic; no dialogs. Designs above 100000 overrides refuse",
    ),
    CommandSpec::new("appearance.library", "Appearance Library", appearance_library).icon("library")
        .noundo()
        .params("→ 20 legacy scalar appearances, six opaque PBR finishes and 40 starter recipes with IDs, categories, provenance, requirements and availability; six starter glass recipes are unavailable"),
    CommandSpec::new("appearance.list", "List Appearances", appearance_list).icon("list")
        .noundo()
        .params("body?: name (or a placed copy's, `KB (K:2)`: the body it shows) → assigned appearances; with body, its resolved look and its faces' (face index, look)"),
    CommandSpec::new("material.list", "List Materials", material_list).icon("list")
        .noundo()
        .doc_only()
        .params("→ built-in and design-owned materials (user: true, look, optional physical_card/physical_validation/density_comparison). Read-only; candidate engineering records do not certify sources or supply a solver"),
    CommandSpec::new("engine.commands", "List Commands", commands).icon("list").noundo(),
    CommandSpec::new("engine.describe", "Describe Command", crate::checked!("engine.describe", describe))
        .icon("list")
        .noundo()
        .doc_only()
        .contract(&DESCRIBE_CONTRACT)
        .output(crate::schema::output_schema_for::<crate::schema::inspect::DescribeResult>)
        .params("command: a command id. Returns {id, label, params, input, output} (and param_warnings in the warn parameter-check mode): the command's parameter documentation and, once the command has a typed contract, the JSON Schemas (2020-12) of its parameters and of a successful result (null otherwise). Read-only."),
];

/// Does `body` enclose a volume? A B-rep body does when every shell is closed and consistently
/// oriented (not a surface body), a mesh body when every triangle side is shared by exactly two
/// triangles. Only that is asked: other findings of a full validity check (faces that don't
/// mesh, tolerance gaps of an imported solid) don't take a closed body's volume away, and the
/// check stays cheap for designs with many bodies.
pub(crate) fn encloses_volume(body: &solidcraft_kernel::Body) -> bool {
    if body.is_mesh() { body.is_closed_mesh() } else { !body.is_surface() }
}

pub fn measure_json(b: &solidcraft_doc::ModelBody) -> Value {
    match b.measure() {
        Ok(m) => {
            let closed = encloses_volume(&b.body);
            json!({
                "name": b.name,
                "closed": closed,
                "volume_mm3": closed.then_some(m.volume),
                "area_mm2": m.area,
                "center_of_mass": closed.then_some(m.centroid),
                "bbox": {"min": m.bbox.min, "max": m.bbox.max},
                "faces": m.merged.faces,
                "edges": m.merged.edges,
                "vertices": m.merged.vertices,
                "face_types": m.merged.face_types,
                "kernel": {"faces": m.faces, "edges": m.edges, "vertices": m.vertices, "shells": m.shells},
            })
        }
        Err(e) => json!({"name": b.name, "error": e.to_string()}),
    }
}

fn measure(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.measure";
    if let Some(items) = p.get("items") {
        return super::measure_sel::measure_items(s, items);
    }
    let want = string_list(cmd, p, "bodies")?;
    let st = s.world_state();
    let selected: Vec<_> = st.bodies.iter().filter(|b| want.is_empty() || want.contains(&b.name)).collect();
    if !want.is_empty() && selected.len() != want.len() {
        return Err(bad(cmd, "unknown body name"));
    }
    if bool_(p, "names_only").unwrap_or(false) {
        let bodies: Vec<_> = selected.iter().map(|b| json!({"name": b.name, "definition": b.definition_name()})).collect();
        return Ok(json!({"body_count": bodies.len(), "bodies": bodies}));
    }
    // Each body is measured on its own: on all cores, in body order.
    let mut bodies: Vec<Value> = solidcraft_kernel::map_on_threads(&selected, 16, |b| measure_json(b))
        .into_iter()
        .zip(&selected)
        .map(|(m, b)| m.unwrap_or_else(|| json!({"name": b.name, "error": "measuring this body failed"})))
        .collect();
    // Mass from each body's material.
    for (b, body) in bodies.iter_mut().zip(selected) {
        let source = body.definition_name();
        // Every body's material, measured or not: naming it takes no measuring.
        b["material"] = json!(s.doc.body_material(source));
        if let Some(v) = b["volume_mm3"].as_f64() {
            let rho = s.doc.density(source);
            b["density_g_cm3"] = json!(rho);
            b["mass_g"] = json!(v * rho / 1000.0);
        }
    }
    // Sheet metal keeps the edges where bends meet flat faces (so bends stay visible): count its
    // B-rep faces as they are, without merging coplanar neighbours.
    let sheets = s.model.state().sheets.clone();
    for b in &mut bodies {
        if let Some(sh) = sheets.iter().find(|x| b["name"].as_str() == Some(x.body.as_str())) {
            let (df, de, dv) = sh.round_hole_correction();
            for (k, d) in [("faces", df), ("edges", de), ("vertices", dv)] {
                b[k] = json!(b["kernel"][k].as_i64().unwrap_or(0) + d);
            }
            b["sheet_metal"] = json!(true);
            // A perforation field (#732) is not cut into the B-rep: take its open area out of the volume, the mass
            // and the area (both faces minus the holes, plus the bores).
            let open = sh.perforated_area();
            if open > 0.0 {
                if let Some(v) = b["volume_mm3"].as_f64() {
                    let nv = v - open * sh.t;
                    if let Some(m) = b["mass_g"].as_f64().filter(|_| v > 0.0) {
                        b["mass_g"] = json!(m * nv / v);
                    }
                    b["volume_mm3"] = json!(nv);
                }
                let bores: f64 = sh
                    .perforations
                    .iter()
                    .map(|p| sh.perforation_holes(p).map_or(0.0, |h| h.len() as f64) * std::f64::consts::PI * p.diameter * sh.t)
                    .sum();
                b["area_mm2"] = json!(b["area_mm2"].as_f64().unwrap_or(0.0) - 2.0 * open + bores);
            }
        }
    }
    let skipped = bodies.iter().filter(|b| b["closed"] == false).count();
    let tv: f64 = bodies.iter().filter_map(|b| b["volume_mm3"].as_f64()).sum();
    let ta: f64 = bodies.iter().filter_map(|b| b["area_mm2"].as_f64()).sum();
    let tf: u64 = bodies.iter().filter_map(|b| b["faces"].as_u64()).sum();
    let te: u64 = bodies.iter().filter_map(|b| b["edges"].as_u64()).sum();
    let tvx: u64 = bodies.iter().filter_map(|b| b["vertices"].as_u64()).sum();
    let tm: f64 = bodies.iter().filter_map(|b| b["mass_g"].as_f64()).sum();
    Ok(
        json!({"body_count": bodies.len(), "bodies": bodies, "total": {"open_bodies_skipped": skipped, "volume_mm3": tv, "area_mm2": ta, "mass_g": tm, "faces": tf, "edges": te, "vertices": tvx}}),
    )
}

fn sketch_json(s: &Session, id: u64) -> Option<Value> {
    let state = s.model.state();
    let ss = state.sketch(id)?;
    // Where the sketch is in the world (its component's placement).
    let world = s.world_state();
    let plane = world.sketch(id).map(|w| w.plane).unwrap_or(ss.plane);
    let sk = &ss.sketch;
    let curves: Vec<Value> = sk
        .curves
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let det = ss.report.curve_determined.get(i).copied().unwrap_or(false);
            let pid = |k: usize| sk.points.get(k).map(|p| p.id.clone()).unwrap_or_default();
            let mut v = match c.kind {
                CurveKind::Line { a, b } => json!({"id": c.id, "type": "line", "start": pid(a), "end": pid(b), "start_at": sk.point(a), "end_at": sk.point(b), "construction": c.construction, "fixed": sk.curve_locked(i), "fully_constrained": det}),
                CurveKind::Circle { c: cc, r } => json!({"id": c.id, "type": "circle", "center": pid(cc), "center_at": sk.point(cc), "radius": r, "construction": c.construction, "fixed": sk.curve_locked(i), "fully_constrained": det}),
                CurveKind::Arc { c: cc, a, b } => json!({"id": c.id, "type": "arc", "center": pid(cc), "start": pid(a), "end": pid(b), "center_at": sk.point(cc), "start_at": sk.point(a), "end_at": sk.point(b), "radius": sk.radius(i), "construction": c.construction, "fixed": sk.curve_locked(i), "fully_constrained": det}),
                CurveKind::Ellipse { c: cc, m, r } => json!({"id": c.id, "type": "ellipse", "center": pid(cc), "major": pid(m), "center_at": sk.point(cc), "major_at": sk.point(m), "minor_radius": r, "construction": c.construction, "fixed": sk.curve_locked(i), "fully_constrained": det}),
                CurveKind::Spline { ref pts, control, degree, periodic } => json!({"id": c.id, "type": "spline", "points": pts.iter().map(|q| pid(*q)).collect::<Vec<_>>(), "points_at": pts.iter().map(|q| sk.point(*q)).collect::<Vec<_>>(), "control": control, "degree": degree, "periodic": periodic, "construction": c.construction, "fixed": sk.curve_locked(i), "fully_constrained": det}),
                CurveKind::Conic { a, b, apex, rho } => json!({"id": c.id, "type": "conic", "start": pid(a), "end": pid(b), "apex": pid(apex), "start_at": sk.point(a), "end_at": sk.point(b), "apex_at": sk.point(apex), "rho": rho, "construction": c.construction, "fixed": sk.curve_locked(i), "fully_constrained": det}),
            };
            if let (Some(l), Some(o)) = (&c.link, v.as_object_mut()) {
                o.insert("link".into(), json!(l));
            }
            v
        })
        .collect();
    let locked = sk.locked_points();
    let points: Vec<Value> = sk
        .points
        .iter()
        .enumerate()
        .map(|(i, p)| json!({"id": p.id, "at": p.pos, "world": plane.to_world(p.pos), "fixed": locked.get(i).copied().unwrap_or(false), "link": p.link, "fully_constrained": ss.report.point_determined.get(i).copied().unwrap_or(false)}))
        .collect();
    let constraints: Vec<Value> = sk
        .constraints
        .iter()
        .map(|c| {
            let mut v = serde_json::to_value(&c.kind).unwrap_or_default();
            if let Some(o) = v.as_object_mut() {
                o.insert("id".into(), json!(c.id));
                o.insert("name".into(), json!(c.kind.name()));
                if let Some(p) = &c.param {
                    o.insert("param".into(), json!(p));
                    o.insert("expression".into(), json!(s.doc.param(p).map(|x| x.expr.clone())));
                }
            }
            v
        })
        .collect();
    let profiles: Vec<Value> = ss
        .profiles
        .iter()
        .enumerate()
        // A loop records one entry per piece it walks, and a conic or spline is walked as many
        // short pieces: each loop names a curve once, in loop order.
        .map(|(i, p)| {
            let holes: Vec<Vec<String>> = p.hole_curves.iter().map(|h| solidcraft_sketch::distinct_curves(h)).collect();
            json!({"index": i, "area_mm2": p.area, "centroid_sketch": p.centroid, "curves": solidcraft_sketch::distinct_curves(&p.outer_curves), "holes": holes})
        })
        .collect();
    Some(json!({
        "id": id,
        "name": ss.name,
        "plane": ss.plane,
        "status": ss.report.status,
        "dof": ss.report.dof,
        "fully_constrained": ss.report.fully_constrained(),
        "failing": ss.report.failing,
        "curves": curves,
        "points": points,
        "constraints": constraints,
        "profiles": profiles,
        "links": sk.links,
        "wires": sk.wires.iter().map(|w| json!({"id": w.id, "points": w.pts.len(), "start": w.pts.first(), "end": w.pts.last(), "link": w.link, "arc": w.arc.map(|a| json!({"center": a.center, "radius": a.radius, "start": a.start, "end": a.end, "sweep_deg": solidcraft_sketch::arc_sweep(&a).to_degrees()}))})).collect::<Vec<_>>(),
        "visible": crate::view::sketch_shown(s, id),
    }))
}

fn sketch_inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let id = super::sketch_store::target(s, p, "sketch.inspect")?;
    sketch_json(s, id).ok_or_else(|| EngineError::Other("that sketch is not evaluated (rolled back or not a sketch)".into()))
}

/// Revisions of every design of this run (#6646).
static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

/// The design as `document.revision` last saw it, which its revision names.
pub(crate) struct Followed {
    /// Held weakly: a design something holds weakly is never changed in place (`Arc::make_mut`
    /// moves it), and its address can't go to another design meanwhile.
    doc: Weak<solidcraft_doc::Document>,
    visibility: crate::Visibility,
    pending_moves: BTreeMap<u64, solidcraft_doc::Mat>,
    revision: u64,
}

/// A new design's id: one for this run, then a number, so no two designs of one run, or of two
/// runs, have the same.
pub(crate) fn new_document_id() -> String {
    static RUN: OnceLock<u64> = OnceLock::new();
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let run = RUN.get_or_init(|| {
        let started = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        // Hashed with this process's random keys (where the system has them), so two runs differ
        // even with the clock set back.
        RandomState::new().hash_one(started)
    });
    format!("{run:016x}-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

impl Session {
    /// The revision of what the design holds now (`document.revision`), a new one when it changed
    /// since last asked. Changes between two asks that end where they began (an edit undone) need
    /// no new one: the design is the one that revision named.
    pub fn design_revision(&mut self) -> u64 {
        let seen = |f: &Followed| {
            std::ptr::eq(f.doc.as_ptr(), Arc::as_ptr(&self.doc)) && f.visibility == self.visibility && f.pending_moves == self.pending_moves
        };
        match self.followed.as_ref().filter(|f| seen(f)) {
            Some(f) => f.revision,
            None => {
                let revision = NEXT_REVISION.fetch_add(1, Ordering::Relaxed);
                let (doc, visibility, pending_moves) = (Arc::downgrade(&self.doc), self.visibility.clone(), self.pending_moves.clone());
                self.followed = Some(Followed { doc, visibility, pending_moves, revision });
                revision
            }
        }
    }
}

/// `document.revision`: the design's id and [`Session::design_revision`].
fn revision(s: &mut Session, _p: &Value) -> Result<Value> {
    let revision = s.design_revision();
    Ok(json!({"document_id": s.document_id, "revision": revision}))
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.model.state();
    let (vals, perr) = s.doc.param_values();
    let params: Vec<Value> = s
        .doc
        .params
        .iter()
        .map(|p| json!({"name": p.name, "expression": p.expr, "unit": p.unit, "value": vals.get(&p.name).map(|v| v.v), "model": p.model, "comment": p.comment, "error": perr.get(&p.name)}))
        .collect();
    let marker = s.doc.marker.unwrap_or(s.doc.features.len());
    let timeline: Vec<Value> = s
        .doc
        .features
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let r = s.model.result(f.id);
            json!({
                "id": f.id, "name": f.name, "type": f.kind.type_name(), "suppressed": f.suppressed, "rolled_back": i >= marker,
                "error": r.and_then(|r| r.error.clone()), "warning": r.and_then(|r| r.warning.clone()), "ms": r.map(|r| r.ms),
                "boolean_diagnostics": r.map(|r| &r.boolean_diagnostics),
            })
        })
        .collect();
    let with_measure = bool_(p, "measure").unwrap_or(false);
    // Each body is measured on its own: on all cores, in body order (as `inspect.measure`).
    let mut measured = if with_measure { solidcraft_kernel::map_on_threads(&st.bodies, 16, measure_json) } else { Vec::new() }.into_iter();
    let bodies: Vec<Value> = st
        .bodies
        .iter()
        .map(|b| {
            let mut v = match measured.next() {
                Some(m) => m.unwrap_or_else(|| json!({"name": b.name, "error": "measuring this body failed"})),
                None => json!({"name": b.name, "feature": b.feature}),
            };
            if let Some(o) = v.as_object_mut() {
                o.insert("visible".into(), json!(s.body_visible(&b.name)));
                if s.body_removed_later(&b.name) {
                    // Editing a sketch rolls the timeline back; this body is deleted further on.
                    o.insert("removed_later".into(), json!(true));
                }
            }
            v
        })
        .collect();
    let sketches: Vec<Value> = st
        .sketches
        .iter()
        .map(|ss| json!({"id": ss.feature, "name": ss.name, "dof": ss.report.dof, "status": ss.report.status, "curves": ss.sketch.curves.len(), "profiles": ss.profiles.len(), "visible": crate::view::sketch_shown(s, ss.feature)}))
        .collect();
    Ok(json!({
        "name": s.doc.name, "units": s.doc.units, "metadata": s.doc.metadata, "path": s.path, "dirty": s.is_dirty(), "revision": s.revision,
        "active_sketch": s.active_sketch, "marker": marker,
        "params": params, "timeline": timeline, "bodies": bodies, "sketches": sketches,
        "selection": s.selection, "undo": s.undo.len(), "redo": s.redo.len(), "hidden_origin": s.visibility.hidden_origin,
    }))
}

fn body_of<'a>(st: &'a solidcraft_doc::ModelState, p: &Value, cmd: &str) -> Result<&'a solidcraft_doc::ModelBody> {
    let n = str_(p, "body").ok_or_else(|| bad(cmd, "`body` is required"))?;
    st.body(n).ok_or_else(|| bad(cmd, format!("no body `{n}`")))
}

fn model_edges(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.world_state();
    let b = body_of(&st, p, "model.edges")?;
    let tol = (b.body.size() * 2e-3).max(1e-3);
    let names = solidcraft_doc::naming::edge_names(b);
    let edges: Vec<Value> = b
        .body
        .edges(tol)?
        .iter()
        .map(|e| json!({"index": e.index, "name": names.get(e.index), "mid": e.mid, "length": e.length, "start": e.points.first(), "end": e.points.last()}))
        .collect();
    Ok(json!({"body": b.name, "edges": edges}))
}

fn model_faces(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.world_state();
    let b = body_of(&st, p, "model.faces")?;
    let names = solidcraft_doc::naming::face_names(b);
    let tol = (b.body.size() * 2e-3).max(1e-3);
    let spots = solidcraft_doc::naming::face_spots(b);
    // A folded sheet's faces also have the readable names its sheet gives them, worked out in
    // the body's own coordinates.
    let model = s.model.state();
    let sheet_names = model.body(&b.name).map(|own| solidcraft_doc::sheet::body_sheet_face_names(&model, own)).unwrap_or_default();
    let faces: Vec<Value> = b
        .body
        .faces(tol)?
        .iter()
        .map(|f| {
            let spot = spots.get(f.index).copied().flatten();
            let mut v = json!({"index": f.index, "name": names.get(f.index), "exact": solidcraft_doc::naming::face_name_exact(b, f.index),
                   "area": f.area, "centroid": f.centroid, "plane_normal": f.plane_normal, "point": spot.map(|x| x.point)});
            if let (Some(Some(name)), Some(o)) = (sheet_names.get(f.index), v.as_object_mut()) {
                o.insert("sheet_face".into(), json!(name));
            }
            if let (Some(solidcraft_kernel::FaceSurface::Cylinder { origin, axis, radius }), Some(o)) =
                (spot.and_then(|x| x.surface), v.as_object_mut())
            {
                o.insert("cylinder".into(), json!({"origin": origin, "axis": axis, "radius": radius}));
            }
            v
        })
        .collect();
    Ok(json!({"body": b.name, "faces": faces}))
}

const DESCRIBE_CONTRACT: crate::schema::TypedContract<crate::schema::inspect::DescribeParams> = crate::schema::TypedContract::new();

/// A command's prose documentation and, for converted commands, its schemas.
fn describe(_s: &mut Session, p: &Value) -> Result<Value> {
    let id = p.get("command").and_then(Value::as_str).ok_or_else(|| crate::params::bad("engine.describe", "`command` must be a command id"))?;
    let spec = super::find_command(id).ok_or_else(|| crate::EngineError::UnknownCommand(id.to_string()))?;
    let reply = crate::schema::inspect::DescribeResult {
        id: spec.id.to_string(),
        label: spec.label.to_string(),
        params: spec.params.to_string(),
        input: spec.contract.map(|c| c.input_schema().to_value()),
        output: spec.output.map(|f| f().to_value()),
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| crate::EngineError::Other(e.to_string()))
}

fn find_holes(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "inspect.find_holes";
    let length = |key: &str| -> Result<Option<f64>> {
        match expr(p, key) {
            Some(e) => {
                let d = s.doc.eval(&e, solidcraft_doc::expr::Kind::Length)?;
                if !(d.is_finite() && d > 0.0 && d <= 1e6) {
                    return Err(bad(cmd, format!("`{key}` must be a positive length in millimetres")));
                }
                Ok(Some(d))
            }
            None => Ok(None),
        }
    };
    let max_diameter = length("max_diameter")?;
    let min_diameter = length("min_diameter")?;
    let type_filter = str_(p, "type").map(str::trim).filter(|s| !s.is_empty());
    let feature_filter = str_(p, "feature").map(str::trim).filter(|s| !s.is_empty());
    if max_diameter.is_none() && min_diameter.is_none() && type_filter.is_none() && feature_filter.is_none() {
        return Err(bad(cmd, "give `max_diameter`, `min_diameter`, `type` or `feature`"));
    }
    if let Some(want) = feature_filter
        && !s.doc.features.iter().any(|f| f.name.eq_ignore_ascii_case(want) || f.id.to_string() == want)
    {
        return Err(bad(cmd, format!("no feature `{want}`")));
    }
    let want_bodies = string_list(p, "bodies");
    let holes_only = bool_(p, "holes_only").unwrap_or(true);
    let feats = hole_features(s);
    let st = s.world_state();
    let selected: Vec<_> = st.bodies.iter().filter(|b| want_bodies.is_empty() || want_bodies.iter().any(|n| n == &b.name)).collect();
    if !want_bodies.is_empty() && selected.len() != want_bodies.len() {
        return Err(bad(cmd, "unknown body name"));
    }
    let mut holes: Vec<Value> = Vec::new();
    for b in selected {
        let tol = (b.body.size() * 2e-3).max(1e-3);
        let Ok(edges) = b.body.edges(tol) else { continue };
        let Ok(faces) = b.body.faces(tol) else { continue };
        let spots = solidcraft_doc::naming::face_spots(b);
        let face_names = solidcraft_doc::naming::face_names(b);
        let mesh = b.mesh();
        // One hole per cylinder: a drilled hole's wall splits into halves that share axis and radius.
        let mut found: Vec<HoleGroup> = Vec::new();
        for f in faces {
            let Some(Some(spot)) = spots.get(f.index).copied() else { continue };
            let Some(solidcraft_kernel::FaceSurface::Cylinder { origin, axis, radius }) = spot.surface else { continue };
            let diameter = 2.0 * radius;
            if max_diameter.is_some_and(|max| diameter > max + tol) {
                continue;
            }
            if min_diameter.is_some_and(|min| diameter <= min + tol) {
                continue;
            }
            let axis = Vec3::from(axis);
            let concave = cylinder_leans_in(&mesh, f.index, origin, axis, f.centroid);
            if holes_only && concave == Some(false) {
                continue;
            }
            let mut edge_points: Vec<[f64; 3]> = edges
                .iter()
                .filter(|e| {
                    let on = |q: &Vec3| (radial_distance(*q, origin, axis) - radius).abs() <= tol;
                    let curved = e.points.len() > 2 && {
                        let Some(a) = e.points.first() else { return false };
                        let Some(b) = e.points.last() else { return false };
                        let along: f64 = e.points.windows(2).map(|w| w[0].dist(w[1])).sum();
                        along - a.dist(*b) > (0.02 * radius).max(1e-3)
                    };
                    curved && e.points.iter().all(on)
                })
                .map(|e| [e.mid.x, e.mid.y, e.mid.z])
                .collect();
            edge_points.sort_unstable_by_key(key3);
            edge_points.dedup_by(|a, b| Vec3::from(*a).dist(Vec3::from(*b)) <= tol);
            let mentioned = face_names.get(f.index).cloned().unwrap_or_default();
            if let Some(group) = found
                .iter_mut()
                .find(|g| (g.radius - radius).abs() <= tol && axis.cross(g.axis).len() <= 1e-3 && g.axis.cross(g.origin - origin).len() <= tol)
            {
                group.concave = group.concave.or(concave);
                group.edges.extend(edge_points);
                if !mentioned.is_empty() {
                    group.names.push(mentioned);
                }
            } else {
                found.push(HoleGroup {
                    origin,
                    axis,
                    radius,
                    concave,
                    edges: edge_points,
                    names: if mentioned.is_empty() { Vec::new() } else { vec![mentioned] },
                });
            }
        }
        for mut group in found {
            group.edges.sort_unstable_by_key(key3);
            group.edges.dedup_by(|a, b| Vec3::from(*a).dist(Vec3::from(*b)) <= tol);
            let n = group.edges.len().max(1) as f64;
            let mut sum = Vec3::ZERO;
            for pt in &group.edges {
                sum += Vec3::from(*pt);
            }
            let center = if group.edges.is_empty() { group.origin } else { Vec3::new(sum.x / n, sum.y / n, sum.z / n) };
            let feat = attribute_hole(&feats, group.radius * 2.0, group.axis, center, &group.names, tol);
            if let Some(want) = feature_filter
                && !feat.as_ref().is_some_and(|f| f.name.eq_ignore_ascii_case(want) || f.id.to_string() == want)
            {
                continue;
            }
            if let Some(want) = type_filter
                && !feat.as_ref().is_some_and(|f| hole_feat_matches_type(f, want))
            {
                continue;
            }
            let mut hole = json!({
                "body": b.name,
                "diameter": 2.0 * group.radius,
                "axis": [group.axis.x, group.axis.y, group.axis.z],
                "center": [center.x, center.y, center.z],
                "concave": group.concave,
                "edges": group.edges,
            });
            if let Some(f) = feat
                && let Some(o) = hole.as_object_mut()
            {
                o.insert("feature".into(), json!({"id": f.id, "name": f.name, "type": f.kind}));
            }
            holes.push(hole);
        }
    }
    holes.sort_by(|a, b| {
        let k = |h: &Value| {
            (
                h.get("body").and_then(Value::as_str).unwrap_or_default().to_string(),
                h.get("center")
                    .and_then(Value::as_array)
                    .map(|c| c.iter().filter_map(Value::as_f64).map(|x| (x * 1e6) as i64).collect::<Vec<_>>())
                    .unwrap_or_default(),
            )
        };
        k(a).cmp(&k(b))
    });
    let mut all_edges = Vec::new();
    for h in &holes {
        if let Some(es) = h.get("edges").and_then(Value::as_array) {
            all_edges.extend(es.iter().cloned());
        }
    }
    Ok(json!({"max_diameter": max_diameter, "min_diameter": min_diameter, "holes": holes, "count": holes.len(), "edges": all_edges}))
}

struct HoleGroup {
    origin: Vec3,
    axis: Vec3,
    radius: f64,
    concave: Option<bool>,
    edges: Vec<[f64; 3]>,
    names: Vec<String>,
}

struct HoleFeat {
    id: u64,
    name: String,
    kind: String,
    kind_label: String,
    diameter: Option<f64>,
    origin: Option<Vec3>,
    axis: Option<Vec3>,
}

fn hole_features(s: &Session) -> Vec<HoleFeat> {
    s.doc
        .features
        .iter()
        .filter(|f| !f.suppressed)
        .map(|f| {
            let (diameter, origin, axis) = if let solidcraft_doc::FeatureKind::Hole { position, direction, diameter, .. } = &f.kind {
                let dia = s.doc.eval(diameter, solidcraft_doc::expr::Kind::Length).ok().filter(|d| d.is_finite() && *d > 0.0);
                (dia, Some(position.0), direction.normalized())
            } else {
                (None, None, None)
            };
            HoleFeat {
                id: f.id,
                name: f.name.clone(),
                kind: f.kind.base_name().to_string(),
                kind_label: f.kind.type_name().to_string(),
                diameter,
                origin,
                axis,
            }
        })
        .collect()
}

fn hole_feat_matches_type(f: &HoleFeat, want: &str) -> bool {
    f.kind.eq_ignore_ascii_case(want)
        || f.kind_label.eq_ignore_ascii_case(want)
        || want.eq_ignore_ascii_case(&format!("solid.{}", f.kind.to_ascii_lowercase()))
}

fn attribute_hole<'a>(feats: &'a [HoleFeat], diameter: f64, axis: Vec3, center: Vec3, names: &[String], tol: f64) -> Option<&'a HoleFeat> {
    let mut best: Option<(&HoleFeat, f64)> = None;
    for f in feats.iter().filter(|f| f.kind.eq_ignore_ascii_case("Hole")) {
        let (Some(d), Some(ax), Some(origin)) = (f.diameter, f.axis, f.origin) else { continue };
        if (d - diameter).abs() > (0.02 * d).max(tol) {
            continue;
        }
        if ax.cross(axis).len() > 1e-2 && ax.cross(-axis).len() > 1e-2 {
            continue;
        }
        let radial = radial_distance(origin, center, axis);
        if radial > (0.05 * d).max(1.0) {
            continue;
        }
        if best.is_none_or(|(_, old)| radial < old) {
            best = Some((f, radial));
        }
    }
    if let Some((f, _)) = best {
        return Some(f);
    }
    feats.iter().find(|f| names.iter().any(|n| name_mentions(n, &f.name)))
}

fn name_mentions(face: &str, feature: &str) -> bool {
    face.split([':', ' ', '/']).any(|part| part.eq_ignore_ascii_case(feature))
}

fn key3(p: &[f64; 3]) -> [i64; 3] {
    [p[0], p[1], p[2]].map(|x| (x * 1e6) as i64)
}

fn radial_distance(p: Vec3, origin: Vec3, axis: Vec3) -> f64 {
    let d = p - origin;
    (d - axis * d.dot(axis)).len()
}

fn cylinder_leans_in(mesh: &solidcraft_geom::Mesh, face: usize, origin: Vec3, axis: Vec3, centroid: Vec3) -> Option<bool> {
    let mut n = Vec3::ZERO;
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        if *f as usize != face {
            continue;
        }
        if let Some([a, b, c]) = mesh.tri(t) {
            n += (b - a).cross(c - b);
        }
    }
    let len = n.dot(n).sqrt();
    if !(len > 1e-9) {
        return None;
    }
    let d = centroid - origin;
    let radial = d - axis * d.dot(axis);
    Some(n.dot(radial) < 0.0)
}

fn commands(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(Value::Array(crate::command_specs().iter().map(|c| serde_json::to_value(c.info(s)).unwrap_or_default()).collect()))
}

fn model_threads(s: &mut Session, _p: &Value) -> Result<Value> {
    let st = s.world_state();
    Ok(json!({ "threads": st.threads }))
}

fn physical_material(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "material.assign";
    let bodies = string_list(cmd, p, "bodies")?;
    let material = str_(p, "material").ok_or_else(|| bad(cmd, "`material` must be a material name"))?;
    let canonical = s.doc.material_name(material).ok_or_else(|| bad(cmd, format!("unknown material `{material}` (see material.list)")))?;
    let st = s.model.state();
    if bodies.is_empty() || bodies.iter().any(|b| st.body(b).is_none()) {
        return Err(bad(cmd, "`bodies` must list existing bodies"));
    }
    for b in &bodies {
        if canonical == solidcraft_doc::DEFAULT_MATERIAL {
            s.doc_mut().materials.remove(b);
        } else {
            s.doc_mut().materials.insert(b.clone(), canonical.clone());
        }
    }
    Ok(json!({"bodies": bodies, "material": canonical}))
}

/// A colour parameter: `"#rrggbb"` or `[r, g, b]` (0–255).
pub(super) fn color_param(v: &Value) -> Option<[u8; 3]> {
    if let Some(t) = v.as_str() {
        let h = t.trim().trim_start_matches('#');
        if h.len() != 6 || !h.is_ascii() {
            return None;
        }
        let byte = |i: usize| h.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok());
        return Some([byte(0)?, byte(2)?, byte(4)?]);
    }
    let a = v.as_array().filter(|a| a.len() == 3)?;
    let c = |i: usize| a.get(i).and_then(Value::as_f64).filter(|x| x.is_finite() && (0.0..=255.0).contains(x)).map(|x| x.round() as u8);
    Some([c(0)?, c(1)?, c(2)?])
}

pub(super) fn appearance(s: &mut Session, p: &Value) -> Result<Value> {
    use solidcraft_doc::appearance::FaceLook;
    let cmd = "appearance.assign";
    let look = appearance_look(cmd, p)?;
    let bodies = string_list(cmd, p, "bodies")?;
    let st = s.model.state();
    if bodies.iter().any(|b| st.body(b).is_none()) {
        return Err(bad(cmd, "`bodies` must list existing bodies"));
    }
    // Faces: a point on each, on the named body or the nearest one.
    let mut faces: Vec<(String, solidcraft_geom::Vec3)> = Vec::new();
    if let Some(list) = p.get("faces") {
        let list = list.as_array().filter(|a| a.len() <= 10_000).ok_or_else(|| bad(cmd, "`faces` must list points [x, y, z]"))?;
        for v in list {
            let pt = crate::params::vec3(v)
                .or_else(|| v.get("point").and_then(crate::params::vec3))
                .ok_or_else(|| bad(cmd, "`faces` must list points [x, y, z]"))?;
            let on = |b: &solidcraft_doc::ModelBody| solidcraft_doc::appearance::face_index_at(b, pt).is_some();
            let body = match v.get("body").and_then(Value::as_str).or_else(|| str_(p, "body")) {
                Some(n) => st.body(n).filter(|b| on(b)).map(|b| b.name.clone()),
                None => st.bodies.iter().filter(|b| on(b)).min_by(|a, b| dist_to(a, pt).total_cmp(&dist_to(b, pt))).map(|b| b.name.clone()),
            };
            faces.push((body.ok_or_else(|| bad(cmd, format!("no face at {pt:?}")))?, pt));
        }
    }
    let mut comps: Vec<u64> = Vec::new();
    for c in string_list(cmd, p, "components")? {
        let id = s.component_or_placement(&c).ok_or_else(|| bad(cmd, format!("no component `{c}`")))?;
        comps.push(id);
    }
    if bodies.is_empty() && faces.is_empty() && comps.is_empty() {
        return Err(bad(cmd, "give `bodies`, `faces` or `components`"));
    }
    // Persistent names of the picked faces.
    let st_names: Vec<(String, solidcraft_geom::Vec3, Option<String>)> = faces
        .iter()
        .map(|(b, pt)| {
            let n = st.body(b).and_then(|mb| {
                solidcraft_doc::appearance::face_index_at(mb, *pt).and_then(|i| solidcraft_doc::naming::face_names(mb).get(i).cloned())
            });
            (b.clone(), *pt, n)
        })
        .collect();
    let a = &mut s.doc_mut().appearances;
    for b in &bodies {
        match &look {
            Some(l) => a.bodies.insert(b.clone(), l.clone()),
            None => a.bodies.remove(b),
        };
    }
    for (body, pt) in &faces {
        // One entry per face: a new one replaces any at the same spot.
        a.faces.retain(|f| !(f.body == *body && f.point.dist(*pt) < 1e-9));
        if let Some(l) = &look {
            if a.faces.len() >= 100_000 {
                return Err(bad(cmd, "too many face appearances"));
            }
            let name = st_names.iter().find(|(b, p, _)| b == body && p.dist(*pt) < 1e-12).and_then(|(_, _, n)| n.clone());
            a.faces.push(FaceLook { body: body.clone(), point: *pt, look: l.clone(), name, strict_name: false, certificate: None });
        }
    }
    for c in &comps {
        match &look {
            Some(l) => a.components.insert(*c, l.clone()),
            None => a.components.remove(c),
        };
    }
    // Clearing a face: drop every entry on that face, not just at the same point.
    if look.is_none() && !faces.is_empty() {
        let st = s.model.state();
        let doc = s.doc_mut();
        let picked: Vec<(String, Option<usize>, Option<String>)> = faces
            .iter()
            .map(|(b, pt)| {
                (
                    b.clone(),
                    st.body(b).and_then(|mb| solidcraft_doc::appearance::face_index_at(mb, *pt)),
                    st_names.iter().find(|(name, p, _)| name == b && p.dist(*pt) < 1e-12).and_then(|(_, _, name)| name.clone()),
                )
            })
            .collect();
        doc.appearances.faces.retain(|f| {
            let idx = st.body(&f.body).and_then(|mb| solidcraft_doc::appearance::face_index_at(mb, f.point));
            !picked.iter().any(|(b, i, name)| {
                *b == f.body
                    && (f
                        .name
                        .as_ref()
                        .zip(name.as_ref())
                        .is_some_and(|(a, b)| solidcraft_doc::naming::strip_piece(a) == solidcraft_doc::naming::strip_piece(b))
                        || (!f.strict_name && i.is_some() && *i == idx))
            })
        });
    }
    Ok(json!({
        "bodies": bodies,
        "faces": faces.iter().map(|(b, p)| json!({"body": b, "point": p})).collect::<Vec<_>>(),
        "components": comps,
        "appearance": look.as_ref().map(look_json),
    }))
}

/// Shared look parser for assignment and atomic replacement.
pub(super) fn appearance_look(cmd: &str, p: &Value) -> Result<Option<solidcraft_doc::appearance::Look>> {
    use solidcraft_doc::appearance::{Look, library};
    // A saved label is data, not a request to resolve a recipe. Keep the explicit
    // appearance selector separate so an owned look may have any recipe's ID as its name.
    let copied_name = match p.get("name") {
        None | Some(Value::Null) => None,
        Some(Value::String(name)) if !name.is_empty() && name.chars().count() <= 128 && !name.chars().any(char::is_control) => Some(name),
        _ => return Err(bad(cmd, "`name` must be a nonempty label of at most 128 characters without controls")),
    };
    if copied_name.is_some() && (p.get("color").is_none_or(Value::is_null) || p.get("appearance").is_some_and(|value| !value.is_null())) {
        return Err(bad(cmd, "`name` requires an explicit color and cannot be combined with `appearance`"));
    }
    // Both the stable ID and a registered display name explicitly select a recipe.
    // Copied labels use `name`, so color overrides cannot bypass unsupported optics.
    let preset = str_(p, "appearance").and_then(solidcraft_doc::appearance_pack::find);
    let mut look: Option<Look> = if let Some(preset) = preset {
        // Refuse unsupported optics before any color/PBR/opacity override.
        let mut look = preset.opaque_look().map_err(|e| bad(cmd, e.to_string()))?;
        if let Some(color) = p.get("color").filter(|v| !v.is_null()) {
            look.color = color_param(color).ok_or_else(|| bad(cmd, "invalid preset color override"))?;
        }
        if let Some(opacity) = p.get("opacity").and_then(Value::as_f64).filter(|x| x.is_finite()) {
            look.opacity = opacity.clamp(0.0, 1.0);
        }
        Some(look)
    } else {
        match (str_(p, "appearance"), p.get("color")) {
            // A colour with a name: that colour, called that (a name not in the library is fine).
            (Some(n), Some(v)) if !v.is_null() => {
                let c = color_param(v).ok_or_else(|| bad(cmd, "`color` must be \"#rrggbb\" or [r, g, b] with values 0–255"))?;
                let o = p.get("opacity").and_then(Value::as_f64).filter(|x| x.is_finite()).unwrap_or(1.0);
                let mut l = Look::custom(c, o);
                l.name = n.chars().take(128).collect();
                Some(l)
            }
            (Some(n), _) => {
                let mut l = if let Some(preset) = solidcraft_doc::appearance_pack::find(n) {
                    preset.opaque_look().map_err(|e| bad(cmd, e.to_string()))?
                } else {
                    library(n).ok_or_else(|| bad(cmd, format!("no appearance `{n}` in the library (appearance.library)")))?
                };
                if let Some(o) = p.get("opacity").and_then(Value::as_f64).filter(|x| x.is_finite()) {
                    l.opacity = o.clamp(0.0, 1.0);
                }
                Some(l)
            }
            (None, None | Some(Value::Null)) => None,
            (None, Some(v)) => {
                let c = color_param(v).ok_or_else(|| bad(cmd, "`color` must be \"#rrggbb\" or [r, g, b] with values 0–255"))?;
                let o = match p.get("opacity") {
                    Some(o) => o.as_f64().filter(|x| x.is_finite() && (0.0..=1.0).contains(x)).ok_or_else(|| bad(cmd, "`opacity` must be 0…1"))?,
                    None => 1.0,
                };
                Some(Look::custom(c, o))
            }
        }
    };
    if let Some(l) = &mut look {
        if let Some(name) = copied_name {
            l.name = name.clone();
        }
        attach_pbr(cmd, p, l)?;
    } else if p.get("pbr").is_some_and(|v| !v.is_null()) {
        return Err(bad(cmd, "`pbr` requires a color or appearance"));
    }
    Ok(look)
}

fn dist_to(b: &solidcraft_doc::ModelBody, p: solidcraft_geom::Vec3) -> f64 {
    let bb = b.mesh().bounds();
    let c = solidcraft_geom::Vec3::new(p.x.clamp(bb.min.x, bb.max.x), p.y.clamp(bb.min.y, bb.max.y), p.z.clamp(bb.min.z, bb.max.z));
    c.dist(p)
}

pub(super) fn look_json(l: &solidcraft_doc::appearance::Look) -> Value {
    let mut value = json!({"name": l.name, "color": l.hex(), "opacity": l.opacity});
    if let Some(pbr) = l.pbr {
        value["pbr"] = json!(pbr);
    }
    value
}

#[cfg(test)]
#[path = "pbr_appearance_tests.rs"]
mod pbr_tests;

pub(super) fn attach_pbr(cmd: &str, params: &Value, look: &mut solidcraft_doc::appearance::Look) -> Result<()> {
    if let Some(value) = params.get("pbr") {
        look.pbr = if value.is_null() {
            None
        } else {
            let object = value
                .as_object()
                .filter(|o| o.len() == 3 && o.keys().all(|k| matches!(k.as_str(), "metallic" | "roughness" | "dielectric_ior")))
                .ok_or_else(|| bad(cmd, "`pbr` requires exactly metallic, roughness and dielectric_ior"))?;
            let number = |key: &str| object.get(key).and_then(Value::as_f64).ok_or_else(|| bad(cmd, format!("`pbr.{key}` must be a finite number")));
            Some(
                solidcraft_doc::pbr_appearance::PbrParameters::new(number("metallic")?, number("roughness")?, number("dielectric_ior")?)
                    .map_err(|e| bad(cmd, e.to_string()))?,
            )
        };
    }
    // Validate the final look, including a named preset's inherited response.
    // Both appearance assignments and physical-material defaults use this path.
    if look.pbr.is_some() {
        if let Some(opacity) = params.get("opacity")
            && opacity.as_f64().filter(|v| v.is_finite() && *v == 1.0).is_none()
        {
            return Err(bad(cmd, "opaque PBR requires numeric opacity 1"));
        }
        if look.opacity != 1.0 {
            return Err(bad(cmd, "opaque PBR requires opacity 1; refraction/transmission is not supported"));
        }
    }
    Ok(())
}

fn appearance_library(_s: &mut Session, _p: &Value) -> Result<Value> {
    let mut list: Vec<Value> = solidcraft_doc::appearance::LIBRARY
        .iter()
        .map(|(n, c, o)| {
            let category = n.split(" - ").next().unwrap_or(n);
            json!({"name": n, "category": category, "color": format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]), "opacity": o})
        })
        .collect();
    list.extend(solidcraft_doc::appearance::OPAQUE_FINISHES.iter().filter_map(|(name, ..)| {
        let look = solidcraft_doc::appearance::library(name)?;
        let mut value = look_json(&look);
        value["category"] = json!(name.split(" - ").next().unwrap_or(name));
        Some(value)
    }));
    for preset in solidcraft_doc::appearance_pack::STARTER_PACK {
        let c = preset.preview_color();
        let pbr = preset.opaque_look().ok().and_then(|look| look.pbr);
        list.push(json!({"id":preset.id,"name":preset.name,"category":preset.category,"revision":preset.revision,
            "color":format!("#{:02x}{:02x}{:02x}",c[0],c[1],c[2]),"opacity":1.0,"pbr":pbr,
            "available":preset.is_opaque(),"requirements":preset.requirements,"provenance":preset.provenance,
            "recipe":preset.recipe,"preview_note":preset.preview_note,"future_texture_recipe":preset.future_texture_recipe}));
    }
    Ok(json!({"appearances": list}))
}

fn appearance_list(s: &mut Session, p: &Value) -> Result<Value> {
    let a = &s.doc.appearances;
    // "In this design": each look used, with where.
    let mut used: Vec<(solidcraft_doc::appearance::Look, Vec<Value>, Vec<Value>, Vec<Value>)> = Vec::new();
    fn slot(used: &mut Vec<(solidcraft_doc::appearance::Look, Vec<Value>, Vec<Value>, Vec<Value>)>, l: &solidcraft_doc::appearance::Look) -> usize {
        match used.iter().position(|(u, ..)| u == l) {
            Some(i) => i,
            None => {
                used.push((l.clone(), Vec::new(), Vec::new(), Vec::new()));
                used.len() - 1
            }
        }
    }
    for (b, l) in &a.bodies {
        let i = slot(&mut used, l);
        if let Some(u) = used.get_mut(i) {
            u.1.push(json!(b));
        }
    }
    for f in &a.faces {
        let i = slot(&mut used, &f.look);
        if let Some(u) = used.get_mut(i) {
            u.2.push(json!({"body": f.body, "point": f.point}));
        }
    }
    for (c, l) in &a.components {
        let i = slot(&mut used, l);
        if let Some(u) = used.get_mut(i) {
            u.3.push(json!(c));
        }
    }
    let in_design: Vec<Value> = used
        .iter()
        .map(|(l, b, f, c)| {
            let mut value = look_json(l);
            value["bodies"] = json!(b);
            value["faces"] = json!(f);
            value["components"] = json!(c);
            value
        })
        .collect();
    let mut out = json!({
        "in_design": in_design,
        "bodies": a.bodies.iter().map(|(b, l)| (b.clone(), look_json(l))).collect::<serde_json::Map<_, _>>(),
        "faces": a.faces.iter().map(|f| json!({"body": f.body, "point": f.point, "appearance": look_json(&f.look)})).collect::<Vec<_>>(),
        "components": a.components.iter().map(|(c, l)| json!({"component": c, "appearance": look_json(l)})).collect::<Vec<_>>(),
    });
    if let Some(name) = str_(p, "body") {
        let st = s.model.state();
        // A placed copy's name (`KB (K:2)`) means the body it shows, as appearance.assign takes
        // it (#9740): the reply names that body.
        let own = s.own_body_name(name);
        let b = own.as_deref().and_then(|n| st.body(n)).ok_or_else(|| bad("appearance.list", format!("no body `{name}`")))?;
        out["body"] = json!({
            "name": b.name,
            "appearance": s.doc.model_body_look(b).map(|l| look_json(&l)),
            "color": s.doc.body_color(b),
            "faces": s.doc.face_colors(b).iter().map(|(i, l)| json!({"face": i, "appearance": look_json(l)})).collect::<Vec<_>>(),
        });
    }
    Ok(out)
}

fn material_list(s: &mut Session, _p: &Value) -> Result<Value> {
    let built_in = solidcraft_doc::MATERIALS.iter().map(|(n, d)| json!({"name": n, "density_g_cm3": d}));
    let user = s
        .doc
        .user_materials
        .iter()
        .map(|m| {
            let mut out = super::materials::material_json(m)?;
            out["appearance"] = json!(s.doc.material_look(&m.name).as_ref().map(look_json));
            Ok(out)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"materials": built_in.chain(user).collect::<Vec<_>>()}))
}

fn colored_components(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("enabled").is_some_and(|v| !v.is_boolean()) {
        return Err(bad("inspect.colored_components", "enabled must be a boolean"));
    }
    let enabled = bool_(p, "enabled").unwrap_or(s.colored_components.is_none());
    if enabled && s.colored_components.is_none() {
        s.colored_components = Some(());
    } else if !enabled {
        s.colored_components = None;
    }
    Ok(json!({"enabled": enabled}))
}

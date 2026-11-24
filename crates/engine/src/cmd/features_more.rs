//! Rib, Web, Emboss, Dome, Replace Face, Align and Remove.

use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_doc::expr::Kind;
use solidcraft_doc::{AlignFrame, FeatureKind};
use solidcraft_doc::{BodyName, Expr, SketchId};
use solidcraft_doc::{Dir, DomeShape, FacePick, Position};
use solidcraft_geom::prelude::*;

use super::CommandSpec;
use super::features::{
    add_face_feature, add_feature, axis_expr_param, check_expr, feature_sketch, length_operands, plane_param, point_param, profiles,
    typed_length_unit,
};
use crate::params::{bad, bool_, expr, expr_problem, req_expr, str_, string_list, vec3};
use crate::schema::align::AlignParams;
use crate::schema::features::FeatureResult;
use crate::schema::features_more::{BoundaryFillParams, CoilParams, DomeParams, EmbossParams, RemoveParams, ReplaceFaceParams, RibParams};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

/// Rib and Web take the same keys.
const RIB_CONTRACT: TypedContract<RibParams> = TypedContract::new();
const ALIGN_CONTRACT: TypedContract<AlignParams> = TypedContract::new();
const REMOVE_CONTRACT: TypedContract<RemoveParams> = TypedContract::new();
const BOUNDARY_FILL_CONTRACT: TypedContract<BoundaryFillParams> = TypedContract::new();
const COIL_CONTRACT: TypedContract<CoilParams> = TypedContract::new();
const EMBOSS_CONTRACT: TypedContract<EmbossParams> = TypedContract::new();
const DOME_CONTRACT: TypedContract<DomeParams> = TypedContract::new();
const REPLACE_FACE_CONTRACT: TypedContract<ReplaceFaceParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("solid.boundary_fill", "Boundary Fill", crate::checked!("solid.boundary_fill", boundary_fill)).begins_edit()
        .at("SOLID", "CREATE")
        .icon("boundary_fill")
        .contract(&BOUNDARY_FILL_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params(
            "tools: [bodies]; cells: [[x,y,z] a point inside each cell to fill (a cell: inside some tools, outside the rest)]; operation?: new|join|cut|intersect; remove_tools?: bool",
        ),
    CommandSpec::new("solid.coil", "Coil", crate::checked!("solid.coil", coil)).begins_edit().at("SOLID", "CREATE").icon("coil").contract(&COIL_CONTRACT).output(output_schema_for::<FeatureResult>).params(
        "diameter; two of revolutions (or turns), height, pitch; section_size; section?: circular|square; section_position?: inside|center|outside; \
         base?: [x,y,z] (numbers in mm or length expressions; default: the origin); axis?: X|Y|Z|[x,y,z]|revolve axis reference (default Z; use {type: sketch_axis, axis: x|y} for the profile sketch's axis; picked references supply their base and stay linked); start_angle?; clockwise?: bool; reversed?: bool (grow against the axis; the hand set by clockwise is kept); \
         end_cut?: axial|square (the wire's ends in a plane through the axis, the default, or square to the wire); \
         sketch? (a sketch; its plane is the section plane, so its placement sets the radius and start phase), profiles? (default: all; one closed profile without holes); \
         operation?, targets?, name?, body_name?",
    ),
    CommandSpec::new("solid.rib", "Rib", crate::checked!("solid.rib", rib)).begins_edit()
        .at("SOLID", "CREATE")
        .icon("rib")
        .contract(&RIB_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("sketch, curves: [open curve ids, in order]; thickness (across the sketch plane, or across the curves when normal); depth? (default: to the next face); flip?: bool (fill to the other side); direction?: in_plane|normal (grow across the sketch plane, the default, or along its normal); extend?: bool (run the curves' open ends on to the body; default false)"),
    CommandSpec::new("solid.web", "Web", crate::checked!("solid.web", web)).begins_edit()
        .at("SOLID", "CREATE")
        .icon("web")
        .contract(&RIB_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("sketch, curves: [open curve ids] (one wall each); thickness; depth? (default: to the next face); flip?: bool; direction?: in_plane|normal; extend?: bool (default false)"),
    CommandSpec::new("solid.emboss", "Emboss", crate::checked!("solid.emboss", emboss)).begins_edit()
        .at("SOLID", "CREATE")
        .icon("emboss")
        .contract(&EMBOSS_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("sketch, profiles?; depth; mode?: emboss|deboss; targets?; face?: world point [x,y,z] on the face to emboss: planar, a cylinder wall (on a partial outside wall, a profile running past the wall's edge carries on onto the face there when that face is a rectangle tangent to the wall; the profile must lie within the wall and those faces) or another curved face (inferred when exactly one supported tangent cylinder matches the selected profiles); offset_x?, offset_y?: length expr (a shift of the profiles along the sketch's x and y before they are laid on the face; the sketch is not changed); angle?: angle expr (a turn of the profiles, counter-clockwise in the sketch, about the middle of their bounds; the offsets are not turned with them)"),
    CommandSpec::new("solid.dome", "Dome", crate::checked!("solid.dome", dome)).begins_edit()
        .at("SOLID", "CREATE")
        .icon("dome")
        .contract(&DOME_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .from_selection(super::model_selection::body_faces)
        .params(
            "face: [x,y,z] on a planar face, or a face name from model.faces; height: expr (along the face's outward normal); shape?: elliptical|spherical (default elliptical: a quarter ellipse, on a circle a half ellipsoid; spherical: the sphere through the rim and the apex, on a circle); reverse?: bool (cut into the body: not supported yet, refused); body? (the face's body when the name alone does not say)",
        ),
    CommandSpec::new("solid.replace_face", "Replace Face", crate::checked!("solid.replace_face", replace_face)).begins_edit()
        .contract(&REPLACE_FACE_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .at("SOLID", "MODIFY")
        .icon("replace_face")
        .params("faces: [[x,y,z] points on planar faces, or face names from model.faces]; target: plane (XY|XZ|YZ|construction plane|{origin, normal}) | target_face: [x,y,z] on a parallel planar face; body?"),
    CommandSpec::new("solid.align", "Align", crate::checked!("solid.align", align)).begins_edit()
        .at("SOLID", "MODIFY")
        .icon("align")
        .contract(&ALIGN_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("bodies: [names]; from: [x,y,z] | \"origin\" | {snap: vertex|edge_mid|face_center|circle_center, at: [x,y,z]} | from_face: [x,y,z] | from_plane: XY|XZ|YZ|plane name or id|{origin, normal} | from_frame: a joint origin as joint.create takes one, or {sketch, point|curve}; to, to_face, to_plane, to_frame: the same (a plane or frame is found again when the design changes: the bodies follow it); a face or face_center faces along its normal, a circle_center along its axis out of its body, a plane or frame along its normal or z, a point, vertex or edge_mid no way; flip?: bool (two facing sides meet, or point the same way; one: flip turns the bodies half round about an axis square to its way; none: flip refused); axes?: [x, y, z] (two points only: the world axes the move keeps); angle?: expr (a turn about the target's direction)"),
    CommandSpec::new("solid.remove", "Remove", crate::checked!("solid.remove", remove)).begins_edit()
        .at("SOLID", "MODIFY")
        .icon("delete")
        .contract(&REMOVE_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("bodies: [names] (removed from here on in the timeline)"),
];

fn coil(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.coil";
    let given = |k: &str| p.get(k).is_some_and(|v| !v.is_null());
    let profile_mode = given("sketch");
    if profile_mode {
        if let Some(k) = ["diameter", "section_size", "section", "section_position", "start_angle", "end_cut"].into_iter().find(|k| given(k)) {
            return Err(bad(cmd, format!("`{k}` does not apply with `sketch`: the section comes from the sketch")));
        }
    } else if given("profiles") {
        return Err(bad(cmd, "`profiles` needs `sketch`"));
    }
    let sketch = if profile_mode { Some(solidcraft_doc::SketchId(super::features::sketch_id(s, p.get("sketch"), cmd, "sketch")?)) } else { None };
    let profiles = if profile_mode { super::features::profiles(p, cmd)? } else { solidcraft_doc::ProfileSel::All };
    let base = point_param(s, p, "base", cmd)?.unwrap_or_else(|| solidcraft_doc::point_expr(Vec3::ZERO));
    let reference = p.get("axis").is_some_and(|v| {
        super::features::axis_dir_form(v).is_err()
            && (v.as_str().is_some_and(|name| !name.trim().is_empty())
                || v.is_object() && ["origin", "dir", "curve", "edge", "face", "type"].iter().any(|key| v.get(*key).is_some()))
    });
    let (axis, axis_ref) = if let Some(geo) = coil_placed_edge(s, p.get("axis"), cmd)? {
        (solidcraft_doc::dir_expr(Vec3::Z), Some(solidcraft_doc::AxisRef::Model { geo }))
    } else if reference {
        (solidcraft_doc::dir_expr(Vec3::Z), Some(super::features::axis_param(s, sketch.map_or(0, |id| id.0), p, cmd)?))
    } else {
        (axis_expr_param(s, p, cmd)?.0, None)
    };
    let diameter = if profile_mode {
        None
    } else {
        let d = req_expr(cmd, p, "diameter")?;
        check_expr(s, &d, Kind::Length, cmd, "diameter")?;
        Some(d)
    };
    let turns = expr(p, "revolutions").or_else(|| expr(p, "turns"));
    let (height, pitch) = (expr(p, "height"), expr(p, "pitch"));
    // The pitch or the revolutions from the height keep the units the lengths were given in.
    let unit = typed_length_unit(s);
    let (turns, pitch) = match (turns, height, pitch) {
        (Some(n), _, Some(pt)) => (n, pt),
        (Some(n), Some(h), None) => {
            let [h] = length_operands(s, [(&h, &unit)]);
            (n.clone(), format!("({h}) / ({n})").into())
        }
        (None, Some(h), Some(pt)) => {
            let [h, by] = length_operands(s, [(&h, &unit), (&pt, &unit)]);
            (format!("({h}) / ({by})").into(), pt)
        }
        _ => return Err(bad(cmd, "give two of `revolutions`, `height` and `pitch`")),
    };
    check_expr(s, &turns, Kind::Unitless, cmd, "revolutions")?;
    check_expr(s, &pitch, Kind::Length, cmd, "pitch")?;
    let section_size = if profile_mode {
        None
    } else {
        let size = req_expr(cmd, p, "section_size")?;
        check_expr(s, &size, Kind::Length, cmd, "section_size")?;
        Some(size)
    };
    let section = match str_(p, "section").map(str::to_ascii_lowercase).as_deref() {
        None | Some("circular" | "circle") => solidcraft_doc::CoilSection::Circular,
        Some("square") => solidcraft_doc::CoilSection::Square,
        Some(o) => return Err(bad(cmd, format!("unknown section `{o}` (circular or square)"))),
    };
    let position = match str_(p, "section_position").map(str::to_ascii_lowercase).as_deref() {
        None | Some("center" | "on" | "on_center") => 0,
        Some("inside") => -1,
        Some("outside") => 1,
        Some(o) => return Err(bad(cmd, format!("unknown section position `{o}` (inside, center or outside)"))),
    };
    let start_angle = expr(p, "start_angle");
    if let Some(a) = &start_angle {
        check_expr(s, a, Kind::Angle, cmd, "start_angle")?;
    }
    let clockwise = bool_(p, "clockwise").unwrap_or(false);
    let end_cut = match str_(p, "end_cut").map(str::to_ascii_lowercase).as_deref() {
        None | Some("axial") => solidcraft_doc::CoilEndCut::Axial,
        Some("square") => solidcraft_doc::CoilEndCut::Square,
        Some(o) => return Err(bad(cmd, format!("unknown end cut `{o}` (axial or square)"))),
    };
    let operation = super::features::operation(p, cmd)?;
    add_feature(
        s,
        p,
        FeatureKind::Coil {
            base,
            axis,
            axis_ref,
            reversed: super::reversed_param(p, "reversed", cmd)?,
            sketch,
            profiles,
            diameter,
            pitch,
            turns,
            section_size,
            section,
            position,
            start_angle,
            clockwise,
            end_cut,
            operation,
            targets: string_list(cmd, p, "targets")?.into_iter().map(BodyName).collect(),
        },
    )
}

fn curves_param(p: &Value, cmd: &str) -> Result<Vec<String>> {
    let c = string_list(cmd, p, "curves")?;
    if c.is_empty() || c.len() > 1000 {
        return Err(bad(cmd, "`curves` must list 1…1000 sketch curve ids"));
    }
    Ok(c)
}

fn rib_like(s: &mut Session, p: &Value, cmd: &str, web: bool) -> Result<Value> {
    let sketch = feature_sketch(s, p, cmd)?;
    let curves = curves_param(p, cmd)?;
    {
        let st = s.model.state();
        let ss = st.sketch(sketch).ok_or_else(|| bad(cmd, "the sketch is not evaluated"))?;
        if let Some(c) = curves.iter().find(|c| ss.sketch.curve_index(c).is_none()) {
            return Err(bad(cmd, format!("no curve `{c}` in the sketch")));
        }
    }
    let thickness = req_expr(cmd, p, "thickness")?;
    check_expr(s, &thickness, Kind::Length, cmd, "thickness")?;
    let depth = expr(p, "depth");
    if let Some(d) = &depth {
        check_expr(s, d, Kind::Length, cmd, "depth")?;
    }
    let flip = bool_(p, "flip").unwrap_or(false);
    let normal = match str_(p, "direction").map(str::to_ascii_lowercase).as_deref() {
        None | Some("in_plane" | "plane") => false,
        Some("normal") => true,
        Some(o) => return Err(bad(cmd, format!("unknown direction `{o}` (in_plane or normal)"))),
    };
    // Opt-in (#13019): without `extend` a rib ends where its curves end, as before.
    let extend = bool_(p, "extend").unwrap_or(false);
    add_feature(s, p, FeatureKind::Rib { sketch: SketchId(sketch), curves, thickness, depth, flip, web, normal, extend })
}

fn rib(s: &mut Session, p: &Value) -> Result<Value> {
    rib_like(s, p, "solid.rib", false)
}

fn web(s: &mut Session, p: &Value) -> Result<Value> {
    rib_like(s, p, "solid.web", true)
}

/// `solid.dome`: a dome on one planar face of a body (#19532). The face is a point on it or a
/// name, found on the body as `solid.shell`'s faces are.
fn dome(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.dome";
    let raw = p.get("face").ok_or_else(|| bad(cmd, "`face` must be a point [x, y, z] on the face or a face name"))?;
    let face = super::face::face_ref(s, raw, str_(p, "body"), cmd)?;
    let picked = super::face::faces_body(s, p, std::slice::from_ref(raw), &[face]);
    let height = req_expr(cmd, p, "height")?;
    check_expr(s, &height, Kind::Length, cmd, "height")?;
    let shape = match str_(p, "shape").map(str::to_ascii_lowercase).as_deref() {
        None | Some("elliptical") => DomeShape::Elliptical,
        Some("spherical") => DomeShape::Spherical,
        Some(m) => return Err(bad(cmd, format!("unknown shape `{m}` (elliptical or spherical)"))),
    };
    let reverse = bool_(p, "reverse").unwrap_or(false);
    add_face_feature(&picked, s, p, FeatureKind::Dome { face: FacePick(face), height, shape, reverse, body: picked.body.clone().map(BodyName) })
}

fn emboss(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.emboss";
    let sketch = feature_sketch(s, p, cmd)?;
    let profiles = profiles(p, cmd)?;
    let depth = req_expr(cmd, p, "depth")?;
    check_expr(s, &depth, Kind::Length, cmd, "depth")?;
    // #20386: where the profiles go, apart from where the sketch has them.
    let placement = EmbossPlacement {
        offset_x: placement_expr(s, p, "offset_x", Kind::Length)?,
        offset_y: placement_expr(s, p, "offset_y", Kind::Length)?,
        angle: placement_expr(s, p, "angle", Kind::Angle)?,
    };
    let deboss = match str_(p, "mode").map(str::to_ascii_lowercase).as_deref() {
        None | Some("emboss") => false,
        Some("deboss") => true,
        Some(m) => return Err(bad(cmd, format!("unknown mode `{m}` (emboss or deboss)"))),
    };
    let mut targets = string_list(cmd, p, "targets")?;
    let face = if let Some(value) = p.get("face") {
        let local = vec3(value).ok_or_else(|| bad(cmd, "`face` must be a finite point [x,y,z]"))?;
        let at = solidcraft_doc::apply_point(&s.active_component_frame(), local);
        let state = s.world_state()?;
        let mut candidates = Vec::new();
        // The nearest face off the point: (distance, body), for the error when none holds it.
        let mut nearest: Option<(f64, &str)> = None;
        for body in &state.bodies {
            let definition = body.definition_name();
            if !targets.is_empty() && !targets.iter().any(|n| n == definition || n == &body.name) {
                continue;
            }
            let mesh = body.mesh();
            let closest = mesh
                .triangles
                .iter()
                .zip(&mesh.tri_face)
                .filter_map(|(tri, face)| {
                    let [a, b, c] = mesh.tri(tri)?;
                    let n = (b - a).cross(c - a).normalized()?;
                    let q = at - n * (at - a).dot(n);
                    let inside = [(a, b), (b, c), (c, a)].iter().all(|(u, v)| (*v - *u).cross(q - *u).dot(n) >= -1e-9);
                    inside.then_some(((at - a).dot(n).abs(), *face as usize))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            match closest {
                Some((distance, index)) if distance < (body.body.size() * 1e-3).max(1e-3) => candidates.push((body, index)),
                Some((distance, _)) if nearest.is_none_or(|(d, _)| distance < d) => nearest = Some((distance, body.name.as_str())),
                _ => {}
            }
        }
        // #392: say what is wrong with the point, not only that it picks no single face.
        let point = format!("[{}, {}, {}]", local.x, local.y, local.z);
        match (candidates.as_slice(), nearest) {
            ([], Some((d, body))) => {
                return Err(bad(
                    cmd,
                    format!("`face` {point} lies on no face: the nearest, on `{body}`, is {d:.3} mm away; give a point on the face"),
                ));
            }
            ([], None) => return Err(bad(cmd, format!("`face` {point} lies on no face of the target bodies"))),
            ([_], _) => {}
            (more, _) => {
                let names: Vec<&str> = more.iter().map(|(b, _)| b.name.as_str()).collect();
                return Err(bad(
                    cmd,
                    format!("`face` {point} lies on faces of {} bodies ({}): name one in `targets`", names.len(), names.join(", ")),
                ));
            }
        }
        let (selected, index) = candidates.first().copied().ok_or_else(|| bad(cmd, "no target face"))?;
        let name = selected.definition_name();
        let body = &selected.body;
        if targets.len() > 1 {
            return Err(bad(cmd, "an emboss on a selected face needs one target body"));
        }
        // Viewport picks lie on tessellated chords: certify their distance and store the point
        // on the exact surface for later replay.
        let at = solidcraft_kernel::emboss_anchor(body, index, at).map_err(|e| bad(cmd, e.to_string()))?;
        let face = emboss_selection(s, selected, at)?;
        check_emboss_positions(s, sketch, selected, &face)?;
        targets = vec![name.to_owned()];
        Some(face)
    } else {
        let inferred = infer_emboss_selection(s, sketch, &profiles, &placement, &depth, deboss, &targets)?;
        if let Some(face) = &inferred {
            targets = vec![face.body.0.clone()];
        }
        inferred
    };
    let EmbossPlacement { offset_x, offset_y, angle } = placement;
    add_feature(
        s,
        p,
        FeatureKind::Emboss {
            sketch: SketchId(sketch),
            profiles,
            depth,
            deboss,
            targets: targets.into_iter().map(BodyName).collect(),
            face,
            offset_x,
            offset_y,
            angle,
        },
    )
}

/// Where an Emboss puts its profiles, apart from where their sketch has them (#20386): a shift
/// along the sketch's x and y and a turn about the middle of their bounds, each left out or an
/// expression.
struct EmbossPlacement {
    offset_x: Option<Expr>,
    offset_y: Option<Expr>,
    angle: Option<Expr>,
}

/// An Emboss placement input: an expression of `kind`, or `None` when the call leaves it out or
/// gives `null`. A value given that isn't a number or an expression is refused rather than read
/// as left out.
fn placement_expr(s: &Session, p: &Value, key: &str, kind: Kind) -> Result<Option<Expr>> {
    let Some(given) = p.get(key).filter(|v| !v.is_null()) else { return Ok(None) };
    let e = expr(p, key).ok_or_else(|| bad("solid.emboss", format!("`{key}` {}", expr_problem(Some(given)))))?;
    check_expr(s, &e, kind, "solid.emboss", key)?;
    Ok(Some(e))
}

impl EmbossPlacement {
    /// `regions` as the feature will lay them on its face (`solidcraft_doc::emboss_placement`),
    /// with the values the inputs give now. A bare length reads in the design's unit, as it is
    /// saved.
    fn place(&self, s: &Session, regions: Vec<solidcore_record::Region2>) -> Result<Vec<solidcore_record::Region2>> {
        let cmd = "solid.emboss";
        let now = |e: &Option<Expr>, kind: Kind| match e {
            Some(e) => s.doc.eval(&s.doc.with_design_unit(e, kind), kind).map_err(|err| bad(cmd, err.to_string())),
            None => Ok(0.0),
        };
        let shift = solidcore_record::Vec2::new(now(&self.offset_x, Kind::Length)?, now(&self.offset_y, Kind::Length)?);
        solidcraft_doc::emboss_placement(regions, shift, now(&self.angle, Kind::Angle)?).map_err(|e| bad(cmd, e.to_string()))
    }
}

fn emboss_selection(s: &Session, selected: &solidcraft_doc::ModelBody, at: Vec3) -> Result<solidcraft_doc::EmbossFace> {
    let cmd = "solid.emboss";
    let name = selected.definition_name();
    let original = s.model.state();
    let owner = s.doc.model_body_component(selected);
    let paths: Vec<_> = s
        .doc
        .placements()
        .into_iter()
        .filter(|(comp, path, _)| *comp == owner && s.doc.world_body_name(&original, name, owner, path) == selected.name)
        .map(|(_, path, _)| path)
        .collect();
    if paths.len() != 1 {
        return Err(bad(cmd, "the selected body's placement is ambiguous"));
    }
    let path = paths.into_iter().next().ok_or_else(|| bad(cmd, "selected placement"))?;
    let author_path = s.active_occurrence_path.clone().unwrap_or_else(|| s.doc.first_path(s.active_component));
    if path.len() > 64 || author_path.len() > 64 {
        return Err(bad(cmd, "the selected placement is too deep"));
    }
    let inverse = solidcraft_doc::mat_inverse(&s.active_component_frame()).ok_or_else(|| bad(cmd, "invalid active frame"))?;
    Ok(solidcraft_doc::EmbossFace { body: name.into(), at: FacePick(solidcraft_doc::apply_point(&inverse, at)), path, author_path })
}

fn check_emboss_positions(s: &Session, sketch: u64, body: &solidcraft_doc::ModelBody, face: &solidcraft_doc::EmbossFace) -> Result<()> {
    let cmd = "solid.emboss";
    let owner = s.doc.model_body_component(body);
    let sketch_owner = s.doc.feature(sketch).map(|f| f.component).ok_or_else(|| bad(cmd, "profile sketch no longer exists"))?;
    let sketch_path = if sketch_owner == owner {
        face.path.clone()
    } else if sketch_owner == s.active_component {
        face.author_path.clone()
    } else {
        s.doc.first_path(sketch_owner)
    };
    if face.path.iter().chain(&face.author_path).chain(&sketch_path).any(|id| s.pending_moves.contains_key(id)) {
        return Err(bad(cmd, "capture pending source, author and profile positions with Capture Position before an emboss on a face"));
    }
    Ok(())
}

fn infer_emboss_selection(
    s: &Session,
    sketch: u64,
    profiles: &solidcraft_doc::ProfileSel,
    placement: &EmbossPlacement,
    depth: &str,
    deboss: bool,
    targets: &[String],
) -> Result<Option<solidcraft_doc::EmbossFace>> {
    let cmd = "solid.emboss";
    let original = s.model.state();
    let ss = original.sketch(sketch).ok_or_else(|| bad(cmd, "the sketch is not evaluated"))?;
    // The wall is the one the profiles lie on once the Emboss has placed them (#20386).
    let regions = placement.place(s, solidcraft_doc::select_profiles(ss, profiles).map_err(|e| bad(cmd, e.to_string()))?)?;
    let sketch_owner = s.doc.feature(sketch).map(|f| f.component).ok_or_else(|| bad(cmd, "profile sketch no longer exists"))?;
    let d = s.doc.eval(&s.doc.with_design_unit(depth, Kind::Length), Kind::Length).map_err(|e| bad(cmd, e.to_string()))?;
    let world = s.world_state()?;
    let placed = s.placed_document();
    let mut chosen = None;
    let mut faces = 0usize;
    for selected in &world.bodies {
        let name = selected.definition_name();
        if !targets.is_empty() && !targets.iter().any(|n| n == name || n == &selected.name) {
            continue;
        }
        let raw = original.body(name).ok_or_else(|| bad(cmd, "target definition no longer exists"))?;
        faces = faces.saturating_add(raw.body.face_count());
        if faces > 50_000 {
            return Err(bad(cmd, "cylinder target search exceeds face work limit"));
        }
        if !solidcraft_kernel::has_cylinder_emboss_wall(&raw.body).map_err(|e| bad(cmd, e.to_string()))? {
            continue;
        }
        let mut face = emboss_selection(s, selected, Vec3::ZERO)?;
        let body_frame = face.body_frame(&placed, raw).map_err(|e| bad(cmd, e.to_string()))?;
        let inverse = solidcraft_doc::mat_inverse(&body_frame).ok_or_else(|| bad(cmd, "invalid cylinder body frame"))?;
        let displayed = world.sketch(sketch).ok_or_else(|| bad(cmd, "the profile sketch is not displayed"))?;
        let plane = solidcore_record::Plane::new(
            solidcraft_doc::apply_point(&inverse, displayed.plane.origin),
            solidcraft_doc::apply_vector(&inverse, displayed.plane.x),
            solidcraft_doc::apply_vector(&inverse, displayed.plane.y),
        )
        .ok_or_else(|| bad(cmd, "invalid cylinder sketch frame"))?;
        if let Some((_, at)) =
            solidcraft_kernel::infer_cylinder_emboss(&raw.body, &plane, &regions, if deboss { -d } else { d }).map_err(|e| bad(cmd, e.to_string()))?
        {
            face.sketch_frame(&placed, s.active_component, sketch_owner, raw).map_err(|e| bad(cmd, e.to_string()))?;
            check_emboss_positions(s, sketch, selected, &face)?;
            let author_inverse = solidcraft_doc::mat_inverse(&s.active_component_frame()).ok_or_else(|| bad(cmd, "invalid author frame"))?;
            face.at = FacePick(solidcraft_doc::apply_point(&solidcraft_doc::mat_mul(&author_inverse, &body_frame), at));
            if chosen.is_some() {
                return Err(bad(cmd, "multiple tangent cylinders match the selected profiles; select a target face or body"));
            }
            chosen = Some(face);
        }
    }
    Ok(chosen)
}
fn face_points(s: &Session, p: &Value, k: &str, cmd: &str) -> Result<Vec<Vec3>> {
    let a = p.get(k).and_then(Value::as_array).ok_or_else(|| bad(cmd, format!("`{k}` must list points [x, y, z] or face names")))?;
    if a.is_empty() || a.len() > 1000 {
        return Err(bad(cmd, format!("`{k}` must list 1…1000 points")));
    }
    a.iter().map(|v| super::face::face_ref(s, v, str_(p, "body"), cmd)).collect()
}

fn replace_face(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.replace_face";
    let faces = face_points(s, p, "faces", cmd)?;
    let refs = p.get("faces").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    // The body first: a face flush with another body's is looked up on that body alone.
    let picked = super::face::faces_body(s, p, refs, &faces);
    for f in &faces {
        if super::face::face_normal_in(s, *f, picked.lookup()).is_none() {
            return Err(bad(cmd, "not supported yet: replacing a face that is not planar"));
        }
    }
    let target = match p.get("target_face").and_then(vec3) {
        Some(tp) => {
            let n = super::face::face_normal(s, tp).ok_or_else(|| bad(cmd, "the target face must be planar"))?;
            let plane = solidcore_record::Plane::from_normal(tp, n).ok_or_else(|| bad(cmd, "the target face's plane"))?;
            solidcraft_doc::PlaneRef::Custom { plane }
        }
        None => plane_param(s, p.get("target"), cmd)?,
    };
    let body = picked.body.clone().map(BodyName);
    super::features::add_face_feature(&picked, s, p, FeatureKind::ReplaceFace { faces: faces.into_iter().map(FacePick).collect(), target, body })
}

/// What `solid.align` says when `axes` is not three booleans, or when none is true.
pub(crate) const AXES_SHAPE: &str = "`axes` must be [x, y, z] booleans";
pub(crate) const AXES_NONE: &str = "`axes` must enable at least one of X, Y and Z";

/// `axes` as `solid.align` reads it: three booleans, one per world axis, at least one of them
/// true; the message is the handler's.
pub(crate) fn axes_mask(v: &Value) -> std::result::Result<[bool; 3], &'static str> {
    let a = v.as_array().filter(|a| a.len() == 3).ok_or(AXES_SHAPE)?;
    let mut mask = [false; 3];
    for (slot, v) in mask.iter_mut().zip(a) {
        *slot = v.as_bool().ok_or(AXES_SHAPE)?;
    }
    if !mask.iter().any(|on| *on) {
        return Err(AXES_NONE);
    }
    Ok(mask)
}

/// `axes`: three booleans, one per world axis, for a point-to-point Align.
/// `None` when the call does not restrict the move.
fn axes_param(p: &Value, cmd: &str) -> Result<Option<[bool; 3]>> {
    let Some(v) = p.get("axes").filter(|v| !v.is_null()) else { return Ok(None) };
    let mask = axes_mask(v).map_err(|why| bad(cmd, why))?;
    Ok((mask != [true; 3]).then_some(mask))
}

/// The kinds of snap point an Align side takes.
pub(crate) const SNAP_KINDS: [&str; 4] = ["vertex", "edge_mid", "face_center", "circle_center"];

/// An Align side given as a point (`from` or `to`), as the handler reads it.
pub(crate) enum SidePoint {
    /// The text `origin` (any case): the world origin.
    Origin,
    /// `{snap?, at}`: the nearest `snap` (vertex unless the object says another) to `at`.
    Snap { at: Vec3, kind: String },
    /// `[x, y, z]`.
    Point(Vec3),
}

/// The side `pk` (`from` or `to`) as a point, with the handler's message when it is none of the
/// forms; `fk` is the face key beside it (`from_face`).
pub(crate) fn side_point(v: Option<&Value>, pk: &str, fk: &str) -> std::result::Result<SidePoint, String> {
    // `[x, y, z]`, which reads no coordinate 1e9 mm or more out: that is said, not taken for no point.
    let point = |v: &Value| {
        let far = v.as_array().is_some_and(|a| a.len() == 3 && a.iter().any(|c| c.as_f64().is_some_and(|c| c.abs() >= 1e9)));
        if far { Err(format!("`{pk}` must lie within 1e9 mm of the origin")) } else { Ok(vec3(v)) }
    };
    // The origin point.
    if v.and_then(Value::as_str).is_some_and(|o| o.eq_ignore_ascii_case("origin")) {
        return Ok(SidePoint::Origin);
    }
    // A snap point: {snap: vertex|edge_mid|face_center|circle_center, at: [x,y,z]}.
    if let Some(o) = v.filter(|v| v.is_object()) {
        let at = o.get("at").map(point).transpose()?.flatten().ok_or_else(|| format!("`{pk}.at` must be [x, y, z]"))?;
        let kind = o.get("snap").and_then(Value::as_str).unwrap_or("vertex").to_string();
        if !SNAP_KINDS.contains(&kind.as_str()) {
            return Err(format!("`{pk}.snap` must be {}", SNAP_KINDS.join(", ")));
        }
        return Ok(SidePoint::Snap { at, kind });
    }
    v.map(point)
        .transpose()?
        .flatten()
        .map(SidePoint::Point)
        .ok_or_else(|| format!("give `{pk}`, `{fk}` or `{pk}_plane` ([x, y, z], \"origin\", or a plane)"))
}

fn align(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.align";
    let bodies = string_list(cmd, p, "bodies")?;
    if bodies.is_empty() {
        return Err(bad(cmd, "`bodies` must list the bodies to move"));
    }
    let mut seen = std::collections::BTreeSet::new();
    if let Some(b) = bodies.iter().find(|b| !seen.insert(b.as_str())) {
        return Err(bad(cmd, format!("`bodies` lists `{b}` twice")));
    }
    {
        let st = s.model.state();
        if let Some(b) = bodies.iter().find(|b| st.body(b).is_none()) {
            return Err(bad(cmd, format!("no body `{b}`")));
        }
    }
    // Picks and snaps are where the view shows them (the world); the feature moves the bodies
    // in their component's frame, so both ends go into the first moved body's frame. A body
    // picked in another placed component (aligning two imported parts) is found where it
    // is shown, not where its own frame has it.
    let frame = s
        .placed_document()
        .body_frames_with_source(&s.model.state())
        .into_iter()
        .find(|(world, own, ..)| *world == bodies[0] || *own == bodies[0])
        .map(|(.., m)| m);
    let inverse = frame.and_then(|m| solidcraft_doc::mat_inverse(&m)).unwrap_or(solidcraft_doc::IDENTITY);
    // A side is a plane (`from_plane`/`to_plane`: XY|XZ|YZ, a construction or offset plane, or
    // {origin, normal}), kept as a plane so the bodies follow it when it moves; or a point and, for
    // a face, its normal.
    let plane = |s: &Session, key: &str| -> Result<Option<solidcraft_doc::PlaneRef>> {
        p.get(key).map(|_| super::features::plane_param(s, p.get(key), cmd)).transpose()
    };
    let (from_plane, to_plane) = (plane(s, "from_plane")?, plane(s, "to_plane")?);
    // Or a frame (#6257), found again when the feature is evaluated.
    let (from_frame, to_frame) = (align_frame(s, p, "from", &bodies)?, align_frame(s, p, "to", &bodies)?);
    // A target on the bodies that move would move away with them: it must be found the same
    // without them.
    if let Some(fr @ AlignFrame::Origin { .. }) = &to_frame {
        let st = s.model.state();
        let mut rest = (*st).clone();
        rest.bodies.retain(|b| !bodies.contains(&b.name));
        let same = |(a, n): (Vec3, Vec3), (b, m): (Vec3, Vec3)| a.dist(b) <= 1e-9 * (1.0 + a.len()) && n.dot(m) > 1.0 - 1e-12;
        if !matches!((fr.resolve(&s.doc, &st), fr.resolve(&s.doc, &rest)), (Ok(a), Ok(b)) if same(a, b)) {
            return Err(bad(cmd, on_moving("to_frame")));
        }
    }
    // Worked out from the plane or the frame when the feature is evaluated.
    let held = |plane: &Option<solidcraft_doc::PlaneRef>, frame: &Option<AlignFrame>| plane.is_some() || frame.is_some();
    // The bodies that move, by the names picks find them under in the world.
    let moved: Vec<String> = {
        let frames = s.placed_document().body_frames_with_source(&s.model.state());
        let picked = frames.into_iter().filter(|(world, own, ..)| bodies.contains(world) || bodies.contains(own));
        picked.map(|(world, ..)| world).chain(bodies.clone()).collect()
    };
    // A face or snap picked for the start lies on a body that moves, and for the target on one that
    // does not; a typed point is a coordinate, wherever it lies.
    let mine = |from: bool| {
        let moved = &moved;
        move |b: &solidcraft_doc::ModelBody| moved.contains(&b.name) == from
    };
    // The planar normal of the face of the side of `key` that the first of `at` on one lies on
    // (the pick and where it snaps to): `Err` when they lie only on faces of the other bodies.
    let side_face = |st: &solidcraft_doc::ModelState, key: &str, at: &[Vec3]| -> Result<Option<Vec3>> {
        let from = key.starts_with("from");
        let Some((b, ..)) = at.iter().find_map(|&q| face_pick(st, q)) else { return Ok(None) };
        if let Some((.., n)) = at.iter().find_map(|&q| face_pick_on(st, q, mine(from))) {
            return Ok(n);
        }
        if from {
            Err(bad(cmd, format!("`{key}` is on `{}`, which is not moved: pick the start on a body in `bodies`", b.name)))
        } else {
            Err(bad(cmd, on_moving(key)))
        }
    };
    let world_pick = |s: &Session, pk: &str, fk: &str, held: bool| -> Result<(Vec3, Option<Vec3>)> {
        if held {
            if p.get(pk).is_some() || p.get(fk).is_some() {
                return Err(bad(cmd, format!("give `{pk}_plane` or `{pk}`/`{fk}`, not both")));
            }
            return Ok((Vec3::ZERO, None));
        }
        let world = s.world_state()?;
        let st = &*world;
        if let Some(fp) = p.get(fk).and_then(vec3) {
            let n = side_face(st, fk, &[fp])?.ok_or_else(|| bad(cmd, format!("`{fk}` must be on a planar face of a solid body")))?;
            return Ok((fp, Some(n)));
        }
        let SidePoint::Snap { at: near, kind } = side_point(p.get(pk), pk, fk).map_err(|why| bad(cmd, why))? else {
            return side_pick(st, p.get(pk), pk, fk).map_err(|why| bad(cmd, why));
        };
        // A face or circle is taken on the side's own bodies; a vertex or an edge's middle is
        // the same point on whichever body has it, of the solid bodies the check reads.
        let (side, solid) = (mine(pk == "from"), |b: &solidcraft_doc::ModelBody| !b.body.is_mesh());
        let keep: &dyn Fn(&solidcraft_doc::ModelBody) -> bool = match kind.as_str() {
            "face_center" | "circle_center" => &side,
            _ => &solid,
        };
        let got = snap_pick(st, &kind, near, keep).map_err(|why| bad(cmd, why))?;
        side_face(st, pk, &std::iter::once(near).chain(got.map(|g| g.0)).collect::<Vec<_>>())?;
        got.ok_or_else(|| bad(cmd, format!("no {kind} to snap to near `{pk}`")))
    };
    let pick = |s: &Session, pk: &str, fk: &str, held: bool| -> Result<(Vec3, Option<Vec3>)> {
        let (at, n) = world_pick(s, pk, fk, held)?;
        if held {
            return Ok((at, n));
        }
        Ok((solidcraft_doc::apply_point(&inverse, at), n.and_then(|n| solidcraft_doc::apply_vector(&inverse, n).normalized())))
    };
    let (from, from_normal) = pick(s, "from", "from_face", held(&from_plane, &from_frame))?;
    let (to, to_normal) = pick(s, "to", "to_face", held(&to_plane, &to_frame))?;
    // A turn about the target's direction after the move.
    let angle = expr(p, "angle");
    if let Some(a) = &angle {
        check_expr(s, a, Kind::Angle, cmd, "angle")?;
        if to_normal.is_none() && !held(&to_plane, &to_frame) {
            return Err(bad(cmd, "`angle` turns about the target's direction: give `to_frame`, `to_face` or `to_plane`"));
        }
    }
    let axes = axes_param(p, cmd)?;
    let facing = from_normal.is_some() || to_normal.is_some() || held(&from_plane, &from_frame) || held(&to_plane, &to_frame);
    if axes.is_some() && facing {
        return Err(bad(
            cmd,
            "`axes` applies to point-to-point alignment: leave out faces, face and circle centres, `from_plane`, `to_plane`, `from_frame` and `to_frame`",
        ));
    }
    let flip = bool_(p, "flip").unwrap_or(false);
    if flip && !facing {
        return Err(bad(cmd, FLIP_NEEDS));
    }
    let st = s.model.state();
    let (vals, _) = s.doc.param_values();
    let way = |plane: &Option<solidcraft_doc::PlaneRef>, frame: &Option<AlignFrame>| held_way(&s.doc, &vals, &st, s.active_component, plane, frame);
    let (from_normal, to_normal) = mate_ways(from_normal, to_normal, way(&from_plane, &from_frame)?, way(&to_plane, &to_frame)?);
    // The directions are stored in the same frame as `from`/`to` (the first moved body's frame).
    let axes = axes.map(|mask| {
        mask.into_iter()
            .zip([Vec3::X, Vec3::Y, Vec3::Z])
            .filter(|&(on, _)| on)
            .map(|(_, world)| Dir(solidcraft_doc::apply_vector(&inverse, world).normalized().unwrap_or(world)))
            .collect::<Vec<_>>()
    });
    add_feature(
        s,
        p,
        FeatureKind::Align {
            bodies: bodies.into_iter().map(BodyName).collect(),
            from: Position(from),
            to: Position(to),
            from_normal: from_normal.map(Dir),
            to_normal: to_normal.map(Dir),
            flip,
            flip_moves_only: false,
            from_plane,
            to_plane,
            axes,
            from_frame,
            to_frame,
            angle,
        },
    )
}

/// A side of an Align that `timeline.edit` picks again: a point as `from` takes one, or a point
/// on a planar face (`from_face`).
pub(crate) enum AlignPick {
    Point(Value),
    Face(Value),
}

/// `timeline.edit` of an Align (#21331): its `from`, `to`, `from_face` and `to_face` are picks,
/// read as `solid.align` reads them, so they are taken out of `set` before it is merged into the
/// stored feature.
pub(crate) fn take_align_picks(set: &mut Value) -> Result<[Option<AlignPick>; 2]> {
    let mut take = |k: &str| set.as_object_mut().and_then(|o| o.remove(k)).filter(|v| !v.is_null());
    let mut side = |k: &str| match (take(k), take(&format!("{k}_face"))) {
        (Some(_), Some(_)) => Err(bad("timeline.edit", format!("give `{k}` or `{k}_face`, not both"))),
        (point, face) => Ok(face.map(AlignPick::Face).or(point.map(AlignPick::Point))),
    };
    Ok([side("from")?, side("to")?])
}

/// The keys of an Align's `timeline.edit` besides its picks that decide the stored ways again.
pub(crate) const ALIGN_WAY_KEYS: [&str; 5] = ["from_plane", "to_plane", "from_frame", "to_frame", "flip"];

/// The edited Align `new` (`old` before the edit, feature `id` in `component`) with the sides
/// `picks` gives picked again in the model before it ([`side_pick`]), and both stored ways worked
/// out again ([`mate_ways`]), so none is left from an old pick, plane or frame. A side not picked
/// again keeps its stored way; one that was stored as the exact opposite of the side picked again
/// may have been taken from it, so then the new pick must face as the old one did, or both sides
/// are asked for. `flip` with no side that faces a way is refused. An edit of an Align from an
/// older design takes this build's reading of `flip` (`flip_moves_only` is cleared).
pub(crate) fn repick_align(
    s: &Session,
    id: u64,
    component: u64,
    old: &FeatureKind,
    new: &mut FeatureKind,
    picks: [Option<AlignPick>; 2],
) -> Result<()> {
    let cmd = "timeline.edit";
    let (
        FeatureKind::Align { from_normal: was_from, to_normal: was_to, .. },
        FeatureKind::Align { from, to, from_normal, to_normal, flip, flip_moves_only, from_plane, to_plane, from_frame, to_frame, axes, .. },
    ) = (old, new)
    else {
        return Ok(());
    };
    let st = s.model.state_before(id);
    let (vals, _) = s.doc.param_values();
    let held = |plane: &Option<solidcraft_doc::PlaneRef>, frame: &Option<AlignFrame>| held_way(&s.doc, &vals, &st, component, plane, frame);
    let way = |n: &Option<Dir>| n.as_ref().map(|d| d.0);
    let (mut from_way, mut to_way) = (way(from_normal), way(to_normal));
    let [from_pick, to_pick] = picks;
    let repick = |v: &AlignPick,
                  key: &str,
                  held: bool,
                  at: &mut Position,
                  mine: &mut Option<Vec3>,
                  was: Option<Vec3>,
                  other: Option<Vec3>,
                  other_given: bool|
     -> Result<()> {
        if held {
            return Err(bad(cmd, format!("`{key}` is a plane or a frame here: set `{key}_plane` or `{key}_frame` instead")));
        }
        let (p, n) = match v {
            AlignPick::Point(v) => side_pick(&st, Some(v), key, &format!("{key}_face")).map_err(|why| bad(cmd, why))?,
            AlignPick::Face(v) => {
                let fp = vec3(v).ok_or_else(|| bad(cmd, format!("`{key}_face` must be [x, y, z]")))?;
                (fp, Some(planar_face_normal(&st, fp).ok_or_else(|| bad(cmd, format!("`{key}_face` must be on a planar face of a solid body")))?))
            }
        };
        let opposite = matches!((was, other), (Some(a), Some(b)) if a * -1.0 == b);
        if opposite && !other_given && n != was {
            return Err(bad(
                cmd,
                "this Align's two sides face opposite ways, so one may have taken its way from the other: give both `from` and `to`",
            ));
        }
        (*at, *mine) = (Position(p), n);
        Ok(())
    };
    let given_to = to_pick.is_some();
    if let Some(v) = &from_pick {
        repick(v, "from", from_plane.is_some() || from_frame.is_some(), from, &mut from_way, way(was_from), way(was_to), given_to)?;
    }
    if let Some(v) = &to_pick {
        repick(v, "to", to_plane.is_some() || to_frame.is_some(), to, &mut to_way, way(was_to), way(was_from), from_pick.is_some())?;
    }
    let (from_held, to_held) = (held(&*from_plane, &*from_frame)?, held(&*to_plane, &*to_frame)?);
    from_way = from_way.filter(|_| from_held.is_none());
    to_way = to_way.filter(|_| to_held.is_none());
    let facing = from_way.is_some() || to_way.is_some() || from_held.is_some() || to_held.is_some();
    if *flip && !facing {
        return Err(bad(cmd, FLIP_NEEDS));
    }
    if axes.is_some() && facing && (from_pick.is_some() || to_pick.is_some()) {
        return Err(bad(cmd, "`axes` applies to point-to-point alignment: pick two points, or set `axes` to null"));
    }
    let (f, t) = mate_ways(from_way, to_way, from_held, to_held);
    (*from_normal, *to_normal, *flip_moves_only) = (f.map(Dir), t.map(Dir), false);
    Ok(())
}

/// The longest sketch point or curve id an Align frame takes.
pub(crate) const MAX_SKETCH_ID: usize = 256;

/// The point or curve of an Align `{sketch, point|curve}` frame: an id of 1 to
/// [`MAX_SKETCH_ID`] characters. `None` when the frame gives both, neither or an id that is not
/// one.
pub(crate) enum SketchFrameId {
    Point(String),
    Curve(String),
}

pub(crate) fn sketch_frame_id(v: &Value) -> Option<SketchFrameId> {
    let id = |k: &str| str_(v, k).filter(|n| !n.is_empty() && n.chars().count() <= MAX_SKETCH_ID).map(str::to_string);
    match (id("point"), id("curve")) {
        (Some(point), None) => Some(SketchFrameId::Point(point)),
        (None, Some(curve)) => Some(SketchFrameId::Curve(curve)),
        _ => None,
    }
}

/// An Align side's frame (#6257), `<key>_frame`: a joint origin as `joint.create` takes one, or a
/// sketch point or the centre of a sketch circle, arc or ellipse (`{sketch, point|curve}`), on the
/// moved bodies' component (all of them on one, placed once). It takes the place of `<key>`,
/// `<key>_face` and `<key>_plane`, and is checked to resolve now.
fn align_frame(s: &Session, p: &Value, key: &str, bodies: &[String]) -> Result<Option<AlignFrame>> {
    let cmd = "solid.align";
    let fk = format!("{key}_frame");
    let Some(v) = p.get(&fk).filter(|v| !v.is_null()) else { return Ok(None) };
    if let Some(other) = [key.to_string(), format!("{key}_face"), format!("{key}_plane")].into_iter().find(|k| p.get(k).is_some()) {
        return Err(bad(cmd, format!("give `{fk}` or `{other}`, not both")));
    }
    let st = s.model.state();
    let (frame, on) = match v.get("sketch") {
        Some(sk) => {
            let sketch = super::features::sketch_id(s, Some(sk), cmd, &fk)?;
            let frame = match sketch_frame_id(v) {
                Some(SketchFrameId::Point(point)) => AlignFrame::SketchPoint { sketch: SketchId(sketch), point },
                Some(SketchFrameId::Curve(curve)) => AlignFrame::SketchCurve { sketch: SketchId(sketch), curve },
                None => return Err(bad(cmd, format!("`{fk}`: a sketch frame takes `point` or `curve` (an id of 1 to {MAX_SKETCH_ID} characters)"))),
            };
            (frame, s.doc.feature(sketch).map_or(0, |f| f.component))
        }
        None => {
            // On its part, in the part's own coordinates: the same in every copy of a
            // sub-assembly the part is in, wherever it was picked.
            let picked = super::joints::origin_param(s, Some(v), cmd)?;
            let origin = solidcraft_doc::joints::JointOrigin::new(picked.part(), picked.snap);
            let on = super::joints::component_of(s, origin.occurrence);
            (AlignFrame::Origin { origin: Box::new(origin) }, on)
        }
    };
    // The move is worked out in that component's coordinates: every body must be in it, and a
    // component placed twice would not say which placement the picks were made in.
    if bodies.iter().any(|b| st.body(b).map_or(0, |b| s.doc.model_body_component(b)) != on) {
        return Err(bad(cmd, format!("not supported yet: `{fk}` on another component than the bodies")));
    }
    if on != 0 && s.doc.occurrences.iter().filter(|o| o.component == on).count() != 1 {
        return Err(bad(cmd, format!("not supported yet: `{fk}` on a component placed more than once")));
    }
    frame.resolve(&s.doc, &st).map_err(|e| bad(cmd, format!("`{fk}`: {e}")))?;
    Ok(Some(frame))
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.remove";
    let bodies = string_list(cmd, p, "bodies")?;
    if bodies.is_empty() {
        return Err(bad(cmd, "`bodies` must list bodies"));
    }
    let st = s.model.state();
    if let Some(b) = bodies.iter().find(|b| st.body(b).is_none()) {
        return Err(bad(cmd, format!("no body `{b}`")));
    }
    add_feature(s, p, FeatureKind::Remove { bodies: bodies.into_iter().map(BodyName).collect() })
}

#[cfg(test)]
#[path = "features_more_tests.rs"]
mod tests;

/// What `flip` says when neither side of an Align faces a way.
pub(crate) const FLIP_NEEDS: &str =
    "flip needs a face, a face centre, a circle centre, a plane or a frame on one side: two points face no way to turn over";

/// An Align side given as a point (`from` or `to`) in the model `st`: where it lands and the way
/// the pick faces, decided by what it picks (#21331). A circle's centre faces along the circle's
/// axis out of its body, a face's centre as the face does; a vertex, an edge's middle, the
/// origin and a bare point face no way. `fk` is the face key beside it, for the message.
pub(crate) fn side_pick(st: &solidcraft_doc::ModelState, v: Option<&Value>, pk: &str, fk: &str) -> std::result::Result<(Vec3, Option<Vec3>), String> {
    match side_point(v, pk, fk)? {
        SidePoint::Origin => Ok((Vec3::ZERO, None)),
        SidePoint::Point(v) => Ok((v, None)),
        SidePoint::Snap { at, kind } => snap_pick(st, &kind, at, &|_| true)?.ok_or_else(|| format!("no {kind} to snap to near `{pk}`")),
    }
}

/// A snap near `at` on the placed model `st` and the way it faces ([`side_pick`]): the nearest
/// vertex or edge midpoint (no way), the centre of the planar face `at` lies on ([`face_pick`]:
/// its outward normal; `Err` for a face that is not planar), or the centre of the circle `at`
/// picks ([`circle_pick`]), on the bodies `keep` takes. `None` when there is none to snap to.
pub(crate) fn snap_pick(
    st: &solidcraft_doc::ModelState,
    kind: &str,
    at: Vec3,
    keep: &dyn Fn(&solidcraft_doc::ModelBody) -> bool,
) -> std::result::Result<Option<(Vec3, Option<Vec3>)>, String> {
    let near = |pts: Vec<Vec3>| pts.into_iter().min_by(|a, b| a.dist(at).total_cmp(&b.dist(at)));
    match kind {
        "vertex" => Ok(near(
            st.bodies
                .iter()
                .filter(|b| keep(b))
                .flat_map(|b| b.mesh().edges.iter().flat_map(|e| [e.first().copied(), e.last().copied()]).flatten().collect::<Vec<_>>())
                .collect(),
        )
        .map(|q| (q, None))),
        "edge_mid" => {
            let d = |e: &solidcraft_doc::kernel::EdgeInfo| e.points.windows(2).map(|w| at.dist_to_segment(w[0], w[1])).fold(f64::MAX, f64::min);
            Ok(st
                .bodies
                .iter()
                .filter(|b| keep(b))
                .filter_map(|b| b.body.edges(solidcraft_doc::pick_reach(b)).ok())
                .flatten()
                .min_by(|a, b| d(a).total_cmp(&d(b)))
                .map(|e| (e.mid, None)))
        }
        "face_center" => {
            let Some((b, i, normal)) = face_pick_on(st, at, keep) else { return Ok(None) };
            let normal = normal.ok_or("face_center snaps to the centre of a planar face: the face there is curved")?;
            let faces = b.body.faces(solidcraft_doc::pick_reach(b)).map_err(|e| e.to_string())?;
            Ok(faces.into_iter().find(|f| f.index == i).map(|f| (f.centroid, Some(normal))))
        }
        "circle_center" => Ok(circle_pick(st, at, keep)?.map(|(centre, axis)| (centre, Some(axis)))),
        _ => Ok(None),
    }
}

/// What Align says when the target `key` is on the bodies it moves.
fn on_moving(key: &str) -> String {
    let instead = if key == "to_frame" {
        "pick the target on another body or on a sketch"
    } else {
        "pick the target on another body, or use `to_frame` for a sketch"
    };
    format!("`{key}` is on a body that moves: {instead}")
}

/// The face a pick at `at` lies on in the placed model `st`, found on the exact geometry, never
/// the display mesh: of the faces within the reach references take picks at (`pick_reach`), the
/// nearest. Its body, its index and, when it is planar, its outward normal.
fn face_pick(st: &solidcraft_doc::ModelState, at: Vec3) -> Option<(&solidcraft_doc::ModelBody, usize, Option<Vec3>)> {
    face_pick_on(st, at, |_| true)
}

/// [`face_pick`] among the bodies `keep` takes.
fn face_pick_on(
    st: &solidcraft_doc::ModelState,
    at: Vec3,
    keep: impl Fn(&solidcraft_doc::ModelBody) -> bool,
) -> Option<(&solidcraft_doc::ModelBody, usize, Option<Vec3>)> {
    use solidcraft_doc::kernel::{FaceSurface, face_surface};
    let mut best: Option<(f64, &solidcraft_doc::ModelBody, usize)> = None;
    for b in st.bodies.iter().filter(|b| !b.body.is_mesh() && keep(b)) {
        let reach = solidcraft_doc::pick_reach(b);
        for f in 0..b.body.face_count() {
            if let Some(d) = b.body.point_lies_on_face(f, at, reach).ok().flatten().map(|q| q.dist(at))
                && best.is_none_or(|(x, ..)| d < x)
            {
                best = Some((d, b, f));
            }
        }
    }
    let (_, b, f) = best?;
    let normal = match face_surface(&b.body, f) {
        Some(FaceSurface::Plane { normal, .. }) => Some(normal.vec()),
        _ => None,
    };
    Some((b, f, normal))
}

/// The outward normal of the planar face a pick at `at` lies on ([`face_pick`]); `None` off the
/// faces or on one that is not planar.
pub(crate) fn planar_face_normal(st: &solidcraft_doc::ModelState, at: Vec3) -> Option<Vec3> {
    face_pick(st, at)?.2
}

/// The circle a pick at `at` stands for in the placed model `st`, by what the pick lies on: a
/// circular edge (on it or at its centre), else the cylindrical face it lies on (that face's
/// nearer rim). Its centre, and its axis pointing out of its body. `None` when the pick lies on
/// neither; `Err` for a circle that faces out of its body neither way (between two walls).
fn circle_pick(
    st: &solidcraft_doc::ModelState,
    at: Vec3,
    keep: &dyn Fn(&solidcraft_doc::ModelBody) -> bool,
) -> std::result::Result<Option<(Vec3, Vec3)>, String> {
    use solidcraft_doc::kernel::{FaceSurface, circle_edge_at, face_edge_indices, face_surface, rim_circle};
    let failed = |e: solidcraft_doc::kernel::KernelError| e.to_string();
    let bodies: Vec<&solidcraft_doc::ModelBody> = st.bodies.iter().filter(|b| !b.body.is_mesh() && keep(b)).collect();
    // A pick lies on what is within the reach references take picks at (`pick_reach`).
    let mut on_edge: Option<(f64, &solidcraft_doc::ModelBody, usize)> = None;
    for &b in &bodies {
        if let Some((e, d)) = circle_edge_at(&b.body, at, solidcraft_doc::pick_reach(b), &[]).map_err(failed)?
            && on_edge.is_none_or(|(x, ..)| d < x)
        {
            on_edge = Some((d, b, e));
        }
    }
    let mut found = on_edge.map(|(_, b, e)| (b, e));
    if found.is_none() {
        for &b in &bodies {
            let walls = solidcraft_doc::appearance::faces_within(b, at, solidcraft_doc::pick_reach(b));
            let walls: Vec<usize> = walls.into_iter().filter(|&f| matches!(face_surface(&b.body, f), Some(FaceSurface::Cylinder { .. }))).collect();
            let rims: Vec<usize> = walls.iter().flat_map(|&f| face_edge_indices(&b.body, f)).collect();
            if rims.is_empty() {
                continue;
            }
            // Of the face's own rims, however far along the wall.
            if let Some((e, _)) = circle_edge_at(&b.body, at, f64::MAX, &rims).map_err(failed)? {
                found = Some((b, e));
                break;
            }
        }
    }
    let Some((b, e)) = found else { return Ok(None) };
    let rim = rim_circle(&b.body, e).map_err(failed)?.ok_or("that circle lies between two round faces and faces out of its body neither way")?;
    Ok(Some((rim.center, rim.axis.vec())))
}

/// The way an Align side held as a plane or a frame faces now, in the frame of `component` (the
/// feature works it out again when it is evaluated). `None` for a side held by neither.
pub(crate) fn held_way(
    doc: &solidcraft_doc::Document,
    vals: &std::collections::BTreeMap<String, solidcraft_doc::expr::Value>,
    st: &solidcraft_doc::ModelState,
    component: u64,
    plane: &Option<solidcraft_doc::PlaneRef>,
    frame: &Option<AlignFrame>,
) -> Result<Option<Vec3>> {
    if let Some(fr) = frame {
        return Ok(Some(fr.resolve(doc, st)?.1));
    }
    let Some(pl) = plane else { return Ok(None) };
    Ok(Some(doc.resolve_plane_in(vals, pl, 0, component, Some(st))?.normal()))
}

/// The stored directions of an Align's two sides, from each pick's own (`from`, `to`), with the
/// sides held as a plane or a frame (`from_held`, `to_held`). A held side is worked out when
/// evaluated, so it stores none. A side that faces no way takes the opposite of the other side's
/// pick, so the bodies keep their turn and `flip` turns them half round about an axis square to
/// it; against a plane or a frame it stores none, and is turned against it when evaluated.
pub(crate) fn mate_ways(from: Option<Vec3>, to: Option<Vec3>, from_held: Option<Vec3>, to_held: Option<Vec3>) -> (Option<Vec3>, Option<Vec3>) {
    let take = |own: Option<Vec3>, held: Option<Vec3>, other: Option<Vec3>| match held {
        Some(_) => None,
        None => own.or(other.map(|b| b * -1.0)),
    };
    (take(from, from_held, to), take(to, to_held, from))
}

fn boundary_fill(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "solid.boundary_fill";
    let tools = string_list(cmd, p, "tools")?;
    let st = s.model.state();
    if tools.is_empty() || tools.len() > 64 || tools.iter().any(|t| st.body(t).is_none()) {
        return Err(bad(cmd, "`tools` must list 1…64 bodies"));
    }
    let cells: Vec<Vec3> = p.get("cells").and_then(Value::as_array).map(|a| a.iter().filter_map(vec3).collect()).unwrap_or_default();
    if cells.is_empty() || cells.len() > 1000 {
        return Err(bad(cmd, "`cells` must list 1…1000 points [x, y, z], one inside each cell"));
    }
    // Planes that also bound the cells: origin planes or construction planes, by name.
    let mut planes = Vec::new();
    for name in string_list(cmd, p, "planes")? {
        planes.push(super::features::plane_param(s, Some(&Value::String(name)), cmd)?);
    }
    if tools.len() + planes.len() > 64 {
        return Err(bad(cmd, "`tools` and `planes` together must list at most 64"));
    }
    let operation = super::features::operation(p, cmd)?;
    add_feature(
        s,
        p,
        FeatureKind::BoundaryFill {
            tools: tools.into_iter().map(BodyName).collect(),
            planes,
            cells: cells.into_iter().map(Position).collect(),
            operation,
            remove_tools: bool_(p, "remove_tools").unwrap_or(false),
        },
    )
}

/// A Coil axis saved as the edge of a picked placement (`{type: placed_edge, …}`), checked to
/// resolve as the Coil reads it; `None` for any other form of `axis`.
pub(crate) fn coil_placed_edge(s: &Session, v: Option<&Value>, cmd: &str) -> Result<Option<solidcraft_doc::construct::GeoRef>> {
    let Some(value) = v.filter(|axis| str_(axis, "type") == Some("placed_edge")) else { return Ok(None) };
    let geo = super::construct::geo(s, Some(value), cmd, "axis")?;
    solidcraft_doc::construct::model_axis_in(Some(&s.placed_document()), &s.model.state(), &geo, s.active_component)
        .map_err(|error| bad(cmd, format!("`axis`: {error}")))?;
    Ok(Some(geo))
}

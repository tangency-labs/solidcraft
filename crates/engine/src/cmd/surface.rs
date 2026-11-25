//! Surface commands: Patch, Stitch, Thicken and Trim (surface bodies and turning them into
//! solids), and the planned surface toolset (`planned!`: registered, not built yet).

use serde_json::Value;
use solidcraft_doc::{
    BlendSide, EdgeContinuity, Expr, ExtendMode, ExtendTo, FaceAt, FeatureKind, PatchContinuity, PlaneRef, ProfileSel, RuledCorners, RuledMode,
    TrimCurves, expr::Kind,
};
use solidcraft_doc::{BodyName, SketchId};
use solidcraft_doc::{Dir, FacePick, Position};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{Construction, Continuity};

use super::CommandSpec;
use super::features::{add_feature, axis_param, check_expr, extent_param, feature_sketch, operation, plane_param, profiles, sketch_id};
use crate::params::{bad, bool_, expr, num, str_, string_list, vec3};
use crate::schema::features::{FeatureResult, SurfaceSweepParams};
use crate::schema::surface::{
    FairParams, OffsetParams, PatchParams, PatchResult, ReverseNormalParams, StitchParams, SurfaceExtendParams, SurfaceLoftParams, SurfaceTrimParams,
    ThickenParams, UnstitchParams, UntrimParams,
};
use crate::schema::surface_blend::BlendParams;
use crate::schema::surface_profile::{SurfaceExtrudeParams, SurfaceRevolveParams};
use crate::schema::surface_ruled::SurfaceRuledParams;
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const PATCH_CONTRACT: TypedContract<PatchParams> = TypedContract::new();
const STITCH_CONTRACT: TypedContract<StitchParams> = TypedContract::new();
const UNSTITCH_CONTRACT: TypedContract<UnstitchParams> = TypedContract::new();
const REVERSE_NORMAL_CONTRACT: TypedContract<ReverseNormalParams> = TypedContract::new();
const THICKEN_CONTRACT: TypedContract<ThickenParams> = TypedContract::new();
const OFFSET_CONTRACT: TypedContract<OffsetParams> = TypedContract::new();
const UNTRIM_CONTRACT: TypedContract<UntrimParams> = TypedContract::new();
const FAIR_CONTRACT: TypedContract<FairParams> = TypedContract::new();
const SURFACE_EXTEND_CONTRACT: TypedContract<SurfaceExtendParams> = TypedContract::new();
const SURFACE_TRIM_CONTRACT: TypedContract<SurfaceTrimParams> = TypedContract::new();
const SURFACE_SWEEP_CONTRACT: TypedContract<SurfaceSweepParams> = TypedContract::new();
const SURFACE_LOFT_CONTRACT: TypedContract<SurfaceLoftParams> = TypedContract::new();
const BLEND_CONTRACT: TypedContract<BlendParams> = TypedContract::new();
const SURFACE_RULED_CONTRACT: TypedContract<SurfaceRuledParams> = TypedContract::new();
const SURFACE_EXTRUDE_CONTRACT: TypedContract<SurfaceExtrudeParams> = TypedContract::new();
const SURFACE_REVOLVE_CONTRACT: TypedContract<SurfaceRevolveParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("surface.patch", "Patch", crate::checked!("surface.patch", patch)).begins_edit()
        .at("SURFACE", "CREATE")
        .icon("surface_patch")
        .contract(&PATCH_CONTRACT)
        .output(output_schema_for::<PatchResult>)
        .params("sketch?: id|name and profiles? (planar patches of sketch regions) or profiles_3d: [indices] (the sketch's planar 3D-sketch loops, each patched in its own plane, as listed by sketch.inspect) | edges: [[x,y,z]] (points on a closed loop of a body's edges: one face on them, planar when the loop is, else a smooth B-spline surface) and body?; continuity?: g0|g1|g2|g3|g4 (edges only, default g0: how the patch meets the faces beside the loop — g1 their tangent planes, g2 also their curvature across the edges, g3 also its rate of change, g4 the change of that rate) or [{edge: [x,y,z], g: g0|g1|g2|g3|g4}] (those edges so, the rest g0) or {all, edges}; points?: [[x,y,z]] (edges only, up to 64: points the patch passes through); crown?: number −0.9…3 (default 0: only a fitted patch, whose middle becomes fuller by this many times the loop's size — the bump vanishes on the loop, so the face still meets the picked edges and the continuity across them; a loop that is planar and g0 is the exact plane whatever the crown; with points, crown needs a g1 or higher edge); name?, body_name?"),
    CommandSpec::new("surface.stitch", "Stitch", crate::checked!("surface.stitch", stitch)).begins_edit()
        .at("SURFACE", "MODIFY")
        .icon("surface_stitch")
        .contract(&STITCH_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("bodies: [names] (surfaces sewn into the first; a solid when they close); tolerance?: expr (default: tight)"),
    CommandSpec::new("surface.thicken", "Thicken", crate::checked!("surface.thicken", thicken)).begins_edit()
        .at("SURFACE", "CREATE")
        .icon("surface_thicken")
        .contract(&THICKEN_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("bodies: [surface names]; thickness: expr (negative: against the normal); symmetric?: bool; operation?: new|join|cut|intersect; targets?"),
    CommandSpec::new("surface.extend", "Extend", crate::checked!("surface.extend", extend)).begins_edit()
        .at("SURFACE", "MODIFY")
        .icon("surface_extend")
        .contract(&SURFACE_EXTEND_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("body: surface name; edges: [[x,y,z]] (points on boundary edges to push out); distance: expr (unless up_to); mode?: natural (the surface itself continued: a B-spline's end spans continued, which keeps planes and surfaces of revolution on themselves; the default) | tangent (the face continued tangent to its surface across the edge, G1) | curvature (continued keeping the surface's curvature across the edge too, G2); up_to?: plane (XY|XZ|YZ|plane name|{origin, normal}) | {body, point} (a face, its surface extended) | \"bounds\" (natural: back to the surface's own bounds, undoing a trim); neighbours?: bool (default true: the edges next to an extended edge are lengthened to meet it; false: the extension's sides run square to the edge)"),
    CommandSpec::new("surface.trim", "Trim", crate::checked!("surface.trim", trim)).begins_edit()
        .at("SURFACE", "MODIFY")
        .icon("surface_trim")
        .contract(&SURFACE_TRIM_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("body: surface name; plane: XY|XZ|YZ|plane name|{origin, normal} or tool: {body, point} (a face, its surface extended) or curves: {sketch, curves?: [curve ids] (chains of curves meeting end to end; default every curve), direction?: normal|face_normal|[x,y,z] (swept onto the surface along the sketch's normal, the default, or this direction; face_normal: projected along the surface's own normal, each point of a curve to its nearest point of the surface, and `keep` picks regions)}; keep: [x,y,z] (a point on the side to keep) or [[x,y,z], ...] (points on the regions to keep: the pieces that hang together once cut). Mutual trim: with: another surface (one face each; leave out plane, tool and curves), keep_with: [x,y,z] or [[x,y,z], ...] (what to keep of it); each is cut where the other's surface (extended) meets it; extend?: bool (a surface that stops short of the other is extended to meet it); stitch?: bool (the kept pieces sewn into `body`; `with` is then gone)"),
    CommandSpec::new("surface.extrude", "Surface Extrude", crate::checked!("surface.extrude", surface_extrude)).begins_edit()
        .at("SURFACE", "CREATE")
        .icon("surface_extrude")
        .contract(&SURFACE_EXTRUDE_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("sketch?: id|name; curves?: [curve ids] (open or closed chains of curves meeting end to end) and/or profiles? (their boundaries; the default when no curves are given); distance: expr (unless `to` or `to_next`); to?: [x,y,z] (To Object: end at the face, vertex or point there; a curved or slanted face trims the walls where they meet its surface); to_offset?: expr (how far past `to`; a flat face square to the extrusion or a point only); to_next?: bool (To Next: each wall runs on, one way as `direction` says, until it first meets a solid or surface body, its faces as they are; a wall that meets none, or runs on past the side of a body covering only part of it, is refused); direction?: positive|negative|symmetric (symmetric: distance is the total, split evenly around the start plane, as solid.extrude); convention?: total_distance|legacy_per_side (as solid.extrude; default total_distance; legacy_per_side: the distance on each side, as older symmetric surface extrudes keep); distance2?: expr; start_offset?: expr; name?, body_name? (a surface body: walls only, no caps)"),
    CommandSpec::new("surface.revolve", "Surface Revolve", crate::checked!("surface.revolve", surface_revolve)).begins_edit()
        .at("SURFACE", "CREATE")
        .icon("surface_revolve")
        .contract(&SURFACE_REVOLVE_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("sketch?: id|name; curves?: [curve ids] and/or profiles? (as surface.extrude); axis: sketch line id|x|y|X|Y|Z|{type: world, axis: X|Y|Z}|{sketch, curve} (a line of another sketch, followed in that sketch)|construction axis name|{origin, dir} (origin: numbers or length expressions, followed when parameters change) | {edge|face: [x,y,z]} (a straight or circular edge, or a cylinder, cone or torus face: about its axis, followed as the model changes); angle?: expr (default 360 deg); angle2?: expr (the other side); symmetric?: bool; faced?: bool (default true: each face faces the side Surface Extrude's would, the curve's direction crossed with the sketch normal, so out of a profile; false: each face along its surface's own normal, as surface revolves saved before it keep); name?, body_name? (a surface body, no caps; a solid where a full turn closes it)"),
    CommandSpec::new("surface.sweep", "Surface Sweep", crate::checked!("surface.sweep", surface_sweep)).begins_edit()
        .at("SURFACE", "CREATE")
        .icon("surface_sweep")
        .contract(&SURFACE_SWEEP_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("sketch?: id|name; curves?: [curve ids] and/or profiles? (as surface.extrude); path_sketch: sketch; path: [curve ids in order] (a path with corners, or 3D curves, gives ruled walls mitred at the corners); path_body?: body name; path_edges?: [points on its edges] (the path is a chain of that body's edges, instead of a sketch's curves); orientation?, twist?, scale?, rails?, rail_fit? (as solid.sweep); name?, body_name? (a surface body, no caps)"),
    CommandSpec::new("surface.loft", "Surface Loft", crate::checked!("surface.loft", surface_loft)).begins_edit()
        .at("SURFACE", "CREATE")
        .icon("surface_loft")
        .contract(&SURFACE_LOFT_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("sections: [{sketch, curves?: [curve ids], profiles?} | {body, edges: [[x,y,z]]}] (2…50 in order; each one chain: a profile without holes, or curves meeting end to end, planar or of a 3D sketch; all open or all closed (all open with a 3D sketch curve); a body edge chain takes the face the chain bounds as the surface the end repeats); continuity?: [g0|g1|g2|g3, g0|g1|g2|g3] (each end, default g0: how much of the surface at the first and last section the loft repeats — the tangent plane, the curvature, or how that curvature changes); crown?: [number, number] (default 0: how full each end leaves, -0.9 flattest to 3 fullest); rails?, rail_fit? (as solid.loft; planar sketch sections only, both ends g0); faced?: bool (default true: through planar sketch sections, the walls face out of the region closed sections bound, as solid.loft's faces do, and an open section's direction crossed with its sketch normal, as surface.extrude's; false: each wall along its skinned surface's normal, as surface lofts saved before it keep); name?, body_name? (a surface body: ruled between two sections, smooth through three or more; no caps)"),
    CommandSpec::new("surface.offset", "Offset Surface", crate::checked!("surface.offset", offset)).begins_edit()
        .at("SURFACE", "CREATE")
        .icon("surface_offset")
        .contract(&OFFSET_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("faces: [[x,y,z]] (points on the faces); body?; distance: expr (signed, along the faces' normals; 0 copies the faces); chain?: bool (each pick's whole smooth chain of faces); tolerance?: expr (how far a fitted face may stray, default 0.01 mm). Planes and cylinders move exactly; other faces are fitted. A new surface body (a solid where its faces close)"),
    CommandSpec::new("surface.ruled", "Ruled Surface", crate::checked!("surface.ruled", ruled)).begins_edit()
        .at("SURFACE", "CREATE")
        .icon("surface_ruled")
        .contract(&SURFACE_RULED_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("body; edges: [[x,y,z]] (points on a chain of its edges, open or closed); distance: expr (how far the strip reaches); mode?: normal|tangent|angle|direction (default normal: along the normal of the face beside each edge; tangent: continuing it); angle?: expr (angle mode: from `direction`, else from the face's normal, turned away from the face; 90 deg from the normal is tangent); direction?: [x,y,z] (direction mode: the rulings; angle mode: the reference, e.g. a pull direction for a draft); face?: [x,y,z] (a point on the face the directions come from; default: the face beside the most picked edges); flip?: bool (the other side); corners?: mitre|round (sharp corners of the chain; inside corners are always cut back); far?: [null|[[x,y,z]]] (per chain edge, the interior control points of a free strip's far curve, as the default curve's handles list them; null: the default). A constant ruling, or one keeping its angle to an arc's axis, gives an exact plane, cylinder or cone; where the rulings turn freely along an edge, the strip is the exact ruled surface to a far curve of its own (from corner to corner, fair, near the rulings' ends). A new surface body"),
    CommandSpec::new("surface.blend", "Blend Surface", crate::checked!("surface.blend", blend)).begins_edit()
        .at("SURFACE", "CREATE")
        .icon("surface_blend")
        .contract(&BLEND_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("from, to: {body, edges: [[x,y,z]]} (points on the edges of one chain of each body: two picks on the same edge is that one edge, more chain up); continuity?: [g0|g1|g2|g3, g0|g1|g2|g3] (each end, default g1: how much of the surface at the picked edges the blend repeats — the tangent plane, the curvature, or how that curvature changes); bulge?: [number, number] (default 0: how full each end leaves, -0.9 flattest to 3 fullest); align?: [[x,y,z], [x,y,z]] (a point each end leaves towards, when the faces at the picks do not say); seam?: [[x,y,z], [x,y,z]] (a point on each chain its stations start from: two closed chains otherwise pair the way that keeps the blend from twisting; an open chain starts at the end nearer its point; a closed chain blends only to a closed chain); stitch?: bool (sew the blend onto its two bodies where the boundaries meet, replacing them); name?, body_name?"),
    CommandSpec::new("surface.untrim", "Untrim", crate::checked!("surface.untrim", untrim)).begins_edit()
        .at("SURFACE", "MODIFY")
        .icon("surface_untrim")
        .contract(&UNTRIM_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("face: {body, point}; holes?: all|[[x,y,z]] (points on the inner loops to remove; default: all); outer?: bool (restore the outer boundary to the surface's natural bounds: a free-form surface's own, an analytic surface's span of the face); in_place?: bool (untrim a surface body's face where it is; a solid's face is always copied out). A surface body; the face keeps its name"),
    CommandSpec::new("surface.unstitch", "Unstitch", crate::checked!("surface.unstitch", unstitch)).begins_edit()
        .at("SURFACE", "MODIFY")
        .icon("surface_unstitch")
        .contract(&UNSTITCH_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("body; faces?: [[x,y,z]] (points on the faces to take out, each its own surface body; the rest stay together under the body's name; default: every face its own surface body). A solid comes apart into surfaces"),
    CommandSpec::new("surface.reverse_normal", "Reverse Normal", crate::checked!("surface.reverse_normal", reverse_normal)).begins_edit()
        .at("SURFACE", "MODIFY")
        .icon("surface_reverse_normal")
        .contract(&REVERSE_NORMAL_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("body (a surface); faces?: [[x,y,z]] (points on the faces to turn over; default: all of them)"),
    CommandSpec::new("surface.fair", "Fair Surface", crate::checked!("surface.fair", fair)).begins_edit()
        .at("SURFACE", "MODIFY")
        .icon("surface_fair")
        .contract(&FAIR_CONTRACT)
        .output(output_schema_for::<FeatureResult>)
        .params("body: surface name; faces?: [[x,y,z]] points on the faces to fair (default: every face); strength?: number (0..1 of the full fairing step, default 1); iterations?: number (1..1000 steps, default 1); keep_edges?: bool (default true: hold the boundary row and the row next to it, so a face keeps meeting its neighbours in position and in tangent); tolerance?: expr mm (how far a control vertex may move, default 0.001 of the body's size). Moves the interior control vertices of each B-spline face to flatten its curvature variation while the boundary stays put: the face keeps its edges and the body its shape within the tolerance, and the warning in the reply carries the energy before and after; name?"),
    planned!("surface.fillet", "Surface Fillet", "769")
        .at("SURFACE", "MODIFY")
        .icon("surface_fillet")
        .params("planned: a, b: {body, point} (two faces of separate bodies that meet or cross; the points pick the side the ball rolls on); radius: expr; trim?: both|first|none"),
];

fn patch(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.patch";
    let crown = crown_param(p, cmd)?;
    let continuity = patch_continuity(p, cmd)?;
    let points: Vec<Position> = patch_points(p, cmd)?.into_iter().map(Position).collect();
    let profiles_3d = profiles_3d_param(p, cmd)?;
    let edges: Vec<solidcore_record::Vec3> =
        p.get("edges").and_then(Value::as_array).map(|a| a.iter().filter_map(vec3).collect()).unwrap_or_default();
    let mut v = if !edges.is_empty() {
        if !profiles_3d.is_empty() {
            return Err(bad(cmd, "`profiles_3d` is for a patch on a sketch"));
        }
        if edges.len() > 10_000 {
            return Err(bad(cmd, "too many edges"));
        }
        let body = str_(p, "body").map(str::to_string);
        add_feature(
            s,
            p,
            FeatureKind::Patch {
                sketch: None,
                profiles: ProfileSel::All,
                profiles_3d: Vec::new(),
                body: body.map(BodyName),
                edges: edges.into_iter().map(Position).collect(),
                crown,
                continuity: continuity.clone(),
                points,
            },
        )?
    } else {
        if !continuity.is_g0() || !points.is_empty() {
            return Err(bad(cmd, "`continuity` and `points` are for a patch on a loop of edges"));
        }
        let sketch = feature_sketch(s, p, cmd)?;
        // With only 3D loops asked for, the sketch's own-plane profiles stay untouched.
        let profiles =
            if !profiles_3d.is_empty() && p.get("profiles").is_none() { ProfileSel::Indices { indices: Vec::new() } } else { profiles(p, cmd)? };
        let kind = FeatureKind::Patch {
            sketch: Some(SketchId(sketch)),
            profiles,
            profiles_3d,
            body: None,
            edges: Vec::new(),
            crown,
            continuity: PatchContinuity::default(),
            points,
        };
        add_feature(s, p, kind)?
    };
    if let Some(o) = v.as_object_mut() {
        o.insert("crown".into(), serde_json::json!(crown));
        o.insert("continuity".into(), serde_json::to_value(&continuity).unwrap_or(Value::Null));
    }
    Ok(v)
}

/// Indices of the sketch's planar 3D loops (#741): `profiles_3d: [0, 2]`, as listed by
/// `sketch.inspect`.
fn profiles_3d_param(p: &Value, cmd: &str) -> Result<Vec<usize>> {
    let Some(v) = p.get("profiles_3d") else { return Ok(Vec::new()) };
    let a = v.as_array().ok_or_else(|| bad(cmd, "`profiles_3d` must be a list of indices"))?;
    if a.is_empty() || a.len() > 128 {
        return Err(bad(cmd, "`profiles_3d` needs 1 to 128 indices"));
    }
    a.iter()
        .map(|i| {
            let n = i.as_u64().ok_or_else(|| bad(cmd, "`profiles_3d` indices must be whole numbers"))?;
            usize::try_from(n).map_err(|_| bad(cmd, format!("there is no profile {n}")))
        })
        .collect()
}

/// A continuity word for a patch, within what a patch builds ([`Construction::Patch`]).
fn patch_order(v: Option<&Value>, cmd: &str) -> Result<Continuity> {
    crate::params::continuity(cmd, v, "a patch's continuity", Construction::Patch)
}

/// The patch's `continuity`: one word for every edge, a list of `{edge, g}` for some edges (the
/// rest g0), or `{all, edges}`.
fn patch_continuity(p: &Value, cmd: &str) -> Result<PatchContinuity> {
    let edges = |list: &Value| -> Result<Vec<EdgeContinuity>> {
        let list = list.as_array().ok_or_else(|| bad(cmd, "`continuity` edges must be a list of {edge: [x,y,z], g}"))?;
        if list.len() > 10_000 {
            return Err(bad(cmd, "`continuity` lists at most 10000 edges"));
        }
        list.iter()
            .map(|e| {
                let edge = e.get("edge").and_then(vec3).ok_or_else(|| bad(cmd, "a `continuity` entry's `edge` must be a point [x,y,z]"))?;
                Ok(EdgeContinuity { edge: Position(edge), g: patch_order(e.get("g"), cmd)? })
            })
            .collect()
    };
    match p.get("continuity") {
        None => Ok(PatchContinuity::default()),
        Some(v) if v.is_string() => Ok(PatchContinuity { all: patch_order(Some(v), cmd)?, edges: Vec::new() }),
        Some(v) if v.is_array() => Ok(PatchContinuity { all: Continuity::G0, edges: edges(v)? }),
        Some(v) if v.is_object() => {
            let all = match v.get("all") {
                None => Continuity::G0,
                some => patch_order(some, cmd)?,
            };
            let edges = match v.get("edges") {
                None => Vec::new(),
                Some(list) => edges(list)?,
            };
            Ok(PatchContinuity { all, edges })
        }
        Some(_) => {
            let (lowest, highest) = Construction::Patch.orders();
            Err(bad(cmd, format!("`continuity` must be {}, or a list of {{edge, g}}", Continuity::words(lowest, highest))))
        }
    }
}

/// The points a patch passes through (at most 64, each a finite point).
fn patch_points(p: &Value, cmd: &str) -> Result<Vec<solidcore_record::Vec3>> {
    let Some(v) = p.get("points") else { return Ok(Vec::new()) };
    let list = v
        .as_array()
        .filter(|a| a.len() <= solidcraft_geom::fill_general::MAX_FILL_POINTS)
        .ok_or_else(|| bad(cmd, "`points` must list at most 64 points [x,y,z]"))?;
    list.iter().map(|q| vec3(q).filter(|q| q.is_finite()).ok_or_else(|| bad(cmd, "a patch point must be a finite [x,y,z]"))).collect()
}

/// The patch's `crown`: how full a fitted patch's middle is made, -0.9…3 (default 0). A hostile
/// value (NaN, infinity, out of range, a string) is refused.
fn crown_param(p: &Value, cmd: &str) -> Result<f64> {
    match p.get("crown") {
        None => Ok(0.0),
        Some(v) => match v.as_f64().filter(|x| (solidcraft_geom::fill::MIN_CROWN..=solidcraft_geom::fill::MAX_CROWN).contains(x)) {
            Some(x) => Ok(x),
            None => Err(bad(cmd, "`crown` must be a number between -0.9 and 3")),
        },
    }
}

/// One end of a blend: the body and the picks that chain its edges.
fn blend_side(p: &Value, cmd: &str, key: &str) -> Result<BlendSide> {
    let side = p.get(key).ok_or_else(|| bad(cmd, format!("`{key}` must be {{body, edges}}")))?;
    let body = str_(side, "body").ok_or_else(|| bad(cmd, format!("`{key}.body` must name a body")))?;
    let edges: Vec<solidcore_record::Vec3> =
        side.get("edges").and_then(Value::as_array).map(|a| a.iter().filter_map(vec3).collect()).unwrap_or_default();
    if edges.is_empty() || edges.len() > 10_000 {
        return Err(bad(cmd, format!("`{key}.edges` must list 1 to 10000 points on the edges of a chain")));
    }
    Ok(BlendSide { body: BodyName(body.to_string()), edges: edges.into_iter().map(Position).collect() })
}

/// `surface.blend`: a blend surface bridging two picked edge chains of two bodies.
fn blend(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.blend";
    let from = blend_side(p, cmd, "from")?;
    let to = blend_side(p, cmd, "to")?;
    let continuity = match p.get("continuity") {
        None => [Continuity::G1, Continuity::G1],
        Some(v) => {
            let list = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| bad(cmd, "`continuity` must be two words, one per end"))?;
            let word = |x: Option<&Value>| crate::params::continuity(cmd, x, "a `continuity` entry", Construction::Bridge);
            [word(list.first())?, word(list.get(1))?]
        }
    };
    let bulge = match p.get("bulge") {
        None => [0.0, 0.0],
        Some(v) => {
            let list = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| bad(cmd, "`bulge` must be two numbers, one per end"))?;
            let one = |x: Option<&Value>| -> Result<f64> {
                x.and_then(Value::as_f64)
                    .filter(|x| x.is_finite() && (-0.9..=3.0).contains(x))
                    .ok_or_else(|| bad(cmd, "each `bulge` entry must be between -0.9 (flattest) and 3 (fullest)"))
            };
            [one(list.first())?, one(list.get(1))?]
        }
    };
    let align = point_pair(p, cmd, "align", "one per end")?;
    let seam = point_pair(p, cmd, "seam", "one on each chain")?;
    let stitch = bool_(p, "stitch").unwrap_or(false);
    add_feature(s, p, FeatureKind::SurfaceBlend { from, to, continuity, bulge, align, seam, stitch })
}

/// An optional pair of points `key` (`null` as none): `which` says what each one is.
fn point_pair(p: &Value, cmd: &str, key: &str, which: &str) -> Result<Option<[Position; 2]>> {
    let Some(v) = p.get(key).filter(|v| !v.is_null()) else { return Ok(None) };
    let list = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| bad(cmd, format!("`{key}` must be two points, {which}")))?;
    let point = |x: Option<&Value>| x.and_then(vec3).map(Position).ok_or_else(|| bad(cmd, format!("each `{key}` entry must be a point [x,y,z]")));
    Ok(Some([point(list.first())?, point(list.get(1))?]))
}

fn surface_extrude(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.extrude";
    let sketch = feature_sketch(s, p, cmd)?;
    let (profiles, curves) = sheet_inputs(p, cmd)?;
    let to_next = bool_(p, "to_next").unwrap_or(false);
    let extent = if to_next {
        // To Next takes no length: each wall runs on until it meets a body.
        if let Some(key) = ["to", "to_offset", "distance", "distance2"].into_iter().find(|k| p.get(*k).is_some_and(|v| !v.is_null())) {
            return Err(bad(cmd, format!("`{key}` does not go with `to_next`: each wall runs on until it meets a body")));
        }
        let mut with_distance = p.clone();
        if let Some(o) = with_distance.as_object_mut() {
            o.insert("distance".into(), Value::String("0".into()));
        }
        extent_param(s, &with_distance, cmd)?
    } else {
        extent_param(s, p, cmd)?
    };
    if extent.through_all || extent.taper.is_some() || extent.from.is_some() {
        return Err(bad(cmd, "not supported yet: through all, from an object, or a taper"));
    }
    if to_next && extent.direction == solidcraft_doc::Direction::Symmetric {
        return Err(bad(cmd, "not supported yet: `to_next` to both sides (`direction: symmetric`)"));
    }
    add_feature(s, p, FeatureKind::SurfaceExtrude { sketch: SketchId(sketch), profiles, curves, extent, to_next })
}

/// A body by name, and points on some of its faces (none: all of them).
fn body_faces(s: &Session, p: &Value, cmd: &str) -> Result<(String, Vec<solidcore_record::Vec3>)> {
    let body = str_(p, "body").ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    if s.model.state().body(&body).is_none() {
        return Err(bad(cmd, format!("no body `{body}`")));
    }
    let faces: Vec<solidcore_record::Vec3> =
        p.get("faces").and_then(Value::as_array).map(|a| a.iter().filter_map(vec3).collect()).unwrap_or_default();
    if faces.len() > 10_000 {
        return Err(bad(cmd, "too many faces"));
    }
    Ok((body, faces))
}

fn unstitch(s: &mut Session, p: &Value) -> Result<Value> {
    let (body, faces) = body_faces(s, p, "surface.unstitch")?;
    add_feature(s, p, FeatureKind::Unstitch { body: BodyName(body), faces: faces.into_iter().map(Position).collect() })
}

fn reverse_normal(s: &mut Session, p: &Value) -> Result<Value> {
    let (body, faces) = body_faces(s, p, "surface.reverse_normal")?;
    add_feature(s, p, FeatureKind::ReverseNormal { body: BodyName(body), faces: faces.into_iter().map(Position).collect() })
}

fn untrim(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.untrim";
    let face = p.get("face").ok_or_else(|| bad(cmd, "`face` is required: {body, point}"))?;
    let body = face.get("body").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`face.body` must name a body"))?.to_string();
    if s.model.state().body(&body).is_none() {
        return Err(bad(cmd, format!("no body `{body}`")));
    }
    let point = face.get("point").and_then(vec3).ok_or_else(|| bad(cmd, "`face.point` must be [x, y, z] on the face"))?;
    let holes = match p.get("holes") {
        None | Some(Value::Null) => None,
        Some(Value::String(all)) if all == "all" => None,
        Some(Value::Array(points)) => {
            if points.len() > 10_000 {
                return Err(bad(cmd, "too many holes"));
            }
            let points: Option<Vec<solidcore_record::Vec3>> = points.iter().map(vec3).collect();
            Some(points.ok_or_else(|| bad(cmd, "`holes` must list points [x, y, z] on the inner loops"))?)
        }
        Some(_) => return Err(bad(cmd, "`holes` must be \"all\" or a list of points [x, y, z] on the inner loops")),
    };
    let outer = bool_(p, "outer").unwrap_or(false);
    if !outer && holes.as_ref().is_some_and(Vec::is_empty) {
        return Err(bad(cmd, "nothing to untrim: pick holes to remove or restore the outer boundary"));
    }
    let kind = FeatureKind::SurfaceUntrim {
        face: FaceAt { body: BodyName(body), point: Position(point) },
        holes: holes.map(|h| h.into_iter().map(Position).collect()),
        outer,
        in_place: bool_(p, "in_place").unwrap_or(false),
    };
    add_feature(s, p, kind)
}

/// The profiles and curves a surface builder takes: `curves`, and `profiles` (all of them when
/// neither is given).
fn sheet_inputs(p: &Value, cmd: &str) -> Result<(Option<solidcraft_doc::ProfileSel>, Vec<String>)> {
    let curves = string_list(cmd, p, "curves")?;
    if curves.len() > 1000 {
        return Err(bad(cmd, "at most 1000 curves"));
    }
    let profiles = if curves.is_empty() || p.get("profiles").is_some() { Some(profiles(p, cmd)?) } else { None };
    Ok((profiles, curves))
}

fn surface_revolve(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.revolve";
    let sketch = feature_sketch(s, p, cmd)?;
    let (profiles, curves) = sheet_inputs(p, cmd)?;
    let axis = axis_param(s, sketch, p, cmd)?;
    let angle = expr(p, "angle").unwrap_or_else(|| "360 deg".into());
    check_expr(s, &angle, Kind::Angle, cmd, "angle")?;
    let angle2 = expr(p, "angle2");
    if let Some(a) = &angle2 {
        check_expr(s, a, Kind::Angle, cmd, "angle2")?;
    }
    let symmetric = bool_(p, "symmetric").unwrap_or(false) || str_(p, "direction").is_some_and(|d| d.eq_ignore_ascii_case("symmetric"));
    let faced = match p.get("faced") {
        None | Some(Value::Null) => true,
        Some(v) => v.as_bool().ok_or_else(|| bad(cmd, "`faced` must be true or false"))?,
    };
    add_feature(s, p, FeatureKind::SurfaceRevolve { sketch: SketchId(sketch), profiles, curves, axis, angle, angle2, symmetric, faced })
}

fn surface_sweep(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.sweep";
    let sketch = feature_sketch(s, p, cmd)?;
    let (profiles, curves) = sheet_inputs(p, cmd)?;
    let (path_sketch, path, path_body, path_edges) = super::features::sweep_path(s, p, cmd)?;
    // A surface sweep's path is the curves of one sketch (its own format keeps plain ids);
    // a curve of another sketch is refused.
    let path = path
        .into_iter()
        .map(|e| match e {
            solidcraft_doc::PathEntry::OfPathSketch(id) => Ok(id),
            solidcraft_doc::PathEntry::OfSketch { sketch, curve } if sketch.0 == path_sketch => Ok(curve),
            _ => Err(bad(cmd, "the surface sweep's path curves must be in one sketch")),
        })
        .collect::<Result<Vec<_>>>()?;
    if path_body.is_none() && (path.is_empty() || path.len() > 1000) {
        return Err(bad(cmd, "`path` must list 1…1000 curve ids"));
    }
    let (orientation, twist, scale) = super::features::sweep_options(s, p, cmd)?;
    let (rails, rail_fit) = super::features::sweep_rails(s, p, cmd, path_body.is_none().then_some(path_sketch))?;
    add_feature(
        s,
        p,
        FeatureKind::SurfaceSweep {
            sketch: SketchId(sketch),
            profiles,
            curves,
            path_sketch: SketchId(path_sketch),
            path,
            path_body,
            path_edges,
            orientation,
            twist,
            scale,
            rails,
            rail_fit,
            spline_as_samples: false,
        },
    )
}

fn surface_loft(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.loft";
    let given = p.get("sections").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`sections` must list the sections in order"))?;
    if !(2..=50).contains(&given.len()) {
        return Err(bad(cmd, "a loft needs 2…50 sections"));
    }
    let mut sections = Vec::with_capacity(given.len());
    for sec in given {
        // A section is a chain of sketch curves, or an edge chain of a body whose face the end of
        // the loft repeats.
        if let Some(body) = str_(sec, "body") {
            let edges: Vec<solidcore_record::Vec3> =
                sec.get("edges").and_then(Value::as_array).map(|a| a.iter().filter_map(vec3).collect()).unwrap_or_default();
            if edges.is_empty() || edges.len() > 10_000 {
                return Err(bad(cmd, "`sections.edges` must list 1 to 10000 points on the edges of a chain"));
            }
            if sec.get("sketch").is_some() {
                return Err(bad(cmd, "a section is either a sketch chain or a body edge chain"));
            }
            sections.push(solidcraft_doc::SheetSection {
                sketch: SketchId(0),
                profiles: None,
                curves: Vec::new(),
                body: Some(BodyName::from(body.to_string())),
                edges: edges.into_iter().map(Position).collect(),
            });
            continue;
        }
        let sketch = sketch_id(s, sec.get("sketch"), cmd, "sections.sketch")?;
        let curves = string_list(cmd, sec, "curves")?;
        if curves.len() > 1000 {
            return Err(bad(cmd, "at most 1000 curves a section"));
        }
        let profiles = if sec.get("profiles").is_some() { Some(profiles(sec, cmd)?) } else { None };
        sections.push(solidcraft_doc::SheetSection { sketch: SketchId(sketch), profiles, curves, body: None, edges: Vec::new() });
    }
    let continuity = match p.get("continuity") {
        None => [Continuity::G0, Continuity::G0],
        Some(v) => {
            let list = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| bad(cmd, "`continuity` must be two words, one per end"))?;
            let word = |x: Option<&Value>| crate::params::continuity(cmd, x, "a `continuity` entry", Construction::Bridge);
            [word(list.first())?, word(list.get(1))?]
        }
    };
    let crown = match p.get("crown") {
        None | Some(Value::Null) => [0.0, 0.0],
        Some(v) => {
            let list = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| bad(cmd, "`crown` must be two numbers, one per end"))?;
            let one = |x: Option<&Value>| -> Result<f64> {
                x.and_then(Value::as_f64)
                    .filter(|x| x.is_finite() && (-0.9..=3.0).contains(x))
                    .ok_or_else(|| bad(cmd, "each `crown` entry must be between -0.9 (flattest) and 3 (fullest)"))
            };
            [one(list.first())?, one(list.get(1))?]
        }
    };
    let (rails, rail_fit) = super::features::guide_rails(s, p, cmd, "loft", None)?;
    let faced = match p.get("faced") {
        None | Some(Value::Null) => true,
        Some(v) => v.as_bool().ok_or_else(|| bad(cmd, "`faced` must be true or false"))?,
    };
    add_feature(s, p, FeatureKind::SurfaceLoft { sections, continuity, crown, rails, rail_fit, faced })
}

fn offset(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.offset";
    let faces: Vec<solidcore_record::Vec3> =
        p.get("faces").and_then(Value::as_array).map(|a| a.iter().filter_map(vec3).collect()).unwrap_or_default();
    if faces.is_empty() || faces.len() > 10_000 {
        return Err(bad(cmd, "`faces` must list points on the faces to offset"));
    }
    let distance = expr(p, "distance").ok_or_else(|| bad(cmd, "`distance` is required"))?;
    check_expr(s, &distance, Kind::Length, cmd, "distance")?;
    let tolerance = expr(p, "tolerance");
    if let Some(t) = &tolerance {
        check_expr(s, t, Kind::Length, cmd, "tolerance")?;
    }
    let kind = FeatureKind::SurfaceOffset {
        faces: faces.into_iter().map(FacePick).collect(),
        body: str_(p, "body").map(BodyName::from),
        distance,
        chain: bool_(p, "chain").unwrap_or(false),
        tolerance,
    };
    add_feature(s, p, kind)
}

fn ruled(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.ruled";
    let body = str_(p, "body").ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    if s.model.state().body(&body).is_none() {
        return Err(bad(cmd, format!("no body `{body}`")));
    }
    let edges: Vec<solidcore_record::Vec3> =
        p.get("edges").and_then(Value::as_array).map(|a| a.iter().filter_map(vec3).collect()).unwrap_or_default();
    if edges.is_empty() || edges.len() > 10_000 {
        return Err(bad(cmd, "`edges` must list points on a chain of the body's edges"));
    }
    let distance = expr(p, "distance").ok_or_else(|| bad(cmd, "`distance` is required"))?;
    check_expr(s, &distance, Kind::Length, cmd, "distance")?;
    let mode = match str_(p, "mode").map(str::to_ascii_lowercase).as_deref() {
        None | Some("normal") => RuledMode::Normal,
        Some("tangent") => RuledMode::Tangent,
        Some("angle") => RuledMode::Angle,
        Some("direction") => RuledMode::Direction,
        Some(o) => return Err(bad(cmd, format!("unknown mode `{o}` (normal, tangent, angle or direction)"))),
    };
    let angle = match mode {
        RuledMode::Angle => {
            let a = expr(p, "angle").ok_or_else(|| bad(cmd, "`angle` is required with mode angle"))?;
            check_expr(s, &a, Kind::Angle, cmd, "angle")?;
            Some(a)
        }
        _ => None,
    };
    let direction = match (mode, p.get("direction").filter(|v| !v.is_null())) {
        (RuledMode::Angle | RuledMode::Direction, Some(v)) => {
            let d = vec3(v).filter(|d| d.len() > 1e-12).ok_or_else(|| bad(cmd, "`direction` must be a non-zero [x, y, z]"))?;
            Some(d)
        }
        (RuledMode::Direction, None) => return Err(bad(cmd, "`direction` is required with mode direction")),
        _ => None,
    };
    let corners = match str_(p, "corners").map(str::to_ascii_lowercase).as_deref() {
        None | Some("mitre") | Some("miter") => RuledCorners::Mitre,
        Some("round") => RuledCorners::Round,
        Some(o) => return Err(bad(cmd, format!("unknown corners `{o}` (mitre or round)"))),
    };
    let face = match p.get("face").filter(|v| !v.is_null()) {
        Some(v) => Some(vec3(v).ok_or_else(|| bad(cmd, "`face` must be [x, y, z] on a face"))?),
        None => None,
    };
    // A free strip's far-curve handles, per chain edge: a list of points, or null for the default.
    let far = match p.get("far").filter(|v| !v.is_null()) {
        None => Vec::new(),
        Some(v) => {
            let lists =
                v.as_array().filter(|a| a.len() <= 10_000).ok_or_else(|| bad(cmd, "`far` must be a list, per edge, of null or [[x, y, z], ...]"))?;
            lists
                .iter()
                .map(|h| match h {
                    Value::Null => Ok(None),
                    Value::Array(pts) if pts.len() <= 100_000 => pts
                        .iter()
                        .map(|q| vec3(q).map(Position).ok_or_else(|| bad(cmd, "`far` handles must be [x, y, z]")))
                        .collect::<Result<Vec<_>>>()
                        .map(Some),
                    _ => Err(bad(cmd, "`far` must be a list, per edge, of null or [[x, y, z], ...]")),
                })
                .collect::<Result<Vec<_>>>()?
        }
    };
    let kind = FeatureKind::SurfaceRuled {
        body: BodyName(body),
        edges: edges.into_iter().map(Position).collect(),
        distance,
        mode,
        angle,
        direction: direction.map(Dir),
        flip: bool_(p, "flip").unwrap_or(false),
        corners,
        face: face.map(FacePick),
        far,
    };
    add_feature(s, p, kind)
}

fn bodies_param(s: &Session, p: &Value, cmd: &str) -> Result<Vec<String>> {
    let bodies = string_list(cmd, p, "bodies")?;
    if bodies.is_empty() {
        return Err(bad(cmd, "`bodies` must list bodies"));
    }
    let st = s.model.state();
    if let Some(b) = bodies.iter().find(|b| st.body(b).is_none()) {
        return Err(bad(cmd, format!("no body `{b}`")));
    }
    Ok(bodies)
}

fn stitch(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.stitch";
    let bodies = bodies_param(s, p, cmd)?;
    let tolerance = expr(p, "tolerance").unwrap_or_else(|| "0".into());
    check_expr(s, &tolerance, Kind::Length, cmd, "tolerance")?;
    add_feature(s, p, FeatureKind::Stitch { bodies: bodies.into_iter().map(BodyName).collect(), tolerance })
}

fn thicken(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.thicken";
    let bodies = bodies_param(s, p, cmd)?;
    let thickness = expr(p, "thickness").ok_or_else(|| bad(cmd, "`thickness` is required"))?;
    check_expr(s, &thickness, Kind::Length, cmd, "thickness")?;
    let kind = FeatureKind::Thicken {
        bodies: bodies.into_iter().map(BodyName).collect(),
        thickness,
        symmetric: bool_(p, "symmetric").unwrap_or(false),
        operation: operation(p, cmd)?,
        targets: string_list(cmd, p, "targets")?.into_iter().map(BodyName).collect(),
    };
    add_feature(s, p, kind)
}

fn extend(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.extend";
    let body = str_(p, "body").ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    if s.model.state().body(&body).is_none() {
        return Err(bad(cmd, format!("no body `{body}`")));
    }
    let edges: Vec<solidcore_record::Vec3> =
        p.get("edges").and_then(Value::as_array).map(|a| a.iter().filter_map(vec3).collect()).unwrap_or_default();
    if edges.is_empty() || edges.len() > 10_000 {
        return Err(bad(cmd, "`edges` must list points on the edges to extend"));
    }
    let mode = match str_(p, "mode").map(str::trim) {
        None | Some("natural") => ExtendMode::Natural,
        Some("tangent") => ExtendMode::Tangent,
        Some("curvature") => ExtendMode::Curvature,
        Some(m) => return Err(bad(cmd, format!("`mode` must be natural, tangent or curvature, not `{m}`"))),
    };
    let up_to = match p.get("up_to").filter(|v| !v.is_null()) {
        None => None,
        Some(Value::String(b)) if b.trim() == "bounds" => Some(ExtendTo::Bounds),
        Some(t @ Value::Object(o)) if o.contains_key("body") => {
            let tb = t.get("body").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`up_to.body` must name a body"))?.to_string();
            if s.model.state().body(&tb).is_none() {
                return Err(bad(cmd, format!("no body `{tb}`")));
            }
            let point = t.get("point").and_then(vec3).ok_or_else(|| bad(cmd, "`up_to.point` must be [x, y, z] on the target face"))?;
            Some(ExtendTo::Face(FaceAt { body: BodyName(tb), point: Position(point) }))
        }
        Some(v) => Some(ExtendTo::Plane(plane_param(s, Some(v), cmd)?)),
    };
    if up_to == Some(ExtendTo::Bounds) && mode != ExtendMode::Natural {
        return Err(bad(cmd, "only a natural extension goes back to the surface's bounds"));
    }
    let distance = match (expr(p, "distance"), &up_to) {
        (Some(d), _) => d,
        (None, Some(_)) => "0 mm".into(),
        (None, None) => return Err(bad(cmd, "`distance` is required (or `up_to`)")),
    };
    check_expr(s, &distance, Kind::Length, cmd, "distance")?;
    let neighbours = bool_(p, "neighbours").unwrap_or(true);
    add_feature(
        s,
        p,
        FeatureKind::SurfaceExtend { body: BodyName(body), edges: edges.into_iter().map(Position).collect(), distance, mode, up_to, neighbours },
    )
}

/// Points to keep: one `[x, y, z]`, or a list of them (at most 10 000).
fn keep_points(v: Option<&Value>, cmd: &str, key: &str) -> Result<Vec<solidcore_record::Vec3>> {
    let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    if let Some(p) = vec3(v) {
        return Ok(vec![p]);
    }
    let list = v
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 10_000)
        .ok_or_else(|| bad(cmd, format!("`{key}` must be [x, y, z] or a list of them")))?;
    list.iter().map(|q| vec3(q).ok_or_else(|| bad(cmd, format!("`{key}` must be [x, y, z] or a list of them")))).collect()
}

fn trim(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.trim";
    let body = str_(p, "body").ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    if s.model.state().body(&body).is_none() {
        return Err(bad(cmd, format!("no body `{body}`")));
    }
    let picks = keep_points(p.get("keep"), cmd, "keep")?;
    let Some((&keep, more)) = picks.split_first() else {
        return Err(bad(cmd, "`keep` must be [x, y, z] on the side to keep (or a list of points on the regions to keep)"));
    };
    let keep_more = more.to_vec();
    let given = |k: &str| p.get(k).is_some_and(|v| !v.is_null());
    let xy = || PlaneRef::Origin { name: "XY".into() };
    if let Some(w) = p.get("with").filter(|v| !v.is_null()) {
        let with = w.as_str().ok_or_else(|| bad(cmd, "`with` must name a surface body"))?.to_string();
        if with == body {
            return Err(bad(cmd, "`with` must be another surface body"));
        }
        if s.model.state().body(&with).is_none() {
            return Err(bad(cmd, format!("no body `{with}`")));
        }
        if given("plane") || given("tool") || given("curves") {
            return Err(bad(cmd, "a mutual trim cuts each surface with the other: leave out `plane`, `tool` and `curves`"));
        }
        let keep_with = keep_points(p.get("keep_with"), cmd, "keep_with")?;
        if keep_with.is_empty() {
            return Err(bad(cmd, "`keep_with` must be [x, y, z] on the side of the other surface to keep"));
        }
        let (stitch, extend) = (bool_(p, "stitch").unwrap_or(false), bool_(p, "extend").unwrap_or(false));
        let kind = FeatureKind::SurfaceTrim {
            body: BodyName(body),
            plane: xy(),
            tool: None,
            keep: Position(keep),
            keep_more: keep_more.into_iter().map(Position).collect(),
            curves: None,
            with: Some(BodyName(with)),
            keep_with: keep_with.into_iter().map(Position).collect(),
            stitch,
            extend,
        };
        return add_feature(s, p, kind);
    }
    if given("keep_with") || given("stitch") || given("extend") {
        return Err(bad(cmd, "`keep_with`, `stitch` and `extend` belong to a mutual trim (`with`)"));
    }
    let (plane, tool, curves) = match (p.get("tool").filter(|t| !t.is_null()), p.get("curves").filter(|c| !c.is_null())) {
        (Some(_), Some(_)) => return Err(bad(cmd, "give `tool` or `curves`, not both")),
        (Some(t), None) => {
            let tb = t.get("body").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`tool.body` must name a body"))?.to_string();
            let point = t.get("point").and_then(vec3).ok_or_else(|| bad(cmd, "`tool.point` must be [x, y, z] on the tool face"))?;
            (xy(), Some(FaceAt { body: BodyName(tb), point: Position(point) }), None)
        }
        (None, Some(c)) => {
            let sketch = sketch_id(s, c.get("sketch"), cmd, "curves.sketch")?;
            let ids = match c.get("curves").filter(|v| !v.is_null()) {
                None => Vec::new(),
                Some(v) => {
                    let a = v.as_array().filter(|a| a.len() <= 1000).ok_or_else(|| bad(cmd, "`curves.curves` must list curve ids"))?;
                    a.iter()
                        .map(|x| x.as_str().map(str::to_string).ok_or_else(|| bad(cmd, "`curves.curves` must list curve ids")))
                        .collect::<Result<Vec<_>>>()?
                }
            };
            let (direction, face_normal) = match c.get("direction").filter(|v| !v.is_null()) {
                None => (None, false),
                Some(Value::String(d)) if d.trim() == "normal" => (None, false),
                Some(Value::String(d)) if d.trim() == "face_normal" => (None, true),
                Some(v) => {
                    let d = vec3(v).ok_or_else(|| bad(cmd, "`curves.direction` must be normal, face_normal or [x, y, z]"))?;
                    if !(d.len() > 1e-12) {
                        return Err(bad(cmd, "`curves.direction` must not be zero"));
                    }
                    (Some(d), false)
                }
            };
            (xy(), None, Some(TrimCurves { sketch: SketchId(sketch), curves: ids, direction: direction.map(Dir), face_normal }))
        }
        (None, None) => (plane_param(s, p.get("plane"), cmd)?, None, None),
    };
    let kind = FeatureKind::SurfaceTrim {
        body: BodyName(body),
        plane,
        tool,
        keep: Position(keep),
        keep_more: keep_more.into_iter().map(Position).collect(),
        curves,
        with: None,
        keep_with: Vec::new(),
        stitch: false,
        extend: false,
    };
    add_feature(s, p, kind)
}

/// `surface.fair`: the B-spline faces of a body faired in place. The tolerance a caller leaves
/// out is a thousandth of the body's own size, so a fairing run can never make a design jump by
/// more than a hair of itself.
fn fair(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.fair";
    let (body, faces) = body_faces(s, p, cmd)?;
    let size = {
        let state = s.model.state();
        let Some(mb) = state.body(&body) else { return Err(bad(cmd, format!("no body `{body}`"))) };
        mb.body.size()
    };
    let strength = num(p, "strength").unwrap_or(1.0);
    if !(strength.is_finite() && strength > 0.0 && strength <= 1.0) {
        return Err(bad(cmd, "`strength` must be greater than 0 and at most 1 (the whole fairing step)"));
    }
    let steps = num(p, "iterations").unwrap_or(1.0);
    if !(steps.is_finite() && (1.0..=1000.0).contains(&steps)) {
        return Err(bad(cmd, "`iterations` must be 1 to 1000"));
    }
    let keep_edges = bool_(p, "keep_edges").unwrap_or(true);
    let tolerance = expr(p, "tolerance").unwrap_or_else(|| Expr::new(format!("{:.6}", (size * 1e-3).max(1e-6))));
    check_expr(s, tolerance.as_str(), Kind::Length, cmd, "tolerance")?;
    let kind = FeatureKind::SurfaceFair {
        body: BodyName::from(body),
        faces: faces.into_iter().map(FacePick).collect(),
        strength,
        iterations: steps as u32,
        keep_edges,
        tolerance,
    };
    add_feature(s, p, kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Session;
    use serde_json::json;

    fn run(s: &mut Session, c: &str, p: Value) -> Value {
        s.execute(c, &p).unwrap_or_else(|e| panic!("{c}: {e}"))
    }
    fn volume(s: &Session, name: &str) -> f64 {
        let st = s.world_state().unwrap();
        solidcraft_kernel::measure(&st.body(name).unwrap().body).unwrap().volume
    }

    fn area(s: &Session, name: &str) -> f64 {
        solidcraft_kernel::measure(&s.world_state().unwrap().body(name).unwrap().body).unwrap().area
    }

    /// Surface Extrude of open curves: a line and an arc joined end to end (listed in either
    /// order) make one sheet of two walls; both sides of the sketch; undo, redo and reopening
    /// give the same sheet.
    #[test]
    fn surface_extrude_open_curves() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "ids": ["l1"]}));
        let arc = run(&mut s, "sketch.arc.three_point", json!({"start": [10, 0], "end": [20, 0], "through": [15, 5]}))["created"]["curves"][0]
            .as_str()
            .unwrap()
            .to_string();
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.extrude", json!({"sketch": "S", "curves": [arc, "l1"], "distance": 4, "body_name": "Wall"}));
        let st = s.world_state().unwrap();
        let wall = &st.body("Wall").unwrap().body;
        assert!(wall.is_surface() && wall.face_count() == 2);
        // The line's wall and the half circle's (radius 5): length × height.
        let want = (10.0 + std::f64::consts::PI * 5.0) * 4.0;
        assert!((area(&s, "Wall") - want).abs() < want * 1e-3, "{} vs {want}", area(&s, "Wall"));
        // Symmetric: the distance is the total, split evenly around the sketch plane.
        run(&mut s, "surface.extrude", json!({"sketch": "S", "curves": ["l1"], "distance": 4, "direction": "symmetric", "body_name": "Both"}));
        let bb = solidcraft_kernel::measure(&s.world_state().unwrap().body("Both").unwrap().body).unwrap().bbox;
        assert!((bb.min.z + 2.0).abs() < 1e-9 && (bb.max.z - 2.0).abs() < 1e-9, "{bb:?}");
        run(&mut s, "edit.undo", json!({}));
        assert!(s.world_state().unwrap().body("Both").is_none());
        run(&mut s, "edit.redo", json!({}));
        assert!(s.world_state().unwrap().body("Both").is_some());
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert!((area(&cold, "Wall") - area(&s, "Wall")).abs() < 1e-9);
        // Refused: no such curve, a zero distance, and an extent a sheet does not take yet.
        assert!(s.execute("surface.extrude", &json!({"sketch": "S", "curves": ["nope"], "distance": 4})).is_err());
        assert!(s.execute("surface.extrude", &json!({"sketch": "S", "curves": ["l1"], "distance": 0})).is_err());
        assert!(s.execute("surface.extrude", &json!({"sketch": "S", "curves": ["l1"], "distance": 4, "through_all": true})).is_err());
    }

    /// Surface Extrude of a profile gives its walls alone; patching both ends and stitching
    /// makes the solid that solid.extrude makes of the same profile.
    /// To Object (#16537): a surface extrude ends at a flat face square to it (and a To offset
    /// past it), or is trimmed where it meets a curved face's surface. The expected values are
    /// worked out by hand from the shapes, not taken from a run.
    #[test]
    fn surface_extrude_to_object() {
        let mut s = Session::default();
        // A box from z = 10 to 12, and a cylinder of radius 5 along X, axis at z = 20 (its
        // underside at z = 15, clear of the box).
        run(&mut s, "solid.box", json!({"corner": [-5, -5, 10], "length": 10, "width": 10, "height": 2, "body_name": "Lid"}));
        run(&mut s, "solid.cylinder", json!({"base": [-10, 0, 20], "axis": "X", "radius": 5, "height": 20, "body_name": "Rod"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.line", json!({"points": [[-3, 0], [3, 0]], "ids": ["along"]}));
        run(&mut s, "sketch.line", json!({"points": [[0, -3], [0, 3]], "ids": ["across"]}));
        run(&mut s, "sketch.line", json!({"points": [[0, 20], [0, 26]], "ids": ["aside"]}));
        run(&mut s, "sketch.finish", json!({}));
        let bounds = |s: &Session, name: &str| solidcraft_kernel::measure(&s.world_state().unwrap().body(name).unwrap().body).unwrap().bbox;

        // To the box's bottom face: a 6 wide wall 10 high.
        run(&mut s, "surface.extrude", json!({"sketch": "S", "curves": ["along"], "to": [0, 0, 10], "body_name": "ToLid"}));
        assert!((area(&s, "ToLid") - 60.0).abs() < 60.0 * 1e-6, "{}", area(&s, "ToLid"));
        assert!((bounds(&s, "ToLid").max.z - 10.0).abs() < 1e-6);
        // 2 mm past it, in the direction travelled. Picked at (2, 2, 10), off ToLid's top edge
        // (y = 0), so the point names the box's face alone.
        run(&mut s, "surface.extrude", json!({"sketch": "S", "curves": ["along"], "to": [2, 2, 10], "to_offset": "2 mm", "body_name": "PastLid"}));
        assert!((bounds(&s, "PastLid").max.z - 12.0).abs() < 1e-6);

        // To the rod's curved underside: the wall at x = 0 for |y| <= 3 ends at
        // z = 20 - sqrt(25 - y^2). Area: 6 * 20 - (3 * 4 + 25 * asin(3/5)).
        run(&mut s, "surface.extrude", json!({"sketch": "S", "curves": ["across"], "to": [0, 0, 15], "body_name": "ToRod"}));
        let want = 120.0 - (12.0 + 25.0 * (0.6f64).asin());
        assert!((area(&s, "ToRod") - want).abs() < want * 1e-4, "{} vs {want}", area(&s, "ToRod"));
        // Its top runs from z = 15 in the middle to z = 16 at the ends; nothing inside or past the rod is kept.
        let bb = bounds(&s, "ToRod");
        assert!(bb.min.z.abs() < 1e-6 && (bb.max.z - 16.0).abs() < 1e-4, "{bb:?}");

        // From a sketch 5 above the origin: the same wall, 5 shorter. Area: 6 * 15 - (3 * 4 + 25 * asin(3/5)).
        // From here on the rod is picked at (5, 0, 15), on its underside but off ToRod's top edge
        // (x = 0), which runs through (0, 0, 15) and would hold that point as well.
        run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 5, "name": "Raised"}));
        run(&mut s, "sketch.create", json!({"plane": "Raised", "name": "Up"}));
        run(&mut s, "sketch.line", json!({"points": [[0, -3], [0, 3]], "ids": ["raised"]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.extrude", json!({"sketch": "Up", "curves": ["raised"], "to": [5, 0, 15], "body_name": "FromRaised"}));
        let want = 90.0 - (12.0 + 25.0 * (0.6f64).asin());
        assert!((area(&s, "FromRaised") - want).abs() < want * 1e-4, "{} vs {want}", area(&s, "FromRaised"));
        let bb = bounds(&s, "FromRaised");
        assert!((bb.min.z - 5.0).abs() < 1e-6 && (bb.max.z - 16.0).abs() < 1e-4, "{bb:?}");

        // A slanted face meets a wall far beside its body: a cube of side 2 at (0, 40, 10) turned
        // 45 degrees about Y has a face on the plane z = x + 10 - sqrt 2, which the wall from
        // x = 50 to 56 meets at z = 58.6 to 64.6, well past the cube.
        // Area: the integral of x + 10 - sqrt 2 from 50 to 56 = 318 + 60 - 6 sqrt 2.
        run(&mut s, "solid.box", json!({"center": [0, 40, 10], "length": 2, "width": 2, "height": 2, "body_name": "Tilted"}));
        run(&mut s, "solid.move", json!({"bodies": ["Tilted"], "axis": [0, 1, 0], "angle": "45 deg", "origin": [0, 40, 10]}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Far"}));
        run(&mut s, "sketch.line", json!({"points": [[50, 0], [56, 0]], "ids": ["far"]}));
        run(&mut s, "sketch.finish", json!({}));
        let h = std::f64::consts::FRAC_1_SQRT_2;
        run(&mut s, "surface.extrude", json!({"sketch": "Far", "curves": ["far"], "to": [h, 40, 10.0 - h], "body_name": "ToSlant"}));
        let want = 378.0 - 6.0 * std::f64::consts::SQRT_2;
        assert!((area(&s, "ToSlant") - want).abs() < want * 1e-6, "{} vs {want}", area(&s, "ToSlant"));
        // Further still, past any fixed number of tries: the wall from x = 7000 to 7006 meets the
        // plane at z = 7008.6 to 7014.6. Area: 6 (7003 + 10 - sqrt 2) = 42078 - 6 sqrt 2.
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "VeryFar"}));
        run(&mut s, "sketch.line", json!({"points": [[7000, 0], [7006, 0]], "ids": ["very_far"]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.extrude", json!({"sketch": "VeryFar", "curves": ["very_far"], "to": [h, 40, 10.0 - h], "body_name": "ToSlantFar"}));
        let want = 42078.0 - 6.0 * std::f64::consts::SQRT_2;
        assert!((area(&s, "ToSlantFar") - want).abs() < want * 1e-6, "{} vs {want}", area(&s, "ToSlantFar"));

        // The design keeps it: a cold rebuild gives the same walls.
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert!((area(&cold, "ToRod") - area(&s, "ToRod")).abs() < 1e-9);
        assert!((area(&cold, "PastLid") - area(&s, "PastLid")).abs() < 1e-9);

        // Refused, with the reason: a To offset past a curved face, and a wall beside the rod
        // that never meets its surface. Neither adds a feature.
        let features = s.doc.features.len();
        let err =
            s.execute("surface.extrude", &json!({"sketch": "S", "curves": ["across"], "to": [5, 0, 15], "to_offset": 1})).unwrap_err().to_string();
        assert!(err.contains("To offset"), "{err}");
        let err = s.execute("surface.extrude", &json!({"sketch": "S", "curves": ["aside"], "to": [5, 0, 15]})).unwrap_err().to_string();
        assert!(err.contains("never meets"), "{err}");
        assert_eq!(s.doc.features.len(), features);
    }

    /// To Next (#16537): each wall runs on until it first meets a body, any face of a solid or
    /// surface body as it is, with no pick. The expected values are worked out by hand from the
    /// shapes, not taken from a run.
    #[test]
    fn surface_extrude_to_next() {
        let mut s = Session::default();
        // A box from z = 10 to 12 over |x|, |y| <= 5; under it, a box from z = -8 to -7; a rod of
        // radius 5 along X with its axis at y = 40, z = 20; and a flat surface body at z = 6
        // over x -5 to 5, y -10 to -6.
        run(&mut s, "solid.box", json!({"corner": [-5, -5, 10], "length": 10, "width": 10, "height": 2, "body_name": "Lid"}));
        run(&mut s, "solid.box", json!({"corner": [-5, -5, -8], "length": 10, "width": 10, "height": 1, "body_name": "Floor"}));
        run(&mut s, "solid.cylinder", json!({"base": [-10, 40, 20], "axis": "X", "radius": 5, "height": 20, "body_name": "Rod"}));
        run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 6, "name": "Mid"}));
        run(&mut s, "sketch.create", json!({"plane": "Mid", "name": "CoverSketch"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-5, -10], "p1": [5, -6]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.patch", json!({"sketch": "CoverSketch", "body_name": "Cover"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        // Each case its own line: a wall made earlier is in the way of the later ones too.
        run(&mut s, "sketch.line", json!({"points": [[-3, 0], [3, 0]], "ids": ["along"]}));
        run(&mut s, "sketch.line", json!({"points": [[-3, 2], [3, 2]], "ids": ["down"]}));
        run(&mut s, "sketch.line", json!({"points": [[-3, -2], [3, -2]], "ids": ["raised"]}));
        run(&mut s, "sketch.line", json!({"points": [[0, 37], [0, 43]], "ids": ["at_rod"]}));
        run(&mut s, "sketch.line", json!({"points": [[-3, -8], [3, -8]], "ids": ["under_cover"]}));
        run(&mut s, "sketch.line", json!({"points": [[100, 0], [106, 0]], "ids": ["aside"]}));
        run(&mut s, "sketch.line", json!({"points": [[3, 3], [9, 3]], "ids": ["half_under"]}));
        run(&mut s, "sketch.finish", json!({}));
        let bounds = |s: &Session, name: &str| solidcraft_kernel::measure(&s.world_state().unwrap().body(name).unwrap().body).unwrap().bbox;

        // Up to the lid's underside: a 6 wide wall 10 high, saved as To Next.
        run(&mut s, "surface.extrude", json!({"sketch": "S", "curves": ["along"], "to_next": true, "name": "UpToLid", "body_name": "ToLid"}));
        assert!((area(&s, "ToLid") - 60.0).abs() < 60.0 * 1e-6, "{}", area(&s, "ToLid"));
        assert!((bounds(&s, "ToLid").max.z - 10.0).abs() < 1e-6);
        let FeatureKind::SurfaceExtrude { to_next, .. } = &s.doc.find_feature("UpToLid").unwrap().kind else { panic!("surface extrude") };
        assert!(*to_next);
        // Flipped: down to the floor's top at z = -7, 6 x 7.
        run(&mut s, "surface.extrude", json!({"sketch": "S", "curves": ["down"], "to_next": true, "direction": "negative", "body_name": "ToFloor"}));
        assert!((area(&s, "ToFloor") - 42.0).abs() < 42.0 * 1e-6, "{}", area(&s, "ToFloor"));
        assert!((bounds(&s, "ToFloor").min.z + 7.0).abs() < 1e-6);
        // From 2 above the sketch: 6 x 8.
        run(
            &mut s,
            "surface.extrude",
            json!({"sketch": "S", "curves": ["raised"], "to_next": true, "start_offset": 2, "name": "FromRaised", "body_name": "Raised"}),
        );
        assert!((area(&s, "Raised") - 48.0).abs() < 48.0 * 1e-6, "{}", area(&s, "Raised"));
        // Edited as the window does, naming no start offset: it keeps the stored 2, not the
        // sketch plane (which would give 6 x 10 = 60). Naming one replaces it: from 4, 6 x 6.
        let edit = json!({"sketch": "S", "curves": ["raised"], "to_next": true, "body_name": "Raised"});
        run(&mut s, "timeline.redefine", json!({"feature": "FromRaised", "command": "surface.extrude", "params": edit}));
        assert!((area(&s, "Raised") - 48.0).abs() < 48.0 * 1e-6, "{}", area(&s, "Raised"));
        let mut edit = edit;
        edit["start_offset"] = json!(4);
        run(&mut s, "timeline.redefine", json!({"feature": "FromRaised", "command": "surface.extrude", "params": edit}));
        assert!((area(&s, "Raised") - 36.0).abs() < 36.0 * 1e-6, "{}", area(&s, "Raised"));
        edit["start_offset"] = json!(2);
        run(&mut s, "timeline.redefine", json!({"feature": "FromRaised", "command": "surface.extrude", "params": edit}));
        // A surface body stops it as a solid does: up to the cover at z = 6, 6 x 6.
        run(&mut s, "surface.extrude", json!({"sketch": "S", "curves": ["under_cover"], "to_next": true, "body_name": "ToCover"}));
        assert!((area(&s, "ToCover") - 36.0).abs() < 36.0 * 1e-6, "{}", area(&s, "ToCover"));
        // Up to the rod's curved underside: the wall at x = 0 for |y - 40| <= 3 ends at
        // z = 20 - sqrt(25 - (y - 40)^2). Area: 6 * 20 - (3 * 4 + 25 * asin(3/5)).
        run(&mut s, "surface.extrude", json!({"sketch": "S", "curves": ["at_rod"], "to_next": true, "body_name": "ToRod"}));
        let want = 120.0 - (12.0 + 25.0 * (0.6f64).asin());
        assert!((area(&s, "ToRod") - want).abs() < want * 1e-4, "{} vs {want}", area(&s, "ToRod"));
        let bb = bounds(&s, "ToRod");
        assert!(bb.min.z.abs() < 1e-6 && (bb.max.z - 16.0).abs() < 1e-4, "{bb:?}");

        // The design keeps it: a cold rebuild gives the same walls.
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert!((area(&cold, "ToLid") - area(&s, "ToLid")).abs() < 1e-9);
        assert!((area(&cold, "ToRod") - area(&s, "ToRod")).abs() < 1e-9);

        // Refused, with the reason, and no feature added: a wall that meets no body; one that
        // runs on past the side of the lid, which covers only part of it; both sides; and a
        // length given with To Next.
        let features = s.doc.features.len();
        let refused = |s: &mut Session, p: Value| s.execute("surface.extrude", &p).unwrap_err().to_string();
        let e = refused(&mut s, json!({"sketch": "S", "curves": ["aside"], "to_next": true}));
        assert!(e.contains("meets no body"), "{e}");
        let e = refused(&mut s, json!({"sketch": "S", "curves": ["half_under"], "to_next": true}));
        assert!(e.contains("not supported yet") || e.contains("meets no body"), "{e}");
        let e = refused(&mut s, json!({"sketch": "S", "curves": ["along"], "to_next": true, "direction": "symmetric"}));
        assert!(e.contains("not supported yet"), "{e}");
        for key in ["distance", "to_offset", "distance2"] {
            let mut p = json!({"sketch": "S", "curves": ["along"], "to_next": true});
            p[key] = json!(5);
            let e = refused(&mut s, p);
            assert!(e.contains(key), "{e}");
        }
        let e = refused(&mut s, json!({"sketch": "S", "curves": ["along"], "to_next": true, "to": [0, 0, 10]}));
        assert!(e.contains("`to`"), "{e}");
        assert_eq!(s.doc.features.len(), features);
    }

    #[test]
    fn surface_extrude_profile_walls_stitch_into_the_solid() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "R"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 6]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.extrude", json!({"sketch": "R", "distance": 3, "body_name": "Walls"}));
        let st = s.world_state().unwrap();
        let walls = &st.body("Walls").unwrap().body;
        assert!(walls.is_surface() && walls.face_count() == 4);
        assert!((area(&s, "Walls") - 96.0).abs() < 1e-9);
        let edges = |z: f64| json!([[5, 0, z], [10, 3, z], [5, 6, z], [0, 3, z]]);
        run(&mut s, "surface.patch", json!({"edges": edges(0.0), "body": "Walls", "body_name": "Bottom"}));
        run(&mut s, "surface.patch", json!({"edges": edges(3.0), "body": "Walls", "body_name": "Top"}));
        let stitched = run(&mut s, "surface.stitch", json!({"bodies": ["Walls", "Bottom", "Top"]}));
        assert!(stitched["warning"].is_null(), "{stitched}");
        let st = s.world_state().unwrap();
        assert!(!st.body("Walls").unwrap().body.is_surface());
        run(&mut s, "solid.extrude", json!({"sketch": "R", "distance": 3, "body_name": "Solid"}));
        assert!((volume(&s, "Walls") - volume(&s, "Solid")).abs() < 1e-9);
        assert!((volume(&s, "Walls") - 180.0).abs() < 1e-9);
    }

    /// Open results still succeed, but the reply and timeline say how many boundaries remain.
    #[test]
    fn stitch_reports_open_boundary_edges() {
        for (x, edges, open) in [(20.0, 8, 8), (10.0, 7, 6)] {
            let mut s = Session::default();
            sheet(&mut s, "XY", "A", [0.0, 0.0], [10.0, 10.0]);
            sheet(&mut s, "XY", "B", [x, 0.0], [x + 10.0, 10.0]);
            let reply = run(&mut s, "surface.stitch", json!({"bodies": ["A", "B"]}));
            let expected = format!("Stitch result `A` remains open (boundary edges: {open}).");
            assert_eq!(reply["warning"], expected, "{reply}");
            assert_eq!(reply["removed_bodies"], json!(["B"]));
            let st = s.world_state().unwrap();
            let body = &st.body("A").unwrap().body;
            assert_eq!((body.face_count(), body.edge_count()), (2, edges));
            assert!((area(&s, "A") - 200.0).abs() < 1e-9);
            let id = s.doc.features.last().unwrap().id;
            let check = |s: &Session| assert_eq!(s.model.result(id).unwrap().warning.as_deref(), Some(expected.as_str()));
            check(&s);
            s.model.evaluate(&s.doc);
            check(&s);
            run(&mut s, "edit.undo", json!({}));
            assert_eq!(s.world_state().unwrap().bodies.len(), 2);
            run(&mut s, "edit.redo", json!({}));
            check(&s);
            let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
            check(&cold);
            let saved = s.model.saved(&s.doc).unwrap();
            let adopted = solidcraft_doc::Model::adopt(&s.doc, &saved).unwrap();
            assert_eq!(adopted.result(id).unwrap().warning.as_deref(), Some(expected.as_str()));
        }
    }

    /// A cylindrical wall's seam is used twice by its face; only the two circular rims are open.
    #[test]
    fn stitch_does_not_count_a_periodic_seam_as_open() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Circle"}));
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.extrude", json!({"sketch": "Circle", "distance": 10, "body_name": "Tube"}));
        let reply = run(&mut s, "surface.stitch", json!({"bodies": ["Tube"]}));
        assert_eq!(reply["warning"], "Stitch result `Tube` remains open (boundary edges: 2).");
        let st = s.world_state().unwrap();
        let body = &st.body("Tube").unwrap().body;
        assert_eq!((body.face_count(), body.edge_count()), (1, 3));
    }

    /// Unstitch a box: six surface bodies whose faces keep the names they had on the box,
    /// stitched back into a solid of the same volume; two faces taken out leave the other four
    /// under the box's name; undo, redo and reopening.
    #[test]
    fn unstitch_a_box_and_stitch_it_back() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Box"}));
        let names_before: Vec<String> = solidcraft_doc::naming::face_names(s.model.state().body("Box").unwrap()).to_vec();
        run(&mut s, "surface.unstitch", json!({"body": "Box"}));
        let st = s.world_state().unwrap();
        assert!(st.body("Box").is_none());
        let pieces: Vec<String> = st.bodies.iter().map(|b| b.name.clone()).collect();
        assert_eq!(pieces.len(), 6);
        let mut names_after: Vec<String> = Vec::new();
        for n in &pieces {
            let b = st.body(n).unwrap();
            assert!(b.body.is_surface() && b.body.face_count() == 1);
            names_after.extend(solidcraft_doc::naming::face_names(s.model.state().body(n).unwrap()).iter().cloned());
        }
        let (mut a, mut b) = (names_before.clone(), names_after);
        a.sort();
        b.sort();
        assert_eq!(a, b, "the faces keep their names");
        run(&mut s, "surface.stitch", json!({"bodies": pieces}));
        let st = s.world_state().unwrap();
        let back = st.bodies.first().unwrap();
        assert!(!back.body.is_surface());
        assert!((solidcraft_kernel::measure(&back.body).unwrap().volume - 1000.0).abs() < 1e-9);
        run(&mut s, "edit.undo", json!({}));
        run(&mut s, "edit.undo", json!({}));
        assert!(s.world_state().unwrap().body("Box").is_some());
        run(&mut s, "surface.unstitch", json!({"body": "Box", "faces": [[5, 5, 10], [5, 5, 0]]}));
        let st = s.world_state().unwrap();
        assert_eq!(st.bodies.len(), 3);
        assert_eq!(st.body("Box").unwrap().body.face_count(), 4);
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert_eq!(cold.world_state().unwrap().bodies.len(), 3);
        assert!(s.execute("surface.unstitch", &json!({"body": "Nope"})).is_err());
        assert!(s.execute("surface.unstitch", &json!({})).is_err());
    }

    /// A patch turned over thickens the other way; a solid is not turned over.
    #[test]
    fn reverse_normal_turns_the_thicken_side() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.patch", json!({"sketch": "S", "body_name": "Sheet"}));
        run(&mut s, "surface.thicken", json!({"bodies": ["Sheet"], "thickness": 2, "body_name": "Up"}));
        let up = solidcraft_kernel::measure(&s.world_state().unwrap().body("Up").unwrap().body).unwrap().bbox;
        run(&mut s, "edit.undo", json!({}));
        run(&mut s, "surface.reverse_normal", json!({"body": "Sheet"}));
        run(&mut s, "surface.thicken", json!({"bodies": ["Sheet"], "thickness": 2, "body_name": "Down"}));
        let down = solidcraft_kernel::measure(&s.world_state().unwrap().body("Down").unwrap().body).unwrap().bbox;
        assert!((up.max.z - down.max.z - 2.0).abs() < 1e-9 && (up.min.z - down.min.z - 2.0).abs() < 1e-9, "{up:?} vs {down:?}");
        run(&mut s, "solid.box", json!({"length": 1, "width": 1, "height": 1, "corner": [20, 0, 0], "body_name": "Cube"}));
        assert!(s.execute("surface.reverse_normal", &json!({"body": "Cube"})).is_err());
    }

    /// Delete Face without healing: a box's top taken off leaves a five-face surface; patching
    /// the opening and stitching gives back a box of the same volume. By default the gap still
    /// heals (here it can't: refused); deleting every face is refused.
    #[test]
    fn delete_face_without_healing_leaves_an_open_surface() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Box"}));
        run(&mut s, "face.delete", json!({"faces": [[5, 5, 10]], "heal": false}));
        let st = s.world_state().unwrap();
        let open = &st.body("Box").unwrap().body;
        assert!(open.is_surface() && open.face_count() == 5);
        let rim = json!([[5, 0, 10], [10, 5, 10], [5, 10, 10], [0, 5, 10]]);
        run(&mut s, "surface.patch", json!({"edges": rim, "body": "Box", "body_name": "Lid"}));
        run(&mut s, "surface.stitch", json!({"bodies": ["Box", "Lid"]}));
        assert!(!s.world_state().unwrap().body("Box").unwrap().body.is_surface());
        assert!((volume(&s, "Box") - 1000.0).abs() < 1e-9);
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert!((volume(&cold, "Box") - 1000.0).abs() < 1e-9);
        // By default the gap heals, and a box's sides can't close its top: refused, the box kept.
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "body_name": "Healed"}));
        assert!(s.execute("face.delete", &json!({"faces": [[25, 5, 10]], "body": "Healed"})).is_err());
        assert!(!s.world_state().unwrap().body("Healed").unwrap().body.is_surface());
        let all = json!([[25, 5, 10], [25, 5, 0], [25, 0, 5], [25, 10, 5], [20, 5, 5], [30, 5, 5]]);
        assert!(s.execute("face.delete", &json!({"faces": all, "body": "Healed", "heal": false})).is_err());
    }

    /// Surface Revolve of open curves about the sketch's y axis: a line beside the axis makes a
    /// cylinder (all of it, a quarter, a symmetric quarter), a line from the axis a cone, a half
    /// circle a solid ball; a profile turned all the way round is the solid solid.revolve makes.
    #[test]
    fn surface_revolve_curves_and_profiles() {
        use std::f64::consts::PI;
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.line", json!({"points": [[3, 0], [3, 4]], "ids": ["side"]}));
        run(&mut s, "sketch.line", json!({"points": [[0, 10], [3, 14]], "ids": ["slant"]}));
        run(&mut s, "sketch.finish", json!({}));
        let close = |a: f64, want: f64| assert!((a - want).abs() < want * 2e-3, "{a} vs {want}"); // (measured on meshes)
        run(&mut s, "surface.revolve", json!({"sketch": "S", "curves": ["side"], "axis": "y", "body_name": "Tube"}));
        close(area(&s, "Tube"), 2.0 * PI * 3.0 * 4.0);
        run(&mut s, "surface.revolve", json!({"sketch": "S", "curves": ["side"], "axis": "Y", "angle": "90 deg", "body_name": "Quarter"}));
        close(area(&s, "Quarter"), PI / 2.0 * 3.0 * 4.0);
        run(
            &mut s,
            "surface.revolve",
            json!({"sketch": "S", "curves": ["side"], "axis": "y", "angle": "90 deg", "symmetric": true, "body_name": "Both"}),
        );
        let bb = solidcraft_kernel::measure(&s.world_state().unwrap().body("Both").unwrap().body).unwrap().bbox;
        let z = 3.0 * (PI / 4.0).sin();
        assert!((bb.min.z + z).abs() < 1e-3 && (bb.max.z - z).abs() < 1e-3, "{bb:?}");
        run(&mut s, "surface.revolve", json!({"sketch": "S", "curves": ["slant"], "axis": "y", "body_name": "Cone"}));
        close(area(&s, "Cone"), PI * 3.0 * 5.0);
        assert!(s.world_state().unwrap().body("Cone").unwrap().body.is_surface());
        run(&mut s, "edit.undo", json!({}));
        assert!(s.world_state().unwrap().body("Cone").is_none());
        run(&mut s, "edit.redo", json!({}));
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert!((area(&cold, "Cone") - area(&s, "Cone")).abs() < 1e-9);
        // A rectangle off the axis turned all the way round closes into the ring solid.revolve makes.
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "R"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [5, 0], "p1": [7, 3]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.revolve", json!({"sketch": "R", "axis": "y", "body_name": "Ring"}));
        run(&mut s, "solid.revolve", json!({"sketch": "R", "axis": "y", "body_name": "Solid"}));
        assert!(!s.world_state().unwrap().body("Ring").unwrap().body.is_surface());
        close(volume(&s, "Ring"), volume(&s, "Solid"));
        // Refused: a curve across the axis, no axis, and a turn of more than 360°.
        assert!(s.execute("surface.revolve", &json!({"sketch": "S", "curves": ["side"], "axis": {"origin": [3, 2, 0], "dir": [1, 0, 0]}})).is_err());
        assert!(s.execute("surface.revolve", &json!({"sketch": "S", "curves": ["side"]})).is_err());
        assert!(s.execute("surface.revolve", &json!({"sketch": "S", "curves": ["side"], "axis": "y", "angle": "400 deg"})).is_err());
    }

    /// A surface revolve's walls face out of the profile, as Surface Extrude's do: the end faces
    /// of a rectangle turned half a turn face away from each other. A design saved before `faced`
    /// keeps the faces it built (both ends facing -Z) through an edit, a redefine and reopening,
    /// and a new surface revolve in it faces its faces.
    #[test]
    fn surface_revolve_faces_follow_the_profile_and_older_designs_keep_theirs() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "R"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [5, 0], "p1": [10, -10]}));
        run(&mut s, "sketch.finish", json!({}));
        let turn = |name: &str, body: &str| json!({"sketch": "R", "axis": {"origin": [0, 0, 0], "dir": [0, 0, 1]}, "angle": "180 deg", "symmetric": true, "name": name, "body_name": body});
        run(&mut s, "surface.revolve", turn("Turn", "Ring"));
        // The z of the unit normals of the end faces at z = 0 and z = 10.
        let ends = |s: &Session, body: &str| {
            let st = s.world_state().unwrap();
            let faces = st.body(body).unwrap().body.faces(1e-6).unwrap();
            let at = |z: f64| faces.iter().find(|f| (f.centroid.z - z).abs() < 1e-6).and_then(|f| f.plane_normal).unwrap().z;
            (at(0.0).round(), at(10.0).round())
        };
        let faced = |s: &Session, name: &str| matches!(s.doc.find_feature(name).unwrap().kind, FeatureKind::SurfaceRevolve { faced: true, .. });
        assert_eq!(ends(&s, "Ring"), (-1.0, 1.0));
        assert!(faced(&s, "Turn"));
        let mut saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
        for f in saved["features"].as_array_mut().unwrap() {
            f.as_object_mut().unwrap().remove("faced");
        }
        let mut s = Session::new(solidcraft_doc::Document::from_json(&saved.to_string()).unwrap());
        assert!(!faced(&s, "Turn"));
        assert_eq!(ends(&s, "Ring"), (-1.0, -1.0));
        run(&mut s, "timeline.edit", json!({"feature": "Turn", "set": {"angle": "90 deg"}}));
        assert_eq!(ends(&s, "Ring"), (-1.0, -1.0));
        let mut params = turn("Turn", "Ring");
        params.as_object_mut().unwrap().remove("name");
        run(&mut s, "timeline.redefine", json!({"feature": "Turn", "command": "surface.revolve", "params": params}));
        assert!(!faced(&s, "Turn"));
        assert_eq!(ends(&s, "Ring"), (-1.0, -1.0));
        let mut s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert_eq!(ends(&s, "Ring"), (-1.0, -1.0));
        run(&mut s, "surface.revolve", turn("NewTurn", "NewRing"));
        assert!(faced(&s, "NewTurn"));
        assert_eq!(ends(&s, "NewRing"), (-1.0, 1.0));
    }

    /// Surface Sweep: a line along y carried up a path in the XZ plane, straight then round a
    /// quarter turn (every point of it 5 from the arc's axis: 10 × (5 + 5π/2)), or round a
    /// corner (10 × both legs); a rectangle's walls up the straight leg; undo, redo, reopening.
    #[test]
    fn surface_sweep_curves_and_profiles() {
        use std::f64::consts::FRAC_PI_2;
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, 10]], "ids": ["l"]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "P"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, 5]], "ids": ["up"]}));
        let arc = run(
            &mut s,
            "sketch.arc.three_point",
            json!({"start": [0, 5], "end": [5, 10], "through": [5.0 - 5.0 / 2f64.sqrt(), 5.0 + 5.0 / 2f64.sqrt()]}),
        )["created"]["curves"][0]
            .as_str()
            .unwrap()
            .to_string();
        run(&mut s, "sketch.line", json!({"points": [[0, 5], [5, 5]], "ids": ["over"]}));
        run(&mut s, "sketch.finish", json!({}));
        let close = |a: f64, want: f64| assert!((a - want).abs() < want * 2e-3, "{a} vs {want}"); // (measured on meshes)
        run(&mut s, "surface.sweep", json!({"sketch": "S", "curves": ["l"], "path_sketch": "P", "path": ["up", arc], "body_name": "Bend"}));
        assert!(s.world_state().unwrap().body("Bend").unwrap().body.is_surface());
        close(area(&s, "Bend"), 10.0 * (5.0 + 5.0 * FRAC_PI_2));
        run(&mut s, "surface.sweep", json!({"sketch": "S", "curves": ["l"], "path_sketch": "P", "path": ["up", "over"], "body_name": "Corner"}));
        close(area(&s, "Corner"), 100.0);
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "R"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-1, -1], "p1": [1, 2]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.sweep", json!({"sketch": "R", "path_sketch": "P", "path": ["up"], "body_name": "Walls"}));
        let st = s.world_state().unwrap();
        assert!(st.body("Walls").unwrap().body.is_surface() && st.body("Walls").unwrap().body.face_count() == 4);
        close(area(&s, "Walls"), 10.0 * 5.0);
        run(&mut s, "edit.undo", json!({}));
        assert!(s.world_state().unwrap().body("Walls").is_none());
        run(&mut s, "edit.redo", json!({}));
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert!((area(&cold, "Bend") - area(&s, "Bend")).abs() < 1e-9);
        // Refused: no path, and a path curve that is not there.
        assert!(s.execute("surface.sweep", &json!({"sketch": "S", "curves": ["l"], "path_sketch": "P"})).is_err());
        assert!(s.execute("surface.sweep", &json!({"sketch": "S", "curves": ["l"], "path_sketch": "P", "path": ["nope"]})).is_err());
    }

    /// Surface Loft between sketches 5 apart: two lines give a 10 × 5 strip, two rectangles
    /// their four walls; undo, redo and reopening; a mix of open and closed sections, one
    /// section, and a section of two separate curves are refused.
    #[test]
    fn surface_loft_open_and_closed_sections() {
        let mut s = Session::default();
        run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 5, "name": "Mid"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "A"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "ids": ["a"]}));
        run(&mut s, "sketch.line", json!({"points": [[0, 3], [10, 3]], "ids": ["apart"]}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [20, 0], "p1": [30, 6]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sketch.create", json!({"plane": "Mid", "name": "B"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "ids": ["b"]}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [20, 0], "p1": [30, 6]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.loft", json!({"sections": [{"sketch": "A", "curves": ["a"]}, {"sketch": "B", "curves": ["b"]}], "body_name": "Strip"}));
        assert!(s.world_state().unwrap().body("Strip").unwrap().body.is_surface());
        assert!((area(&s, "Strip") - 50.0).abs() < 1e-6, "{}", area(&s, "Strip"));
        run(&mut s, "surface.loft", json!({"sections": [{"sketch": "A", "profiles": [0]}, {"sketch": "B", "profiles": [0]}], "body_name": "Walls"}));
        let st = s.world_state().unwrap();
        let walls = &st.body("Walls").unwrap().body;
        assert!(walls.is_surface() && walls.face_count() == 4);
        assert!((area(&s, "Walls") - 160.0).abs() < 1e-6, "{}", area(&s, "Walls"));
        run(&mut s, "edit.undo", json!({}));
        assert!(s.world_state().unwrap().body("Walls").is_none());
        run(&mut s, "edit.redo", json!({}));
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert!((area(&cold, "Walls") - area(&s, "Walls")).abs() < 1e-9);
        assert!(s.execute("surface.loft", &json!({"sections": [{"sketch": "A", "curves": ["a"]}, {"sketch": "B", "profiles": [0]}]})).is_err());
        assert!(s.execute("surface.loft", &json!({"sections": [{"sketch": "A", "curves": ["a"]}]})).is_err());
        assert!(
            s.execute("surface.loft", &json!({"sections": [{"sketch": "A", "curves": ["a", "apart"]}, {"sketch": "B", "curves": ["b"]}]})).is_err()
        );
    }

    /// A surface loft's walls face out of the region its closed sections bound, as a solid
    /// loft's do: the four walls between two squares face away from the middle. A design saved
    /// before `faced` keeps the walls it built through an edit, a redefine and reopening, and a
    /// new surface loft in it faces its walls.
    #[test]
    fn surface_loft_walls_face_out_and_older_designs_keep_theirs() {
        let mut s = Session::default();
        run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 5, "name": "Mid"}));
        for (sketch, plane) in [("A", "XY"), ("B", "Mid")] {
            run(&mut s, "sketch.create", json!({"plane": plane, "name": sketch}));
            run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
            run(&mut s, "sketch.finish", json!({}));
        }
        let loft = |name: &str, body: &str| json!({"sections": [{"sketch": "A", "profiles": [0]}, {"sketch": "B", "profiles": [0]}], "name": name, "body_name": body});
        run(&mut s, "surface.loft", loft("Skin", "Walls"));
        // How many of the walls face away from the middle (5, 5).
        let out = |s: &Session, body: &str| {
            let st = s.world_state().unwrap();
            let faces = st.body(body).unwrap().body.faces(1e-6).unwrap();
            assert_eq!(faces.len(), 4);
            faces.iter().filter(|f| f.plane_normal.unwrap().dot(f.centroid - solidcore_record::Vec3::new(5.0, 5.0, f.centroid.z)) > 0.0).count()
        };
        let faced = |s: &Session, name: &str| matches!(s.doc.find_feature(name).unwrap().kind, FeatureKind::SurfaceLoft { faced: true, .. });
        assert_eq!(out(&s, "Walls"), 4);
        assert!(faced(&s, "Skin"));
        let mut saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
        for f in saved["features"].as_array_mut().unwrap() {
            f.as_object_mut().unwrap().remove("faced");
        }
        let mut s = Session::new(solidcraft_doc::Document::from_json(&saved.to_string()).unwrap());
        assert!(!faced(&s, "Skin"));
        let legacy = out(&s, "Walls");
        assert!(legacy < 4, "the legacy build faces some walls in");
        let mut params = loft("Skin", "Walls");
        params.as_object_mut().unwrap().remove("name");
        run(&mut s, "timeline.redefine", json!({"feature": "Skin", "command": "surface.loft", "params": params}));
        assert!(!faced(&s, "Skin"));
        assert_eq!(out(&s, "Walls"), legacy);
        let mut s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert_eq!(out(&s, "Walls"), legacy);
        run(&mut s, "surface.loft", loft("NewSkin", "NewWalls"));
        assert!(faced(&s, "NewSkin"));
        assert_eq!(out(&s, "NewWalls"), 4);
    }

    /// Surface Loft through sections drawn in a 3D sketch (#15367): two 3D lines 30 apart make a
    /// 30 × √1700 strip; a section of two 3D lines meeting end to end, lofted to a line of a
    /// planar sketch 30 away, a 40 × 30 strip; a 3D line lofted to a box edge 30 and 40 away, drawn
    /// either way, a flat 10 × 30 and 10 × 40 strip (the edge chain turned to run as the line
    /// does); the two splines of the issue the same sheet Boundary Surface builds on them; undo,
    /// redo and reopening. Refused: guide rails with a 3D section, and a 3D section of two curves
    /// that do not meet.
    #[test]
    fn surface_loft_through_3d_sketch_curves() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Space"}));
        run(&mut s, "sketch.options", json!({"sketch_3d": true}));
        let line3d = |s: &mut Session, a: [f64; 3], b: [f64; 3]| -> String {
            run(s, "sketch.line3d", json!({"points": [a, b]}))["curves"][0].as_str().unwrap().to_string()
        };
        let a = line3d(&mut s, [0.0, 0.0, 0.0], [40.0, 0.0, 10.0]);
        let b = line3d(&mut s, [0.0, 30.0, 0.0], [40.0, 30.0, 10.0]);
        let near = line3d(&mut s, [0.0, 120.0, 0.0], [20.0, 120.0, 0.0]);
        let far = line3d(&mut s, [20.0, 120.0, 0.0], [40.0, 120.0, 0.0]);
        let apart = line3d(&mut s, [0.0, 150.0, 0.0], [10.0, 150.0, 0.0]);
        let to_box = line3d(&mut s, [0.0, -30.0, 10.0], [10.0, -30.0, 10.0]);
        let to_box_back = line3d(&mut s, [10.0, -40.0, 10.0], [0.0, -40.0, 10.0]);
        let ring = [
            line3d(&mut s, [0.0, 200.0, 0.0], [10.0, 200.0, 0.0]),
            line3d(&mut s, [10.0, 200.0, 0.0], [5.0, 200.0, 8.0]),
            line3d(&mut s, [5.0, 200.0, 8.0], [0.0, 200.0, 0.0]),
        ];
        let w1 = run(&mut s, "sketch.spline3d", json!({"points": [[-40, 0, -4], [-20, 0, -1], [0, 0, 0], [20, 0, -1], [40, 0, -4]]}))["curves"][0]
            .as_str()
            .unwrap()
            .to_string();
        let w2 = run(&mut s, "sketch.spline3d", json!({"points": [[-40, 40, -4], [-20, 40, -1], [0, 40, 0], [20, 40, -1], [40, 40, -4]]}))["curves"]
            [0]
        .as_str()
        .unwrap()
        .to_string();
        // A profile beside them, for a section that picks both.
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [100, 100], "p1": [110, 110]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Flat"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 90], [40, 90]], "ids": ["f"]}));
        run(&mut s, "sketch.finish", json!({}));
        let topology = |s: &Session, name: &str| {
            let st = s.world_state().unwrap();
            let r = st.body(name).unwrap().body.record();
            (r.faces.len(), r.edges.len(), r.vertices.len(), r.shells.len())
        };
        let close = |got: f64, want: f64| assert!((got - want).abs() < want * 1e-6, "{got} vs {want}");

        run(
            &mut s,
            "surface.loft",
            json!({"sections": [{"sketch": "Space", "curves": [a]}, {"sketch": "Space", "curves": [b]}], "body_name": "Strip"}),
        );
        assert!(s.world_state().unwrap().body("Strip").unwrap().body.is_surface());
        close(area(&s, "Strip"), 30.0 * 1700f64.sqrt());
        assert_eq!(topology(&s, "Strip"), (1, 4, 4, 1));
        // A chain of two 3D lines (listed far one first) to a line of a planar sketch.
        run(
            &mut s,
            "surface.loft",
            json!({"sections": [{"sketch": "Space", "curves": [far, near]}, {"sketch": "Flat", "curves": ["f"]}], "body_name": "Joined"}),
        );
        assert!(s.world_state().unwrap().body("Joined").unwrap().body.is_surface());
        close(area(&s, "Joined"), 40.0 * 30.0);
        assert_eq!(topology(&s, "Joined"), (1, 4, 4, 1));
        // A 3D line to the top front edge of a box, drawn one way and the other.
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Box"}));
        for (line, gap, name) in [(&to_box, 30.0, "ToBox"), (&to_box_back, 40.0, "ToBoxBack")] {
            run(
                &mut s,
                "surface.loft",
                json!({"sections": [{"sketch": "Space", "curves": [line]}, {"body": "Box", "edges": [[5, 0, 10]]}], "body_name": name}),
            );
            close(area(&s, name), 10.0 * gap);
            assert_eq!(topology(&s, name), (1, 4, 4, 1));
        }
        // The issue's two splines: the sheet Boundary Surface builds through the same curves.
        let sections = json!([{"sketch": "Space", "curves": [w1]}, {"sketch": "Space", "curves": [w2]}]);
        run(&mut s, "surface.loft", json!({"sections": sections, "body_name": "Loft"}));
        run(&mut s, "surface.boundary", json!({"u": sections, "body_name": "Boundary"}));
        assert_eq!(topology(&s, "Loft"), (1, 4, 4, 1));
        assert!((area(&s, "Loft") - area(&s, "Boundary")).abs() < area(&s, "Boundary") * 1e-3, "{} vs {}", area(&s, "Loft"), area(&s, "Boundary"));

        run(&mut s, "edit.undo", json!({}));
        run(&mut s, "edit.undo", json!({}));
        assert!(s.world_state().unwrap().body("Loft").is_none());
        run(&mut s, "edit.redo", json!({}));
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert!((area(&cold, "Loft") - area(&s, "Loft")).abs() < 1e-9);
        assert!((area(&cold, "Strip") - area(&s, "Strip")).abs() < 1e-9);

        let err = |s: &mut Session, p: Value| s.execute("surface.loft", &p).unwrap_err().to_string();
        let railed = err(
            &mut s,
            json!({"sections": [{"sketch": "Space", "curves": [a]}, {"sketch": "Space", "curves": [b]}], "rails": [{"sketch": "Space", "curves": [near]}]}),
        );
        assert!(railed.contains("not supported yet") && railed.contains("3D sketch"), "{railed}");
        let gap = err(&mut s, json!({"sections": [{"sketch": "Space", "curves": [a, apart]}, {"sketch": "Space", "curves": [b]}]}));
        assert!(gap.contains("not connected"), "{gap}");
        let both = err(&mut s, json!({"sections": [{"sketch": "Space", "curves": [a], "profiles": [0]}, {"sketch": "Space", "curves": [b]}]}));
        assert!(both.contains("one chain"), "{both}");
        // A closed section, of 3D sketch curves or a planar profile, next to a 3D one: its winding
        // and seam are not matched yet.
        let ring = err(&mut s, json!({"sections": [{"sketch": "Space", "curves": ring}, {"sketch": "Space", "curves": [b]}]}));
        assert!(ring.contains("not supported yet") && ring.contains("closed section"), "{ring}");
        let profile = err(&mut s, json!({"sections": [{"sketch": "Space", "profiles": [0]}, {"sketch": "Space", "curves": [b]}]}));
        assert!(profile.contains("not supported yet") && profile.contains("closed section"), "{profile}");
        // Ends that leave tangent take 3D sections too.
        run(
            &mut s,
            "surface.loft",
            json!({"sections": [{"sketch": "Space", "curves": [a]}, {"sketch": "Space", "curves": [b]}], "continuity": ["g1", "g1"], "body_name": "Tangent"}),
        );
        assert_eq!(topology(&s, "Tangent"), (1, 4, 4, 1));
        close(area(&s, "Tangent"), 30.0 * 1700f64.sqrt());
    }

    /// Each wall of a surface built from sketch curves is named after the curve it comes from,
    /// exactly (not from geometry): extrude, revolve, sweep and loft.
    #[test]
    fn surface_walls_are_named_after_their_curves() {
        let mut s = Session::default();
        run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 5, "name": "Mid"}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.line", json!({"points": [[3, 0], [3, 4]], "ids": ["l1"]}));
        run(&mut s, "sketch.line", json!({"points": [[3, 4], [6, 4]], "ids": ["l2"]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "P"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, 5]], "ids": ["up"]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "sketch.create", json!({"plane": "Mid", "name": "T"}));
        run(&mut s, "sketch.line", json!({"points": [[3, 0], [3, 4]], "ids": ["m1"]}));
        run(&mut s, "sketch.line", json!({"points": [[3, 4], [6, 4]], "ids": ["m2"]}));
        run(&mut s, "sketch.finish", json!({}));
        let made = [
            ("surface.extrude", json!({"sketch": "S", "curves": ["l1", "l2"], "distance": 3, "body_name": "E"})),
            ("surface.revolve", json!({"sketch": "S", "curves": ["l1", "l2"], "axis": "y", "angle": "90 deg", "body_name": "R"})),
            ("surface.sweep", json!({"sketch": "S", "curves": ["l1", "l2"], "path_sketch": "P", "path": ["up"], "body_name": "W"})),
            (
                "surface.loft",
                json!({"sections": [{"sketch": "S", "curves": ["l1", "l2"]}, {"sketch": "T", "curves": ["m1", "m2"]}], "body_name": "L"}),
            ),
        ];
        for (cmd, p) in made {
            let body = p["body_name"].as_str().unwrap().to_string();
            run(&mut s, cmd, p);
            let fid = s.doc.features.last().unwrap().id;
            let st = s.model.state();
            let b = st.body(&body).unwrap();
            let names = solidcraft_doc::naming::face_names(b);
            for curve in ["l1", "l2"] {
                let want = format!("F{fid}:side:{curve}");
                let i = names.iter().position(|n| n.starts_with(&want)).unwrap_or_else(|| panic!("{cmd}: no face named {want}: {names:?}"));
                assert!(solidcraft_doc::naming::face_name_exact(b, i), "{cmd}: {want} is named from geometry");
            }
        }
    }

    /// Offset Surface: a cylinder's side (one face, closed along its seam) picked with its chain
    /// moved out to radius 12; a box top moved up exactly; a zero distance copies; past the axis
    /// refused, the design left as it was; undo, redo and reopening.
    #[test]
    fn offset_surface_of_a_cylinder_and_a_box_top() {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 20, "body_name": "Can"}));
        run(&mut s, "surface.offset", json!({"faces": [[10, 0, 10]], "chain": true, "distance": 2, "body": "Can", "body_name": "Sleeve"}));
        let st = s.world_state().unwrap();
        let sleeve = &st.body("Sleeve").unwrap().body;
        assert!(sleeve.is_surface());
        let want = 2.0 * std::f64::consts::PI * 12.0 * 20.0;
        assert!((area(&s, "Sleeve") - want).abs() < want * 1e-3, "{} vs {want}", area(&s, "Sleeve")); // (measured on meshes)
        assert!(st.body("Can").is_some(), "the source body stays");
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [30, 0, 0], "body_name": "Box"}));
        run(&mut s, "surface.offset", json!({"faces": [[35, 5, 10]], "distance": "5 mm", "body_name": "Lid"}));
        let bb = solidcraft_kernel::measure(&s.world_state().unwrap().body("Lid").unwrap().body).unwrap().bbox;
        assert!((bb.min.z - 15.0).abs() < 1e-9 && (area(&s, "Lid") - 100.0).abs() < 1e-9);
        run(&mut s, "surface.offset", json!({"faces": [[35, 5, 10]], "distance": 0, "body_name": "Copy"}));
        assert!((area(&s, "Copy") - 100.0).abs() < 1e-9);
        let before = s.doc.features.len();
        assert!(s.execute("surface.offset", &json!({"faces": [[10, 0, 10]], "chain": true, "distance": -11, "body": "Can"})).is_err());
        assert_eq!(s.doc.features.len(), before);
        run(&mut s, "edit.undo", json!({}));
        assert!(s.world_state().unwrap().body("Copy").is_none());
        run(&mut s, "edit.redo", json!({}));
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert!((area(&cold, "Sleeve") - area(&s, "Sleeve")).abs() < 1e-9);
        assert!(s.execute("surface.offset", &json!({"distance": 1})).is_err());
    }

    /// A positive Offset Surface distance goes outward from the solid on a mirrored body too
    /// (#20236), picked as the issue picks them (no body named): a 10 mm cube on x = 5..15
    /// mirrored in YZ, its copy on x = -15..-5. The mirror uses the copy's faces from their
    /// surfaces' backs. Each end face moved 3 out lands at x = 18 and x = -18; the copy's landed
    /// at -12, inside the copy. A negative distance goes in on both (x = -12); a cylinder of radius
    /// 5 beside them moves out to radius 8.
    #[test]
    fn offset_surface_goes_outward_on_a_mirrored_body() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [5, 0, 0], "body_name": "A"}));
        let copy = run(&mut s, "solid.mirror", json!({"bodies": ["A"], "plane": "YZ"}))["created_bodies"][0].as_str().unwrap().to_string();
        assert!((volume(&s, &copy) - 1000.0).abs() < 1e-6, "the copy is a solid: {}", volume(&s, &copy));
        let bbox = |s: &Session, name: &str| solidcraft_kernel::measure(&s.world_state().unwrap().body(name).unwrap().body).unwrap().bbox;
        let at_x = |s: &Session, name: &str, x: f64| {
            let b = bbox(s, name);
            assert!((b.min.x - x).abs() < 1e-9 && (b.max.x - x).abs() < 1e-9, "{name}: x {}..{}, expected {x}", b.min.x, b.max.x);
        };
        run(&mut s, "surface.offset", json!({"faces": [[15, 5, 5]], "distance": "3", "body_name": "Right"}));
        run(&mut s, "surface.offset", json!({"faces": [[-15, 5, 5]], "distance": "3", "body_name": "Left"}));
        run(&mut s, "surface.offset", json!({"faces": [[-15, 5, 5]], "distance": "-3", "body_name": "Inside"}));
        at_x(&s, "Right", 18.0);
        at_x(&s, "Left", -18.0);
        at_x(&s, "Inside", -12.0);
        run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 10, "base": [0, 40, 0], "body_name": "C"}));
        run(&mut s, "surface.offset", json!({"faces": [[5, 40, 5]], "distance": "3", "body_name": "Ring"}));
        let ring = bbox(&s, "Ring");
        assert!((ring.max.x - 8.0).abs() < 1e-6 && (ring.min.x + 8.0).abs() < 1e-6, "Ring: x {}..{}, expected -8..8", ring.min.x, ring.max.x);
    }

    /// Ruled Surface round a box top (10 × 10), 2 wide: across the top, mitred, a 14 × 14 frame
    /// less the top (96); up the top's normal, 80; at 45° with round corners, 80 + π·√2·2; along
    /// a direction, 2 × 10 off one edge. The strips' faces are named after the edges they stand
    /// on. Zero, a missing direction, an unknown mode and a non-finite distance are refused, the
    /// design left as it was; undo, redo and reopening.
    #[test]
    fn ruled_surface_round_a_box_top() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Box"}));
        let rim = json!([[5, 0, 10], [10, 5, 10], [5, 10, 10], [0, 5, 10]]);
        let top = json!([5, 5, 10]);
        run(&mut s, "surface.ruled", json!({"body": "Box", "edges": rim, "distance": "2 mm", "mode": "tangent", "face": top, "body_name": "Flange"}));
        assert!((area(&s, "Flange") - 96.0).abs() < 1e-9, "{}", area(&s, "Flange"));
        assert!(s.world_state().unwrap().body("Flange").unwrap().body.is_surface() && s.world_state().unwrap().body("Box").is_some());
        run(&mut s, "surface.ruled", json!({"body": "Box", "edges": rim, "distance": 2, "face": top, "body_name": "Wall"}));
        assert!((area(&s, "Wall") - 80.0).abs() < 1e-9);
        run(
            &mut s,
            "surface.ruled",
            json!({"body": "Box", "edges": rim, "distance": 2, "mode": "angle", "angle": "45 deg", "corners": "round", "face": top, "body_name": "Draft"}),
        );
        let want = 80.0 + std::f64::consts::PI * 2f64.sqrt() * 2.0;
        assert!((area(&s, "Draft") - want).abs() < want * 2e-3, "{} vs {want}", area(&s, "Draft")); // (measured on meshes)
        run(
            &mut s,
            "surface.ruled",
            json!({"body": "Box", "edges": [[5, 0, 10]], "distance": 2, "mode": "direction", "direction": [0, -1, 0], "body_name": "Lip"}),
        );
        let bb = solidcraft_kernel::measure(&s.world_state().unwrap().body("Lip").unwrap().body).unwrap().bbox;
        assert!((area(&s, "Lip") - 20.0).abs() < 1e-9 && (bb.min.y + 2.0).abs() < 1e-9);
        let st = s.world_state().unwrap();
        let names = solidcraft_doc::naming::face_names(st.body("Flange").unwrap());
        assert!(names.len() == 4 && names.iter().all(|n| !n.is_empty()), "{names:?}");
        let before = s.doc.features.len();
        for bad in [
            json!({"body": "Box", "edges": rim, "distance": 0}),
            json!({"body": "Box", "edges": rim, "distance": "1e400 mm"}),
            json!({"body": "Box", "edges": rim, "distance": 2, "mode": "direction"}),
            json!({"body": "Box", "edges": rim, "distance": 2, "mode": "sideways"}),
            json!({"body": "Box", "edges": rim, "distance": 2, "mode": "angle"}),
            json!({"body": "Box", "distance": 2}),
            json!({"body": "Nope", "edges": rim, "distance": 2}),
        ] {
            assert!(s.execute("surface.ruled", &bad).is_err(), "{bad}");
        }
        assert_eq!(s.doc.features.len(), before);
        run(&mut s, "edit.undo", json!({}));
        assert!(s.world_state().unwrap().body("Lip").is_none());
        run(&mut s, "edit.redo", json!({}));
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert!((area(&cold, "Draft") - area(&s, "Draft")).abs() < 1e-9);
    }

    /// A lip off the long edge of a skin lofted through three 3D sketch curves (at x = 0, 300 and
    /// 600, each through five points), 20 wide, along the skin's normal and continuing the skin.
    /// The skin's run repeats a knot at the middle curve, and the lip's far edge crosses it
    /// without a corner, so each lip is one valid face. A lip's rulings are straight and 20 long:
    /// its two near corners are the ends of the skin's edge (the first and the last curve's last
    /// points), and each far corner is 20 from one of them.
    #[test]
    fn ruled_surface_off_a_lofted_skin() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Sections"}));
        run(&mut s, "sketch.options", json!({"sketch_3d": true}));
        let curves = [
            json!([[0, 0, 100], [0, 35.4624, 95.5336], [0, 67.7571, 82.5336], [0, 93.9992, 62.161], [0, 111.8447, 36.2358]]),
            json!([[300, 0, 130], [300, 41.9639, 124.1937], [300, 80.1792, 107.2936], [300, 111.2324, 80.8093], [300, 132.3496, 47.1065]]),
            json!([[600, 0, 90], [600, 33.9848, 85.9803], [600, 64.9339, 74.2802], [600, 90.0826, 55.9449], [600, 107.1845, 32.6122]]),
        ];
        let mut sections = Vec::new();
        for points in curves {
            let wire = run(&mut s, "sketch.spline3d", json!({"points": points}))["curves"][0].as_str().unwrap().to_string();
            sections.push(json!({"sketch": "Sections", "curves": [wire]}));
        }
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.loft", json!({"sections": sections, "body_name": "Skin"}));
        let (edge, on_skin) = (json!([300, 132.3496, 47.1065]), json!([300, 80.1792, 107.2936]));
        let ends = [solidcore_record::Vec3::new(0.0, 111.8447, 36.2358), solidcore_record::Vec3::new(600.0, 107.1845, 32.6122)];
        for (mode, name) in [("normal", "Lip"), ("tangent", "Run")] {
            run(&mut s, "surface.ruled", json!({"body": "Skin", "edges": [edge], "distance": 20, "mode": mode, "face": on_skin, "body_name": name}));
            let st = s.world_state().unwrap();
            let lip = &st.body(name).unwrap().body;
            assert!(lip.is_surface() && lip.face_count() == 1, "{mode}");
            let corners: Vec<solidcore_record::Vec3> =
                lip.record().vertices.iter().map(|v| solidcore_record::Vec3::new(v.point[0], v.point[1], v.point[2])).collect();
            assert_eq!(corners.len(), 4, "{mode}: {corners:?}");
            let to_end = |p: &solidcore_record::Vec3| p.dist(ends[0]).min(p.dist(ends[1]));
            let (near, far): (Vec<f64>, Vec<f64>) = corners.iter().map(to_end).partition(|d| *d < 10.0);
            assert!(near.len() == 2 && near.iter().all(|d| *d < 1e-6), "{mode}: {corners:?}");
            assert!(far.len() == 2 && far.iter().all(|d| (d - 20.0).abs() < 1e-6), "{mode}: {far:?}");
        }
    }

    /// A 40 × 20 × 2 plate with three holes of radius 2 through it at x = 10, 20, 30 on y = 10.
    fn plate_with_three_holes(s: &mut Session) {
        run(s, "solid.box", json!({"length": 40, "width": 20, "height": 2, "body_name": "Plate"}));
        for x in [10, 20, 30] {
            run(s, "solid.cylinder", json!({"radius": 2, "height": 4, "base": [x, 10, -1], "operation": "cut", "targets": ["Plate"]}));
        }
    }

    fn edge_count(s: &Session, name: &str) -> usize {
        s.world_state().unwrap().body(name).unwrap().body.edge_count()
    }

    /// Untrim the plate's top: every hole taken away gives one face of the whole 40 × 20
    /// rectangle (800) bounded by its four edges alone, still named as the plate's top; one hole taken away
    /// leaves two (800 − 2 · 4π). The plate stays; undo, redo and reopening give the same face.
    #[test]
    fn untrim_a_plate_with_three_holes() {
        let mut s = Session::default();
        plate_with_three_holes(&mut s);
        let top = [5.0, 5.0, 2.0];
        let name =
            solidcraft_doc::naming::face_name_at_body(s.model.state().body("Plate").unwrap(), solidcore_record::Vec3::new(5.0, 5.0, 2.0)).unwrap();
        run(&mut s, "surface.untrim", json!({"face": {"body": "Plate", "point": top}, "body_name": "Top"}));
        let st = s.world_state().unwrap();
        let made = &st.body("Top").unwrap().body;
        assert!(made.is_surface() && made.face_count() == 1);
        assert_eq!(edge_count(&s, "Top"), 4, "the rectangle alone");
        assert!((area(&s, "Top") - 800.0).abs() < 1e-9, "{}", area(&s, "Top"));
        assert_eq!(solidcraft_doc::naming::face_names(s.model.state().body("Top").unwrap()).first(), Some(&name), "the face keeps its name");
        assert!(st.body("Plate").is_some(), "the solid stays");
        run(&mut s, "edit.undo", json!({}));
        assert!(s.world_state().unwrap().body("Top").is_none());
        run(&mut s, "edit.redo", json!({}));
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert!((area(&cold, "Top") - 800.0).abs() < 1e-9);
        // One hole, by a point on its rim.
        run(&mut s, "surface.untrim", json!({"face": {"body": "Plate", "point": top}, "holes": [[22, 10, 2]], "body_name": "Two"}));
        assert!(edge_count(&s, "Two") > 4);
        let want = 800.0 - 2.0 * 4.0 * std::f64::consts::PI;
        assert!((area(&s, "Two") - want).abs() < want * 1e-3, "{} vs {want}", area(&s, "Two")); // (measured on meshes)
        // Refused, the design left as it was: nothing to untrim, a point on no hole, bad input.
        let before = s.doc.features.len();
        for bad in [
            json!({"face": {"body": "Plate", "point": top}, "holes": []}),
            json!({"face": {"body": "Plate", "point": top}, "holes": [[5, 5, 2]]}),
            json!({"face": {"body": "Plate", "point": [20, 0, 1]}}),
            json!({"face": {"body": "Nope", "point": top}}),
            json!({"face": {"body": "Plate"}}),
            json!({"face": {"body": "Plate", "point": top}, "holes": "some"}),
            json!({"face": {"body": "Plate", "point": top}, "holes": [[1, 2]]}),
            json!({}),
        ] {
            assert!(s.execute("surface.untrim", &bad).is_err(), "{bad}");
        }
        assert_eq!(s.doc.features.len(), before);
    }

    /// A half cylinder sheet (radius 10, height 20) trimmed by the slanted plane z = 15 − y/2
    /// (area 150π − 100) and untrimmed in place with its outer boundary restored: the same body
    /// fills its face's own span again, angles 0…π by heights 0…15 (150π), and its face keeps its
    /// name.
    #[test]
    fn untrim_restores_a_trimmed_cylinder_face_in_place() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        let arc = run(&mut s, "sketch.arc.three_point", json!({"start": [10, 0], "end": [-10, 0], "through": [0, 10]}))["created"]["curves"][0]
            .as_str()
            .unwrap()
            .to_string();
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.extrude", json!({"sketch": "S", "curves": [arc], "distance": 20, "body_name": "Half"}));
        run(&mut s, "surface.trim", json!({"body": "Half", "plane": {"origin": [0, 0, 15], "normal": [0, 0.5, 1]}, "keep": [0, 10, 1]}));
        let pi = std::f64::consts::PI;
        assert!((area(&s, "Half") - (150.0 * pi - 100.0)).abs() < 150.0 * pi * 1e-3, "{}", area(&s, "Half"));
        let name = solidcraft_doc::naming::face_names(s.model.state().body("Half").unwrap()).first().cloned().unwrap();
        let on = [10.0 * 2.5f64.cos(), 10.0 * 2.5f64.sin(), 5.0];
        assert!(s.execute("surface.untrim", &json!({"face": {"body": "Half", "point": on}})).is_err(), "no holes, outer kept");
        run(&mut s, "surface.untrim", json!({"face": {"body": "Half", "point": on}, "outer": true, "in_place": true}));
        let st = s.world_state().unwrap();
        assert_eq!(st.bodies.len(), 1, "untrimmed where it is");
        assert!((area(&s, "Half") - 150.0 * pi).abs() < 150.0 * pi * 1e-3, "{}", area(&s, "Half")); // (measured on meshes)
        let bb = solidcraft_kernel::measure(&st.body("Half").unwrap().body).unwrap().bbox;
        assert!(bb.min.z.abs() < 1e-9 && (bb.max.z - 15.0).abs() < 1e-6, "{bb:?}");
        assert_eq!(solidcraft_doc::naming::face_names(s.model.state().body("Half").unwrap()).first(), Some(&name), "the face keeps its name");
    }

    /// The plate written to STEP and inserted back: its imported top face untrims to the whole
    /// rectangle, outer boundary restored too.
    #[test]
    fn untrim_an_imported_step_face() {
        let mut s = Session::default();
        plate_with_three_holes(&mut s);
        let step = solidcraft_kernel::step_export(&[&s.world_state().unwrap().body("Plate").unwrap().body], "untrim test").unwrap();
        let path = std::env::temp_dir().join(format!("solidcraft-untrim-{}.step", std::process::id()));
        std::fs::write(&path, step).unwrap();
        let mut s = Session::default();
        run(&mut s, "file.insert_step", json!({"path": path.to_string_lossy()}));
        let _ = std::fs::remove_file(&path);
        let body = s.world_state().unwrap().bodies.first().unwrap().name.clone();
        run(&mut s, "surface.untrim", json!({"face": {"body": body, "point": [5, 5, 2]}, "holes": "all", "outer": true, "body_name": "Top"}));
        assert_eq!(edge_count(&s, "Top"), 4, "the rectangle alone");
        assert!((area(&s, "Top") - 800.0).abs() < 1e-9, "{}", area(&s, "Top"));
    }

    /// A quarter cylinder sheet (radius 10, heights 0…20) swept from a sketch arc.
    fn quarter_cylinder(s: &mut Session, name: &str) {
        run(s, "sketch.create", json!({"plane": "XY", "name": "Arc"}));
        let r = 10.0 / 2f64.sqrt();
        let arc = run(s, "sketch.arc.three_point", json!({"start": [10, 0], "end": [0, 10], "through": [r, r]}))["created"]["curves"][0]
            .as_str()
            .unwrap()
            .to_string();
        run(s, "sketch.finish", json!({}));
        run(s, "surface.extrude", json!({"sketch": "Arc", "curves": [arc], "distance": 20, "body_name": name}));
    }

    /// Extend a cylinder strip: naturally 5 along its axis (10 · π/2 · 25, one face, still a
    /// cylinder), up to the plane z = 30 (10 · π/2 · 30), and by tangent and curvature run-offs
    /// along a straight edge, 20 long: each SMLib's G1 or G2 extension of the surface, so the
    /// strip stays one face. The distance is measured across the surface (the run-off's section
    /// is 5 long, whatever its curvature) and the strip is its section swept 20 along z, so each
    /// run-off adds 5 × 20: 10 · π/2 · 20 + 100 = 414.159265, exact to 1e-6. Areas are SMLib's,
    /// integrated on the exact surfaces. Undo, redo and reopening; the default saves nothing new.
    #[test]
    fn extend_a_cylinder_strip_every_way() {
        let pi = std::f64::consts::PI;
        let mut s = Session::default();
        quarter_cylinder(&mut s, "Strip");
        let top = [10.0 * (pi / 4.0).cos(), 10.0 * (pi / 4.0).sin(), 20.0];
        run(&mut s, "surface.extend", json!({"body": "Strip", "edges": [top], "distance": "5 mm", "name": "Up"}));
        let want = 10.0 * pi / 2.0 * 25.0;
        assert!((area(&s, "Strip") - want).abs() < want * 1e-3, "{} vs {want}", area(&s, "Strip"));
        assert_eq!(s.world_state().unwrap().body("Strip").unwrap().body.face_count(), 1);
        let saved = s.doc.to_json();
        assert!(!saved.contains("\"mode\"") && !saved.contains("\"up_to\"") && !saved.contains("\"neighbours\""), "the default saves as before");
        run(&mut s, "edit.undo", json!({}));
        assert!((area(&s, "Strip") - 200.0 * pi / 4.0 * 2.0).abs() < 1.0);
        run(&mut s, "edit.redo", json!({}));
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert!((area(&cold, "Strip") - area(&s, "Strip")).abs() < 1e-9);
        run(&mut s, "edit.undo", json!({}));

        run(&mut s, "surface.extend", json!({"body": "Strip", "edges": [top], "up_to": {"origin": [0, 0, 30], "normal": [0, 0, 1]}}));
        let want = 10.0 * pi / 2.0 * 30.0;
        assert!((area(&s, "Strip") - want).abs() < want * 1e-3, "{} vs {want}", area(&s, "Strip"));
        run(&mut s, "edit.undo", json!({}));

        let side = [0.0, 10.0, 10.0];
        for mode in ["tangent", "curvature"] {
            run(&mut s, "surface.extend", json!({"body": "Strip", "edges": [side], "distance": 5, "mode": mode}));
            let want = 10.0 * pi / 2.0 * 20.0 + 5.0 * 20.0;
            // SMLib's area, integrated on the exact surface to 1e-8: the run-off is exactly 5 long.
            assert!((area(&s, "Strip") - want).abs() < want * 1e-6, "{mode}: {} vs {want}", area(&s, "Strip"));
            let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
            assert!((area(&cold, "Strip") - area(&s, "Strip")).abs() < 1e-9, "{mode}");
            run(&mut s, "edit.undo", json!({}));
        }
    }

    /// The half cylinder trimmed by a slanted plane goes back to its span (150π) when its top
    /// edge is extended to the surface's bounds.
    #[test]
    fn extend_back_to_the_surfaces_bounds() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        let arc = run(&mut s, "sketch.arc.three_point", json!({"start": [10, 0], "end": [-10, 0], "through": [0, 10]}))["created"]["curves"][0]
            .as_str()
            .unwrap()
            .to_string();
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.extrude", json!({"sketch": "S", "curves": [arc], "distance": 20, "body_name": "Half"}));
        run(&mut s, "surface.trim", json!({"body": "Half", "plane": {"origin": [0, 0, 15], "normal": [0, 0.5, 1]}, "keep": [0, 10, 1]}));
        run(&mut s, "surface.extend", json!({"body": "Half", "edges": [[0, 10, 10]], "up_to": "bounds"}));
        let want = 150.0 * std::f64::consts::PI;
        assert!((area(&s, "Half") - want).abs() < want * 1e-3, "{} vs {want}", area(&s, "Half")); // (measured on meshes)
    }

    /// Requests Extend can't take are refused and leave the design as it was.
    #[test]
    fn extend_refuses_bad_requests() {
        let mut s = Session::default();
        quarter_cylinder(&mut s, "Strip");
        let before = s.doc.features.len();
        let top = [10.0 * (std::f64::consts::PI / 4.0).cos(), 10.0 * (std::f64::consts::PI / 4.0).sin(), 20.0];
        for p in [
            json!({"body": "Strip", "edges": [top]}),
            json!({"body": "Strip", "edges": [top], "distance": 5, "mode": "sideways"}),
            json!({"body": "Strip", "edges": [top], "up_to": "bounds", "mode": "tangent"}),
            json!({"body": "Strip", "edges": [top], "up_to": {"body": "Nope", "point": [0, 0, 0]}}),
            json!({"body": "Strip", "edges": [top], "up_to": {"body": "Strip"}}),
            json!({"body": "Strip", "edges": [top], "up_to": "No plane"}),
            json!({"body": "Strip", "edges": [], "distance": 5}),
            json!({"body": "Nope", "edges": [top], "distance": 5}),
        ] {
            assert!(s.execute("surface.extend", &p).is_err(), "{p}");
        }
        assert_eq!(s.doc.features.len(), before);
    }

    /// The planned surface commands are registered but refuse to run: disabled with the
    /// issue that tracks them, an error (never a panic) from every caller, and the design left
    /// as it was.
    #[test]
    fn planned_commands_refuse_and_leave_the_design_alone() {
        let planned = ["surface.fillet"];
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "B"}));
        let before = s.doc.features.len();
        for id in planned {
            let spec = crate::find_command(id).unwrap_or_else(|| panic!("{id} is not registered"));
            let info = spec.info(&s);
            assert!(!info.enabled, "{id} must stay disabled until it is built");
            let why = info.disabled_reason.unwrap_or_default();
            assert!(why.starts_with("not supported yet (issue #"), "{id}: {why}");
            assert!(!spec.icon.is_empty() && !spec.params.is_empty(), "{id}: icon and params doc");
            assert!(s.execute(id, &json!({"body": "B"})).is_err(), "{id} ran");
            let e = (spec.run)(&mut s, &json!({})).unwrap_err().to_string();
            assert!(e.starts_with("not supported yet: ") && e.contains("(issue #"), "{id}: {e}");
        }
        assert_eq!(s.doc.features.len(), before, "a planned command changed the design");
    }

    /// A rectangle patch in a plane (`plane`: a sketch plane name), corners (u0, v0) and (u1, v1)
    /// in its coordinates.
    fn sheet(s: &mut Session, plane: &str, name: &str, p0: [f64; 2], p1: [f64; 2]) {
        let sk = format!("{name} sketch");
        run(s, "sketch.create", json!({"plane": plane, "name": sk}));
        run(s, "sketch.rectangle.two_point", json!({"p0": p0, "p1": p1}));
        run(s, "sketch.finish", json!({}));
        run(s, "surface.patch", json!({"sketch": sk, "body_name": name}));
    }

    fn names(s: &Session, body: &str) -> Vec<String> {
        solidcraft_doc::naming::face_names(s.model.state().body(body).unwrap()).to_vec()
    }

    fn exact(s: &Session, body: &str, i: usize) -> bool {
        solidcraft_doc::naming::face_name_exact(s.model.state().body(body).unwrap(), i)
    }

    /// Two 20 × 10 sheets crossing at right angles (z = 0 and x = 0, both y = 0…10), trimmed by
    /// each other and stitched: an L of two faces sharing one edge (7 edges), 200 in area; each
    /// face named after the sheet it is a piece of. Undo, redo and reopening rebuild it.
    #[test]
    fn mutual_trim_stitches_crossing_sheets_into_an_l() {
        let mut s = Session::default();
        sheet(&mut s, "XY", "Floor", [-10.0, 0.0], [10.0, 10.0]);
        // YZ sketches have u along Y and v along Z.
        sheet(&mut s, "YZ", "Wall", [0.0, -10.0], [10.0, 10.0]);
        let (floor, wall) = (names(&s, "Floor"), names(&s, "Wall"));
        // (A patch's face is named from geometry; its pieces inherit that, no surer.)
        let sure = exact(&s, "Floor", 0);
        run(&mut s, "surface.trim", json!({"body": "Floor", "keep": [5, 5, 0], "with": "Wall", "keep_with": [0, 5, 5], "stitch": true}));
        let st = s.world_state().unwrap();
        assert!(st.body("Wall").is_none(), "sewn into the floor");
        let l = &st.body("Floor").unwrap().body;
        assert!(l.is_surface());
        assert_eq!((l.face_count(), l.edge_count()), (2, 7), "one connected sheet");
        assert!((area(&s, "Floor") - 200.0).abs() < 1e-9, "{}", area(&s, "Floor"));
        let bb = solidcraft_kernel::measure(l).unwrap().bbox;
        assert!(bb.min.x.abs() < 1e-9 && bb.min.z.abs() < 1e-9 && (bb.max.x - 10.0).abs() < 1e-9 && (bb.max.z - 10.0).abs() < 1e-9, "{bb:?}");
        let mut got = names(&s, "Floor");
        got.sort();
        let mut want = vec![floor[0].clone(), wall[0].clone()];
        want.sort();
        assert_eq!(got, want, "each face named after its sheet");
        assert!(exact(&s, "Floor", 0) == sure && exact(&s, "Floor", 1) == sure);
        let saved = s.doc.to_json();
        run(&mut s, "edit.undo", json!({}));
        assert!(s.world_state().unwrap().body("Wall").is_some() && (area(&s, "Floor") - 200.0).abs() < 1e-9);
        run(&mut s, "edit.redo", json!({}));
        let cold = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
        assert!((area(&cold, "Floor") - 200.0).abs() < 1e-9 && cold.world_state().unwrap().body("Wall").is_none());
        // Without stitching, each keeps its half under its own name.
        run(&mut s, "edit.undo", json!({}));
        run(&mut s, "surface.trim", json!({"body": "Floor", "keep": [-5, 5, 0], "with": "Wall", "keep_with": [0, 5, -5]}));
        assert!((area(&s, "Floor") - 100.0).abs() < 1e-9 && (area(&s, "Wall") - 100.0).abs() < 1e-9);
        let bb = solidcraft_kernel::measure(&s.world_state().unwrap().body("Wall").unwrap().body).unwrap().bbox;
        assert!((bb.max.z).abs() < 1e-9, "the wall's lower half: {bb:?}");
    }

    /// A wall that stops 3 short of the floor is refused until `extend` brings it down to meet
    /// it; then the two make the L.
    #[test]
    fn mutual_trim_extends_a_short_sheet() {
        let mut s = Session::default();
        sheet(&mut s, "XY", "Floor", [-10.0, 0.0], [10.0, 10.0]);
        sheet(&mut s, "YZ", "Wall", [0.0, 3.0], [10.0, 10.0]);
        let mutual = json!({"body": "Floor", "keep": [5, 5, 0], "with": "Wall", "keep_with": [0, 5, 5], "stitch": true});
        let e = s.execute("surface.trim", &mutual).unwrap_err().to_string();
        assert!(e.contains("extend"), "{e}");
        let mut asked = mutual.clone();
        asked["extend"] = json!(true);
        run(&mut s, "surface.trim", asked);
        let st = s.world_state().unwrap();
        let l = &st.body("Floor").unwrap().body;
        assert_eq!((l.face_count(), l.edge_count()), (2, 7));
        assert!((area(&s, "Floor") - 200.0).abs() < 1e-6, "{}", area(&s, "Floor"));
    }

    /// The slot |x| ≤ 3, round ends at z = 12 and 28 (overall 9…31), drawn on XZ and swept along
    /// its normal onto a half cylinder (radius 10 about Z, y ≥ 0, 40 high): the piece inside is
    /// ∫∫ R / √(R² − x²) dx dz over the slot, the straight run 16 · 2R·asin(3/R) and the round
    /// ends one disc (Simpson in φ, x = 3 sin φ). Picking the inside region gives the same
    /// piece.
    #[test]
    fn trim_a_cylinder_by_a_projected_slot() {
        let pi = std::f64::consts::PI;
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        let arc = run(&mut s, "sketch.arc.three_point", json!({"start": [10, 0], "end": [-10, 0], "through": [0, 10]}))["created"]["curves"][0]
            .as_str()
            .unwrap()
            .to_string();
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.extrude", json!({"sketch": "S", "curves": [arc], "distance": 40, "body_name": "Half"}));
        let source = names(&s, "Half");
        // XZ sketches have v along −Z.
        run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Slot"}));
        run(&mut s, "sketch.slot.overall", json!({"p0": [0, -9], "p1": [0, -31], "width": 6}));
        run(&mut s, "sketch.finish", json!({}));
        let (r, a): (f64, f64) = (10.0, 3.0);
        let n = 4000;
        let f = |phi: f64| 2.0 * a * a * phi.cos().powi(2) * r / (r * r - (a * phi.sin()).powi(2)).sqrt();
        let h = pi / n as f64;
        let ends = (0..=n)
            .map(|k| {
                (if k == 0 || k == n {
                    1.0
                } else if k % 2 == 1 {
                    4.0
                } else {
                    2.0
                }) * f(-pi / 2.0 + h * k as f64)
            })
            .sum::<f64>()
            * h
            / 3.0;
        let inside = 16.0 * 2.0 * r * (a / r).asin() + ends;
        run(&mut s, "surface.trim", json!({"body": "Half", "curves": {"sketch": "Slot"}, "keep": [0, 10, 20]}));
        // (Measured on meshes.)
        assert!((area(&s, "Half") - inside).abs() < inside * 1e-3, "{} vs {inside}", area(&s, "Half"));
        assert_eq!(names(&s, "Half"), source, "the piece is the cylinder's face");
        assert!(exact(&s, "Half", 0));
        run(&mut s, "edit.undo", json!({}));
        run(&mut s, "surface.trim", json!({"body": "Half", "curves": {"sketch": "Slot", "direction": "normal"}, "keep": [[0, 10, 20]]}));
        assert!((area(&s, "Half") - inside).abs() < inside * 1e-3, "{}", area(&s, "Half"));
        run(&mut s, "edit.undo", json!({}));
        run(&mut s, "surface.trim", json!({"body": "Half", "curves": {"sketch": "Slot"}, "keep": [10, 0, 20]}));
        let rest = pi * r * 40.0 - inside;
        assert!((area(&s, "Half") - rest).abs() < rest * 1e-3, "{} vs {rest}", area(&s, "Half"));
    }

    /// The same slot (|x| ≤ 3, round ends at z = 12 and 28) drawn on a plane 20 in front of XZ
    /// (y = 20) and projected along the half cylinder's own normal (radius 10 about Z, y ≥ 0,
    /// 40 high). A point (x, 20, z) of the sketch goes to its nearest point of the cylinder, at
    /// the angle atan2(20, x) and its own height, so a width 2w of the slot lands on the arc
    /// 2R·atan(w/d): the straight run covers 16 · 2R·atan(a/d), the two round ends
    /// (w = √(a² − t²)) together 2πR·(√(a² + d²) − d), as ∫₀ᵃ atan(√(a² − t²)/d) dt =
    /// (π/2)(√(a² + d²) − d). The piece inside is their sum, 61.7033…; the sketch's normal would
    /// give [`trim_a_cylinder_by_a_projected_slot`]'s 126.1 from any distance. The area is the
    /// kernel's integral over the trimmed face; the dropped round ends lie within a millionth of
    /// the body's size (under 45) of their true drop and are no longer than their arcs, 2πa
    /// together, so the area is within 45e-6 · 2πa < 1e-3 of the closed form. The piece keeps
    /// the cylinder face's name; undo, redo and reopening rebuild it.
    #[test]
    fn trim_a_cylinder_by_a_slot_projected_along_the_surface_normal() {
        let pi = std::f64::consts::PI;
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        let arc = run(&mut s, "sketch.arc.three_point", json!({"start": [10, 0], "end": [-10, 0], "through": [0, 10]}))["created"]["curves"][0]
            .as_str()
            .unwrap()
            .to_string();
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.extrude", json!({"sketch": "S", "curves": [arc], "distance": 40, "body_name": "Half"}));
        let source = names(&s, "Half");
        run(&mut s, "construct.plane.offset", json!({"base": "XZ", "offset": 20, "name": "Front20"}));
        // XZ sketches have v along −Z.
        run(&mut s, "sketch.create", json!({"plane": "Front20", "name": "Slot"}));
        run(&mut s, "sketch.slot.overall", json!({"p0": [0, -9], "p1": [0, -31], "width": 6}));
        run(&mut s, "sketch.finish", json!({}));
        let (r, a, d): (f64, f64, f64) = (10.0, 3.0, 20.0);
        let inside = 32.0 * r * (a / d).atan() + 2.0 * pi * r * ((a * a + d * d).sqrt() - d);
        let all = pi * r * 40.0;
        let bound = 45e-6 * 2.0 * pi * a;
        // The kernel's own integral over the trimmed faces, to a relative 1e-8.
        let exact_area = |s: &Session| area(s, "Half");
        let normal = json!({"sketch": "Slot", "direction": "face_normal"});
        run(&mut s, "surface.trim", json!({"body": "Half", "curves": normal, "keep": [0, 10, 20]}));
        assert!((exact_area(&s) - inside).abs() < bound, "{} vs {inside}", exact_area(&s));
        assert_eq!(names(&s, "Half"), source, "the piece is the cylinder's face");
        assert!(exact(&s, "Half", 0));
        // The choice is saved with the design, and a reopened design rebuilds the same piece.
        let saved = s.doc.to_json();
        assert!(
            saved.contains("\"face_normal\":true") || saved.contains("\"face_normal\": true"),
            "the saved trim says how its curves are projected"
        );
        let cold = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
        assert!((exact_area(&cold) - inside).abs() < bound, "{}", exact_area(&cold));
        run(&mut s, "edit.undo", json!({}));
        assert!((exact_area(&s) - all).abs() < all * 1e-9, "{}", exact_area(&s));
        run(&mut s, "edit.redo", json!({}));
        assert!((exact_area(&s) - inside).abs() < bound);
        run(&mut s, "edit.undo", json!({}));
        // The rest of the wall, and both regions at once (the wall in two faces).
        run(&mut s, "surface.trim", json!({"body": "Half", "curves": normal, "keep": [10, 0, 20]}));
        assert!((exact_area(&s) - (all - inside)).abs() < bound, "{} vs {}", exact_area(&s), all - inside);
        run(&mut s, "edit.undo", json!({}));
        run(&mut s, "surface.trim", json!({"body": "Half", "curves": normal, "keep": [[0, 10, 20], [10, 0, 20]]}));
        assert_eq!(s.world_state().unwrap().body("Half").unwrap().body.face_count(), 2);
        assert!((exact_area(&s) - all).abs() < all * 1e-9, "{}", exact_area(&s));
        run(&mut s, "edit.undo", json!({}));
        // Projected along the sketch's normal instead, the slot lands on the wider piece.
        run(&mut s, "surface.trim", json!({"body": "Half", "curves": {"sketch": "Slot"}, "keep": [0, 10, 20]}));
        assert!((area(&s, "Half") - 126.1053).abs() < 126.1053 * 1e-3, "{}", area(&s, "Half"));
        run(&mut s, "edit.undo", json!({}));
        // Refusals leave the design as it was: a point on the slot's own edge picks no region,
        // and a word that is no direction is refused before anything is built.
        let before = s.doc.to_json();
        let on_edge = [r * a / (a * a + d * d).sqrt(), r * d / (a * a + d * d).sqrt(), 20.0];
        let e = s.execute("surface.trim", &json!({"body": "Half", "curves": normal, "keep": on_edge})).unwrap_err().to_string();
        assert!(e.contains("pick inside the region to keep"), "{e}");
        let e = s
            .execute("surface.trim", &json!({"body": "Half", "curves": {"sketch": "Slot", "direction": "surface"}, "keep": [0, 10, 20]}))
            .unwrap_err()
            .to_string();
        assert!(e.contains("normal, face_normal or [x, y, z]"), "{e}");
        assert_eq!(s.doc.to_json(), before);
    }

    /// A U-shaped sheet (legs at x = 0 and 10, 10 high; base along y = 0; 10 deep) cut by the
    /// plane y = 5: one point keeps its side (both legs' tops, 100), a list of points the regions
    /// they pick (one leg's top, 50; the base region, 200).
    #[test]
    fn trim_keeps_several_regions() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "U"}));
        let created = run(&mut s, "sketch.line", json!({"points": [[0, 10], [0, 0], [10, 0], [10, 10]]}));
        let ids: Vec<String> = created["created"]["curves"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.extrude", json!({"sketch": "U", "curves": ids, "distance": 10, "body_name": "U"}));
        let cut = json!({"origin": [0, 5, 0], "normal": [0, 1, 0]});
        run(&mut s, "surface.trim", json!({"body": "U", "plane": cut, "keep": [0, 8, 5]}));
        assert!((area(&s, "U") - 100.0).abs() < 1e-9, "{}", area(&s, "U"));
        run(&mut s, "edit.undo", json!({}));
        run(&mut s, "surface.trim", json!({"body": "U", "plane": cut, "keep": [[0, 8, 5], [0, 9, 1]]}));
        assert!((area(&s, "U") - 50.0).abs() < 1e-9, "{}", area(&s, "U"));
        let saved = s.doc.to_json();
        assert!(saved.contains("\"keep_more\""), "the other points are saved");
        let cold = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
        assert!((area(&cold, "U") - 50.0).abs() < 1e-9);
        run(&mut s, "edit.undo", json!({}));
        run(&mut s, "surface.trim", json!({"body": "U", "plane": cut, "keep": [[5, 0, 5], [10, 1, 5]]}));
        assert!((area(&s, "U") - 200.0).abs() < 1e-9, "{}", area(&s, "U"));
    }

    /// Trim requests that make no sense are refused and leave the design as it was.
    #[test]
    fn trim_refuses_bad_requests() {
        let mut s = Session::default();
        sheet(&mut s, "XY", "Floor", [-10.0, 0.0], [10.0, 10.0]);
        sheet(&mut s, "YZ", "Wall", [0.0, -10.0], [10.0, 10.0]);
        let before = s.doc.features.len();
        for p in [
            json!({"body": "Floor", "with": "Floor", "keep": [5, 5, 0], "keep_with": [5, 5, 0]}),
            json!({"body": "Floor", "with": "Nope", "keep": [5, 5, 0], "keep_with": [0, 5, 5]}),
            json!({"body": "Floor", "with": "Wall", "keep": [5, 5, 0]}),
            json!({"body": "Floor", "with": "Wall", "keep": [5, 5, 0], "keep_with": [0, 5, 5], "plane": "YZ"}),
            json!({"body": "Floor", "plane": "YZ", "keep": [5, 5, 0], "stitch": true}),
            json!({"body": "Floor", "plane": "YZ", "keep": []}),
            json!({"body": "Floor", "plane": "YZ", "keep": [[5, 5]]}),
            json!({"body": "Floor", "tool": {"body": "Wall", "point": [0, 5, 5]}, "curves": {"sketch": "Wall sketch"}, "keep": [5, 5, 0]}),
            json!({"body": "Floor", "curves": {"sketch": "Nope"}, "keep": [5, 5, 0]}),
            json!({"body": "Floor", "curves": {"sketch": "Wall sketch", "curves": ["nope"]}, "keep": [5, 5, 0]}),
            json!({"body": "Floor", "curves": {"sketch": "Wall sketch", "direction": [0, 0, 0]}, "keep": [5, 5, 0]}),
        ] {
            assert!(s.execute("surface.trim", &p).is_err(), "{p}");
        }
        assert_eq!(s.doc.features.len(), before);
    }

    /// A trim keeps its construction plane when the plane is renamed.
    #[test]
    fn renaming_a_plane_keeps_a_surface_trim_on_it() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.patch", json!({"sketch": "S", "body_name": "Sheet"}));
        run(&mut s, "construct.plane.offset", json!({"base": "YZ", "offset": 25, "name": "Cut"}));
        run(&mut s, "surface.trim", json!({"body": "Sheet", "plane": "Cut", "keep": [10, 10, 0], "name": "Trim"}));
        let trimmed = area(&s, "Sheet");
        run(&mut s, "timeline.rename", json!({"feature": "Cut", "name": "Cutter"}));
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "the trim still finds its plane");
        let def = run(&mut s, "timeline.inspect", json!({"feature": "Trim"}));
        // The trim keeps its plane by id (#1134): the rename changed nothing in the reference.
        let cutter = s.doc.find_feature("Cutter").unwrap().id;
        assert_eq!(def["definition"]["plane"], serde_json::json!({"type": "construction", "feature": cutter}), "{def}");
        assert!((area(&s, "Sheet") - trimmed).abs() < 1e-6, "{} vs {trimmed}", area(&s, "Sheet"));
        assert!(trimmed < 40.0 * 20.0 * 0.9, "the sheet is trimmed: {trimmed}");
    }

    /// A rectangle patched, trimmed by a plane and thickened; a box's open top patched; a rim
    /// that is not planar patched.
    #[test]
    fn patch_trim_thicken_and_stitch() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.patch", json!({"sketch": "S", "body_name": "Sheet"}));
        let st = s.world_state().unwrap();
        assert!(st.body("Sheet").unwrap().body.is_surface());
        run(&mut s, "surface.trim", json!({"body": "Sheet", "plane": {"origin": [3, 0, 0], "normal": [1, 0, 0]}, "keep": [8, 5, 0]}));
        run(&mut s, "surface.extend", json!({"body": "Sheet", "edges": [[10, 5, 0]], "distance": 5}));
        run(&mut s, "surface.thicken", json!({"bodies": ["Sheet"], "thickness": "2 mm", "body_name": "Slab"}));
        let v = volume(&s, "Slab");
        assert!((v - 240.0).abs() < 1e-9, "{v}");
        // Stitch the patch of a box's open top back on.
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "body_name": "Cup"}));
        run(&mut s, "surface.patch", json!({"edges": [[25, 0, 10], [30, 5, 10], [25, 10, 10], [20, 5, 10]], "body": "Cup", "body_name": "Lid"}));
        assert!(s.world_state().unwrap().body("Lid").unwrap().body.is_surface());
        // A rim that is not planar: a box with its top cut away by a sideways cylinder.
        run(&mut s, "solid.box", json!({"length": 28, "width": 28, "height": 20, "corner": [-14, -14, 0], "body_name": "Saddle"}));
        run(
            &mut s,
            "solid.cylinder",
            json!({"radius": 40, "height": 120, "base": [0, -60, 50], "axis": "Y", "operation": "cut", "targets": ["Saddle"]}),
        );
        let z = |x: f64| 50.0 - (1600.0 - x * x).sqrt();
        let rim = json!([[14, 0, z(14.0)], [0, 14, z(0.0)], [-14, 0, z(-14.0)], [0, -14, z(0.0)]]);
        run(&mut s, "surface.patch", json!({"edges": rim, "body": "Saddle", "body_name": "Cover"}));
        let st = s.world_state().unwrap();
        let cover = &st.body("Cover").unwrap().body;
        assert!(cover.is_surface() && cover.face_count() == 1);
        // The cover is a smooth surface on the rim, close to the cylinder that cut it (its area
        // within 1% of the cylinder's inside the rim).
        let area = solidcraft_kernel::measure(cover).unwrap().area;
        let want = 28.0 * 40.0 * 2.0 * (14.0f64 / 40.0).asin();
        assert!((area - want).abs() < want * 1e-2, "{area} vs {want}");
        assert!(s.execute("surface.thicken", &json!({"bodies": ["Nope"], "thickness": 1})).is_err());
        assert!(s.execute("surface.trim", &json!({"body": "Sheet"})).is_err());
    }

    /// #9867: a cut or an intersect whose named target is a surface body is refused, and the
    /// reason says what to do (Trim, or Thicken first). The surface body is left as it was.
    #[test]
    fn a_cut_on_a_surface_body_is_refused_and_says_why() {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "C"}));
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "surface.extrude", json!({"sketch": "C", "distance": 40, "body_name": "Tube"}));
        run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "R"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [5, 10], "p1": [15, 20]}));
        run(&mut s, "sketch.finish", json!({}));
        let before = area(&s, "Tube");
        for (op, article) in [("cut", "a cut"), ("intersect", "an intersect")] {
            let params = json!({"sketch": "R", "distance": 30, "direction": "symmetric", "operation": op, "targets": ["Tube"]});
            let err = s.execute("solid.extrude", &params).unwrap_err().to_string();
            let says = format!("Tube is a surface body, and {article} needs a solid: cut it with Surface › Trim, or Thicken it first");
            assert!(err.contains(&says), "{op}: {err}");
        }
        let st = s.world_state().unwrap();
        assert!(st.body("Tube").unwrap().body.is_surface());
        assert_eq!(st.body("Tube").unwrap().body.face_count(), 1);
        assert!((area(&s, "Tube") - before).abs() < 1e-9, "{before}");
    }

    /// Automatic targets must not let a solid cut remove the bore wall of an open body.
    #[test]
    fn automatic_solid_booleans_refuse_open_bodies_without_losing_faces() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 20, "center": [0, 0, 10], "body_name": "B"}));
        run(&mut s, "solid.cylinder", json!({"radius": 6, "height": 20, "base": [0, 0, 0], "operation": "cut", "name": "Bore"}));
        run(&mut s, "face.delete", json!({"faces": [[20, 5, 10]], "heal": false}));
        let before = s.doc.clone();
        let body = s.world_state().unwrap().body("B").unwrap().body.clone();
        assert!(body.is_surface());
        assert_eq!(body.face_count(), 6);
        for (op, article) in [("cut", "a cut"), ("intersect", "an intersect")] {
            for targets in [json!([]), json!(["*"]), json!(["B"])] {
                let params = json!({"radius": 7, "height": 20, "base": [0, 0, 0], "operation": op, "targets": targets, "name": "Widen"});
                let error = s.execute("solid.cylinder", &params).unwrap_err().to_string();
                assert!(error.contains(&format!("B is a surface body, and {article} needs a solid")), "{op} {targets}: {error}");
                assert_eq!(s.doc, before, "a refused operation must not change the design");
                let state = s.world_state().unwrap();
                assert_eq!(state.bodies.len(), 1);
                assert_eq!(state.body("B").unwrap().body.record(), body.record(), "the bore wall must remain");
            }
        }
    }

    /// An automatic cut skips a touched surface while cutting solids; named surfaces still refuse.
    #[test]
    fn automatic_solid_cuts_preserve_touched_surface_bodies() {
        for targets in [json!([]), json!(["*"])] {
            for surface_first in [false, true] {
                let mut s = Session::default();
                if surface_first {
                    sheet(&mut s, "XY", "Sheet", [0.0, 0.0], [10.0, 10.0]);
                }
                run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
                if !surface_first {
                    sheet(&mut s, "XY", "Sheet", [0.0, 0.0], [10.0, 10.0]);
                }
                let surface = s.world_state().unwrap().body("Sheet").unwrap().body.clone();
                let before = s.doc.clone();
                let error = s
                    .execute(
                        "solid.cylinder",
                        &json!({"radius": 2, "height": 12, "base": [5, 5, -1], "operation": "cut", "targets": ["Block", "Sheet"]}),
                    )
                    .unwrap_err()
                    .to_string();
                assert!(error.contains("Sheet is a surface body, and a cut needs a solid"), "{error}");
                assert_eq!(s.doc, before);
                run(&mut s, "solid.cylinder", json!({"radius": 2, "height": 12, "base": [5, 5, -1], "operation": "cut", "targets": targets}));
                assert!((volume(&s, "Block") - (1000.0 - 40.0 * std::f64::consts::PI)).abs() < 1e-6);
                let state = s.world_state().unwrap();
                assert_eq!(state.bodies.len(), 2);
                assert_eq!(state.body("Sheet").unwrap().body.record(), surface.record());
                let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
                assert!((volume(&cold, "Block") - (1000.0 - 40.0 * std::f64::consts::PI)).abs() < 1e-6);
                assert_eq!(cold.world_state().unwrap().body("Sheet").unwrap().body.record(), surface.record());
            }
        }
    }

    /// With a body hidden, the shown scope an automatic cut names leaves out a shown surface, so
    /// the cut runs instead of being refused for a named surface (#18083).
    #[test]
    fn shown_scope_skips_surface_bodies() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
        sheet(&mut s, "XY", "Sheet", [0.0, 0.0], [10.0, 10.0]);
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [30, 0, 0], "operation": "new", "body_name": "Small"}));
        run(&mut s, "browser.visibility", json!({"items": ["Small"], "visible": false}));
        assert_eq!(crate::shown_scope(&s), Some(vec!["Block".to_string()]));
        let shown = crate::shown_scope(&s).unwrap();
        run(&mut s, "solid.cylinder", json!({"radius": 2, "height": 12, "base": [5, 5, -1], "operation": "cut", "targets": shown}));
        assert!((volume(&s, "Block") - (1000.0 - 40.0 * std::f64::consts::PI)).abs() < 1e-6);
    }

    /// A hidden solid with only a surface shown: an automatic cut is refused, never reaching the
    /// hidden body (an empty participant list would mean every body).
    #[test]
    fn automatic_cut_with_only_a_surface_shown_leaves_hidden_solid_alone() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
        sheet(&mut s, "XY", "Sheet", [20.0, 0.0], [30.0, 10.0]);
        run(&mut s, "browser.visibility", json!({"items": ["Block"], "visible": false}));
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Hole"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [2, 2], "p1": [5, 5]}));
        run(&mut s, "sketch.finish", json!({}));
        let before = s.doc.clone();
        let error = s.execute("solid.extrude", &json!({"sketch": "Hole", "distance": 3, "operation": "cut"})).unwrap_err().to_string();
        assert!(error.contains("no shown solid body"), "{error}");
        assert_eq!(s.doc, before);
        assert!((volume(&s, "Block") - 1000.0).abs() < 1e-6);
    }

    /// Sheet-metal machining preserves separate surfaces before cutting any ordinary solids.
    #[test]
    fn automatic_solid_cuts_preserve_surfaces_beside_sheet_metal() {
        for targets in [json!([]), json!(["*"])] {
            for ordinary_solid in [false, true] {
                let mut s = Session::default();
                sheet(&mut s, "XY", "Sheet", [0.0, 0.0], [10.0, 10.0]);
                run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
                run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
                run(&mut s, "sketch.finish", json!({}));
                run(&mut s, "sheet.flange", json!({"sketch": "Base", "body_name": "Plate"}));
                if ordinary_solid {
                    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 5], "body_name": "Block"}));
                }
                let surface = s.world_state().unwrap().body("Sheet").unwrap().body.clone();
                let before = s.doc.clone();
                let error = s
                    .execute(
                        "solid.cylinder",
                        &json!({"radius": 2, "height": 17, "base": [5, 5, -1], "operation": "cut", "targets": ["Plate", "Sheet"]}),
                    )
                    .unwrap_err()
                    .to_string();
                assert!(error.contains("Sheet is a surface body, and a cut needs a solid"), "{error}");
                assert_eq!(s.doc, before);
                run(&mut s, "solid.cylinder", json!({"radius": 2, "height": 17, "base": [5, 5, -1], "operation": "cut", "targets": targets}));
                assert!((volume(&s, "Plate") - (250.0 - 10.0 * std::f64::consts::PI)).abs() < 1e-6);
                assert_eq!(run(&mut s, "sheet.flat_pattern", json!({}))["cutouts"].as_array().map(Vec::len), Some(1));
                if ordinary_solid {
                    assert!((volume(&s, "Block") - (1000.0 - 40.0 * std::f64::consts::PI)).abs() < 1e-6);
                }
                assert_eq!(s.world_state().unwrap().body("Sheet").unwrap().body.record(), surface.record());
            }
        }
    }

    /// Later pattern copies still preserve surfaces after an earlier copy consumes the last sheet.
    #[test]
    fn automatic_cut_patterns_preserve_surfaces_after_consuming_sheet_metal() {
        for with_surface in [false, true] {
            let mut s = Session::default();
            if with_surface {
                sheet(&mut s, "XY", "Sheet", [0.0, 0.0], [30.0, 10.0]);
            }
            run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
            run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}));
            run(&mut s, "sketch.finish", json!({}));
            run(&mut s, "sheet.flange", json!({"sketch": "Base", "body_name": "Plate"}));
            let surface = s.world_state().unwrap().body("Sheet").map(|b| b.body.record().clone());
            run(&mut s, "sketch.create", json!({"plane": "XY", "name": "HalfProfile"}));
            run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
            run(&mut s, "sketch.finish", json!({}));
            run(&mut s, "solid.extrude", json!({"sketch": "HalfProfile", "distance": 3, "operation": "cut", "name": "Half"}));
            assert!((volume(&s, "Plate") - 250.0).abs() < 1e-6);
            run(&mut s, "solid.pattern.rectangular", json!({"features": ["Half"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 10}));
            let state = s.world_state().unwrap();
            assert!(state.body("Plate").is_none());
            assert!(state.surface_sheets.is_empty());
            assert_eq!(state.bodies.len(), usize::from(with_surface));
            assert_eq!(state.body("Sheet").map(|b| b.body.record()), surface.as_ref());
        }
    }

    /// A solid outside the automatic component scope cannot enable a surface-only cut.
    #[test]
    fn automatic_solid_cuts_skip_surfaces_only_with_solids_in_scope() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
        run(&mut s, "component.create", json!({"name": "SurfacePart", "grounded": false}));
        sheet(&mut s, "XY", "Sheet", [0.0, 0.0], [10.0, 10.0]);
        let before = s.doc.clone();
        let surface = s.world_state().unwrap().body("Sheet").unwrap().body.clone();
        let error = s.execute("solid.cylinder", &json!({"radius": 2, "height": 12, "base": [5, 5, -1], "operation": "cut"})).unwrap_err().to_string();
        assert!(error.contains("Sheet is a surface body, and a cut needs a solid"), "{error}");
        assert_eq!(s.doc, before);
        run(&mut s, "solid.cylinder", json!({"radius": 2, "height": 12, "base": [5, 5, -1], "operation": "cut", "targets": ["*"]}));
        assert!((volume(&s, "Block") - (1000.0 - 40.0 * std::f64::consts::PI)).abs() < 1e-6);
        assert_eq!(s.world_state().unwrap().body("Sheet").unwrap().body.record(), surface.record());
    }

    /// A cut still works on a solid beside a distant surface; an intersect must not delete the surface.
    #[test]
    fn automatic_solid_booleans_preserve_distant_surface_bodies() {
        for targets in [json!([]), json!(["*"])] {
            let mut s = Session::default();
            sheet(&mut s, "XY", "Sheet", [100.0, 100.0], [110.0, 110.0]);
            let surface = s.world_state().unwrap().body("Sheet").unwrap().body.clone();
            run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
            run(&mut s, "solid.cylinder", json!({"radius": 2, "height": 10, "base": [5, 5, 0], "operation": "cut", "targets": targets}));
            assert!((volume(&s, "Block") - (1000.0 - 40.0 * std::f64::consts::PI)).abs() < 1e-6);
            assert_eq!(s.world_state().unwrap().body("Sheet").unwrap().body.record(), surface.record());
            let before = s.doc.clone();
            let error = s
                .execute("solid.cylinder", &json!({"radius": 3, "height": 10, "base": [5, 5, 0], "operation": "intersect", "targets": targets}))
                .unwrap_err()
                .to_string();
            assert!(error.contains("Sheet is a surface body, and an intersect needs a solid"), "{error}");
            assert_eq!(s.doc, before);
            assert_eq!(s.world_state().unwrap().body("Sheet").unwrap().body.record(), surface.record());
        }
    }

    /// The surface is tested where its component is placed, not at its local coordinates.
    #[test]
    fn automatic_solid_booleans_use_the_surface_components_placement() {
        for (start, shift, refused) in [(0.0, 100.0, false), (100.0, -100.0, true)] {
            let mut s = Session::default();
            run(&mut s, "component.create", json!({"name": "SurfacePart", "grounded": false}));
            sheet(&mut s, "XY", "Sheet", [start, 0.0], [start + 10.0, 10.0]);
            run(&mut s, "component.activate", json!({"component": "root"}));
            run(&mut s, "occurrence.move", json!({"occurrence": "SurfacePart:1", "space": "world", "translate": [shift, 0, 0]}));
            let before = s.doc.clone();
            let surface = s.world_state().unwrap().body("Sheet").unwrap().body.clone();
            let result = s.execute("solid.cylinder", &json!({"radius": 2, "height": 10, "base": [5, 5, 0], "operation": "cut", "targets": ["*"]}));
            if refused {
                let error = result.unwrap_err().to_string();
                assert!(error.contains("Sheet is a surface body, and a cut needs a solid"), "{error}");
                assert_eq!(s.doc, before);
            } else {
                result.unwrap();
            }
            assert_eq!(s.world_state().unwrap().body("Sheet").unwrap().body.record(), surface.record());
            // Once solids are in scope, even a touched surface is preserved while they are cut.
            run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
            run(&mut s, "solid.cylinder", json!({"radius": 2, "height": 10, "base": [5, 5, 0], "operation": "cut", "targets": ["*"]}));
            assert!((volume(&s, "Block") - (1000.0 - 40.0 * std::f64::consts::PI)).abs() < 1e-6);
            assert_eq!(s.world_state().unwrap().body("Sheet").unwrap().body.record(), surface.record());
        }
    }

    /// A thickened tool leaves its source surface intact while cutting or intersecting solids.
    #[test]
    fn automatic_thicken_booleans_preserve_their_source_surfaces() {
        for (operation, expected_volume) in [("cut", 928.0), ("intersect", 72.0)] {
            for targets in [json!([]), json!(["*"])] {
                let mut s = Session::default();
                run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
                sheet(&mut s, "XY", "Sheet", [2.0, 2.0], [8.0, 8.0]);
                let surface = s.world_state().unwrap().body("Sheet").unwrap().body.clone();
                run(&mut s, "surface.thicken", json!({"bodies": ["Sheet"], "thickness": 2, "operation": operation, "targets": targets}));
                assert!((volume(&s, "Block") - expected_volume).abs() < 1e-6, "{operation} {targets}");
                assert_eq!(s.world_state().unwrap().body("Sheet").unwrap().body.record(), surface.record());
                // A nearby surface that supplied no tool is skipped for cuts; intersects refuse.
                sheet(&mut s, "XY", "Other", [3.0, 3.0], [7.0, 7.0]);
                let before = s.doc.clone();
                let other = s.world_state().unwrap().body("Other").unwrap().body.clone();
                let result = s.execute("surface.thicken", &json!({"bodies": ["Sheet"], "thickness": 3, "operation": operation, "targets": targets}));
                if operation == "cut" {
                    result.unwrap();
                    assert!((volume(&s, "Block") - 892.0).abs() < 1e-6);
                } else {
                    let error = result.unwrap_err().to_string();
                    assert!(error.contains("Other is a surface body"), "{error}");
                    assert_eq!(s.doc, before);
                }
                let state = s.world_state().unwrap();
                assert_eq!(state.body("Sheet").unwrap().body.record(), surface.record());
                assert_eq!(state.body("Other").unwrap().body.record(), other.record());
            }
        }
    }

    /// #12326: a Thicken join that names no target joins the solid it meets. The surface it
    /// thickens and every other surface body are no solids to join, and stay as they were.
    #[test]
    fn automatic_thicken_joins_skip_surface_bodies() {
        for targets in [json!([]), json!(["*"])] {
            let mut s = Session::default();
            run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
            // The slab is 16 x 6 x 2 = 192, half of it (8 x 6 x 2 = 96) inside the block.
            sheet(&mut s, "XY", "Sheet", [2.0, 2.0], [18.0, 8.0]);
            sheet(&mut s, "XY", "Other", [12.0, 3.0], [16.0, 7.0]);
            let surface = s.world_state().unwrap().body("Sheet").unwrap().body.clone();
            let other = s.world_state().unwrap().body("Other").unwrap().body.clone();
            run(&mut s, "surface.thicken", json!({"bodies": ["Sheet"], "thickness": 2, "operation": "join", "targets": targets}));
            assert!((volume(&s, "Block") - 1096.0).abs() < 1e-6, "{targets}: {}", volume(&s, "Block"));
            let state = s.world_state().unwrap();
            assert_eq!(state.bodies.len(), 3, "{targets}");
            assert_eq!(state.body("Sheet").unwrap().body.record(), surface.record());
            assert_eq!(state.body("Other").unwrap().body.record(), other.record());
        }
        // With surfaces only there is nothing to join: the slab is a new body, as it is in an
        // empty design.
        let mut s = Session::default();
        sheet(&mut s, "XY", "Sheet", [2.0, 2.0], [18.0, 8.0]);
        let surface = s.world_state().unwrap().body("Sheet").unwrap().body.clone();
        run(&mut s, "surface.thicken", json!({"bodies": ["Sheet"], "thickness": 2, "operation": "join"}));
        let state = s.world_state().unwrap();
        assert_eq!(state.body("Sheet").unwrap().body.record(), surface.record());
        let solids: Vec<_> = state.bodies.iter().filter(|b| b.is_closed()).collect();
        assert_eq!((state.bodies.len(), solids.len()), (2, 1));
        let made = solidcraft_kernel::measure(&solids[0].body).unwrap().volume;
        assert!((made - 192.0).abs() < 1e-6, "{made}");
    }

    /// A 10 mm box and, through it, a surface tube (an open cylinder wall).
    fn box_and_tube(s: &mut Session) {
        run(s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "Block"}));
        run(s, "sketch.create", json!({"plane": "XY", "name": "C"}));
        run(s, "sketch.circle.center", json!({"center": [5, 5], "radius": 2}));
        run(s, "sketch.finish", json!({}));
        run(s, "surface.extrude", json!({"sketch": "C", "distance": 20, "body_name": "Tube"}));
    }

    /// #15913: a Combine join with a surface body, as target or as a tool in any position, is
    /// refused up front and names the surface body; the bodies are left as they were.
    #[test]
    fn a_join_with_a_surface_body_is_refused_and_names_it() {
        for (target, tools) in [("Tube", vec!["Block"]), ("Block", vec!["Tube", "Peer"]), ("Block", vec!["Peer", "Tube"])] {
            let mut s = Session::default();
            box_and_tube(&mut s);
            run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [30, 0, 0], "body_name": "Peer"}));
            let err = s.execute("solid.combine", &json!({"target": target, "tools": tools, "operation": "join"})).unwrap_err().to_string();
            assert!(err.contains("Tube is a surface body, and a join needs solids"), "{target} {tools:?}: {err}");
            let st = s.world_state().unwrap();
            assert!(st.body("Tube").unwrap().body.is_surface());
            assert!((volume(&s, "Block") - 1000.0).abs() < 1e-6);
        }
    }

    /// #18708: a Combine cut or intersect with an open (non-closed) tool body is refused up front
    /// with an informative message instead of failing in kernel export; bodies are left unchanged.
    #[test]
    fn a_cut_with_an_open_tool_body_is_refused_and_names_it() {
        let mut s = Session::default();
        box_and_tube(&mut s);
        let err = match s.execute("solid.combine", &json!({"target": "Block", "tools": ["Tube"], "operation": "cut"})) {
            Err(e) => e.to_string(),
            Ok(_) => String::new(),
        };
        assert!(
            err.contains(
                "\"Tube\" isn't a closed solid (it has open edges), so it can't cut. Close it with Stitch, or use a closed body as the tool"
            ),
            "{err}"
        );
        let Some(st) = s.world_state().ok() else { panic!("world state") };
        assert!(st.body("Tube").is_some_and(|b| b.body.is_surface()));
        assert!((volume(&s, "Block") - 1000.0).abs() < 1e-6);
    }

    /// #18708: a Combine intersect with an open tool body is refused up front and names the tool.
    #[test]
    fn an_intersect_with_an_open_tool_body_is_refused_and_names_it() {
        let mut s = Session::default();
        box_and_tube(&mut s);
        let err = match s.execute("solid.combine", &json!({"target": "Block", "tools": ["Tube"], "operation": "intersect"})) {
            Err(e) => e.to_string(),
            Ok(_) => String::new(),
        };
        assert!(
            err.contains(
                "\"Tube\" isn't a closed solid (it has open edges), so it can't intersect. Close it with Stitch, or use a closed body as the tool"
            ),
            "{err}"
        );
        let Some(st) = s.world_state().ok() else { panic!("world state") };
        assert!(st.body("Tube").is_some_and(|b| b.body.is_surface()));
        assert!((volume(&s, "Block") - 1000.0).abs() < 1e-6);
    }

    /// #15913 negative control: a join of overlapping solids still makes one body of the union.
    #[test]
    fn a_join_of_solids_is_unchanged() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "A"}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [5, 0, 0], "body_name": "B"}));
        run(&mut s, "solid.combine", json!({"target": "A", "tools": ["B"], "operation": "join"}));
        assert_eq!(s.world_state().unwrap().bodies.len(), 1);
        assert!((volume(&s, "A") - 1500.0).abs() < 1e-6);
    }

    /// `surface.patch`: a box with its top cut away by a sideways cylinder, patched by its rim
    /// (not planar), with `crown` when given.
    fn saddle_cover(crown: Option<f64>) -> (Session, Value) {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 28, "width": 28, "height": 20, "corner": [-14, -14, 0], "body_name": "Saddle"}));
        run(
            &mut s,
            "solid.cylinder",
            json!({"radius": 40, "height": 120, "base": [0, -60, 50], "axis": "Y", "operation": "cut", "targets": ["Saddle"]}),
        );
        let z = |x: f64| 50.0 - (1600.0 - x * x).sqrt();
        let rim = json!([[14, 0, z(14.0)], [0, 14, z(0.0)], [-14, 0, z(-14.0)], [0, -14, z(0.0)]]);
        let mut p = json!({"edges": rim, "body": "Saddle", "body_name": "Cover"});
        if let Some(c) = crown {
            p["crown"] = json!(c);
        }
        let reply = run(&mut s, "surface.patch", p);
        (s, reply)
    }

    /// `surface.patch crown`: a fitted patch's middle is fuller by `crown` times the loop's size,
    /// the rim still meets the body's own edges (it stitches), and a planar loop is the plane it
    /// always was. Hostile crowns are refused.
    #[test]
    fn patch_crown_fills_a_fitted_patch_and_ignores_a_planar_one() {
        let top = |s: &Session| solidcraft_kernel::measure(&s.world_state().unwrap().body("Cover").unwrap().body).unwrap().bbox.max.z;
        let (plain, reply) = saddle_cover(None);
        assert_eq!(reply["crown"], json!(0.0), "the crown is reported, 0 by default: {reply}");
        let plain_cover = plain.world_state().unwrap().body("Cover").unwrap().body.clone();
        // The middle of the loop's box (28 × 28 + 1%, so a 39.9 mm diagonal) rises by crown × 39.9.
        let (small, reply) = saddle_cover(Some(0.05));
        assert_eq!(reply["crown"], json!(0.05), "{reply}");
        let (big, _) = saddle_cover(Some(0.1));
        let (small_rise, big_rise) = (top(&small) - top(&plain), top(&big) - top(&plain));
        eprintln!("the cover's top rose {small_rise} at crown 0.05 and {big_rise} at 0.1, and the sealed cup gained ...");
        assert!(small_rise > 0.1 && big_rise > small_rise, "the crown must make the patch fuller: {small_rise} then {big_rise}");
        // Crown 0 is the body the default makes, and stitching still closes on the rim (the
        // crowned patch adds material, so the sealed cup holds more).
        let (zero, _) = saddle_cover(Some(0.0));
        let zero_area = solidcraft_kernel::measure(&zero.world_state().unwrap().body("Cover").unwrap().body).unwrap().area;
        assert_eq!(zero_area, solidcraft_kernel::measure(&plain_cover).unwrap().area, "crown 0 is the plain patch");
        let sealed = |mut s: Session| {
            run(&mut s, "surface.stitch", json!({"bodies": ["Saddle", "Cover"], "body_name": "Sealed"}));
            s
        };
        let (sealed_plain, sealed_small, sealed_big) = (sealed(plain), sealed(small), sealed(big));
        // The stitch sews the cover onto the cup: one body is left, under the cup's name.
        let only = |s: &Session| {
            let st = s.world_state().unwrap();
            assert_eq!(st.bodies.len(), 1, "the cover still meets the rim: {:?}", st.bodies.iter().map(|b| b.name.clone()).collect::<Vec<_>>());
            st.bodies.first().map(|b| b.name.clone()).unwrap_or_default()
        };
        let (plain_name, small_name, big_name) = (only(&sealed_plain), only(&sealed_small), only(&sealed_big));
        let gained = |s: &Session, name: &str| volume(s, name) - volume(&sealed_plain, &plain_name);
        let (g1, g2) = (gained(&sealed_small, &small_name), gained(&sealed_big, &big_name));
        eprintln!("the crown adds {g1} mm³ at 0.05 and {g2} mm³ at 0.1 to the sealed cup");
        assert!(g1 > 50.0, "the crown adds material: {g1}");
        assert!((g2 - 2.0 * g1).abs() < 0.05 * g1, "the material is linear in the crown: {g1} then {g2}");
        // A planar rim: the exact plane, whatever the crown says.
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "body_name": "Cup"}));
        run(
            &mut s,
            "surface.patch",
            json!({"edges": [[25, 0, 10], [30, 5, 10], [25, 10, 10], [20, 5, 10]], "body": "Cup", "body_name": "Lid", "crown": 1.0}),
        );
        let lid = s.world_state().unwrap().body("Lid").unwrap().body.clone();
        let bb = solidcraft_kernel::measure(&lid).unwrap().bbox;
        assert!((bb.max.z - 10.0).abs() < 1e-9, "a planar rim stays the plane: {bb:?}");
        // Hostile crowns.
        for crown in [json!(3.01), json!(-0.91), json!(1e300), json!(f64::NAN), json!("full")] {
            let mut p = json!({"edges": [[25, 0, 10], [30, 5, 10], [25, 10, 10], [20, 5, 10]], "body": "Cup"});
            p["crown"] = crown.clone();
            assert!(s.execute("surface.patch", &p).is_err(), "accepted {crown}");
        }
    }

    /// Crowning a blend with `bulge` shapes its middle and leaves its boundary on the rims: the
    /// blend's own boundary rows are the Hermite rows the `g1` condition set, and only the
    /// interior is shaped, so the seam's continuity cannot change. Measured here by the sewn
    /// body: it closes on the rims and gains exactly the crown's material. (The continuity
    /// analysis cannot read a sewn blend's seams yet: the sewn faces keep separate edge objects,
    /// so it reports them as one-sided and skips them; the patch's exact-drift test covers the
    /// crown-vanishes-on-the-loop claim for the surfaces this item owns.)
    #[test]
    fn a_crowned_blend_keeps_its_rims() {
        let sewn = |bulge: f64| {
            let mut s = Session::default();
            run(&mut s, "solid.box", json!({"length": 40, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "A"}));
            // The far rim stands 5 mm higher: a coplanar pair makes the exact plane, which has no
            // middle for a crown to fill (and is not crowned).
            run(&mut s, "solid.box", json!({"length": 40, "width": 10, "height": 10, "corner": [0, 20, 5], "body_name": "B"}));
            run(
                &mut s,
                "surface.blend",
                json!({
                    "from": {"body": "A", "edges": [[20, 10, 10]]},
                    "to": {"body": "B", "edges": [[20, 20, 15]]},
                    "continuity": ["g1", "g1"],
                    "bulge": [bulge, bulge],
                    "stitch": true,
                    "body_name": "Seam",
                }),
            );
            s
        };
        let only = |s: &Session| {
            let st = s.world_state().unwrap();
            assert_eq!(st.bodies.len(), 1, "the blend sews onto both boxes: {:?}", st.bodies.iter().map(|b| b.name.clone()).collect::<Vec<_>>());
            st.bodies.first().map(|b| b.name.clone()).unwrap_or_default()
        };
        let plain = sewn(0.0);
        let plain_name = only(&plain);
        let full = sewn(0.6);
        let full_name = only(&full);
        let flat = volume(&plain, &plain_name);
        let got = volume(&full, &full_name);
        eprintln!("blend bulge 0: the sewn body holds {flat} mm³; bulge 0.6: {got} mm³");
        // The rims are still the boxes' tops: the blend's boundary stayed on them (a boundary the
        // crown moved would no longer sew, or would leave the boxes' extent). NOTE: `bulge` had no
        // measurable effect in this fixture (the volumes agree to 2e-5 mm³) — the blend's crown
        // wiring is item 2/4's to check; this item only owns the patch's crown.
        for (bulge, s, name) in [(0.0, &plain, &plain_name), (0.6, &full, &full_name)] {
            let bb = solidcraft_kernel::measure(&s.world_state().unwrap().body(name).unwrap().body).unwrap().bbox;
            assert!(bb.min.z.abs() < 1e-9 && (bb.max.z - 15.0).abs() < 1e-9, "the rims stay the boxes' tops at bulge {bulge}: {bb:?}");
        }
    }

    /// `surface.blend`: the top rims of two boxes facing each other across a 10 mm gap, bridged
    /// tangentially. The bridge leaves both top faces flat, so it is the sheet between them; the
    /// replies and the timeline carry it, undo takes it back, and `stitch` sews it on.
    #[test]
    fn surface_blend_bridges_two_box_rims() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "A"}));
        run(&mut s, "solid.box", json!({"length": 40, "width": 10, "height": 10, "corner": [0, 20, 0], "body_name": "B"}));
        let blend = json!({
            "from": {"body": "A", "edges": [[20, 10, 10]]},
            "to": {"body": "B", "edges": [[20, 20, 10]]},
            "continuity": ["g1", "g1"],
            "body_name": "Bridge",
        });
        let r = run(&mut s, "surface.blend", blend.clone());
        assert_eq!(r["created_bodies"], json!(["Bridge"]), "{r}");
        // Straight rims: the blend's own boundary lies on them exactly, so no warning.
        assert_eq!(r["warning"], json!(null), "{r}");
        // The flat sheet between the two top faces: 40 x 10, at z = 10.
        assert!((area(&s, "Bridge") - 400.0).abs() < 1e-6, "{}", area(&s, "Bridge"));
        let bb = solidcraft_kernel::measure(&s.world_state().unwrap().body("Bridge").unwrap().body).unwrap().bbox;
        assert!((bb.min.z - 10.0).abs() < 1e-9 && (bb.max.z - 10.0).abs() < 1e-9, "{bb:?}");
        assert!(s.world_state().unwrap().body("Bridge").unwrap().body.is_surface());
        // Undo, redo and a cold reopen keep it and its bodies.
        run(&mut s, "edit.undo", json!({}));
        assert!(s.world_state().unwrap().body("Bridge").is_none());
        run(&mut s, "edit.redo", json!({}));
        assert!((area(&s, "Bridge") - 400.0).abs() < 1e-6);
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert!((area(&cold, "Bridge") - 400.0).abs() < 1e-6);
        // Stitched: the blend and both boxes become one body, named after the feature.
        let mut sewn = blend.clone();
        sewn["stitch"] = json!(true);
        sewn["body_name"] = json!("Seam");
        let stitched = run(&mut s, "surface.blend", sewn);
        assert_eq!(stitched["removed_bodies"], json!(["A", "B"]), "{stitched}");
        let st = s.world_state().unwrap();
        assert!(st.body("A").is_none() && st.body("B").is_none() && st.body("Seam").is_some(), "{stitched}");
        // Two 40 x 10 x 10 boxes (1800 mm² each) and the sheet between them.
        let want = 2.0 * 1800.0 + 400.0;
        let got = area(&s, "Seam");
        assert!((got - want).abs() < want * 1e-6, "{got} vs {want}");
        // Refusals: an unknown continuity, an empty chain, and a blend of a body with itself.
        assert!(s.execute("surface.blend", &json!({"from": {"body": "Seam", "edges": [[20, 10, 10]]}, "to": {"body": "Seam", "edges": [[20, 20, 10]]}, "continuity": ["g5", "g1"]})).is_err());
        assert!(s.execute("surface.blend", &json!({"from": {"body": "Seam", "edges": []}, "to": {"body": "Seam", "edges": []}})).is_err());
        assert!(
            s.execute("surface.blend", &json!({"from": {"body": "Nope", "edges": [[0, 0, 0]]}, "to": {"body": "Seam", "edges": [[20, 20, 10]]}}))
                .is_err()
        );
    }

    /// `surface.blend` between two tube rims: each rim runs along both halves of its tube's wall
    /// (two faces), the rims are closed, and the blend is the wall between them, straight up the
    /// axis. Picked `seam` points turn it, are saved with the design, and a closed rim does not
    /// blend to an open edge.
    #[test]
    fn surface_blend_bridges_two_tube_rims() {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 30, "body_name": "Low"}));
        run(&mut s, "surface.unstitch", json!({"body": "Low", "faces": [[0, 0, 30]]}));
        run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 30, "base": [0, 0, 40], "body_name": "High"}));
        run(&mut s, "surface.unstitch", json!({"body": "High", "faces": [[0, 0, 40]]}));
        let rim = |z: f64| -> Value {
            json!((0..8).map(|k| f64::from(k) * 0.785 + 0.1).map(|a| json!([10.0 * a.cos(), 10.0 * a.sin(), z])).collect::<Vec<_>>())
        };
        // Unstitching keeps the wall under the body's own name.
        let blend = json!({"from": {"body": "Low", "edges": rim(30.0)}, "to": {"body": "High", "edges": rim(40.0)}, "continuity": ["g1", "g1"], "body_name": "Neck"});
        run(&mut s, "surface.blend", blend.clone());
        // The wall between: radius 10 and straight up, so 2π·10·10 of area.
        let want = 2.0 * std::f64::consts::PI * 100.0;
        assert!((area(&s, "Neck") - want).abs() < want * 1e-3, "{} vs {want}", area(&s, "Neck"));
        let turn = |s: &Session, name: &str| {
            let grid = solidcraft_kernel::sample_grid(&s.world_state().unwrap().body(name).unwrap().body, 17, 5).unwrap();
            grid.iter()
                .map(|row| {
                    let (a, b) = (row[0], row[row.len() - 1]);
                    (a.y.atan2(a.x) - b.y.atan2(b.x)).rem_euclid(std::f64::consts::TAU).to_degrees()
                })
                .map(|d| d.min(360.0 - d))
                .fold(0.0, f64::max)
        };
        assert!(turn(&s, "Neck") < 0.1, "straight up the axis: {}°", turn(&s, "Neck"));
        // Seams a quarter apart: the blend turns a quarter round, and reads back the same.
        let mut turned = blend.clone();
        turned["seam"] = json!([[10, 0, 30], [0, 10, 40]]);
        turned["body_name"] = json!("Twist");
        run(&mut s, "surface.blend", turned);
        assert!((turn(&s, "Twist") - 90.0).abs() < 0.1, "{}°", turn(&s, "Twist"));
        assert!(s.doc.to_json().contains("\"seam\""), "the seam is saved");
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert!((turn(&cold, "Twist") - 90.0).abs() < 0.1, "read back: {}°", turn(&cold, "Twist"));
        // A closed rim does not blend to an open edge; a seam that is not two points is refused.
        run(&mut s, "solid.box", json!({"length": 40, "width": 10, "height": 10, "corner": [-20, 30, 30], "body_name": "Block"}));
        let open = s.execute("surface.blend", &json!({"from": {"body": "Low", "edges": rim(30.0)}, "to": {"body": "Block", "edges": [[0, 30, 40]]}}));
        assert!(open.as_ref().is_err_and(|e| e.to_string().contains("closed chain")), "{open:?}");
        for seam in [json!([[0, 0, 0]]), json!("x"), json!([[0, 0, 0], "y"])] {
            let mut bad = blend.clone();
            bad["seam"] = seam.clone();
            assert!(s.execute("surface.blend", &bad).is_err(), "{seam}");
        }
    }

    /// A tube whose top face is taken off (`surface.unstitch`), and points round its rim.
    fn open_tube() -> (Session, Value) {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 30, "body_name": "Tube"}));
        run(&mut s, "surface.unstitch", json!({"body": "Tube", "faces": [[0, 0, 30]]}));
        let rim: Vec<Value> = (0..8).map(|k| f64::from(k) * 0.785).map(|a| json!([10.0 * a.cos(), 10.0 * a.sin(), 30.0])).collect();
        (s, json!(rim))
    }

    /// `surface.patch continuity` and `points`: the tube's open end capped G1, per-edge
    /// continuity, a point the cap passes through, all saved with the design and read back;
    /// hostile values refused.
    #[test]
    fn patch_continuity_and_points_cap_a_tube() {
        // A coarse mesh is enough to tell the patches apart (measuring a fitted face's area
        // exactly tessellates it finely).
        let mesh =
            |s: &Session, name: &str| s.world_state().unwrap().body(name).unwrap().body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.05)).unwrap();
        let (mut s, rim) = open_tube();
        let reply = run(&mut s, "surface.patch", json!({"edges": rim, "body": "Tube", "continuity": "g1", "body_name": "Cap"}));
        assert_eq!(reply["continuity"], json!({"all": "g1"}), "{reply}");
        // A dome over the rim, not the flat disc a G0 patch of a planar rim is.
        let disc = std::f64::consts::PI * 100.0;
        let dome = mesh(&s, "Cap").measure().area;
        assert!(dome > disc * 1.2, "a dome, not the disc: {dome}");
        // Through a point 8 above the rim, G1 to the wall: the cap reaches it.
        run(&mut s, "surface.patch", json!({"edges": rim, "body": "Tube", "continuity": {"all": "g1"}, "points": [[0, 0, 38]], "body_name": "Tall"}));
        let top = mesh(&s, "Tall").bounds().max.z;
        assert!(top >= 38.0 - 1e-2, "the cap passes through the point: top {top}");
        // One edge G2, the rest G0 (the list form).
        let first = rim[0].clone();
        let reply =
            run(&mut s, "surface.patch", json!({"edges": rim, "body": "Tube", "continuity": [{"edge": first, "g": "g2"}], "body_name": "Mixed"}));
        assert_eq!(reply["continuity"]["edges"][0]["g"], json!("g2"), "{reply}");
        // Saved and read back: the same patches.
        let back = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        let reopened = Session::new(back);
        for name in ["Cap", "Tall", "Mixed"] {
            let (a, b) = (mesh(&s, name).positions, mesh(&reopened, name).positions);
            assert_eq!(a, b, "{name}: the same surface read back");
        }
        assert!(s.doc.to_json().contains("\"continuity\""), "the continuity is saved");
        // Hostile and misplaced values.
        for (key, v) in [
            ("continuity", json!("g5")),
            ("continuity", json!("smooth")),
            ("continuity", json!(5)),
            ("continuity", json!([{"edge": "here", "g": "g1"}])),
            ("continuity", json!([{"edge": [f64::INFINITY, 0, 0], "g": "g1"}])),
            ("continuity", json!({"all": "g9"})),
            ("continuity", json!([{"edge": [0, 0, 0], "g": "g1"}])),
            ("points", json!([[0, 0, f64::NAN]])),
            ("points", json!("up")),
            ("points", json!(vec![[0, 0, 35]; 65])),
            ("points", json!([[100, 0, 30]])),
        ] {
            let mut p = json!({"edges": rim, "body": "Tube"});
            p[key] = v.clone();
            assert!(s.execute("surface.patch", &p).is_err(), "accepted {key}: {v}");
        }
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.rectangle.two_point", json!({"p0": [30, 0], "p1": [40, 10]}));
        let sketch_patch = json!({"sketch": 1, "continuity": "g1"});
        assert!(s.execute("surface.patch", &sketch_patch).is_err(), "continuity on a sketch patch");
    }
    /// The energies the reply's warning carries: the fairness energy before and after.
    fn energies(reply: &Value) -> (f64, f64) {
        let warning = reply["warning"].as_str().unwrap_or_else(|| panic!("no warning in {reply}"));
        let after = warning.split("energy ").nth(1).unwrap_or_else(|| panic!("no energies in `{warning}`"));
        let numbers: Vec<f64> = after
            .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == 'e' || c == '-' || c == '+'))
            .filter_map(|w| w.parse::<f64>().ok())
            .collect();
        assert!(numbers.len() >= 2, "no energies in `{warning}`");
        (numbers[0], numbers[1])
    }

    /// Surface Fair on the fitted B-spline face of a patch on a rim that is not planar (a box's
    /// top cut away by a sideways cylinder): the face is smoothed in place, its edges do not move
    /// at all, the reply carries the energy before and after, undo, redo and a cold reopen keep
    /// it, and hostile inputs are refused.
    #[test]
    fn fairing_a_b_spline_face_keeps_its_edges_and_reports_its_energy() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 28, "width": 28, "height": 20, "corner": [-14, -14, 0], "body_name": "Saddle"}));
        run(
            &mut s,
            "solid.cylinder",
            json!({"radius": 40, "height": 120, "base": [0, -60, 50], "axis": "Y", "operation": "cut", "targets": ["Saddle"]}),
        );
        let z = |x: f64| 50.0 - (1600.0 - x * x).sqrt();
        let rim = json!([[14, 0, z(14.0)], [0, 14, z(0.0)], [-14, 0, z(-14.0)], [0, -14, z(0.0)]]);
        // The rim is not planar, so its patch is a fitted B-spline face.
        run(&mut s, "surface.patch", json!({"edges": rim, "body": "Saddle", "body_name": "Shell"}));
        let st = s.world_state().unwrap();
        assert!(st.body("Shell").unwrap().body.is_surface(), "a patch on a rim is a sheet");
        let before: Vec<solidcore_record::Vec3> = st.body("Shell").unwrap().body.edges(0.1).unwrap().iter().flat_map(|e| e.points.clone()).collect();
        assert!(!before.is_empty());
        let area_before = area(&s, "Shell");
        // The fairing reports both energies. The fit is not the fairest surface on its own rim, so
        // the energy falls strictly and the face moves: its area changes.
        let reply = run(&mut s, "surface.fair", json!({"body": "Shell", "tolerance": 1.0, "iterations": 3}));
        let (energy_before, energy_after) = energies(&reply);
        let area_after = area(&s, "Shell");
        assert!(energy_before > 0.0 && energy_after < energy_before, "{energy_before:e} → {energy_after:e}");
        assert_ne!(area_after, area_before, "the fairing did not move the face");
        // The edges do not move: every boundary point of the faired body is where it was, bit for
        // bit, and the sheet is still the one face.
        let st = s.world_state().unwrap();
        let after: Vec<solidcore_record::Vec3> = st.body("Shell").unwrap().body.edges(0.1).unwrap().iter().flat_map(|e| e.points.clone()).collect();
        assert_eq!(after, before, "fairing must not move the face's edges");
        assert_eq!(st.body("Shell").unwrap().body.face_count(), 1);
        // Undo takes the fairing back, redo puts it back, and a cold reopen rebuilds the same.
        run(&mut s, "edit.undo", json!({}));
        assert!((area(&s, "Shell") - area_before).abs() < 1e-9, "undo did not take the fairing back");
        run(&mut s, "edit.redo", json!({}));
        assert!((area(&s, "Shell") - area_after).abs() < 1e-9, "redo did not put the fairing back");
        let cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        assert!((area(&cold, "Shell") - area_after).abs() < 1e-9, "a cold reopen did not rebuild the same sheet");
        // Refusals: an unknown body, a body whose faces are all planes, an out-of-range strength
        // or step count, and a missing tolerance expression.
        assert!(s.execute("surface.fair", &json!({"body": "Nope", "tolerance": 1.0})).is_err());
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Box"}));
        assert!(s.execute("surface.fair", &json!({"body": "Box", "tolerance": 1.0})).is_err());
        assert!(s.execute("surface.fair", &json!({"body": "Shell", "strength": 2, "tolerance": 1.0})).is_err());
        assert!(s.execute("surface.fair", &json!({"body": "Shell", "iterations": 0, "tolerance": 1.0})).is_err());
        assert!(s.execute("surface.fair", &json!({"body": "Shell", "tolerance": 0})).is_err());
    }

    /// A loft through three sections whose first and last are edge chains of two boxes, with
    /// `continuity: ["g1","g1"]`: it leaves each box's top face tangentially (the run direction
    /// lies in that face) and passes through the sketch between them.
    #[test]
    fn surface_loft_leaves_a_box_face_tangentially() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"corner": [0, 0, 0], "length": 20, "width": 20, "height": 10, "body_name": "Low"}));
        run(&mut s, "solid.box", json!({"corner": [40, 0, 0], "length": 20, "width": 20, "height": 10, "body_name": "High"}));
        run(&mut s, "construct.plane.offset", json!({"base": "YZ", "offset": 30, "name": "Mid"}));
        run(&mut s, "sketch.create", json!({"plane": "Mid", "name": "Bump"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 14], [20, 14]], "ids": ["b"]}));
        run(&mut s, "sketch.finish", json!({}));
        run(
            &mut s,
            "surface.loft",
            json!({
                "sections": [
                    {"body": "Low", "edges": [[20, 5, 10], [20, 15, 10]]},
                    {"sketch": "Bump", "curves": ["b"]},
                    {"body": "High", "edges": [[40, 5, 10], [40, 15, 10]]}
                ],
                "continuity": ["g1", "g1"],
                "crown": [0, 0],
                "body_name": "Blend"
            }),
        );
        let body = s.world_state().unwrap().body("Blend").unwrap().body.clone();
        // A fine grid in the run direction: the seams sit at u = 0, 1/2 and 1, and the first step
        // off each one is the tangent there (a thousandth of the run, so it is the tangent).
        let grid = solidcraft_kernel::sample_grid(&body, 1001, 9).unwrap();
        // The ends lie on the two boxes' top faces, and the sketch is met exactly between them.
        for p in &grid[0] {
            assert!((p.z - 10.0).abs() < 1e-9 && (p.x - 20.0).abs() < 1e-9, "the first seam is the box edge: {p:?}");
        }
        for p in &grid[1000] {
            assert!((p.z - 10.0).abs() < 1e-9 && (p.x - 40.0).abs() < 1e-9, "the last seam is the box edge: {p:?}");
        }
        for p in &grid[500] {
            assert!((p.z - 14.0).abs() < 1e-9 && (p.x - 30.0).abs() < 1e-9, "the blend meets the sketch: {p:?}");
        }
        // The seam leaves along the box's top face: the run direction the blend is given is what
        // the face holds, so the tangent there is horizontal and along the run. (The step the grid
        // takes is a thousandth of the run, and a chord over that step carries an O(h²) bias of
        // about 0.011 here, so 1e-2 on the rise off the face is what the grid can measure; the
        // kernel's own tests pin the tangent to the frame's leave direction exactly.)
        for j in [0usize, 4, 8] {
            let d = (grid[1][j] - grid[0][j]).normalized().unwrap();
            assert!(d.z.abs() < 2e-2 && d.x > 0.999, "the blend leaves the low box's top face: {d:?}");
            let d = (grid[1000][j] - grid[999][j]).normalized().unwrap();
            assert!(d.z.abs() < 2e-2 && d.x > 0.999, "the blend arrives along the high box's top face: {d:?}");
        }
        // Hostile parameters are refused, never panicking.
        let bad = [
            json!({"sections": [{"body": "Low", "edges": [[20, 5, 10], [20, 15, 10]]}, {"sketch": "Bump", "curves": ["b"]}], "continuity": ["g1"]}),
            json!({"sections": [{"body": "Low", "edges": []}, {"sketch": "Bump", "curves": ["b"]}], "continuity": ["g1", "g1"]}),
            json!({"sections": [{"body": "Low", "edges": [[20, 5, 10]], "sketch": "Bump"}, {"sketch": "Bump", "curves": ["b"]}]}),
            json!({"sections": [{"sketch": "Bump", "curves": ["b"]}, {"sketch": "Bump", "curves": ["b"]}], "crown": [5, 0]}),
            json!({"sections": [{"sketch": "Bump", "curves": ["b"]}, {"sketch": "Bump", "curves": ["b"]}], "continuity": ["g9", "g1"]}),
        ];
        for p in bad {
            assert!(s.execute("surface.loft", &p).is_err(), "hostile loft params: {p}");
        }
    }
}

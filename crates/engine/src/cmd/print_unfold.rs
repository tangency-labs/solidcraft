//! Unfold for Printing: a solid of flat faces is laid on one of them in a design of its own and
//! unfolded there by hand, step by step (`solidcraft_doc::print_unfold` has the rules).
//!
//! `print.unfold.start` copies the body, lying on its base face, into a new design that replaces
//! the session's (the app opens that design in a tab of its own, so the design the body came
//! from stays as it is). `print.unfold.bend` sets or clears a bend at an edge, `print.unfold.surface`
//! closes the step once every face hangs on the pattern and makes the surface model (every face
//! a flat sheet of its own, the solid hidden), and `print.unfold.edit` goes back to the edges.

use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_doc::print_unfold::{
    BendChange, EdgeKind, EdgeTurn, PrintUnfold, PrintUnfoldSource, PrintUnfoldStamp, PrintUnfoldStep, UnfoldGraph, UnfoldLayout,
};
use solidcraft_doc::{Document, FeatureKind, ModelBody, PrintFace};
use solidcraft_geom::Vec3Geom;

use super::CommandSpec;
use crate::params::bad;
use crate::schema::print_unfold::{PrintUnfoldBed, PrintUnfoldEdge, PrintUnfoldFlatEdge, PrintUnfoldPart, PrintUnfoldState};
use crate::{EngineError, Result, Session};
use solidcraft_doc::print_unfold::PrintBed;
use solidcraft_doc::print_unfold_flat::{FlatLayout, MinRect};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("print.unfold.start", "Unfold for Printing", crate::checked!("print.unfold.start", start))
        .at("SHEET METAL", "CREATE")
        .icon("print_unfold")
        .noundo()
        .contract(&START_CONTRACT)
        .output(crate::schema::output_schema_for::<PrintUnfoldState>)
        .params("body: its name; face: [x, y, z] on a flat face of it, in mm → the state of the unfolded print: copies the body into a new design, lying on that face (the base face) at Z = 0 with its centre of mass over the origin, and starts unfolding it there. The new design replaces the session's (the app shows it in a tab of its own, leaving the design the body came from as it is). Every face of the body must be flat. Then set the bends with print.unfold.bend"),
    CommandSpec::new("print.unfold.bend", "Set Bend Edge", crate::checked!("print.unfold.bend", bend))
        .icon("print_unfold")
        .contract(&BEND_CONTRACT)
        .output(crate::schema::output_schema_for::<PrintUnfoldState>)
        .params("edge: [x, y, z] on an edge of the body being unfolded, in mm → the state of the unfolded print: an edge that is a cut becomes a bend, the face across it hangs on the pattern, and that face's other edges are cuts (a face that hung by another bend moves to this one, with what hangs on it); an edge that is a bend becomes a cut again, and what hung by it is off the pattern. Bends are set outward from the base face. An edge in an inside corner of the body can't fold"),
    CommandSpec::new("print.unfold.group", "Carve Out Faces", crate::checked!("print.unfold.group", group))
        .icon("print_unfold")
        .contract(&GROUP_CONTRACT)
        .output(crate::schema::output_schema_for::<PrintUnfoldState>)
        .params("faces: [[x, y, z], …] on faces of the body being unfolded, in mm → the state of the unfolded print: those faces are carved out of the pattern into a group, a part of its own that starts at the first of them. Every edge between the group and the other faces is a cut; inside the group, print.unfold.bend sets bends at inside corners of the body (they turn toward the outer skin, on the film alone). Bends that reached the faces are cleared"),
    CommandSpec::new("print.unfold.ungroup", "Put Carved Faces Back", crate::checked!("print.unfold.ungroup", ungroup))
        .icon("print_unfold")
        .contract(&UNGROUP_CONTRACT)
        .output(crate::schema::output_schema_for::<PrintUnfoldState>)
        .params("face: [x, y, z] on a face of a carved-out group, in mm → the state of the unfolded print: the group is no more, its faces are off the pattern until bends reach them again"),
    CommandSpec::new("print.unfold.surface", "Make Surface Model", crate::checked!("print.unfold.surface", surface))
        .icon("print_unfold")
        .contract(&EMPTY_CONTRACT)
        .output(crate::schema::output_schema_for::<PrintUnfoldState>)
        .params("→ the state of the unfolded print: once every face hangs on the pattern (every edge is a bend or a cut), makes the surface model, every face a flat sheet of its own, and hides the solid"),
    CommandSpec::new("print.unfold.flatten", "Lay Pattern Flat", crate::checked!("print.unfold.flatten", flatten))
        .icon("print_unfold")
        .contract(&EMPTY_CONTRACT)
        .output(crate::schema::output_schema_for::<PrintUnfoldState>)
        .params("→ the state of the unfolded print: lays the surface model flat on the base face's plane, every face turned about its bends (making the surface model first when it is not made yet). The state then lists the pattern's parts with the smallest rectangle round each"),
    CommandSpec::new("print.unfold.bed", "Choose Print Bed", crate::checked!("print.unfold.bed", bed))
        .icon("print_unfold")
        .contract(&BED_CONTRACT)
        .output(crate::schema::output_schema_for::<PrintUnfoldState>)
        .params("width, depth: the bed's size in mm; name?: the printer's name → the state of the unfolded print, each part saying whether a rectangle round it fits the bed (either way round)"),
    CommandSpec::new("print.unfold.split", "Split Pattern", crate::checked!("print.unfold.split", split))
        .icon("print_unfold")
        .contract(&SPLIT_CONTRACT)
        .output(crate::schema::output_schema_for::<PrintUnfoldState>)
        .params("edge: [x, y, z] on a bend of the flat pattern, in mm → the state of the unfolded print: the pattern is split at that bend into a part of its own, laid beside the others (or joined there again when it was split). The two edges the split leaves are a pair, as a cut's are"),
    CommandSpec::new("print.unfold.joints", "Choose Wall and Joints", crate::checked!("print.unfold.joints", joints))
        .icon("print_unfold")
        .contract(&JOINTS_CONTRACT)
        .output(crate::schema::output_schema_for::<PrintUnfoldState>)
        .params("wall?: the wall thickness, a length; bends?: film | filament | press, what every bend is; cuts?: mitre | filament | press, how walls meet at every cut → the state of the unfolded print. What is left out stays as it is (3 mm, film and mitre at first)"),
    CommandSpec::new("print.unfold.joint", "Set Edge Joint", crate::checked!("print.unfold.joint", joint))
        .icon("print_unfold")
        .contract(&JOINT_CONTRACT)
        .output(crate::schema::output_schema_for::<PrintUnfoldState>)
        .params("edge: [x, y, z] on an edge of the flat pattern, in mm; kind: plain | filament | press | default → the state of the unfolded print: that edge gets a joint of its own (plain: a bend's film, a cut's mitre), or follows what all bends or cuts get again (default)"),
    CommandSpec::new("print.unfold.make", "Make Printable Body", crate::checked!("print.unfold.make", make))
        .icon("print_unfold")
        .contract(&EMPTY_CONTRACT)
        .output(crate::schema::output_schema_for::<PrintUnfoldState>)
        .params("→ the state of the unfolded print: makes the printable body from the flat pattern, with the wall thickness and the joints chosen: each part of the pattern a body of its own, the walls of the body's faces laid flat and joined by a film under each bend. The surface model is removed"),
    CommandSpec::new("print.unfold.unmake", "Back to Wall and Joints", crate::checked!("print.unfold.unmake", unmake))
        .icon("print_unfold")
        .contract(&EMPTY_CONTRACT)
        .output(crate::schema::output_schema_for::<PrintUnfoldState>)
        .params("→ the state of the unfolded print: removes the printable body and shows the flat pattern again"),
    CommandSpec::new("print.unfold.edit", "Edit Bend Edges", crate::checked!("print.unfold.edit", edit))
        .icon("print_unfold")
        .contract(&EMPTY_CONTRACT)
        .output(crate::schema::output_schema_for::<PrintUnfoldState>)
        .params("→ the state of the unfolded print: removes the surface model and shows the solid again, to change its bends and cuts"),
    CommandSpec::new("print.unfold.state", "Unfolded Print State", crate::checked!("print.unfold.state", state))
        .icon("print_unfold")
        .noundo()
        .contract(&EMPTY_CONTRACT)
        .output(crate::schema::output_schema_for::<PrintUnfoldState>)
        .params("→ {body, step, faces, attached, complete, bends, cuts, open, missing}: the body this design unfolds for printing, how far the work has come, and every edge as a bend, a cut (with the two faces it is an edge of) or open, and the outlines of the faces not on the pattern yet"),
];

const START_CONTRACT: crate::schema::TypedContract<crate::schema::print_unfold::StartParams> = crate::schema::TypedContract::new();
const BEND_CONTRACT: crate::schema::TypedContract<crate::schema::print_unfold::BendParams> = crate::schema::TypedContract::new();
const GROUP_CONTRACT: crate::schema::TypedContract<crate::schema::print_unfold::GroupParams> = crate::schema::TypedContract::new();
const UNGROUP_CONTRACT: crate::schema::TypedContract<crate::schema::print_unfold::UngroupParams> = crate::schema::TypedContract::new();
const BED_CONTRACT: crate::schema::TypedContract<crate::schema::print_unfold::BedParams> = crate::schema::TypedContract::new();
const SPLIT_CONTRACT: crate::schema::TypedContract<crate::schema::print_unfold::SplitParams> = crate::schema::TypedContract::new();
const JOINTS_CONTRACT: crate::schema::TypedContract<crate::schema::print_unfold::JointsParams> = crate::schema::TypedContract::new();
const JOINT_CONTRACT: crate::schema::TypedContract<crate::schema::print_unfold::JointParams> = crate::schema::TypedContract::new();
const EMPTY_CONTRACT: crate::schema::TypedContract<crate::schema::print_unfold::NoParams> = crate::schema::TypedContract::new();

fn point(p: &Value, key: &str, cmd: &str) -> Result<Vec3> {
    p.get(key).and_then(crate::params::vec3).ok_or_else(|| bad(cmd, format!("`{key}` must be [x, y, z] of numbers, in mm")))
}

fn invalid(cmd: &str, e: impl std::fmt::Display) -> EngineError {
    bad(cmd, e.to_string())
}

/// What `body` measures now, to tell a change of it later.
pub fn stamp_of(body: &ModelBody) -> Result<PrintUnfoldStamp> {
    let m = solidcraft_kernel::measure(&body.body)?;
    Ok(PrintUnfoldStamp { faces: m.faces, edges: m.edges, volume: m.volume, area: m.area, min: m.bbox.min, max: m.bbox.max })
}

/// The design that unfolds `name` of `s`, lying on the face at `face` (world coordinates).
pub fn unfold_design(s: &Session, name: &str, face: Vec3) -> Result<Session> {
    let cmd = "print.unfold.start";
    let world = s.world_state()?;
    let body = world.body(name).ok_or_else(|| bad(cmd, format!("no body `{name}`")))?;
    let graph = UnfoldGraph::of(&body.body).map_err(|e| invalid(cmd, e))?;
    let base = graph.face_at(face).ok_or_else(|| bad(cmd, "no face of the body at `face`"))?;
    let normal = graph.faces.get(base).map(|f| f.normal()).ok_or_else(|| bad(cmd, "no face of the body at `face`"))?;
    let centre = solidcraft_kernel::measure_mass(&body.body)?.centroid;
    let lay = super::print_face::bed_matrix(PrintFace { index: base, point: face, normal }, centre);
    let laid = solidcraft_kernel::transform_matrix(&body.body, lay)?;
    let step = solidcraft_kernel::step_export(&[&laid], "SolidCraft unfold for printing")?;
    let mut doc = Document::new(&format!("{name} unfolded"));
    doc.units = s.doc.units.clone();
    let mut fresh = Session::new(doc);
    let kind = FeatureKind::Import { file: format!("{name} ({})", s.doc.name), step: step.into(), components: Vec::new(), skip: Vec::new() };
    let id = fresh.doc_mut().add_feature(kind, None)?;
    if let Some(f) = fresh.doc_mut().feature_mut(id) {
        f.body_names = vec![name.to_string()];
    }
    let source =
        PrintUnfoldSource { design: s.doc.name.clone(), path: s.path.clone(), body: name.to_string(), face, stamp: stamp_of(body)?, session: s.uid };
    fresh.doc_mut().print_unfold = Some(PrintUnfold {
        body: name.to_string(),
        base: solidcraft_doc::apply_point(&lay, face),
        bends: Vec::new(),
        groups: Vec::new(),
        step: PrintUnfoldStep::Edges,
        source: Some(source),
        surface: None,
        splits: Vec::new(),
        bed: None,
        joints: None,
        parts: None,
    });
    // Nothing is sketched here: the origin's planes and axes would only lie over the edges to click.
    fresh.visibility.origin_hidden = Some(true);
    fresh.refresh();
    if let Some(error) = fresh.model.result(id).and_then(|r| r.error.as_ref()) {
        return Err(bad(cmd, format!("the body didn't copy: {error}")));
    }
    // It must read there as it does here: a flat-faced solid with its base face found again.
    read(&fresh, cmd)?;
    Ok(fresh)
}

fn start(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "print.unfold.start";
    let name = crate::params::str_(p, "body").ok_or_else(|| bad(cmd, "give `body`"))?;
    let fresh = unfold_design(s, name, point(p, "face", cmd)?)?;
    s.replace_design(fresh);
    reply(s, cmd)
}

/// The unfolded print of `s`, its body's faces and edges, and the pattern its bends make.
pub fn read(s: &Session, cmd: &str) -> Result<(PrintUnfold, UnfoldGraph, usize, Vec<usize>, UnfoldLayout)> {
    let unfold = s.doc.print_unfold.clone().ok_or_else(|| bad(cmd, "this design unfolds no body for printing (start with print.unfold.start)"))?;
    let world = s.world_state()?;
    let body = world.body(&unfold.body).ok_or_else(|| bad(cmd, format!("the body `{}` being unfolded is gone", unfold.body)))?;
    let graph = UnfoldGraph::of(&body.body).map_err(|e| invalid(cmd, e))?;
    let (base, bends) = unfold.resolve(&graph).map_err(|e| invalid(cmd, e))?;
    let groups = unfold.resolve_groups(&graph).map_err(|e| invalid(cmd, e))?;
    let layout = graph.layout(base, &bends, &groups).map_err(|e| invalid(cmd, e))?;
    Ok((unfold, graph, base, bends, layout))
}

fn edge_out(graph: &UnfoldGraph, layout: &UnfoldLayout, e: usize) -> Option<PrintUnfoldEdge> {
    let edge = graph.edges.get(e)?;
    let (ga, gb) = (layout.group.get(edge.faces.0).copied().flatten(), layout.group.get(edge.faces.1).copied().flatten());
    let (angle, foldable) = match edge.turn {
        EdgeTurn::Outside(a) => (Some(a.to_degrees()), ga.is_none() && gb.is_none()),
        EdgeTurn::Inside => (None, ga.is_some() && ga == gb),
        EdgeTurn::Flat => (None, false),
    };
    let xyz = |p: Vec3| [p.x, p.y, p.z];
    Some(PrintUnfoldEdge { start: xyz(edge.ends.0), end: xyz(edge.ends.1), faces: [edge.faces.0, edge.faces.1], angle, foldable })
}

/// The state of the unfolded print of `s`, as its commands return it.
pub fn state_of(s: &Session, cmd: &str) -> Result<PrintUnfoldState> {
    // Once the printable body is made, the body is no longer the solid of flat faces to read.
    if let Some(u) = s.doc.print_unfold.as_ref().filter(|u| u.step == PrintUnfoldStep::Solid) {
        let joints = u.joints.clone().unwrap_or_default();
        return Ok(PrintUnfoldState {
            body: u.body.clone(),
            step: "solid".into(),
            base: 0,
            faces: 0,
            attached: 0,
            complete: true,
            bends: Vec::new(),
            cuts: Vec::new(),
            open: Vec::new(),
            missing: Vec::new(),
            groups: u.groups.len(),
            carved: Vec::new(),
            group_points: Vec::new(),
            wall: joints.wall,
            min_wall: 0.0,
            bend_joint: joints.bends,
            cut_joint: joints.cuts,
            bed: u.bed.as_ref().map(|b| PrintUnfoldBed { name: b.name.clone(), width: b.width, depth: b.depth }),
            parts: Vec::new(),
            flat_bends: Vec::new(),
            flat_cuts: Vec::new(),
            overlapping: Vec::new(),
            changed: None,
            param_warnings: Vec::new(),
        });
    }
    let (unfold, graph, base, bends, layout) = read(s, cmd)?;
    let (parts, flat_bends, flat_cuts, overlapping) = if unfold.step == PrintUnfoldStep::Flat {
        let splits = unfold.resolve_splits(&graph, &bends);
        let flat = graph.flat(&layout, &splits).map_err(|e| invalid(cmd, e))?;
        flat_state(&unfold, &graph, &layout, &flat, &splits)
    } else {
        Default::default()
    };
    let of = |kind: EdgeKind| -> Vec<PrintUnfoldEdge> {
        layout.kinds.iter().enumerate().filter(|(_, k)| **k == kind).filter_map(|(e, _)| edge_out(&graph, &layout, e)).collect()
    };
    Ok(PrintUnfoldState {
        body: unfold.body.clone(),
        step: match unfold.step {
            PrintUnfoldStep::Edges => "edges".into(),
            PrintUnfoldStep::Surface => "surface".into(),
            PrintUnfoldStep::Flat => "flat".into(),
            PrintUnfoldStep::Solid => "solid".into(),
        },
        base,
        faces: graph.faces.len(),
        attached: layout.attached.iter().filter(|a| **a).count(),
        complete: layout.complete(),
        bends: of(EdgeKind::Bend),
        cuts: of(EdgeKind::Cut),
        open: of(EdgeKind::Open),
        missing: graph
            .faces
            .iter()
            .zip(&layout.attached)
            .filter(|(_, on)| !**on)
            .flat_map(|(f, _)| f.loops.iter().map(|lp| lp.iter().map(|p| [p.x, p.y, p.z]).collect()))
            .collect(),
        groups: layout.roots.len(),
        carved: graph
            .faces
            .iter()
            .zip(&layout.group)
            .filter(|(_, g)| g.is_some())
            .flat_map(|(f, _)| f.loops.iter().map(|lp| lp.iter().map(|p| [p.x, p.y, p.z]).collect()))
            .collect(),
        group_points: layout.roots.iter().filter_map(|f| graph.faces.get(*f)?.inner_point()).map(|p| [p.x, p.y, p.z]).collect(),
        wall: unfold.joints.clone().unwrap_or_default().wall,
        min_wall: if unfold.step == PrintUnfoldStep::Flat { min_wall(&unfold, &graph, &layout, &bends) } else { 0.0 },
        bend_joint: unfold.joints.clone().unwrap_or_default().bends,
        cut_joint: unfold.joints.clone().unwrap_or_default().cuts,
        bed: unfold.bed.as_ref().map(|b| PrintUnfoldBed { name: b.name.clone(), width: b.width, depth: b.depth }),
        parts,
        flat_bends,
        flat_cuts,
        overlapping,
        changed: None,
        param_warnings: Vec::new(),
    })
}

/// The printers offered for the bed check: a name and the bed's width and depth (mm).
pub static PRINTERS: &[(&str, f64, f64)] = &[
    ("Bambu Lab A1 mini", 180.0, 180.0),
    ("Bambu Lab A1 / P1 / X1", 256.0, 256.0),
    ("Prusa MINI+", 180.0, 180.0),
    ("Prusa MK4 / MK3", 250.0, 210.0),
    ("Prusa XL", 360.0, 360.0),
    ("Creality Ender-3", 220.0, 220.0),
    ("Creality K1", 220.0, 220.0),
    ("Creality K1 Max", 300.0, 300.0),
    ("Anycubic Kobra 2", 220.0, 220.0),
    ("Elegoo Neptune 4", 225.0, 225.0),
    ("Voron 2.4 (350)", 350.0, 350.0),
];

type FlatState = (Vec<PrintUnfoldPart>, Vec<PrintUnfoldFlatEdge>, Vec<PrintUnfoldFlatEdge>, Vec<Vec<[f64; 3]>>);

/// The parts and edges of the flat pattern, as the state lists them, and the outlines of the
/// faces that lie on each other.
fn flat_state(unfold: &PrintUnfold, graph: &UnfoldGraph, layout: &UnfoldLayout, flat: &FlatLayout, splits: &[usize]) -> FlatState {
    let parts = flat
        .parts
        .iter()
        .enumerate()
        .map(|(k, part)| {
            let pts = flat.points(graph, k);
            let fitting = unfold.bed.as_ref().map(|b| MinRect::fitting(&pts, b.width, b.depth));
            let rect = fitting.flatten().or(part.rect);
            PrintUnfoldPart {
                faces: part.faces.len(),
                long: rect.map_or(0.0, |r| r.long),
                short: rect.map_or(0.0, |r| r.short),
                corners: rect.map_or([[0.0; 2]; 4], |r| r.corners.map(|c| [c.x, c.y])),
                fits: fitting.map(|f| f.is_some()),
                overlaps: part.overlaps.len(),
            }
        })
        .collect();
    // An edge has a joint only where it gets fingers. A cut gets none where another wall lies
    // beside it in the flat, and is mitred there.
    let kinds: Vec<String> = (0..graph.edges.len()).map(|e| joint_of(unfold, graph, layout, splits, e).0).collect();
    let fingers = graph.jointed_edges(layout, flat, splits, walls_hint(unfold), &kinds);
    let (mut bends, mut cuts) = (Vec::new(), Vec::new());
    for (e, edge) in graph.edges.iter().enumerate() {
        let on = |f: usize| -> Option<PrintUnfoldFlatEdge> {
            let m = flat.moves.get(f)?;
            let (a, b) = (solidcraft_doc::apply_point(m, edge.ends.0), solidcraft_doc::apply_point(m, edge.ends.1));
            let (joint, own) = joint_of(unfold, graph, layout, splits, e);
            let bare = ["press", "filament"].contains(&joint.as_str()) && fingers.get(e) == Some(&false);
            let folds = layout.kinds.get(e) == Some(&EdgeKind::Bend) && !splits.contains(&e);
            let joint = if bare { if folds { "film" } else { "mitre" }.to_string() } else { joint };
            Some(PrintUnfoldFlatEdge { start: [a.x, a.y, a.z], end: [b.x, b.y, b.z], edge: e, split: splits.contains(&e), joint, own })
        };
        if layout.kinds.get(e) == Some(&EdgeKind::Bend) && !splits.contains(&e) {
            bends.extend(on(edge.faces.0));
        } else {
            cuts.extend(on(edge.faces.0));
            cuts.extend(on(edge.faces.1));
        }
    }
    // The faces that lie on another face of their part, each once.
    let mut over: Vec<usize> = flat.parts.iter().flat_map(|p| p.overlaps.iter().flat_map(|(a, b)| [*a, *b])).collect();
    over.sort_unstable();
    over.dedup();
    let overlapping = over
        .iter()
        .filter_map(|f| Some((graph.faces.get(*f)?, flat.moves.get(*f)?)))
        .flat_map(|(face, m)| {
            face.loops
                .iter()
                .map(|lp| lp.iter().map(|p| solidcraft_doc::apply_point(m, *p)).map(|p| [p.x, p.y, p.z]).collect())
                .collect::<Vec<Vec<[f64; 3]>>>()
        })
        .collect();
    (parts, bends, cuts, overlapping)
}

/// A wall thickness typed as a plain number of millimetres (`2`, `2.4 mm`, `2,4`).
fn wall_mm(text: &str) -> Option<f64> {
    text.trim().trim_end_matches("mm").trim().replace(',', ".").parse::<f64>().ok().filter(|w| w.is_finite() && *w > 0.0 && *w < 1000.0)
}

/// The walls of the print: as thick as chosen (its default while that is no plain number).
fn walls_of(unfold: &PrintUnfold) -> solidcraft_doc::print_unfold_walls::WallPrint {
    solidcraft_doc::print_unfold_walls::WallPrint {
        wall: unfold.joints.as_ref().and_then(|j| wall_mm(&j.wall)).unwrap_or(solidcraft_doc::print_unfold_walls::DEFAULT_WALL),
        film: solidcraft_doc::print_joint::DEFAULT_FILM,
        clearance: solidcraft_doc::print_joint::DEFAULT_PRINT_CLEARANCE,
        gap: solidcraft_doc::print_unfold_walls::DEFAULT_FINGER_GAP,
    }
}

/// The walls as the joints are shown before the body is made: thick enough for
/// any of them, so a joint chosen is shown and the wall it needs is asked for.
fn walls_hint(unfold: &PrintUnfold) -> solidcraft_doc::print_unfold_walls::WallPrint {
    let mut w = walls_of(unfold);
    w.wall = w.wall.max(solidcraft_doc::print_unfold_walls::FILAMENT_MIN_WALL);
    w
}

/// Whether edge `e` can be the joint `kind` (`press` or `filament`). A cut: its faces meet
/// square round the outside of the body, and it is long enough for its fingers clear of both its
/// ends. A bend (`folds`): it folds by an angle the fingers round it can do, and is long enough
/// for two of them.
fn joint_at(unfold: &PrintUnfold, graph: &UnfoldGraph, e: usize, kind: &str, folds: bool) -> bool {
    use solidcraft_doc::print_unfold_walls::{bend_joint_possible, cut_filament_possible, press_possible};
    let w = walls_hint(unfold);
    graph.edges.get(e).is_some_and(|edge| {
        let len = edge.ends.0.dist(edge.ends.1);
        match (kind, folds) {
            ("press", false) => press_possible(edge.turn, len, w),
            ("filament", false) => cut_filament_possible(edge.turn, len, w),
            (_, true) => bend_joint_possible(edge.turn, len, w, kind == "press"),
            _ => false,
        }
    })
}

/// The joint edge `e` gets, and whether it is its own: a bend that folds gets what bends get, a
/// cut and a split bend what cuts get. A bend turns on its film, a cut is mitred; either is a
/// press fit or a filament joint where that is chosen and can be made, and plain where it can't.
fn joint_of(unfold: &PrintUnfold, graph: &UnfoldGraph, layout: &UnfoldLayout, splits: &[usize], e: usize) -> (String, bool) {
    let joints = unfold.joints.clone().unwrap_or_default();
    let folds = layout.kinds.get(e) == Some(&EdgeKind::Bend) && !splits.contains(&e);
    let plain = if folds { "film" } else { "mitre" };
    let own = joints.edges.iter().find(|(p, _)| graph.edge_at(*p) == Some(e)).map(|(_, kind)| kind.as_str());
    let all = if folds { joints.bends.as_str() } else { joints.cuts.as_str() };
    let kind = own.unwrap_or(all);
    let able = ["press", "filament"].contains(&kind) && joint_at(unfold, graph, e, kind, folds);
    (if able { kind } else { plain }.into(), own.is_some())
}

/// The least wall the joints chosen need over all edges of the flat pattern (mm): a press fit
/// on a cut room for its nub, a filament joint on a cut the channel with a film of wall either
/// side. Round a bend the fingers carry the joint: the wall can be thinner than it.
pub fn min_wall(unfold: &PrintUnfold, graph: &UnfoldGraph, layout: &UnfoldLayout, bends: &[usize]) -> f64 {
    use solidcraft_doc::print_unfold_walls::{FILAMENT_MIN_WALL, PRESS_MIN_WALL};
    let splits = unfold.resolve_splits(graph, bends);
    let mut least: f64 = 0.0;
    for e in 0..graph.edges.len() {
        let folds = layout.kinds.get(e) == Some(&EdgeKind::Bend) && !splits.contains(&e);
        let need = match joint_of(unfold, graph, layout, &splits, e).0.as_str() {
            _ if folds => 0.0,
            "press" => PRESS_MIN_WALL,
            "filament" => FILAMENT_MIN_WALL,
            _ => 0.0,
        };
        least = least.max(need);
    }
    // Rounded up to a tenth of a millimetre, so a wall typed at the value is enough.
    (least * 10.0 - 1e-9).ceil() / 10.0
}

fn joints(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "print.unfold.joints";
    let (unfold, ..) = read(s, cmd)?;
    let mut joints = unfold.joints.unwrap_or_default();
    if let Some(wall) = crate::params::str_(p, "wall") {
        let text = wall.trim();
        if text.is_empty() || text.len() > 200 {
            return Err(bad(cmd, "`wall` must be a length, such as \"2 mm\""));
        }
        joints.wall = text.to_string();
    }
    if let Some(kind) = crate::params::str_(p, "bends") {
        if !["film", "filament", "press"].contains(&kind) {
            return Err(bad(cmd, "`bends` must be film, filament or press"));
        }
        joints.bends = kind.into();
    }
    if let Some(kind) = crate::params::str_(p, "cuts") {
        if !["mitre", "filament", "press"].contains(&kind) {
            return Err(bad(cmd, "`cuts` must be mitre, filament or press"));
        }
        joints.cuts = kind.into();
    }
    if let Some(u) = s.doc_mut().print_unfold.as_mut() {
        u.joints = Some(joints);
    }
    reply(s, cmd)
}

/// The edge of the flat pattern at `at`: any edge, on either of its two faces in the flat.
fn flat_edge_at(graph: &UnfoldGraph, flat: &FlatLayout, at: Vec3) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (e, edge) in graph.edges.iter().enumerate() {
        for f in [edge.faces.0, edge.faces.1] {
            let Some(m) = flat.moves.get(f) else { continue };
            let d = at.dist_to_segment(solidcraft_doc::apply_point(m, edge.ends.0), solidcraft_doc::apply_point(m, edge.ends.1));
            if d <= graph.near && best.is_none_or(|(_, b)| d < b) {
                best = Some((e, d));
            }
        }
    }
    best.map(|(e, _)| e)
}

fn joint(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "print.unfold.joint";
    let at = point(p, "edge", cmd)?;
    let kind = crate::params::str_(p, "kind")
        .filter(|k| ["plain", "filament", "press", "default"].contains(k))
        .ok_or_else(|| bad(cmd, "`kind` must be plain, filament, press or default"))?;
    let (unfold, graph, _, bends, layout) = read(s, cmd)?;
    if unfold.step != PrintUnfoldStep::Flat {
        return Err(bad(cmd, "the pattern is not laid flat (print.unfold.flatten), or its body is made already (print.unfold.unmake)"));
    }
    let flat = graph.flat(&layout, &unfold.resolve_splits(&graph, &bends)).map_err(|e| invalid(cmd, e))?;
    let e = flat_edge_at(&graph, &flat, at).ok_or_else(|| bad(cmd, "no edge of the flat pattern at `edge`"))?;
    let folds = layout.kinds.get(e) == Some(&EdgeKind::Bend) && !unfold.resolve_splits(&graph, &bends).contains(&e);
    if ["press", "filament"].contains(&kind) {
        let name = if kind == "press" { "press fit" } else { "filament joint" };
        if !joint_at(&unfold, &graph, e, kind, folds) {
            return Err(bad(
                cmd,
                if folds {
                    format!(
                        "this bend can't be a {name}: its faces meet in an inside corner of the body, fold by an angle the fingers can't do, or it is too short for two fingers clear of its ends (it turns on its film)"
                    )
                } else {
                    format!(
                        "this cut can't be a {name}: its faces don't meet square round the outside of the body, or it is too short for its fingers clear of its ends (it is mitred)"
                    )
                },
            ));
        }
        // Asked of this edge alone: has it room for its fingers?
        let mut kinds = vec![String::new(); graph.edges.len()];
        if let Some(k) = kinds.get_mut(e) {
            *k = kind.into();
        }
        let splits = unfold.resolve_splits(&graph, &bends);
        if graph.jointed_edges(&layout, &flat, &splits, walls_hint(&unfold), &kinds).get(e) != Some(&true) {
            return Err(bad(
                cmd,
                if folds {
                    format!("this bend can't be a {name}: its fingers would stand in another wall of the body (it turns on its film)")
                } else {
                    format!(
                        "this cut can't be a {name}: another wall of the flat pattern lies beside it, and its fingers would print as one with that wall (it is mitred)"
                    )
                },
            ));
        }
    }
    let mut joints = unfold.joints.unwrap_or_default();
    joints.edges.retain(|(p, _)| graph.edge_at(*p) != Some(e));
    if kind != "default"
        && let Some(edge) = graph.edges.get(e)
    {
        joints.edges.push((edge.middle(), kind.to_string()));
    }
    if let Some(u) = s.doc_mut().print_unfold.as_mut() {
        u.joints = Some(joints);
    }
    reply(s, cmd)
}

fn make(s: &mut Session, _: &Value) -> Result<Value> {
    let cmd = "print.unfold.make";
    let (unfold, graph, _, bends, layout) = read(s, cmd)?;
    if unfold.step != PrintUnfoldStep::Flat {
        return Err(bad(cmd, "lay the pattern flat first (print.unfold.flatten)"));
    }
    let splits = unfold.resolve_splits(&graph, &bends);
    let flat = graph.flat(&layout, &splits).map_err(|e| invalid(cmd, e))?;
    if flat.parts.iter().any(|p| !p.overlaps.is_empty()) {
        return Err(bad(cmd, "faces of the flat pattern lie on each other: split it at a bend first (print.unfold.split)"));
    }
    // The parts of the flat pattern made from the body's own walls, each a body of its own where
    // the pattern has it. The solid stays as it is, hidden.
    let joints = unfold.joints.clone().unwrap_or_default();
    if wall_mm(&joints.wall).is_none() {
        return Err(bad(cmd, "give the wall thickness as a number in mm"));
    }
    let w = walls_of(&unfold);
    let kinds: Vec<String> = (0..graph.edges.len()).map(|e| joint_of(&unfold, &graph, &layout, &splits, e).0).collect();
    let parts = graph.wall_solids(&layout, &flat, &splits, w, &kinds).map_err(|e| invalid(cmd, e))?;
    let refs: Vec<&solidcraft_kernel::Body> = parts.iter().collect();
    let text = solidcraft_kernel::step_export(&refs, "SolidCraft unfold for printing")?;
    let mut scratch = s.scratch();
    if let Some(old) = unfold.surface {
        scratch.doc_mut().delete_feature(old)?;
    }
    let kind = FeatureKind::Import { file: format!("{} printable parts", unfold.body), step: text.into(), components: Vec::new(), skip: Vec::new() };
    let id = scratch.doc_mut().add_feature(kind, None)?;
    if let Some(f) = scratch.doc_mut().feature_mut(id) {
        f.body_names = (1..=parts.len()).map(|i| format!("{} Part {i}", unfold.body)).collect();
    }
    if let Some(u) = scratch.doc_mut().print_unfold.as_mut() {
        u.step = PrintUnfoldStep::Solid;
        u.surface = None;
        u.parts = Some(id);
    }
    scratch.refresh();
    if let Some(error) = scratch.model.result(id).and_then(|r| r.error.as_ref()) {
        return Err(bad(cmd, format!("the printable parts weren't made: {error}")));
    }
    s.accept_computed(scratch);
    if !s.visibility.hidden_bodies.contains(&unfold.body) {
        s.visibility.hidden_bodies.push(unfold.body.clone());
    }
    reply(s, cmd)
}

fn unmake(s: &mut Session, _: &Value) -> Result<Value> {
    let cmd = "print.unfold.unmake";
    let (unfold, ..) = read_unfold(s, cmd)?;
    if unfold.step != PrintUnfoldStep::Solid {
        return reply(s, cmd);
    }
    let splits = unfold.splits.clone();
    let mut scratch = s.scratch();
    if let Some(id) = unfold.parts {
        scratch.doc_mut().delete_feature(id)?;
    }
    if let Some(u) = scratch.doc_mut().print_unfold.as_mut() {
        u.step = PrintUnfoldStep::Surface;
        u.parts = None;
    }
    scratch.refresh();
    s.accept_computed(scratch);
    remake(s, cmd, PrintUnfoldStep::Flat, splits)?;
    reply(s, cmd)
}

/// The unfolded print of `s` alone, without reading its body (which is the printable body once
/// that is made, no longer the solid of flat faces).
fn read_unfold(s: &Session, cmd: &str) -> Result<(PrintUnfold,)> {
    s.doc.print_unfold.clone().map(|u| (u,)).ok_or_else(|| bad(cmd, "this design unfolds no body for printing (start with print.unfold.start)"))
}

/// Make the surface model again for `step` (the faces where they lie on the body, or flat), with
/// the pattern split at `splits` (points on bends).
fn remake(s: &mut Session, cmd: &str, step: PrintUnfoldStep, splits: Vec<Vec3>) -> Result<()> {
    let (mut unfold, graph, _, bends, layout) = read(s, cmd)?;
    if !layout.complete() {
        let missing = layout.attached.iter().filter(|a| !**a).count();
        return Err(bad(
            cmd,
            format!("{missing} of the body's {} faces are not on the pattern yet: set a bend to each of them first", graph.faces.len()),
        ));
    }
    unfold.splits = splits;
    let patches = if step == PrintUnfoldStep::Flat {
        let flat = graph.flat(&layout, &unfold.resolve_splits(&graph, &bends)).map_err(|e| invalid(cmd, e))?;
        flat.patches(&graph).map_err(|e| invalid(cmd, e))?
    } else {
        graph.patches().map_err(|e| invalid(cmd, e))?
    };
    let refs: Vec<&solidcraft_kernel::Body> = patches.iter().collect();
    let text = solidcraft_kernel::step_export(&refs, "SolidCraft unfold for printing")?;
    let mut scratch = s.scratch();
    if let Some(old) = unfold.surface {
        scratch.doc_mut().delete_feature(old)?;
    }
    let kind = FeatureKind::Import { file: format!("{} faces", unfold.body), step: text.into(), components: Vec::new(), skip: Vec::new() };
    let id = scratch.doc_mut().add_feature(kind, None)?;
    if let Some(f) = scratch.doc_mut().feature_mut(id) {
        f.body_names = (1..=patches.len()).map(|i| format!("{} Face {i}", unfold.body)).collect();
    }
    if let Some(u) = scratch.doc_mut().print_unfold.as_mut() {
        u.step = step;
        u.surface = Some(id);
        u.splits = unfold.splits.clone();
    }
    scratch.refresh();
    if let Some(error) = scratch.model.result(id).and_then(|r| r.error.as_ref()) {
        return Err(bad(cmd, format!("the surface model wasn't made: {error}")));
    }
    s.accept_computed(scratch);
    if !s.visibility.hidden_bodies.contains(&unfold.body) {
        s.visibility.hidden_bodies.push(unfold.body.clone());
    }
    Ok(())
}

fn flatten(s: &mut Session, _: &Value) -> Result<Value> {
    let cmd = "print.unfold.flatten";
    let (unfold, ..) = read(s, cmd)?;
    if unfold.step != PrintUnfoldStep::Flat {
        remake(s, cmd, PrintUnfoldStep::Flat, Vec::new())?;
    }
    reply(s, cmd)
}

fn bed(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "print.unfold.bed";
    read(s, cmd)?;
    let size = |key: &str| {
        p.get(key)
            .and_then(Value::as_f64)
            .filter(|v| (10.0..=5000.0).contains(v))
            .ok_or_else(|| bad(cmd, format!("`{key}` must be 10 to 5000, in mm")))
    };
    let (width, depth) = (size("width")?, size("depth")?);
    let name = crate::params::str_(p, "name").unwrap_or_default().trim().chars().take(80).collect::<String>();
    if let Some(u) = s.doc_mut().print_unfold.as_mut() {
        u.bed = Some(PrintBed { name, width, depth });
    }
    reply(s, cmd)
}

fn split(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "print.unfold.split";
    let at = point(p, "edge", cmd)?;
    let (unfold, graph, _, bends, layout) = read(s, cmd)?;
    if unfold.step != PrintUnfoldStep::Flat {
        return Err(bad(cmd, "the pattern is not laid flat yet (print.unfold.flatten)"));
    }
    let mut splits = unfold.resolve_splits(&graph, &bends);
    let flat = graph.flat(&layout, &splits).map_err(|e| invalid(cmd, e))?;
    // The bend nearest the point, on either of its two faces in the flat.
    let mut best: Option<(usize, f64)> = None;
    for e in &bends {
        let Some(edge) = graph.edges.get(*e) else { continue };
        for f in [edge.faces.0, edge.faces.1] {
            let Some(m) = flat.moves.get(f) else { continue };
            let d = at.dist_to_segment(solidcraft_doc::apply_point(m, edge.ends.0), solidcraft_doc::apply_point(m, edge.ends.1));
            if d <= graph.near && best.is_none_or(|(_, b)| d < b) {
                best = Some((*e, d));
            }
        }
    }
    let Some((e, _)) = best else { return Err(bad(cmd, "no bend of the flat pattern at `edge`: the pattern splits at bends")) };
    let changed = if let Some(i) = splits.iter().position(|x| *x == e) {
        splits.remove(i);
        "joined"
    } else {
        splits.push(e);
        "split"
    };
    let points: Vec<Vec3> = splits.iter().filter_map(|e| graph.edges.get(*e)).map(|e| e.middle()).collect();
    remake(s, cmd, PrintUnfoldStep::Flat, points)?;
    let mut out = state_of(s, cmd)?;
    out.changed = Some(changed.into());
    serde_json::to_value(out).map_err(|e| EngineError::Other(e.to_string()))
}

fn reply(s: &Session, cmd: &str) -> Result<Value> {
    serde_json::to_value(state_of(s, cmd)?).map_err(|e| EngineError::Other(e.to_string()))
}

fn state(s: &mut Session, _: &Value) -> Result<Value> {
    reply(s, "print.unfold.state")
}

fn bend(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "print.unfold.bend";
    let at = point(p, "edge", cmd)?;
    let (unfold, graph, base, mut bends, _) = read(s, cmd)?;
    if unfold.step != PrintUnfoldStep::Edges {
        return Err(bad(cmd, "the surface model is made: go back to the edges first (print.unfold.edit)"));
    }
    let e = graph.edge_at(at).ok_or_else(|| bad(cmd, "no edge of the body at `edge`"))?;
    let groups = unfold.resolve_groups(&graph).map_err(|e| invalid(cmd, e))?;
    let change = graph.toggle_bend(base, &mut bends, &groups, e).map_err(|e| invalid(cmd, e))?;
    let points: Vec<Vec3> = bends.iter().filter_map(|b| graph.edges.get(*b)).map(|e| e.middle()).collect();
    if let Some(u) = s.doc_mut().print_unfold.as_mut() {
        u.bends = points;
    }
    let mut out = state_of(s, cmd)?;
    out.changed = Some(match change {
        BendChange::Added { .. } => "added".into(),
        BendChange::Moved { .. } => "moved".into(),
        BendChange::Removed { .. } => "removed".into(),
    });
    serde_json::to_value(out).map_err(|e| EngineError::Other(e.to_string()))
}

/// The bends of `bends` that still hang on the pattern from the base face and the first face of
/// each group, none of them across a group's border.
fn reached(graph: &UnfoldGraph, base: usize, bends: &[usize], groups: &[Vec<usize>]) -> Vec<usize> {
    let group_of = |f: usize| groups.iter().position(|g| g.contains(&f));
    let mut on = vec![false; graph.faces.len()];
    for f in std::iter::once(base).chain(groups.iter().filter_map(|g| g.first().copied())) {
        if let Some(slot) = on.get_mut(f) {
            *slot = true;
        }
    }
    let mut kept = Vec::new();
    loop {
        let before = kept.len();
        for e in bends {
            let Some(edge) = graph.edges.get(*e) else { continue };
            let (a, b) = edge.faces;
            let (at_a, at_b) = (on.get(a).copied().unwrap_or(false), on.get(b).copied().unwrap_or(false));
            if kept.contains(e) || group_of(a) != group_of(b) || at_a == at_b {
                continue;
            }
            if let Some(slot) = on.get_mut(if at_a { b } else { a }) {
                *slot = true;
            }
            kept.push(*e);
        }
        if kept.len() == before {
            break;
        }
    }
    kept
}

/// Save the groups and the bends that are left with them.
fn set_groups(s: &mut Session, graph: &UnfoldGraph, base: usize, bends: &[usize], groups: &[Vec<usize>], cmd: &str) -> Result<()> {
    let mut saved = Vec::with_capacity(groups.len());
    for g in groups {
        let mut faces = Vec::with_capacity(g.len());
        for f in g {
            let p = graph.faces.get(*f).and_then(|f| f.inner_point()).ok_or_else(|| bad(cmd, "a face to carve out has no inside"))?;
            faces.push(p);
        }
        saved.push(solidcraft_doc::print_unfold::PrintGroup { faces });
    }
    let kept = reached(graph, base, bends, groups);
    graph.layout(base, &kept, groups).map_err(|e| invalid(cmd, e))?;
    let points: Vec<Vec3> = kept.iter().filter_map(|b| graph.edges.get(*b)).map(|e| e.middle()).collect();
    if let Some(u) = s.doc_mut().print_unfold.as_mut() {
        u.groups = saved;
        u.bends = points;
    }
    Ok(())
}

fn group(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "print.unfold.group";
    let (unfold, graph, base, bends, _) = read(s, cmd)?;
    if unfold.step != PrintUnfoldStep::Edges {
        return Err(bad(cmd, "the surface model is made: go back to the edges first (print.unfold.edit)"));
    }
    let picked = p
        .get("faces")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty() && a.len() <= 200)
        .ok_or_else(|| bad(cmd, "give `faces`: 1 to 200 points"))?;
    let mut groups = unfold.resolve_groups(&graph).map_err(|e| invalid(cmd, e))?;
    let mut faces = Vec::with_capacity(picked.len());
    for at in picked {
        let at = crate::params::vec3(at).ok_or_else(|| bad(cmd, "each of `faces` must be [x, y, z] of numbers, in mm"))?;
        let f = graph.face_at(at).ok_or_else(|| bad(cmd, "no face of the body at one of `faces`"))?;
        if f == base {
            return Err(bad(cmd, "the base face can't be carved out: it is where the pattern starts"));
        }
        if groups.iter().any(|g| g.contains(&f)) {
            return Err(bad(cmd, "one of the faces is carved out already (print.unfold.ungroup puts its group back)"));
        }
        if !faces.contains(&f) {
            faces.push(f);
        }
    }
    groups.push(faces);
    set_groups(s, &graph, base, &bends, &groups, cmd)?;
    reply(s, cmd)
}

fn ungroup(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "print.unfold.ungroup";
    let at = point(p, "face", cmd)?;
    let (unfold, graph, base, bends, _) = read(s, cmd)?;
    if unfold.step != PrintUnfoldStep::Edges {
        return Err(bad(cmd, "the surface model is made: go back to the edges first (print.unfold.edit)"));
    }
    let mut groups = unfold.resolve_groups(&graph).map_err(|e| invalid(cmd, e))?;
    let f = graph.face_at(at).ok_or_else(|| bad(cmd, "no face of the body at `face`"))?;
    let k = groups.iter().position(|g| g.contains(&f)).ok_or_else(|| bad(cmd, "the face at `face` is in no carved-out group"))?;
    let gone = groups.remove(k);
    // The bends between its faces go with it: they are inside corners, which fold in a group only.
    let left: Vec<usize> =
        bends.iter().copied().filter(|e| !graph.edges.get(*e).is_some_and(|x| gone.contains(&x.faces.0) || gone.contains(&x.faces.1))).collect();
    set_groups(s, &graph, base, &left, &groups, cmd)?;
    reply(s, cmd)
}

fn surface(s: &mut Session, _: &Value) -> Result<Value> {
    let cmd = "print.unfold.surface";
    let (unfold, graph, _, _, layout) = read(s, cmd)?;
    if unfold.step != PrintUnfoldStep::Edges {
        return Err(bad(cmd, "the surface model is made already"));
    }
    if !layout.complete() {
        let missing = layout.attached.iter().filter(|a| !**a).count();
        return Err(bad(
            cmd,
            format!("{missing} of the body's {} faces are not on the pattern yet: set a bend to each of them first", graph.faces.len()),
        ));
    }
    let patches = graph.patches().map_err(|e| invalid(cmd, e))?;
    let refs: Vec<&solidcraft_kernel::Body> = patches.iter().collect();
    let step = solidcraft_kernel::step_export(&refs, "SolidCraft unfold for printing")?;
    let mut scratch = s.scratch();
    let kind = FeatureKind::Import { file: format!("{} faces", unfold.body), step: step.into(), components: Vec::new(), skip: Vec::new() };
    let id = scratch.doc_mut().add_feature(kind, None)?;
    let taken: Vec<String> = scratch.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    let names: Vec<String> = (1..=patches.len())
        .map(|i| {
            let name = format!("{} Face {i}", unfold.body);
            if taken.contains(&name) { solidcraft_doc::copy_name(&name, |n| taken.iter().any(|t| t == n)) } else { name }
        })
        .collect();
    if let Some(f) = scratch.doc_mut().feature_mut(id) {
        f.body_names = names;
    }
    if let Some(u) = scratch.doc_mut().print_unfold.as_mut() {
        u.step = PrintUnfoldStep::Surface;
        u.surface = Some(id);
    }
    scratch.refresh();
    if let Some(error) = scratch.model.result(id).and_then(|r| r.error.as_ref()) {
        return Err(bad(cmd, format!("the surface model wasn't made: {error}")));
    }
    s.accept_computed(scratch);
    if !s.visibility.hidden_bodies.contains(&unfold.body) {
        s.visibility.hidden_bodies.push(unfold.body.clone());
    }
    reply(s, cmd)
}

fn edit(s: &mut Session, _: &Value) -> Result<Value> {
    let cmd = "print.unfold.edit";
    let (unfold, ..) = read(s, cmd)?;
    if unfold.step == PrintUnfoldStep::Edges {
        return reply(s, cmd);
    }
    let mut scratch = s.scratch();
    if let Some(id) = unfold.surface {
        scratch.doc_mut().delete_feature(id)?;
    }
    if let Some(u) = scratch.doc_mut().print_unfold.as_mut() {
        u.step = PrintUnfoldStep::Edges;
        u.surface = None;
        u.splits = Vec::new();
    }
    scratch.refresh();
    s.accept_computed(scratch);
    s.visibility.hidden_bodies.retain(|b| *b != unfold.body);
    reply(s, cmd)
}

#[cfg(test)]
#[path = "print_unfold_tests.rs"]
mod tests;

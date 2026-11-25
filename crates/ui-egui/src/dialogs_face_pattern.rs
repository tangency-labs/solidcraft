//! The Face type of the pattern dialog (`solid.pattern.face`): copies of bodies standing on a
//! face, on a grid across it, kept clear of the face's edges and of what the Avoid pick holds, each
//! scaled, rotated and offset from the face by the main section's values. The Near/Far modifiers
//! section holds the attractors, which change each copy with its distance to them: the Near and
//! Far objects picks and the near and far values, each left empty the main section's. While
//! objects are picked there, they set the copies and the main section's values are greyed out.

use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{Attract, Avoid, FaceLayout, Falloff, Noise, PatternKind};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, form_row, input_rows_range, set_object_accepting};
use crate::selection::{self, BODIES, FACES, SelInput, SelectionExt as _, point_value as pt};
use crate::ui_kit::{self, choice_row, expr_row, hint_row};

/// The pick inputs the attractors go into, drawn in the Near/Far modifiers section while it is
/// open or they hold picks: the near set, and the optional far set the values blend towards.
pub(crate) const ATTRACTORS: &str = "Near objects";
pub(crate) const FAR_ATTRACTORS: &str = "Far objects";

/// The inputs drawn at the top of the dialog; the attractor picks, added after them, are drawn in
/// the Near/Far modifiers section.
pub(crate) const TOP_INPUTS: std::ops::Range<usize> = 0..3;

/// The pick for what no copy may come near.
pub(crate) const AVOID: &str = "Avoid";

/// What the copies may avoid: a vertex, a sketch or construction point, a face or a body. Not an
/// edge: no exact distance from a body to an edge is measured yet.
pub(crate) const AVOID_PICKS: selection::Accept = selection::VERTICES | selection::POINTS | selection::CONSTRUCTION_POINTS | FACES | BODIES;

/// What an attractor may be: a vertex, a sketch or construction point, an edge, a face or a body.
pub(crate) const ATTRACTOR_PICKS: selection::Accept =
    selection::VERTICES | selection::POINTS | selection::CONSTRUCTION_POINTS | selection::EDGES | FACES | BODIES;

/// Add or remove the attractor picks: added as the Near/Far modifiers section opens (or a pattern
/// with attractors is edited), picks going to Near objects first.
pub(crate) fn set_attractor_inputs(d: &mut Dialog, on: bool) {
    for label in [ATTRACTORS, FAR_ATTRACTORS] {
        set_object_accepting(&mut d.inputs, &mut d.active, label, ATTRACTOR_PICKS, on);
        if let Some(i) = d.inputs.iter_mut().find(|i| i.label == label) {
            i.multi = true;
        }
    }
    if on && let Some(i) = d.inputs.iter().position(|i| i.label == ATTRACTORS) {
        d.active = i;
    }
}

/// The dialog's values; the picks live in the dialog's inputs.
#[derive(Clone, Debug, PartialEq)]
pub struct Form {
    pub count1: String,
    pub count2: String,
    /// 0: grid; 1: grid with every other column half a cell along; 2: random; 3: rows a spacing
    /// apart; 4: a spiral; 5: an even fill; 6: a geodesic sphere ([`LAYOUTS`]).
    pub layout: usize,
    /// Spacing: along the rows and across them (empty: the same), and every other row moved half
    /// a spacing along.
    pub spacing: String,
    pub row_spacing: String,
    pub staggered_rows: bool,
    /// Spiral and even fill: how many instances.
    pub layout_count: String,
    /// Geodesic: its frequency.
    pub frequency: String,
    /// Random: how many instances, the seed, and the noise (0 white, 1 blue, 2 pink, 3 brown).
    pub random_count: String,
    pub seed: String,
    pub noise: usize,
    pub angle: String,
    pub margin: String,
    pub avoid_margin: String,
    /// Copies may hang over the face's edges (only their centres on the face).
    pub overhang: bool,
    /// The outer copies sit flush with the face's edges (with overhang, their centres on them),
    /// the edge margin moving them in. On for a new pattern; off for one made before it, so it
    /// keeps its copies where they were.
    pub fit_edges: bool,
    pub flip: bool,
    /// The instances go into a browser group (on by default).
    pub group: bool,
    /// Every instance's scale, rotation and offset from the face, and the near and far values
    /// left empty.
    pub scale: String,
    pub rotate: String,
    pub offset: String,
    pub near: String,
    pub far: String,
    pub scale_near: String,
    pub scale_far: String,
    pub offset_near: String,
    pub offset_far: String,
    pub rotate_near: String,
    pub rotate_far: String,
    pub cutoff: String,
    /// 0: linear; 1: smooth.
    pub falloff: usize,
    /// When editing: the saved face and attractor references, sent while nothing is picked
    /// in their place (only those the dialog could not show as picks).
    pub saved_face: Option<Value>,
    pub saved_more_faces: Vec<Value>,
    pub saved_geometry: Vec<Value>,
    pub saved_bodies: Vec<String>,
    pub saved_far_geometry: Vec<Value>,
    pub saved_far_bodies: Vec<String>,
    pub saved_avoid_geometry: Vec<Value>,
    pub saved_avoid_bodies: Vec<String>,
}

impl Default for Form {
    fn default() -> Self {
        Form {
            count1: "5".into(),
            count2: "5".into(),
            layout: 0,
            spacing: "10".into(),
            row_spacing: String::new(),
            staggered_rows: false,
            layout_count: "50".into(),
            frequency: "3".into(),
            random_count: "25".into(),
            seed: "1".into(),
            noise: 1,
            angle: String::new(),
            margin: String::new(),
            avoid_margin: String::new(),
            overhang: false,
            fit_edges: true,
            flip: false,
            group: true,
            scale: "1".into(),
            rotate: String::new(),
            offset: String::new(),
            near: String::new(),
            far: String::new(),
            scale_near: String::new(),
            scale_far: String::new(),
            offset_near: String::new(),
            offset_far: String::new(),
            rotate_near: String::new(),
            rotate_far: String::new(),
            cutoff: String::new(),
            falloff: 0,
            saved_face: None,
            saved_more_faces: Vec::new(),
            saved_geometry: Vec::new(),
            saved_bodies: Vec::new(),
            saved_far_geometry: Vec::new(),
            saved_far_bodies: Vec::new(),
            saved_avoid_geometry: Vec::new(),
            saved_avoid_bodies: Vec::new(),
        }
    }
}

/// The Layout choices, in the order of [`Form::layout`].
const LAYOUTS: [&str; 7] = ["Grid", "Staggered", "Random", "Spacing", "Spiral", "Even fill", "Geodesic"];

/// The Face pick takes several faces of one body: one region, copies across the edges they share.
/// The near and far attractors are added after these ([`set_attractor_inputs`]).
pub(crate) fn inputs() -> Vec<SelInput> {
    vec![SelInput::new("Objects", BODIES, true), SelInput::new("Face", FACES, true), SelInput::new(AVOID, AVOID_PICKS, true)]
}

/// The dialog for a new face pattern.
pub(crate) fn kind() -> Kind {
    Kind::FacePattern(Box::default())
}

/// The fields under the picks. Returns whether Enter was pressed in one.
pub(crate) fn draw(app: &SolveApp, d: &mut Dialog, ui: &mut egui::Ui) -> bool {
    let picked = |d: &Dialog, label: &str| d.inputs.iter().any(|i| i.label == label && !i.items.is_empty());
    let (near_picked, far_picked, avoiding) = (picked(d, ATTRACTORS), picked(d, FAR_ATTRACTORS), picked(d, AVOID));
    let Kind::FacePattern(f) = &mut d.kind else { return false };
    // With far attractors, the near and far distances are hold zones round each set.
    let between = far_picked || !f.saved_far_geometry.is_empty() || !f.saved_far_bodies.is_empty();
    let attracting = between || near_picked || !f.saved_geometry.is_empty() || !f.saved_bodies.is_empty();
    let avoiding = avoiding || !f.saved_avoid_geometry.is_empty() || !f.saved_avoid_bodies.is_empty();
    let doc = &app.session.doc;
    let mut enter = false;
    choice_row(ui, "Layout", "fp_layout", &mut f.layout, &LAYOUTS);
    match f.layout {
        2 => {
            enter |= crate::dialogs::quantity_row(ui, doc, "Quantity", &mut f.random_count);
            enter |= expr_row(ui, "Seed", &mut f.seed);
            choice_row(ui, "Noise", "fp_noise", &mut f.noise, &["White noise", "Blue noise", "Pink noise", "Brown noise"]);
        }
        3 => {
            enter |= expr_row(ui, "Spacing", &mut f.spacing);
            enter |= hint_row(ui, "Row spacing", &mut f.row_spacing, "same as spacing");
            form_row(ui, "", |ui| {
                ui_kit::checkbox(ui, "Staggered rows", &mut f.staggered_rows);
            });
        }
        4 | 5 => enter |= crate::dialogs::quantity_row(ui, doc, "Quantity", &mut f.layout_count),
        6 => {
            enter |= expr_row(ui, "Frequency", &mut f.frequency);
            enter |= hint_row(ui, "Turn", &mut f.angle, "none");
        }
        _ => {
            enter |= crate::dialogs::quantity_row(ui, doc, "Quantity U", &mut f.count1);
            enter |= crate::dialogs::quantity_row(ui, doc, "Quantity V", &mut f.count2);
            enter |= hint_row(ui, "Grid angle", &mut f.angle, "none");
        }
    }
    enter |= hint_row(ui, "Edge margin", &mut f.margin, "none");
    form_row(ui, "", |ui| {
        ui_kit::checkbox(ui, "Allow overhang", &mut f.overhang);
    });
    if avoiding {
        enter |= hint_row(ui, "Avoid margin", &mut f.avoid_margin, "none");
    }
    // Every instance's own scale, rotation and offset; while near or far objects are picked they
    // set these instead, from the values below, so these are greyed out.
    enter |= ui
        .add_enabled_ui(!attracting, |ui| {
            let mut enter = expr_row(ui, "Scale", &mut f.scale);
            enter |= hint_row(ui, "Rotate", &mut f.rotate, "none");
            enter | hint_row(ui, "Offset from face", &mut f.offset, "none")
        })
        .inner;
    form_row(ui, "", |ui| {
        ui_kit::checkbox(ui, "Flip", &mut f.flip);
    });
    form_row(ui, "", |ui| {
        ui_kit::checkbox(ui, "Group Instances", &mut f.group);
    });
    let names = app.preferences.origin_plane_names;
    let shown = ui_kit::section(ui, "face_pattern_near_far", "Near/Far modifiers", attracting, |ui| {
        if !d.inputs.iter().any(|i| i.label == ATTRACTORS) {
            set_attractor_inputs(d, true);
        }
        if let Some(at) = d.inputs.iter().position(|i| i.label == ATTRACTORS) {
            input_rows_range(d, ui, &app.session, names, at..at + 2);
        }
        let Kind::FacePattern(f) = &mut d.kind else { return false };
        let mut enter = false;
        if between {
            ui_kit::status_line(ui, "Instances change from the near objects to the far objects", ui_kit::StatusTone::Dim);
            enter |= hint_row(ui, "Near distance", &mut f.near, "hold near values within");
            enter |= hint_row(ui, "Far distance", &mut f.far, "hold far values within");
        } else {
            ui_kit::status_line(ui, "Each instance changes with its distance to the nearest near object", ui_kit::StatusTone::Dim);
            enter |= hint_row(ui, "Near distance", &mut f.near, "nearest instance");
            enter |= hint_row(ui, "Far distance", &mut f.far, "farthest instance");
        }
        // Each value left empty is the main section's, which its hint shows.
        let main = |v: &str, none: &'static str| if v.trim().is_empty() { none.to_string() } else { v.trim().to_string() };
        let (scale, rotate, offset) = (main(&f.scale, "1"), main(&f.rotate, "none"), main(&f.offset, "none"));
        enter |= hint_row(ui, "Scale near", &mut f.scale_near, &scale);
        enter |= hint_row(ui, "Scale far", &mut f.scale_far, &scale);
        enter |= hint_row(ui, "Rotate near", &mut f.rotate_near, &rotate);
        enter |= hint_row(ui, "Rotate far", &mut f.rotate_far, &rotate);
        enter |= hint_row(ui, "Offset from face near", &mut f.offset_near, &offset);
        enter |= hint_row(ui, "Offset from face far", &mut f.offset_far, &offset);
        enter |= hint_row(ui, "Cutoff", &mut f.cutoff, "none");
        choice_row(ui, "Falloff", "fp_falloff", &mut f.falloff, &["Linear", "Smooth"]);
        enter
    });
    // Folded away, the section's picks go while empty, and take no more picks.
    if shown.is_none() {
        if !attracting {
            set_attractor_inputs(d, false);
        }
        if d.inputs.get(d.active).is_some_and(|i| i.label == ATTRACTORS || i.label == FAR_ATTRACTORS) {
            d.active = 0;
        }
    }
    enter | shown.unwrap_or(false)
}

/// Put `v` under `key` unless it is empty.
fn put_value(p: &mut Value, key: &str, v: &str) {
    if !v.trim().is_empty() {
        p[key] = json!(v.trim());
    }
}

/// The command's parameters.
pub(crate) fn params(app: &SolveApp, d: &Dialog) -> Result<Value, String> {
    let Kind::FacePattern(f) = &d.kind else { return Err("not a face pattern".into()) };
    let picks = |label: &str| d.inputs.iter().find(|i| i.label == label).map_or(&[][..], |i| i.items.as_slice());
    let bodies = picks("Objects").body_names();
    if bodies.is_empty() {
        return Err("select the bodies to copy".into());
    }
    // The faces picked, the first the pattern's face; while none is, the faces as saved.
    let picked: Vec<Value> = picks("Face")
        .iter()
        .filter_map(|sel| match sel {
            Sel::Face { point, .. } => Some(json!({"face": pt(*point)})),
            _ => None,
        })
        .collect();
    let (face, more) = match picked.split_first() {
        Some((first, rest)) => (first.clone(), rest.to_vec()),
        None => (f.saved_face.clone().ok_or("select the face to pattern on")?, f.saved_more_faces.clone()),
    };
    let mut p = json!({"bodies": bodies, "face": face});
    if !more.is_empty() {
        p["faces"] = json!(more);
    }
    match f.layout {
        2 => {
            let seed: u32 = f.seed.trim().parse().map_err(|_| "the seed is a whole number from 0 to 4294967295".to_string())?;
            let noise = ["white", "blue", "pink", "brown"].get(f.noise).copied().unwrap_or("white");
            p["random"] = json!({"count": f.random_count.trim(), "seed": seed, "noise": noise});
        }
        3 => {
            let mut l = json!({"type": "spacing", "spacing": f.spacing.trim()});
            if !f.row_spacing.trim().is_empty() {
                l["row_spacing"] = json!(f.row_spacing.trim());
            }
            p["layout"] = l;
        }
        4 => p["layout"] = json!({"type": "spiral", "count": f.layout_count.trim()}),
        5 => p["layout"] = json!({"type": "fill", "count": f.layout_count.trim()}),
        6 => p["layout"] = json!({"type": "geodesic", "frequency": f.frequency.trim()}),
        _ => {
            p["count1"] = json!(f.count1.trim());
            p["count2"] = json!(f.count2.trim());
        }
    }
    let mut put = |key: &str, v: &str| {
        if !v.trim().is_empty() {
            p[key] = json!(v.trim());
        }
    };
    // The grid's angle turns it; the geodesic's, about the sphere's pole.
    if matches!(f.layout, 0 | 1 | 6) {
        put("angle", &f.angle);
    }
    put("margin", &f.margin);
    if f.layout == 1 || (f.layout == 3 && f.staggered_rows) {
        p["staggered"] = json!(true);
    }
    if f.overhang {
        p["overhang"] = json!(true);
    }
    if f.fit_edges {
        p["fit_edges"] = json!(true);
    }
    if f.flip {
        p["flip"] = json!(true);
    }
    if !f.group {
        p["group"] = json!(false);
    }
    // Every instance's scale, rotation and offset; a scale of 1 is the default.
    if f.scale.trim() != "1" {
        put_value(&mut p, "scale", &f.scale);
    }
    put_value(&mut p, "rotate", &f.rotate);
    put_value(&mut p, "offset", &f.offset);
    // One set of picks: geometry references and whole bodies; while nothing is picked, the set as
    // saved.
    let set = |label: &str, saved_geometry: &[Value], saved_bodies: &[String]| -> Result<(Vec<Value>, Vec<String>), String> {
        let items = picks(label);
        if items.is_empty() {
            return Ok((saved_geometry.to_vec(), saved_bodies.to_vec()));
        }
        let mut geometry = Vec::new();
        let mut bodies = Vec::new();
        for sel in items {
            match sel {
                Sel::Body { name } => bodies.push(name.clone()),
                other => geometry.push(crate::dialogs::construct_ref(app, other)?),
            }
        }
        Ok((geometry, bodies))
    };
    let (avoid_geometry, avoid_bodies) = set(AVOID, &f.saved_avoid_geometry, &f.saved_avoid_bodies)?;
    if !avoid_geometry.is_empty() || !avoid_bodies.is_empty() {
        let mut a = json!({});
        if !avoid_geometry.is_empty() {
            a["geometry"] = json!(avoid_geometry);
        }
        if !avoid_bodies.is_empty() {
            a["bodies"] = json!(avoid_bodies);
        }
        if !f.avoid_margin.trim().is_empty() {
            a["margin"] = json!(f.avoid_margin.trim());
        }
        p["avoid"] = a;
    }
    // Near/Far modifiers, while near or far objects are picked.
    let (geometry, bodies) = set(ATTRACTORS, &f.saved_geometry, &f.saved_bodies)?;
    let (far_geometry, far_bodies) = set(FAR_ATTRACTORS, &f.saved_far_geometry, &f.saved_far_bodies)?;
    let far_only = geometry.is_empty() && bodies.is_empty();
    if far_only && (!far_geometry.is_empty() || !far_bodies.is_empty()) {
        return Err("select the near objects as well as the far ones".into());
    }
    if !far_only {
        let mut a = json!({});
        for (key, list) in
            [("geometry", json!(geometry)), ("bodies", json!(bodies)), ("far_geometry", json!(far_geometry)), ("far_bodies", json!(far_bodies))]
        {
            if list.as_array().is_some_and(|l| !l.is_empty()) {
                a[key] = list;
            }
        }
        for (key, v) in [
            ("near", &f.near),
            ("far", &f.far),
            ("scale_near", &f.scale_near),
            ("scale_far", &f.scale_far),
            ("offset_near", &f.offset_near),
            ("offset_far", &f.offset_far),
            ("rotate_near", &f.rotate_near),
            ("rotate_far", &f.rotate_far),
            ("cutoff", &f.cutoff),
        ] {
            if !v.trim().is_empty() {
                a[key] = json!(v.trim());
            }
        }
        if f.falloff == 1 {
            a["falloff"] = json!("smooth");
        }
        p["attract"] = a;
    }
    Ok(p)
}

/// The dialog's values for a saved face pattern.
pub(crate) fn form_of(pattern: &PatternKind) -> Option<Form> {
    let PatternKind::Face {
        face,
        more_faces,
        layout,
        fit_edges,
        count1,
        count2,
        staggered,
        random,
        angle,
        margin,
        overhang,
        avoid,
        flip,
        anchor: _,
        scale,
        offset,
        rotate,
        attract,
    } = pattern
    else {
        return None;
    };
    let s = |e: &Option<solidcraft_engine::doc::Expr>| e.as_deref().map(str::to_string).unwrap_or_default();
    let mut f = Form {
        count1: count1.to_string(),
        count2: count2.to_string(),
        layout: usize::from(*staggered),
        angle: s(angle),
        margin: s(margin),
        overhang: *overhang,
        fit_edges: *fit_edges,
        flip: *flip,
        scale: scale.as_deref().unwrap_or("1").to_string(),
        rotate: s(rotate),
        offset: s(offset),
        saved_face: serde_json::to_value(face).ok(),
        saved_more_faces: more_faces.iter().filter_map(|g| serde_json::to_value(g).ok()).collect(),
        ..Form::default()
    };
    match layout {
        Some(FaceLayout::Spacing { spacing, row_spacing }) => {
            f.layout = 3;
            f.spacing = spacing.to_string();
            f.row_spacing = s(row_spacing);
            f.staggered_rows = *staggered;
        }
        Some(FaceLayout::Spiral { count }) => {
            f.layout = 4;
            f.layout_count = count.to_string();
        }
        Some(FaceLayout::Fill { count }) => {
            f.layout = 5;
            f.layout_count = count.to_string();
        }
        Some(FaceLayout::Geodesic { frequency }) => {
            f.layout = 6;
            f.frequency = frequency.to_string();
        }
        None => {}
    }
    if let Some(r) = random {
        f.layout = 2;
        f.random_count = r.count.to_string();
        f.seed = r.seed.to_string();
        f.noise = match r.noise {
            Noise::White => 0,
            Noise::Blue => 1,
            Noise::Pink => 2,
            Noise::Brown => 3,
        };
    }
    if let Some(Avoid { geometry, bodies, margin }) = avoid.as_deref() {
        f.saved_avoid_geometry = geometry.iter().filter_map(|g| serde_json::to_value(g).ok()).collect();
        f.saved_avoid_bodies = bodies.iter().map(|b| b.0.clone()).collect();
        f.avoid_margin = s(margin);
    }
    if let Some(a) = attract.as_deref() {
        let Attract {
            geometry,
            bodies,
            far_geometry,
            far_bodies,
            near,
            far,
            scale_near,
            scale_far,
            offset_near,
            offset_far,
            rotate_near,
            rotate_far,
            cutoff,
            falloff,
        } = a;
        f.saved_geometry = geometry.iter().filter_map(|g| serde_json::to_value(g).ok()).collect();
        f.saved_bodies = bodies.iter().map(|b| b.0.clone()).collect();
        f.saved_far_geometry = far_geometry.iter().filter_map(|g| serde_json::to_value(g).ok()).collect();
        f.saved_far_bodies = far_bodies.iter().map(|b| b.0.clone()).collect();
        f.near = s(near);
        f.far = s(far);
        f.scale_near = s(scale_near);
        f.scale_far = s(scale_far);
        f.offset_near = s(offset_near);
        f.offset_far = s(offset_far);
        f.rotate_near = s(rotate_near);
        f.rotate_far = s(rotate_far);
        f.cutoff = s(cutoff);
        f.falloff = usize::from(*falloff == Falloff::Smooth);
    }
    Some(f)
}

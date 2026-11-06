//! Sheet metal: rules, Flange (base, edge, contour), Hem, Unfold/Refold, Convert to Sheet Metal,
//! the flat pattern (outline, bend lines, bend table) and its DXF export.

use serde_json::{Value, json};
use solvecraft_doc::FeatureKind;
use solvecraft_doc::expr::Kind;
use solvecraft_doc::sheet::{SheetBody, SheetRule};
use solvecraft_geom::{Vec2, Vec3};

use super::CommandSpec;
use super::features::{add_feature, check_expr, feature_sketch, profiles, sketch_id};
use crate::params::{bad, bool_, expr, req_expr, str_, string_list, vec3};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sheet.flange", "Flange", flange).at("SHEET METAL", "CREATE").icon("flange").params(
        "base: sketch + profiles? (rule?, flip?) | contour: sketch + curves (open chain of lines) + distance (rule?, flip?, reverse?) | \
         edge: edges: [[x,y,z] on top/bottom edges] + height (to the outer face), angle? (default 90 deg), radius? (default: rule), \
         position?: inside|outside|middle, flip?, body?; type?: base|edge|contour (default from the inputs); name?, body_name? (the new body)",
    ),
    CommandSpec::new("sheet.hem", "Hem", hem)
        .at("SHEET METAL", "CREATE")
        .icon("hem")
        .params("edges: [[x,y,z] on sheet edges]; length (to the outer face); gap? (inner radius, default: rule); flip?; body?"),
    CommandSpec::new("sheet.fold", "Fold", fold).at("SHEET METAL", "CREATE").icon("fold").params(
        "line: sketch + curve (a sketch line on the sheet's base face; followed when the sketch changes) or points: [[x,y,z], [x,y,z]]; \
         angle? (default 90 deg); radius? (default: rule); position?: centerline|start|end|mould (where the line sits in the bend, default centerline); \
         fixed?: [x,y,z] on the side that stays (default: the larger side); flip? (fold the other way); body?",
    ),
    CommandSpec::new("sheet.unfold", "Unfold", unfold).at("SHEET METAL", "MODIFY").icon("unfold").params("body? (default: the last sheet body) — all bends"),
    CommandSpec::new("sheet.refold", "Refold", refold).at("SHEET METAL", "MODIFY").icon("refold").params("body? (default: the last unfolded sheet)"),
    CommandSpec::new("sheet.convert", "Convert to Sheet Metal", convert)
        .at("SHEET METAL", "CREATE")
        .icon("convert_sheet")
        .params("body; face: [x,y,z] on its large flat face (a plate of even thickness); rule?"),
    CommandSpec::new("sheet.manage_rules", "Sheet Metal Rules", rules).at("SHEET METAL", "MODIFY").icon("sheet_rules").params(
        "name (new or existing); thickness?, k_factor?, bend_radius?, relief_width?, relief_depth?, corner_relief?, corner_relief_shape? (square|round|laser_weld), hem_gap?, gap? (expressions; `Thickness` is the rule's); active?: bool; delete?: bool",
    ),
    CommandSpec::new("sheet.rules", "List Sheet Metal Rules", list_rules).noundo().params("→ rules with their expressions and values, and the active one"),
    CommandSpec::new("sheet.flat_pattern", "Create Flat Pattern", flat_pattern)
        .at("SHEET METAL", "CREATE")
        .icon("flat")
        .noundo()
        .params("body? → flat size, outline loops and cut-outs (flat coordinates), bend lines and the bend table"),
    CommandSpec::new("sheet.export_dxf", "Export Flat Pattern as DXF", export_dxf)
        .noundo()
        .params("body?, path? → DXF (mm): OUTLINE, BEND_UP and BEND_DOWN (centre lines) layers"),
    CommandSpec::new("sheet.export_bend_table", "Export Bend Table", export_bend_table)
        .noundo()
        .params("body?, format?: csv|json (default csv), path? → bend table in mm and degrees, including rule and flat centre-line coordinates"),
];

fn points(p: &Value, k: &str, cmd: &str) -> Result<Vec<Vec3>> {
    let a = p.get(k).and_then(Value::as_array).ok_or_else(|| bad(cmd, format!("`{k}` must list points [x, y, z] on sheet edges")))?;
    if a.is_empty() || a.len() > 1000 {
        return Err(bad(cmd, format!("`{k}` must list 1…1000 points")));
    }
    a.iter()
        .map(|v| vec3(v).or_else(|| v.get("point").and_then(vec3)).ok_or_else(|| bad(cmd, format!("`{k}` must contain [x, y, z] points"))))
        .collect()
}

fn rule_param(s: &Session, p: &Value, cmd: &str) -> Result<Option<String>> {
    match str_(p, "rule") {
        Some(r) if s.doc.sheet.rules.iter().any(|x| x.name == r) || r == SheetRule::steel().name => Ok(Some(r.to_string())),
        Some(r) => Err(bad(cmd, format!("no sheet metal rule `{r}` (sheet.manage_rules makes one)"))),
        None => Ok(None),
    }
}

fn flange(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.flange";
    let ty = match str_(p, "type") {
        Some(t) => t.to_ascii_lowercase(),
        None if p.get("edges").is_some() => "edge".into(),
        None if p.get("curves").is_some() => "contour".into(),
        None => "base".into(),
    };
    match ty.as_str() {
        "base" => {
            let sketch = feature_sketch(s, p, cmd)?;
            let rule = rule_param(s, p, cmd)?;
            add_feature(s, p, FeatureKind::SheetBase { sketch, profiles: profiles(p, cmd)?, rule, flip: bool_(p, "flip").unwrap_or(false) })
        }
        "contour" => {
            let sketch = sketch_id(s, p.get("sketch"), cmd, "sketch")?;
            let curves = string_list(p, "curves");
            if curves.is_empty() || curves.len() > 1000 {
                return Err(bad(cmd, "`curves` must list the open chain's lines"));
            }
            let distance = req_expr(cmd, p, "distance")?;
            check_expr(s, &distance, Kind::Length, cmd, "distance")?;
            let rule = rule_param(s, p, cmd)?;
            add_feature(
                s,
                p,
                FeatureKind::SheetContour {
                    sketch,
                    curves,
                    distance,
                    rule,
                    flip: bool_(p, "flip").unwrap_or(false),
                    reverse: bool_(p, "reverse").unwrap_or(false),
                },
            )
        }
        "edge" => {
            let edges = points(p, "edges", cmd)?;
            let height = req_expr(cmd, p, "height")?;
            check_expr(s, &height, Kind::Length, cmd, "height")?;
            let angle = expr(p, "angle").unwrap_or_else(|| "90 deg".into());
            check_expr(s, &angle, Kind::Angle, cmd, "angle")?;
            let radius = expr(p, "radius");
            if let Some(r) = &radius {
                check_expr(s, r, Kind::Length, cmd, "radius")?;
            }
            let position = str_(p, "position").unwrap_or("inside").to_ascii_lowercase();
            if !matches!(position.as_str(), "inside" | "outside" | "middle") {
                return Err(bad(cmd, "`position` must be inside, outside or middle"));
            }
            let body = str_(p, "body").map(str::to_string);
            add_feature(s, p, FeatureKind::SheetFlange { edges, height, angle, radius, position, flip: bool_(p, "flip").unwrap_or(false), body })
        }
        other => Err(bad(cmd, format!("unknown flange type `{other}` (base, edge or contour)"))),
    }
}

fn hem(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.hem";
    let edges = points(p, "edges", cmd)?;
    let length = req_expr(cmd, p, "length")?;
    check_expr(s, &length, Kind::Length, cmd, "length")?;
    let gap = expr(p, "gap");
    if let Some(g) = &gap {
        check_expr(s, g, Kind::Length, cmd, "gap")?;
    }
    add_feature(
        s,
        p,
        FeatureKind::SheetHem { edges, length, gap, flip: bool_(p, "flip").unwrap_or(false), body: str_(p, "body").map(str::to_string) },
    )
}

fn fold(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.fold";
    let (sketch, curve, a, b) = match p.get("points") {
        Some(v) => {
            let pts = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| bad(cmd, "`points` must be two points [x, y, z]"))?;
            let a = pts.first().and_then(vec3).ok_or_else(|| bad(cmd, "`points` must be two finite points [x, y, z]"))?;
            let b = pts.get(1).and_then(vec3).ok_or_else(|| bad(cmd, "`points` must be two finite points [x, y, z]"))?;
            (None, None, a, b)
        }
        None => {
            let sk = sketch_id(s, p.get("sketch"), cmd, "sketch")?;
            let c = str_(p, "curve").ok_or_else(|| bad(cmd, "give `sketch` + `curve` (a sketch line) or `points`"))?.to_string();
            let st = s.model.state();
            let ss = st.sketch(sk).ok_or_else(|| bad(cmd, "no such sketch"))?;
            let ci = ss.sketch.curve_index(&c).ok_or_else(|| bad(cmd, format!("no curve `{c}` in the sketch")))?;
            let (a, b) = match ss.sketch.segs(ci).as_slice() {
                [solvecraft_geom::Seg2::Line { a, b }] => (ss.plane.to_world(*a), ss.plane.to_world(*b)),
                _ => return Err(bad(cmd, "the fold line must be a sketch line")),
            };
            (Some(sk), Some(c), a, b)
        }
    };
    if a.dist(b) < 1e-9 {
        return Err(bad(cmd, "the fold line has no length"));
    }
    let angle = expr(p, "angle").unwrap_or_else(|| "90 deg".into());
    check_expr(s, &angle, Kind::Angle, cmd, "angle")?;
    let radius = expr(p, "radius");
    if let Some(r) = &radius {
        check_expr(s, r, Kind::Length, cmd, "radius")?;
    }
    let position = str_(p, "position").unwrap_or("centerline").to_ascii_lowercase();
    if !matches!(position.as_str(), "centerline" | "start" | "end" | "mould" | "mold") {
        return Err(bad(cmd, "`position` must be centerline, start, end or mould"));
    }
    let fixed = match p.get("fixed") {
        Some(v) => Some(vec3(v).ok_or_else(|| bad(cmd, "`fixed` must be a point [x, y, z]"))?),
        None => None,
    };
    let body = str_(p, "body").map(str::to_string);
    add_feature(s, p, FeatureKind::SheetFold { sketch, curve, a, b, angle, radius, position, flip: bool_(p, "flip").unwrap_or(false), fixed, body })
}

fn unfold(s: &mut Session, p: &Value) -> Result<Value> {
    add_feature(s, p, FeatureKind::SheetUnfold { body: str_(p, "body").map(str::to_string), refold: false })
}

fn refold(s: &mut Session, p: &Value) -> Result<Value> {
    add_feature(s, p, FeatureKind::SheetUnfold { body: str_(p, "body").map(str::to_string), refold: true })
}

fn convert(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.convert";
    let body = str_(p, "body").ok_or_else(|| bad(cmd, "`body` is required"))?.to_string();
    if s.model.state().body(&body).is_none() {
        return Err(bad(cmd, format!("no body `{body}`")));
    }
    let face = p.get("face").and_then(vec3).ok_or_else(|| bad(cmd, "`face` must be a point [x, y, z] on the plate's face"))?;
    let rule = rule_param(s, p, cmd)?;
    add_feature(s, p, FeatureKind::SheetConvert { body, face, rule })
}

fn rules(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.manage_rules";
    let name = str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` is required"))?.to_string();
    if bool_(p, "delete").unwrap_or(false) {
        let doc = s.doc_mut();
        let before = doc.sheet.rules.len();
        doc.sheet.rules.retain(|r| r.name != name);
        if doc.sheet.rules.len() == before {
            return Err(bad(cmd, format!("no rule `{name}`")));
        }
        if doc.sheet.active.as_deref() == Some(name.as_str()) {
            doc.sheet.active = None;
        }
        return Ok(json!({"deleted": name}));
    }
    let mut rule =
        s.doc.sheet.rules.iter().find(|r| r.name == name).cloned().unwrap_or_else(|| SheetRule { name: name.clone(), ..s.doc.sheet_rule(None) });
    for (k, slot) in [
        ("thickness", &mut rule.thickness),
        ("k_factor", &mut rule.k_factor),
        ("bend_radius", &mut rule.bend_radius),
        ("relief_width", &mut rule.relief_width),
        ("relief_depth", &mut rule.relief_depth),
        ("corner_relief", &mut rule.corner_relief),
        ("hem_gap", &mut rule.hem_gap),
        ("gap", &mut rule.gap),
    ] {
        if let Some(e) = expr(p, k) {
            *slot = e;
        }
    }
    if let Some(shape) = str_(p, "corner_relief_shape") {
        let s = shape.to_ascii_lowercase();
        if !matches!(s.as_str(), "square" | "round" | "laser_weld") {
            return Err(bad(cmd, "`corner_relief_shape` must be square, round or laser_weld"));
        }
        rule.corner_relief_shape = s;
    }
    let (vals, _) = s.doc.param_values();
    let values = s.doc.rule_values(&vals, &rule).map_err(|e| bad(cmd, e.to_string()))?;
    let doc = s.doc_mut();
    match doc.sheet.rules.iter_mut().find(|r| r.name == name) {
        Some(r) => *r = rule.clone(),
        None => {
            if doc.sheet.rules.len() >= 1000 {
                return Err(EngineError::Other("too many rules".into()));
            }
            doc.sheet.rules.push(rule.clone());
        }
    }
    if bool_(p, "active").unwrap_or(doc.sheet.active.is_none()) {
        doc.sheet.active = Some(name.clone());
    }
    s.refresh();
    let errors: Vec<Value> = s.model.results.iter().filter_map(|r| r.error.as_ref().map(|e| json!({"feature": r.name, "error": e}))).collect();
    Ok(json!({"rule": rule, "values": values, "errors": errors}))
}

fn list_rules(s: &mut Session, _p: &Value) -> Result<Value> {
    let (vals, _) = s.doc.param_values();
    let mut all = s.doc.sheet.rules.clone();
    if all.is_empty() {
        all.push(SheetRule::steel());
    }
    let rules: Vec<Value> = all.iter().map(|r| json!({"rule": r, "values": s.doc.rule_values(&vals, r).ok()})).collect();
    let active = s.doc.sheet_rule(None).name;
    Ok(json!({"rules": rules, "active": active}))
}

fn sheet_of(s: &Session, p: &Value, cmd: &str) -> Result<SheetBody> {
    let st = s.model.state();
    match str_(p, "body") {
        Some(b) => st.sheets.iter().find(|x| x.body == b).cloned().ok_or_else(|| bad(cmd, format!("`{b}` is not a sheet metal body"))),
        None => st.sheets.last().cloned().ok_or_else(|| bad(cmd, "there is no sheet metal body")),
    }
}

fn flat_json(sh: &SheetBody) -> Result<Value> {
    let regions = sh.flat_regions()?;
    let loops: Vec<Vec<Vec2>> = regions.iter().map(|r| r.outer.polyline(1e-3)).collect();
    let (mut lo, mut hi) = (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN));
    for p in loops.iter().flatten() {
        lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    let (w, h) = (hi.x - lo.x, hi.y - lo.y);
    let pt = |v: &Vec2| json!([v.x, v.y]);
    let outline: Vec<Value> = loops.iter().map(|l| Value::Array(l.iter().map(pt).collect())).collect();
    let holes: Vec<Value> = regions.iter().flat_map(|r| &r.holes).map(|h| Value::Array(h.polyline(1e-3).iter().map(pt).collect())).collect();
    let bends: Vec<Value> = sh
        .bends()
        .iter()
        .map(|b| {
            json!({"bend": b.index, "angle_deg": b.angle_deg, "radius": b.radius, "direction": if b.up { "up" } else { "down" },
                "allowance": b.allowance, "length": b.length, "start": pt(&b.start), "end": pt(&b.end)})
        })
        .collect();
    let pockets: Vec<Value> = sh
        .machining()?
        .iter()
        .map(|p| {
            let side = if p.z0 == 0.0 {
                "bottom"
            } else if p.z1 == sh.t {
                "top"
            } else {
                "internal"
            };
            json!({"outline": p.region.outer.polyline(1e-3).iter().map(pt).collect::<Vec<_>>(),
            "cutouts": p.region.holes.iter().map(|h| h.polyline(1e-3).iter().map(pt).collect::<Vec<_>>()).collect::<Vec<_>>(),
            "side": side, "depth_mm": p.z1-p.z0, "z0_mm": p.z0, "z1_mm": p.z1})
        })
        .collect();
    Ok(
        json!({"body": sh.body, "rule": sh.rule, "thickness": sh.t, "k_factor": sh.k, "flat_size_mm": [w, h, sh.t], "outline": outline, "cutouts": holes, "pockets": pockets, "bends": bends, "unfolded": sh.flat}),
    )
}

fn flat_pattern(s: &mut Session, p: &Value) -> Result<Value> {
    let sh = sheet_of(s, p, "sheet.flat_pattern")?;
    flat_json(&sh)
}

/// A DXF (R12 entities, mm) of the flat pattern on named layers.
pub fn flat_dxf(sh: &SheetBody) -> Result<String> {
    let regions = sh.flat_regions()?;
    let pockets = sh.machining()?;
    let pocket_layer = |p: &solvecraft_doc::sheet::SheetPocket| {
        let side = if p.z0 == 0.0 {
            "BOTTOM"
        } else if p.z1 == sh.t {
            "TOP"
        } else {
            "INTERNAL"
        };
        format!("POCKET_{side}_Z{:.6}_TO_{:.6}_MM", p.z0, p.z1)
    };
    let mut layers = std::collections::BTreeSet::from(["OUTLINE".to_string(), "BEND_UP".to_string(), "BEND_DOWN".to_string()]);
    for p in &pockets {
        layers.insert(pocket_layer(p));
    }
    let mut out = String::new();
    let mut w = |c: i32, v: &str| out.push_str(&format!("{c}\n{v}\n"));
    w(0, "SECTION");
    w(2, "HEADER");
    w(9, "$INSUNITS");
    w(70, "4");
    w(0, "ENDSEC");
    w(0, "SECTION");
    w(2, "TABLES");
    w(0, "TABLE");
    w(2, "LAYER");
    w(70, &layers.len().to_string());
    for name in &layers {
        let colour = match name.as_str() {
            "OUTLINE" => "7",
            "BEND_UP" => "1",
            "BEND_DOWN" => "5",
            _ => "3",
        };
        w(0, "LAYER");
        w(2, name);
        w(70, "0");
        w(62, colour);
        w(6, "CONTINUOUS");
    }
    w(0, "ENDTAB");
    w(0, "ENDSEC");
    w(0, "SECTION");
    w(2, "ENTITIES");
    let f = |x: f64| format!("{x:.6}");
    let line = |w: &mut dyn FnMut(i32, &str), layer: &str, a: Vec2, b: Vec2| {
        w(0, "LINE");
        w(8, layer);
        w(10, &f(a.x));
        w(20, &f(a.y));
        w(11, &f(b.x));
        w(21, &f(b.y));
    };
    let mut loops: Vec<_> = regions.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).map(|lp| (lp, "OUTLINE".to_string())).collect();
    for p in &pockets {
        for lp in std::iter::once(&p.region.outer).chain(&p.region.holes) {
            loops.push((lp, pocket_layer(p)));
        }
    }
    for (h, layer) in loops {
        // A circle is often represented by two semicircles. Keep the whole analytic
        // boundary as one CIRCLE rather than approximating outer boundaries with chords.
        if let Some(solvecraft_geom::Seg2::Arc { center, radius, sweep, .. }) = h.segs.first() {
            let total: f64 = h.segs.iter().filter_map(|s| if let solvecraft_geom::Seg2::Arc { sweep, .. } = s { Some(*sweep) } else { None }).sum();
            let circular = h.segs.iter().all(|s| matches!(s, solvecraft_geom::Seg2::Arc { center: c, radius: r, sweep: a, .. } if c.dist(*center) < 1e-9 && (*r-*radius).abs() < 1e-9 && a.signum() == sweep.signum()));
            if circular && (total.abs() - std::f64::consts::TAU).abs() < 1e-9 {
                w(0, "CIRCLE");
                w(8, &layer);
                w(10, &f(center.x));
                w(20, &f(center.y));
                w(40, &f(*radius));
                continue;
            }
        }
        for seg in &h.segs {
            match *seg {
                solvecraft_geom::Seg2::Arc { center, radius, start, sweep } => {
                    if (sweep.abs() - std::f64::consts::TAU).abs() < 1e-9 {
                        w(0, "CIRCLE");
                        w(8, &layer);
                        w(10, &f(center.x));
                        w(20, &f(center.y));
                        w(40, &f(radius));
                        continue;
                    }
                    let (a0, a1) = if sweep >= 0.0 { (start, start + sweep) } else { (start + sweep, start) };
                    w(0, "ARC");
                    w(8, &layer);
                    w(10, &f(center.x));
                    w(20, &f(center.y));
                    w(40, &f(radius));
                    w(50, &f(a0.to_degrees().rem_euclid(360.0)));
                    w(51, &f(a1.to_degrees().rem_euclid(360.0)));
                }
                other => {
                    let pts = other.polyline(1e-3);
                    for p in pts.windows(2) {
                        line(&mut w, &layer, p[0], p[1]);
                    }
                }
            }
        }
    }
    for b in sh.bends() {
        line(&mut w, if b.up { "BEND_UP" } else { "BEND_DOWN" }, b.start, b.end);
    }
    w(0, "ENDSEC");
    w(0, "EOF");
    Ok(out)
}

fn export_dxf(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.export_dxf";
    let sh = sheet_of(s, p, cmd)?;
    let text = flat_dxf(&sh)?;
    if let Some(path) = str_(p, "path").filter(|x| !x.trim().is_empty()) {
        solvecraft_io::vfs::write(path, text.as_bytes()).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        return Ok(json!({"path": path, "bends": sh.bends().len()}));
    }
    Ok(json!({"dxf": text, "bends": sh.bends().len()}))
}

fn export_bend_table(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sheet.export_bend_table";
    let format = str_(p, "format").unwrap_or("csv").to_ascii_lowercase();
    if !matches!(format.as_str(), "csv" | "json") {
        return Err(bad(cmd, "`format` must be csv or json"));
    }
    let sh = sheet_of(s, p, cmd)?;
    let table = flat_json(&sh)?;
    let text = if format == "json" {
        serde_json::to_string_pretty(&table).map_err(|e| EngineError::Other(e.to_string()))?
    } else {
        // Always quote text fields: body/rule names may include commas, quotes or newlines.
        let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let mut csv =
            "body,rule,thickness_mm,k_factor,bend,angle_deg,direction,radius_mm,allowance_mm,length_mm,start_x_mm,start_y_mm,end_x_mm,end_y_mm\r\n"
                .to_string();
        for b in sh.bends() {
            csv.push_str(&format!(
                "{},{},{:.9},{:.9},{},{:.9},{},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9}\r\n",
                quote(&sh.body),
                quote(&sh.rule),
                sh.t,
                sh.k,
                b.index,
                b.angle_deg,
                if b.up { "up" } else { "down" },
                b.radius,
                b.allowance,
                b.length,
                b.start.x,
                b.start.y,
                b.end.x,
                b.end.y
            ));
        }
        csv
    };
    if let Some(path) = str_(p, "path").filter(|x| !x.trim().is_empty()) {
        solvecraft_io::vfs::write(path, text.as_bytes()).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        return Ok(json!({"path": path, "format": format, "bends": sh.bends().len()}));
    }
    Ok(json!({"format": format, "text": text, "bends": sh.bends().len()}))
}

#[cfg(test)]
#[path = "sheet_tests.rs"]
mod tests;

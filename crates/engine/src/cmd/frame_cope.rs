//! Commands for retained tube copes and their fabrication templates.

use serde_json::{Value, json};
use solidcraft_doc::frame_cope::{End, MAX_CUTTERS, MAX_TEMPLATE_SEGMENTS};
use solidcraft_doc::{BodyName, FeatureKind};
use solidcraft_geom::Vec2;
use solidcraft_io::{DxfEntity, DxfLayer, DxfShape};

use crate::params::{bad, expr, str_, string_list};
use crate::schema::frame::CopeDxfResult;
use crate::{Result, Session};

fn end(cmd: &str, p: &Value) -> Result<End> {
    match str_(p, "end") {
        Some("start") => Ok(End::Start),
        Some("end") => Ok(End::End),
        _ => Err(bad(cmd, "`end` must be start or end")),
    }
}

pub(super) fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.cope";
    let member = str_(p, "member").filter(|n| !n.is_empty() && n.len() <= 1024).ok_or_else(|| bad(cmd, "`member` must name a round-tube member"))?;
    let a = p
        .get("tools")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty() && a.len() <= MAX_CUTTERS)
        .ok_or_else(|| bad(cmd, "`tools` must list 1..16 mating members"))?;
    if a.iter().any(|v| v.as_str().is_none_or(|n| n.is_empty() || n.len() > 1024)) {
        return Err(bad(cmd, "invalid mating member name"));
    }
    let tools = string_list(cmd, p, "tools")?.into_iter().map(BodyName).collect();
    super::add_feature(s, p, FeatureKind::FrameCope { member: member.into(), tools, end: end(cmd, p)? })
}

pub(super) fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "frame.export_cope_dxf";
    let member = str_(p, "member").ok_or_else(|| bad(cmd, "`member` is required"))?;
    let end = end(cmd, p)?;
    let seam = match p.get("seam") {
        None | Some(Value::Null) => 0.0,
        Some(Value::Number(v)) => v.as_f64().ok_or_else(|| bad(cmd, "invalid seam angle"))?,
        Some(_) => {
            let e = expr(p, "seam").ok_or_else(|| bad(cmd, "invalid seam angle"))?;
            s.doc.eval(&e, solidcraft_doc::expr::Kind::Angle)?
        }
    };
    let tolerance = match p.get("tolerance") {
        None | Some(Value::Null) => 0.01,
        Some(v) => v.as_f64().ok_or_else(|| bad(cmd, "invalid tolerance"))?,
    };
    let path = match p.get("path") {
        None | Some(Value::Null) => None,
        Some(_) => Some(str_(p, "path").filter(|p| !p.trim().is_empty() && p.len() < 4096).ok_or_else(|| bad(cmd, "invalid path"))?),
    };
    if let Some(path) = path {
        solidcraft_io::vfs::check(path).map_err(|e| bad(cmd, e.to_string()))?;
    }
    let state = s.model.state();
    let mb = state.body(member).ok_or_else(|| bad(cmd, format!("unknown member `{member}`")))?;
    if s.doc.body_offsets.contains_key(member) {
        return Err(bad(cmd, "not supported yet: export of an offset member"));
    }
    let data = mb.round_member()?;
    let points = data.template(end, seam, tolerance)?;
    let terminal = data.terminal(end)?;
    let circumference = std::f64::consts::TAU * data.radius;
    let height = points.iter().map(|p| p.y).fold(0.0_f64, f64::max);
    let mut entities = vec![DxfEntity { layer: "CUT_PROFILE".into(), shape: DxfShape::Polyline { points: points.clone() } }];
    let line = |layer: &str, a, b| DxfEntity { layer: layer.into(), shape: DxfShape::Line(a, b) };
    entities.extend([
        line("DATUM", Vec2::ZERO, Vec2::new(circumference, 0.0)),
        line("SEAM", Vec2::ZERO, Vec2::new(0.0, height + 5.0)),
        line("SEAM", Vec2::new(circumference, 0.0), Vec2::new(circumference, height + 5.0)),
        line("SCALE_CHECK", Vec2::new(0.0, -12.0), Vec2::new(100.0, -12.0)),
        line("SCALE_CHECK", Vec2::new(0.0, -14.0), Vec2::new(0.0, -10.0)),
        line("SCALE_CHECK", Vec2::new(100.0, -14.0), Vec2::new(100.0, -10.0)),
    ]);
    let vector = |p: solidcraft_geom::Vec3| [p.x, p.y, p.z];
    let coordinates = |p: solidcraft_geom::Vec3| format!("({:.6}, {:.6}, {:.6})", p.x, p.y, p.z);
    let seam_direction = terminal.x * seam.cos() + terminal.y * seam.sin();
    let labels = [
        format!("SolidCraft | {member} | {} end | mm | 1:1 | OD {:.6} | circumference {:.6} mm", end.label(), data.radius * 2.0, circumference),
        format!("Datum: component {} original endpoint {}; +Y inward {}", mb.component, coordinates(terminal.origin), coordinates(terminal.axis)),
        format!("Seam {:.9} rad; radial {}; +X right-hand wrap about inward axis", seam, coordinates(seam_direction)),
        format!("CUT_PROFILE: keep +Y side; interpolation error <= {tolerance} mm; no kerf allowance"),
        "Scale check: 100 mm between end ticks. Print at 100%; disable fit-to-page.".into(),
    ];
    for (i, label) in labels.into_iter().enumerate() {
        entities.push(DxfEntity {
            layer: "NOTES".into(),
            shape: DxfShape::Text { at: Vec2::new(0.0, -20.0 - i as f64 * 5.0), height: 2.5, angle_deg: 0.0, text: label },
        });
    }
    let layers = [
        DxfLayer::new("CUT_PROFILE", 7),
        DxfLayer::new("DATUM", 8),
        DxfLayer::new("SEAM", 3),
        DxfLayer::new("SCALE_CHECK", 5),
        DxfLayer::new("NOTES", 8),
    ];
    let dxf = solidcraft_io::write_dxf_drawing(&layers, &entities)?;
    if solidcraft_doc::kernel::watch::current().is_some_and(|w| w.cancelled()) {
        return Err(bad(cmd, "cope template cancelled"));
    }
    let mut result = CopeDxfResult {
        member: member.into(),
        end: end.label().into(),
        units: "mm".into(),
        scale: 1.0,
        outer_diameter: data.radius * 2.0,
        circumference,
        seam,
        origin: vector(terminal.origin),
        component: mb.component,
        axis: vector(terminal.axis),
        seam_direction: vector(seam_direction),
        wrap_direction: "right_hand_about_inward_axis".into(),
        tolerance,
        segments: points.len().saturating_sub(1),
        max_segments: MAX_TEMPLATE_SEGMENTS,
        profile: points.iter().map(|p| [p.x, p.y]).collect(),
        dxf: None,
        path: path.map(str::to_owned),
    };
    if let Some(path) = path {
        solidcraft_io::vfs::check(path).map_err(|e| bad(cmd, e.to_string()))?;
        solidcraft_io::write_atomic(std::path::Path::new(path), dxf.as_bytes(), false).map_err(|e| bad(cmd, e.to_string()))?;
    } else {
        result.dxf = Some(dxf);
    }
    Ok(json!(result))
}

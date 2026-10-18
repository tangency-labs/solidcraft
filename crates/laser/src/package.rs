//! LightBurn handoff package: ASCII DXF (+ optional SVG) and `laser-job.json`.

use serde::{Deserialize, Serialize};

use crate::profile::{LaserProfile, ProfileId, profile};
use crate::{Result, invalid};

/// One open or closed polyline in millimetres (field XY).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Polyline2 {
    pub points: Vec<[f64; 2]>,
    /// When true, close back to the first point in DXF/SVG.
    #[serde(default)]
    pub closed: bool,
    /// Layer name (also used as DXF layer).
    #[serde(default = "default_layer")]
    pub layer: String,
}

fn default_layer() -> String {
    "mark".into()
}

/// Layer intent for operators / future companions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaserLayer {
    Mark,
    Engrave,
    Deep,
}

impl LaserLayer {
    pub fn as_str(self) -> &'static str {
        match self {
            LaserLayer::Mark => "mark",
            LaserLayer::Engrave => "engrave",
            LaserLayer::Deep => "deep",
        }
    }
}

/// Sidecar metadata written as `laser-job.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LaserJobMeta {
    pub profile: String,
    pub profile_name: String,
    pub bed_width_mm: f64,
    pub bed_height_mm: f64,
    /// Largest bed axis (compat for older callers that used a single `field_mm`).
    pub field_mm: f64,
    pub lightburn_device: String,
    pub lightburn_model: String,
    pub source: String,
    pub units: &'static str,
    pub layers: Vec<LayerMeta>,
    pub notes: String,
    pub generator: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerMeta {
    pub name: String,
    pub intent: String,
    pub polylines: usize,
}

/// Files produced for LightBurn import.
#[derive(Clone, Debug, PartialEq)]
pub struct LaserPackage {
    pub dxf: String,
    pub svg: String,
    pub job_json: String,
    pub meta: LaserJobMeta,
}

/// Build a package from polylines.
///
/// `field_mm` when set forces a **square** work area of that size (lens override).
/// Otherwise the profile's published `bed_width_mm` × `bed_height_mm` are used.
pub fn build_package(profile_id: ProfileId, field_mm: Option<f64>, polylines: &[Polyline2]) -> Result<LaserPackage> {
    let pref = profile(profile_id);
    let (width, height) = match field_mm {
        Some(f) => {
            if !(f.is_finite() && f > 0.0) {
                return Err(invalid("field_mm must be a positive finite number"));
            }
            (f, f)
        }
        None => (pref.bed_width_mm, pref.bed_height_mm),
    };
    if !(width.is_finite() && width > 0.0 && height.is_finite() && height > 0.0) {
        return Err(invalid("bed size must be positive finite numbers"));
    }
    if polylines.is_empty() {
        return Err(invalid("no geometry to export"));
    }
    for (i, pl) in polylines.iter().enumerate() {
        if pl.points.len() < 2 {
            return Err(invalid(format!("polyline {i} needs at least two points")));
        }
        for p in &pl.points {
            if !p[0].is_finite() || !p[1].is_finite() {
                return Err(invalid(format!("polyline {i} has a non-finite point")));
            }
        }
    }

    let dxf = write_ascii_dxf(polylines, width, height)?;
    let svg = write_svg(polylines, width, height)?;
    let meta = job_meta(pref, width, height, polylines);
    let job_json = serde_json::to_string_pretty(&meta).map_err(|e| invalid(e.to_string()))?;
    Ok(LaserPackage { dxf, svg, job_json, meta })
}

fn job_meta(pref: &LaserProfile, width: f64, height: f64, polylines: &[Polyline2]) -> LaserJobMeta {
    let mut layers: Vec<(String, usize)> = Vec::new();
    for pl in polylines {
        if let Some((_, n)) = layers.iter_mut().find(|(n, _)| n == &pl.layer) {
            *n += 1;
        } else {
            layers.push((pl.layer.clone(), 1));
        }
    }
    LaserJobMeta {
        profile: pref.id.as_str().into(),
        profile_name: pref.name.clone(),
        bed_width_mm: width,
        bed_height_mm: height,
        field_mm: width.max(height),
        lightburn_device: pref.lightburn_device.clone(),
        lightburn_model: pref.lightburn_model.clone(),
        source: pref.source.clone(),
        units: "mm",
        layers: layers.into_iter().map(|(name, polylines)| LayerMeta { intent: intent_for_layer(&name).as_str().into(), name, polylines }).collect(),
        notes: pref.notes.clone(),
        generator: "solidcraft-laser",
    }
}

fn intent_for_layer(name: &str) -> LaserLayer {
    match name.to_ascii_lowercase().as_str() {
        "engrave" | "engraving" => LaserLayer::Engrave,
        "deep" | "cut" => LaserLayer::Deep,
        _ => LaserLayer::Mark,
    }
}

/// Minimal AutoCAD R12 ASCII DXF (LINE entities), millimetres, centred on the bed.
pub fn write_ascii_dxf(polylines: &[Polyline2], width_mm: f64, height_mm: f64) -> Result<String> {
    let hx = width_mm * 0.5;
    let hy = height_mm * 0.5;
    let mut layers: Vec<String> = Vec::new();
    for pl in polylines {
        if !layers.iter().any(|l| l == &pl.layer) {
            layers.push(pl.layer.clone());
        }
    }
    let mut out = String::new();
    out.push_str("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n9\n$INSUNITS\n70\n4\n");
    out.push_str("9\n$EXTMIN\n10\n");
    out.push_str(&format!("{:.6}\n20\n{:.6}\n", -hx, -hy));
    out.push_str("9\n$EXTMAX\n10\n");
    out.push_str(&format!("{:.6}\n20\n{:.6}\n", hx, hy));
    out.push_str("0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n70\n");
    out.push_str(&format!("{}\n", layers.len().max(1)));
    for (i, name) in layers.iter().enumerate() {
        let color = match i % 6 {
            0 => 1,
            1 => 3,
            2 => 5,
            3 => 4,
            4 => 6,
            _ => 7,
        };
        out.push_str("0\nLAYER\n2\n");
        out.push_str(name);
        out.push_str(&format!("\n70\n0\n62\n{color}\n6\nCONTINUOUS\n"));
    }
    out.push_str("0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
    for pl in polylines {
        let n = pl.points.len();
        let segs = if pl.closed { n } else { n.saturating_sub(1) };
        for i in 0..segs {
            let a = pl.points[i];
            let b = pl.points[(i + 1) % n];
            out.push_str("0\nLINE\n8\n");
            out.push_str(&pl.layer);
            out.push_str(&format!("\n10\n{:.6}\n20\n{:.6}\n30\n0.0\n11\n{:.6}\n21\n{:.6}\n31\n0.0\n", a[0], a[1], b[0], b[1]));
        }
    }
    out.push_str("0\nENDSEC\n0\nEOF\n");
    Ok(out)
}

/// Simple SVG viewBox matching the bed (mm), origin at centre.
pub fn write_svg(polylines: &[Polyline2], width_mm: f64, height_mm: f64) -> Result<String> {
    let hx = width_mm * 0.5;
    let hy = height_mm * 0.5;
    let mut out = String::new();
    out.push_str(&format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{width_mm}mm" height="{height_mm}mm" viewBox="{vx} {vy} {width_mm} {height_mm}" fill="none" stroke="black" stroke-width="0.1">
"#,
        vx = -hx,
        vy = -hy,
    ));
    for pl in polylines {
        if pl.points.is_empty() {
            continue;
        }
        out.push_str(r#"<path data-layer=""#);
        out.push_str(&pl.layer);
        out.push_str(r#"" d="M "#);
        for (i, p) in pl.points.iter().enumerate() {
            if i > 0 {
                out.push_str(" L ");
            }
            // SVG Y is down; CAD Y is up — flip for screen preview; LightBurn DXF is the
            // authoritative import. Keep SVG Y as CAD Y so import tools that read SVG mm
            // match DXF if they treat +Y up.
            out.push_str(&format!("{:.6} {:.6}", p[0], p[1]));
        }
        if pl.closed {
            out.push_str(" Z");
        }
        out.push_str("\" />\n");
    }
    out.push_str("</svg>\n");
    Ok(out)
}

/// Write `{stem}.dxf`, `{stem}.svg`, and `{stem}.laser-job.json` into `dir` conceptually —
/// returns the three filenames (caller writes bytes via VFS).
pub fn package_filenames(stem: &str) -> Result<(String, String, String)> {
    let stem = stem.trim();
    if stem.is_empty() {
        return Err(invalid("empty package stem"));
    }
    if stem.contains('/') || stem.contains('\\') || stem.contains("..") {
        return Err(invalid("package stem must be a bare file name"));
    }
    Ok((format!("{stem}.dxf"), format!("{stem}.svg"), format!("{stem}.laser-job.json")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::ProfileId;

    fn square() -> Vec<Polyline2> {
        vec![Polyline2 { points: vec![[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]], closed: true, layer: "mark".into() }]
    }

    #[test]
    fn package_contains_dxf_entities_and_job_json() {
        let pkg = build_package(ProfileId::Commarker60wMopa, Some(110.0), &square()).unwrap();
        assert!(pkg.dxf.contains("LINE"));
        assert!(pkg.dxf.contains("AC1009"));
        assert!(pkg.svg.contains("<svg"));
        let v: serde_json::Value = serde_json::from_str(&pkg.job_json).unwrap();
        assert_eq!(v["profile"], "commarker-60w-mopa");
        assert_eq!(v["bed_width_mm"], 110.0);
        assert_eq!(v["bed_height_mm"], 110.0);
        assert_eq!(v["lightburn_device"], "JCZFiber");
        assert_eq!(v["lightburn_model"], "B6 MOPA");
    }

    #[test]
    fn xtool_d1_pro_uses_published_bed() {
        let pkg = build_package(ProfileId::XtoolD1Pro, None, &square()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&pkg.job_json).unwrap();
        assert_eq!(v["bed_width_mm"], 432.0);
        assert_eq!(v["bed_height_mm"], 406.0);
        assert_eq!(v["lightburn_model"], "D1 Pro");
    }

    #[test]
    fn empty_geometry_fails() {
        assert!(build_package(ProfileId::Commarker60wMopa, None, &[]).is_err());
    }

    #[test]
    fn golden_dxf_has_four_sides_for_closed_square() {
        let dxf = write_ascii_dxf(&square(), 110.0, 110.0).unwrap();
        let lines = dxf.matches("\nLINE\n").count();
        assert_eq!(lines, 4);
    }
}

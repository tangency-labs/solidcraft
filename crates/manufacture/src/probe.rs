//! Renishaw Inspection Plus–style probe cycle snippets for operator review.
//!
//! Emits template G-code (Haas / Fanuc macro numbers). Confirm stylus calib, work offset
//! (`S` → G54–G59), feeds, and safe heights on the machine before running. Not a live
//! probe driver.

use crate::{GcodeDialect, ProbeMacroStyle, Tool};

/// Built-in probe cycle flavors (Inspection Plus O98xx family).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeCycle {
    /// Protected positioning move (O9810).
    ProtectedMove,
    /// Single surface measure / set offset (O9811).
    SingleSurface,
    /// Web / pocket measure (O9812).
    WebPocket,
    /// Bore / boss measure (O9814).
    BoreBoss,
    /// Internal corner (O9815).
    InternalCorner,
    /// External corner (O9816).
    ExternalCorner,
}

impl ProbeCycle {
    pub fn as_str(self) -> &'static str {
        match self {
            ProbeCycle::ProtectedMove => "protected_move",
            ProbeCycle::SingleSurface => "single_surface",
            ProbeCycle::WebPocket => "web_pocket",
            ProbeCycle::BoreBoss => "bore_boss",
            ProbeCycle::InternalCorner => "internal_corner",
            ProbeCycle::ExternalCorner => "external_corner",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "protected_move" | "protected" | "9810" => Some(ProbeCycle::ProtectedMove),
            "single_surface" | "surface" | "9811" => Some(ProbeCycle::SingleSurface),
            "web_pocket" | "web" | "pocket" | "9812" => Some(ProbeCycle::WebPocket),
            "bore_boss" | "bore" | "boss" | "9814" => Some(ProbeCycle::BoreBoss),
            "internal_corner" | "inside_corner" | "9815" => Some(ProbeCycle::InternalCorner),
            "external_corner" | "outside_corner" | "9816" => Some(ProbeCycle::ExternalCorner),
            _ => None,
        }
    }

    pub fn all() -> &'static [ProbeCycle] {
        &[
            ProbeCycle::ProtectedMove,
            ProbeCycle::SingleSurface,
            ProbeCycle::WebPocket,
            ProbeCycle::BoreBoss,
            ProbeCycle::InternalCorner,
            ProbeCycle::ExternalCorner,
        ]
    }

    fn macro_number(self) -> u16 {
        match self {
            ProbeCycle::ProtectedMove => 9810,
            ProbeCycle::SingleSurface => 9811,
            ProbeCycle::WebPocket => 9812,
            ProbeCycle::BoreBoss => 9814,
            ProbeCycle::InternalCorner => 9815,
            ProbeCycle::ExternalCorner => 9816,
        }
    }
}

/// Parameters for a review probe snippet (millimetres).
#[derive(Clone, Debug, PartialEq)]
pub struct ProbeSnippetRequest {
    pub cycle: ProbeCycle,
    pub style: ProbeMacroStyle,
    pub probe_tool: Tool,
    /// Nominal feature position / size used in the macro call.
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// Clearance Z before/after the cycle.
    pub clearance_z: f64,
    /// Protected-move feed (mm/min).
    pub protected_feed: f64,
    /// Work offset selector for Inspection Plus `S` (1 = G54 … 6 = G59). 0 = omit S.
    pub work_offset_s: u8,
    /// Optional overtravel `Q` (mm); 0 = omit.
    pub overtravel_q: f64,
    /// Nominal web width / bore diameter `D` where the cycle needs it; 0 = omit.
    pub size_d: f64,
}

impl ProbeSnippetRequest {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.style, ProbeMacroStyle::HaasInspectionPlus | ProbeMacroStyle::FanucInspectionPlus) {
            return Err("probe style must be haas_inspection_plus or fanuc_inspection_plus".into());
        }
        if self.probe_tool.number == 0 {
            return Err("probe tool number must be ≥ 1".into());
        }
        for (name, v) in [
            ("x", self.x),
            ("y", self.y),
            ("z", self.z),
            ("clearance_z", self.clearance_z),
            ("protected_feed", self.protected_feed),
            ("overtravel_q", self.overtravel_q),
            ("size_d", self.size_d),
        ] {
            if !v.is_finite() {
                return Err(format!("{name} must be finite"));
            }
        }
        if !(self.protected_feed > 0.0) {
            return Err("protected_feed must be > 0".into());
        }
        if self.work_offset_s > 6 {
            return Err("work_offset_s must be 0–6 (1=G54 … 6=G59)".into());
        }
        if self.overtravel_q < 0.0 {
            return Err("overtravel_q must be ≥ 0".into());
        }
        if self.size_d < 0.0 {
            return Err("size_d must be ≥ 0".into());
        }
        Ok(())
    }
}

/// Map a G-code dialect to the matching Inspection Plus style when possible.
pub fn style_for_dialect(d: GcodeDialect) -> Option<ProbeMacroStyle> {
    match d {
        GcodeDialect::Haas => Some(ProbeMacroStyle::HaasInspectionPlus),
        GcodeDialect::Fanuc => Some(ProbeMacroStyle::FanucInspectionPlus),
        _ => None,
    }
}

/// Emit a review G-code snippet for the requested cycle.
pub fn emit_snippet(req: &ProbeSnippetRequest) -> Result<String, String> {
    req.validate()?;
    let eol = "\n";
    let t = req.probe_tool.number;
    let mut out = String::new();
    out.push_str(&format!("(SolidCraft probe review — {cycle}; confirm macros on machine){eol}", cycle = req.cycle.as_str()));
    out.push_str(&format!("(Style: {:?}; source: Renishaw Inspection Plus O98xx / Haas P9832-P9833){eol}", req.style));
    out.push_str(&format!("T{t} M6{eol}"));
    out.push_str(&format!("G90 G54{eol}"));
    out.push_str(&format!("G0 X{x:.4} Y{y:.4}{eol}", x = req.x, y = req.y));
    out.push_str(&format!("G43 H{t} Z{cz:.4}{eol}", cz = req.clearance_z));
    out.push_str(&format!("G65 P9832{eol}")); // probe on
    out.push_str(&format!("G65 P9810 Z{z:.4} F{f:.1}{eol}", z = req.clearance_z.min(req.z + req.overtravel_q.max(2.0)), f = req.protected_feed));

    let s_arg = if req.work_offset_s > 0 { format!(" S{}", req.work_offset_s) } else { String::new() };
    let q_arg = if req.overtravel_q > 0.0 { format!(" Q{:.4}", req.overtravel_q) } else { String::new() };
    let d_arg = if req.size_d > 0.0 { format!(" D{:.4}", req.size_d) } else { String::new() };

    match req.cycle {
        ProbeCycle::ProtectedMove => {
            out.push_str(&format!("G65 P9810 X{x:.4} Y{y:.4} Z{z:.4} F{f:.1}{eol}", x = req.x, y = req.y, z = req.z, f = req.protected_feed));
        }
        ProbeCycle::SingleSurface => {
            out.push_str(&format!("G65 P{m} Z{z:.4}{q}{s}{eol}", m = req.cycle.macro_number(), z = req.z, q = q_arg, s = s_arg));
        }
        ProbeCycle::WebPocket => {
            out.push_str(&format!("G65 P{m} X{x:.4}{d}{q}{s}{eol}", m = req.cycle.macro_number(), x = req.x, d = d_arg, q = q_arg, s = s_arg));
        }
        ProbeCycle::BoreBoss => {
            out.push_str(&format!(
                "G65 P{m} D{d:.4} Z{z:.4}{q}{s}{eol}",
                m = req.cycle.macro_number(),
                d = if req.size_d > 0.0 { req.size_d } else { 10.0 },
                z = req.z,
                q = q_arg,
                s = s_arg
            ));
        }
        ProbeCycle::InternalCorner | ProbeCycle::ExternalCorner => {
            out.push_str(&format!("G65 P{m} X{x:.4} Y{y:.4}{q}{s}{eol}", m = req.cycle.macro_number(), x = req.x, y = req.y, q = q_arg, s = s_arg));
        }
    }

    out.push_str(&format!("G0 Z{cz:.4}{eol}", cz = req.clearance_z));
    out.push_str(&format!("G65 P9833{eol}")); // probe off
    out.push_str(&format!("M5{eol}"));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Tool, ToolKind};

    fn probe_tool() -> Tool {
        Tool {
            number: 25,
            kind: ToolKind::Probe,
            diameter: 6.0,
            corner_radius: 0.0,
            length: 100.0,
            flute_length: 20.0,
            flutes: 1,
            description: "spindle probe".into(),
            ..Default::default()
        }
    }

    fn base_req(cycle: ProbeCycle) -> ProbeSnippetRequest {
        ProbeSnippetRequest {
            cycle,
            style: ProbeMacroStyle::HaasInspectionPlus,
            probe_tool: probe_tool(),
            x: 0.0,
            y: 0.0,
            z: 0.0,
            clearance_z: 25.0,
            protected_feed: 150.0,
            work_offset_s: 1,
            overtravel_q: 0.25,
            size_d: 50.0,
        }
    }

    #[test]
    fn single_surface_emits_inspection_plus_skeleton() {
        let g = emit_snippet(&base_req(ProbeCycle::SingleSurface)).unwrap();
        assert!(g.contains("T25 M6\n"), "{g}");
        assert!(g.contains("G65 P9832\n"), "{g}");
        assert!(g.contains("G65 P9811 Z0.0000 Q0.2500 S1\n"), "{g}");
        assert!(g.contains("G65 P9833\n"), "{g}");
    }

    #[test]
    fn bore_boss_includes_diameter() {
        let g = emit_snippet(&base_req(ProbeCycle::BoreBoss)).unwrap();
        assert!(g.contains("G65 P9814 D50.0000"), "{g}");
    }

    #[test]
    fn parse_cycle_aliases() {
        assert_eq!(ProbeCycle::parse("9811"), Some(ProbeCycle::SingleSurface));
        assert_eq!(ProbeCycle::parse("web-pocket"), Some(ProbeCycle::WebPocket));
    }

    #[test]
    fn rejects_unsupported_style() {
        let mut req = base_req(ProbeCycle::ProtectedMove);
        req.style = ProbeMacroStyle::None;
        assert!(emit_snippet(&req).is_err());
    }
}

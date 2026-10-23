//! Local G-code post-processors for normalised [`PathSegment`] toolpaths.
//!
//! Used when the CAM companion returns geometry only, or for deterministic golden tests.
//! Output is intended for operator review before running on a machine — not machine-verified.
//!
//! Dialects land one PR at a time (`grbl`, then Marlin / Repetier / Mach3 / LinuxCNC / Fanuc / Haas).

mod grbl;

use crate::{GcodeDialect, LineEnding, PathSegment, PostProfile, Tool, ToolKind};

/// Post a toolpath to G-code using the given profile and tool metadata.
pub fn post_toolpath(segments: &[PathSegment], profile: &PostProfile, tool: &Tool) -> Result<String, String> {
    if segments.is_empty() {
        return Err("toolpath has no segments".into());
    }
    let eol = match profile.line_ending {
        LineEnding::Lf => "\n",
        LineEnding::CrLf => "\r\n",
    };
    let mut out = String::new();
    match profile.dialect {
        GcodeDialect::Grbl | GcodeDialect::Generic => grbl::write_header(&mut out, tool, eol),
        // Other dialects share the GRBL-class header until their peel PRs land.
        _ => grbl::write_header(&mut out, tool, eol),
    }
    if !profile.preamble.is_empty() {
        for line in profile.preamble.lines() {
            out.push_str(line);
            out.push_str(eol);
        }
    }
    let mut pos = [0.0_f64; 3];
    for seg in segments {
        seg.validate().map_err(|e| e.to_string())?;
        match seg {
            PathSegment::Rapid { to } => {
                write_move(&mut out, "G0", *to, None, eol);
                pos = *to;
            }
            PathSegment::Linear { to, feed } => {
                write_move(&mut out, "G1", *to, Some(*feed), eol);
                pos = *to;
            }
            PathSegment::Arc { center, to, feed, clockwise } => {
                let code = if *clockwise { "G2" } else { "G3" };
                let i = center[0] - pos[0];
                let j = center[1] - pos[1];
                out.push_str(&format!("{code} X{:.4} Y{:.4} Z{:.4} I{:.4} J{:.4} F{:.1}{eol}", to[0], to[1], to[2], i, j, feed));
                pos = *to;
            }
        }
    }
    out.push_str(&format!("M5{eol}M30{eol}"));
    if !profile.postamble.is_empty() {
        for line in profile.postamble.lines() {
            out.push_str(line);
            out.push_str(eol);
        }
    }
    Ok(out)
}

pub(crate) fn write_spindle_or_probe_note(out: &mut String, tool: &Tool, eol: &str) {
    if tool.kind == ToolKind::Probe {
        out.push_str(&format!("(Probe T{num}: use manufacture.probe.snippet for Inspection Plus cycles){eol}", num = tool.number));
    } else {
        out.push_str(&format!("M3 S10000{eol}"));
    }
}

fn write_move(out: &mut String, code: &str, to: [f64; 3], feed: Option<f64>, eol: &str) {
    if let Some(f) = feed {
        out.push_str(&format!("{code} X{:.4} Y{:.4} Z{:.4} F{:.1}{eol}", to[0], to[1], to[2], f));
    } else {
        out.push_str(&format!("{code} X{:.4} Y{:.4} Z{:.4}{eol}", to[0], to[1], to[2]));
    }
}

/// Built-in post profile presets for common controllers (grows with dialect peels).
pub fn preset_posts() -> Vec<(&'static str, PostProfile)> {
    vec![grbl::preset()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolKind;

    pub(crate) fn sample_segments() -> Vec<PathSegment> {
        vec![
            PathSegment::Rapid { to: [0.0, 0.0, 5.0] },
            PathSegment::Linear { to: [10.0, 0.0, -1.0], feed: 800.0 },
            PathSegment::Arc { center: [10.0, 5.0, -1.0], to: [10.0, 10.0, -1.0], feed: 600.0, clockwise: true },
        ]
    }

    pub(crate) fn tool() -> Tool {
        Tool {
            number: 1,
            kind: ToolKind::FlatEndmill,
            diameter: 6.0,
            corner_radius: 0.0,
            length: 50.0,
            flute_length: 16.0,
            flutes: 4,
            description: String::new(),
            ..Default::default()
        }
    }

    #[test]
    fn empty_segments_rejected() {
        let profile = preset_posts()[0].1.clone();
        assert!(post_toolpath(&[], &profile, &tool()).is_err());
    }

    #[test]
    fn presets_start_with_grbl() {
        let names: Vec<_> = preset_posts().iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["GRBL"]);
    }
}

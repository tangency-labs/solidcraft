//! Local G-code post-processors for normalised [`PathSegment`] toolpaths.
//!
//! Used when the CAM companion returns geometry only, or for deterministic golden tests.
//! Output is intended for operator review before running on a machine — not machine-verified.

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
        GcodeDialect::Fanuc => write_fanuc_header(&mut out, tool, eol),
        GcodeDialect::Haas => write_haas_header(&mut out, tool, eol),
        GcodeDialect::Mach3Mach4 => write_mach_header(&mut out, tool, eol),
        GcodeDialect::Marlin => write_marlin_header(&mut out, tool, eol),
        GcodeDialect::Repetier => write_repetier_header(&mut out, tool, eol),
        GcodeDialect::Grbl | GcodeDialect::LinuxCnc | GcodeDialect::Generic => write_grblish_header(&mut out, tool, eol),
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

fn write_spindle_or_probe_note(out: &mut String, tool: &Tool, eol: &str) {
    if tool.kind == ToolKind::Probe {
        out.push_str(&format!("(Probe T{num} — use manufacture.probe.snippet for Inspection Plus cycles){eol}", num = tool.number));
    } else {
        out.push_str(&format!("M3 S10000{eol}"));
    }
}

fn write_grblish_header(out: &mut String, tool: &Tool, eol: &str) {
    out.push_str(&format!("(SolidCraft post T{num} D{d:.3}){eol}", num = tool.number, d = tool.diameter));
    out.push_str(&format!("G21{eol}G90{eol}G17{eol}"));
    // GRBL / LinuxCNC class: no ATC assumed; comment the intended tool.
    out.push_str(&format!("(T{num} — manual tool change){eol}", num = tool.number));
    write_spindle_or_probe_note(out, tool, eol);
}

fn write_mach_header(out: &mut String, tool: &Tool, eol: &str) {
    out.push_str(&format!("(SolidCraft Mach post T{num}){eol}", num = tool.number));
    out.push_str(&format!("G21{eol}G90{eol}G17{eol}"));
    out.push_str(&format!("T{num} M6{eol}", num = tool.number));
    write_spindle_or_probe_note(out, tool, eol);
}

fn write_marlin_header(out: &mut String, tool: &Tool, eol: &str) {
    out.push_str(&format!("; SolidCraft Marlin post T{num} D{d:.3}{eol}", num = tool.number, d = tool.diameter));
    out.push_str(&format!("G21{eol}G90{eol}G17{eol}"));
    out.push_str(&format!("; T{num} — manual / Snapmaker tool change{eol}", num = tool.number));
    write_spindle_or_probe_note(out, tool, eol);
}

fn write_repetier_header(out: &mut String, tool: &Tool, eol: &str) {
    out.push_str(&format!("; SolidCraft Repetier post T{num} D{d:.3}{eol}", num = tool.number, d = tool.diameter));
    out.push_str(&format!("G21{eol}G90{eol}G17{eol}"));
    out.push_str(&format!("; T{num} — load in Repetier-Host / serial sender{eol}", num = tool.number));
    write_spindle_or_probe_note(out, tool, eol);
}

fn write_fanuc_header(out: &mut String, tool: &Tool, eol: &str) {
    out.push_str(&format!("O1000 (SOLIDCRAFT T{num}){eol}", num = tool.number));
    out.push_str(&format!("G21 G90 G17{eol}"));
    out.push_str(&format!("T{num} M6{eol}", num = tool.number));
    out.push_str(&format!("G43 H{num}{eol}", num = tool.number));
    write_spindle_or_probe_note(out, tool, eol);
}

fn write_haas_header(out: &mut String, tool: &Tool, eol: &str) {
    out.push_str(&format!("(SolidCraft Haas post T{num} D{d:.3}){eol}", num = tool.number, d = tool.diameter));
    out.push_str(&format!("G21{eol}G90{eol}G17{eol}"));
    out.push_str(&format!("T{num} M06{eol}", num = tool.number));
    out.push_str(&format!("G43 H{num}{eol}", num = tool.number));
    write_spindle_or_probe_note(out, tool, eol);
}

fn write_move(out: &mut String, code: &str, to: [f64; 3], feed: Option<f64>, eol: &str) {
    if let Some(f) = feed {
        out.push_str(&format!("{code} X{:.4} Y{:.4} Z{:.4} F{:.1}{eol}", to[0], to[1], to[2], f));
    } else {
        out.push_str(&format!("{code} X{:.4} Y{:.4} Z{:.4}{eol}", to[0], to[1], to[2]));
    }
}

/// Built-in post profile presets for common controllers.
pub fn preset_posts() -> [(&'static str, PostProfile); 7] {
    [
        (
            "GRBL",
            PostProfile {
                name: "GRBL".into(),
                dialect: GcodeDialect::Grbl,
                line_ending: LineEnding::Lf,
                preamble: String::new(),
                postamble: String::new(),
            },
        ),
        (
            "Marlin",
            PostProfile {
                name: "Marlin".into(),
                dialect: GcodeDialect::Marlin,
                line_ending: LineEnding::Lf,
                preamble: String::new(),
                postamble: String::new(),
            },
        ),
        (
            "Repetier",
            PostProfile {
                name: "Repetier".into(),
                dialect: GcodeDialect::Repetier,
                line_ending: LineEnding::Lf,
                preamble: String::new(),
                postamble: String::new(),
            },
        ),
        (
            "Mach3",
            PostProfile {
                name: "Mach3".into(),
                dialect: GcodeDialect::Mach3Mach4,
                line_ending: LineEnding::CrLf,
                preamble: String::new(),
                postamble: String::new(),
            },
        ),
        (
            "LinuxCNC",
            PostProfile {
                name: "LinuxCNC".into(),
                dialect: GcodeDialect::LinuxCnc,
                line_ending: LineEnding::Lf,
                preamble: String::new(),
                postamble: String::new(),
            },
        ),
        (
            "Fanuc",
            PostProfile {
                name: "Fanuc".into(),
                dialect: GcodeDialect::Fanuc,
                line_ending: LineEnding::Lf,
                preamble: String::new(),
                postamble: String::new(),
            },
        ),
        (
            "Haas",
            PostProfile {
                name: "Haas".into(),
                dialect: GcodeDialect::Haas,
                line_ending: LineEnding::Lf,
                preamble: String::new(),
                postamble: String::new(),
            },
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolKind;

    fn sample_segments() -> Vec<PathSegment> {
        vec![
            PathSegment::Rapid { to: [0.0, 0.0, 5.0] },
            PathSegment::Linear { to: [10.0, 0.0, -1.0], feed: 800.0 },
            PathSegment::Arc { center: [10.0, 5.0, -1.0], to: [10.0, 10.0, -1.0], feed: 600.0, clockwise: true },
        ]
    }

    fn tool() -> Tool {
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

    /// Golden GRBL output — bump deliberately when the post format changes.
    const GOLDEN_GRBL: &str = "\
(SolidCraft post T1 D6.000)\n\
G21\n\
G90\n\
G17\n\
(T1 — manual tool change)\n\
M3 S10000\n\
G0 X0.0000 Y0.0000 Z5.0000\n\
G1 X10.0000 Y0.0000 Z-1.0000 F800.0\n\
G2 X10.0000 Y10.0000 Z-1.0000 I0.0000 J5.0000 F600.0\n\
M5\n\
M30\n";

    /// Golden Mach3/4 output (CRLF) — bump deliberately when the post format changes.
    const GOLDEN_MACH: &str = "\
(SolidCraft Mach post T1)\r\n\
G21\r\n\
G90\r\n\
G17\r\n\
T1 M6\r\n\
M3 S10000\r\n\
G0 X0.0000 Y0.0000 Z5.0000\r\n\
G1 X10.0000 Y0.0000 Z-1.0000 F800.0\r\n\
G2 X10.0000 Y10.0000 Z-1.0000 I0.0000 J5.0000 F600.0\r\n\
M5\r\n\
M30\r\n";

    #[test]
    fn grbl_golden_exact() {
        let profile = preset_posts()[0].1.clone();
        let g = post_toolpath(&sample_segments(), &profile, &tool()).unwrap();
        assert_eq!(g, GOLDEN_GRBL);
    }

    #[test]
    fn mach_golden_exact() {
        let profile = preset_posts().iter().find(|(id, _)| *id == "Mach3").unwrap().1.clone();
        let g = post_toolpath(&sample_segments(), &profile, &tool()).unwrap();
        assert_eq!(g, GOLDEN_MACH);
    }

    #[test]
    fn linuxcnc_smoke_has_units_and_arc() {
        let profile = preset_posts().iter().find(|(id, _)| *id == "LinuxCNC").unwrap().1.clone();
        let g = post_toolpath(&sample_segments(), &profile, &tool()).unwrap();
        assert!(g.contains("G21\n"), "{g}");
        assert!(g.contains("G2 X10.0000 Y10.0000"), "{g}");
        assert!(g.contains("M30\n"), "{g}");
    }

    #[test]
    fn fanuc_emits_tool_change_and_length_comp() {
        let profile = preset_posts().iter().find(|(id, _)| *id == "Fanuc").unwrap().1.clone();
        let g = post_toolpath(&sample_segments(), &profile, &tool()).unwrap();
        assert!(g.starts_with("O1000"), "{g}");
        assert!(g.contains("T1 M6\n"), "{g}");
        assert!(g.contains("G43 H1\n"), "{g}");
        assert!(g.contains("M30\n"), "{g}");
    }

    #[test]
    fn haas_emits_m06_and_g43() {
        let profile = preset_posts().iter().find(|(id, _)| *id == "Haas").unwrap().1.clone();
        let g = post_toolpath(&sample_segments(), &profile, &tool()).unwrap();
        assert!(g.contains("T1 M06\n"), "{g}");
        assert!(g.contains("G43 H1\n"), "{g}");
    }

    #[test]
    fn empty_segments_rejected() {
        let profile = preset_posts()[0].1.clone();
        assert!(post_toolpath(&[], &profile, &tool()).is_err());
    }

    #[test]
    fn presets_cover_priority_controllers() {
        let names: Vec<_> = preset_posts().iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["GRBL", "Marlin", "Repetier", "Mach3", "LinuxCNC", "Fanuc", "Haas"]);
    }
}

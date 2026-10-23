//! Repetier-Firmware / Repetier-Host style post header and preset.

use crate::{GcodeDialect, LineEnding, PostProfile, Tool};

use super::write_spindle_or_probe_note;

pub(crate) fn write_header(out: &mut String, tool: &Tool, eol: &str) {
    out.push_str(&format!("; SolidCraft Repetier post T{num} D{d:.3}{eol}", num = tool.number, d = tool.diameter));
    out.push_str(&format!("G21{eol}G90{eol}G17{eol}"));
    out.push_str(&format!("; T{num} — load in Repetier-Host / serial sender{eol}", num = tool.number));
    write_spindle_or_probe_note(out, tool, eol);
}

pub(crate) fn preset() -> (&'static str, PostProfile) {
    (
        "Repetier",
        PostProfile {
            name: "Repetier".into(),
            dialect: GcodeDialect::Repetier,
            line_ending: LineEnding::Lf,
            preamble: String::new(),
            postamble: String::new(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::post::{post_toolpath, tests::sample_segments, tests::tool};

    #[test]
    fn repetier_uses_semicolon_comments() {
        let profile = preset().1;
        let g = post_toolpath(&sample_segments(), &profile, &tool()).unwrap();
        assert!(g.starts_with("; SolidCraft Repetier post"), "{g}");
        assert!(g.contains("; T1 — load in Repetier-Host / serial sender\n"), "{g}");
        assert!(g.contains("M30\n"), "{g}");
    }
}

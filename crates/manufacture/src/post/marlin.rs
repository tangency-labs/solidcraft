//! Marlin CNC / Snapmaker-class post header and preset.

use crate::{GcodeDialect, LineEnding, PostProfile, Tool};

use super::write_spindle_or_probe_note;

pub(crate) fn write_header(out: &mut String, tool: &Tool, eol: &str) {
    out.push_str(&format!("; SolidCraft Marlin post T{num} D{d:.3}{eol}", num = tool.number, d = tool.diameter));
    out.push_str(&format!("G21{eol}G90{eol}G17{eol}"));
    out.push_str(&format!("; T{num} — manual / Snapmaker tool change{eol}", num = tool.number));
    write_spindle_or_probe_note(out, tool, eol);
}

pub(crate) fn preset() -> (&'static str, PostProfile) {
    (
        "Marlin",
        PostProfile {
            name: "Marlin".into(),
            dialect: GcodeDialect::Marlin,
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
    fn marlin_uses_semicolon_comments() {
        let profile = preset().1;
        let g = post_toolpath(&sample_segments(), &profile, &tool()).unwrap();
        assert!(g.starts_with("; SolidCraft Marlin post"), "{g}");
        assert!(g.contains("; T1 — manual / Snapmaker tool change\n"), "{g}");
        assert!(g.contains("G2 X10.0000 Y10.0000"), "{g}");
        assert!(g.contains("M30\n"), "{g}");
    }
}

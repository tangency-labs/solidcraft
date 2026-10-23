//! Fanuc-style post header and preset (program O-number, T/M6, G43).

use crate::{GcodeDialect, LineEnding, PostProfile, Tool};

use super::write_spindle_or_probe_note;

pub(crate) fn write_header(out: &mut String, tool: &Tool, eol: &str) {
    out.push_str(&format!("O1000 (SOLIDCRAFT T{num}){eol}", num = tool.number));
    out.push_str(&format!("G21 G90 G17{eol}"));
    out.push_str(&format!("T{num} M6{eol}", num = tool.number));
    out.push_str(&format!("G43 H{num}{eol}", num = tool.number));
    write_spindle_or_probe_note(out, tool, eol);
}

pub(crate) fn preset() -> (&'static str, PostProfile) {
    (
        "Fanuc",
        PostProfile {
            name: "Fanuc".into(),
            dialect: GcodeDialect::Fanuc,
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
    fn fanuc_emits_tool_change_and_length_comp() {
        let profile = preset().1;
        let g = post_toolpath(&sample_segments(), &profile, &tool()).unwrap();
        assert!(g.starts_with("O1000"), "{g}");
        assert!(g.contains("T1 M6\n"), "{g}");
        assert!(g.contains("G43 H1\n"), "{g}");
        assert!(g.contains("M30\n"), "{g}");
    }
}

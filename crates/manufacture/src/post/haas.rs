//! Haas post header and preset (M06 tool change, G43 length comp).

use crate::{GcodeDialect, LineEnding, PostProfile, Tool};

use super::write_spindle_or_probe_note;

pub(crate) fn write_header(out: &mut String, tool: &Tool, eol: &str) {
    out.push_str(&format!("(SolidCraft Haas post T{num} D{d:.3}){eol}", num = tool.number, d = tool.diameter));
    out.push_str(&format!("G21{eol}G90{eol}G17{eol}"));
    out.push_str(&format!("T{num} M06{eol}", num = tool.number));
    out.push_str(&format!("G43 H{num}{eol}", num = tool.number));
    write_spindle_or_probe_note(out, tool, eol);
}

pub(crate) fn preset() -> (&'static str, PostProfile) {
    (
        "Haas",
        PostProfile {
            name: "Haas".into(),
            dialect: GcodeDialect::Haas,
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
    fn haas_emits_m06_and_g43() {
        let profile = preset().1;
        let g = post_toolpath(&sample_segments(), &profile, &tool()).unwrap();
        assert!(g.contains("T1 M06\n"), "{g}");
        assert!(g.contains("G43 H1\n"), "{g}");
        assert!(g.contains("M30\n"), "{g}");
    }
}

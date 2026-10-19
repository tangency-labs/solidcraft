//! LinuxCNC post header and preset.

use crate::{GcodeDialect, LineEnding, PostProfile, Tool};

use super::write_spindle_or_probe_note;

pub(crate) fn write_header(out: &mut String, tool: &Tool, eol: &str) {
    out.push_str(&format!("(SolidCraft LinuxCNC post T{num} D{d:.3}){eol}", num = tool.number, d = tool.diameter));
    out.push_str(&format!("G21{eol}G90{eol}G17{eol}"));
    out.push_str(&format!("(T{num} — manual tool change){eol}", num = tool.number));
    write_spindle_or_probe_note(out, tool, eol);
}

pub(crate) fn preset() -> (&'static str, PostProfile) {
    (
        "LinuxCNC",
        PostProfile {
            name: "LinuxCNC".into(),
            dialect: GcodeDialect::LinuxCnc,
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
    fn linuxcnc_smoke_has_units_and_arc() {
        let profile = preset().1;
        let g = post_toolpath(&sample_segments(), &profile, &tool()).unwrap();
        assert!(g.starts_with("(SolidCraft LinuxCNC post"), "{g}");
        assert!(g.contains("G21\n"), "{g}");
        assert!(g.contains("G2 X10.0000 Y10.0000"), "{g}");
        assert!(g.contains("M30\n"), "{g}");
    }
}

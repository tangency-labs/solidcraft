//! GRBL / generic ISO-ish post header and preset.

use crate::{GcodeDialect, LineEnding, PostProfile, Tool};

use super::write_spindle_or_probe_note;

pub(crate) fn write_header(out: &mut String, tool: &Tool, eol: &str) {
    out.push_str(&format!("(SolidCraft post T{num} D{d:.3}){eol}", num = tool.number, d = tool.diameter));
    out.push_str(&format!("G21{eol}G90{eol}G17{eol}"));
    // GRBL class: no ATC assumed; comment the intended tool.
    out.push_str(&format!("(T{num} — manual tool change){eol}", num = tool.number));
    write_spindle_or_probe_note(out, tool, eol);
}

pub(crate) fn preset() -> (&'static str, PostProfile) {
    (
        "GRBL",
        PostProfile {
            name: "GRBL".into(),
            dialect: GcodeDialect::Grbl,
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

    #[test]
    fn grbl_golden_exact() {
        let profile = preset().1;
        let g = post_toolpath(&sample_segments(), &profile, &tool()).unwrap();
        assert_eq!(g, GOLDEN_GRBL);
    }
}

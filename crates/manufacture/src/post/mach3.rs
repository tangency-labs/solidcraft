//! Mach3 / Mach4 post header and preset (CRLF).

use crate::{GcodeDialect, LineEnding, PostProfile, Tool};

use super::write_spindle_or_probe_note;

pub(crate) fn write_header(out: &mut String, tool: &Tool, eol: &str) {
    out.push_str(&format!("(SolidCraft Mach post T{num}){eol}", num = tool.number));
    out.push_str(&format!("G21{eol}G90{eol}G17{eol}"));
    out.push_str(&format!("T{num} M6{eol}", num = tool.number));
    write_spindle_or_probe_note(out, tool, eol);
}

pub(crate) fn preset() -> (&'static str, PostProfile) {
    (
        "Mach3",
        PostProfile {
            name: "Mach3".into(),
            dialect: GcodeDialect::Mach3Mach4,
            line_ending: LineEnding::CrLf,
            preamble: String::new(),
            postamble: String::new(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::post::{post_toolpath, tests::sample_segments, tests::tool};

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
    fn mach_golden_exact() {
        let profile = preset().1;
        let g = post_toolpath(&sample_segments(), &profile, &tool()).unwrap();
        assert_eq!(g, GOLDEN_MACH);
    }
}

//! Surface-texture construction dimensions, kept together for standards review.
//!
//! ISO nominal dimensions below come from ISO surface-texture draft requirements;
//! published-edition verification remains pending. ASME's basic leg construction is
//! supported by a construction reference. Neither establishes complete conformance.
//! Every remaining layout/circle/glyph-size value is explicitly unchecked here.

use super::{Invalid, Result, require};

pub(super) const LEG_ANGLE_RAD: f64 = std::f64::consts::FRAC_PI_3;

struct IsoSize {
    h: f64,
    short: f64,
    minimum_top: f64,
    identifier_gap: f64,
    identifier_length: f64,
}

// ISO draft dimensions in millimetres. These rounded nominal sizes must not be
// replaced by one approximate multiplier or interpolated to unsupported sizes.
const ISO_SIZES: [IsoSize; 7] = [
    IsoSize { h: 2.5, short: 3.5, minimum_top: 7.5, identifier_gap: 2.0, identifier_length: 3.0 },
    IsoSize { h: 3.5, short: 5.0, minimum_top: 10.5, identifier_gap: 2.5, identifier_length: 4.5 },
    IsoSize { h: 5.0, short: 7.0, minimum_top: 15.0, identifier_gap: 3.5, identifier_length: 6.0 },
    IsoSize { h: 7.0, short: 10.0, minimum_top: 21.0, identifier_gap: 5.0, identifier_length: 8.5 },
    IsoSize { h: 10.0, short: 14.0, minimum_top: 30.0, identifier_gap: 7.0, identifier_length: 12.0 },
    IsoSize { h: 14.0, short: 20.0, minimum_top: 42.0, identifier_gap: 10.0, identifier_length: 17.0 },
    IsoSize { h: 20.0, short: 28.0, minimum_top: 60.0, identifier_gap: 14.0, identifier_length: 24.0 },
];

#[derive(Clone, Copy)]
pub(super) struct Profile {
    pub stroke_width_h: f64,
    pub line_pitch_h: f64,
    pub text_margin_h: f64,
    pub parameter_top_gap_h: f64,
    pub process_baseline_h: f64,
    pub lay_gap_h: f64,
    pub lay_baseline_h: f64,
    pub lay_width_h: f64,
    pub circular_lay_width_h: f64,
    pub lay_height_h: f64,
    pub prohibited_radius_h: f64,
    pub all_round_radius_h: f64,
}

struct Profiles {
    iso: Profile,
    asme: Profile,
    asme_short_h: f64,
    asme_minimum_top_h: f64,
}

// One pair of named profiles: change proportions here, not in drawing code.
// ISO's stroke width is supported by the draft dimension rules. All other
// Profile values (including the ASME stroke width) are UNCHECKED preview choices.
// Lay's above/below-line ordering is supported; the numeric offsets are unchecked.
// The ASME basic heights below are supported by its construction reference.
const PROFILES: Profiles = Profiles {
    iso: Profile {
        stroke_width_h: 0.1,
        line_pitch_h: 1.5,
        text_margin_h: 0.3,
        parameter_top_gap_h: 1.3,
        process_baseline_h: 1.0,
        lay_gap_h: 0.5,
        lay_baseline_h: 1.0,
        lay_width_h: 0.8,
        circular_lay_width_h: 1.0,
        lay_height_h: 1.0,
        prohibited_radius_h: 0.4,
        all_round_radius_h: 0.4,
    },
    asme: Profile {
        stroke_width_h: 0.1,
        line_pitch_h: 1.5,
        text_margin_h: 0.3,
        parameter_top_gap_h: 1.3,
        process_baseline_h: 1.0,
        lay_gap_h: 0.5,
        lay_baseline_h: -1.5,
        lay_width_h: 0.8,
        circular_lay_width_h: 1.0,
        lay_height_h: 1.0,
        prohibited_radius_h: 0.4,
        all_round_radius_h: 0.4,
    },
    asme_short_h: 1.5,
    asme_minimum_top_h: 3.0,
};

pub(super) struct Dimensions {
    pub profile: Profile,
    pub short: f64,
    pub top: f64,
    pub stroke: f64,
    pub identifier_length: f64,
    pub identifier_height: f64,
}

pub(super) fn resolve(iso: bool, h: f64, line_count: usize) -> Result<Dimensions> {
    require(h.is_finite() && (2.5..=20.0).contains(&h), "preview height must be finite and in 2.5..=20 mm")?;
    let (profile, short, minimum_top, identifier_length, identifier_height) = if iso {
        let size = ISO_SIZES.iter().find(|size| size.h == h).ok_or(Invalid("ISO text height must be 2.5, 3.5, 5, 7, 10, 14 or 20 mm"))?;
        (PROFILES.iso, size.short, size.minimum_top, size.identifier_length, size.short + size.identifier_gap)
    } else {
        (PROFILES.asme, PROFILES.asme_short_h * h, PROFILES.asme_minimum_top_h * h, 0.0, 0.0)
    };
    // UNCHECKED: extra-line growth uses the preview's line pitch. The ISO minimum
    // depends on the number of indications; its exact growth rule awaits review.
    let top = minimum_top + line_count.saturating_sub(2) as f64 * profile.line_pitch_h * h;
    Ok(Dimensions { profile, short, top, stroke: profile.stroke_width_h * h, identifier_length, identifier_height })
}

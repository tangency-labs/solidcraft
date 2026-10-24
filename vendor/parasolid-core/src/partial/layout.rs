// Modified by sldkit/parasolid-kit; see the crate-root PARTIAL_READERS.md.
// SPDX-License-Identifier: Apache-2.0
// Extracted from cadmpeg-codec-sldprt 0.5.3 layout.rs.
pub(crate) mod world_point {
    /// Record length in bytes. Spec §4.
    pub(crate) const LEN: usize = 38;
    /// Offset of `refs` (`u16[4]`, big-endian). Spec §4.
    pub(crate) const REFS: usize = 6;
    /// Offset of `xyz` (`f64[3]`, big-endian). Spec §4.
    pub(crate) const XYZ: usize = 14;
}

pub(crate) mod bspline_array_header {
    /// Record length in bytes. Spec §7.2.
    pub(crate) const LEN: usize = 6;
    /// Offset of `count` (`u32`, big-endian). Spec §7.2.
    pub(crate) const COUNT: usize = 0;
    /// Offset of `attr` (`u16`, big-endian). Spec §7.2.
    pub(crate) const ATTR: usize = 4;
}

pub(crate) mod bspline_compact_array_header {
    /// Record length in bytes. Spec §7.2.
    pub(crate) const LEN: usize = 4;
    /// Offset of `count` (`u8`). Spec §7.2.
    pub(crate) const COUNT: usize = 1;
    /// Offset of `attr` (`u16`, big-endian). Spec §7.2.
    pub(crate) const ATTR: usize = 2;
}

pub(crate) mod bspline_surface_descriptor {
    /// Record length in bytes. Spec §7.2.
    pub(crate) const LEN: usize = 42;
    /// Offset of `attr` (`u16`, big-endian). Spec §7.2.
    pub(crate) const ATTR: usize = 0;
    /// Offset of `u_periodic` (`u8`). Spec §7.2.
    pub(crate) const U_PERIODIC: usize = 2;
    /// Offset of `v_periodic` (`u8`). Spec §7.2.
    pub(crate) const V_PERIODIC: usize = 3;
    /// Offset of `u_degree` (`u16`, big-endian). Spec §7.2.
    pub(crate) const U_DEGREE: usize = 4;
    /// Offset of `v_degree` (`u16`, big-endian). Spec §7.2.
    pub(crate) const V_DEGREE: usize = 6;
    /// Offset of `u_pole_count` (`u32`, big-endian). Spec §7.2.
    pub(crate) const U_POLE_COUNT: usize = 8;
    /// Offset of `v_pole_count` (`u32`, big-endian). Spec §7.2.
    pub(crate) const V_POLE_COUNT: usize = 12;
    /// Offset of `u_knot_type` (`u8`). Spec §7.2.
    pub(crate) const U_KNOT_TYPE: usize = 16;
    /// Offset of `v_knot_type` (`u8`). Spec §7.2.
    pub(crate) const V_KNOT_TYPE: usize = 17;
    /// Offset of `u_distinct_knot_count` (`u32`, big-endian). Spec §7.2.
    pub(crate) const U_DISTINCT_KNOT_COUNT: usize = 18;
    /// Offset of `v_distinct_knot_count` (`u32`, big-endian). Spec §7.2.
    pub(crate) const V_DISTINCT_KNOT_COUNT: usize = 22;
    /// Offset of `rational` (`u8`). Spec §7.2.
    pub(crate) const RATIONAL: usize = 26;
    /// Offset of `u_closed` (`u8`). Spec §7.2.
    pub(crate) const U_CLOSED: usize = 27;
    /// Offset of `v_closed` (`u8`). Spec §7.2.
    pub(crate) const V_CLOSED: usize = 28;
    /// Offset of `surface_form` (`u8`). Spec §7.2.
    pub(crate) const SURFACE_FORM: usize = 29;
    /// Offset of `vertex_dim` (`u16`, big-endian). Spec §7.2.
    pub(crate) const VERTEX_DIM: usize = 30;
    /// Offset of `array_refs` (`u16[5]`, big-endian). Spec §7.2.
    pub(crate) const ARRAY_REFS: usize = 32;
}

pub(crate) mod compact_analytic_header {
    /// Offset of `refs` (`u16[5]`, big-endian). Spec §7.1.
    pub(crate) const REFS: usize = 6;
    /// Offset of `marker` (`u8`). Spec §7.1.
    pub(crate) const MARKER: usize = 16;
}

pub(crate) mod rolling_ball_blend_00_38 {
    /// Offset of `attr` (`u16`, big-endian). Spec §7.4.
    pub(crate) const ATTR: usize = 0;
    /// Offset of `marker` (`u8`). Spec §7.4.
    pub(crate) const MARKER: usize = 16;
    /// Offset of `selector` (`u8`). Spec §7.4.
    pub(crate) const SELECTOR: usize = 17;
    /// Offset of `support0` (`u16`, big-endian). Spec §7.4.
    pub(crate) const SUPPORT0: usize = 18;
    /// Offset of `support1` (`u16`, big-endian). Spec §7.4.
    pub(crate) const SUPPORT1: usize = 20;
    /// Offset of `spine` (`u16`, big-endian). Spec §7.4.
    pub(crate) const SPINE: usize = 22;
    /// Offset of `offset0` (`f64`, big-endian). Spec §7.4.
    pub(crate) const OFFSET0: usize = 24;
    /// Offset of `offset1` (`f64`, big-endian). Spec §7.4.
    pub(crate) const OFFSET1: usize = 32;
    /// Offset of `side0` (`f64`, big-endian). Spec §7.4.
    pub(crate) const SIDE0: usize = 40;
    /// Offset of `side1` (`f64`, big-endian). Spec §7.4.
    pub(crate) const SIDE1: usize = 48;
}

pub(crate) mod offset_surface_00_3c {
    /// Offset of `attr` (`u16`, big-endian). Spec §7.5.
    pub(crate) const ATTR: usize = 0;
    /// Offset of `refs` (`u16[5]`, big-endian). Spec §7.5.
    pub(crate) const REFS: usize = 6;
    /// Offset of `marker` (`u8`). Spec §7.5.
    pub(crate) const MARKER: usize = 16;
    /// Offset of `discriminator` (`u8`). Spec §7.5.
    pub(crate) const DISCRIMINATOR: usize = 17;
    /// Offset of `true_offset` (`u8`). Spec §7.5.
    pub(crate) const TRUE_OFFSET: usize = 18;
    /// Offset of `support` (`u16`, big-endian). Spec §7.5.
    pub(crate) const SUPPORT: usize = 19;
    /// Offset of `distance` (`f64`, big-endian). Spec §7.5.
    pub(crate) const DISTANCE: usize = 21;
}

pub(crate) mod intersection_composite {
    /// Record length in bytes. Spec §7.3.
    pub(crate) const LEN: usize = 29;
    /// Offset of `attr` (`u16`, big-endian). Spec §7.3.
    pub(crate) const ATTR: usize = 0;
    /// Offset of `marker` (`u8`). Spec §7.3.
    pub(crate) const MARKER: usize = 16;
    /// Offset of `payload` (`u16[6]`, big-endian). Spec §7.3.
    pub(crate) const PAYLOAD: usize = 17;
}

pub(crate) mod support_uv_00_cc {
    /// Record length in bytes. Spec §7.3.
    pub(crate) const LEN: usize = 7;
    /// Offset of `count` (`u32`, big-endian). Spec §7.3.
    pub(crate) const COUNT: usize = 0;
    /// Offset of `attr` (`u16`, big-endian). Spec §7.3.
    pub(crate) const ATTR: usize = 4;
    /// Offset of `width` (`u8`). Spec §7.3.
    pub(crate) const WIDTH: usize = 6;
}

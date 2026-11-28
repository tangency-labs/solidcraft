//! The geometry of the keep-edge round (a concave round taller than its wall, ending on the
//! wall's top edge), measured by the kernel ([`solidcore::blend::keep_edge_section`]); the
//! application reads the faces and builds the prism.

pub use solidcore::blend::keep_edge_section::{
    arc_clears_wall, cylinder_is_upright, plane_end, prism_span, round_section, top_edge,
    unit_between, wall_frame, wall_height, EndKind, Section, TopEdge,
};

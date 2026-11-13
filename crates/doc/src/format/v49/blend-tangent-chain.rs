//! A fillet or chamfer saves `tangent_chain: false` when it blends only its picked edges; absent,
//! the default, it blends each picked edge's tangent chain too, as designs before this change were
//! built (SMLib followed the chains itself). An older design has none and keeps its chains. The
//! last release refuses a design with this change (the newer format number).

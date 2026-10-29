//! A Revolve, Surface Revolve, Circular Pattern or Plane at Angle can turn about an edge or round
//! face of the model, kept as a reference (`AxisRef::Model`, a pattern's `axis_ref`, an angle
//! plane's `line`: a circular edge's axis through its centre, a cylinder's, cone's or torus's
//! axis, or a straight edge), found again by its persistent name on each evaluation so the
//! feature follows the edge or face when the model changes. Older designs need no
//! transformation: their axes stay the fixed lines or named axes they were saved with. The last
//! release refuses a design saved with this (the newer format number).

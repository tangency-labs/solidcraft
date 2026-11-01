//! An edge on a line, circle or ellipse may save `nurbs`: the NURBS form it is parameterised on,
//! as SMLib holds it. Its interval and the pcurves' parameter are then that NURBS's, and `curve`
//! only says what the edge is. An older design has none and keeps the curve's own parameter. The
//! last release refuses a design with this change (the newer format number).

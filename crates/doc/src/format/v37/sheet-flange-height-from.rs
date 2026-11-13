//! An edge flange feature can say where its heights are measured from (#5938): `height_from` is
//! `inner` (the inner virtual sharp) or `tangent` (past 90° the line square to the flange
//! touching the outside of the bend, up to 90° the outer virtual sharp). A feature measured from the outer
//! virtual sharp, as every flange was before, saves no `height_from`, so older designs need no
//! step and open and save as they were. The last release refuses a design that has it (it
//! refuses the newer format number).

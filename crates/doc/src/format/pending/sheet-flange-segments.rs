//! An edge flange saves `segments` on an edge (`{point, angle?, height?, segments}`), one stretch
//! of the edge each (`{from, to, angle?, height?, flip?}`), when `sheet.flange` was asked to split
//! the edge. Absent, an edge is the whole edge with one bend, as designs before this change were
//! built. An older design has none and keeps its flanges whole. The last release refuses a design
//! with this change (the newer format number).

//! A design can hold CAM milling data (`cam`: tools, setups with work offset, optional tool axis
//! and work origin, optional stock, and 2D contour/pocket operations). Operation geometry is a
//! sketch profile, a planar face, or explicit polylines; depths, feeds and stepdowns are
//! expressions evaluated when posting. Toolpaths are never saved. Older designs need no
//! transformation: an absent `cam` field means no setups. The last release refuses a design
//! saved with this (the newer format number).

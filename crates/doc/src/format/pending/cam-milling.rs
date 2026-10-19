//! A design can hold CAM milling data (`cam`: tools, setups with work offset and optional stock,
//! and 2D contour/pocket operations whose regions are planar polygons in work coordinates).
//! Toolpaths are never saved; they are computed when posting. Older designs need no
//! transformation: an absent `cam` field means no setups. The last release refuses a design
//! saved with this (the newer format number).

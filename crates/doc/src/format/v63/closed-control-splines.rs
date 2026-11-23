//! A control-point spline can be closed (`"control": true, "periodic": true`, #19337): its
//! points are the control polygon of a closed cubic or higher-degree B-spline, and the curve
//! wraps around them. An older design has no such spline: its periodic splines are fit splines,
//! and it opens as it was saved. The last release refuses a design that has a closed control
//! spline with "damaged design: invalid periodic spline or revolve convention"; a design with
//! one is written with the next format number, which the last release warns about: it opens
//! the design, keeps what it can read, and says the format is newer.

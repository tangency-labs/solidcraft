//! A face on a cylinder, cone, sphere or torus may save `nurbs`: the NURBS form it is
//! parameterised on, as SMLib holds it. Its domain and pcurves are then that NURBS's (u, v), and
//! `surface` only says what the face is. An older design has none and keeps the analytic (u, v).
//! The last release refuses a design with this change (the newer format number).
